//! The overview of an installation (see `dashboard`).

use crate::access::Access;
use crate::auth::AuthUser;
use crate::dashboard::{self, Layout, Source};
use crate::db;
use crate::error::{ApiError, ApiResult};
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct DashboardView {
    /// `None`: no layout yet, the clients compose one.
    pub layout: Option<Layout>,
    pub source: Source,
    /// There is a default of the installation to go back to.
    pub has_default: bool,
    /// This user may set the default of the installation.
    pub can_set_default: bool,
}

/// A stored layout this version reads; one it cannot (a newer one wrote it) counts as none.
fn read(stored: Option<String>) -> Option<Layout> {
    stored.and_then(|s| serde_json::from_str::<Layout>(&s).ok()).filter(|l| l.version == dashboard::VERSION)
}

async fn view_of(state: &AppState, access: &Access, user: &AuthUser, id: i64) -> ApiResult<DashboardView> {
    let mine = read(db::dashboard(&state.db, id, Some(user.id)).await?);
    let default = read(db::dashboard(&state.db, id, None).await?);
    let has_default = default.is_some();
    let (layout, source) = match (mine, default) {
        (Some(l), _) => (Some(l), Source::Mine),
        (None, Some(l)) => (Some(l), Source::Installation),
        (None, None) => (None, Source::Generated),
    };
    Ok(DashboardView { layout, source, has_default, can_set_default: access.manages(id) })
}

pub async fn get(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<Json<DashboardView>> {
    let access = Access::of(&state.db, &user).await?;
    access.view(id)?;
    Ok(Json(view_of(&state, &access, &user, id).await?))
}

/// The user's own overview of this installation.
pub async fn save_mine(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>, Json(layout): Json<Layout>) -> ApiResult<Json<DashboardView>> {
    let access = Access::of(&state.db, &user).await?;
    access.view(id)?;
    db::set_dashboard(&state.db, id, Some(user.id), &dashboard::validate(&layout)?).await?;
    Ok(Json(view_of(&state, &access, &user, id).await?))
}

/// Back to the installation's default.
pub async fn delete_mine(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<Json<DashboardView>> {
    let access = Access::of(&state.db, &user).await?;
    access.view(id)?;
    db::delete_dashboard(&state.db, id, Some(user.id)).await?;
    Ok(Json(view_of(&state, &access, &user, id).await?))
}

/// The overview everyone sees (who has none of their own): the installation's administrators.
pub async fn save_default(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>, Json(layout): Json<Layout>) -> ApiResult<Json<DashboardView>> {
    let access = Access::of(&state.db, &user).await?;
    access.manage(id)?;
    db::set_dashboard(&state.db, id, None, &dashboard::validate(&layout)?).await?;
    // The one just set is what this user sees from now on.
    db::delete_dashboard(&state.db, id, Some(user.id)).await?;
    Ok(Json(view_of(&state, &access, &user, id).await?))
}

pub async fn delete_default(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    let access = Access::of(&state.db, &user).await?;
    access.manage(id)?;
    if !db::delete_dashboard(&state.db, id, None).await? {
        return Err(ApiError::not_found("no default overview"));
    }
    Ok(StatusCode::NO_CONTENT)
}
