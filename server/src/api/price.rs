//! The price of electricity of an installation: its contract, its tariffs, and the price now.

use crate::access::Access;
use crate::auth::AuthUser;
use crate::db;
use crate::error::{ApiError, ApiResult};
use crate::price::{self, Contract, DayColor, Period, PriceAt, Tariff, Tariffs};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
async fn versions(state: &AppState, id: i64) -> ApiResult<Vec<TariffVersion>> {
    Ok(db::tariffs(&state.db, id).await?.into_iter().filter_map(|(valid_from, json)| serde_json::from_str(&json).ok().map(|tariff| TariffVersion { valid_from, tariff })).collect())
}

async fn view(state: &AppState, access: &Access, id: i64) -> ApiResult<TariffView> {
    Ok(TariffView { contract: db::contract(&state.db, id).await?, versions: versions(state, id).await?, can_edit: access.manages(id) })
}

pub async fn get(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<Json<TariffView>> {
    let access = Access::of(&state.db, &user).await?;
    access.view(id)?;
    Ok(Json(view(&state, &access, id).await?))
}

/// The administrators of the installation.
async fn manager(state: &AppState, user: &AuthUser, id: i64) -> ApiResult<Access> {
    let access = Access::of(&state.db, user).await?;
    access.manage(id)?;
    Ok(access)
}

/// The colours of the Tempo days, read soon after a tariff starts following them.
fn read_colors(state: &AppState) {
    let state = state.clone();
    tokio::spawn(async move {
        if let Err(e) = crate::tempo::refresh(&state).await {
            tracing::warn!("Tempo colours not read: {e:#}");
        }
    });
}

pub async fn save_contract(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>, Json(contract): Json<Contract>) -> ApiResult<Json<TariffView>> {
    let access = manager(&state, &user, id).await?;
    price::validate_contract(&contract).map_err(ApiError::bad_request)?;
    // A regulated offer takes the prices of the CRE: read before anything is kept.
    if crate::cre::is_regulated(&contract.offer) {
        crate::cre::sync_contract(&state, id, &contract).await.map_err(|e| {
            ApiError::bad_gateway(crate::text!("Prix de la CRE illisibles : {0}", "Prices of the CRE not read: {0}", "Precios de la CRE no leídos: {0}", format!("{e:#}")))
        })?;
    }
    db::set_contract(&state.db, id, &contract).await?;
    read_colors(&state);
    Ok(Json(view(&state, &access, id).await?))
}

/// The tariff from a day on (a new one, or the one of that day changed).
pub async fn save_tariff(State(state): State<AppState>, user: AuthUser, Path((id, from)): Path<(i64, String)>, Json(tariff): Json<Tariff>) -> ApiResult<Json<TariffView>> {
    let access = manager(&state, &user, id).await?;
    day(&from)?;
    price::validate(&tariff).map_err(ApiError::bad_request)?;
    let json = serde_json::to_string(&tariff).map_err(|e| ApiError::internal(e.to_string()))?;
    db::set_tariff(&state.db, id, &from, &json).await?;
    read_colors(&state);
    Ok(Json(view(&state, &access, id).await?))
}

pub async fn delete_tariff(State(state): State<AppState>, user: AuthUser, Path((id, from)): Path<(i64, String)>) -> ApiResult<Json<TariffView>> {
    let access = manager(&state, &user, id).await?;
    if !db::delete_tariff(&state.db, id, &from).await? {
        return Err(ApiError::not_found("no tariff from that day"));
    }
    Ok(Json(view(&state, &access, id).await?))
}

/// The tariffs of an installation as the computations use them (`None`: the home, the first
/// installation of this server; no installation, no tariff).
pub async fn tariffs(state: &AppState, installation_id: Option<i64>) -> ApiResult<Tariffs> {
    let Some(id) = (match installation_id {
        Some(id) => Some(id),
        None => db::home_installation(&state.db).await?,
    }) else {
        return Ok(Tariffs::default());
    };
    let mut list = vec![];
    for v in versions(state, id).await? {
        list.push((day(&v.valid_from)?, v.tariff));
    }
    Ok(Tariffs::new(list))
}

/// The colours announced from `from` to `until`, for the prices of those days.
pub async fn colors(state: &AppState, from: NaiveDate, until: NaiveDate) -> ApiResult<HashMap<NaiveDate, DayColor>> {
    Ok(crate::tempo::colors(&state.db, from, until).await?)
}

#[derive(Deserialize)]
pub struct PriceQuery {
    /// How far ahead the periods go.
    #[serde(default = "default_hours")]
    pub hours: i64,
    /// The periods of the whole local day instead (midnight to midnight), the hours gone included.
    #[serde(default)]
    pub today: bool,
}

/// The start of a local day (the first moment after midnight when the clocks skip it).
fn day_start(day: NaiveDate, tz: chrono_tz::Tz) -> i64 {
    use chrono::TimeZone;
    let midnight = day.and_time(chrono::NaiveTime::MIN);
    tz.from_local_datetime(&midnight).earliest().or_else(|| tz.from_local_datetime(&(midnight + chrono::Duration::hours(1))).earliest()).map_or(0, |t| t.timestamp())
}

fn default_hours() -> i64 {
    24
}

/// The Tempo days, when a tariff follows their colour.
#[derive(Serialize)]
pub struct TempoView {
    pub today: Option<DayColor>,
    /// Announced in the morning (`None` before).
    pub tomorrow: Option<DayColor>,
    /// Red and white days left in the season (1 September to 31 August).
    pub red_left: i64,
    pub white_left: i64,
}

#[derive(Serialize)]
pub struct PriceView {
    pub currency: String,
    /// `None`: no tariff for now.
    pub now: Option<PriceAt>,
    /// From now on, each stretch at one price.
    pub periods: Vec<Period>,
    /// `None`: no tariff follows the colour of the days.
    pub tempo: Option<TempoView>,
}

/// The price of electricity now, and over the next hours.
pub async fn now(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>, Query(q): Query<PriceQuery>) -> ApiResult<Json<PriceView>> {
    Access::of(&state.db, &user).await?.view(id)?;
    let tz = state.cfg.timezone;
    let tariffs = tariffs(&state, Some(id)).await?;
    let now = db::now();
    let today = crate::rollup::local_day(now, tz);
    let start = crate::tempo::season_start(today);
    let known = colors(&state, start.min(today - chrono::Days::new(1)), today + chrono::Days::new(8)).await?;
    let color = |d: NaiveDate| known.get(&d).copied();
    let currency = db::contract(&state.db, id).await?.map_or_else(|| "EUR".into(), |c| c.currency);
    let followed = crate::tempo::followed(&versions(&state, id).await?.into_iter().map(|v| v.tariff).collect::<Vec<_>>());
    let used = |c: DayColor| known.iter().filter(|(d, k)| **k == c && **d >= start && **d <= today).count() as i64;
    Ok(Json(PriceView {
        currency,
        now: price::price_at(&tariffs, &color, tz, now),
        periods: if q.today {
            price::periods(&tariffs, &color, tz, day_start(today, tz), day_start(today + chrono::Days::new(1), tz))
        } else {
            price::periods(&tariffs, &color, tz, now, now + q.hours.clamp(1, 7 * 24) * 3600)
        },
        tempo: followed.then(|| TempoView {
            today: color(today),
            tomorrow: color(today + chrono::Days::new(1)),
            red_left: (crate::tempo::RED_DAYS - used(DayColor::Red)).max(0),
            white_left: (crate::tempo::WHITE_DAYS - used(DayColor::White)).max(0),
        }),
    }))
}
