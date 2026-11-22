//! What Enedis measured on the Linky meter, without any hardware (roadmap E33): through Conso API
//! (conso.boris.sh), which holds the Enedis agreement. One gives it the consent of one's meter
//! on its site and gets a personal token; Breezone needs that token and the meter number (PRM).
//!
//!   GET {conso}/api/daily_consumption?prm&start&end      → {interval_reading: [{value: Wh, date}]}
//!   GET {conso}/api/daily_production?prm&start&end       → idem
//!   GET {conso}/api/consumption_load_curve?prm&start&end → {interval_reading: [{value: W, date}]}
//!       (at most 7 days; each date is the END of its half hour, in French time)
//!
//! The days arrive the day after; the half hours too, once their collection is on at Enedis.

use super::{http, record, refused, unreachable};
use crate::db;
use crate::error::{ApiError, ApiResult};
use crate::AppState;
use chrono::{Days, NaiveDate, NaiveDateTime, TimeZone};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

pub const KIND: &str = "linky";
const SERVICE: &str = "Conso API";

#[derive(Serialize, Deserialize)]
pub struct Secret {
    pub token: String,
}

/// The meter number: 14 digits.
fn valid_prm(prm: &str) -> bool {
    prm.len() == 14 && prm.chars().all(|c| c.is_ascii_digit())
}

async fn readings(state: &AppState, token: &str, what: &str, prm: &str, from: NaiveDate, until: NaiveDate) -> ApiResult<Vec<(String, f64)>> {
    let resp = http()
        .get(format!("{}/api/{what}", state.cfg.conso_api_url.trim_end_matches('/')))
        .query(&[("prm", prm.to_string()), ("start", from.format("%Y-%m-%d").to_string()), ("end", until.format("%Y-%m-%d").to_string())])
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| unreachable(SERVICE, e))?;
    if !resp.status().is_success() {
        return Err(refused(SERVICE, resp).await);
    }
    let body: Value = resp.json().await.map_err(|e| unreachable(SERVICE, e))?;
    Ok(body["interval_reading"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let value = r["value"].as_str().and_then(|v| v.parse().ok()).or_else(|| r["value"].as_f64())?;
            Some((r["date"].as_str()?.to_string(), value))
        })
        .collect())
}

/// The start of the half hour ending at `date` ("2026-11-17 00:30:00", French time), as a timestamp.
fn half_hour_start(date: &str) -> Option<i64> {
    let end = NaiveDateTime::parse_from_str(date, "%Y-%m-%d %H:%M:%S").ok()?;
    Some(chrono_tz::Europe::Paris.from_local_datetime(&end).earliest()?.timestamp() - 1800)
}

/// Reads the days from `from` to yesterday, and the half hours of the last week.
async fn read(state: &AppState, token: &str, prm: &str, from: NaiveDate) -> ApiResult<usize> {
    let today = crate::rollup::local_day(db::now(), state.cfg.timezone);
    let days = readings(state, token, "daily_consumption", prm, from, today).await?;
    for (day, wh) in &days {
        db::set_linky_day(&state.db, prm, day, Some(*wh), None).await?;
    }
    // Production and half hours are only there when the meter produces and the collection is on.
    if let Ok(made) = readings(state, token, "daily_production", prm, from, today).await {
        for (day, wh) in &made {
            db::set_linky_day(&state.db, prm, day, None, Some(*wh)).await?;
        }
    }
    if let Ok(curve) = readings(state, token, "consumption_load_curve", prm, today - Days::new(7), today).await {
        let points: Vec<(i64, f64)> = curve.iter().filter_map(|(d, w)| Some((half_hour_start(d)?, *w))).collect();
        db::set_linky_curve(&state.db, prm, &points).await?;
    }
    Ok(days.len())
}

/// Connects the account: the meter must answer with its last days.
pub async fn connect(state: &AppState, token: &str, prm: &str) -> ApiResult<()> {
    let (token, prm) = (token.trim(), prm.trim().replace(' ', ""));
    if token.is_empty() || !valid_prm(&prm) {
        return Err(ApiError::bad_request("le jeton Conso API et le numéro du compteur (PRM, 14 chiffres) sont requis"));
    }
    let today = crate::rollup::local_day(db::now(), state.cfg.timezone);
    // A year at once (one call): the history and its costs are there from the start.
    read(state, token, &prm, today - Days::new(365)).await?;
    super::save(state, KIND, &json!({ "prm": prm }), &Secret { token: token.to_string() }, None).await?;
    db::set_account_state(&state.db, KIND, "ok", None).await?;
    Ok(())
}

/// Reads the last days again (Enedis publishes them the next morning, sometimes later).
pub async fn refresh(state: &AppState) -> ApiResult<bool> {
    refresh_from(state, 10).await
}

/// Reads the days of the last `back` days and the half hours of the last week through the module
/// that gives `energy.history@1` (`Ok(false)`: its account is not connected).
async fn refresh_module(state: &AppState, back: u64) -> ApiResult<bool> {
    let Some(prm) = db::account(&state.db, KIND).await?.and_then(|a| serde_json::from_str::<Value>(&a.config).ok()?["prm"].as_str().map(str::to_string)) else {
        return Ok(false);
    };
    let today = crate::rollup::local_day(db::now(), state.cfg.timezone);
    let asked = json!({"from": (today - Days::new(back)).to_string(), "until": (today - Days::new(1)).to_string()});
    let days = match crate::modules::ask_service(&state.db, "energy.history@1", "days", &asked).await {
        None => return Ok(false),
        Some(Err(e)) => return Err(ApiError::bad_gateway(e)),
        Some(Ok(list)) => list,
    };
    let wh = |v: &Value| v.as_f64().map(|kwh| kwh * 1000.0);
    for d in days.as_array().into_iter().flatten() {
        if let Some(day) = d["day"].as_str() {
            db::set_linky_day(&state.db, &prm, day, wh(&d["consumption"]), wh(&d["production"])).await?;
        }
    }
    for back in 1..=7 {
        let asked = json!({"day": (today - Days::new(back)).to_string()});
        if let Some(Ok(curve)) = crate::modules::ask_service(&state.db, "energy.history@1", "curve", &asked).await {
            let points: Vec<(i64, f64)> = curve.as_array().into_iter().flatten().filter_map(|p| Some((p["ts"].as_i64()?, p["power"].as_f64()?))).collect();
            db::set_linky_curve(&state.db, &prm, &points).await?;
        }
    }
    Ok(true)
}

/// Reads the days of the last `back` days again (a year once connected).
pub async fn refresh_from(state: &AppState, back: u64) -> ApiResult<bool> {
    if super::is_module_account(KIND) {
        return refresh_module(state, back).await;
    }
    let Some((config, s)) = super::load::<Secret>(state, KIND).await? else { return Ok(false) };
    let Some(prm) = config["prm"].as_str().map(str::to_string) else { return Ok(false) };
    let today = crate::rollup::local_day(db::now(), state.cfg.timezone);
    record(state, KIND, read(state, &s.token, &prm, today - Days::new(back)).await).await?;
    Ok(true)
}

pub async fn test(state: &AppState) -> ApiResult<()> {
    match refresh(state).await? {
        true => Ok(()),
        false => Err(ApiError::bad_request("aucun compte Linky")),
    }
}

/// Every six hours while an account is linked.
pub async fn run_loop(state: AppState) {
    loop {
        if let Err(e) = refresh(&state).await {
            tracing::warn!("Linky not read: {}", e.message.en);
        }
        tokio::time::sleep(Duration::from_secs(6 * 3600)).await;
    }
}

/// A day as Enedis measured it, and what the modules measured that day.
#[derive(Debug, Serialize)]
pub struct LinkyDay {
    pub day: String,
    /// kWh drawn from the grid, as the meter counted them.
    pub consumption: Option<f64>,
    /// kWh returned to the grid.
    pub production: Option<f64>,
    /// What it cost at the tariff: each half hour at its price when the curve is known.
    pub cost: Option<f64>,
    /// The cost comes from the day's total at one price (no half hours for that day).
    pub estimated: bool,
    /// kWh drawn from the grid as the modules of Breezone measured them (a grid meter), if kept.
    pub measured: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct LinkyView {
    pub prm: Option<String>,
    pub days: Vec<LinkyDay>,
}

/// The last `n` days (oldest first), with their cost at the tariff of `installation_id`.
pub async fn view(state: &AppState, n: u64, installation_id: Option<i64>) -> ApiResult<LinkyView> {
    let Some(prm) = db::account(&state.db, KIND).await?.and_then(|a| serde_json::from_str::<Value>(&a.config).ok()?["prm"].as_str().map(str::to_string)) else {
        return Ok(LinkyView { prm: None, days: vec![] });
    };
    let tz = state.cfg.timezone;
    let today = crate::rollup::local_day(db::now(), tz);
    let first = today - Days::new(n.clamp(1, 366));
    let label = |d: NaiveDate| d.format("%Y-%m-%d").to_string();
    let rows = db::linky_days(&state.db, &prm, &label(first), &label(today)).await?;
    let tariffs = crate::api::price::tariffs(state, installation_id).await?;
    let known = crate::api::price::colors(state, first - Days::new(1), today).await?;
    let color = |d: NaiveDate| known.get(&d).copied();
    let measured: std::collections::HashMap<String, f64> = sqlx::query_as::<_, (String, String)>("SELECT day, data FROM energy_days WHERE day >= ? AND day <= ?")
        .bind(label(first))
        .bind(label(today))
        .fetch_all(&state.db)
        .await?
        .into_iter()
        .filter_map(|(day, data)| Some((day, serde_json::from_str::<Value>(&data).ok()?["imported"].as_f64()?)))
        .collect();
    let mut days = vec![];
    for (day, consumption, production) in rows {
        let Ok(date) = NaiveDate::parse_from_str(&day, "%Y-%m-%d") else { continue };
        let midnight = |d: NaiveDate| tz.from_local_datetime(&d.and_hms_opt(0, 0, 0).unwrap_or_default()).earliest().map_or(0, |t| t.timestamp());
        let curve = db::linky_curve(&state.db, &prm, midnight(date), midnight(date + Days::new(1))).await?;
        let (cost, estimated) = if curve.len() >= 40 {
            let points: Vec<crate::energy::CurvePoint> =
                curve.iter().map(|(ts, w)| crate::energy::CurvePoint { ts: *ts, flows: crate::energy::Flows { grid: Some(*w), ..Default::default() } }).collect();
            (crate::price::day_cost(&tariffs, &color, tz, date, &points, 1800).map(|c| c.energy + c.subscription), false)
        } else {
            // The whole day at the price of its middle: an estimate, said so.
            let at = crate::price::price_at(&tariffs, &color, tz, midnight(date) + 12 * 3600);
            let sub = crate::price::day_cost(&tariffs, &color, tz, date, &[], 1800).map(|c| c.subscription);
            (at.zip(consumption).map(|(p, wh)| wh / 1000.0 * p.price + sub.unwrap_or(0.0)), true)
        };
        let kwh = |wh: Option<f64>| wh.map(|v| (v / 10.0).round() / 100.0);
        days.push(LinkyDay {
            consumption: kwh(consumption),
            production: kwh(production),
            cost: cost.map(|c| (c * 100.0).round() / 100.0),
            estimated,
            measured: measured.get(&day).map(|v| (v * 100.0).round() / 100.0),
            day,
        });
    }
    Ok(LinkyView { prm: Some(prm), days })
}
