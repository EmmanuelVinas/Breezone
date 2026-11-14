//! The energy of the home (energy page), and the history of each device.

use super::home::{devices_of, DeviceView};
use crate::access::Access;
use crate::auth::AuthUser;
use crate::db;
use crate::energy::{self, Consumer, DayEnergy, Live, Measure, Role};
use crate::error::{ApiError, ApiResult};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::{Days, NaiveDate};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// Every point of the history stands for this long at most (then the device was not read).
fn max_span(state: &AppState) -> i64 {
    state.cfg.history_interval_secs * 2
}

fn is_inverter(d: &DeviceView) -> bool {
    d.traits.iter().any(|t| t == "solar.inverter@1")
}

/// The role a measure has on the energy page. A solar inverter is a production without being told;
/// a measure of a solar inverter (a meter on its line) is a production whatever role it was given.
fn effective_role(d: &DeviceView, by_id: &HashMap<i64, &DeviceView>) -> Option<Role> {
    let of_inverter = d.measures.device_id.and_then(|id| by_id.get(&id)).is_some_and(|t| is_inverter(t));
    match d.role.as_deref().and_then(Role::parse) {
        Some(Role::Consumption) if of_inverter => Some(Role::Solar),
        Some(role) => Some(role),
        None if is_inverter(d) => Some(Role::Solar),
        None => None,
    }
}

/// The energy measures of the home, with what each consumption measures. An inverter measured by
/// another production measure counts once, through that measure (its meter is read more often).
/// A production measure that does not say what it measures may measure the inverters too: they are
/// then left out, unless they were explicitly given the role of a production (another installation).
async fn measures(state: &AppState, devices: &[DeviceView]) -> ApiResult<Vec<Measure>> {
    let rooms: HashMap<i64, String> = db::list_rooms(&state.db).await?.into_iter().map(|r| (r.id, r.name)).collect();
    let names: HashMap<i64, &str> = devices.iter().map(|d| (d.id, d.name.as_str())).collect();
    let by_id: HashMap<i64, &DeviceView> = devices.iter().map(|d| (d.id, d)).collect();
    let covered: Vec<i64> = devices
        .iter()
        .filter(|d| effective_role(d, &by_id) == Some(Role::Solar))
        .filter_map(|d| d.measures.device_id.filter(|t| *t != d.id && by_id.get(t).is_some_and(|t| is_inverter(t))))
        .collect();
    let meter_of_all = devices
        .iter()
        .any(|d| !is_inverter(d) && d.measures.device_id.is_none() && d.traits.iter().any(|t| t == "energy.power@1") && effective_role(d, &by_id) == Some(Role::Solar));
    Ok(devices
        .iter()
        .filter(|d| d.traits.iter().any(|t| t == "energy.power@1"))
        .filter(|d| !covered.contains(&d.id))
        .filter(|d| !(meter_of_all && is_inverter(d) && d.role.is_none()))
        .filter_map(|d| {
            let role = effective_role(d, &by_id)?;
            let consumer = (role == Role::Consumption).then(|| {
                let target = d.measures.device_id.and_then(|id| names.get(&id).map(|n| n.to_string()));
                let room = d.measures.room_id.and_then(|id| rooms.get(&id).cloned());
                Consumer { name: target.or(room).unwrap_or_else(|| d.name.clone()), device_id: d.measures.device_id, room_id: d.measures.room_id }
            });
            Some(Measure { device_id: d.id, role, consumer })
        })
        .collect())
}

#[derive(Serialize)]
pub struct Roles {
    pub grid: bool,
    pub solar: bool,
    pub battery: bool,
    pub consumption: bool,
}

fn roles(measures: &[Measure]) -> Roles {
    let has = |r| measures.iter().any(|m| m.role == r);
    Roles { grid: has(Role::Grid), solar: has(Role::Solar), battery: has(Role::Battery), consumption: has(Role::Consumption) }
}

#[derive(Serialize)]
pub struct LiveView {
    /// The roles the home has a measure of: the page shows only their blocks.
    pub roles: Roles,
    #[serde(flatten)]
    pub live: Live,
}

/// The home now, from the last reading of its measures (every few seconds).
pub async fn live(State(state): State<AppState>, _user: AuthUser) -> ApiResult<Json<LiveView>> {
    let devices = devices_of(&state).await?;
    let measures = measures(&state, &devices).await?;
    let power: HashMap<i64, f64> = devices.iter().filter(|d| d.available).filter_map(|d| Some((d.id, d.state.get("power")?.as_f64()?))).collect();
    let live = energy::live(&measures, |id| power.get(&id).copied());
    Ok(Json(LiveView { roles: roles(&measures), live }))
}

/// Midnight of a local day, as a timestamp.
fn midnight(day: NaiveDate, tz: Tz) -> i64 {
    day.and_hms_opt(0, 0, 0).and_then(|t| t.and_local_timezone(tz).earliest()).map_or(0, |t| t.timestamp())
}

fn today(tz: Tz) -> NaiveDate {
    chrono::Utc::now().with_timezone(&tz).date_naive()
}

/// The spans of the measures from `since` to `until`.
async fn spans(state: &AppState, measures: &[Measure], since: i64, until: i64) -> ApiResult<Vec<energy::Span>> {
    let ids: Vec<i64> = measures.iter().map(|m| m.device_id).collect();
    // From one point earlier: the first span of the period starts before it.
    let samples = db::device_samples(&state.db, &ids, energy::FIELDS, since - max_span(state), until).await?;
    Ok(energy::spans(&samples, max_span(state)))
}

#[derive(Deserialize)]
pub struct DayQuery {
    /// "YYYY-MM-DD", today by default.
    pub date: Option<String>,
}

/// Length of a point of the curve of a day.
const BUCKET: i64 = 900;

#[derive(Serialize)]
pub struct DayView {
    pub date: String,
    /// Seconds per point.
    pub bucket: i64,
    pub points: Vec<energy::CurvePoint>,
    pub totals: DayEnergy,
}

/// The power of the home along a local day (a point every 15 minutes), and its energy.
pub async fn day(State(state): State<AppState>, _user: AuthUser, Query(q): Query<DayQuery>) -> ApiResult<Json<DayView>> {
    let tz = state.cfg.timezone;
    let date = match q.date.as_deref() {
        Some(d) => NaiveDate::parse_from_str(d, "%Y-%m-%d").map_err(|_| ApiError::bad_request("date: YYYY-MM-DD expected"))?,
        None => today(tz),
    };
    let from = midnight(date, tz);
    let until = midnight(date + Days::new(1), tz);
    let devices = devices_of(&state).await?;
    let measures = measures(&state, &devices).await?;
    let spans = spans(&state, &measures, from, until).await?;
    let day = date.format("%Y-%m-%d").to_string();
    let count = ((until - from) / BUCKET).max(1) as usize;
    Ok(Json(DayView {
        points: energy::curve(&measures, &spans, from, BUCKET, count),
        totals: energy::daily(&measures, &spans, tz, std::slice::from_ref(&day)).remove(0),
        date: day,
        bucket: BUCKET,
    }))
}

#[derive(Deserialize)]
pub struct DaysQuery {
    #[serde(default = "default_days")]
    pub days: u64,
}

fn default_days() -> u64 {
    30
}

/// The energy of the home day by day (local days, oldest first, today included).
pub async fn days(State(state): State<AppState>, _user: AuthUser, Query(q): Query<DaysQuery>) -> ApiResult<Json<Vec<DayEnergy>>> {
    let tz = state.cfg.timezone;
    let n = q.days.clamp(1, 366);
    let last = today(tz);
    let first = last - Days::new(n - 1);
    let days: Vec<String> = (0..n).map(|i| (first + Days::new(i)).format("%Y-%m-%d").to_string()).collect();
    let devices = devices_of(&state).await?;
    let measures = measures(&state, &devices).await?;
    let spans = spans(&state, &measures, midnight(first, tz), midnight(last + Days::new(1), tz)).await?;
    Ok(Json(energy::daily(&measures, &spans, tz, &days)))
}

#[derive(Deserialize)]
pub struct HistoryQuery {
    #[serde(default = "default_hours")]
    pub hours: i64,
    #[serde(default = "default_points")]
    pub points: i64,
    /// A period instead of the last hours (timestamps), e.g. a local day.
    pub from: Option<i64>,
    pub until: Option<i64>,
    /// Seconds per point (default: the period divided by `points`).
    pub bucket: Option<i64>,
}

fn default_hours() -> i64 {
    24
}

fn default_points() -> i64 {
    96
}

#[derive(Serialize)]
pub struct HistoryView {
    /// Seconds per point.
    pub bucket: i64,
    /// Per field of the state: `[start of the bucket, value]`, the mean over the bucket (the last
    /// value for a counter). Buckets without a reading are left out.
    pub series: BTreeMap<String, Vec<(i64, f64)>>,
}

/// The history of a device: every number of its state, over the last hours or a given period.
pub async fn device_history(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>, Query(q): Query<HistoryQuery>) -> ApiResult<Json<HistoryView>> {
    let device = db::device_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("device"))?;
    if let Some(inst) = device.installation_id {
        Access::of(&state.db, &user).await?.view(inst)?;
    }
    let (from, until, bucket) = match (q.from, q.until) {
        (Some(from), Some(until)) => {
            if until <= from {
                return Err(ApiError::bad_request("until must come after from"));
            }
            let span = (until - from).min(31 * 86_400);
            let bucket = q.bucket.unwrap_or(span / q.points.clamp(6, 500)).clamp(60, span);
            (from, from + span, bucket)
        }
        (None, None) => {
            let span = q.hours.clamp(1, 24 * 31) * 3600;
            let bucket = q.bucket.unwrap_or(span / q.points.clamp(6, 500)).max(60);
            let now = db::now();
            ((now - span).div_euclid(bucket) * bucket, now + 1, bucket)
        }
        _ => return Err(ApiError::bad_request("from and until go together")),
    };
    let samples = db::device_samples(&state.db, &[id], &[], from, until).await?;
    // Per field and bucket: (sum, count, last).
    let mut acc: BTreeMap<String, BTreeMap<i64, (f64, u32, f64)>> = BTreeMap::new();
    for s in samples {
        let b = from + (s.ts - from).div_euclid(bucket) * bucket;
        let e = acc.entry(s.field).or_default().entry(b).or_insert((0.0, 0, 0.0));
        e.0 += s.value;
        e.1 += 1;
        e.2 = s.value;
    }
    let series = acc
        .into_iter()
        .map(|(field, buckets)| {
            let counter = crate::traits::is_counter(&field);
            let points = buckets.into_iter().map(|(b, (sum, n, last))| (b, if counter { last } else { (sum / n as f64 * 100.0).round() / 100.0 })).collect();
            (field, points)
        })
        .collect();
    Ok(Json(HistoryView { bucket, series }))
}
