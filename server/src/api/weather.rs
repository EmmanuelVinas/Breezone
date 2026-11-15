//! The sun and the weather of the home (roadmap F9).

use crate::auth::{AdminUser, AuthUser};
use crate::db;
use crate::error::{ApiError, ApiResult};
use crate::weather::View;
use crate::AppState;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

/// The weather now, today and in the next hours, and the sun of today. `weather` is null when the
/// home has no city yet, or before the first reading.
pub async fn weather(State(state): State<AppState>, _user: AuthUser) -> Json<View> {
    Json(state.weather.view().await)
}

#[derive(Deserialize)]
pub struct HistoryQuery {
    #[serde(default = "default_hours")]
    pub hours: i64,
}

fn default_hours() -> i64 {
    24
}

/// The outdoor temperature over the last hours: `[[ts, °C]]`, at most one point per 30 min (per
/// day beyond a month).
pub async fn history(State(state): State<AppState>, _user: AuthUser, Query(q): Query<HistoryQuery>) -> ApiResult<Json<Vec<(i64, f64)>>> {
    let hours = q.hours.clamp(1, 24 * 366 * 5);
    let bucket = if hours > 24 * 31 { 86_400 } else { 1800 };
    Ok(Json(state.weather.history(db::now() - hours * 3600, bucket).await?))
}

#[derive(Deserialize)]
pub struct PlaceInput {
    /// "Labège, Haute-Garonne"; empty: the city of the first installation.
    pub place: String,
}

/// Sets the city the weather is for, and reads it at once.
pub async fn set_place(State(state): State<AppState>, _admin: AdminUser, Json(input): Json<PlaceInput>) -> ApiResult<StatusCode> {
    let place = input.place.trim();
    if place.len() > 120 {
        return Err(ApiError::bad_request("place too long"));
    }
    db::set_setting(&state.db, "weather.place", place).await?;
    state.weather.refresh().await;
    Ok(StatusCode::NO_CONTENT)
}
