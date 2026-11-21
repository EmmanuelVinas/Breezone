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

use super::{Described, DeviceInfo, Driver, DriverError, DriverFuture, State};
use chrono::{NaiveDateTime, TimeZone};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

pub const REFERENCE: &str = "forecast";
const ASK_EVERY: Duration = Duration::from_secs(1800);

/// A forecast as read: power over the day (W, by time), Wh since the morning, Wh per day.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Forecast {
    pub watts: Vec<(i64, f64)>,
    pub cumulated: Vec<(i64, f64)>,
    pub days: HashMap<String, f64>,
}

/// The forecast in an answer, its times read in the time zone of the place.
pub fn parse(body: &Value) -> Result<Forecast, String> {
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
    http: reqwest::Client,
    url: String,
    tz: chrono_tz::Tz,
    last: Mutex<Option<(Instant, Forecast)>>,
}

impl ForecastSolar {
    pub fn new(config: &Value, secret: Option<&str>) -> Result<Self, String> {
        let num = |k: &str| config[k].as_f64().or_else(|| config[k].as_str().and_then(|s| s.trim().replace(',', ".").parse().ok()));
        let kwp = num("kwp").filter(|k| *k > 0.0 && *k <= 1000.0).ok_or("donnez la puissance crête des panneaux (kWc)")?;
        let dec = num("declination").unwrap_or(30.0).clamp(0.0, 90.0);
        let az = num("azimuth").unwrap_or(0.0).clamp(-180.0, 180.0);
        let (lat, lon) = (num("latitude").ok_or("la latitude est inconnue")?, num("longitude").ok_or("la longitude est inconnue")?);
        let key = secret.and_then(|s| serde_json::from_str::<Value>(s).ok()).and_then(|v| v["api_key"].as_str().map(str::to_string)).filter(|k| !k.is_empty());
        let base = config["url"].as_str().map(str::to_string).or_else(|| std::env::var("BREEZONE_FORECAST_SOLAR_URL").ok()).unwrap_or_else(|| "https://api.forecast.solar".into());
        let base = base.trim_end_matches('/');
        let url = match key {
            Some(k) => format!("{base}/{k}/estimate/{lat}/{lon}/{dec}/{az}/{kwp}"),
            None => format!("{base}/estimate/{lat}/{lon}/{dec}/{az}/{kwp}"),
        };
        let tz = config["timezone"].as_str().and_then(|t| t.parse().ok()).unwrap_or(chrono_tz::Europe::Paris);
        let http = reqwest::Client::builder().timeout(Duration::from_secs(20)).user_agent(concat!("Breezone/", env!("CARGO_PKG_VERSION"))).build().unwrap_or_default();
        Ok(Self { http, url, tz, last: Mutex::new(None) })
    }

    /// The forecast, asked again when it is old (the last one kept when the service refuses).
    async fn forecast(&self) -> Result<Forecast, DriverError> {
        let mut last = self.last.lock().await;
        if let Some((_, f)) = last.as_ref().filter(|(at, _)| at.elapsed() < ASK_EVERY) {
            return Ok(f.clone());
        }
        let asked = async {
            let resp = self.http.get(&self.url).header("Accept", "application/json").send().await.map_err(|e| DriverError::Unreachable(format!("Forecast.Solar: {e}")))?;
            let body: Value = resp.json().await.map_err(|e| DriverError::Unreachable(format!("Forecast.Solar: {e}")))?;
            parse(&body).map_err(|e| DriverError::Unreachable(format!("Forecast.Solar: {e}")))
        };
        match asked.await {
            Ok(f) => {
                *last = Some((Instant::now(), f.clone()));
                Ok(f)
            }
            // Too many requests, or a moment without network: the last forecast holds.
            Err(e) => match last.as_ref() {
                Some((_, f)) => {
                    tracing::debug!("{e}: the last forecast is kept");
                    Ok(f.clone())
                }
                None => Err(e),
            },
        }
    }
}

impl Driver for ForecastSolar {
    fn describe(&self) -> DriverFuture<'_, Described> {
        Box::pin(async move {
            self.forecast().await?;
            Ok(Described {
                model: "Forecast.Solar".into(),
                firmware: None,
                devices: vec![DeviceInfo { reference: REFERENCE.into(), kind: "solar.forecast".into(), traits: vec!["solar.forecast@1".into()], name: "Forecast.Solar".into() }],
                identity: None,
            })
        })
    }

    fn read(&self) -> DriverFuture<'_, HashMap<String, State>> {
        Box::pin(async move {
            let f = self.forecast().await?;
            Ok(HashMap::from([(REFERENCE.to_string(), state_at(&f, crate::db::now(), self.tz))]))
        })
    }

    fn act<'a>(&'a self, _reference: &'a str, _trait_name: &'a str, _action: &'a str, _params: &'a State) -> DriverFuture<'a, ()> {
        Box::pin(async { Err(DriverError::Unsupported("a forecast is only read".into())) })
    }

    /// The power expected along today and tomorrow, as Forecast.Solar gives it.
    fn forecast<'a>(&'a self, _reference: &'a str) -> DriverFuture<'a, super::Forecast> {
        Box::pin(async move {
            let f = self.forecast().await?;
            Ok(super::Forecast::from([("power".to_string(), f.watts)]))
        })
    }

    /// The power expected now moves along the day: worked out every minute from the forecast.
    fn read_every(&self) -> Duration {
        Duration::from_secs(60)
    }

    fn diagnostic(&self) -> DriverFuture<'_, Value> {
        Box::pin(async move {
            let f = self.forecast().await?;
            Ok(json!({ "points": f.watts.len(), "days": f.days }))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono_tz::Europe::Paris;

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
}
