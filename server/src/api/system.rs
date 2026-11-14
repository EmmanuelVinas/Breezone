//! Updates of Breezone itself (issue #40), for administrators.

use crate::auth::AdminUser;
use crate::error::{ApiError, ApiResult};
use crate::update::UpdateView;
use crate::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;

/// The running version, the latest release and whether it can be installed from here.
pub async fn update_status(State(state): State<AppState>, _admin: AdminUser) -> Json<UpdateView> {
    Json(state.updater.view())
}

/// Looks for a release now (otherwise once a day).
pub async fn check(State(state): State<AppState>, _admin: AdminUser) -> ApiResult<Json<UpdateView>> {
    state.updater.check().await.map(Json).map_err(|e| {
        ApiError::bad_gateway(crate::text!(
            "Impossible de consulter les versions de Breezone : {0}",
            "Cannot read the Breezone releases: {0}",
            "No se pueden consultar las versiones de Breezone: {0}",
            e
        ))
    })
}

#[derive(serde::Deserialize)]
pub struct ChannelInput {
    pub beta: bool,
}

/// Joins or leaves the beta channel (pre-releases). The release found on the other channel is
/// forgotten: the client then looks again (`POST /api/system/update/check`), or the daily check does.
pub async fn set_channel(State(state): State<AppState>, _admin: AdminUser, Json(input): Json<ChannelInput>) -> ApiResult<Json<UpdateView>> {
    if input.beta {
        crate::db::set_setting(&state.db, crate::update::CHANNEL_SETTING, "beta").await?;
    } else {
        crate::db::delete_setting(&state.db, crate::update::CHANNEL_SETTING).await?;
    }
    state.updater.set_beta(input.beta);
    Ok(Json(state.updater.view()))
}

/// Installs the latest release, then restarts on it. Answers at once; the progress shows in
/// `GET /api/system/update`, and the server comes back with its new version.
pub async fn install(State(state): State<AppState>, AdminUser(admin): AdminUser) -> ApiResult<(StatusCode, Json<UpdateView>)> {
    state.updater.start_install(state.db.clone(), state.hub.clone(), admin.username.clone()).map_err(ApiError::conflict)?;
    Ok((StatusCode::ACCEPTED, Json(state.updater.view())))
}
