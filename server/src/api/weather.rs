//! The sun and the weather of each installation (roadmap F9).

use crate::access::Access;
use crate::auth::{AdminUser, AuthUser};
use crate::db;
use crate::error::{ApiError, ApiResult};
use crate::weather::View;
use crate::AppState;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct WeatherQuery {
    /// The installation (by default the home: the first installation of this server with a city).
    pub installation_id: Option<i64>,
}

/// The installation asked, if this user sees it.
async fn visible(state: &AppState, user: &AuthUser, id: Option<i64>) -> ApiResult<Option<i64>> {
    if let Some(id) = id {
        Access::of(&state.db, user).await?.view(id)?;
    }
    Ok(id)
}

/// The weather now, today and in the next hours, and the sun of today, at an installation.
/// `weather` is null when it has no city yet, or before the first reading.
pub async fn weather(State(state): State<AppState>, user: AuthUser, Query(q): Query<WeatherQuery>) -> ApiResult<Json<View>> {
    let id = visible(&state, &user, q.installation_id).await?;
    Ok(Json(state.weather.view(id).await))
}

#[derive(Deserialize)]
pub struct HistoryQuery {
    #[serde(default = "crate::api::default_hours")]
    pub hours: i64,
    pub installation_id: Option<i64>,
}

/// The outdoor temperature over the last hours: `[[ts, °C]]`, at most one point per 30 min (per
/// day beyond a month).
pub async fn history(State(state): State<AppState>, user: AuthUser, Query(q): Query<HistoryQuery>) -> ApiResult<Json<Vec<(i64, f64)>>> {
    let id = visible(&state, &user, q.installation_id).await?;
    let hours = q.hours.clamp(1, 24 * 366 * 5);
    let bucket = if hours > 24 * 31 { 86_400 } else { 1800 };
    Ok(Json(state.weather.history(id, db::now() - hours * 3600, bucket).await?))
}

#[derive(Deserialize)]
pub struct PlaceInput {
    /// "Labège, Haute-Garonne"; empty: the city of the first installation.
    pub place: String,
}

/// Sets the city of the home: that of the first installation of this server (each installation has
/// its own city, set with the installation). Reads its weather at once.
pub async fn set_place(State(state): State<AppState>, _admin: AdminUser, Json(input): Json<PlaceInput>) -> ApiResult<StatusCode> {
    let place = input.place.trim();
    if place.len() > 120 {
        return Err(ApiError::bad_request("place too long"));
    }
    let first = db::list_installations(&state.db).await?.into_iter().find(|i| i.is_local()).ok_or_else(|| ApiError::not_found("installation not found"))?;
    db::set_installation_place(&state.db, first.id, Some(place), first.country.as_deref()).await?;
    state.hub.reload_installations().await?;
    state.weather.refresh().await;
    Ok(StatusCode::NO_CONTENT)
}
