//! Forecast.Solar: what the solar panels should produce, from their place, tilt, orientation and
//! peak power (free, no account; a key raises the limits). One device, `solar.forecast@1`: the
//! power expected now, the energy expected today, still to come today, and tomorrow.
//!
//!   GET {forecast}/[{key}/]estimate/{lat}/{lon}/{tilt}/{azimuth}/{kwp}
//!     → {result: {watts: {"YYYY-MM-DD HH:MM:SS": W}, watt_hours: {…: Wh since the morning},
//!        watt_hours_day: {"YYYY-MM-DD": Wh}}, message: {type, text, info: {timezone}}}
//!
//! The free service answers 12 times an hour from one address: the forecast is asked again every
//! 30 minutes, and what is shown in between is worked out from it.

use breezone_module_sdk::{device, host, json, module, Curve, Described, Driver, Error, Map, Result, State, Value};
use chrono::{NaiveDateTime, TimeZone};
use std::cell::RefCell;
use std::collections::HashMap;

pub const REFERENCE: &str = "forecast";
/// Seconds between two forecasts asked.
const ASK_EVERY: i64 = 1800;

/// A forecast as read: power over the day (W, by time), Wh since the morning, Wh per day.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Forecast {
    pub watts: Vec<(i64, f64)>,
    pub cumulated: Vec<(i64, f64)>,
    pub days: HashMap<String, f64>,
}

/// The forecast in an answer, its times read in the time zone of the place.
pub fn parse(body: &Value) -> std::result::Result<Forecast, String> {
    if body["message"]["type"].as_str() == Some("error") {
        return Err(body["message"]["text"].as_str().unwrap_or("error").to_string());
    }
    let tz: chrono_tz::Tz = body["message"]["info"]["timezone"].as_str().and_then(|t| t.parse().ok()).unwrap_or(chrono_tz::UTC);
    let series = |key: &str| -> Vec<(i64, f64)> {
        let mut out: Vec<(i64, f64)> = body["result"][key]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(t, v)| {
                let local = NaiveDateTime::parse_from_str(t, "%Y-%m-%d %H:%M:%S").ok()?;
                Some((tz.from_local_datetime(&local).earliest()?.timestamp(), v.as_f64()?))
            })
            .collect();
        out.sort_by_key(|(t, _)| *t);
        out
    };
    let days = body["result"]["watt_hours_day"].as_object().into_iter().flatten().filter_map(|(d, v)| Some((d.clone(), v.as_f64()?))).collect();
    Ok(Forecast { watts: series("watts"), cumulated: series("watt_hours"), days })
}

/// The value of a series at `t`, between its points (0 before the first, the last after it… on
/// the same day only: the sun is down between two days).
fn at(series: &[(i64, f64)], t: i64, after_last: bool) -> f64 {
    let Some(i) = series.iter().position(|(ts, _)| *ts >= t) else {
        return if after_last { series.last().map(|(_, v)| *v).unwrap_or(0.0) } else { 0.0 };
    };
    if i == 0 {
        return if series[0].0 == t { series[0].1 } else { 0.0 };
    }
    let ((t0, v0), (t1, v1)) = (series[i - 1], series[i]);
    // Two days apart: night in between.
    if t1 - t0 > 6 * 3600 {
        return if after_last { v0 } else { 0.0 };
    }
    v0 + (v1 - v0) * (t - t0) as f64 / (t1 - t0) as f64
}

/// What the forecast says at `now`, in the time zone of the home.
pub fn state_at(f: &Forecast, now: i64, tz: chrono_tz::Tz) -> State {
    let today = tz.timestamp_opt(now, 0).single().map(|d| d.date_naive()).unwrap_or_default();
    let tomorrow = today.succ_opt().unwrap_or(today);
    let day_total = f.days.get(&today.to_string()).copied();
    // The cumulated energy of today only (the series holds two days).
    let today_points: Vec<(i64, f64)> = f.cumulated.iter().copied().filter(|(t, _)| tz.timestamp_opt(*t, 0).single().map(|d| d.date_naive()) == Some(today)).collect();
    let done = if today_points.is_empty() { 0.0 } else { at(&today_points, now, true) };
    let kwh = |wh: f64| (wh / 100.0).round() / 10.0;
    let mut out = Map::new();
    out.insert("power".into(), json!(at(&f.watts, now, false).round()));
    if let Some(total) = day_total {
        out.insert("energy_today".into(), json!(kwh(total)));
        out.insert("energy_remaining".into(), json!(kwh((total - done).max(0.0))));
    }
    if let Some(t) = f.days.get(&tomorrow.to_string()) {
        out.insert("energy_tomorrow".into(), json!(kwh(*t)));
    }
    out
}

pub struct ForecastSolar {
    url: String,
    tz: chrono_tz::Tz,
    /// The last forecast, and when it was asked (unix time).
    last: RefCell<Option<(i64, Forecast)>>,
}

impl ForecastSolar {
    /// The forecast, asked again when it is old (the last one kept when the service refuses).
    fn forecast(&self) -> Result<Forecast> {
        let now = host::now();
        if let Some((_, f)) = self.last.borrow().as_ref().filter(|(at, _)| now - at < ASK_EVERY) {
            return Ok(f.clone());
        }
        let asked = host::request("GET", &self.url, &[("Accept", "application/json")], None, 20_000)
            .and_then(|a| a.json())
            .and_then(|body| parse(&body).map_err(|e| Error::Unreachable(format!("Forecast.Solar: {e}"))));
        match asked {
            Ok(f) => {
                *self.last.borrow_mut() = Some((now, f.clone()));
                Ok(f)
            }
            // Too many requests, or a moment without network: the last forecast holds.
            Err(e) => match self.last.borrow().as_ref() {
                Some((_, f)) => {
                    host::log("debug", &format!("{e:?}: the last forecast is kept"));
                    Ok(f.clone())
                }
                None => Err(e),
            },
        }
    }
}

impl Driver for ForecastSolar {
    fn open(config: Value, secret: Option<Value>) -> Result<Self> {
        let num = |k: &str| config[k].as_f64().or_else(|| config[k].as_str().and_then(|s| s.trim().replace(',', ".").parse().ok()));
        let invalid = |m: &str| Error::Invalid(m.to_string());
        let kwp = num("kwp").filter(|k| *k > 0.0 && *k <= 1000.0).ok_or_else(|| invalid("donnez la puissance crête des panneaux (kWc)"))?;
        let dec = num("declination").unwrap_or(30.0).clamp(0.0, 90.0);
        let az = num("azimuth").unwrap_or(0.0).clamp(-180.0, 180.0);
        let lat = num("latitude").ok_or_else(|| invalid("la latitude est inconnue"))?;
        let lon = num("longitude").ok_or_else(|| invalid("la longitude est inconnue"))?;
        let key = secret.and_then(|v| v["api_key"].as_str().map(str::to_string)).filter(|k| !k.is_empty());
        let base = config["url"].as_str().unwrap_or("https://api.forecast.solar").trim_end_matches('/').to_string();
        let url = match key {
            Some(k) => format!("{base}/{k}/estimate/{lat}/{lon}/{dec}/{az}/{kwp}"),
            None => format!("{base}/estimate/{lat}/{lon}/{dec}/{az}/{kwp}"),
        };
        let tz = config["timezone"].as_str().and_then(|t| t.parse().ok()).unwrap_or(chrono_tz::Europe::Paris);
        Ok(ForecastSolar { url, tz, last: RefCell::new(None) })
    }

    fn describe(&self) -> Result<Described> {
        self.forecast()?;
        Ok(Described {
            model: "Forecast.Solar".into(),
            firmware: None,
            devices: vec![device(REFERENCE, "solar.forecast", &["solar.forecast@1"], "Forecast.Solar")],
            identity: None,
        })
    }

    fn read(&self) -> Result<Vec<(String, State)>> {
        let f = self.forecast()?;
        Ok(vec![(REFERENCE.to_string(), state_at(&f, host::now(), self.tz))])
    }

    fn act(&self, _reference: &str, _trait_name: &str, _action: &str, _params: &State) -> Result<()> {
        Err(Error::Unsupported("a forecast is only read".into()))
    }

    /// The power expected along today and tomorrow, as Forecast.Solar gives it.
    fn forecast(&self, _reference: &str) -> Result<Vec<Curve>> {
        Ok(vec![Curve { name: "power".into(), points: self.forecast()?.watts }])
    }

    fn diagnostic(&self) -> Result<Value> {
        let f = self.forecast()?;
        Ok(json!({ "points": f.watts.len(), "days": f.days }))
    }
}

module!(ForecastSolar);

#[cfg(test)]
mod tests {
    use super::*;
    use breezone_module_sdk::testing;
    use chrono_tz::Europe::Paris;

    const MANIFEST: &str = include_str!("../manifest.json");

    fn ts(s: &str) -> i64 {
        Paris.from_local_datetime(&NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()).earliest().unwrap().timestamp()
    }

    #[test]
    fn the_power_now_and_the_energy_left_from_a_forecast() {
        let body = json!({
            "result": {
                "watts": {"2026-11-19 08:00:00": 0, "2026-11-19 12:00:00": 2000, "2026-11-19 17:00:00": 0, "2026-11-20 08:00:00": 0, "2026-11-20 12:00:00": 1000},
                "watt_hours": {"2026-11-19 08:00:00": 0, "2026-11-19 12:00:00": 4000, "2026-11-19 17:00:00": 9000, "2026-11-20 08:00:00": 0},
                "watt_hours_day": {"2026-11-19": 9000, "2026-11-20": 4500}
            },
            "message": {"type": "success", "info": {"timezone": "Europe/Paris"}}
        });
        let f = parse(&body).unwrap();
        let s = state_at(&f, ts("2026-11-19 10:00:00"), Paris);
        assert_eq!(s["power"], 1000.0, "halfway between 0 and 2 kW");
        assert_eq!((s["energy_today"].clone(), s["energy_remaining"].clone(), s["energy_tomorrow"].clone()), (json!(9.0), json!(7.0), json!(4.5)));
        assert_eq!(state_at(&f, ts("2026-11-19 20:00:00"), Paris)["power"], 0.0, "night: nothing");
        assert_eq!(state_at(&f, ts("2026-11-19 20:00:00"), Paris)["energy_remaining"], 0.0);
        assert!(parse(&json!({"message": {"type": "error", "text": "Rate limit exceeded"}})).unwrap_err().contains("Rate limit"));
    }

    #[test]
    fn asked_once_then_kept_and_checked() {
        testing::reset();
        testing::answer_json(
            "GET",
            "/estimate/",
            &json!({
                "result": {"watts": {"2026-11-19 12:00:00": 2000}, "watt_hours": {"2026-11-19 12:00:00": 4000}, "watt_hours_day": {"2026-11-19": 9000}},
                "message": {"type": "success", "info": {"timezone": "Europe/Paris"}}
            }),
        );
        let f: ForecastSolar =
            testing::open(json!({"kwp": "4,5", "declination": 30, "azimuth": 0, "latitude": 43.5, "longitude": 1.5, "timezone": "Europe/Paris"}), Some(json!({"api_key": "k"})));
        testing::check(&f, MANIFEST);
        f.read().unwrap();
        let asked = testing::requests();
        assert_eq!(asked.len(), 1, "asked once: {asked:?}");
        assert_eq!(asked[0], "GET https://api.forecast.solar/k/estimate/43.5/1.5/30/0/4.5");
        assert_eq!(Driver::forecast(&f, REFERENCE).unwrap()[0].points.len(), 1);
        assert!(ForecastSolar::open(json!({"latitude": 1, "longitude": 1}), None).is_err(), "no peak power");
    }
}
