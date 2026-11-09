//! Schedules of the Airzone app (mirror of Airzone Cloud, edited there).

use crate::airzone_schedules::{self, ScheduleInput, SchedulesState};
use crate::access::Access;
use crate::auth::AuthUser;
use crate::db;
use crate::error::{ApiError, ApiResult};
use crate::AppState;
use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;

/// Only the schedules of the installations the user can see.
fn visible(mut list: SchedulesState, access: &Access) -> SchedulesState {
    if !access.is_admin() {
        list.schedules.retain(|s| s.installation_id.is_some_and(|id| access.can_view(id)));
    }
    list
}

/// Switching a schedule on or off needs control over the Breezone installation linked to it;
/// creating, changing or deleting one (`edit`) also needs the right to edit schedules.
async fn may_change(state: &AppState, user: &AuthUser, cloud_id: &str, edit: bool) -> ApiResult<Access> {
    let access = Access::of(&state.db, user).await?;
    if !access.is_admin() {
        let linked = db::list_installations(&state.db).await?.into_iter().find(|i| i.cloud_id.as_deref() == Some(cloud_id));
        let Some(i) = linked else { return Err(ApiError::not_found("installation not found")) };
        access.control(i.id)?;
        if edit {
            access.edit_schedules(i.id)?;
        }
    }
    Ok(access)
}

pub async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<SchedulesState>> {
    let access = Access::of(&state.db, &user).await?;
    Ok(Json(visible(airzone_schedules::list(&state).await?, &access)))
}

pub async fn sync(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<SchedulesState>> {
    let access = Access::of(&state.db, &user).await?;
    Ok(Json(visible(airzone_schedules::sync(&state).await?, &access)))
}

pub async fn create(State(state): State<AppState>, user: AuthUser, Json(input): Json<ScheduleInput>) -> ApiResult<Json<SchedulesState>> {
    let cloud_id = input.cloud_id.clone().unwrap_or_default();
    let access = may_change(&state, &user, &cloud_id, true).await?;
    Ok(Json(visible(airzone_schedules::create(&state, &input, &user.username).await?, &access)))
}

pub async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    Path((cloud_id, id)): Path<(String, String)>,
    Json(input): Json<ScheduleInput>,
) -> ApiResult<Json<SchedulesState>> {
    let access = may_change(&state, &user, &cloud_id, true).await?;
    Ok(Json(visible(airzone_schedules::update(&state, &cloud_id, &id, &input, &user.username).await?, &access)))
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
    let access = may_change(&state, &user, &cloud_id, false).await?;
    Ok(Json(visible(airzone_schedules::set_enabled(&state, &cloud_id, &id, input.enabled, &user.username).await?, &access)))
}

pub async fn delete(State(state): State<AppState>, user: AuthUser, Path((cloud_id, id)): Path<(String, String)>) -> ApiResult<Json<SchedulesState>> {
    let access = may_change(&state, &user, &cloud_id, true).await?;
    Ok(Json(visible(airzone_schedules::delete(&state, &cloud_id, &id, &user.username).await?, &access)))
}
