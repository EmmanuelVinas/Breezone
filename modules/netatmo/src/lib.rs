//! Netatmo's cloud (Legrand Home + Control, its Ecometer…), through the module's own account: one's
//! app on dev.netatmo.com, its client id and secret, and a refresh token from its "Token generator"
//! with the scopes `read_magellan` and `write_magellan`. Breezone renews the access itself
//! (`host::access_token`) and keeps the new refresh token each time.
//!
//! A cloud module: what it reads also exists locally for most of it (the shutters through
//! HomeKit), so Breezone keeps one device per real one and uses this module for what the local
//! one lacks (stopping a shutter midway) or does not have at all (the Ecometer).
//!
//!   GET  /api/homesdata             → {body: {homes: [{id, name, rooms, modules: [{id, type, name, bridge}]}]}}
//!   GET  /api/homestatus?home_id=…  → {body: {home: {modules: [{id, current_position, on, power…}]}}}
//!   POST /api/setstate {home: {id, modules: [{id, bridge, target_position | on | brightness}]}}
//!        target_position -1 stops a Legrand shutter where it is.
//!   POST /api/getmeasure (device_id, module_id, scale=30min, type=sum_energy_elec…, date_begin, date_end)
//!        → {body: [{beg_time, step_time, value: [[Wh…], …]}]}: what the lines of an Ecometer (NLE)
//!        used, by half hour. Their status has no power: the power shown is the mean of the last
//!        half hour told, a little late.

use breezone_module_sdk::{host, json, module, Described, DeviceInfo, Driver, Error, Map, Result, Service, State, Value};
use std::cell::RefCell;
use std::collections::HashMap;

const NETATMO: &str = "https://api.netatmo.com";
/// The lines of an Ecometer are read again this often: Legrand tells them by half hour, late.
const LINES_EVERY: i64 = 600;
/// What Legrand's Ecometer measures (its older API), all tariffs.
const ECOMETER_TYPES: &str = "sum_energy_elec,sum_energy_elec$0,sum_energy_elec$1,sum_energy_elec$2";

/// What a module of Netatmo is, for Breezone.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Cover,
    Light {
        dimmable: bool,
    },
    Relay,
    /// Measures only (the lines of an Ecometer, a meter).
    Power,
}

/// The kind of a module from its type; `None`: nothing to show (a gateway, a remote…) unless its
/// status measures a power.
fn kind_of(module_type: &str) -> Option<Kind> {
    match module_type {
        "NLV" | "NLLV" | "NLIV" | "NBR" | "NBO" | "NBS" => Some(Kind::Cover),
        "NLF" | "NLFN" | "NLFE" => Some(Kind::Light { dimmable: true }),
        "NLL" | "NLIS" | "NLPT" => Some(Kind::Light { dimmable: false }),
        "NLP" | "NLPM" | "NLPO" | "NLPD" | "NLC" | "NLM" | "NLAO" => Some(Kind::Relay),
        "NLE" | "NLPC" => Some(Kind::Power),
        _ => None,
    }
}

#[derive(Debug, Clone)]
struct Module {
    home: String,
    id: String,
    name: String,
    bridge: Option<String>,
    kind: Kind,
    /// Its status gives a power: a relay that measures, a meter.
    measures: bool,
    /// A line of an Ecometer: its power comes from its measures by half hour.
    ecometer: bool,
}

impl Module {
    fn reference(&self) -> String {
        format!("{}/{}", self.home, self.id)
    }

    fn info(&self) -> DeviceInfo {
        let (kind, mut traits) = match self.kind {
            Kind::Cover => ("shutter", vec!["cover.position@1"]),
            Kind::Light { .. } => ("light", vec!["light@1"]),
            Kind::Relay => ("switch.relay", vec!["switch.relay@1"]),
            Kind::Power => ("energy.power", vec![]),
        };
        if self.measures || self.kind == Kind::Power {
            traits.push("energy.power@1");
        }
        DeviceInfo { reference: self.reference(), kind: kind.into(), traits: traits.into_iter().map(str::to_string).collect(), name: self.name.clone() }
    }
}

/// The mean power (W) of the last whole slot of a `getmeasure` answer, `None` without any.
fn last_power(body: &Value, now: i64) -> Option<f64> {
    let mut last: Option<(i64, f64)> = None;
    for lot in body.as_array().into_iter().flatten() {
        let (Some(beg), Some(step)) = (lot["beg_time"].as_i64(), lot["step_time"].as_i64().or(Some(1800))) else { continue };
        for (i, values) in lot["value"].as_array().into_iter().flatten().enumerate() {
            let start = beg - step / 2 + i as i64 * step;
            let wh: Vec<f64> = values.as_array().into_iter().flatten().filter_map(Value::as_f64).collect();
            if wh.is_empty() || start + step > now || step <= 0 {
                continue;
            }
            let w = wh.iter().sum::<f64>() * 3600.0 / step as f64;
            if last.is_none_or(|(t, _)| start > t) {
                last = Some((start, w));
            }
        }
    }
    last.map(|(_, w)| w)
}

/// The state of a module as its trait says it, from its status.
fn state_of(m: &Module, s: &Value) -> State {
    let mut out = Map::new();
    let num = |k: &str| s[k].as_f64();
    match m.kind {
        Kind::Cover => {
            let position = num("current_position").map(|p| p.clamp(0.0, 100.0).round() as i64);
            let target = num("target_position").filter(|t| *t >= 0.0).map(|t| t.clamp(0.0, 100.0).round() as i64);
            if let Some(p) = position {
                out.insert("position".into(), json!(p));
                let moving = match target {
                    Some(t) if t > p => "opening",
                    Some(t) if t < p => "closing",
                    _ => "stopped",
                };
                out.insert("moving".into(), json!(moving));
            }
            if let Some(t) = target {
                out.insert("target".into(), json!(t));
            }
            out.insert("stoppable".into(), json!(true));
        }
        Kind::Light { dimmable } => {
            out.insert("on".into(), json!(s["on"].as_bool().unwrap_or(false)));
            if dimmable {
                if let Some(b) = num("brightness") {
                    out.insert("brightness".into(), json!(b.round() as i64));
                }
            }
        }
        Kind::Relay => {
            out.insert("on".into(), json!(s["on"].as_bool().unwrap_or(false)));
        }
        Kind::Power => {}
    }
    if let Some(p) = num("power") {
        out.insert("power".into(), json!(p));
    }
    out
}

fn unreachable(e: impl std::fmt::Display) -> Error {
    Error::Unreachable(format!("Netatmo: {e}"))
}

/// A call to Netatmo with the account's access: its answer, or what Netatmo says went wrong.
fn call(method: &str, path: &str, content_type: Option<&str>, body: Option<&[u8]>) -> Result<Value> {
    let token = host::access_token()?;
    let auth = format!("Bearer {token}");
    let mut headers = vec![("Authorization", auth.as_str())];
    if let Some(c) = content_type {
        headers.push(("Content-Type", c));
    }
    let a = host::request(method, &format!("{NETATMO}{path}"), &headers, body, 20_000)?;
    let v: Value = a.json().unwrap_or(Value::Null);
    if !(200..300).contains(&a.status) {
        return Err(unreachable(v["error"]["message"].as_str().map(str::to_string).unwrap_or_else(|| format!("HTTP {}", a.status))));
    }
    Ok(v)
}

fn get(path: &str) -> Result<Value> {
    call("GET", path, None, None)
}

pub struct Netatmo {
    /// The modules of every home, read again at each `describe`.
    modules: RefCell<Vec<Module>>,
    /// The power of the lines of the Ecometer, and when they were read.
    lines: RefCell<(Option<i64>, HashMap<String, f64>)>,
}

impl Netatmo {
    fn status(&self, home: &str) -> Result<Vec<Value>> {
        Ok(get(&format!("/api/homestatus?home_id={home}"))?["body"]["home"]["modules"].as_array().cloned().unwrap_or_default())
    }

    fn modules(&self) -> Result<Vec<Module>> {
        if self.modules.borrow().is_empty() {
            return self.load();
        }
        Ok(self.modules.borrow().clone())
    }

    /// Reads the homes and their modules again (and which ones measure a power).
    fn load(&self) -> Result<Vec<Module>> {
        let data = get("/api/homesdata")?;
        let mut out = vec![];
        for home in data["body"]["homes"].as_array().into_iter().flatten() {
            let Some(home_id) = home["id"].as_str() else { continue };
            let status = self.status(home_id).unwrap_or_default();
            let measures = |id: &str| status.iter().any(|m| m["id"].as_str() == Some(id) && m["power"].is_number());
            for m in home["modules"].as_array().into_iter().flatten() {
                let (Some(id), Some(t)) = (m["id"].as_str(), m["type"].as_str()) else { continue };
                // A module of a type not known here still counts when it measures a power.
                let Some(kind) = kind_of(t).or_else(|| measures(id).then_some(Kind::Power)) else { continue };
                out.push(Module {
                    home: home_id.to_string(),
                    id: id.to_string(),
                    name: m["name"].as_str().unwrap_or(id).to_string(),
                    bridge: m["bridge"].as_str().map(str::to_string),
                    kind,
                    measures: measures(id),
                    ecometer: t == "NLE" && !measures(id),
                });
            }
        }
        *self.modules.borrow_mut() = out.clone();
        Ok(out)
    }

    /// The power of each line of the Ecometer, read again when it is old.
    fn lines(&self, modules: &[Module]) -> HashMap<String, f64> {
        let now = host::now();
        if self.lines.borrow().0.is_some_and(|t| now - t < LINES_EVERY) {
            return self.lines.borrow().1.clone();
        }
        let mut out = HashMap::new();
        for m in modules.iter().filter(|m| m.ecometer) {
            let form = format!(
                "device_id={}&module_id={}&scale=30min&type={}&date_begin={}&date_end={now}",
                m.bridge.as_deref().unwrap_or(&m.id).replace(':', "%3A"),
                m.id.replace(':', "%3A"),
                ECOMETER_TYPES.replace('$', "%24").replace(',', "%2C"),
                now - 4 * 3600
            );
            match call("POST", "/api/getmeasure", Some("application/x-www-form-urlencoded"), Some(form.as_bytes())) {
                Ok(body) => {
                    if let Some(w) = last_power(&body["body"], now) {
                        out.insert(m.reference(), w);
                    }
                }
                Err(e) => host::log("debug", &format!("getmeasure of {}: {e:?}", m.name)),
            }
        }
        *self.lines.borrow_mut() = (Some(now), out.clone());
        out
    }
}

impl Driver for Netatmo {
    fn open(_config: Value, _secret: Option<Value>) -> Result<Self> {
        Ok(Self { modules: RefCell::new(vec![]), lines: RefCell::new((None, HashMap::new())) })
    }

    fn describe(&self) -> Result<Described> {
        let modules = self.load()?;
        Ok(Described { model: "Netatmo · Legrand".into(), firmware: None, devices: modules.iter().map(Module::info).collect(), identity: None })
    }

    fn read(&self) -> Result<Vec<(String, State)>> {
        let modules = self.modules()?;
        let mut homes: Vec<&str> = modules.iter().map(|m| m.home.as_str()).collect();
        homes.dedup();
        let lines = self.lines(&modules);
        let mut out = vec![];
        for home in homes {
            for s in self.status(home)? {
                let Some(m) = modules.iter().find(|m| m.home == home && s["id"].as_str() == Some(m.id.as_str())) else { continue };
                // A module the gateway does not reach is left without a state: unavailable.
                if s["reachable"].as_bool() == Some(false) {
                    continue;
                }
                let mut state = state_of(m, &s);
                if let (false, Some(w)) = (state.contains_key("power"), lines.get(&m.reference())) {
                    state.insert("power".into(), json!(w.round()));
                }
                out.push((m.reference(), state));
            }
        }
        Ok(out)
    }

    fn act(&self, reference: &str, trait_name: &str, action: &str, params: &State) -> Result<()> {
        let modules = self.modules()?;
        let m = modules.iter().find(|m| m.reference() == reference).ok_or_else(|| Error::Unsupported(format!("{reference}: unknown module")))?;
        let mut change = Map::new();
        change.insert("id".into(), json!(m.id));
        if let Some(b) = &m.bridge {
            change.insert("bridge".into(), json!(b));
        }
        match (trait_name, action) {
            ("cover.position", "set") => {
                let p = params.get("position").and_then(Value::as_i64).ok_or_else(|| Error::Invalid("`position` is required".into()))?;
                change.insert("target_position".into(), json!(p.clamp(0, 100)));
            }
            ("cover.position", "stop") => {
                change.insert("target_position".into(), json!(-1));
            }
            ("light", "set") | ("switch.relay", "set") => {
                if let Some(on) = params.get("on").and_then(Value::as_bool) {
                    change.insert("on".into(), json!(on));
                }
                if let Some(b) = params.get("brightness").and_then(Value::as_i64) {
                    change.insert("brightness".into(), json!(b.clamp(1, 100)));
                    change.insert("on".into(), json!(true));
                }
            }
            _ => return Err(Error::Unsupported(format!("{reference}: {trait_name}.{action}"))),
        }
        let body = json!({ "home": { "id": m.home, "modules": [Value::Object(change)] } });
        call("POST", "/api/setstate", Some("application/json"), Some(body.to_string().as_bytes())).map(|_| ())
    }

    fn diagnostic(&self) -> Result<Value> {
        let data = get("/api/homesdata")?;
        let mut status = Map::new();
        for home in data["body"]["homes"].as_array().into_iter().flatten() {
            if let Some(id) = home["id"].as_str() {
                status.insert(id.into(), Value::Array(self.status(id).unwrap_or_default()));
            }
        }
        Ok(json!({ "homes": data["body"]["homes"], "status": status }))
    }
}

/// The account: Netatmo must give an access and the homes it reaches.
pub struct Account;

impl Service for Account {
    fn check() -> Result<()> {
        get("/api/homesdata").map(|_| ())
    }

    fn query(contract: &str, query: &str, _params: Value) -> Result<Value> {
        Err(Error::Unsupported(format!("{contract} {query}")))
    }
}

module!(driver: Netatmo, service: Account);

#[cfg(test)]
mod tests {
    use super::*;
    use breezone_module_sdk::testing;

    const MANIFEST: &str = include_str!("../manifest.json");

    fn module(kind: Kind, measures: bool) -> Module {
        Module { home: "h".into(), id: "m".into(), name: "Volet salon".into(), bridge: Some("g".into()), kind, measures, ecometer: false }
    }

    #[test]
    fn the_power_of_a_line_of_the_ecometer_is_its_last_whole_half_hour() {
        let body = json!([{"beg_time": 900, "step_time": 1800, "value": [[100, null, null, null], [null, null, 200, 50], [null, null, 10, null]]}]);
        assert_eq!(last_power(&body, 5000), Some(500.0), "250 Wh in half an hour: 500 W; the third is not over");
        assert_eq!(last_power(&body, 5400), Some(20.0));
        assert_eq!(last_power(&json!([]), 5000), None);
    }

    #[test]
    fn a_shutter_moving_and_a_line_of_the_ecometer() {
        let s = state_of(&module(Kind::Cover, false), &json!({"current_position": 20, "target_position": 100}));
        assert_eq!((s["position"].clone(), s["moving"].clone(), s["stoppable"].clone()), (json!(20), json!("opening"), json!(true)));
        let s = state_of(&module(Kind::Cover, false), &json!({"current_position": 40, "target_position": -1}));
        assert_eq!(s["moving"], "stopped", "-1: stopped where it is");
        let line = module(Kind::Power, true);
        assert_eq!(line.info().traits, ["energy.power@1"]);
        assert_eq!(kind_of("NLG"), None, "the gateway itself is not a device");
    }

    #[test]
    fn a_home_is_read_and_a_shutter_stopped() {
        testing::reset();
        testing::access_token("t");
        testing::answer_json(
            "GET",
            "/api/homesdata",
            &json!({"body": {"homes": [{"id": "h1", "name": "Maison", "modules": [
                {"id": "g", "type": "NLG", "name": "Passerelle"},
                {"id": "v1", "type": "NLV", "name": "Volet salon", "bridge": "g"},
                {"id": "e1", "type": "NLE", "name": "Ecocompteur", "bridge": "g"}
            ]}]}}),
        );
        testing::answer_json(
            "GET",
            "/api/homestatus",
            &json!({"body": {"home": {"modules": [{"id": "v1", "current_position": 40, "target_position": -1, "reachable": true}, {"id": "e1", "reachable": true}]}}}),
        );
        let now = breezone_module_sdk::host::now();
        let beg = now - 3 * 1800;
        testing::answer_json("POST", "/api/getmeasure", &json!({"body": [{"beg_time": beg, "step_time": 1800, "value": [[400, null, null, null]]}]}));
        testing::answer_json("POST", "/api/setstate", &json!({"status": "ok"}));
        let n: Netatmo = testing::open(json!({}), None);
        let d = n.describe().unwrap();
        assert_eq!(d.devices.iter().map(|x| x.reference.as_str()).collect::<Vec<_>>(), ["h1/v1", "h1/e1"]);
        let state = testing::check(&n, MANIFEST);
        assert_eq!((state["h1/v1"]["position"].clone(), state["h1/e1"]["power"].clone()), (json!(40), json!(800.0)));
        n.act("h1/v1", "cover.position", "stop", &Map::new()).unwrap();
        assert!(testing::requests().iter().any(|r| r == "POST https://api.netatmo.com/api/setstate"));
        assert!(Account::check().is_ok());
    }

    #[test]
    fn without_an_account_nothing_is_reached() {
        testing::reset();
        let n: Netatmo = testing::open(json!({}), None);
        assert!(matches!(n.describe(), Err(Error::Unreachable(_))));
    }
}
