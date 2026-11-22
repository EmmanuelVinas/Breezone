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

use breezone_module_sdk::{device, host, json, module, Described, Driver, Error, Found, HardwareRoom, Map, Result, State, Value};
use std::collections::HashMap;

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

/// An address as typed: a name or an IP, with a port maybe.
fn valid_host(host_: &str) -> bool {
    !host_.is_empty() && host_.len() <= 253 && host_.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'))
}

pub struct Renson {
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
        // The unit's own judgement of the room (its app colours it: up to 40 good, then medium,
        // above 100 bad), and what drives it.
        if kind.contains("air quality index") {
            if let Some(i) = num(&sensor["parameter"]["index"]["value"]) {
                s.entry("air_quality_index").or_insert(json!(i));
            }
            if let Some(m) = sensor["parameter"]["main_pollutant"]["value"].as_str().filter(|m| !m.is_empty()) {
                s.entry("main_pollutant").or_insert(json!(pollutant(m)));
            }
            continue;
        }
        // Renson's documentation names the parameter to read for each sensor (the VOC sensor also
        // gives its raw values: baseline, raw, filtered…).
        let (field, params): (&str, &[&str]) = match kind {
            k if k.contains("relative humidity") => ("humidity", &["humidity"]),
            k if k.contains("temperature") => ("temperature", &["temperature"]),
            k if k.contains("CO2") => ("co2", &["concentration"]),
            k if k.contains("volatile organic") => ("voc", &["voc_calc_embedded", "concentration"]),
            _ => continue,
        };
        if let Some(v) = params.iter().find_map(|p| num(&sensor["parameter"][*p]["value"])) {
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
    fn get(&self, path: &str) -> Result<Value> {
        let a = host::request("GET", &format!("{}{path}", self.base), &[], None, 8000)?;
        if !(200..300).contains(&a.status) {
            return Err(Error::Invalid(format!("{path}: HTTP {}", a.status)));
        }
        a.json().map_err(|e| Error::Invalid(format!("{path}: {e:?}")))
    }

    fn send(&self, method: &str, path: &str, body: &Value) -> Result<()> {
        let a = host::request(method, &format!("{}{path}", self.base), &[("Content-Type", "application/json")], Some(body.to_string().as_bytes()), 8000)?;
        if !(200..300).contains(&a.status) {
            return Err(Error::Invalid(format!("{path}: HTTP {} {}", a.status, String::from_utf8_lossy(&a.body).trim())));
        }
        Ok(())
    }

    /// The unit accepts the measures of its rooms (a valid API key was given).
    fn sensors(&self) -> bool {
        self.get("/v2/api/api_key/status").is_ok_and(|s| s["state"] == "valid")
    }

    fn room_ids(&self) -> Result<Vec<String>> {
        Ok(rooms(&self.get("/v2/api/data/current")?).into_iter().map(|(id, _)| id).collect())
    }

    fn boost(&self, room: &str, minutes: i64) -> Result<()> {
        let current = self.get(&format!("/v2/api/boost/{room}")).unwrap_or(Value::Null);
        let level = num(&current["default_level"]).filter(|l| *l > 0.0).unwrap_or(BOOST_LEVEL);
        self.send("PUT", &format!("/v2/api/boost/{room}"), &json!({"enable": true, "level": level, "timeout": minutes * 60}))
    }

    fn stop_boost(&self, room: &str) -> Result<()> {
        self.send("PUT", &format!("/v2/api/boost/{room}"), &json!({"enable": false}))
    }
}

fn minutes(params: &State) -> Result<i64> {
    params.get("minutes").and_then(Value::as_i64).ok_or_else(|| Error::Invalid("`minutes` is required".into()))
}

impl Driver for Renson {
    fn open(config: Value, _secret: Option<Value>) -> Result<Self> {
        let host_ = config["host"].as_str().map(str::trim).unwrap_or_default();
        if !valid_host(host_) {
            return Err(Error::Invalid("invalid Healthbox address".into()));
        }
        let device_id = config["device_id"].as_str().map(str::trim).filter(|d| !d.is_empty()).map(str::to_string);
        Ok(Self { base: format!("http://{host_}"), device_id })
    }

    fn describe(&self) -> Result<Described> {
        let data = self.get("/v2/api/data/current")?;
        let found = identify(&data).ok_or_else(|| Error::Invalid("not a Renson Healthbox".into()))?;
        if self.device_id.as_ref().is_some_and(|expected| *expected != found.serial) {
            return Err(Error::Invalid(format!("another unit answers at this address ({}): scan the network again", found.serial)));
        }
        let firmware = self.get("/v1/global").ok().and_then(|g| g["firmware version"].as_str().map(str::to_string));
        let mut devices = vec![device(UNIT, "ventilation.unit", &["ventilation.unit@1"], "VMC")];
        devices.extend(rooms(&data).into_iter().map(|(id, r)| {
            let name = r["name"].as_str().filter(|n| !n.trim().is_empty()).map_or_else(|| format!("Bouche {id}"), str::to_string);
            device(format!("{ROOM}{id}"), "ventilation.room", &["ventilation.room@1"], name)
        }));
        Ok(Described { model: found.model, firmware, identity: (!found.serial.is_empty()).then_some(found.serial), devices })
    }

    fn read(&self) -> Result<Vec<(String, State)>> {
        let data = self.get("/v2/api/data/current")?;
        let fan = self.get("/v2/device").ok();
        let mut boosts = HashMap::new();
        for (id, _) in rooms(&data) {
            if let Ok(b) = self.get(&format!("/v2/api/boost/{id}")) {
                boosts.insert(id, b);
            }
        }
        Ok(states_of(&data, fan.as_ref(), &boosts, self.sensors()).into_iter().collect())
    }

    fn act(&self, reference: &str, trait_name: &str, action: &str, params: &State) -> Result<()> {
        match (reference.strip_prefix(ROOM), trait_name, action) {
            (Some(room), "ventilation.room", "boost") => self.boost(room, minutes(params)?),
            (Some(room), "ventilation.room", "stop_boost") => self.stop_boost(room),
            (Some(room), "ventilation.room", "set_profile") => {
                let profile = params.get("profile").and_then(Value::as_str).ok_or_else(|| Error::Invalid("`profile` is required".into()))?;
                self.send("PUT", &format!("/v2/api/data/current/room/{room}/profile_name"), &json!(profile))
            }
            (None, "ventilation.unit", "boost") if reference == UNIT => {
                let minutes = minutes(params)?;
                self.room_ids()?.iter().try_for_each(|room| self.boost(room, minutes))
            }
            (None, "ventilation.unit", "stop_boost") if reference == UNIT => self.room_ids()?.iter().try_for_each(|room| self.stop_boost(room)),
            _ => Err(Error::Unsupported(format!("{reference}: {trait_name}.{action}"))),
        }
    }

    /// The unit's answers as they are (each sensor's raw values, each room's own air quality
    /// index), without the address of the home: a diagnostic is meant to be shared.
    fn diagnostic(&self) -> Result<Value> {
        let mut current = self.get("/v2/api/data/current")?;
        if let Some(p) = current["global"]["parameter"].as_object_mut() {
            for private in ["street", "postal code", "city", "country", "device name"] {
                p.remove(private);
            }
        }
        let device = self.get("/v2/device").unwrap_or(Value::Null);
        let key = self.get("/v2/api/api_key/status").unwrap_or(Value::Null);
        Ok(json!({ "current": current, "device": device, "api_key_status": key }))
    }

    /// Each room of the unit, as named in the Renson app.
    fn rooms(&self) -> Result<Vec<HardwareRoom>> {
        let data = self.get("/v2/api/data/current")?;
        Ok(rooms(&data)
            .into_iter()
            .filter_map(|(id, r)| r["name"].as_str().filter(|n| !n.trim().is_empty()).map(|n| HardwareRoom { name: n.trim().to_string(), references: vec![format!("{ROOM}{id}")] }))
            .collect())
    }

    /// Gives the unit the API key that unlocks the measures of its rooms; true once it accepts it.
    fn unlock(&self, key: &str) -> Result<bool> {
        if self.sensors() {
            return Ok(true);
        }
        self.send("POST", "/v2/api/api_key", &json!(key.trim()))?;
        // The unit checks the key for a few seconds.
        for _ in 0..6 {
            host::wait(2000);
            if self.sensors() {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn identify(host_: &str, announced: Option<&Value>) -> Option<Found> {
        if announced.is_some() {
            return None;
        }
        let a = host::request("GET", &format!("http://{host_}/v2/api/data/current"), &[], None, 2000).ok()?;
        let r = identify(&a.json().ok()?)?;
        Some(Found { model: r.model, note: Some(format!("{} bouches · API locale", r.rooms)), device_id: (!r.serial.is_empty()).then_some(r.serial), ..Found::default() })
    }

    fn probe(host_: &str) -> Option<String> {
        let a = host::request("GET", &format!("http://{host_}/v2/api/data/current"), &[], None, 2000).ok()?;
        identify(&a.json().ok()?).map(|r| r.model)
    }
}

module!(Renson);

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
            // As Renson's documentation gives it: raw values of the sensor, and the one to show.
            {"type": "indoor volatile organic compounds", "parameter": {
                "baseline": {"unit": "ppm", "value": 5318.4}, "raw": {"unit": "ppm", "value": 960.0},
                "total_filtered": {"unit": "ppm", "value": 1194.0}, "voc_calc_embedded": {"unit": "ppm", "value": 68.19}
            }},
            {"type": "indoor air quality index", "parameter": {"index": {"value": 80.0}}}
        ]);
        let bath = &states_of(&data, None, &HashMap::new(), true)["room:2"];
        assert_eq!((bath["humidity"].clone(), bath["temperature"].clone()), (json!(83.2), json!(21.5)));
        assert_eq!((bath["co2"].clone(), bath["voc"].clone()), (json!(1120.0), json!(68.19)));
        assert_eq!(bath["air_quality_index"], json!(80.0));
        assert!(!bath.contains_key("main_pollutant"), "no cause under 40: none given");
    }

    /// As firmware 2.6.9 answers: the VOC as `concentration` (not the documented
    /// `voc_calc_embedded`), and each room's own index with its cause.
    #[test]
    fn the_measures_as_firmware_2_6_9_gives_them() {
        let mut data = current();
        data["room"]["2"]["sensor"] = json!([
            {"type": "indoor relative humidity", "parameter": {"humidity": {"unit": "pct", "value": 93.6}}},
            {"type": "indoor volatile organic compounds", "parameter": {
                "concentration": {"unit": "ppm", "value": 691.9}, "concentration_maximum": {"unit": "ppm", "value": 765.2}, "raw": {"unit": "ppm", "value": 45420.0}
            }},
            {"type": "indoor air quality index", "parameter": {"index": {"unit": "", "value": 198.9}, "main_pollutant": {"unit": "", "value": "indoor relative humidity"}}}
        ]);
        let bath = &states_of(&data, None, &HashMap::new(), true)["room:2"];
        assert_eq!(bath["voc"], json!(691.9));
        assert_eq!((bath["air_quality_index"].clone(), bath["main_pollutant"].clone()), (json!(198.9), json!("humidity")));
    }

    const MANIFEST: &str = include_str!("../manifest.json");

    #[test]
    fn a_unit_is_read_boosted_and_unlocked() {
        use breezone_module_sdk::testing;
        testing::reset();
        testing::answer_json("GET", "/v2/api/data/current", &current());
        testing::answer_json("GET", "/v2/device", &json!({"fan": {"flow": 63.5}, "power": 8.6}));
        testing::answer_json("GET", "/v2/api/boost/", &json!({"enable": false, "default_level": 150.0}));
        testing::answer_json("GET", "/v1/global", &json!({"firmware version": "2.6.9"}));
        testing::answer("GET", "/v2/api/api_key/status", 200, br#"{"state": "not set"}"#.to_vec());
        testing::answer("PUT", "/v2/api/boost/", 200, b"".to_vec());
        testing::answer("POST", "/v2/api/api_key", 200, b"".to_vec());
        let unit: Renson = testing::open(json!({"host": "192.168.1.70", "device_id": "240205P0173"}), None);
        let d = unit.describe().unwrap();
        assert_eq!((d.firmware.as_deref(), d.devices.len()), (Some("2.6.9"), 3));
        let state = testing::check(&unit, MANIFEST);
        assert_eq!(state["unit"]["flow"], json!(63.5));
        unit.act("unit", "ventilation.unit", "boost", &json!({"minutes": 15}).as_object().unwrap().clone()).unwrap();
        assert_eq!(testing::requests().iter().filter(|r| r.starts_with("PUT")).count(), 2, "each room");
        // The key is not accepted: it is asked again for a while, without computing.
        assert!(!unit.unlock("CLE").unwrap());
        assert_eq!(testing::waited_ms(), 12_000);
        testing::answer("GET", "/v2/api/api_key/status", 200, br#"{"state": "valid"}"#.to_vec());
        assert!(unit.unlock("CLE").unwrap());
        assert_eq!(unit.rooms().unwrap()[1].name, "Toilettes");
        assert_eq!(Renson::probe("192.168.1.70").as_deref(), Some("Renson Healthbox 3.0"));
    }

    /// Real answers of a unit, saved with curl in `RENSON_DIR` (current.json, device.json, boost{n}.json).
    #[test]
    #[ignore]
    fn real_answers_are_read() {
        let Ok(dir) = std::env::var("RENSON_DIR") else { return };
        let load = |f: &str| breezone_module_sdk::serde_json::from_str::<Value>(&std::fs::read_to_string(format!("{dir}/{f}")).unwrap()).unwrap();
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
