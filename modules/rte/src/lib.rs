//! RTE's data portal (data.rte-france.com): the official colour of the Tempo days
//! (`tariff.day_color@1`), with the "Tempo Like Supply Contract" API. It also brings conditions
//! to the automations (`conditions@1`): today's and tomorrow's colour, the red and white days left. One creates an application on
//! the portal, subscribes it to that API and gives Breezone its client id and secret (OAuth client
//! credentials): the module's account.
//!
//!   POST {rte}/token/oauth/  (Basic client_id:client_secret)          → {access_token, expires_in}
//!   GET  {rte}/open_api/tempo_like_supply_contract/v1/tempo_like_calendars?start_date&end_date
//!        → {tempo_like_calendars: {values: [{start_date, end_date, value: BLUE|WHITE|RED}]}}

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use breezone_module_sdk::{host, json, module, Error, Result, Service, Value};
use chrono::{Datelike, Days, Duration, NaiveDate, Weekday};

const RTE: &str = "https://digital.iservices.rte-france.com";

fn refused(status: u16, body: &[u8]) -> Error {
    let v: Value = breezone_module_sdk::serde_json::from_slice(body).unwrap_or(Value::Null);
    let detail = ["error_description", "message", "error"]
        .iter()
        .find_map(|k| v[*k].as_str())
        .map(str::to_string)
        .unwrap_or_else(|| String::from_utf8_lossy(body).chars().take(160).collect());
    if status == 401 || status == 403 {
        Error::Invalid(format!("RTE refuse les identifiants ({detail})"))
    } else {
        Error::Unreachable(format!("RTE répond {status} : {detail}"))
    }
}

fn token() -> Result<String> {
    let account = host::account("rte")?;
    let (id, secret) = (account["client_id"].as_str().unwrap_or_default(), account["client_secret"].as_str().unwrap_or_default());
    if id.is_empty() || secret.is_empty() {
        return Err(Error::Invalid("l'ID client et l'ID secret de l'application RTE sont requis".into()));
    }
    let basic = format!("Basic {}", STANDARD.encode(format!("{id}:{secret}")));
    let a = host::request("POST", &format!("{RTE}/token/oauth/"), &[("Authorization", &basic), ("Content-Type", "application/x-www-form-urlencoded")], Some(b""), 20_000)?;
    if !(200..300).contains(&a.status) {
        return Err(refused(a.status, &a.body));
    }
    a.json()?["access_token"].as_str().map(str::to_string).ok_or_else(|| Error::Invalid("RTE: no access token".into()))
}

/// The last Sunday of a month.
fn last_sunday(year: i32, month: u32) -> NaiveDate {
    let mut d = NaiveDate::from_ymd_opt(year, month, 31).unwrap_or_default();
    while d.weekday() != Weekday::Sun {
        d = d.pred_opt().unwrap_or(d);
    }
    d
}

/// Midnight of a day in France, as the API wants it ("2026-11-18T00:00:00+01:00"): summer time
/// from the last Sunday of March to the last Sunday of October (at midnight, the day it changes
/// still has the time before).
pub fn paris(day: NaiveDate) -> String {
    let summer = day > last_sunday(day.year(), 3) && day <= last_sunday(day.year(), 10);
    format!("{day}T00:00:00+0{}:00", if summer { 2 } else { 1 })
}

fn color(value: &str) -> Option<&'static str> {
    match value {
        "BLUE" => Some("blue"),
        "WHITE" => Some("white"),
        "RED" => Some("red"),
        _ => None,
    }
}

fn day(params: &Value, key: &str) -> Result<NaiveDate> {
    params[key].as_str().and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()).ok_or_else(|| Error::Invalid(format!("`{key}`: YYYY-MM-DD expected")))
}

/// The colours from `from` to `until` (both included), as RTE published them.
pub fn calendar(from: NaiveDate, until: NaiveDate) -> Result<Value> {
    let token = token()?;
    let url = format!(
        "{RTE}/open_api/tempo_like_supply_contract/v1/tempo_like_calendars?start_date={}&end_date={}",
        paris(from).replace('+', "%2B"),
        paris(until + Days::new(1)).replace('+', "%2B")
    );
    let a = host::request("GET", &url, &[("Authorization", &format!("Bearer {token}"))], None, 20_000)?;
    if !(200..300).contains(&a.status) {
        return Err(refused(a.status, &a.body));
    }
    let body = a.json()?;
    let mut days: Vec<Value> = body["tempo_like_calendars"]["values"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| Some(json!({"day": v["start_date"].as_str()?.get(..10)?, "color": color(v["value"].as_str()?)?})))
        .collect();
    days.sort_by(|a, b| a["day"].as_str().cmp(&b["day"].as_str()));
    Ok(Value::Array(days))
}

/// Red and white days of a season (1 September to 31 August).
const RED_DAYS: i64 = 22;
const WHITE_DAYS: i64 = 43;

/// The day in France at `now` (seconds).
pub fn today_in_france(now: i64) -> NaiveDate {
    let utc = chrono::DateTime::from_timestamp(now, 0).unwrap_or_default().naive_utc();
    let winter = (utc + Duration::hours(1)).date();
    let summer = winter > last_sunday(winter.year(), 3) && winter <= last_sunday(winter.year(), 10);
    (utc + Duration::hours(if summer { 2 } else { 1 })).date()
}

/// What the automations may read: today's and tomorrow's colour (tomorrow's once announced), the
/// red and white days left in the season.
pub fn conditions(today: NaiveDate) -> Result<Value> {
    let year = if today.month() >= 9 { today.year() } else { today.year() - 1 };
    let season = NaiveDate::from_ymd_opt(year, 9, 1).unwrap_or(today);
    let days = calendar(season, today + Days::new(1))?;
    let color = |d: NaiveDate| days.as_array().into_iter().flatten().find(|v| v["day"] == d.to_string()).and_then(|v| v["color"].as_str()).map(str::to_string);
    let used = |c: &str| days.as_array().into_iter().flatten().filter(|v| v["color"] == c && v["day"].as_str().is_some_and(|d| d <= today.to_string().as_str())).count() as i64;
    let mut out = breezone_module_sdk::Map::new();
    if let Some(c) = color(today) {
        out.insert("today".into(), json!(c));
    }
    if let Some(c) = color(today + Days::new(1)) {
        out.insert("tomorrow".into(), json!(c));
    }
    out.insert("red_left".into(), json!((RED_DAYS - used("red")).max(0)));
    out.insert("white_left".into(), json!((WHITE_DAYS - used("white")).max(0)));
    Ok(Value::Object(out))
}

pub struct Rte;

impl Service for Rte {
    /// The account must give the colours of the last days.
    fn check() -> Result<()> {
        let today = chrono::DateTime::from_timestamp(host::now(), 0).map(|t| t.date_naive()).unwrap_or_default();
        calendar(today - Days::new(7), today).map(|_| ())
    }

    fn query(contract: &str, query: &str, params: Value) -> Result<Value> {
        match (contract, query) {
            ("tariff.day_color@1", "colors") => calendar(day(&params, "from")?, day(&params, "until")?),
            ("conditions@1", "read") => conditions(today_in_france(host::now())),
            _ => Err(Error::Unsupported(format!("{contract} {query}"))),
        }
    }
}

module!(service: Rte);

#[cfg(test)]
mod tests {
    use super::*;
    use breezone_module_sdk::testing;

    #[test]
    fn midnight_in_france() {
        let d = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        assert_eq!(paris(d("2026-11-18")), "2026-11-18T00:00:00+01:00");
        assert_eq!(paris(d("2026-07-01")), "2026-07-01T00:00:00+02:00");
        // 2026: summer time from 29 March, winter time from 25 October.
        assert_eq!(paris(d("2026-03-29")), "2026-03-29T00:00:00+01:00");
        assert_eq!(paris(d("2026-03-30")), "2026-03-30T00:00:00+02:00");
        assert_eq!(paris(d("2026-10-25")), "2026-10-25T00:00:00+02:00");
        assert_eq!(paris(d("2026-10-26")), "2026-10-26T00:00:00+01:00");
    }

    #[test]
    fn today_in_france_at_midnight() {
        let at = |s: &str| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap().and_utc().timestamp();
        assert_eq!(today_in_france(at("2026-11-23 23:30")).to_string(), "2026-11-24", "00:30 in winter");
        assert_eq!(today_in_france(at("2026-07-01 22:30")).to_string(), "2026-07-02", "00:30 in summer");
        assert_eq!(today_in_france(at("2026-07-01 21:30")).to_string(), "2026-07-01");
    }

    #[test]
    fn the_conditions_of_the_automations() {
        testing::reset();
        testing::account("rte", &json!({"client_id": "id", "client_secret": "secret"}));
        testing::answer_json("POST", "/token/oauth/", &json!({"access_token": "t", "expires_in": 7200}));
        testing::answer_json(
            "GET",
            "/tempo_like_calendars",
            &json!({"tempo_like_calendars": {"values": [
                {"start_date": "2026-11-24T00:00:00+01:00", "end_date": "2026-11-25T00:00:00+01:00", "value": "RED"},
                {"start_date": "2026-11-23T00:00:00+01:00", "end_date": "2026-11-24T00:00:00+01:00", "value": "WHITE"},
                {"start_date": "2026-11-20T00:00:00+01:00", "end_date": "2026-11-21T00:00:00+01:00", "value": "RED"},
                {"start_date": "2026-09-02T00:00:00+02:00", "end_date": "2026-09-03T00:00:00+02:00", "value": "BLUE"}
            ]}}),
        );
        let today = NaiveDate::from_ymd_opt(2026, 11, 23).unwrap();
        let v = conditions(today).unwrap();
        // Tomorrow's red is not counted yet: only the days that have come.
        assert_eq!(v, json!({"today": "white", "tomorrow": "red", "red_left": 21, "white_left": 42}));
        assert!(testing::requests().iter().any(|r| r.contains("start_date=2026-09-01T00:00:00%2B02:00")), "from the start of the season: {:?}", testing::requests());
    }

    #[test]
    fn the_colours_of_the_days() {
        testing::reset();
        testing::account("rte", &json!({"client_id": "id", "client_secret": "secret"}));
        testing::answer_json("POST", "/token/oauth/", &json!({"access_token": "t", "expires_in": 7200}));
        testing::answer_json(
            "GET",
            "/tempo_like_calendars",
            &json!({"tempo_like_calendars": {"values": [
                {"start_date": "2026-11-23T00:00:00+01:00", "end_date": "2026-11-24T00:00:00+01:00", "value": "RED"},
                {"start_date": "2026-11-22T00:00:00+01:00", "end_date": "2026-11-23T00:00:00+01:00", "value": "WHITE"}
            ]}}),
        );
        let colors = Rte::query("tariff.day_color@1", "colors", json!({"from": "2026-11-22", "until": "2026-11-23"})).unwrap();
        assert_eq!(colors, json!([{"day": "2026-11-22", "color": "white"}, {"day": "2026-11-23", "color": "red"}]));
        assert!(testing::requests().iter().any(|r| r.contains("start_date=2026-11-22T00:00:00%2B01:00&end_date=2026-11-24T00:00:00%2B01:00")), "{:?}", testing::requests());
        assert!(Rte::check().is_ok());
        assert!(Rte::query("tariff.day_color@1", "colors", json!({"from": "hier"})).is_err());
        testing::answer("POST", "/token/oauth/", 401, br#"{"error_description": "bad client"}"#.to_vec());
        assert!(matches!(Rte::check(), Err(Error::Invalid(m)) if m.contains("bad client")));
    }
}
