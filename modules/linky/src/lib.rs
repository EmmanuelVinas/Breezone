//! What Enedis measured on the Linky meter, without any hardware (`energy.history@1`): through
//! Conso API (conso.boris.sh), which holds the Enedis agreement. One gives it the consent of one's
//! meter on its site and gets a personal token; the module's account is that token and the meter
//! number (PRM).
//!
//!   GET {conso}/api/daily_consumption?prm&start&end      → {interval_reading: [{value: Wh, date}]}
//!   GET {conso}/api/daily_production?prm&start&end       → idem
//!   GET {conso}/api/consumption_load_curve?prm&start&end → {interval_reading: [{value: W, date}]}
//!       (each date is the END of its half hour, in French time)

use breezone_module_sdk::{host, json, module, Error, Map, Result, Service, Value};
use chrono::{Datelike, Days, NaiveDate, NaiveDateTime, Weekday};
use std::collections::BTreeMap;

const CONSO: &str = "https://conso.boris.sh";

/// The meter number: 14 digits.
fn valid_prm(prm: &str) -> bool {
    prm.len() == 14 && prm.chars().all(|c| c.is_ascii_digit())
}

fn account() -> Result<(String, String)> {
    let a = host::account("linky")?;
    let token = a["token"].as_str().map(str::trim).unwrap_or_default().to_string();
    let prm = a["prm"].as_str().unwrap_or_default().replace(' ', "");
    if token.is_empty() || !valid_prm(&prm) {
        return Err(Error::Invalid("le jeton Conso API et le numéro du compteur (PRM, 14 chiffres) sont requis".into()));
    }
    Ok((token, prm))
}

fn readings(what: &str, from: NaiveDate, until: NaiveDate) -> Result<Vec<(String, f64)>> {
    let (token, prm) = account()?;
    let url = format!("{CONSO}/api/{what}?prm={prm}&start={from}&end={until}");
    let a = host::request("GET", &url, &[("Authorization", &format!("Bearer {token}"))], None, 20_000)?;
    if !(200..300).contains(&a.status) {
        let detail: String = String::from_utf8_lossy(&a.body).chars().take(160).collect();
        return Err(if a.status == 401 || a.status == 403 {
            Error::Invalid(format!("Conso API refuse les identifiants ({detail})"))
        } else {
            Error::Unreachable(format!("Conso API répond {} : {detail}", a.status))
        });
    }
    Ok(a.json()?["interval_reading"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let value = r["value"].as_str().and_then(|v| v.parse().ok()).or_else(|| r["value"].as_f64())?;
            Some((r["date"].as_str()?.to_string(), value))
        })
        .collect())
}

fn last_sunday(year: i32, month: u32) -> NaiveDate {
    let mut d = NaiveDate::from_ymd_opt(year, month, 31).unwrap_or_default();
    while d.weekday() != Weekday::Sun {
        d = d.pred_opt().unwrap_or(d);
    }
    d
}

/// A time in France as a timestamp: summer time from 2:00 on the last Sunday of March to 3:00 on
/// the last Sunday of October (the hour told twice is taken the first time).
pub fn paris(local: NaiveDateTime) -> i64 {
    let y = local.year();
    let start = last_sunday(y, 3).and_hms_opt(2, 0, 0).unwrap_or_default();
    let end = last_sunday(y, 10).and_hms_opt(3, 0, 0).unwrap_or_default();
    let offset = if local >= start && local < end { 2 } else { 1 };
    local.and_utc().timestamp() - offset * 3600
}

/// The start of the half hour ending at `date` ("2026-11-17 00:30:00", French time).
fn half_hour_start(date: &str) -> Option<i64> {
    Some(paris(NaiveDateTime::parse_from_str(date, "%Y-%m-%d %H:%M:%S").ok()?) - 1800)
}

fn day(params: &Value, key: &str) -> Result<NaiveDate> {
    params[key].as_str().and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()).ok_or_else(|| Error::Invalid(format!("`{key}`: YYYY-MM-DD expected")))
}

/// The energy of each day from `from` to `until` (included), in kWh.
fn days(from: NaiveDate, until: NaiveDate) -> Result<Value> {
    // Conso API's end is excluded.
    let end = until + Days::new(1);
    let mut out: BTreeMap<String, Map<String, Value>> = BTreeMap::new();
    for (date, wh) in readings("daily_consumption", from, end)? {
        out.entry(date.clone()).or_default().insert("consumption".into(), json!(wh / 1000.0));
    }
    // Only there when the meter produces.
    if let Ok(made) = readings("daily_production", from, end) {
        for (date, wh) in made {
            out.entry(date).or_default().insert("production".into(), json!(wh / 1000.0));
        }
    }
    Ok(Value::Array(
        out.into_iter()
            .map(|(d, mut m)| {
                m.insert("day".into(), json!(d.get(..10).unwrap_or(&d)));
                Value::Object(m)
            })
            .collect(),
    ))
}

/// The mean power of each half hour of a day.
fn curve(day: NaiveDate) -> Result<Value> {
    let list = readings("consumption_load_curve", day, day + Days::new(1))?;
    Ok(Value::Array(list.iter().filter_map(|(d, w)| Some(json!({"ts": half_hour_start(d)?, "power": w}))).collect()))
}

pub struct Linky;

impl Service for Linky {
    /// The meter must answer with its last days.
    fn check() -> Result<()> {
        let today = chrono::DateTime::from_timestamp(host::now(), 0).map(|t| t.date_naive()).unwrap_or_default();
        readings("daily_consumption", today - Days::new(7), today).map(|_| ())
    }

    fn query(contract: &str, query: &str, params: Value) -> Result<Value> {
        match (contract, query) {
            ("energy.history@1", "days") => days(day(&params, "from")?, day(&params, "until")?),
            ("energy.history@1", "curve") => curve(day(&params, "day")?),
            _ => Err(Error::Unsupported(format!("{contract} {query}"))),
        }
    }
}

module!(service: Linky);

#[cfg(test)]
mod tests {
    use super::*;
    use breezone_module_sdk::testing;

    #[test]
    fn french_time() {
        let at = |s: &str| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap();
        assert_eq!(paris(at("2026-11-17 01:00:00")), at("2026-11-17 00:00:00").and_utc().timestamp());
        assert_eq!(paris(at("2026-07-01 02:00:00")), at("2026-07-01 00:00:00").and_utc().timestamp());
        assert_eq!(half_hour_start("2026-11-17 00:30:00"), Some(at("2026-11-16 23:00:00").and_utc().timestamp()));
    }

    #[test]
    fn days_and_half_hours() {
        testing::reset();
        testing::account("linky", &json!({"token": "t", "prm": "12345678901234"}));
        testing::answer_json("GET", "/api/daily_consumption", &json!({"interval_reading": [{"value": "8450", "date": "2026-11-20"}, {"value": "9100", "date": "2026-11-21"}]}));
        testing::answer_json("GET", "/api/daily_production", &json!({"interval_reading": [{"value": "1200", "date": "2026-11-21"}]}));
        testing::answer_json("GET", "/api/consumption_load_curve", &json!({"interval_reading": [{"value": "640", "date": "2026-11-21 00:30:00"}]}));
        let d = Linky::query("energy.history@1", "days", json!({"from": "2026-11-20", "until": "2026-11-21"})).unwrap();
        assert_eq!(d, json!([{"day": "2026-11-20", "consumption": 8.45}, {"day": "2026-11-21", "consumption": 9.1, "production": 1.2}]));
        assert!(testing::requests()[0].ends_with("prm=12345678901234&start=2026-11-20&end=2026-11-22"), "{:?}", testing::requests());
        let c = Linky::query("energy.history@1", "curve", json!({"day": "2026-11-21"})).unwrap();
        assert_eq!(c[0]["power"], json!(640.0));
        assert!(Linky::check().is_ok());
        testing::account("linky", &json!({"token": "t", "prm": "123"}));
        assert!(matches!(Linky::check(), Err(Error::Invalid(_))));
    }
}
