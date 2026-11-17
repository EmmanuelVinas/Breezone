//! The energy of the home (energy page), and the history of each device.

use crate::access::Access;
use crate::auth::AuthUser;
use crate::db;
use crate::devices::{self, DeviceView};
use crate::energy::{self, DayEnergy, Measure};
use crate::error::{ApiError, ApiResult};
use crate::rollup::midnight;
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::{Days, NaiveDate};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// The energy of the home: for the users given its measures (type "energy" or "solar"), or all the
/// devices of the modules.
pub(crate) async fn reads_energy(state: &AppState, user: &AuthUser) -> ApiResult<()> {
    let access = Access::of(&state.db, user).await?;
    if access.sees_type("energy") || access.sees_type("solar") {
        Ok(())
    } else {
        Err(ApiError::forbidden("vous n'avez pas accès à l'énergie de la maison"))
    }
}

/// Every point of the history stands for this long at most (then the device was not read).
fn max_span(state: &AppState) -> i64 {
    state.cfg.history_interval_secs * 2
}

/// The energy measures of the home (see [`energy::measures`]).
async fn measures(state: &AppState, devices: &[DeviceView]) -> ApiResult<Vec<Measure>> {
    let rooms: HashMap<i64, String> = db::list_rooms(&state.db).await?.into_iter().map(|r| (r.id, r.name)).collect();
    Ok(energy::measures(devices, &rooms))
}

/// The home now, from the last reading of its measures (every few seconds).
pub async fn live(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<energy::LiveView>> {
    reads_energy(&state, &user).await?;
    let devices = devices::list(&state).await?;
    Ok(Json(energy::live_of(&state.db, &devices).await?))
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
    /// Whose prices the costs are at (the home by default).
    pub installation_id: Option<i64>,
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
pub async fn day(State(state): State<AppState>, user: AuthUser, Query(q): Query<DayQuery>) -> ApiResult<Json<DayView>> {
    reads_energy(&state, &user).await?;
    let tz = state.cfg.timezone;
    let date = match q.date.as_deref() {
        Some(d) => NaiveDate::parse_from_str(d, "%Y-%m-%d").map_err(|_| ApiError::bad_request("date: YYYY-MM-DD expected"))?,
        None => today(tz),
    };
    let from = midnight(date, tz);
    let until = midnight(date + Days::new(1), tz);
    let devices = devices::list(&state).await?;
    let measures = measures(&state, &devices).await?;
    let spans = spans(&state, &measures, from, until).await?;
    let day = date.format("%Y-%m-%d").to_string();
    let count = ((until - from) / BUCKET).max(1) as usize;
    let points = energy::curve(&measures, &spans, from, BUCKET, count);
    let mut totals = energy::daily(&measures, &spans, tz, std::slice::from_ref(&day)).remove(0);
    let tariffs = crate::api::price::tariffs(&state, q.installation_id).await?;
    let known = crate::api::price::colors(&state, date - Days::new(1), date).await?;
    totals.cost = crate::price::day_cost(&tariffs, &|d| known.get(&d).copied(), tz, date, &points, BUCKET);
    Ok(Json(DayView { points, totals, date: day, bucket: BUCKET }))
}

#[derive(Deserialize)]
pub struct DaysQuery {
    #[serde(default = "default_days")]
    pub days: u64,
    /// Whose prices the costs are at (the home by default).
    pub installation_id: Option<i64>,
}

fn default_days() -> u64 {
    30
}

/// The energy of the home day by day (local days, oldest first, today included). The days whose
/// detail is still kept are computed (and kept in `energy_days`); older ones come from what was
/// kept, so that the page goes back years.
pub async fn days(State(state): State<AppState>, user: AuthUser, Query(q): Query<DaysQuery>) -> ApiResult<Json<Vec<DayEnergy>>> {
    reads_energy(&state, &user).await?;
    let tz = state.cfg.timezone;
    let n = q.days.clamp(1, 5 * 366);
    let last = today(tz);
    let first = last - Days::new(n - 1);
    // The first day whose detail is complete: computed from it on.
    let detail = crate::rollup::local_day(db::now() - state.cfg.history_retention_days * 86_400, tz) + Days::new(1);
    let computed_from = first.max(detail);
    let label = |d: NaiveDate| d.format("%Y-%m-%d").to_string();
    let kept: HashMap<String, DayEnergy> = if first < computed_from {
        sqlx::query_as::<_, (String, String)>("SELECT day, data FROM energy_days WHERE day >= ? AND day < ?")
            .bind(label(first))
            .bind(label(computed_from))
            .fetch_all(&state.db)
            .await?
            .into_iter()
            .filter_map(|(day, data)| Some((day, serde_json::from_str(&data).ok()?)))
            .collect()
    } else {
        HashMap::new()
    };
    let mut out: Vec<DayEnergy> = (0..(computed_from - first).num_days().max(0) as u64)
        .map(|i| label(first + Days::new(i)))
        .map(|day| kept.get(&day).cloned().unwrap_or(DayEnergy { day, ..Default::default() }))
        .collect();
    if computed_from <= last {
        let days: Vec<String> = (0..=(last - computed_from).num_days() as u64).map(|i| label(computed_from + Days::new(i))).collect();
        let devices = devices::list(&state).await?;
        let measures = measures(&state, &devices).await?;
        let spans = spans(&state, &measures, midnight(computed_from, tz), midnight(last + Days::new(1), tz)).await?;
        let mut computed = energy::daily(&measures, &spans, tz, &days);
        // Each day at the prices of its moments.
        let tariffs = crate::api::price::tariffs(&state, q.installation_id).await?;
        let known = crate::api::price::colors(&state, computed_from - Days::new(1), last).await?;
        let color = |d: NaiveDate| known.get(&d).copied();
        for d in computed.iter_mut() {
            let Ok(date) = NaiveDate::parse_from_str(&d.day, "%Y-%m-%d") else { continue };
            let (from, until) = (midnight(date, tz), midnight(date + Days::new(1), tz));
            let points = energy::curve(&measures, &spans, from, BUCKET, ((until - from) / BUCKET).max(1) as usize);
            d.cost = crate::price::day_cost(&tariffs, &color, tz, date, &points, BUCKET);
        }
        // The past days are kept: they stay once their detail is deleted.
        let today_label = label(last);
        let mut tx = state.db.begin().await?;
        for d in computed.iter().filter(|d| d.day != today_label && (d.consumption.is_some() || d.production.is_some() || d.imported.is_some())) {
            sqlx::query("INSERT INTO energy_days (day, data) VALUES (?, ?) ON CONFLICT (day) DO UPDATE SET data = excluded.data")
                .bind(&d.day)
                .bind(serde_json::to_string(d).unwrap_or_default())
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        out.extend(computed);
    }
    Ok(Json(out))
}

#[derive(Serialize)]
pub struct RangeView {
    /// The first local day the home has an energy measure of: the periods offered stop there.
    pub first_day: Option<String>,
}

/// How far back the energy of the home goes.
pub async fn range(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<RangeView>> {
    reads_energy(&state, &user).await?;
    let tz = state.cfg.timezone;
    let devices = devices::list(&state).await?;
    let measures = measures(&state, &devices).await?;
    let mut first: Option<NaiveDate> =
        sqlx::query_scalar::<_, Option<String>>("SELECT MIN(day) FROM energy_days").fetch_one(&state.db).await?.and_then(|d| NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok());
    for m in &measures {
        if let Some(ts) = crate::rollup::device_oldest(&state.db, m.device_id).await? {
            let day = crate::rollup::local_day(ts, tz);
            first = Some(first.map_or(day, |f| f.min(day)));
        }
    }
    Ok(Json(RangeView { first_day: first.map(|d| d.format("%Y-%m-%d").to_string()) }))
}

#[derive(Deserialize)]
pub struct HistoryQuery {
    #[serde(default = "crate::api::default_hours")]
    pub hours: i64,
    #[serde(default = "default_points")]
    pub points: i64,
    /// A period instead of the last hours (timestamps), e.g. a local day.
    pub from: Option<i64>,
    pub until: Option<i64>,
    /// Seconds per point (default: the period divided by `points`).
    pub bucket: Option<i64>,
}

fn default_points() -> i64 {
    96
}

/// The zone an Airzone device stands for: (installation, system, zone).
fn zone_of(device: &db::Device) -> Option<(i64, i64, i64)> {
    let inst = device.installation_id.filter(|_| device.source == "airzone")?;
    match device.source_ref.split('/').collect::<Vec<_>>()[..] {
        [_, system, zone] => Some((inst, system.parse().ok()?, zone.parse().ok()?)),
        _ => None,
    }
}

/// The readings of a zone as the samples of its device (fields of `climate.zone@1`).
async fn zone_samples(db: &db::Db, device_id: i64, (inst, system, zone): (i64, i64, i64), from: i64, until: i64) -> ApiResult<Vec<db::DeviceSample>> {
    let mut out = vec![];
    for r in db::readings(db, inst, system, zone, from).await?.into_iter().filter(|r| r.ts < until) {
        let fields = [("room_temp", r.room_temp), ("humidity", r.humidity.filter(|h| *h > 0).map(|h| h as f64)), ("setpoint", r.setpoint)];
        for (field, value) in fields {
            if let Some(value) = value {
                out.push(db::DeviceSample { device_id, ts: r.ts, field: field.into(), value });
            }
        }
    }
    Ok(out)
}

/// A device with a history, and the fields it has one of.
#[derive(Serialize)]
pub struct Charted {
    pub device_id: i64,
    pub fields: Vec<String>,
}

/// The devices this user sees that have a history, with its fields: what a chart can show.
pub async fn charted(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<Charted>>> {
    let access = Access::of(&state.db, &user).await?;
    let since = db::now() - 3 * 86_400;
    let devices = db::list_devices(&state.db).await?;
    let mut by_device: BTreeMap<i64, Vec<String>> = BTreeMap::new();
    for (id, field) in db::charted_fields(&state.db, since).await? {
        by_device.entry(id).or_default().push(field);
    }
    for (inst, system, zone, temp, humidity, setpoint) in db::charted_zones(&state.db, since).await? {
        let reference = format!("{inst}/{system}/{zone}");
        if let Some(d) = devices.iter().find(|d| d.source == "airzone" && d.source_ref == reference) {
            let fields = [("room_temp", temp), ("humidity", humidity), ("setpoint", setpoint)];
            by_device.entry(d.id).or_default().extend(fields.into_iter().filter(|(_, has)| *has).map(|(f, _)| f.to_string()));
        }
    }
    Ok(Json(
        by_device
            .into_iter()
            .filter(|(id, fields)| !fields.is_empty() && devices.iter().find(|d| d.id == *id).is_some_and(|d| d.installation_id.is_none_or(|i| access.can_view(i))))
            .map(|(device_id, fields)| Charted { device_id, fields })
            .collect(),
    ))
}

#[derive(Serialize)]
pub struct HistoryView {
    /// Seconds per point.
    pub bucket: i64,
    /// Per field of the state: `[start of the bucket, value]`, the mean over the bucket (the last
    /// value for a counter). Buckets without a reading are left out.
    pub series: BTreeMap<String, Vec<(i64, f64)>>,
    /// The oldest point of the device: the periods a client offers stop there.
    pub oldest: Option<i64>,
}

/// The longest period a curve covers: five years and a bit.
const LONGEST: i64 = 5 * 366 * 86_400;

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
            let span = (until - from).min(LONGEST);
            let bucket = q.bucket.unwrap_or(span / q.points.clamp(6, 500)).clamp(60, span);
            (from, from + span, bucket)
        }
        (None, None) => {
            let span = q.hours.clamp(1, LONGEST / 3600) * 3600;
            let bucket = q.bucket.unwrap_or(span / q.points.clamp(6, 500)).max(60);
            let now = db::now();
            // Days start at local midnight, as the daily summaries do.
            let start = if bucket >= 86_400 {
                midnight(chrono::DateTime::from_timestamp(now - span, 0).unwrap_or_default().with_timezone(&state.cfg.timezone).date_naive(), state.cfg.timezone)
            } else {
                (now - span).div_euclid(bucket) * bucket
            };
            (start, now + 1, bucket)
        }
        _ => return Err(ApiError::bad_request("from and until go together")),
    };
    let (samples, oldest) = match zone_of(&device) {
        // An Airzone zone keeps its own history: its temperature, humidity and setpoint.
        Some((inst, system, zone)) => (zone_samples(&state.db, id, (inst, system, zone), from, until).await?, None),
        // The summary that fits the bucket: a curve over years reads days, not every few minutes.
        None => (
            crate::rollup::device_points(&state.db, state.cfg.timezone, id, from, until, crate::rollup::Level::for_bucket(bucket)).await?,
            crate::rollup::device_oldest(&state.db, id).await?,
        ),
    };
    // Per field and bucket: (sum, count, last).
    let mut acc: BTreeMap<String, BTreeMap<i64, (f64, u32, f64)>> = BTreeMap::new();
    for s in samples {
        // Two hours of slack: a local day lasts 23 or 25 hours when the clocks change.
        let slack = if bucket >= 86_400 { 7200 } else { 0 };
        let b = from + (s.ts - from + slack).div_euclid(bucket) * bucket;
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
    Ok(Json(HistoryView { bucket, series, oldest }))
}
