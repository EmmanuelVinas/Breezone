//! Timers ("minuteries") and the routines of the Airzone app.

use crate::access::Access;
use crate::auth::AuthUser;
use crate::db;
use crate::error::{ApiError, ApiResult};
use crate::timers::{self, TimerInput, TimerView};
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};

pub async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<TimerView>>> {
    let access = Access::of(&state.db, &user).await?;
    Ok(Json(timers::list(&state.db).await?.into_iter().filter(|t| access.sees_target(&t.target)).collect()))
}

pub async fn create(State(state): State<AppState>, user: AuthUser, Json(input): Json<TimerInput>) -> ApiResult<(StatusCode, Json<TimerView>)> {
    Access::of(&state.db, &user).await?.control_target(&input.target)?;
    Ok((StatusCode::CREATED, Json(timers::create(&state.hub, &state.db, &input, &user.username).await?)))
}

pub async fn cancel(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    let timer = timers::list(&state.db).await?.into_iter().find(|t| t.id == id).ok_or_else(|| ApiError::not_found("minuterie introuvable"))?;
    Access::of(&state.db, &user).await?.control_target(&timer.target)?;
    timers::cancel(&state.hub, &state.db, id, &user.username).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Routines of the Airzone app, as read at the last synchronisation (names and raw data).
pub async fn airzone_routines(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Value>> {
    let access = Access::of(&state.db, &user).await?;
    // Airzone installation → Breezone installation (to apply the user's access).
    let linked: std::collections::HashMap<String, i64> =
        db::list_installations(&state.db).await?.into_iter().filter_map(|i| i.cloud_id.map(|c| (c, i.id))).collect();
    let names: std::collections::HashMap<String, String> = db::list_cloud_installations(&state.db).await?.into_iter().map(|r| (r.cloud_id, r.name)).collect();
    let mut out = vec![];
    for (cloud_id, data) in db::list_airzone_routines(&state.db).await? {
        if !access.is_admin() && !linked.get(&cloud_id).is_some_and(|id| access.can_view(*id)) {
            continue;
        }
        let v: Value = serde_json::from_str(&data).unwrap_or_default();
        let list = v["data"]["routines"].as_array().or_else(|| v["routines"].as_array()).or_else(|| v["data"].as_array()).or_else(|| v.as_array()).cloned().unwrap_or_default();
        for r in list {
            out.push(json!({
                "cloud_id": cloud_id,
                "installation": names.get(&cloud_id).cloned().unwrap_or_else(|| cloud_id.clone()),
                "id": r["_id"].as_str().or_else(|| r["id"].as_str()),
                "name": r["name"].as_str().unwrap_or("Routine"),
                "enabled": r["enabled"].as_bool().or_else(|| r["active"].as_bool()),
                "raw": r,
            }));
        }
    }
    Ok(Json(json!(out)))
}
