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

use breezone_module_sdk::{host, json, module, Described, DeviceInfo, Driver, Error, Found, Map, Result, State, Value};
use std::cell::RefCell;

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

pub struct Tasmota {
    base: String,
    password: Option<String>,
    status: RefCell<Value>,
}

impl Tasmota {
    fn command(&self, cmnd: &str) -> Result<Value> {
        let mut url = format!("{}/cm?cmnd={}", self.base, encode(cmnd));
        if let Some(p) = &self.password {
            url.push_str(&format!("&user=admin&password={}", encode(p)));
        }
        let (status, v) = host::get_json(&url, 5000)?;
        if status == 401 {
            return Err(Error::Invalid("Tasmota : mot de passe refusé".into()));
        }
        if v.get("WARNING").is_some() {
            return Err(Error::Invalid(format!("Tasmota : {}", v["WARNING"].as_str().unwrap_or("refusé"))));
        }
        Ok(v)
    }

    fn status(&self) -> Result<Value> {
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
            out.push(breezone_module_sdk::device(format!("power{n}"), "light", &["light@1"], name));
        } else {
            out.push(breezone_module_sdk::device(format!("power{n}"), "switch.relay", &["switch.relay@1"], name));
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

impl Driver for Tasmota {
    fn open(config: Value, secret: Option<Value>) -> Result<Self> {
        let host = config["host"].as_str().map(str::trim).filter(|h| !h.is_empty()).ok_or_else(|| Error::Invalid("l'adresse est requise".into()))?;
        let base = if host.starts_with("http") { host.trim_end_matches('/').to_string() } else { format!("http://{host}") };
        let password = secret.and_then(|v| v["password"].as_str().map(str::to_string)).filter(|p| !p.is_empty());
        Ok(Tasmota { base, password, status: RefCell::new(Value::Null) })
    }

    fn describe(&self) -> Result<Described> {
        let v = self.status()?;
        Ok(Described {
            model: v["StatusFWR"]["Hardware"].as_str().map(|h| format!("Tasmota · {h}")).unwrap_or_else(|| "Tasmota".into()),
            firmware: v["StatusFWR"]["Version"].as_str().map(str::to_string),
            devices: devices_of(&v),
            identity: v["StatusNET"]["Mac"].as_str().map(str::to_string),
        })
    }

    fn read(&self) -> Result<Vec<(String, State)>> {
        Ok(states_of(&self.status()?))
    }

    fn act(&self, reference: &str, trait_name: &str, action: &str, params: &State) -> Result<()> {
        let n: u32 = reference.strip_prefix("power").and_then(|n| n.parse().ok()).ok_or_else(|| Error::Unsupported(format!("{reference}: nothing to drive")))?;
        match (trait_name, action) {
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

    fn diagnostic(&self) -> Result<Value> {
        let mut v = self.status()?;
        // What tells the hardware apart, not where it is.
        if let Some(o) = v.as_object_mut() {
            o.remove("StatusNET");
            o.remove("StatusMQT");
        }
        Ok(v)
    }

    fn identify(host: &str, announced: Option<&Value>) -> Option<Found> {
        if announced.is_some() {
            return None;
        }
        Self::probe(host).map(|name| Found { model: "Tasmota".into(), note: Some(format!("« {name} »")), ..Found::default() })
    }

    fn probe(host: &str) -> Option<String> {
        let (_, v) = host::get_json(&format!("http://{host}/cm?cmnd=Status%200"), 2000).ok()?;
        let version = v["StatusFWR"]["Version"].as_str()?;
        // "14.2.0(tasmota)": a Tasmota, not another JSON.
        if !version.contains("tasmota") {
            return None;
        }
        Some(v["Status"]["DeviceName"].as_str().filter(|n| !n.is_empty()).or(v["Status"]["FriendlyName"][0].as_str()).unwrap_or("Tasmota").to_string())
    }
}

module!(Tasmota);

#[cfg(test)]
mod tests {
    use super::*;
    use breezone_module_sdk::testing;

    const MANIFEST: &str = include_str!("../manifest.json");

    fn status() -> Value {
        json!({
            "Status": {"DeviceName": "Salon", "FriendlyName": ["Lampe", "Prise"]},
            "StatusSTS": {"POWER1": "ON", "POWER2": "OFF", "Dimmer": 40, "CT": 250, "Color": "000000FF00"},
            "StatusSNS": {"ENERGY": {"Power": 52, "Voltage": 231, "Total": 12.5}, "AM2301": {"Temperature": 21.4, "Humidity": 48}, "TempUnit": "C"},
            "StatusFWR": {"Version": "14.2.0(tasmota)", "Hardware": "ESP8266EX"},
            "StatusNET": {"Mac": "AA:BB:CC:DD:EE:FF"}
        })
    }

    #[test]
    fn a_dimmer_two_relays_an_energy_meter_and_a_sensor() {
        testing::reset();
        testing::answer_json("GET", "cmnd=Status%200", &status());
        let t: Tasmota = testing::open(json!({"host": "192.168.1.30"}), None);
        // What it says fits the catalog of traits and its manifest.
        let s = testing::check(&t, MANIFEST);
        assert_eq!((s["power1"]["on"].clone(), s["power1"]["brightness"].clone(), s["power1"]["color_temp"].clone()), (json!(true), json!(40), json!(4000)));
        assert_eq!(s["power2"]["on"], false);
        assert_eq!((s["energy"]["power"].clone(), s["energy"]["energy_in"].clone()), (json!(52.0), json!(12.5)));
        assert_eq!((s["sensor/AM2301"]["temperature"].clone(), s["sensor/AM2301"]["humidity"].clone()), (json!(21.4), json!(48.0)));
        let d = t.describe().unwrap();
        let names: Vec<(&str, &str)> = d.devices.iter().map(|x| (x.reference.as_str(), x.kind.as_str())).collect();
        assert_eq!(names, [("power1", "light"), ("power2", "switch.relay"), ("energy", "energy.power"), ("sensor/AM2301", "sensor")]);
        assert_eq!((d.devices[0].name.as_str(), d.identity.as_deref()), ("Lampe", Some("AA:BB:CC:DD:EE:FF")));
    }

    #[test]
    fn a_single_relay_is_named_after_the_module() {
        let d = devices_of(&json!({"Status": {"DeviceName": "Chauffe-eau"}, "StatusSTS": {"POWER": "OFF"}, "StatusSNS": {}}));
        assert_eq!((d.len(), d[0].name.as_str(), d[0].reference.as_str()), (1, "Chauffe-eau", "power1"));
    }

    #[test]
    fn it_drives_a_relay_with_its_password() {
        testing::reset();
        testing::answer_json("GET", "/cm?", &json!({"POWER2": "ON"}));
        let t: Tasmota = testing::open(json!({"host": "192.168.1.30"}), Some(json!({"password": "s3cr&t"})));
        let on = json!({"on": true});
        t.act("power2", "switch.relay", "set", on.as_object().unwrap()).unwrap();
        assert_eq!(testing::requests(), ["GET http://192.168.1.30/cm?cmnd=Power2%20On&user=admin&password=s3cr%26t"]);
        assert!(matches!(t.act("energy", "switch.relay", "set", on.as_object().unwrap()), Err(Error::Unsupported(_))));
    }

    #[test]
    fn a_refused_password_and_another_json_are_told() {
        testing::reset();
        testing::answer("GET", "/cm?", 401, "{}");
        let t: Tasmota = testing::open(json!({"host": "192.168.1.30"}), None);
        assert!(matches!(t.read(), Err(Error::Invalid(m)) if m.contains("mot de passe")));
        testing::answer_json("GET", "/cm?", &json!({"StatusFWR": {"Version": "1.0"}}));
        assert_eq!(Tasmota::probe("192.168.1.30"), None);
        testing::answer_json("GET", "/cm?", &status());
        assert_eq!(Tasmota::probe("192.168.1.30").as_deref(), Some("Salon"));
    }
}
