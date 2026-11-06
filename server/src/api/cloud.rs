//! Airzone Cloud synchronisation endpoints.

use crate::auth::{AdminUser, AuthUser};
use crate::cloud::{self, AccessInput, AttachResult, CloudStatus, ConnectRequest};
use crate::error::ApiResult;
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};

/// Sync status and the list of Airzone Cloud installations (incl. the ones on other sites).
pub async fn status(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<CloudStatus>> {
    Ok(Json(cloud::status(&state, user.is_admin()).await?))
}

/// Connects the Airzone account: imports now, then synchronises daily if `remember`.
pub async fn connect(State(state): State<AppState>, AdminUser(admin): AdminUser, Json(req): Json<ConnectRequest>) -> ApiResult<Json<Value>> {
    let report = cloud::connect(&state, &req, &admin.username).await?;
    Ok(Json(json!({ "report": report, "status": cloud::status(&state, true).await? })))
}

/// Synchronises now with the remembered credentials.
pub async fn sync(State(state): State<AppState>, AdminUser(admin): AdminUser) -> ApiResult<Json<Value>> {
    let report = cloud::sync_stored(&state, &admin.username).await?;
    Ok(Json(json!({ "report": report, "status": cloud::status(&state, true).await? })))
}

/// Forgets the Airzone credentials and the cloud installation list.
pub async fn disconnect(State(state): State<AppState>, AdminUser(admin): AdminUser) -> ApiResult<StatusCode> {
    cloud::forget(&state).await?;
    state.hub.log("system", "info", None, Some(&admin.username), "Compte Airzone Cloud oublié", None).await;
    Ok(StatusCode::NO_CONTENT)
}

/// Sets how this server reaches the WebServer of one Airzone installation (fixed address or
/// subnet to scan) and tries to reach it right away.
pub async fn set_access(
    State(state): State<AppState>,
    AdminUser(admin): AdminUser,
    Path(cloud_id): Path<String>,
    Json(input): Json<AccessInput>,
) -> ApiResult<Json<Value>> {
    let result: AttachResult = cloud::set_access(&state, &cloud_id, &input, &admin.username).await?;
    Ok(Json(json!({ "result": result, "status": cloud::status(&state, true).await? })))
}
