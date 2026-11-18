//! Schedules of the Airzone app (mirror of Airzone Cloud, edited there).

use crate::access::Access;
use crate::airzone_schedules::{self, ScheduleInput, ScheduleOp, SchedulesState};
use crate::auth::{AdminUser, AuthUser};
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
/// creating, changing or deleting one (`edit`) also needs the right to edit schedules. Returns the
/// linked installation.
async fn may_change(state: &AppState, user: &AuthUser, cloud_id: &str, edit: bool) -> ApiResult<(Access, Option<db::Installation>)> {
    let access = Access::of(&state.db, user).await?;
    let linked = db::list_installations(&state.db).await?.into_iter().find(|i| i.cloud_id.as_deref() == Some(cloud_id));
    if !access.is_admin() {
        let Some(i) = &linked else { return Err(ApiError::not_found("installation not found")) };
        access.control(i.id)?;
        if edit {
            access.edit_schedules(i.id)?;
        }
    }
    Ok((access, linked))
}

/// Applies the change here, or has the node owning the installation apply it with its Airzone
/// account (this server's account may not be allowed to write its schedules), then reads them back.
async fn change(state: &AppState, linked: Option<db::Installation>, cloud_id: &str, op: ScheduleOp, actor: &str, access: &Access) -> ApiResult<Json<SchedulesState>> {
    let schedules = match linked.and_then(|i| Some((i.node_id?, i.remote_id?))) {
        Some((node_id, remote_id)) => {
            state.hub.schedule_remote(node_id, remote_id, &op, actor).await?;
            airzone_schedules::sync(state).await?
        }
        None => airzone_schedules::apply(state, cloud_id, &op, actor).await?,
    };
    Ok(Json(visible(schedules, access)))
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
    let (access, linked) = may_change(&state, &user, &cloud_id, true).await?;
    change(&state, linked, &cloud_id, ScheduleOp::Create { input }, &user.username, &access).await
}

pub async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    Path((cloud_id, id)): Path<(String, String)>,
    Json(input): Json<ScheduleInput>,
) -> ApiResult<Json<SchedulesState>> {
    let (access, linked) = may_change(&state, &user, &cloud_id, true).await?;
    change(&state, linked, &cloud_id, ScheduleOp::Update { schedule_id: id, input }, &user.username, &access).await
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
    let (access, linked) = may_change(&state, &user, &cloud_id, false).await?;
    change(&state, linked, &cloud_id, ScheduleOp::Enable { schedule_id: id, enabled: input.enabled }, &user.username, &access).await
}

pub async fn delete(State(state): State<AppState>, user: AuthUser, Path((cloud_id, id)): Path<(String, String)>) -> ApiResult<Json<SchedulesState>> {
    let (access, linked) = may_change(&state, &user, &cloud_id, true).await?;
    change(&state, linked, &cloud_id, ScheduleOp::Delete { schedule_id: id }, &user.username, &access).await
}

/// Switches all the Airzone schedules off, so that Breezone's own do not compete with them, or on
/// again: only those it switched off, as they were (one switched off in the Airzone app stays off).
pub async fn set_all_enabled(State(state): State<AppState>, AdminUser(user): AdminUser, Json(input): Json<EnabledInput>) -> ApiResult<Json<SchedulesState>> {
    let access = Access::of(&state.db, &user).await?;
    let installations = db::list_installations(&state.db).await?;
    let linked = |cloud_id: &str| installations.iter().find(|i| i.cloud_id.as_deref() == Some(cloud_id)).cloned();
    let already = airzone_schedules::paused(&state).await?;
    let mut failed = None;
    if input.enabled {
        for key in already.unwrap_or_default() {
            let Some((cloud_id, id)) = key.split_once('/') else { continue };
            let op = ScheduleOp::Enable { schedule_id: id.to_string(), enabled: true };
            if let Err(e) = change(&state, linked(cloud_id), cloud_id, op, &user.username, &access).await {
                failed = Some(e);
            }
        }
        db::delete_setting(&state.db, airzone_schedules::KEY_PAUSED).await?;
    } else {
        let mut off = already.unwrap_or_default();
        for s in airzone_schedules::list(&state).await?.schedules.into_iter().filter(|s| s.enabled) {
            let op = ScheduleOp::Enable { schedule_id: s.id.clone(), enabled: false };
            match change(&state, linked(&s.cloud_id), &s.cloud_id, op, &user.username, &access).await {
                Ok(_) => off.push(format!("{}/{}", s.cloud_id, s.id)),
                Err(e) => failed = Some(e),
            }
        }
        db::set_setting(&state.db, airzone_schedules::KEY_PAUSED, &serde_json::to_string(&off).unwrap_or_default()).await?;
    }
    // Those that could be changed are: the list says where things stand, the error what failed.
    if let Some(e) = failed {
        return Err(e);
    }
    Ok(Json(visible(airzone_schedules::list(&state).await?, &access)))
}
