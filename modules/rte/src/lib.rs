//! RTE's data portal (data.rte-france.com): the official colour of the Tempo days
//! (`tariff.day_color@1`), with the "Tempo Like Supply Contract" API. One creates an application on
//! the portal, subscribes it to that API and gives Breezone its client id and secret (OAuth client
//! credentials): the module's account.
//!
//!   POST {rte}/token/oauth/  (Basic client_id:client_secret)          → {access_token, expires_in}
//!   GET  {rte}/open_api/tempo_like_supply_contract/v1/tempo_like_calendars?start_date&end_date
//!        → {tempo_like_calendars: {values: [{start_date, end_date, value: BLUE|WHITE|RED}]}}

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use breezone_module_sdk::{host, json, module, Error, Result, Service, Value};
use chrono::{Datelike, Days, NaiveDate, Weekday};

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
