//! The context of each installation (roadmap F9): the sun, computed here without any network, and
//! the weather, read every 30 minutes from Open-Meteo (free, no account, Météo-France data for
//! France), with the weather warnings of its area (MeteoAlarm).
//!
//! The city of an installation is turned into coordinates once, when it is set (by hand, from the
//! Airzone account, from another node), and kept on the installation with its area and country:
//! only the city goes to Open-Meteo's geocoding, only the coordinates to its forecast. Installations
//! in the same place share one reading. The outdoor temperature of each is kept, so that the curves
//! of its zones can show it.

use crate::db::{self, Db};
use chrono::{Datelike, NaiveDate};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::f64::consts::PI;
use std::time::Duration;
use tokio::sync::RwLock;

const EVERY: Duration = Duration::from_secs(1800);

/// Sunrise and sunset of a day (timestamps); `None` in a polar day or night.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SunTimes {
    pub sunrise: Option<i64>,
    pub sunset: Option<i64>,
}

/// Sunrise and sunset at `lat`, `lon` on `day` (NOAA's sunrise equation, within a minute or two).
pub fn sun(day: NaiveDate, lat: f64, lon: f64) -> SunTimes {
    // The sun's centre 0.833° below the horizon: refraction and its radius.
    sun_at(day, lat, lon, -0.833)
}

/// Civil dawn and dusk: the sun 6° below the horizon (`sunrise` is the dawn, `sunset` the dusk).
pub fn twilight(day: NaiveDate, lat: f64, lon: f64) -> SunTimes {
    sun_at(day, lat, lon, -6.0)
}

/// When the sun's centre crosses `altitude` (degrees) on `day`, rising then setting.
fn sun_at(day: NaiveDate, lat: f64, lon: f64, altitude: f64) -> SunTimes {
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
    let cos_h = ((altitude * rad).sin() - (lat * rad).sin() * decl.sin()) / ((lat * rad).cos() * decl.cos());
    if !(-1.0..=1.0).contains(&cos_h) {
        return SunTimes { sunrise: None, sunset: None };
    }
    let h = cos_h.acos() / rad / 360.0;
    let ts = |j: f64| ((j - 2_440_587.5) * 86_400.0).round() as i64;
    SunTimes { sunrise: Some(ts(transit - h)), sunset: Some(ts(transit + h)) }
}

/// Where the sun is at `ts` seen from `lat`, `lon`: (elevation, azimuth) in degrees, the azimuth
/// from the north, clockwise (90 east, 180 south). Within a degree, enough for a façade.
pub fn position(ts: i64, lat: f64, lon: f64) -> (f64, f64) {
    let rad = PI / 180.0;
    let n = ts as f64 / 86_400.0 + 2_440_587.5 - 2_451_545.0;
    let mean = (280.460 + 0.985_647_4 * n).rem_euclid(360.0);
    let g = ((357.528 + 0.985_600_3 * n).rem_euclid(360.0)) * rad;
    let lambda = (mean + 1.915 * g.sin() + 0.020 * (2.0 * g).sin()) * rad;
    let eps = (23.439 - 0.000_000_4 * n) * rad;
    let ra = (eps.cos() * lambda.sin()).atan2(lambda.cos());
    let decl = (eps.sin() * lambda.sin()).asin();
    let gmst = (18.697_374_558 + 24.065_709_824_419_08 * n).rem_euclid(24.0);
    let hour_angle = ((gmst * 15.0 + lon) * rad - ra + PI).rem_euclid(2.0 * PI) - PI;
    let lat = lat * rad;
    let elevation = (lat.sin() * decl.sin() + lat.cos() * decl.cos() * hour_angle.cos()).asin();
    let azimuth = (-hour_angle.sin() * decl.cos()).atan2(decl.sin() * lat.cos() - decl.cos() * lat.sin() * hour_angle.cos());
    (elevation / rad, (azimuth / rad).rem_euclid(360.0))
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
    /// Relative humidity outside (%).
    #[serde(default)]
    pub humidity: Option<f64>,
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
    /// The weather warnings of the home's area in force now (MeteoAlarm), when its country has some.
    #[serde(default)]
    pub vigilance: Option<Vigilance>,
}

/// The weather warnings in force for an area: Météo-France's vigilance, relayed by MeteoAlarm.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Vigilance {
    /// 0 green, 1 yellow, 2 orange, 3 red: the highest warning in force.
    pub level: u8,
    /// What the warnings are about ("Wind", "Rain-flood"…), the highest first.
    pub events: Vec<String>,
    /// The area they are for ("Haute-Savoie").
    pub area: String,
}

/// The MeteoAlarm feed of a country (ISO 3166-1 alpha-2), for the countries it covers.
fn meteoalarm_feed(country: &str) -> Option<&'static str> {
    Some(match country.to_ascii_uppercase().as_str() {
        "FR" => "france",
        "BE" => "belgium",
        "CH" => "switzerland",
        "ES" => "spain",
        "IT" => "italy",
        "DE" => "germany",
        "PT" => "portugal",
        "LU" => "luxembourg",
        "NL" => "netherlands",
        "AT" => "austria",
        _ => return None,
    })
}

/// "Côte-d'Or" and "cote d or" are the same area.
fn same_area(a: &str) -> String {
    let plain: String = a
        .chars()
        .map(|c| match c {
            'à' | 'â' | 'ä' | 'á' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' | 'í' => 'i',
            'ô' | 'ö' | 'ó' => 'o',
            'ù' | 'û' | 'ü' | 'ú' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            '-' | '\'' | '’' => ' ',
            c => c,
        })
        .collect::<String>()
        .to_lowercase();
    plain.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The text of the first `<tag>…</tag>` in `xml`.
fn tag<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&format!("</{tag}>"))? + start;
    Some(xml[start..end].trim())
}

/// The warnings of `area` in force at `now`, from a MeteoAlarm Atom feed (CAP 1.2): the colour is
/// its severity (Moderate yellow, Severe orange, Extreme red), else the colour in its title.
pub fn vigilance_of(feed: &str, area: &str, now: i64) -> Vigilance {
    let wanted = same_area(area);
    let mut found: Vec<(u8, String)> = vec![];
    for entry in feed.split("<entry>").skip(1) {
        if tag(entry, "cap:areaDesc").map(same_area).as_deref() != Some(wanted.as_str()) {
            continue;
        }
        let time = |t: &str| tag(entry, t).and_then(|v| chrono::DateTime::parse_from_rfc3339(v).ok()).map(|d| d.timestamp());
        let from = time("cap:onset").or(time("cap:effective")).unwrap_or(i64::MIN);
        let until = time("cap:expires").unwrap_or(i64::MAX);
        if now < from || now >= until {
            continue;
        }
        let title = tag(entry, "title").unwrap_or_default().to_lowercase();
        let level = match tag(entry, "cap:severity") {
            Some("Extreme") => 3,
            Some("Severe") => 2,
            Some("Moderate") => 1,
            _ if title.starts_with("red") => 3,
            _ if title.starts_with("orange") => 2,
            _ if title.starts_with("yellow") => 1,
            _ => 0,
        };
        let event =
            tag(entry, "cap:event").unwrap_or_default().trim_end_matches(" warning").trim_start_matches("Moderate ").trim_start_matches("Severe ").trim_start_matches("Extreme ");
        if level > 0 && !found.iter().any(|(_, e)| e == event) {
            found.push((level, event.to_string()));
        }
    }
    found.sort_by_key(|(level, _)| std::cmp::Reverse(*level));
    Vigilance { level: found.first().map_or(0, |(l, _)| *l), events: found.into_iter().map(|(_, e)| e).collect(), area: area.to_string() }
}

/// What the weather page gets: the weather (if any) and the sun of today.
#[derive(Debug, Clone, Serialize)]
pub struct View {
    pub weather: Option<Report>,
    pub sun: Option<SunTimes>,
    /// This server reads the weather (`BREEZONE_WEATHER`); the sun is computed anyway.
    pub online: bool,
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
        humidity: num(&cur["relative_humidity_2m"]),
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
        vigilance: None,
    })
}

/// A report from the answers of a module (`weather.forecast@1`: `now` and `hours`).
pub fn report_from_service(place: &str, now: &Value, hours: Vec<Hour>, read_at: i64) -> Report {
    let n = |k: &str| now[k].as_f64();
    let i = |k: &str| now[k].as_i64();
    Report {
        place: place.to_string(),
        temperature: n("temperature"),
        apparent: n("apparent"),
        humidity: n("humidity"),
        code: i("code"),
        precipitation: n("precipitation"),
        wind: n("wind"),
        today_min: n("today_min"),
        today_max: n("today_max"),
        tomorrow_min: n("tomorrow_min"),
        tomorrow_max: n("tomorrow_max"),
        tomorrow_code: i("tomorrow_code"),
        hours,
        read_at,
        vigilance: None,
    }
}

/// Where an installation is, as found when its city was set.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub installation_id: i64,
    /// Its city ("Annecy, Haute-Savoie").
    pub city: String,
    pub lat: f64,
    pub lon: f64,
    /// Its area ("Haute-Savoie") and country ("FR"), for the weather warnings.
    pub area: Option<String>,
    pub country: Option<String>,
}

/// What a city was looked up with: its name and its country, as set.
fn geo_key(location: &str, country: Option<&str>) -> String {
    format!("{}#{}", location.trim(), country.unwrap_or("").trim().to_ascii_uppercase())
}

pub struct Weather {
    db: Db,
    tz: Tz,
    http: reqwest::Client,
    /// The weather and the cities are looked up on the internet (`BREEZONE_WEATHER`).
    online: bool,
    /// The last weather of each installation.
    last: RwLock<HashMap<i64, Report>>,
}

impl Weather {
    pub fn new(db: Db, tz: Tz, online: bool) -> Self {
        let http = reqwest::Client::builder().timeout(Duration::from_secs(10)).user_agent("Breezone").build().unwrap_or_default();
        if !online {
            tracing::info!("weather off: nothing is asked of Open-Meteo or MeteoAlarm (BREEZONE_WEATHER=1 to turn it on)");
        }
        Self { db, tz, http, online, last: RwLock::new(HashMap::new()) }
    }

    /// Looks up the cities set since the last time (once each): their coordinates, area and country
    /// are kept on the installation.
    pub async fn locate(&self) {
        let installations = match db::list_installations(&self.db).await {
            Ok(list) => list,
            Err(e) => return tracing::warn!("cannot read the installations: {e}"),
        };
        for inst in installations {
            let Some(city) = inst.location.as_deref().map(str::trim).filter(|l| !l.is_empty()) else { continue };
            let key = geo_key(city, inst.country.as_deref());
            if inst.geo_key.as_deref() == Some(key.as_str()) {
                continue;
            }
            let found = self.geocode(city, inst.country.as_deref()).await;
            // Not found (or no network): asked again at the next reading.
            let Some((lat, lon, area, country)) = found else { continue };
            if let Err(e) = db::set_installation_geo(&self.db, inst.id, &key, Some((lat, lon)), area.as_deref(), country.as_deref()).await {
                tracing::warn!("cannot keep where installation {} is: {e}", inst.id);
            }
        }
    }

    /// Where `city` is (in `country` when known), from Open-Meteo's geocoding: (latitude,
    /// longitude, area, country).
    async fn geocode(&self, city: &str, country: Option<&str>) -> Option<(f64, f64, Option<String>, Option<String>)> {
        // "Annecy, Haute-Savoie": the city is asked, the region helps to choose.
        let (name, region) = city.split_once(',').map_or((city.trim(), ""), |(c, r)| (c.trim(), r.trim()));
        let answer: Value = self
            .http
            .get("https://geocoding-api.open-meteo.com/v1/search")
            .query(&[("name", name), ("count", "10"), ("language", "fr"), ("format", "json")])
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
        // The French "département" is admin2.
        let area = best["admin2"].as_str().or(best["admin1"].as_str()).map(str::to_string);
        let code = best["country_code"].as_str().map(str::to_string).or(country.map(str::to_string));
        Some((best["latitude"].as_f64()?, best["longitude"].as_f64()?, area, code))
    }

    /// The installations whose place is known, those of this server first.
    pub async fn places(&self) -> Vec<Place> {
        let mut list = db::list_installations(&self.db).await.unwrap_or_default();
        list.sort_by_key(|i| (i.node_id.is_some(), i.id));
        list.into_iter()
            .filter_map(|i| {
                Some(Place {
                    installation_id: i.id,
                    city: i.location.clone().filter(|l| !l.trim().is_empty())?,
                    lat: i.latitude?,
                    lon: i.longitude?,
                    area: i.area.clone(),
                    country: i.geo_country.clone().or(i.country.clone()),
                })
            })
            .collect()
    }

    /// The place of an installation, or (none given, or not located) that of the home: the first
    /// installation of this server with a city.
    pub async fn place(&self, installation_id: Option<i64>) -> Option<Place> {
        let places = self.places().await;
        installation_id.and_then(|id| places.iter().find(|p| p.installation_id == id).cloned()).or_else(|| places.into_iter().next())
    }

    /// The warnings in force for `area` (`None`: its country has no feed, or it does not answer):
    /// from a module that gives them first.
    async fn vigilance(&self, area: &str, country: &str) -> Option<Vigilance> {
        let asked = serde_json::json!({"country": country, "area": area});
        if let Some(Ok(v)) = crate::modules::ask_service(&self.db, "weather.warnings@1", "current", &asked).await {
            let events = v["events"].as_str().unwrap_or_default().split(',').map(str::trim).filter(|e| !e.is_empty()).map(str::to_string).collect();
            return Some(Vigilance { level: v["level"].as_u64().unwrap_or(0).min(3) as u8, events, area: v["area"].as_str().unwrap_or(area).to_string() });
        }
        let feed = meteoalarm_feed(country)?;
        let url = format!("https://feeds.meteoalarm.org/feeds/meteoalarm-legacy-atom-{feed}");
        let text = self.http.get(url).timeout(Duration::from_secs(20)).send().await.ok()?.error_for_status().ok()?.text().await.ok()?;
        Some(vigilance_of(&text, area, db::now()))
    }

    /// The forecast at a place, from a module that gives it (then the core asks Open-Meteo itself).
    async fn forecast(&self, place: &Place) -> Option<Report> {
        let at = serde_json::json!({"lat": place.lat, "lon": place.lon});
        if let Some(Ok(now)) = crate::modules::ask_service(&self.db, "weather.forecast@1", "now", &at).await {
            let hours = match crate::modules::ask_service(&self.db, "weather.forecast@1", "hours", &at).await {
                Some(Ok(list)) => serde_json::from_value(list).unwrap_or_default(),
                _ => vec![],
            };
            return Some(report_from_service(&place.city, &now, hours, db::now()));
        }
        let data: Value = self
            .http
            .get("https://api.open-meteo.com/v1/forecast")
            .query(&[
                ("latitude", place.lat.to_string()),
                ("longitude", place.lon.to_string()),
                ("current", "temperature_2m,apparent_temperature,relative_humidity_2m,weather_code,precipitation,wind_speed_10m".into()),
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
        report_of(&place.city, &data, db::now())
    }

    /// The weather is read: this server may go on the internet, and the service is switched on
    /// (Settings › Accounts and services).
    pub async fn enabled(&self) -> bool {
        self.online && db::account_enabled(&self.db, crate::accounts::WEATHER).await
    }

    /// Reads the weather of every installation now (one reading per place); keeps their outdoor
    /// temperature. Switched off, what was read goes.
    pub async fn refresh(&self) {
        if !self.enabled().await {
            self.last.write().await.clear();
            return;
        }
        self.locate().await;
        // Installations a few hundred metres apart share a reading.
        let mut read: HashMap<(i64, i64), Option<Report>> = HashMap::new();
        let mut warnings: HashMap<(String, String), Option<Vigilance>> = HashMap::new();
        let mut reports = HashMap::new();
        for place in self.places().await {
            let spot = ((place.lat * 100.0).round() as i64, (place.lon * 100.0).round() as i64);
            if let Entry::Vacant(e) = read.entry(spot) {
                e.insert(self.forecast(&place).await);
            }
            let Some(mut report) = read[&spot].clone() else { continue };
            report.place = place.city.clone();
            if let (Some(area), Some(country)) = (&place.area, &place.country) {
                let key = (area.clone(), country.clone());
                if let Entry::Vacant(e) = warnings.entry(key.clone()) {
                    e.insert(self.vigilance(area, country).await);
                }
                report.vigilance = warnings[&key].clone();
            }
            if let Some(t) = report.temperature {
                let kept = sqlx::query("INSERT OR REPLACE INTO outdoor_samples (installation_id, ts, temperature) VALUES (?, ?, ?)")
                    .bind(place.installation_id)
                    .bind(db::now())
                    .bind(t)
                    .execute(&self.db)
                    .await;
                if let Err(e) = kept {
                    tracing::warn!("cannot keep the outdoor temperature: {e}");
                }
            }
            reports.insert(place.installation_id, report);
        }
        let state = match (read.is_empty(), read.values().any(Option::is_some)) {
            (true, _) => None,
            (false, true) => Some(("ok", None)),
            (false, false) => Some(("error", Some(crate::text!("Open-Meteo ne répond pas", "Open-Meteo does not answer", "Open-Meteo no responde").store()))),
        };
        if let Some((s, error)) = state {
            if let Err(e) = db::set_account_state(&self.db, crate::accounts::WEATHER, s, error.as_deref()).await {
                tracing::warn!("cannot note how the weather went: {e}");
            }
        }
        // A reading that failed keeps the last one.
        let mut last = self.last.write().await;
        last.retain(|id, _| !reports.contains_key(id));
        last.extend(reports);
    }

    /// The last weather of each installation read.
    pub async fn reports(&self) -> HashMap<i64, Report> {
        self.last.read().await.clone()
    }

    /// The weather of an installation (or of the home) and the sun of today there.
    pub async fn view(&self, installation_id: Option<i64>) -> View {
        let online = self.enabled().await;
        let Some(place) = self.place(installation_id).await else { return View { weather: None, sun: None, online } };
        let weather = self.last.read().await.get(&place.installation_id).cloned();
        let today = chrono::Utc::now().with_timezone(&self.tz).date_naive();
        View { weather, sun: Some(sun(today, place.lat, place.lon)), online }
    }

    /// The outdoor temperature of an installation (or of the home) since `since`: `[ts, °C]`, one
    /// point per `bucket` seconds at most.
    pub async fn history(&self, installation_id: Option<i64>, since: i64, bucket: i64) -> sqlx::Result<Vec<(i64, f64)>> {
        let Some(place) = self.place(installation_id).await else { return Ok(vec![]) };
        sqlx::query_as("SELECT (ts / ?) * ?, ROUND(AVG(temperature), 1) FROM outdoor_samples WHERE installation_id = ? AND ts >= ? GROUP BY ts / ? ORDER BY 1")
            .bind(bucket)
            .bind(bucket)
            .bind(place.installation_id)
            .bind(since)
            .bind(bucket)
            .fetch_all(&self.db)
            .await
    }

    /// Reads the weather every 30 minutes.
    pub async fn run(self: std::sync::Arc<Self>) {
        loop {
            self.refresh().await;
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
    fn dawn_dusk_and_where_the_sun_is() {
        // Toulouse, 15 November: civil dawn about 07:17, dusk about 17:59 (CET).
        let t = twilight(NaiveDate::from_ymd_opt(2026, 11, 15).unwrap(), 43.6045, 1.444);
        let (dawn, dusk) = (local(t.sunrise.unwrap()), local(t.sunset.unwrap()));
        assert!(("07:12".."07:22").contains(&dawn.as_str()), "{dawn}");
        assert!(("17:54".."18:04").contains(&dusk.as_str()), "{dusk}");
        // Solar noon there is about 12:39: the sun is south, 27° high (90 − 43.6 − 18.6 of declination).
        let noon = chrono::NaiveDate::from_ymd_opt(2026, 11, 15).unwrap().and_hms_opt(11, 39, 0).unwrap().and_utc().timestamp();
        let (elevation, azimuth) = position(noon, 43.6045, 1.444);
        assert!((elevation - 27.8).abs() < 1.5, "{elevation}");
        assert!((azimuth - 180.0).abs() < 2.0, "{azimuth}");
        // At 09:00 it is south-east and low.
        let (elevation, azimuth) = position(noon - 3 * 3600 - 39 * 60, 43.6045, 1.444);
        assert!((5.0..20.0).contains(&elevation) && (120.0..150.0).contains(&azimuth), "{elevation} {azimuth}");
    }

    #[test]
    fn the_warnings_of_the_area() {
        let entry = |area: &str, severity: &str, event: &str, onset: &str, expires: &str| {
            format!(
                "<entry><title>Yellow {event} issued for France - {area}</title><cap:areaDesc>{area}</cap:areaDesc><cap:event>{severity} {event}</cap:event>\
                 <cap:severity>{severity}</cap:severity><cap:onset>{onset}</cap:onset><cap:expires>{expires}</cap:expires></entry>"
            )
        };
        let feed = [
            entry("Haute-Garonne", "Moderate", "rain-flood warning", "2026-11-15T06:00:00+00:00", "2026-11-16T06:00:00+00:00"),
            entry("Haute-Garonne", "Severe", "wind warning", "2026-11-15T10:00:00+00:00", "2026-11-15T20:00:00+00:00"),
            entry("Haute-Garonne", "Extreme", "flooding warning", "2026-11-17T10:00:00+00:00", "2026-11-18T20:00:00+00:00"),
            entry("Côte-d'Or", "Extreme", "snow-ice warning", "2026-11-15T00:00:00+00:00", "2026-11-16T00:00:00+00:00"),
        ]
        .join("");
        let noon = chrono::DateTime::parse_from_rfc3339("2026-11-15T12:00:00+00:00").unwrap().timestamp();
        let v = vigilance_of(&feed, "Haute-Garonne", noon);
        assert_eq!((v.level, v.events.clone()), (2, vec!["wind".to_string(), "rain-flood".to_string()]), "orange wind, yellow rain; red not yet");
        assert_eq!(vigilance_of(&feed, "cote d'or", noon).level, 3, "names compared without accents or dashes");
        assert_eq!(vigilance_of(&feed, "Ariège", noon).level, 0, "nothing: green");
        assert_eq!(vigilance_of(&feed, "Haute-Garonne", noon + 86_400).level, 0, "over");
    }

    #[test]
    fn a_report_from_a_module() {
        let now = serde_json::json!({"temperature": 3.5, "code": 61, "tomorrow_max": 9.0});
        let hours: Vec<Hour> = serde_json::from_value(serde_json::json!([{"ts": 10, "temperature": 3.0}, {"ts": 3610, "code": 2}])).unwrap();
        let r = report_from_service("Annecy", &now, hours, 5);
        assert_eq!((r.temperature, r.code, r.tomorrow_max, r.humidity), (Some(3.5), Some(61), Some(9.0), None));
        assert_eq!((r.hours.len(), r.hours[1].code, r.read_at), (2, Some(2), 5));
    }

    #[test]
    fn a_forecast_is_read() {
        let now = 1_000_000;
        let data = serde_json::json!({
            "current": {"temperature_2m": 14.2, "apparent_temperature": 12.9, "weather_code": 2, "precipitation": 0.0, "wind_speed_10m": 9.4},
            "hourly": {"time": [now - 7200, now - 1800, now + 1800, now + 90_000], "temperature_2m": [10.0, 13.5, 14.0, 9.0], "weather_code": [0, 1, 2, 61], "precipitation_probability": [0, 0, 10, 80]},
            "daily": {"temperature_2m_min": [8.0, 6.5], "temperature_2m_max": [18.0, 15.0], "weather_code": [2, 61]}
        });
        let r = report_of("Annecy, Haute-Savoie", &data, now).unwrap();
        assert_eq!((r.temperature, r.code, r.today_min, r.today_max), (Some(14.2), Some(2), Some(8.0), Some(18.0)));
        assert_eq!((r.tomorrow_code, r.tomorrow_min), (Some(61), Some(6.5)));
        assert_eq!(r.hours.iter().map(|h| h.ts).collect::<Vec<_>>(), vec![now - 1800, now + 1800], "the hour under way and the next 24 hours");
    }
}
