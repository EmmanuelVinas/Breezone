//! Authentication.
//!
//! * Passwords: Argon2id (PHC strings).
//! * Access tokens: short-lived JWT (HS256), sent as `Authorization: Bearer <jwt>`.
//! * Refresh tokens: long-lived random secrets, one per device ("session"), stored hashed.
//!   They are *sliding*: every refresh extends the expiry, so a device that opens the app
//!   at least once a year never has to type the password again. The mobile app keeps the
//!   refresh token in the iOS Keychain / Android Keystore; the password is only asked again
//!   if that secure storage is lost, or if the device is revoked from the web UI.
//! * Rotation: each refresh returns a new refresh token. The previous one stays valid for a
//!   short grace window so that a response lost on a flaky mobile network does not log the
//!   user out.

use crate::db::{self, Db, Session, User};
use crate::error::{ApiError, ApiResult};
use crate::AppState;
use anyhow::anyhow;
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use argon2::Argon2;
use axum::extract::FromRequestParts;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use axum::http::StatusCode;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

/// Grace window during which the previous refresh token is still accepted.
const ROTATION_GRACE_SECS: i64 = 120;
const LOCKOUT_SECS: i64 = 15 * 60;
/// Failed sign-ins allowed per window: for one account from one address, from one address
/// (any account), and for one account (from anywhere: a distributed attack). The account limit
/// is high so that someone else's attempts cannot easily lock its owner out.
const MAX_FAILED_LOGINS: u32 = 5;
const MAX_FAILED_PER_ADDRESS: u32 = 20;
const MAX_FAILED_PER_ACCOUNT: u32 = 50;
/// Counters kept at most (attempts with random names or addresses must not fill the memory).
const MAX_TRACKED: usize = 10_000;
/// Password hashes computed at the same time: Argon2 is heavy for a small NAS.
static HASHING: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub uid: i64,
    pub name: String,
    pub role: String,
    pub sid: i64,
    pub iat: i64,
    pub exp: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct UserView {
    pub id: i64,
    pub username: String,
    pub role: String,
}

impl From<&User> for UserView {
    fn from(u: &User) -> Self {
        Self { id: u.id, username: u.username.clone(), role: u.role.clone() }
    }
}

#[derive(Debug, Serialize)]
pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: &'static str,
    /// Access token lifetime in seconds.
    pub expires_in: i64,
    /// Refresh token expiry (unix seconds). Sliding: pushed back at every refresh.
    pub refresh_expires_at: i64,
    pub user: UserView,
}

pub struct AuthService {
    encoding: EncodingKey,
    decoding: DecodingKey,
    access_ttl: i64,
    refresh_ttl: i64,
    failures: Mutex<HashMap<String, (u32, i64)>>,
}

impl AuthService {
    pub fn new(secret: &[u8], access_ttl_secs: i64, refresh_ttl_days: i64) -> Self {
        Self {
            encoding: EncodingKey::from_secret(secret),
            decoding: DecodingKey::from_secret(secret),
            access_ttl: access_ttl_secs,
            refresh_ttl: refresh_ttl_days * 86_400,
            failures: Mutex::new(HashMap::new()),
        }
    }

    /// Loads the JWT secret from config, or from `<data_dir>/jwt.secret` (created on first
    /// start) so that restarting the server does not invalidate tokens.
    pub fn load_secret(configured: Option<&str>, data_dir: &Path) -> anyhow::Result<Vec<u8>> {
        if let Some(s) = configured {
            return Ok(s.as_bytes().to_vec());
        }
        let path = data_dir.join("jwt.secret");
        if let Ok(existing) = std::fs::read_to_string(&path) {
            if existing.trim().len() >= 32 {
                return Ok(existing.trim().as_bytes().to_vec());
            }
        }
        std::fs::create_dir_all(data_dir)?;
        let secret = hex::encode(random_bytes::<48>());
        std::fs::write(&path, &secret)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(secret.into_bytes())
    }

    pub fn access_token(&self, user: &User, session_id: i64) -> anyhow::Result<String> {
        let now = db::now();
        let claims = Claims {
            uid: user.id,
            name: user.username.clone(),
            role: user.role.clone(),
            sid: session_id,
            iat: now,
            exp: now + self.access_ttl,
        };
        jsonwebtoken::encode(&Header::new(Algorithm::HS256), &claims, &self.encoding).map_err(|e| anyhow!(e))
    }

    pub fn verify_access(&self, token: &str) -> ApiResult<Claims> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.leeway = 30;
        jsonwebtoken::decode::<Claims>(token, &self.decoding, &validation)
            .map(|d| d.claims)
            .map_err(|_| ApiError::unauthorized("invalid or expired token"))
    }

    /// Creates a new device session and returns its tokens.
    pub async fn open_session(&self, db: &Db, user: &User, device_name: &str) -> ApiResult<TokenPair> {
        let refresh = new_refresh_token();
        let expires_at = db::now() + self.refresh_ttl;
        let device = sanitize_device_name(device_name);
        let session = db::insert_session(db, user.id, &hash_token(&refresh), &device, expires_at).await?;
        self.pair(user, &session, refresh, expires_at)
    }

    /// Exchanges a refresh token for a fresh token pair (rotation + sliding expiry).
    pub async fn refresh(&self, db: &Db, refresh_token: &str) -> ApiResult<TokenPair> {
        let hash = hash_token(refresh_token);
        let now = db::now();
        let (session, keep_prev) = match db::session_by_token(db, &hash).await? {
            Some(s) => (s, false),
            None => match db::session_by_prev_token(db, &hash).await? {
                Some(s) if s.rotated_at.is_some_and(|r| now - r <= ROTATION_GRACE_SECS) => (s, true),
                _ => return Err(ApiError::unauthorized("invalid refresh token")),
            },
        };
        if session.expires_at < now {
            db::delete_session(db, session.id).await?;
            return Err(ApiError::unauthorized("refresh token expired"));
        }
        let user = db::user_by_id(db, session.user_id)
            .await?
            .ok_or_else(|| ApiError::unauthorized("user no longer exists"))?;
        let new_refresh = new_refresh_token();
        let expires_at = now + self.refresh_ttl;
        db::rotate_session(db, session.id, &hash_token(&new_refresh), keep_prev, expires_at).await?;
        self.pair(&user, &session, new_refresh, expires_at)
    }

    pub async fn revoke(&self, db: &Db, refresh_token: &str) -> ApiResult<()> {
        let hash = hash_token(refresh_token);
        if let Some(s) = db::session_by_token(db, &hash).await? {
            db::delete_session(db, s.id).await?;
        }
        Ok(())
    }

    fn pair(&self, user: &User, session: &Session, refresh: String, refresh_expires_at: i64) -> ApiResult<TokenPair> {
        Ok(TokenPair {
            access_token: self.access_token(user, session.id)?,
            refresh_token: refresh,
            token_type: "Bearer",
            expires_in: self.access_ttl,
            refresh_expires_at,
            user: user.into(),
        })
    }

    fn throttle_keys(username: &str, address: &str) -> [(String, u32); 3] {
        let user = username.to_lowercase();
        [
            (format!("pair:{address}|{user}"), MAX_FAILED_LOGINS),
            (format!("address:{address}"), MAX_FAILED_PER_ADDRESS),
            (format!("account:{user}"), MAX_FAILED_PER_ACCOUNT),
        ]
    }

    /// Brute-force protection: counts a login attempt (before the password is checked, in the
    /// same lock, so parallel attempts cannot all pass) and rejects it once a limit is reached.
    pub fn check_throttle(&self, username: &str, address: &str) -> ApiResult<()> {
        let mut failures = self.failures.lock().unwrap();
        let now = db::now();
        if failures.len() >= MAX_TRACKED {
            failures.retain(|_, (_, since)| now - *since <= LOCKOUT_SECS);
        }
        if failures.len() >= MAX_TRACKED {
            // Still full: forget the oldest half rather than refusing everyone.
            let mut since: Vec<i64> = failures.values().map(|(_, s)| *s).collect();
            since.sort_unstable();
            let cut = since[since.len() / 2];
            failures.retain(|_, (_, s)| *s > cut);
        }
        let keys = Self::throttle_keys(username, address);
        for (key, limit) in &keys {
            if let Some(entry) = failures.get_mut(key) {
                if now - entry.1 > LOCKOUT_SECS {
                    *entry = (0, now);
                }
                if entry.0 >= *limit {
                    return Err(ApiError::new(StatusCode::TOO_MANY_REQUESTS, "too many failed attempts, retry in a few minutes"));
                }
            }
        }
        for (key, _) in keys {
            failures.entry(key).or_insert((0, now)).0 += 1;
        }
        Ok(())
    }

    /// A successful login clears the counters of that account (failures are already counted);
    /// the address keeps its count.
    pub fn record_login(&self, username: &str, address: &str, success: bool) {
        if success {
            let mut failures = self.failures.lock().unwrap();
            for (key, _) in Self::throttle_keys(username, address).into_iter().filter(|(k, _)| !k.starts_with("address:")) {
                failures.remove(&key);
            }
        }
    }
}

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    rand::fill(&mut bytes[..]);
    bytes
}

pub fn new_refresh_token() -> String {
    URL_SAFE_NO_PAD.encode(random_bytes::<32>())
}

pub fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn sanitize_device_name(name: &str) -> String {
    let trimmed: String = name.trim().chars().take(80).collect();
    if trimmed.is_empty() {
        "Appareil inconnu".into()
    } else {
        trimmed
    }
}

pub fn validate_password(password: &str) -> ApiResult<()> {
    if password.chars().count() < 8 {
        return Err(ApiError::bad_request("password must be at least 8 characters"));
    }
    Ok(())
}

pub fn validate_username(username: &str) -> ApiResult<()> {
    let ok = (2..=40).contains(&username.chars().count())
        && username.chars().all(|c| c.is_alphanumeric() || "._-@".contains(c));
    if !ok {
        return Err(ApiError::bad_request("username must be 2-40 characters (letters, digits, . _ - @)"));
    }
    Ok(())
}

/// Argon2id hashing is CPU and memory heavy: run it off the async runtime.
pub async fn hash_password(password: String) -> anyhow::Result<String> {
    let _permit = HASHING.acquire().await?;
    tokio::task::spawn_blocking(move || {
        Argon2::default()
            .hash_password(password.as_bytes())
            .map(|h| h.to_string())
            .map_err(|e| anyhow!("password hashing failed: {e}"))
    })
    .await?
}

pub async fn verify_password(password: String, hash: String) -> bool {
    let Ok(_permit) = HASHING.acquire().await else { return false };
    tokio::task::spawn_blocking(move || match PasswordHash::new(&hash) {
        Ok(parsed) => Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok(),
        Err(_) => false,
    })
    .await
    .unwrap_or(false)
}

/// Address of the client, for brute-force protection. Behind a reverse proxy, the last entry of
/// `X-Forwarded-For` is the one the proxy added (earlier ones come from the client).
pub fn client_ip(headers: &axum::http::HeaderMap, connect: Option<std::net::IpAddr>) -> String {
    let get = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).map(str::trim).filter(|v| !v.is_empty());
    get("x-forwarded-for")
        .and_then(|v| v.rsplit(',').next())
        .map(str::trim)
        .or_else(|| get("x-real-ip"))
        .map(|v| v.chars().take(64).collect())
        .or_else(|| connect.map(|ip| ip.to_string()))
        .unwrap_or_else(|| "unknown".into())
}

// ---------------------------------------------------------------------------------------
// Extractors
// ---------------------------------------------------------------------------------------

/// An authenticated user. Checks the JWT *and* that the device session still exists, so
/// revoking a device from the web UI cuts it off immediately.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id: i64,
    pub username: String,
    pub role: String,
    pub session_id: i64,
}

impl AuthUser {
    pub fn is_admin(&self) -> bool {
        self.role == "admin"
    }
}

pub async fn authenticate(state: &AppState, token: &str) -> ApiResult<AuthUser> {
    let claims = state.auth.verify_access(token)?;
    let session = db::session_by_id(&state.db, claims.sid)
        .await?
        .ok_or_else(|| ApiError::unauthorized("session revoked"))?;
    if session.user_id != claims.uid {
        return Err(ApiError::unauthorized("session revoked"));
    }
    let user = db::user_by_id(&state.db, claims.uid)
        .await?
        .ok_or_else(|| ApiError::unauthorized("user no longer exists"))?;
    Ok(AuthUser { id: user.id, username: user.username, role: user.role, session_id: session.id })
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| ApiError::unauthorized("missing Authorization header"))?;
        let token = header
            .strip_prefix("Bearer ")
            .or_else(|| header.strip_prefix("bearer "))
            .ok_or_else(|| ApiError::unauthorized("expected a Bearer token"))?;
        authenticate(state, token.trim()).await
    }
}

/// An authenticated administrator.
#[derive(Debug, Clone)]
pub struct AdminUser(pub AuthUser);

impl FromRequestParts<AppState> for AdminUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let user = AuthUser::from_request_parts(parts, state).await?;
        if !user.is_admin() {
            return Err(ApiError::forbidden("administrator role required"));
        }
        Ok(AdminUser(user))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn password_hash_roundtrip() {
        let h = hash_password("correct horse".into()).await.unwrap();
        assert!(h.starts_with("$argon2id$"));
        assert!(verify_password("correct horse".into(), h.clone()).await);
        assert!(!verify_password("wrong".into(), h).await);
    }

    #[test]
    fn refresh_tokens_are_random_and_hashed() {
        let a = new_refresh_token();
        let b = new_refresh_token();
        assert_ne!(a, b);
        assert_eq!(hash_token(&a).len(), 64);
        assert_eq!(hash_token(&a), hash_token(&a));
    }

    #[test]
    fn throttle_locks_after_failures() {
        let s = AuthService::new(b"secret-secret-secret-secret-secret", 900, 365);
        for _ in 0..MAX_FAILED_LOGINS {
            assert!(s.check_throttle("bob", "1.2.3.4").is_ok());
            s.record_login("bob", "1.2.3.4", false);
        }
        assert!(s.check_throttle("Bob", "1.2.3.4").is_err());
        assert!(s.check_throttle("bob", "5.6.7.8").is_ok(), "the owner, elsewhere, is not locked out");
        s.record_login("bob", "1.2.3.4", true);
        assert!(s.check_throttle("bob", "1.2.3.4").is_ok());
    }

    #[test]
    fn throttle_limits_an_address_and_an_account() {
        let s = AuthService::new(b"secret-secret-secret-secret-secret", 900, 365);
        for i in 0..MAX_FAILED_PER_ADDRESS {
            assert!(s.check_throttle(&format!("user{i}"), "1.2.3.4").is_ok());
        }
        assert!(s.check_throttle("someone", "1.2.3.4").is_err(), "too many accounts tried from one address");
        for i in 0..MAX_FAILED_PER_ACCOUNT {
            assert!(s.check_throttle("admin", &format!("10.0.{}.{}", i / 250, i % 250)).is_ok());
        }
        assert!(s.check_throttle("admin", "10.9.9.9").is_err(), "a distributed attack on one account");
    }

    #[test]
    fn validates_inputs() {
        assert!(validate_password("short").is_err());
        assert!(validate_password("long enough").is_ok());
        assert!(validate_username("alice").is_ok());
        assert!(validate_username("a b").is_err());
    }
}
