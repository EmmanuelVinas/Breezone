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
    state.hub.log("auth", "info", None, Some(&user.username), "Compte administrateur créé", None).await;
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
        state.hub.log("auth", "warn", None, Some(username), "Échec de connexion", None).await;
        return Err(ApiError::unauthorized("invalid username or password"));
    };
    let device = req.device_name.as_deref().unwrap_or("Appareil inconnu");
    let pair = state.auth.open_session(&state.db, &user, device).await?;
    state.hub.log("auth", "info", None, Some(&user.username), &format!("Connexion depuis « {device} »"), None).await;
    Ok(Json(pair))
}

pub async fn refresh(State(state): State<AppState>, Json(req): Json<RefreshRequest>) -> ApiResult<Json<TokenPair>> {
    Ok(Json(state.auth.refresh(&state.db, req.refresh_token.trim()).await?))
}

pub async fn logout(State(state): State<AppState>, Json(req): Json<RefreshRequest>) -> ApiResult<StatusCode> {
    state.auth.revoke(&state.db, req.refresh_token.trim()).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn me(user: AuthUser) -> Json<Value> {
    Json(json!({ "id": user.id, "username": user.username, "role": user.role, "session_id": user.session_id }))
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
    state.hub.log("auth", "info", None, Some(&user.username), "Mot de passe modifié", None).await;
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
    state.hub.log("auth", "info", None, Some(&user.username), &format!("Appareil « {} » déconnecté", session.device_name), None).await;
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
}

pub async fn list_users(State(state): State<AppState>, _admin: AdminUser) -> ApiResult<Json<Vec<UserDetails>>> {
    let users = db::list_users(&state.db).await?;
    Ok(Json(users.iter().map(|u| UserDetails { user: u.into(), created_at: u.created_at }).collect()))
}

#[derive(Deserialize)]
pub struct NewUser {
    pub username: String,
    pub password: String,
    #[serde(default = "default_role")]
    pub role: String,
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

pub async fn create_user(State(state): State<AppState>, AdminUser(admin): AdminUser, Json(req): Json<NewUser>) -> ApiResult<(StatusCode, Json<UserView>)> {
    let username = req.username.trim();
    auth::validate_username(username)?;
    auth::validate_password(&req.password)?;
    check_role(&req.role)?;
    let hash = auth::hash_password(req.password).await?;
    let user = db::insert_user(&state.db, username, &hash, &req.role).await?;
    state.hub.log("auth", "info", None, Some(&admin.username), &format!("Utilisateur « {} » créé ({})", user.username, user.role), None).await;
    Ok((StatusCode::CREATED, Json((&user).into())))
}

#[derive(Deserialize)]
pub struct UserUpdate {
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
}

pub async fn update_user(State(state): State<AppState>, AdminUser(admin): AdminUser, Path(id): Path<i64>, Json(req): Json<UserUpdate>) -> ApiResult<Json<UserView>> {
    let user = db::user_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("user not found"))?;
    if let Some(role) = &req.role {
        check_role(role)?;
        if !db::update_user_role_keeping_an_admin(&state.db, id, role).await? {
            return Err(ApiError::bad_request("cannot demote the last administrator"));
        }
    }
    if let Some(password) = req.password {
        auth::validate_password(&password)?;
        let hash = auth::hash_password(password).await?;
        db::update_user_password(&state.db, id, &hash).await?;
        // A password reset signs the user out everywhere.
        db::delete_user_sessions(&state.db, id, None).await?;
    }
    state.hub.log("auth", "info", None, Some(&admin.username), &format!("Utilisateur « {} » modifié", user.username), None).await;
    let updated = db::user_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("user not found"))?;
    Ok(Json((&updated).into()))
}

pub async fn delete_user(State(state): State<AppState>, AdminUser(admin): AdminUser, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    let user = db::user_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("user not found"))?;
    if user.id == admin.id {
        return Err(ApiError::bad_request("you cannot delete your own account"));
    }
    if !db::delete_user_keeping_an_admin(&state.db, id).await? {
        return Err(ApiError::bad_request("cannot delete the last administrator"));
    }
    state.hub.log("auth", "info", None, Some(&admin.username), &format!("Utilisateur « {} » supprimé", user.username), None).await;
    Ok(StatusCode::NO_CONTENT)
}
