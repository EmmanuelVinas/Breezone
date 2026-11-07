//! Routines CRUD and manual run.

use crate::access::Access;
use crate::auth::AuthUser;
use crate::db;
use crate::error::{ApiError, ApiResult};
use crate::routines::{self, RoutineInput, RoutineView};
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};

pub async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<RoutineView>>> {
    let access = Access::of(&state.db, &user).await?;
    Ok(Json(db::list_routines(&state.db).await?.iter().map(routines::to_view).filter(|r| access.sees_actions(&r.actions)).collect()))
}

pub async fn get(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<Json<RoutineView>> {
    let row = db::routine_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("routine not found"))?;
    let view = routines::to_view(&row);
    if !Access::of(&state.db, &user).await?.sees_actions(&view.actions) {
        return Err(ApiError::not_found("routine not found"));
    }
    Ok(Json(view))
}

pub async fn create(State(state): State<AppState>, user: AuthUser, Json(input): Json<RoutineInput>) -> ApiResult<(StatusCode, Json<RoutineView>)> {
    let (name, actions) = routines::validate(&input)?;
    Access::of(&state.db, &user).await?.control_actions(&input.actions)?;
    let row = db::insert_routine(&state.db, &name, input.enabled, 0, None, &actions).await?;
    state.hub.log("routine", "info", None, Some(&user.username), &format!("Routine « {name} » créée"), None).await;
    state.hub.routines_changed();
    Ok((StatusCode::CREATED, Json(routines::to_view(&row))))
}

pub async fn update(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>, Json(input): Json<RoutineInput>) -> ApiResult<Json<RoutineView>> {
    let before = db::routine_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("routine not found"))?;
    let (name, actions) = routines::validate(&input)?;
    let access = Access::of(&state.db, &user).await?;
    access.control_actions(&routines::to_view(&before).actions)?;
    access.control_actions(&input.actions)?;
    db::update_routine(&state.db, id, &name, input.enabled, 0, None, &actions).await?;
    state.hub.log("routine", "info", None, Some(&user.username), &format!("Routine « {name} » modifiée"), None).await;
    state.hub.routines_changed();
    let row = db::routine_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("routine not found"))?;
    Ok(Json(routines::to_view(&row)))
}

pub async fn delete(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    let row = db::routine_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("routine not found"))?;
    Access::of(&state.db, &user).await?.control_actions(&routines::to_view(&row).actions)?;
    db::delete_routine(&state.db, id).await?;
    state.hub.log("routine", "info", None, Some(&user.username), &format!("Routine « {} » supprimée", row.name), None).await;
    state.hub.routines_changed();
    Ok(StatusCode::NO_CONTENT)
}

pub async fn run(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let row = db::routine_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("routine not found"))?;
    Access::of(&state.db, &user).await?.control_actions(&routines::to_view(&row).actions)?;
    let results = routines::run_routine(&state.hub, &state.db, &row, &user.username).await?;
    Ok(Json(json!({ "results": results })))
}

