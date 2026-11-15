//! Renson Healthbox 3.0 ventilation unit, through its local HTTP API (no password, no cloud).
//!
//! * `GET /v2/api/data/current`: the rooms (one per valve), their air flow and profile, and the
//!   global air quality index with the room most in demand and its cause.
//! * `GET /v2/device`: the fan (total flow, power).
//! * `GET|PUT /v2/api/boost/{room}`: the boost of a room (`{"enable", "level", "timeout"}`, seconds).
//! * `PUT /v2/api/data/current/room/{room}/profile_name`: the profile, as a JSON string.
//! * `POST /v2/api/api_key` (a JSON string), `GET /v2/api/api_key/status`: a key Renson gives on
//!   request unlocks the measures of each room (humidity, CO₂, VOC, temperature). The unit keeps it:
//!   Breezone does not store it.
//!
//! Devices: `unit`, the whole house (`ventilation.unit@1`), and `room:{id}`, one per room
//! (`ventilation.room@1`).

use super::{ConfigField, Described, DeviceInfo, Driver, DriverError, DriverFuture, DriverInfo, HardwareRoom, State};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::time::Duration;

pub const UNIT: &str = "unit";
const ROOM: &str = "room:";
/// The rooms' boost when the unit has no level of its own.
const BOOST_LEVEL: f64 = 100.0;

/// What answers `/v2/api/data/current` at an address.
#[derive(Debug, Clone, PartialEq)]
pub struct Identified {
    pub serial: String,
    pub model: String,
    pub rooms: usize,
}

pub fn identify(data: &Value) -> Option<Identified> {
    let kind = data["device_type"].as_str()?;
    if !kind.starts_with("HEALTHBOX") || !data["room"].is_object() {
        return None;
    }
    Some(Identified {
        serial: data["serial"].as_str().unwrap_or_default().to_string(),
        model: format!("Renson {}", data["description"].as_str().unwrap_or("Healthbox")),
        rooms: data["room"].as_object().map_or(0, Map::len),
    })
}

/// A Healthbox at `host`, if it is one (the network scanner).
pub async fn probe(host: &str) -> Option<Identified> {
    let http = reqwest::Client::builder().timeout(Duration::from_secs(2)).build().ok()?;
    let data: Value = http.get(format!("http://{host}/v2/api/data/current")).send().await.ok()?.json().await.ok()?;
    identify(&data)
}

pub fn info() -> DriverInfo {
    DriverInfo {
        id: "renson",
        label: "Renson Healthbox",
        provides: vec!["ventilation.unit@1", "ventilation.room@1"],
        models: vec!["Renson Healthbox 3.0".into()],
        config: vec![ConfigField { name: "host", kind: "string", optional: false, label: crate::text!("Adresse IP ou nom", "IP address or name", "Dirección IP o nombre") }],
    }
}

pub struct Renson {
    http: reqwest::Client,
    base: String,
    /// Serial number expected at this address (from the scanner): another unit is refused.
    device_id: Option<String>,
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64().filter(|f| f.is_finite())
}

/// The rooms, sorted by their number.
fn rooms(data: &Value) -> Vec<(String, &Value)> {
    let mut rooms: Vec<(String, &Value)> = data["room"].as_object().map(|o| o.iter().map(|(id, r)| (id.clone(), r)).collect()).unwrap_or_default();
    rooms.sort_by_key(|(id, _)| id.parse::<u32>().unwrap_or(u32::MAX));
    rooms
}

/// The air flow of a room: the sum of its valves.
fn room_flow(room: &Value) -> f64 {
    room["actuator"].as_array().map_or(0.0, |a| a.iter().filter_map(|v| num(&v["parameter"]["flow_rate"]["value"])).sum())
}

/// What the unit calls a pollutant, as `ventilation.unit@1` names it.
fn pollutant(name: &str) -> &'static str {
    let name = name.to_ascii_lowercase();
    if name.contains("humidity") {
        "humidity"
    } else if name.contains("co2") {
        "co2"
    } else if name.contains("volatile") || name.contains("voc") {
        "voc"
    } else {
        "other"
    }
}

/// The measures of a room's sensors (given only once the API key is accepted).
fn measures(room: &Value, s: &mut State) {
    for sensor in room["sensor"].as_array().into_iter().flatten() {
        let kind = sensor["type"].as_str().unwrap_or_default();
        let (field, param) = match kind {
            k if k.contains("relative humidity") => ("humidity", "humidity"),
            k if k.contains("temperature") => ("temperature", "temperature"),
            k if k.contains("CO2") => ("co2", "concentration"),
            k if k.contains("volatile organic") => ("voc", "concentration"),
            _ => continue,
        };
        if let Some(v) = num(&sensor["parameter"][param]["value"]) {
            s.entry(field).or_insert(json!(v));
        }
    }
}

/// The state of every device, from `/v2/api/data/current`, `/v2/device` and each room's boost.
pub fn states_of(data: &Value, fan: Option<&Value>, boosts: &HashMap<String, Value>, sensors: bool) -> HashMap<String, State> {
    let mut out = HashMap::new();
    let mut boosting = false;
    let mut total = 0.0;
    for (id, room) in rooms(data) {
        let mut s = State::new();
        let flow = room_flow(room);
        total += flow;
        s.insert("flow".into(), json!(flow));
        if let Some(nominal) = num(&room["parameter"]["nominal"]["value"]).filter(|n| *n > 0.0) {
            let offset = num(&room["parameter"]["offset"]["value"]).unwrap_or(0.0);
            s.insert("nominal".into(), json!(nominal));
            s.insert("level".into(), json!((flow / (nominal + offset).max(1.0) * 100.0).round() as i64));
        }
        if let Some(p) = room["profile_name"].as_str().map(str::to_ascii_lowercase).filter(|p| ["eco", "health", "intense"].contains(&p.as_str())) {
            s.insert("profile".into(), json!(p));
        }
        let boost = boosts.get(&id);
        let on = boost.and_then(|b| b["enable"].as_bool()).unwrap_or(false);
        boosting |= on;
        s.insert("boost".into(), json!(on));
        if let Some(r) = boost.and_then(|b| num(&b["remaining"])).filter(|_| on) {
            s.insert("boost_remaining".into(), json!(r.round() as i64));
        }
        measures(room, &mut s);
        out.insert(format!("{ROOM}{id}"), s);
    }
    let mut unit = State::new();
    let fan_flow = fan.and_then(|f| num(&f["fan"]["flow"]));
    unit.insert("flow".into(), json!(fan_flow.unwrap_or(total)));
    if let Some(p) = fan.and_then(|f| num(&f["power"]).or_else(|| num(&f["fan"]["power"]))) {
        unit.insert("power".into(), json!(p));
    }
    if let Some(aqi) = data["sensor"].as_array().into_iter().flatten().find(|s| s["type"] == "global air quality index") {
        let p = &aqi["parameter"];
        if let Some(i) = num(&p["index"]["value"]) {
            unit.insert("air_quality_index".into(), json!(i));
        }
        if let Some(m) = p["main_pollutant"]["value"].as_str().filter(|m| !m.is_empty()) {
            unit.insert("main_pollutant".into(), json!(pollutant(m)));
        }
        if let Some(r) = p["room"]["value"].as_str().filter(|r| !r.is_empty()) {
            unit.insert("worst_room".into(), json!(r));
        }
    }
    unit.insert("boost".into(), json!(boosting));
    unit.insert("sensors".into(), json!(sensors));
    out.insert(UNIT.into(), unit);
    out
}

impl Renson {
    pub fn new(config: &Value) -> Result<Self, String> {
        let host = config["host"].as_str().map(str::trim).unwrap_or_default();
        if !super::shelly::valid_host(host) {
            return Err("invalid Healthbox address".into());
        }
        let http = reqwest::Client::builder().timeout(Duration::from_secs(8)).build().map_err(|e| e.to_string())?;
        let device_id = config["device_id"].as_str().map(str::trim).filter(|d| !d.is_empty()).map(str::to_string);
        Ok(Self { http, base: format!("http://{host}"), device_id })
    }

    fn unreachable(e: reqwest::Error) -> DriverError {
        DriverError::Unreachable(if e.is_timeout() { "timed out".into() } else { e.to_string() })
    }

    async fn get(&self, path: &str) -> Result<Value, DriverError> {
        let resp = self.http.get(format!("{}{path}", self.base)).send().await.map_err(Self::unreachable)?;
        if !resp.status().is_success() {
            return Err(DriverError::Invalid(format!("{path}: HTTP {}", resp.status())));
        }
        resp.json().await.map_err(|e| DriverError::Invalid(format!("{path}: {e}")))
    }

    async fn send(&self, method: reqwest::Method, path: &str, body: &Value) -> Result<(), DriverError> {
        let resp = self.http.request(method, format!("{}{path}", self.base)).json(body).send().await.map_err(Self::unreachable)?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(DriverError::Invalid(format!("{path}: HTTP {status} {}", text.trim())));
        }
        Ok(())
    }

    /// The unit accepts the measures of its rooms (a valid API key was given).
    async fn sensors(&self) -> bool {
        self.get("/v2/api/api_key/status").await.is_ok_and(|s| s["state"] == "valid")
    }

    async fn room_ids(&self) -> Result<Vec<String>, DriverError> {
        Ok(rooms(&self.get("/v2/api/data/current").await?).into_iter().map(|(id, _)| id).collect())
    }

    async fn boost(&self, room: &str, minutes: i64) -> Result<(), DriverError> {
        let current = self.get(&format!("/v2/api/boost/{room}")).await.unwrap_or(Value::Null);
        let level = num(&current["default_level"]).filter(|l| *l > 0.0).unwrap_or(BOOST_LEVEL);
        self.send(reqwest::Method::PUT, &format!("/v2/api/boost/{room}"), &json!({"enable": true, "level": level, "timeout": minutes * 60})).await
    }

    async fn stop_boost(&self, room: &str) -> Result<(), DriverError> {
        self.send(reqwest::Method::PUT, &format!("/v2/api/boost/{room}"), &json!({"enable": false})).await
    }

    /// Gives the unit the API key that unlocks the measures of its rooms; true once it accepts it.
    pub async fn unlock(&self, key: &str) -> Result<bool, DriverError> {
        if self.sensors().await {
            return Ok(true);
        }
        self.send(reqwest::Method::POST, "/v2/api/api_key", &json!(key.trim())).await?;
        // The unit checks the key for a few seconds.
        for _ in 0..6 {
            tokio::time::sleep(Duration::from_secs(2)).await;
            if self.sensors().await {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

fn minutes(params: &State) -> Result<i64, DriverError> {
    params.get("minutes").and_then(Value::as_i64).ok_or_else(|| DriverError::Invalid("`minutes` is required".into()))
}

impl Driver for Renson {
    fn describe(&self) -> DriverFuture<'_, Described> {
        Box::pin(async move {
            let data = self.get("/v2/api/data/current").await?;
            let found = identify(&data).ok_or_else(|| DriverError::Invalid("not a Renson Healthbox".into()))?;
            if self.device_id.as_ref().is_some_and(|expected| *expected != found.serial) {
                return Err(DriverError::Invalid(format!("another unit answers at this address ({}): scan the network again", found.serial)));
            }
            let firmware = self.get("/v1/global").await.ok().and_then(|g| g["firmware version"].as_str().map(str::to_string));
            let mut devices = vec![DeviceInfo { reference: UNIT.into(), kind: "ventilation.unit".into(), traits: vec!["ventilation.unit@1".into()], name: "VMC".into() }];
            devices.extend(rooms(&data).into_iter().map(|(id, r)| DeviceInfo {
                reference: format!("{ROOM}{id}"),
                kind: "ventilation.room".into(),
                traits: vec!["ventilation.room@1".into()],
                name: r["name"].as_str().filter(|n| !n.trim().is_empty()).map_or_else(|| format!("Bouche {id}"), str::to_string),
            }));
            Ok(Described { model: found.model, firmware, identity: (!found.serial.is_empty()).then_some(found.serial), devices })
        })
    }

    fn read(&self) -> DriverFuture<'_, HashMap<String, State>> {
        Box::pin(async move {
            let data = self.get("/v2/api/data/current").await?;
            let fan = self.get("/v2/device").await.ok();
            let mut boosts = HashMap::new();
            for (id, _) in rooms(&data) {
                if let Ok(b) = self.get(&format!("/v2/api/boost/{id}")).await {
                    boosts.insert(id, b);
                }
            }
            Ok(states_of(&data, fan.as_ref(), &boosts, self.sensors().await))
        })
    }

    fn act<'a>(&'a self, reference: &'a str, trait_name: &'a str, action: &'a str, params: &'a State) -> DriverFuture<'a, ()> {
        Box::pin(async move {
            match (reference.strip_prefix(ROOM), trait_name, action) {
                (Some(room), "ventilation.room", "boost") => self.boost(room, minutes(params)?).await,
                (Some(room), "ventilation.room", "stop_boost") => self.stop_boost(room).await,
                (Some(room), "ventilation.room", "set_profile") => {
                    let profile = params.get("profile").and_then(Value::as_str).ok_or_else(|| DriverError::Invalid("`profile` is required".into()))?;
                    self.send(reqwest::Method::PUT, &format!("/v2/api/data/current/room/{room}/profile_name"), &json!(profile)).await
                }
                (None, "ventilation.unit", "boost") if reference == UNIT => {
                    let minutes = minutes(params)?;
                    for room in self.room_ids().await? {
                        self.boost(&room, minutes).await?;
                    }
                    Ok(())
                }
                (None, "ventilation.unit", "stop_boost") if reference == UNIT => {
                    for room in self.room_ids().await? {
                        self.stop_boost(&room).await?;
                    }
                    Ok(())
                }
                _ => Err(DriverError::Unsupported(format!("{reference}: {trait_name}.{action}"))),
            }
        })
    }

    fn read_every(&self) -> Duration {
        Duration::from_secs(30)
    }

    /// Each room of the unit, as named in the Renson app.
    fn rooms(&self) -> DriverFuture<'_, Vec<HardwareRoom>> {
        Box::pin(async move {
            let data = self.get("/v2/api/data/current").await?;
            Ok(rooms(&data)
                .into_iter()
                .filter_map(|(id, r)| {
                    r["name"].as_str().filter(|n| !n.trim().is_empty()).map(|n| HardwareRoom { name: n.trim().to_string(), references: vec![format!("{ROOM}{id}")] })
                })
                .collect())
        })
    }

    fn unlock<'a>(&'a self, key: &'a str) -> DriverFuture<'a, bool> {
        Box::pin(Renson::unlock(self, key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real answer of a Healthbox 3.0 (firmware 2.6.9), without API key, cut to two rooms.
    pub fn current() -> Value {
        json!({
            "device_type": "HEALTHBOX3", "description": "Healthbox 3.0", "serial": "240205P0173",
            "room": {
                "2": {"name": "Salle de bains", "type": "BathRoomWithoutToilet", "profile_name": "health",
                      "parameter": {"nominal": {"unit": "m³/h", "value": 16.0}, "offset": {"unit": "", "value": 0.0}},
                      "actuator": [{"type": "air valve", "parameter": {"flow_rate": {"unit": "m³/h", "value": 16.0}}}], "sensor": []},
                "10": {"name": "Toilettes", "type": "Toilet", "profile_name": "eco",
                      "parameter": {"nominal": {"unit": "m³/h", "value": 12.0}, "offset": {"unit": "", "value": 0.0}},
                      "actuator": [{"type": "air valve", "parameter": {"flow_rate": {"unit": "m³/h", "value": 3.0}}}], "sensor": []}
            },
            "sensor": [{"type": "global air quality index", "parameter": {
                "index": {"value": 147.42}, "main_pollutant": {"value": "indoor relative humidity"}, "room": {"value": "Salle de bains"}}}]
        })
    }

    #[test]
    fn a_healthbox_is_recognised() {
        let found = identify(&current()).unwrap();
        assert_eq!((found.serial.as_str(), found.model.as_str(), found.rooms), ("240205P0173", "Renson Healthbox 3.0", 2));
        assert_eq!(identify(&json!({"device_type": "OTHER", "room": {}})), None);
        assert_eq!(identify(&json!({"ChargeState": 0})), None);
    }

    #[test]
    fn rooms_and_the_house_without_api_key() {
        let fan = json!({"fan": {"flow": 63.5, "power": 4.0}, "power": 8.6});
        let boosts = HashMap::from([("10".to_string(), json!({"enable": true, "remaining": 840, "level": 100.0}))]);
        let s = states_of(&current(), Some(&fan), &boosts, false);
        let bath = &s["room:2"];
        assert_eq!((bath["flow"].clone(), bath["nominal"].clone(), bath["level"].clone()), (json!(16.0), json!(16.0), json!(100)));
        assert_eq!((bath["profile"].clone(), bath["boost"].clone()), (json!("health"), json!(false)));
        assert!(!bath.contains_key("humidity"), "no measure without the key");
        let wc = &s["room:10"];
        assert_eq!((wc["level"].clone(), wc["boost"].clone(), wc["boost_remaining"].clone()), (json!(25), json!(true), json!(840)));
        let unit = &s[UNIT];
        assert_eq!((unit["flow"].clone(), unit["power"].clone()), (json!(63.5), json!(8.6)));
        assert_eq!((unit["main_pollutant"].clone(), unit["worst_room"].clone()), (json!("humidity"), json!("Salle de bains")));
        assert_eq!((unit["boost"].clone(), unit["sensors"].clone()), (json!(true), json!(false)));
        // The rooms in their number's order, not the text's.
        assert_eq!(rooms(&current()).into_iter().map(|(id, _)| id).collect::<Vec<_>>(), ["2", "10"]);
    }

    #[test]
    fn the_measures_of_a_room_once_the_key_is_accepted() {
        let mut data = current();
        data["room"]["2"]["sensor"] = json!([
            {"type": "indoor relative humidity", "parameter": {"humidity": {"unit": "%", "value": 83.2}}},
            {"type": "indoor temperature", "parameter": {"temperature": {"unit": "°C", "value": 21.5}}},
            {"type": "indoor CO2", "parameter": {"concentration": {"unit": "ppm", "value": 1120.0}}},
            {"type": "indoor volatile organic compounds", "parameter": {"concentration": {"unit": "ppm", "value": 0.31}}},
            {"type": "indoor air quality index", "parameter": {"index": {"value": 80.0}}}
        ]);
        let bath = &states_of(&data, None, &HashMap::new(), true)["room:2"];
        assert_eq!((bath["humidity"].clone(), bath["temperature"].clone()), (json!(83.2), json!(21.5)));
        assert_eq!((bath["co2"].clone(), bath["voc"].clone()), (json!(1120.0), json!(0.31)));
    }

    /// Real answers of a unit, saved with curl in `RENSON_DIR` (current.json, device.json, boost{n}.json).
    #[test]
    #[ignore]
    fn real_answers_are_read() {
        let Ok(dir) = std::env::var("RENSON_DIR") else { return };
        let load = |f: &str| serde_json::from_str::<Value>(&std::fs::read_to_string(format!("{dir}/{f}")).unwrap()).unwrap();
        let data = load("current.json");
        println!("{:?}", identify(&data));
        let boosts = rooms(&data).into_iter().map(|(id, _)| (id.clone(), load(&format!("boost{id}.json")))).collect();
        let mut states: Vec<_> = states_of(&data, Some(&load("device.json")), &boosts, false).into_iter().collect();
        states.sort_by(|a, b| a.0.cmp(&b.0));
        for (r, s) in states {
            println!("{r}: {}", Value::Object(s));
        }
    }
}
