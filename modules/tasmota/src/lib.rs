//! Tasmota (Sonoff and many others flashed with it), through its local HTTP API: no MQTT needed.
//!
//!   GET /cm?cmnd=Status%200[&user=admin&password=…] → {Status: {DeviceName, FriendlyName: [..]},
//!       StatusSTS: {POWER | POWER1.., Dimmer, Color, CT}, StatusSNS: {ENERGY: {Power, Total},
//!       <sensor>: {Temperature, Humidity, Pressure, CarbonDioxide, Illuminance}},
//!       StatusFWR: {Version}, StatusNET: {Mac}}
//!   GET /cm?cmnd=Power2%20On, Dimmer%2050, Color%20FF8800, CT%20300 → the new state
//!
//! Its devices: each relay (a light when the module dims), the energy it measures, and each
//! sensor that gives a temperature, a humidity… (`environment.sensor@1`).

wit_bindgen::generate!({
    path: "../../server/contracts/wit",
    world: "module",
});

use breezone::module::host::{self, Level};
use breezone::module::types::{Described, DeviceInfo, Error, Request};
use exports::breezone::module::driver::{Guest, GuestSource, Source as SourceHandle};
use serde_json::{json, Map, Value};
use std::cell::RefCell;

struct Module;

pub struct Source {
    base: String,
    password: Option<String>,
    status: RefCell<Value>,
}

fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn get(url: &str, timeout_ms: u32) -> Result<(u16, Value), Error> {
    let resp = host::http(&Request { method: "GET".into(), url: url.into(), headers: vec![], body: None, timeout_ms }).map_err(|e| Error::Unreachable(format!("Tasmota: {e}")))?;
    let v = serde_json::from_slice(&resp.body).map_err(|e| Error::Unreachable(format!("Tasmota: {e}")))?;
    Ok((resp.status, v))
}

impl Source {
    fn command(&self, cmnd: &str) -> Result<Value, Error> {
        let mut url = format!("{}/cm?cmnd={}", self.base, encode(cmnd));
        if let Some(p) = &self.password {
            url.push_str(&format!("&user=admin&password={}", encode(p)));
        }
        let (status, v) = get(&url, 5000)?;
        if status == 401 {
            return Err(Error::Invalid("Tasmota : mot de passe refusé".into()));
        }
        if v.get("WARNING").is_some() {
            return Err(Error::Invalid(format!("Tasmota : {}", v["WARNING"].as_str().unwrap_or("refusé"))));
        }
        Ok(v)
    }

    fn status(&self) -> Result<Value, Error> {
        let v = self.command("Status 0")?;
        *self.status.borrow_mut() = v.clone();
        Ok(v)
    }
}

/// The sensors of `StatusSNS` that tell the air: (their name, their readings in Breezone's fields).
pub fn sensors(sns: &Value) -> Vec<(String, Map<String, Value>)> {
    let mut out = vec![];
    for (name, v) in sns.as_object().into_iter().flatten() {
        let Some(o) = v.as_object() else { continue };
        if name == "ENERGY" {
            continue;
        }
        let mut m = Map::new();
        for (from, to) in
            [("Temperature", "temperature"), ("Humidity", "humidity"), ("Pressure", "pressure"), ("CarbonDioxide", "co2"), ("eCO2", "co2"), ("Illuminance", "illuminance")]
        {
            if let Some(x) = o.get(from).and_then(Value::as_f64) {
                m.insert(to.into(), json!(x));
            }
        }
        if !m.is_empty() {
            out.push((name.clone(), m));
        }
    }
    out
}

/// The relays of `StatusSTS`: "POWER" alone, or "POWER1", "POWER2"…
fn relays(sts: &Value) -> Vec<(u32, bool)> {
    let o = sts.as_object();
    let on = |v: &Value| v.as_str() == Some("ON");
    if let Some(v) = o.and_then(|o| o.get("POWER")) {
        return vec![(1, on(v))];
    }
    let mut out: Vec<(u32, bool)> = o.into_iter().flatten().filter_map(|(k, v)| k.strip_prefix("POWER").and_then(|n| n.parse().ok()).map(|n: u32| (n, on(v)))).collect();
    out.sort();
    out
}

/// A colour temperature of Tasmota (mireds, 153–500) in kelvin.
fn kelvin(mireds: f64) -> i64 {
    (1_000_000.0 / mireds.max(1.0)).round() as i64
}

/// The devices of a module, from its status.
pub fn devices_of(v: &Value) -> Vec<DeviceInfo> {
    let names: Vec<String> = v["Status"]["FriendlyName"].as_array().into_iter().flatten().filter_map(|n| n.as_str().map(str::to_string)).collect();
    let device = v["Status"]["DeviceName"].as_str().filter(|n| !n.is_empty()).map(str::to_string).or_else(|| names.first().cloned()).unwrap_or_else(|| "Tasmota".into());
    let sts = &v["StatusSTS"];
    let dims = sts.get("Dimmer").is_some();
    let mut out = vec![];
    let list = relays(sts);
    for (n, _) in &list {
        let name = names.get(*n as usize - 1).cloned().filter(|s| !s.is_empty()).unwrap_or_else(|| if list.len() > 1 { format!("{device} {n}") } else { device.clone() });
        if dims && *n == 1 {
            out.push(DeviceInfo { reference: format!("power{n}"), kind: "light".into(), traits: vec!["light@1".into()], name });
        } else {
            out.push(DeviceInfo { reference: format!("power{n}"), kind: "switch.relay".into(), traits: vec!["switch.relay@1".into()], name });
        }
    }
    if v["StatusSNS"]["ENERGY"].is_object() {
        out.push(DeviceInfo {
            reference: "energy".into(),
            kind: "energy.power".into(),
            traits: vec!["energy.power@1".into(), "energy.meter@1".into()],
            name: format!("{device} · mesure"),
        });
    }
    for (name, _) in sensors(&v["StatusSNS"]) {
        out.push(DeviceInfo { reference: format!("sensor/{name}"), kind: "sensor".into(), traits: vec!["environment.sensor@1".into()], name: format!("{device} · {name}") });
    }
    out
}

/// The state of each device, from the status.
pub fn states_of(v: &Value) -> Vec<(String, Map<String, Value>)> {
    let sts = &v["StatusSTS"];
    let mut out = vec![];
    for (n, on) in relays(sts) {
        let mut s = Map::new();
        s.insert("on".into(), json!(on));
        if n == 1 {
            if let Some(d) = sts["Dimmer"].as_f64() {
                s.insert("brightness".into(), json!(d.round() as i64));
                if let Some(ct) = sts["CT"].as_f64() {
                    s.insert("color_temp".into(), json!(kelvin(ct)));
                    s.insert("color_temp_min".into(), json!(2000));
                    s.insert("color_temp_max".into(), json!(6500));
                }
                if let Some(c) = sts["Color"].as_str().filter(|c| c.len() >= 6 && !c.contains(',')) {
                    s.insert("color".into(), json!(format!("#{}", c[..6].to_lowercase())));
                    let white = sts["White"].as_f64().unwrap_or(0.0) > 0.0 || sts["CT"].is_number() && c[..6].eq_ignore_ascii_case("000000");
                    s.insert("mode".into(), json!(if white { "white" } else { "color" }));
                }
                s.insert("reachable".into(), json!(true));
            }
        }
        out.push((format!("power{n}"), s));
    }
    let energy = &v["StatusSNS"]["ENERGY"];
    if energy.is_object() {
        let mut s = Map::new();
        for (from, to) in [("Power", "power"), ("Voltage", "voltage"), ("Current", "current")] {
            if let Some(x) = energy[from].as_f64() {
                s.insert(to.into(), json!(x));
            }
        }
        if let Some(t) = energy["Total"].as_f64() {
            s.insert("energy_in".into(), json!(t));
        }
        out.push(("energy".into(), s));
    }
    for (name, s) in sensors(&v["StatusSNS"]) {
        out.push((format!("sensor/{name}"), s));
    }
    out
}

impl GuestSource for Source {
    fn open(config: String, secret: Option<String>) -> Result<SourceHandle, Error> {
        let config: Value = serde_json::from_str(&config).map_err(|e| Error::Invalid(e.to_string()))?;
        let host = config["host"].as_str().map(str::trim).filter(|h| !h.is_empty()).ok_or_else(|| Error::Invalid("l'adresse est requise".into()))?;
        let base = if host.starts_with("http") { host.trim_end_matches('/').to_string() } else { format!("http://{host}") };
        let password = secret.and_then(|s| serde_json::from_str::<Value>(&s).ok()).and_then(|v| v["password"].as_str().map(str::to_string)).filter(|p| !p.is_empty());
        Ok(SourceHandle::new(Source { base, password, status: RefCell::new(Value::Null) }))
    }

    fn describe(&self) -> Result<Described, Error> {
        let v = self.status()?;
        Ok(Described {
            model: v["StatusFWR"]["Hardware"].as_str().map(|h| format!("Tasmota · {h}")).unwrap_or_else(|| "Tasmota".into()),
            firmware: v["StatusFWR"]["Version"].as_str().map(str::to_string),
            devices: devices_of(&v),
            identity: v["StatusNET"]["Mac"].as_str().map(str::to_string),
        })
    }

    fn read(&self) -> Result<Vec<(String, String)>, Error> {
        Ok(states_of(&self.status()?).into_iter().map(|(r, s)| (r, Value::Object(s).to_string())).collect())
    }

    fn act(&self, reference: String, trait_name: String, action: String, params: String) -> Result<(), Error> {
        let params: Map<String, Value> = serde_json::from_str(&params).map_err(|e| Error::Invalid(e.to_string()))?;
        let n: u32 = reference.strip_prefix("power").and_then(|n| n.parse().ok()).ok_or_else(|| Error::Unsupported(format!("{reference}: nothing to drive")))?;
        match (trait_name.as_str(), action.as_str()) {
            ("switch.relay" | "light", "set") => {
                if let Some(c) = params.get("color").and_then(Value::as_str) {
                    self.command(&format!("Color {}", c.trim_start_matches('#')))?;
                }
                if let Some(k) = params.get("color_temp").and_then(Value::as_f64) {
                    self.command(&format!("CT {}", (1_000_000.0 / k.max(1.0)).round().clamp(153.0, 500.0)))?;
                }
                if let Some(b) = params.get("brightness").and_then(Value::as_f64) {
                    self.command(&format!("Dimmer {}", b.round().clamp(1.0, 100.0)))?;
                }
                if let Some(on) = params.get("on").and_then(Value::as_bool) {
                    self.command(&format!("Power{n} {}", if on { "On" } else { "Off" }))?;
                }
                Ok(())
            }
            _ => Err(Error::Unsupported(format!("{reference}: {trait_name}.{action}"))),
        }
    }

    fn diagnostic(&self) -> Result<String, Error> {
        let mut v = self.status()?;
        if let Some(o) = v.as_object_mut() {
            o.remove("StatusNET");
            o.remove("StatusMQT");
        }
        Ok(v.to_string())
    }
}

impl Guest for Module {
    type Source = Source;

    fn probe(host: String) -> Option<String> {
        let (_, v) = get(&format!("http://{host}/cm?cmnd=Status%200"), 2000).ok()?;
        let version = v["StatusFWR"]["Version"].as_str()?;
        if !version.contains("tasmota") {
            return None;
        }
        host::log(Level::Debug, &format!("Tasmota found at {host}"));
        Some(v["Status"]["DeviceName"].as_str().filter(|n| !n.is_empty()).or(v["Status"]["FriendlyName"][0].as_str()).unwrap_or("Tasmota").to_string())
    }
}

export!(Module);
