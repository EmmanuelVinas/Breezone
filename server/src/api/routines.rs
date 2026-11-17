//! Routines CRUD and manual run.

use crate::access::Access;
use crate::auth::AuthUser;
use crate::db;
use crate::error::{ApiError, ApiResult};
use crate::routines::{self, DeviceScopes, RoutineInput, RoutineView};
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};

pub async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<RoutineView>>> {
    let access = Access::of(&state.db, &user).await?;
    let scopes = DeviceScopes::load(&state).await?;
    Ok(Json(db::list_routines(&state.db).await?.iter().map(routines::to_view).filter(|r| access.sees_scopes(&scopes.of(&r.actions))).collect()))
}

pub async fn get(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<Json<RoutineView>> {
    let row = db::routine_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("routine not found"))?;
    let view = routines::to_view(&row);
    let scopes = DeviceScopes::load(&state).await?;
    if !Access::of(&state.db, &user).await?.sees_scopes(&scopes.of(&view.actions)) {
        return Err(ApiError::not_found("routine not found"));
    }
    Ok(Json(view))
}

pub async fn create(State(state): State<AppState>, user: AuthUser, Json(input): Json<RoutineInput>) -> ApiResult<(StatusCode, Json<RoutineView>)> {
    let (name, actions) = routines::validate(&input)?;
    let scopes = DeviceScopes::load(&state).await?;
    Access::of(&state.db, &user).await?.edit_routine(&scopes.of(&input.actions))?;
    routines::check_devices(&state, &input.actions).await?;
    let row = db::insert_routine(&state.db, &name, input.enabled, 0, None, &actions).await?;
    state
        .hub
        .log(
            "routine",
            "info",
            routines::installation_of(&input.actions),
            Some(&user.username),
            crate::text!("Routine « {0} » créée", "Routine “{0}” created", "Rutina «{0}» creada", name),
            None,
        )
        .await;
    state.hub.routines_changed();
    Ok((StatusCode::CREATED, Json(routines::to_view(&row))))
}

pub async fn update(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>, Json(input): Json<RoutineInput>) -> ApiResult<Json<RoutineView>> {
    let before = db::routine_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("routine not found"))?;
    let (name, actions) = routines::validate(&input)?;
    let access = Access::of(&state.db, &user).await?;
    let scopes = DeviceScopes::load(&state).await?;
    access.edit_routine(&scopes.of(&routines::to_view(&before).actions))?;
    access.edit_routine(&scopes.of(&input.actions))?;
    routines::check_devices(&state, &input.actions).await?;
    db::update_routine(&state.db, id, &name, input.enabled, 0, None, &actions).await?;
    state
        .hub
        .log(
            "routine",
            "info",
            routines::installation_of(&input.actions),
            Some(&user.username),
            crate::text!("Routine « {0} » modifiée", "Routine “{0}” changed", "Rutina «{0}» modificada", name),
            None,
        )
        .await;
    state.hub.routines_changed();
    let row = db::routine_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("routine not found"))?;
    Ok(Json(routines::to_view(&row)))
}

pub async fn delete(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    let row = db::routine_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("routine not found"))?;
    let scopes = DeviceScopes::load(&state).await?;
    Access::of(&state.db, &user).await?.edit_routine(&scopes.of(&routines::to_view(&row).actions))?;
    db::delete_routine(&state.db, id).await?;
    let installation = routines::installation_of(&routines::to_view(&row).actions);
    state
        .hub
        .log("routine", "info", installation, Some(&user.username), crate::text!("Routine « {0} » supprimée", "Routine “{0}” deleted", "Rutina «{0}» eliminada", row.name), None)
        .await;
    state.hub.routines_changed();
    Ok(StatusCode::NO_CONTENT)
}

pub async fn run(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let row = db::routine_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("routine not found"))?;
    let scopes = DeviceScopes::load(&state).await?;
    Access::of(&state.db, &user).await?.control_scopes(&scopes.of(&routines::to_view(&row).actions))?;
    let results = routines::run_routine(&state, &row, &user.username).await?;
    Ok(Json(json!({ "results": results })))
}
