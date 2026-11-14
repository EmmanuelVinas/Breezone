//! A HomeKit accessory or bridge (a Legrand gateway, a VELUX gateway…) Breezone is paired with:
//! its window coverings (`cover.position@1`) and its switches, outlets and lights
//! (`switch.relay@1`), read and driven on the local network through an encrypted session.
//!
//! The source keeps the accessory's address (`host`, "ip:port") and pairing id (`device_id`); the
//! pairing itself (Breezone's key) is its secret, stored encrypted.

use super::{Described, DeviceInfo, Driver, DriverError, DriverFuture, DriverInfo, State};
use crate::homekit::{Connection, HapError, Pairing};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::Mutex;

/// Short HomeKit types (the UUIDs of Apple's base: "0000008C-0000-1000-8000-0026BB765291" → "8C").
mod hap {
    pub const INFORMATION: &str = "3E";
    pub const WINDOW_COVERING: &str = "8C";
    pub const WINDOW: &str = "8B";
    pub const DOOR: &str = "81";
    pub const SWITCH: &str = "49";
    pub const OUTLET: &str = "47";
    pub const LIGHTBULB: &str = "43";
    pub const NAME: &str = "23";
    pub const MANUFACTURER: &str = "20";
    pub const MODEL: &str = "21";
    pub const FIRMWARE: &str = "52";
    pub const CURRENT_POSITION: &str = "6D";
    pub const TARGET_POSITION: &str = "7C";
    pub const POSITION_STATE: &str = "72";
    pub const HOLD_POSITION: &str = "6F";
    pub const ON: &str = "25";
}

/// "0000008C-0000-1000-8000-0026BB765291" or "8C" → "8C"; any other UUID as it is.
fn short(kind: &str) -> String {
    let k = kind.to_ascii_uppercase();
    match k.strip_suffix("-0000-1000-8000-0026BB765291") {
        Some(head) => head.trim_start_matches('0').to_string(),
        None => k,
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Kind {
    Cover { current: u64, target: u64, state: Option<u64>, hold: Option<u64> },
    Switch { on: u64 },
}

/// A device of the accessory: where its characteristics are.
#[derive(Debug, Clone, PartialEq)]
struct Tracked {
    reference: String,
    aid: u64,
    kind: Kind,
}

#[derive(Debug, Clone, PartialEq)]
struct Layout {
    model: String,
    firmware: Option<String>,
    devices: Vec<(DeviceInfo, Tracked)>,
}

fn characteristic<'a>(service: &'a Value, kind: &str) -> Option<&'a Value> {
    service["characteristics"].as_array()?.iter().find(|c| c["type"].as_str().map(short).as_deref() == Some(kind))
}

fn iid(service: &Value, kind: &str) -> Option<u64> {
    characteristic(service, kind)?["iid"].as_u64()
}

fn text(service: &Value, kind: &str) -> Option<String> {
    characteristic(service, kind)?["value"].as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

/// The devices of an accessory database (`GET /accessories`).
fn layout(database: &Value) -> Layout {
    let accessories = database["accessories"].as_array().cloned().unwrap_or_default();
    let info = |a: &Value| a["services"].as_array().and_then(|s| s.iter().find(|s| s["type"].as_str().map(short).as_deref() == Some(hap::INFORMATION)).cloned());
    let bridge = accessories.iter().find(|a| a["aid"] == 1).and_then(info);
    let maker = bridge.as_ref().and_then(|b| text(b, hap::MANUFACTURER));
    let model = bridge.as_ref().and_then(|b| text(b, hap::MODEL)).or_else(|| bridge.as_ref().and_then(|b| text(b, hap::NAME))).unwrap_or_else(|| "HomeKit".into());
    let mut devices = vec![];
    for a in &accessories {
        let Some(aid) = a["aid"].as_u64() else { continue };
        let accessory_name = info(a).and_then(|i| text(&i, hap::NAME)).unwrap_or_else(|| format!("Accessoire {aid}"));
        let services = a["services"].as_array().cloned().unwrap_or_default();
        let several = services
            .iter()
            .filter(|s| matches!(s["type"].as_str().map(short).as_deref(), Some(hap::WINDOW_COVERING | hap::WINDOW | hap::DOOR | hap::SWITCH | hap::OUTLET | hap::LIGHTBULB)))
            .count()
            > 1;
        for s in &services {
            let Some(siid) = s["iid"].as_u64() else { continue };
            let kind = s["type"].as_str().map(short).unwrap_or_default();
            let name = match (text(s, hap::NAME), several) {
                (Some(n), _) => n,
                (None, true) => format!("{accessory_name} {siid}"),
                (None, false) => accessory_name.clone(),
            };
            let reference = format!("{aid}.{siid}");
            let (tracked, traits, device_kind) = match kind.as_str() {
                hap::WINDOW_COVERING | hap::WINDOW | hap::DOOR => {
                    let (Some(current), Some(target)) = (iid(s, hap::CURRENT_POSITION), iid(s, hap::TARGET_POSITION)) else { continue };
                    (Kind::Cover { current, target, state: iid(s, hap::POSITION_STATE), hold: iid(s, hap::HOLD_POSITION) }, "cover.position@1", "cover.position")
                }
                hap::SWITCH | hap::OUTLET | hap::LIGHTBULB => {
                    let Some(on) = iid(s, hap::ON) else { continue };
                    (Kind::Switch { on }, "switch.relay@1", "switch.relay")
                }
                _ => continue,
            };
            devices.push((DeviceInfo { reference: reference.clone(), kind: device_kind.into(), traits: vec![traits.into()], name }, Tracked { reference, aid, kind: tracked }));
        }
    }
    Layout { model: [maker, Some(model)].into_iter().flatten().collect::<Vec<_>>().join(" "), firmware: bridge.as_ref().and_then(|b| text(b, hap::FIRMWARE)), devices }
}

/// The state of each device, from the values read (`aid.iid` → value).
fn states_of(layout: &Layout, values: &HashMap<(u64, u64), Value>) -> HashMap<String, State> {
    let mut out = HashMap::new();
    for (_, t) in &layout.devices {
        let get = |iid: u64| values.get(&(t.aid, iid)).filter(|v| !v.is_null());
        let mut s = State::new();
        match &t.kind {
            Kind::Cover { current, target, state, .. } => {
                let Some(position) = get(*current).and_then(Value::as_i64) else { continue };
                s.insert("position".into(), json!(position.clamp(0, 100)));
                if let Some(v) = get(*target).and_then(Value::as_i64) {
                    s.insert("target".into(), json!(v.clamp(0, 100)));
                }
                // HomeKit: 0 decreasing (closing), 1 increasing (opening), 2 stopped.
                if let Some(v) = state.and_then(get).and_then(Value::as_i64) {
                    s.insert("moving".into(), json!(["closing", "opening", "stopped"].get(v as usize).copied().unwrap_or("stopped")));
                }
            }
            Kind::Switch { on } => {
                let Some(v) = get(*on) else { continue };
                s.insert("on".into(), json!(v.as_bool().unwrap_or_else(|| v.as_i64() == Some(1))));
            }
        }
        out.insert(t.reference.clone(), s);
    }
    out
}

pub fn info() -> DriverInfo {
    DriverInfo {
        id: "homekit",
        label: "HomeKit (Legrand, VELUX…)",
        provides: vec!["cover.position@1", "switch.relay@1"],
        models: vec!["Legrand Home + Control".into(), "VELUX App Control (KIG 300)".into()],
        config: vec![],
    }
}

pub struct HomeKit {
    addr: String,
    pairing: Pairing,
    /// One encrypted session, kept open; opened again after an error.
    conn: Mutex<Option<Connection>>,
    layout: Mutex<Option<Layout>>,
}

fn from_hap(e: HapError) -> DriverError {
    match e {
        HapError::Unreachable(m) => DriverError::Unreachable(m),
        other => DriverError::Invalid(other.to_string()),
    }
}

impl HomeKit {
    pub fn new(config: &Value, secret: Option<&str>) -> Result<Self, String> {
        let host = config["host"].as_str().map(str::trim).unwrap_or_default();
        if !super::shelly::valid_host(host) {
            return Err("invalid HomeKit accessory address".into());
        }
        let pairing: Pairing = secret.and_then(|s| serde_json::from_str(s).ok()).ok_or("a HomeKit accessory is added by pairing it with its code")?;
        Ok(Self { addr: host.to_string(), pairing, conn: Mutex::new(None), layout: Mutex::new(None) })
    }

    /// GET on the session, opened (again) when needed.
    async fn get(&self, path: &str) -> Result<Value, DriverError> {
        let mut conn = self.conn.lock().await;
        for attempt in 0..2 {
            if conn.is_none() {
                *conn = Some(Connection::verify(&self.addr, &self.pairing).await.map_err(from_hap)?);
            }
            match conn.as_mut().expect("opened").get_json(path).await {
                Ok(v) => return Ok(v),
                Err(e) => {
                    *conn = None;
                    if attempt == 1 {
                        return Err(from_hap(e));
                    }
                }
            }
        }
        unreachable!()
    }

    async fn put(&self, body: Value) -> Result<(), DriverError> {
        let mut conn = self.conn.lock().await;
        for attempt in 0..2 {
            if conn.is_none() {
                *conn = Some(Connection::verify(&self.addr, &self.pairing).await.map_err(from_hap)?);
            }
            match conn.as_mut().expect("opened").put_json("/characteristics", &body).await {
                Ok(()) => return Ok(()),
                Err(e) => {
                    *conn = None;
                    if attempt == 1 {
                        return Err(from_hap(e));
                    }
                }
            }
        }
        unreachable!()
    }

    async fn layout(&self) -> Result<Layout, DriverError> {
        if let Some(l) = self.layout.lock().await.clone() {
            return Ok(l);
        }
        let l = layout(&self.get("/accessories").await?);
        *self.layout.lock().await = Some(l.clone());
        Ok(l)
    }
}

impl Driver for HomeKit {
    fn describe(&self) -> DriverFuture<'_, Described> {
        Box::pin(async move {
            // The database again: accessories may have been added in their own app.
            *self.layout.lock().await = None;
            let l = self.layout().await?;
            let covers = l.devices.iter().filter(|(d, _)| d.kind == "cover.position").count();
            let note = format!("{covers} volet{} · {} autre{}", if covers > 1 { "s" } else { "" }, l.devices.len() - covers, if l.devices.len() - covers > 1 { "s" } else { "" });
            Ok(Described {
                model: format!("{} ({note})", l.model),
                firmware: l.firmware.clone(),
                devices: l.devices.iter().map(|(d, _)| d.clone()).collect(),
                identity: Some(self.pairing.accessory_id.clone()),
            })
        })
    }

    fn read(&self) -> DriverFuture<'_, HashMap<String, State>> {
        Box::pin(async move {
            let l = self.layout().await?;
            let ids: Vec<String> = l
                .devices
                .iter()
                .flat_map(|(_, t)| {
                    let iids = match &t.kind {
                        Kind::Cover { current, target, state, .. } => [Some(*current), Some(*target), *state].into_iter().flatten().collect::<Vec<_>>(),
                        Kind::Switch { on } => vec![*on],
                    };
                    iids.into_iter().map(move |i| format!("{}.{i}", t.aid))
                })
                .collect();
            if ids.is_empty() {
                return Ok(HashMap::new());
            }
            let answer = self.get(&format!("/characteristics?id={}", ids.join(","))).await?;
            let values: HashMap<(u64, u64), Value> = answer["characteristics"]
                .as_array()
                .map(|cs| cs.iter().filter_map(|c| Some(((c["aid"].as_u64()?, c["iid"].as_u64()?), c["value"].clone()))).collect())
                .unwrap_or_default();
            Ok(states_of(&l, &values))
        })
    }

    fn act<'a>(&'a self, reference: &'a str, trait_name: &'a str, action: &'a str, params: &'a State) -> DriverFuture<'a, ()> {
        Box::pin(async move {
            let l = self.layout().await?;
            let t = l.devices.iter().map(|(_, t)| t).find(|t| t.reference == reference).ok_or_else(|| DriverError::Unsupported(format!("{reference}: unknown device")))?;
            let write = |iid: u64, value: Value| json!({"characteristics": [{"aid": t.aid, "iid": iid, "value": value}]});
            match (&t.kind, trait_name, action) {
                (Kind::Cover { target, .. }, "cover.position", "set") => {
                    let position = params.get("position").and_then(Value::as_i64).ok_or_else(|| DriverError::Invalid("`position` is required".into()))?;
                    self.put(write(*target, json!(position.clamp(0, 100)))).await
                }
                (Kind::Cover { hold: Some(hold), .. }, "cover.position", "stop") => self.put(write(*hold, json!(true))).await,
                (Kind::Cover { current, target, .. }, "cover.position", "stop") => {
                    // Without "hold position": asked to stay where it is now.
                    let now = self.get(&format!("/characteristics?id={}.{current}", t.aid)).await?;
                    let position = now["characteristics"][0]["value"].as_i64().ok_or_else(|| DriverError::Invalid("no position".into()))?;
                    self.put(write(*target, json!(position))).await
                }
                (Kind::Switch { on }, "switch.relay", "set") => {
                    let value = params.get("on").and_then(Value::as_bool).ok_or_else(|| DriverError::Invalid("`on` is required".into()))?;
                    self.put(write(*on, json!(value))).await
                }
                _ => Err(DriverError::Unsupported(format!("{reference}: {trait_name}.{action}"))),
            }
        })
    }

    fn read_every(&self) -> Duration {
        Duration::from_secs(10)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> Value {
        json!({"accessories": [
            {"aid": 1, "services": [{"iid": 1, "type": "3E", "characteristics": [
                {"iid": 2, "type": "23", "value": "Module Control"}, {"iid": 3, "type": "20", "value": "Legrand"},
                {"iid": 4, "type": "21", "value": "Gateway"}, {"iid": 5, "type": "52", "value": "2.1.0"}]}]},
            {"aid": 2, "services": [
                {"iid": 1, "type": "0000003E-0000-1000-8000-0026BB765291", "characteristics": [{"iid": 2, "type": "00000023-0000-1000-8000-0026BB765291", "value": "Volet salon"}]},
                {"iid": 8, "type": "0000008C-0000-1000-8000-0026BB765291", "characteristics": [
                    {"iid": 9, "type": "0000006D-0000-1000-8000-0026BB765291", "value": 40},
                    {"iid": 10, "type": "0000007C-0000-1000-8000-0026BB765291", "value": 40},
                    {"iid": 11, "type": "00000072-0000-1000-8000-0026BB765291", "value": 2},
                    {"iid": 12, "type": "0000006F-0000-1000-8000-0026BB765291", "value": false}]}]},
            {"aid": 3, "services": [
                {"iid": 1, "type": "3E", "characteristics": [{"iid": 2, "type": "23", "value": "Prise cuisine"}]},
                {"iid": 8, "type": "47", "characteristics": [{"iid": 9, "type": "25", "value": true}]}]},
            {"aid": 4, "services": [
                {"iid": 1, "type": "3E", "characteristics": [{"iid": 2, "type": "23", "value": "Ecometer"}]},
                {"iid": 8, "type": "E863F007-079E-48FF-8F27-9C2605A29F52", "characteristics": []}]}
        ]})
    }

    #[test]
    fn covers_and_switches_of_a_bridge() {
        let l = layout(&database());
        assert_eq!(l.model, "Legrand Gateway");
        assert_eq!(l.firmware.as_deref(), Some("2.1.0"));
        let names: Vec<(&str, &str)> = l.devices.iter().map(|(d, _)| (d.reference.as_str(), d.name.as_str())).collect();
        assert_eq!(names, vec![("2.8", "Volet salon"), ("3.8", "Prise cuisine")], "the Ecometer's own service is left out");
        assert_eq!(l.devices[0].1.kind, Kind::Cover { current: 9, target: 10, state: Some(11), hold: Some(12) });

        let values = HashMap::from([((2, 9), json!(35)), ((2, 10), json!(80)), ((2, 11), json!(1)), ((3, 9), json!(1))]);
        let states = states_of(&l, &values);
        assert_eq!(states["2.8"]["position"], 35);
        assert_eq!(states["2.8"]["target"], 80);
        assert_eq!(states["2.8"]["moving"], "opening");
        assert_eq!(states["3.8"]["on"], true);
        assert_eq!(short("0000008C-0000-1000-8000-0026BB765291"), "8C");
    }

    /// Through the API: pairing with the code, a shutter read and driven, the key never shown, and
    /// Breezone's own pairing removed with the source.
    #[tokio::test]
    async fn paired_through_the_api_then_removed() {
        let fake = crate::homekit::fake::start().await;
        let dir = std::env::temp_dir().join(format!("breezone-homekit-{}-{}", std::process::id(), fake.port));
        let _ = std::fs::remove_dir_all(&dir);
        let state = crate::init_state(crate::config::Config::for_tests(dir)).await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, crate::app(state)).await.unwrap() });
        let http = reqwest::Client::new();
        let call = |method: &str, path: &str, token: Option<&str>, body: Option<Value>| {
            let mut req = http.request(method.parse().unwrap(), format!("{base}{path}"));
            if let Some(t) = token {
                req = req.bearer_auth(t);
            }
            if let Some(b) = body {
                req = req.json(&b);
            }
            async move {
                let resp = req.send().await.unwrap();
                let status = resp.status().as_u16();
                let text = resp.text().await.unwrap();
                (status, text.clone(), serde_json::from_str::<Value>(&text).unwrap_or(Value::Null))
            }
        };
        let (_, _, v) = call("POST", "/api/setup", None, Some(json!({"username":"admin","password":"motdepasse1"}))).await;
        let token = v["access_token"].as_str().unwrap().to_string();
        let t = Some(token.as_str());
        let host = format!("127.0.0.1:{}", fake.port);

        let (s, _, _) = call("POST", "/api/homekit/pair", t, Some(json!({"host": host, "name": "Legrand", "code": "1234"}))).await;
        assert_eq!(s, 400, "not a code");
        let (s, _, v) = call("POST", "/api/homekit/pair", t, Some(json!({"host": host, "name": "Legrand", "code": "111-11-111", "flags": 1}))).await;
        assert_eq!(s, 422);
        assert!(v.to_string().contains("Code incorrect") || v.to_string().contains("Wrong code"), "{v}");
        let (s, text, source) = call("POST", "/api/homekit/pair", t, Some(json!({"host": host, "name": "Legrand", "code": "03145154", "flags": 1}))).await;
        assert_eq!(s, 201, "{text}");
        assert_eq!(source["status"]["state"], "online", "{source}");
        assert_eq!(source["config"]["device_id"], "AA:BB:CC:DD:EE:FF");
        assert!(!text.contains("controller_ltsk") && !text.contains("secret"), "the pairing key is never shown: {text}");
        // The generic route cannot add an accessory without pairing it.
        let (s, _, _) = call("POST", "/api/sources", t, Some(json!({"driver":"homekit","name":"x","config":{"host": host}}))).await;
        assert_eq!(s, 400);

        let (_, _, devices) = call("GET", "/api/devices", t, None).await;
        let cover = devices.as_array().unwrap().iter().find(|d| d["source"] == "homekit").unwrap().clone();
        assert_eq!(cover["name"], "Volet salon");
        assert_eq!(cover["traits"], json!(["cover.position@1"]));
        assert_eq!(cover["state"]["position"], 40);
        let (s, text, d) = call("POST", &format!("/api/devices/{}/actions", cover["id"]), t, Some(json!({"trait":"cover.position","action":"set","params":{"position":75}}))).await;
        assert_eq!(s, 200, "{text}");
        assert_eq!(fake.state.lock().unwrap().position, 75);
        assert_eq!(d["state"]["position"], 75);

        let (s, _, _) = call("DELETE", &format!("/api/sources/{}", source["id"]), t, None).await;
        assert_eq!(s, 204);
        assert!(fake.state.lock().unwrap().controllers.is_empty(), "Breezone removed its own pairing");
    }

    #[test]
    fn needs_a_pairing() {
        assert!(HomeKit::new(&json!({"host": "192.168.1.25:5001"}), None).is_err());
    }
}
