//! The weather, without an account: the forecast of Open-Meteo (`weather.forecast@1`) and the
//! weather warnings of MeteoAlarm (`weather.warnings@1`, Météo-France's vigilance and its
//! neighbours'). Breezone keeps what was read and the outdoor temperatures.
//!
//!   GET https://api.open-meteo.com/v1/forecast?latitude&longitude&current&hourly&daily…
//!   GET https://feeds.meteoalarm.org/feeds/meteoalarm-legacy-atom-{country}  (Atom, CAP 1.2)

use breezone_module_sdk::{host, json, module, Error, Map, Result, Service, Value};

fn number(params: &Value, key: &str, limit: f64) -> Result<f64> {
    params[key].as_f64().filter(|v| v.abs() <= limit).ok_or_else(|| Error::Invalid(format!("`{key}`: a number expected")))
}

/// Open-Meteo's forecast at a place, the days in the place's own time zone.
fn open_meteo(params: &Value) -> Result<Value> {
    let (lat, lon) = (number(params, "lat", 90.0)?, number(params, "lon", 180.0)?);
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}\
         &current=temperature_2m,apparent_temperature,relative_humidity_2m,weather_code,precipitation,wind_speed_10m\
         &hourly=temperature_2m,weather_code,precipitation_probability&daily=temperature_2m_max,temperature_2m_min,weather_code\
         &timezone=auto&timeformat=unixtime&forecast_days=2"
    );
    let a = host::request("GET", &url, &[], None, 10_000)?;
    if !(200..300).contains(&a.status) {
        return Err(Error::Unreachable(format!("Open-Meteo: HTTP {}", a.status)));
    }
    a.json()
}

/// Weather now, today and tomorrow, from Open-Meteo's answer.
pub fn now_of(data: &Value) -> Value {
    let cur = &data["current"];
    let daily = |key: &str, i: usize| data["daily"][key].get(i).cloned().unwrap_or(Value::Null);
    let code = |v: Value| v.as_f64().map_or(Value::Null, |c| json!(c as i64));
    let mut out = Map::new();
    for (name, value) in [
        ("temperature", cur["temperature_2m"].clone()),
        ("apparent", cur["apparent_temperature"].clone()),
        ("humidity", cur["relative_humidity_2m"].clone()),
        ("code", code(cur["weather_code"].clone())),
        ("precipitation", cur["precipitation"].clone()),
        ("wind", cur["wind_speed_10m"].clone()),
        ("today_min", daily("temperature_2m_min", 0)),
        ("today_max", daily("temperature_2m_max", 0)),
        ("tomorrow_min", daily("temperature_2m_min", 1)),
        ("tomorrow_max", daily("temperature_2m_max", 1)),
        ("tomorrow_code", code(daily("weather_code", 1))),
    ] {
        if !value.is_null() {
            out.insert(name.into(), value);
        }
    }
    Value::Object(out)
}

/// The hour under way and the next 24 hours, from Open-Meteo's answer.
pub fn hours_of(data: &Value, now: i64) -> Value {
    let times: Vec<i64> = data["hourly"]["time"].as_array().into_iter().flatten().filter_map(Value::as_i64).collect();
    let hourly = |key: &str, i: usize| data["hourly"][key].get(i).and_then(Value::as_f64);
    Value::Array(
        times
            .iter()
            .enumerate()
            .filter(|(_, ts)| **ts + 3600 > now && **ts < now + 24 * 3600)
            .map(|(i, ts)| {
                let mut h = Map::new();
                h.insert("ts".into(), json!(ts));
                if let Some(t) = hourly("temperature_2m", i) {
                    h.insert("temperature".into(), json!(t));
                }
                if let Some(c) = hourly("weather_code", i) {
                    h.insert("code".into(), json!(c as i64));
                }
                if let Some(p) = hourly("precipitation_probability", i) {
                    h.insert("precipitation_probability".into(), json!(p));
                }
                Value::Object(h)
            })
            .collect(),
    )
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
pub fn vigilance_of(feed: &str, area: &str, now: i64) -> Value {
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
    let events: Vec<String> = found.iter().map(|(_, e)| e.clone()).collect();
    json!({"level": found.first().map_or(0, |(l, _)| *l), "events": events.join(","), "area": area})
}

fn warnings(params: &Value) -> Result<Value> {
    let (country, area) = (params["country"].as_str().unwrap_or_default(), params["area"].as_str().unwrap_or_default());
    let feed = meteoalarm_feed(country).ok_or_else(|| Error::Unsupported(format!("no warnings for {country}")))?;
    let a = host::request("GET", &format!("https://feeds.meteoalarm.org/feeds/meteoalarm-legacy-atom-{feed}"), &[], None, 20_000)?;
    if !(200..300).contains(&a.status) {
        return Err(Error::Unreachable(format!("MeteoAlarm: HTTP {}", a.status)));
    }
    Ok(vigilance_of(&String::from_utf8_lossy(&a.body), area, host::now()))
}

pub struct Weather;

impl Service for Weather {
    fn query(contract: &str, query: &str, params: Value) -> Result<Value> {
        match (contract, query) {
            ("weather.forecast@1", "now") => Ok(now_of(&open_meteo(&params)?)),
            ("weather.forecast@1", "hours") => Ok(hours_of(&open_meteo(&params)?, host::now())),
            ("weather.warnings@1", "current") => warnings(&params),
            _ => Err(Error::Unsupported(format!("{contract} {query}"))),
        }
    }
}

module!(service: Weather);

#[cfg(test)]
mod tests {
    use super::*;
    use breezone_module_sdk::testing;

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
        assert_eq!(vigilance_of(&feed, "Haute-Garonne", noon), json!({"level": 2, "events": "wind,rain-flood", "area": "Haute-Garonne"}), "orange wind, yellow rain; red not yet");
        assert_eq!(vigilance_of(&feed, "cote d'or", noon)["level"], 3, "names compared without accents or dashes");
        assert_eq!(vigilance_of(&feed, "Ariège", noon)["level"], 0, "nothing: green");
        assert_eq!(vigilance_of(&feed, "Haute-Garonne", noon + 86_400)["level"], 0, "over");
    }

    #[test]
    fn a_forecast_is_read() {
        let now = 1_000_000;
        let data = json!({
            "current": {"temperature_2m": 14.2, "apparent_temperature": 12.9, "weather_code": 2, "precipitation": 0.0, "wind_speed_10m": 9.4},
            "hourly": {"time": [now - 7200, now - 1800, now + 1800, now + 90_000], "temperature_2m": [10.0, 13.5, 14.0, 9.0], "weather_code": [0, 1, 2, 61], "precipitation_probability": [0, 0, 10, 80]},
            "daily": {"temperature_2m_min": [8.0, 6.5], "temperature_2m_max": [18.0, 15.0], "weather_code": [2, 61]}
        });
        let n = now_of(&data);
        assert_eq!((n["temperature"].clone(), n["code"].clone(), n["today_min"].clone(), n["tomorrow_code"].clone()), (json!(14.2), json!(2), json!(8.0), json!(61)));
        assert!(n.get("humidity").is_none(), "what is not given is left out");
        let hours = hours_of(&data, now);
        assert_eq!(hours.as_array().unwrap().iter().map(|h| h["ts"].as_i64().unwrap()).collect::<Vec<_>>(), vec![now - 1800, now + 1800]);
    }

    #[test]
    fn the_queries_go_to_open_meteo_and_meteoalarm() {
        testing::reset();
        testing::answer_json("GET", "api.open-meteo.com/v1/forecast", &json!({"current": {"temperature_2m": 3.5}, "hourly": {"time": []}, "daily": {}}));
        testing::answer("GET", "meteoalarm-legacy-atom-france", 200, b"<feed></feed>".to_vec());
        assert_eq!(Weather::query("weather.forecast@1", "now", json!({"lat": 45.9, "lon": 6.1})).unwrap(), json!({"temperature": 3.5}));
        assert!(testing::requests()[0].contains("latitude=45.9&longitude=6.1"));
        assert_eq!(Weather::query("weather.warnings@1", "current", json!({"country": "fr", "area": "Haute-Savoie"})).unwrap()["level"], 0);
        assert!(matches!(Weather::query("weather.warnings@1", "current", json!({"country": "US", "area": "x"})), Err(Error::Unsupported(_))));
        assert!(Weather::query("weather.forecast@1", "now", json!({"lat": 400})).is_err());
    }
}
