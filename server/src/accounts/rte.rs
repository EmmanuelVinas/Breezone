//! RTE's data portal (data.rte-france.com): the official colour of the Tempo days, with the
//! "Tempo Like Supply Contract" API. One creates an application on the portal, subscribes it to
//! that API and gives Breezone its client id and secret (OAuth client credentials).
//!
//!   POST {rte}/token/oauth/  (Basic client_id:client_secret)          → {access_token, expires_in}
//!   GET  {rte}/open_api/tempo_like_supply_contract/v1/tempo_like_calendars?start_date&end_date
//!        → {tempo_like_calendars: {values: [{start_date, end_date, value: BLUE|WHITE|RED}]}}

use super::{http, record, refused, unreachable};
use crate::db;
use crate::error::{ApiError, ApiResult};
use crate::price::DayColor;
use crate::AppState;
use chrono::{Days, NaiveDate, TimeZone};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const KIND: &str = "rte";
const SERVICE: &str = "RTE";

#[derive(Serialize, Deserialize)]
pub struct Secret {
    pub client_id: String,
    pub client_secret: String,
}

async fn token(state: &AppState, s: &Secret) -> ApiResult<String> {
    let resp = http()
        .post(format!("{}/token/oauth/", state.cfg.rte_url.trim_end_matches('/')))
        .basic_auth(&s.client_id, Some(&s.client_secret))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send()
        .await
        .map_err(|e| unreachable(SERVICE, e))?;
    if !resp.status().is_success() {
        return Err(refused(SERVICE, resp).await);
    }
    let body: Value = resp.json().await.map_err(|e| unreachable(SERVICE, e))?;
    body["access_token"].as_str().map(str::to_string).ok_or_else(|| ApiError::bad_gateway("RTE: no access token"))
}

/// Midnight of a day in France, as the API wants it ("2026-11-18T00:00:00+01:00").
fn paris(day: NaiveDate) -> String {
    let at = chrono_tz::Europe::Paris.from_local_datetime(&day.and_hms_opt(0, 0, 0).unwrap_or_default()).earliest();
    at.map(|t| t.to_rfc3339()).unwrap_or_else(|| format!("{day}T00:00:00+01:00"))
}

fn color(value: &str) -> Option<DayColor> {
    match value {
        "BLUE" => Some(DayColor::Blue),
        "WHITE" => Some(DayColor::White),
        "RED" => Some(DayColor::Red),
        _ => None,
    }
}

/// The colours from `from` to `until` (both included), as RTE published them.
async fn calendar(state: &AppState, s: &Secret, from: NaiveDate, until: NaiveDate) -> ApiResult<Vec<(NaiveDate, DayColor)>> {
    let token = token(state, s).await?;
    let resp = http()
        .get(format!("{}/open_api/tempo_like_supply_contract/v1/tempo_like_calendars", state.cfg.rte_url.trim_end_matches('/')))
        .query(&[("start_date", paris(from)), ("end_date", paris(until + Days::new(1)))])
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| unreachable(SERVICE, e))?;
    if !resp.status().is_success() {
        return Err(refused(SERVICE, resp).await);
    }
    let body: Value = resp.json().await.map_err(|e| unreachable(SERVICE, e))?;
    Ok(body["tempo_like_calendars"]["values"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| {
            let day = NaiveDate::parse_from_str(v["start_date"].as_str()?.get(..10)?, "%Y-%m-%d").ok()?;
            Some((day, color(v["value"].as_str()?)?))
        })
        .collect())
}

async fn store(state: &AppState, days: &[(NaiveDate, DayColor)]) -> ApiResult<()> {
    for (day, c) in days {
        db::set_day_color(&state.db, &day.format("%Y-%m-%d").to_string(), crate::tempo::name(*c)).await?;
    }
    Ok(())
}

/// Connects the account: it must give the colours of the last days.
pub async fn connect(state: &AppState, client_id: &str, client_secret: &str) -> ApiResult<()> {
    let s = Secret { client_id: client_id.trim().to_string(), client_secret: client_secret.trim().to_string() };
    if s.client_id.is_empty() || s.client_secret.is_empty() {
        return Err(ApiError::bad_request("l'ID client et l'ID secret de l'application RTE sont requis"));
    }
    let today = crate::rollup::local_day(db::now(), state.cfg.timezone);
    let days = calendar(state, &s, today - Days::new(7), today + Days::new(1)).await?;
    super::save(state, KIND, &json!({ "client_id": s.client_id }), &s, None).await?;
    store(state, &days).await?;
    db::set_account_state(&state.db, KIND, "ok", None).await?;
    Ok(())
}

/// Reads today's and tomorrow's colours, and the season's when `season` (once a day is enough).
/// `Ok(false)`: no account, the public relay is used instead.
pub async fn refresh(state: &AppState, season: bool) -> ApiResult<bool> {
    let Some((_, s)) = super::load::<Secret>(state, KIND).await? else { return Ok(false) };
    let today = crate::rollup::local_day(db::now(), state.cfg.timezone);
    let from = if season { crate::tempo::season_start(today) } else { today };
    let result = async {
        let days = calendar(state, &s, from, today + Days::new(1)).await?;
        store(state, &days).await
    }
    .await;
    record(state, KIND, result).await?;
    Ok(true)
}

/// Checks the account again.
pub async fn test(state: &AppState) -> ApiResult<()> {
    match refresh(state, false).await? {
        true => Ok(()),
        false => Err(ApiError::bad_request("aucun compte RTE")),
    }
}
