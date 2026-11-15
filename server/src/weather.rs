//! The context of the home (roadmap F9): the sun, computed here without any network, and the
//! weather, read every 30 minutes from Open-Meteo (free, no account, Météo-France data for France).
//! The place is the city of the home (its first installation of this server), turned once into
//! coordinates; nothing else leaves the server. The outdoor temperature is kept, so that the
//! curves of the zones can show it.

use crate::db::{self, Db};
use chrono::{Datelike, NaiveDate};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::f64::consts::PI;
use std::time::Duration;
use tokio::sync::RwLock;

const EVERY: Duration = Duration::from_secs(1800);
const PLACE: &str = "weather.place";
const COORDS: &str = "weather.coords";

/// Sunrise and sunset of a day (timestamps); `None` in a polar day or night.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SunTimes {
    pub sunrise: Option<i64>,
    pub sunset: Option<i64>,
}

/// Sunrise and sunset at `lat`, `lon` on `day` (NOAA's sunrise equation, within a minute or two).
pub fn sun(day: NaiveDate, lat: f64, lon: f64) -> SunTimes {
    let rad = PI / 180.0;
    // Julian day at noon UTC, then the mean solar noon at this longitude.
    let jd = day.num_days_from_ce() as f64 + 1_721_424.5 + 0.5;
    let n = (jd - 2_451_545.0 + 0.0008).round();
    let j_star = n - lon / 360.0;
    let m = (357.5291 + 0.985_600_28 * j_star).rem_euclid(360.0);
    let c = 1.9148 * (m * rad).sin() + 0.02 * (2.0 * m * rad).sin() + 0.0003 * (3.0 * m * rad).sin();
    let lambda = (m + c + 180.0 + 102.9372).rem_euclid(360.0);
    let transit = 2_451_545.0 + j_star + 0.0053 * (m * rad).sin() - 0.0069 * (2.0 * lambda * rad).sin();
    let decl = ((lambda * rad).sin() * (23.4397 * rad).sin()).asin();
    // The sun's centre 0.833° below the horizon: refraction and its radius.
    let cos_h = ((-0.833 * rad).sin() - (lat * rad).sin() * decl.sin()) / ((lat * rad).cos() * decl.cos());
    if !(-1.0..=1.0).contains(&cos_h) {
        return SunTimes { sunrise: None, sunset: None };
    }
    let h = cos_h.acos() / rad / 360.0;
    let ts = |j: f64| ((j - 2_440_587.5) * 86_400.0).round() as i64;
    SunTimes { sunrise: Some(ts(transit - h)), sunset: Some(ts(transit + h)) }
}

/// An hour of the forecast.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hour {
    pub ts: i64,
    pub temperature: Option<f64>,
    /// WMO weather code.
    pub code: Option<i64>,
    pub precipitation_probability: Option<f64>,
}

/// The weather of the home now, today and in the next hours.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    /// The city it is for.
    pub place: String,
    pub temperature: Option<f64>,
    pub apparent: Option<f64>,
    pub code: Option<i64>,
    pub precipitation: Option<f64>,
    pub wind: Option<f64>,
    pub today_min: Option<f64>,
    pub today_max: Option<f64>,
    pub tomorrow_min: Option<f64>,
    pub tomorrow_max: Option<f64>,
    pub tomorrow_code: Option<i64>,
    /// The next 24 hours.
    pub hours: Vec<Hour>,
    /// When it was read (seconds).
    pub read_at: i64,
}

/// What the weather page gets: the weather (if any) and the sun of today.
#[derive(Debug, Clone, Serialize)]
pub struct View {
    pub weather: Option<Report>,
    pub sun: Option<SunTimes>,
}

/// Reads the forecast of Open-Meteo's answer (`current`, `hourly`, `daily`, `timezone=UTC`).
pub fn report_of(place: &str, data: &Value, now: i64) -> Option<Report> {
    let cur = &data["current"];
    let num = |v: &Value| v.as_f64();
    let times: Vec<i64> = data["hourly"]["time"].as_array()?.iter().filter_map(|t| t.as_i64()).collect();
    let hourly = |key: &str, i: usize| data["hourly"][key].get(i).and_then(Value::as_f64);
    let hours = times
        .iter()
        .enumerate()
        .filter(|(_, ts)| **ts + 3600 > now && **ts < now + 24 * 3600)
        .map(|(i, ts)| Hour {
            ts: *ts,
            temperature: hourly("temperature_2m", i),
            code: hourly("weather_code", i).map(|c| c as i64),
            precipitation_probability: hourly("precipitation_probability", i),
        })
        .collect();
    let daily = |key: &str, i: usize| data["daily"][key].get(i).and_then(Value::as_f64);
    Some(Report {
        place: place.to_string(),
        temperature: num(&cur["temperature_2m"]),
        apparent: num(&cur["apparent_temperature"]),
        code: num(&cur["weather_code"]).map(|c| c as i64),
        precipitation: num(&cur["precipitation"]),
        wind: num(&cur["wind_speed_10m"]),
        today_min: daily("temperature_2m_min", 0),
        today_max: daily("temperature_2m_max", 0),
        tomorrow_min: daily("temperature_2m_min", 1),
        tomorrow_max: daily("temperature_2m_max", 1),
        tomorrow_code: daily("weather_code", 1).map(|c| c as i64),
        hours,
        read_at: now,
    })
}

pub struct Weather {
    db: Db,
    tz: Tz,
    http: reqwest::Client,
    last: RwLock<Option<Report>>,
}

impl Weather {
    pub fn new(db: Db, tz: Tz) -> Self {
        let http = reqwest::Client::builder().timeout(Duration::from_secs(10)).user_agent("Breezone").build().unwrap_or_default();
        Self { db, tz, http, last: RwLock::new(None) }
    }

    /// The city of the home and its country: set in the settings, else those of its first
    /// installation (its city as set by hand or read from the Airzone account).
    async fn place(&self) -> sqlx::Result<Option<(String, Option<String>)>> {
        if let Some(p) = db::get_setting(&self.db, PLACE).await?.filter(|p| !p.trim().is_empty()) {
            return Ok(Some((p, None)));
        }
        sqlx::query_as("SELECT location, country FROM installations WHERE node_id IS NULL AND location IS NOT NULL AND location <> '' ORDER BY id LIMIT 1")
            .fetch_optional(&self.db)
            .await
    }

    /// The coordinates of `place` (in `country` when known), asked once to Open-Meteo's geocoding
    /// and kept.
    async fn coords(&self, place: &str, country: Option<&str>) -> Option<(f64, f64)> {
        let key = format!("{place}#{}", country.unwrap_or(""));
        if let Ok(Some(kept)) = db::get_setting(&self.db, COORDS).await {
            if let Some((p, lat, lon)) = kept.split_once('|').and_then(|(p, rest)| rest.split_once('|').map(|(a, b)| (p, a, b))) {
                if p == key {
                    return Some((lat.parse().ok()?, lon.parse().ok()?));
                }
            }
        }
        // "Labège, Haute-Garonne": the city is asked, the region helps to choose.
        let (city, region) = place.split_once(',').map_or((place.trim(), ""), |(c, r)| (c.trim(), r.trim()));
        let answer: Value = self
            .http
            .get("https://geocoding-api.open-meteo.com/v1/search")
            .query(&[("name", city), ("count", "10"), ("language", "fr"), ("format", "json")])
            // An empty country code finds nothing: only when known.
            .query(&country.map(|c| vec![("countryCode", c)]).unwrap_or_default())
            .send()
            .await
            .ok()?
            .json()
            .await
            .ok()?;
        let results = answer["results"].as_array()?;
        let best =
            results.iter().find(|r| !region.is_empty() && r["admin2"].as_str().or(r["admin1"].as_str()).is_some_and(|a| a.eq_ignore_ascii_case(region))).or(results.first())?;
        let (lat, lon) = (best["latitude"].as_f64()?, best["longitude"].as_f64()?);
        let _ = db::set_setting(&self.db, COORDS, &format!("{key}|{lat}|{lon}")).await;
        Some((lat, lon))
    }

    /// Reads the weather now; keeps the outdoor temperature.
    pub async fn refresh(&self) -> Option<Report> {
        let (place, country) = self.place().await.ok()??;
        let (lat, lon) = self.coords(&place, country.as_deref()).await?;
        let data: Value = self
            .http
            .get("https://api.open-meteo.com/v1/forecast")
            .query(&[
                ("latitude", lat.to_string()),
                ("longitude", lon.to_string()),
                ("current", "temperature_2m,apparent_temperature,weather_code,precipitation,wind_speed_10m".into()),
                ("hourly", "temperature_2m,weather_code,precipitation_probability".into()),
                ("daily", "temperature_2m_max,temperature_2m_min,weather_code".into()),
                ("timezone", self.tz.name().into()),
                ("timeformat", "unixtime".into()),
                ("forecast_days", "2".into()),
            ])
            .send()
            .await
            .ok()?
            .json()
            .await
            .ok()?;
        let report = report_of(&place, &data, db::now())?;
        if let Some(t) = report.temperature {
            if let Err(e) = sqlx::query("INSERT OR REPLACE INTO outdoor_samples (ts, temperature) VALUES (?, ?)").bind(db::now()).bind(t).execute(&self.db).await {
                tracing::warn!("cannot keep the outdoor temperature: {e}");
            }
        }
        *self.last.write().await = Some(report.clone());
        Some(report)
    }

    /// The last outdoor temperature read.
    pub async fn temperature(&self) -> Option<f64> {
        self.last.read().await.as_ref().and_then(|r| r.temperature)
    }

    /// Where the home is (latitude, longitude), when its city is known.
    pub async fn coordinates(&self) -> Option<(f64, f64)> {
        let (place, country) = self.place().await.ok()??;
        self.coords(&place, country.as_deref()).await
    }

    /// The weather (the last one read) and the sun of today.
    pub async fn view(&self) -> View {
        let weather = self.last.read().await.clone();
        let today = chrono::Utc::now().with_timezone(&self.tz).date_naive();
        let sun = match self.place().await.ok().flatten() {
            Some((place, country)) => self.coords(&place, country.as_deref()).await.map(|(lat, lon)| sun(today, lat, lon)),
            None => None,
        };
        View { weather, sun }
    }

    /// The outdoor temperature since `since`: `[ts, °C]`, one point per `bucket` seconds at most.
    pub async fn history(&self, since: i64, bucket: i64) -> sqlx::Result<Vec<(i64, f64)>> {
        sqlx::query_as("SELECT (ts / ?) * ?, ROUND(AVG(temperature), 1) FROM outdoor_samples WHERE ts >= ? GROUP BY ts / ? ORDER BY 1")
            .bind(bucket)
            .bind(bucket)
            .bind(since)
            .bind(bucket)
            .fetch_all(&self.db)
            .await
    }

    /// Reads the weather every 30 minutes.
    pub async fn run(self: std::sync::Arc<Self>) {
        loop {
            if self.refresh().await.is_none() {
                tracing::debug!("no weather: no city for the home, or Open-Meteo unreachable");
            }
            tokio::time::sleep(EVERY).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono_tz::Europe::Paris;

    fn local(ts: i64) -> String {
        chrono::DateTime::from_timestamp(ts, 0).unwrap().with_timezone(&Paris).format("%H:%M").to_string()
    }

    #[test]
    fn sunrise_and_sunset_within_a_few_minutes() {
        // Paris, summer solstice: 05:47 and 21:58 (CEST).
        let s = sun(NaiveDate::from_ymd_opt(2024, 6, 21).unwrap(), 48.8566, 2.3522);
        let (rise, set) = (local(s.sunrise.unwrap()), local(s.sunset.unwrap()));
        assert!(("05:45".."05:50").contains(&rise.as_str()), "{rise}");
        assert!(("21:56".."22:01").contains(&set.as_str()), "{set}");
        // Toulouse, 15 November: about 07:48 and 17:29 (CET; solar noon about 12:39).
        let s = sun(NaiveDate::from_ymd_opt(2026, 11, 15).unwrap(), 43.6045, 1.444);
        let (rise, set) = (local(s.sunrise.unwrap()), local(s.sunset.unwrap()));
        assert!(("07:44".."07:54").contains(&rise.as_str()), "{rise}");
        assert!(("17:24".."17:34").contains(&set.as_str()), "{set}");
        // Svalbard in June: no sunset.
        assert_eq!(sun(NaiveDate::from_ymd_opt(2024, 6, 21).unwrap(), 78.2, 15.6), SunTimes { sunrise: None, sunset: None });
    }

    #[test]
    fn a_forecast_is_read() {
        let now = 1_000_000;
        let data = serde_json::json!({
            "current": {"temperature_2m": 14.2, "apparent_temperature": 12.9, "weather_code": 2, "precipitation": 0.0, "wind_speed_10m": 9.4},
            "hourly": {"time": [now - 7200, now - 1800, now + 1800, now + 90_000], "temperature_2m": [10.0, 13.5, 14.0, 9.0], "weather_code": [0, 1, 2, 61], "precipitation_probability": [0, 0, 10, 80]},
            "daily": {"temperature_2m_min": [8.0, 6.5], "temperature_2m_max": [18.0, 15.0], "weather_code": [2, 61]}
        });
        let r = report_of("Labège, Haute-Garonne", &data, now).unwrap();
        assert_eq!((r.temperature, r.code, r.today_min, r.today_max), (Some(14.2), Some(2), Some(8.0), Some(18.0)));
        assert_eq!((r.tomorrow_code, r.tomorrow_min), (Some(61), Some(6.5)));
        assert_eq!(r.hours.iter().map(|h| h.ts).collect::<Vec<_>>(), vec![now - 1800, now + 1800], "the hour under way and the next 24 hours");
    }
}
