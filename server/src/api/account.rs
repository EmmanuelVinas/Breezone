//! Setup, authentication, current user, devices and user management.

use crate::auth::{self, AdminUser, AuthUser, TokenPair, UserView};
use crate::db;
use crate::error::{ApiError, ApiResult};
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Deserialize)]
pub struct Credentials {
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub device_name: Option<String>,
}

#[derive(Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

pub async fn setup_status(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let needs_setup = db::count_users(&state.db).await? == 0;
    Ok(Json(json!({ "needs_setup": needs_setup })))
}

/// First start: creates the administrator account. Only allowed while no user exists.
pub async fn setup(State(state): State<AppState>, Json(req): Json<Credentials>) -> ApiResult<Json<TokenPair>> {
    if db::count_users(&state.db).await? > 0 {
        return Err(ApiError::conflict("Breezone is already set up"));
    }
    let username = req.username.trim();
    auth::validate_username(username)?;
    auth::validate_password(&req.password)?;
    let hash = auth::hash_password(req.password.clone()).await?;
    let user = db::insert_first_admin(&state.db, username, &hash).await?.ok_or_else(|| ApiError::conflict("Breezone is already set up"))?;
    state.hub.log("auth", "info", None, Some(&user.username), crate::text!("Compte administrateur créé", "Administrator account created", "Cuenta de administrador creada"), None).await;
    let device = req.device_name.as_deref().unwrap_or("Navigateur");
    Ok(Json(state.auth.open_session(&state.db, &user, device).await?))
}

pub async fn login(State(state): State<AppState>, Json(req): Json<Credentials>) -> ApiResult<Json<TokenPair>> {
    let username = req.username.trim();
    // Counts the attempt before checking the password: parallel attempts cannot bypass the limit.
    state.auth.check_throttle(username)?;
    let user = db::user_by_name(&state.db, username).await?;
    let valid = match &user {
        Some(u) => auth::verify_password(req.password.clone(), u.password_hash.clone()).await,
        None => {
            // Spend the same time as a real verification to avoid user enumeration.
            let _ = auth::hash_password(req.password.clone()).await;
            false
        }
    };
    state.auth.record_login(username, valid);
    let Some(user) = user.filter(|_| valid) else {
        state.hub.log("auth", "warn", None, Some(username), crate::text!("Échec de connexion", "Failed sign-in", "Inicio de sesión fallido"), None).await;
        return Err(ApiError::unauthorized("invalid username or password"));
    };
    let device = req.device_name.as_deref().unwrap_or("Appareil inconnu");
    let pair = state.auth.open_session(&state.db, &user, device).await?;
    state.hub.log("auth", "info", None, Some(&user.username), crate::text!("Connexion depuis « {0} »", "Signed in from “{0}”", "Inicio de sesión desde «{0}»", device), None).await;
    Ok(Json(pair))
}

pub async fn refresh(State(state): State<AppState>, Json(req): Json<RefreshRequest>) -> ApiResult<Json<TokenPair>> {
    Ok(Json(state.auth.refresh(&state.db, req.refresh_token.trim()).await?))
}

pub async fn logout(State(state): State<AppState>, Json(req): Json<RefreshRequest>) -> ApiResult<StatusCode> {
    state.auth.revoke(&state.db, req.refresh_token.trim()).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn me(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Value>> {
    // Installations this user can see and whether they can drive them (null: all, administrator).
    let access = if user.is_admin() { None } else { Some(access_entries(&state, user.id).await?) };
    let row = db::user_by_id(&state.db, user.id).await?;
    let rights = row.as_ref().map_or(db::UserRights::NONE, |u| u.rights());
    let language = row.and_then(|u| u.language);
    Ok(Json(json!({
        "id": user.id,
        "username": user.username,
        "role": user.role,
        "session_id": user.session_id,
        "access": access,
        "rights": rights,
        "language": language,
    })))
}

#[derive(Deserialize)]
pub struct LanguageChange {
    /// "fr" | "en" | "es", or null to follow the browser.
    pub language: Option<String>,
}

pub async fn change_language(State(state): State<AppState>, user: AuthUser, Json(req): Json<LanguageChange>) -> ApiResult<StatusCode> {
    let language = match req.language.as_deref() {
        None | Some("") => None,
        Some(code) => Some(crate::i18n::Lang::parse(code).filter(|l| l.code() == code).ok_or_else(|| {
            ApiError::bad_request(crate::text!("langue attendue : fr, en ou es", "language expected: fr, en or es", "idioma esperado: fr, en o es"))
        })?),
    };
    db::update_user_language(&state.db, user.id, language.map(|l| l.code())).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct PasswordChange {
    pub current_password: String,
    pub new_password: String,
}

/// Changes the password and signs out every *other* device.
pub async fn change_password(State(state): State<AppState>, user: AuthUser, Json(req): Json<PasswordChange>) -> ApiResult<StatusCode> {
    let current = db::user_by_id(&state.db, user.id).await?.ok_or_else(|| ApiError::not_found("user not found"))?;
    if !auth::verify_password(req.current_password, current.password_hash).await {
        return Err(ApiError::bad_request("current password is incorrect"));
    }
    auth::validate_password(&req.new_password)?;
    let hash = auth::hash_password(req.new_password).await?;
    db::update_user_password(&state.db, user.id, &hash).await?;
    db::delete_user_sessions(&state.db, user.id, Some(user.session_id)).await?;
    state.hub.log("auth", "info", None, Some(&user.username), crate::text!("Mot de passe modifié", "Password changed", "Contraseña cambiada"), None).await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
pub struct SessionView {
    pub id: i64,
    pub device_name: String,
    pub created_at: i64,
    pub last_used_at: i64,
    pub expires_at: i64,
    pub current: bool,
}

pub async fn list_sessions(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<SessionView>>> {
    let sessions = db::list_sessions(&state.db, user.id).await?;
    Ok(Json(
        sessions
            .into_iter()
            .map(|s| SessionView {
                current: s.id == user.session_id,
                id: s.id,
                device_name: s.device_name,
                created_at: s.created_at,
                last_used_at: s.last_used_at,
                expires_at: s.expires_at,
            })
            .collect(),
    ))
}

/// Revokes a device. Its refresh token stops working and it is cut off immediately.
pub async fn delete_session(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    let session = db::session_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("session not found"))?;
    if session.user_id != user.id && !user.is_admin() {
        return Err(ApiError::not_found("session not found"));
    }
    db::delete_session(&state.db, id).await?;
    state.hub.log("auth", "info", None, Some(&user.username), crate::text!("Appareil « {0} » déconnecté", "Device “{0}” signed out", "Dispositivo «{0}» desconectado", session.device_name), None).await;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------------------
// Users (admin)
// ---------------------------------------------------------------------------------------

#[derive(Serialize)]
pub struct UserDetails {
    #[serde(flatten)]
    pub user: UserView,
    pub created_at: i64,
    /// Installations a user (role "user") can see; administrators see everything.
    pub access: Vec<AccessEntry>,
    /// What the user may change besides driving (all true for an administrator).
    pub rights: db::UserRights,
}

/// One installation given to a user.
#[derive(Serialize, Deserialize, Clone)]
pub struct AccessEntry {
    pub installation_id: i64,
    /// "read" | "control"
    pub permission: String,
}

async fn access_entries(state: &AppState, user_id: i64) -> ApiResult<Vec<AccessEntry>> {
    Ok(db::user_access(&state.db, user_id)
        .await?
        .into_iter()
        .map(|(installation_id, permission)| AccessEntry { installation_id, permission })
        .collect())
}

async fn user_details(state: &AppState, u: &db::User) -> ApiResult<UserDetails> {
    Ok(UserDetails { user: u.into(), created_at: u.created_at, access: access_entries(state, u.id).await?, rights: u.rights() })
}

/// Checks and stores the installations given to a user.
async fn save_access(state: &AppState, user_id: i64, entries: &[AccessEntry]) -> ApiResult<()> {
    let mut clean: Vec<(i64, String)> = vec![];
    for e in entries {
        if !["read", "control"].contains(&e.permission.as_str()) {
            return Err(ApiError::bad_request("droit attendu : read ou control"));
        }
        db::installation_by_id(&state.db, e.installation_id)
            .await?
            .ok_or_else(|| ApiError::bad_request(crate::text!("installation {0} inconnue", "unknown installation {0}", "instalación {0} desconocida", e.installation_id)))?;
        if !clean.iter().any(|(id, _)| *id == e.installation_id) {
            clean.push((e.installation_id, e.permission.clone()));
        }
    }
    db::set_user_access(&state.db, user_id, &clean).await?;
    state.hub.access_changed(user_id);
    Ok(())
}

pub async fn list_users(State(state): State<AppState>, _admin: AdminUser) -> ApiResult<Json<Vec<UserDetails>>> {
    let mut out = vec![];
    for u in db::list_users(&state.db).await? {
        out.push(user_details(&state, &u).await?);
    }
    Ok(Json(out))
}

#[derive(Deserialize)]
pub struct NewUser {
    pub username: String,
    pub password: String,
    #[serde(default = "default_role")]
    pub role: String,
    /// Installations given to the user (none by default).
    #[serde(default)]
    pub access: Vec<AccessEntry>,
    /// What the user may change besides driving (everything by default).
    #[serde(default)]
    pub rights: db::UserRights,
}

fn default_role() -> String {
    "user".into()
}

fn check_role(role: &str) -> ApiResult<()> {
    if role != "admin" && role != "user" {
        return Err(ApiError::bad_request("role must be 'admin' or 'user'"));
    }
    Ok(())
}

pub async fn create_user(State(state): State<AppState>, AdminUser(admin): AdminUser, Json(req): Json<NewUser>) -> ApiResult<(StatusCode, Json<UserDetails>)> {
    let username = req.username.trim();
    auth::validate_username(username)?;
    auth::validate_password(&req.password)?;
    check_role(&req.role)?;
    let hash = auth::hash_password(req.password).await?;
    let user = db::insert_user(&state.db, username, &hash, &req.role).await?;
    save_access(&state, user.id, &req.access).await?;
    if req.rights != db::UserRights::default() {
        db::update_user_rights(&state.db, user.id, &req.rights).await?;
    }
    let user = db::user_by_id(&state.db, user.id).await?.ok_or_else(|| ApiError::not_found("user not found"))?;
    state.hub.log("auth", "info", None, Some(&admin.username), crate::text!("Utilisateur « {0} » créé ({1})", "User “{0}” created ({1})", "Usuario «{0}» creado ({1})", user.username, user.role), None).await;
    Ok((StatusCode::CREATED, Json(user_details(&state, &user).await?)))
}

#[derive(Deserialize)]
pub struct UserUpdate {
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    /// Replaces the installations given to the user (unchanged if absent).
    #[serde(default)]
    pub access: Option<Vec<AccessEntry>>,
    /// Replaces what the user may change besides driving (unchanged if absent).
    #[serde(default)]
    pub rights: Option<db::UserRights>,
}

pub async fn update_user(State(state): State<AppState>, AdminUser(admin): AdminUser, Path(id): Path<i64>, Json(req): Json<UserUpdate>) -> ApiResult<Json<UserDetails>> {
    let user = db::user_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("user not found"))?;
    if let Some(role) = &req.role {
        check_role(role)?;
        if !db::update_user_role_keeping_an_admin(&state.db, id, role).await? {
            return Err(ApiError::bad_request("cannot demote the last administrator"));
        }
        state.hub.access_changed(id);
    }
    if let Some(access) = &req.access {
        save_access(&state, id, access).await?;
    }
    if let Some(rights) = &req.rights {
        db::update_user_rights(&state.db, id, rights).await?;
        state.hub.access_changed(id);
    }
    if let Some(password) = req.password {
        auth::validate_password(&password)?;
        let hash = auth::hash_password(password).await?;
        db::update_user_password(&state.db, id, &hash).await?;
        // A password reset signs the user out everywhere.
        db::delete_user_sessions(&state.db, id, None).await?;
    }
    state.hub.log("auth", "info", None, Some(&admin.username), crate::text!("Utilisateur « {0} » modifié", "User “{0}” changed", "Usuario «{0}» modificado", user.username), None).await;
    let updated = db::user_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("user not found"))?;
    Ok(Json(user_details(&state, &updated).await?))
}

pub async fn delete_user(State(state): State<AppState>, AdminUser(admin): AdminUser, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    let user = db::user_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("user not found"))?;
    if user.id == admin.id {
        return Err(ApiError::bad_request("you cannot delete your own account"));
    }
    if !db::delete_user_keeping_an_admin(&state.db, id).await? {
        return Err(ApiError::bad_request("cannot delete the last administrator"));
    }
    state.hub.log("auth", "info", None, Some(&admin.username), crate::text!("Utilisateur « {0} » supprimé", "User “{0}” deleted", "Usuario «{0}» eliminado", user.username), None).await;
    Ok(StatusCode::NO_CONTENT)
}
