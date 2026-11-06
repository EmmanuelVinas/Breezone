//! Schedules of the Airzone app (mirror of Airzone Cloud, edited there).

use crate::airzone_schedules::{self, ScheduleInput, SchedulesState};
use crate::auth::AuthUser;
use crate::error::ApiResult;
use crate::AppState;
use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;

pub async fn list(State(state): State<AppState>, _user: AuthUser) -> ApiResult<Json<SchedulesState>> {
    Ok(Json(airzone_schedules::list(&state).await?))
}

pub async fn sync(State(state): State<AppState>, _user: AuthUser) -> ApiResult<Json<SchedulesState>> {
    Ok(Json(airzone_schedules::sync(&state).await?))
}

pub async fn create(State(state): State<AppState>, user: AuthUser, Json(input): Json<ScheduleInput>) -> ApiResult<Json<SchedulesState>> {
    Ok(Json(airzone_schedules::create(&state, &input, &user.username).await?))
}

pub async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    Path((cloud_id, id)): Path<(String, String)>,
    Json(input): Json<ScheduleInput>,
) -> ApiResult<Json<SchedulesState>> {
    Ok(Json(airzone_schedules::update(&state, &cloud_id, &id, &input, &user.username).await?))
}

#[derive(Deserialize)]
pub struct EnabledInput {
    pub enabled: bool,
}

pub async fn set_enabled(
    State(state): State<AppState>,
    user: AuthUser,
    Path((cloud_id, id)): Path<(String, String)>,
    Json(input): Json<EnabledInput>,
) -> ApiResult<Json<SchedulesState>> {
    Ok(Json(airzone_schedules::set_enabled(&state, &cloud_id, &id, input.enabled, &user.username).await?))
}

pub async fn delete(State(state): State<AppState>, user: AuthUser, Path((cloud_id, id)): Path<(String, String)>) -> ApiResult<Json<SchedulesState>> {
    Ok(Json(airzone_schedules::delete(&state, &cloud_id, &id, &user.username).await?))
}
