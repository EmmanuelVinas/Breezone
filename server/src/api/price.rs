//! The price of electricity: the contract of the home, its tariffs, and the price now.

use crate::auth::{AdminUser, AuthUser};
use crate::db;
use crate::error::{ApiError, ApiResult};
use crate::price::{self, Contract, DayColor, Period, PriceAt, Tariff, Tariffs};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct TariffVersion {
    /// The local day it applies from ("YYYY-MM-DD").
    pub valid_from: String,
    pub tariff: Tariff,
}

#[derive(Serialize)]
pub struct TariffView {
    /// `None`: not set yet.
    pub contract: Option<Contract>,
    /// Oldest first.
    pub versions: Vec<TariffVersion>,
    /// This user may change them.
    pub can_edit: bool,
}

fn day(text: &str) -> ApiResult<NaiveDate> {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").map_err(|_| ApiError::bad_request("date: YYYY-MM-DD expected"))
}

/// The stored tariffs this version reads (one written by a newer version is left out).
async fn versions(state: &AppState) -> ApiResult<Vec<TariffVersion>> {
    Ok(db::tariffs(&state.db).await?.into_iter().filter_map(|(valid_from, json)| serde_json::from_str(&json).ok().map(|tariff| TariffVersion { valid_from, tariff })).collect())
}

async fn view(state: &AppState, user: &AuthUser) -> ApiResult<TariffView> {
    Ok(TariffView { contract: db::contract(&state.db).await?, versions: versions(state).await?, can_edit: user.is_admin() })
}

pub async fn get(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<TariffView>> {
    Ok(Json(view(&state, &user).await?))
}

pub async fn save_contract(State(state): State<AppState>, AdminUser(user): AdminUser, Json(contract): Json<Contract>) -> ApiResult<Json<TariffView>> {
    price::validate_contract(&contract).map_err(ApiError::bad_request)?;
    db::set_contract(&state.db, &contract).await?;
    Ok(Json(view(&state, &user).await?))
}

/// The tariff from a day on (a new one, or the one of that day changed).
pub async fn save_tariff(State(state): State<AppState>, AdminUser(user): AdminUser, Path(from): Path<String>, Json(tariff): Json<Tariff>) -> ApiResult<Json<TariffView>> {
    day(&from)?;
    price::validate(&tariff).map_err(ApiError::bad_request)?;
    let json = serde_json::to_string(&tariff).map_err(|e| ApiError::internal(e.to_string()))?;
    db::set_tariff(&state.db, &from, &json).await?;
    Ok(Json(view(&state, &user).await?))
}

pub async fn delete_tariff(State(state): State<AppState>, AdminUser(user): AdminUser, Path(from): Path<String>) -> ApiResult<Json<TariffView>> {
    if !db::delete_tariff(&state.db, &from).await? {
        return Err(ApiError::not_found("no tariff from that day"));
    }
    Ok(Json(view(&state, &user).await?))
}

/// The tariffs as the computations use them.
pub async fn tariffs(state: &AppState) -> ApiResult<Tariffs> {
    let mut list = vec![];
    for v in versions(state).await? {
        list.push((day(&v.valid_from)?, v.tariff));
    }
    Ok(Tariffs::new(list))
}

/// The colour announced for a day: none until a source of them is set (roadmap E32, Tempo).
pub fn no_color(_: NaiveDate) -> Option<DayColor> {
    None
}

#[derive(Deserialize)]
pub struct PriceQuery {
    /// How far ahead the periods go.
    #[serde(default = "default_hours")]
    pub hours: i64,
}

fn default_hours() -> i64 {
    24
}

#[derive(Serialize)]
pub struct PriceView {
    pub currency: String,
    /// `None`: no tariff for now.
    pub now: Option<PriceAt>,
    /// From now on, each stretch at one price.
    pub periods: Vec<Period>,
}

/// The price of electricity now, and over the next hours.
pub async fn now(State(state): State<AppState>, _user: AuthUser, Query(q): Query<PriceQuery>) -> ApiResult<Json<PriceView>> {
    let tz = state.cfg.timezone;
    let tariffs = tariffs(&state).await?;
    let now = db::now();
    let currency = db::contract(&state.db).await?.map_or_else(|| "EUR".into(), |c| c.currency);
    Ok(Json(PriceView {
        currency,
        now: price::price_at(&tariffs, &no_color, tz, now),
        periods: price::periods(&tariffs, &no_color, tz, now, now + q.hours.clamp(1, 7 * 24) * 3600),
    }))
}
