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

/// The configuration of this server as a file (no secret, no history): `breezone-config-<date>.json`.
pub async fn export_config(State(state): State<AppState>, _admin: AdminUser) -> ApiResult<impl axum::response::IntoResponse> {
    let file = crate::backup::export(&state.db, env!("CARGO_PKG_VERSION")).await?;
    let date = chrono::Local::now().format("%Y-%m-%d");
    let disposition = format!("attachment; filename=\"breezone-config-{date}.json\"");
    Ok(([(axum::http::header::CONTENT_DISPOSITION, disposition)], Json(file)))
}

/// Replaces the configuration with an exported one, then takes it into account at once.
pub async fn import_config(State(state): State<AppState>, AdminUser(admin): AdminUser, Json(file): Json<serde_json::Value>) -> ApiResult<Json<crate::backup::Imported>> {
    use crate::backup::ImportError;
    let imported = crate::backup::import(&state.db, &file, admin.id).await.map_err(|e| match e {
        ImportError::Invalid(why) => ApiError::bad_request(crate::text!(
            "Ce fichier n'est pas une configuration Breezone valable : {0}",
            "This file is not a valid Breezone configuration: {0}",
            "Este archivo no es una configuración Breezone válida: {0}",
            why
        )),
        ImportError::Newer => ApiError::bad_request(crate::text!(
            "Cette configuration vient d'une version plus récente de Breezone : mettez d'abord ce serveur à jour.",
            "This configuration comes from a newer Breezone: update this server first.",
            "Esta configuración viene de una versión más reciente de Breezone: actualice primero este servidor."
        )),
        ImportError::Db(e) => e.into(),
    })?;
    state.hub.reload_installations().await?;
    state.drivers.reload().await?;
    state.updater.set_beta(crate::db::get_setting(&state.db, crate::update::CHANNEL_SETTING).await?.as_deref() == Some("beta"));
    let _ = crate::db::insert_event(
        &state.db,
        "system",
        "info",
        None,
        Some(&admin.username),
        &crate::text!("Configuration importée", "Configuration imported", "Configuración importada"),
        None,
    )
    .await;
    Ok(Json(imported))
}
