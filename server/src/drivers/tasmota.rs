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

use super::{Described, DeviceInfo, Driver, DriverError, DriverFuture, State};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::Mutex;

/// What answers `Status 0` at an address, if it is a Tasmota (the network scanner).
pub async fn probe(host: &str) -> Option<String> {
    let http = reqwest::Client::builder().timeout(Duration::from_secs(2)).build().ok()?;
    let v: Value = http.get(format!("http://{host}/cm?cmnd=Status%200")).send().await.ok()?.json().await.ok()?;
    let version = v["StatusFWR"]["Version"].as_str()?;
    // "14.2.0(tasmota)": a Tasmota, not another JSON.
    version.contains("tasmota").then(|| v["Status"]["DeviceName"].as_str().filter(|n| !n.is_empty()).or(v["Status"]["FriendlyName"][0].as_str()).unwrap_or("Tasmota").to_string())
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

/// A colour temperature of Tasmota (mireds, 153–500) in kelvin, and back.
fn kelvin(mireds: f64) -> i64 {
    (1_000_000.0 / mireds.max(1.0)).round() as i64
}

pub struct Tasmota {
    http: reqwest::Client,
    base: String,
    password: Option<String>,
    status: Mutex<Value>,
}

impl Tasmota {
    pub fn new(config: &Value, secret: Option<&str>) -> Result<Self, String> {
        let host = config["host"].as_str().map(str::trim).filter(|h| !h.is_empty()).ok_or("l'adresse est requise")?;
        let base = if host.starts_with("http") { host.trim_end_matches('/').to_string() } else { format!("http://{host}") };
        let password = secret.and_then(|s| serde_json::from_str::<Value>(s).ok()).and_then(|v| v["password"].as_str().map(str::to_string)).filter(|p| !p.is_empty());
        let http = reqwest::Client::builder().timeout(Duration::from_secs(5)).build().unwrap_or_default();
        Ok(Self { http, base, password, status: Mutex::new(Value::Null) })
    }

    /// Sends a command and returns the answer.
    async fn command(&self, cmnd: &str) -> Result<Value, DriverError> {
        let mut req = self.http.get(format!("{}/cm", self.base)).query(&[("cmnd", cmnd)]);
        if let Some(p) = &self.password {
            req = req.query(&[("user", "admin"), ("password", p.as_str())]);
        }
        let resp = req.send().await.map_err(|e| DriverError::Unreachable(format!("Tasmota: {e}")))?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(DriverError::Invalid("Tasmota : mot de passe refusé".into()));
        }
        let v: Value = resp.json().await.map_err(|e| DriverError::Unreachable(format!("Tasmota: {e}")))?;
        if v.get("WARNING").is_some() {
            return Err(DriverError::Invalid(format!("Tasmota : {}", v["WARNING"].as_str().unwrap_or("refusé"))));
        }
        Ok(v)
    }

    async fn status(&self) -> Result<Value, DriverError> {
        let v = self.command("Status 0").await?;
        *self.status.lock().await = v.clone();
        Ok(v)
    }
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
pub fn states_of(v: &Value) -> HashMap<String, State> {
    let sts = &v["StatusSTS"];
    let mut out = HashMap::new();
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
        out.insert(format!("power{n}"), s);
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
        out.insert("energy".into(), s);
    }
    for (name, s) in sensors(&v["StatusSNS"]) {
        out.insert(format!("sensor/{name}"), s);
    }
    out
}

impl Driver for Tasmota {
    fn describe(&self) -> DriverFuture<'_, Described> {
        Box::pin(async move {
            let v = self.status().await?;
            Ok(Described {
                model: v["StatusFWR"]["Hardware"].as_str().map(|h| format!("Tasmota · {h}")).unwrap_or_else(|| "Tasmota".into()),
                firmware: v["StatusFWR"]["Version"].as_str().map(str::to_string),
                devices: devices_of(&v),
                identity: v["StatusNET"]["Mac"].as_str().map(str::to_string),
            })
        })
    }

    fn read(&self) -> DriverFuture<'_, HashMap<String, State>> {
        Box::pin(async move { Ok(states_of(&self.status().await?)) })
    }

    fn act<'a>(&'a self, reference: &'a str, trait_name: &'a str, action: &'a str, params: &'a State) -> DriverFuture<'a, ()> {
        Box::pin(async move {
            let n: u32 = reference.strip_prefix("power").and_then(|n| n.parse().ok()).ok_or_else(|| DriverError::Unsupported(format!("{reference}: nothing to drive")))?;
            match (trait_name, action) {
                ("switch.relay" | "light", "set") => {
                    if let Some(c) = params.get("color").and_then(Value::as_str) {
                        self.command(&format!("Color {}", c.trim_start_matches('#'))).await?;
                    }
                    if let Some(k) = params.get("color_temp").and_then(Value::as_f64) {
                        self.command(&format!("CT {}", (1_000_000.0 / k.max(1.0)).round().clamp(153.0, 500.0))).await?;
                    }
                    if let Some(b) = params.get("brightness").and_then(Value::as_f64) {
                        self.command(&format!("Dimmer {}", b.round().clamp(1.0, 100.0))).await?;
                    }
                    if let Some(on) = params.get("on").and_then(Value::as_bool) {
                        self.command(&format!("Power{n} {}", if on { "On" } else { "Off" })).await?;
                    }
                    Ok(())
                }
                _ => Err(DriverError::Unsupported(format!("{reference}: {trait_name}.{action}"))),
            }
        })
    }

    fn read_every(&self) -> Duration {
        Duration::from_secs(10)
    }

    fn diagnostic(&self) -> DriverFuture<'_, Value> {
        Box::pin(async move {
            let mut v = self.status().await?;
            // What tells the hardware apart, not where it is.
            if let Some(o) = v.as_object_mut() {
                o.remove("StatusNET");
                o.remove("StatusMQT");
            }
            Ok(v)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dimmer_two_relays_an_energy_meter_and_a_sensor() {
        let v = json!({
            "Status": {"DeviceName": "Salon", "FriendlyName": ["Lampe", "Prise"]},
            "StatusSTS": {"POWER1": "ON", "POWER2": "OFF", "Dimmer": 40, "CT": 250, "Color": "000000FF00"},
            "StatusSNS": {"ENERGY": {"Power": 52, "Voltage": 231, "Total": 12.5}, "AM2301": {"Temperature": 21.4, "Humidity": 48}, "TempUnit": "C"},
            "StatusFWR": {"Version": "14.2.0(tasmota)"}
        });
        let d = devices_of(&v);
        let names: Vec<(&str, &str)> = d.iter().map(|x| (x.reference.as_str(), x.kind.as_str())).collect();
        assert_eq!(names, [("power1", "light"), ("power2", "switch.relay"), ("energy", "energy.power"), ("sensor/AM2301", "sensor")]);
        assert_eq!(d[0].name, "Lampe");
        let s = states_of(&v);
        assert_eq!((s["power1"]["on"].clone(), s["power1"]["brightness"].clone(), s["power1"]["color_temp"].clone()), (json!(true), json!(40), json!(4000)));
        assert_eq!(s["power2"]["on"], false);
        assert_eq!((s["energy"]["power"].clone(), s["energy"]["energy_in"].clone()), (json!(52.0), json!(12.5)));
        assert_eq!((s["sensor/AM2301"]["temperature"].clone(), s["sensor/AM2301"]["humidity"].clone()), (json!(21.4), json!(48.0)));
    }

    #[test]
    fn a_single_relay_is_named_after_the_module() {
        let v = json!({"Status": {"DeviceName": "Chauffe-eau"}, "StatusSTS": {"POWER": "OFF"}, "StatusSNS": {}});
        let d = devices_of(&v);
        assert_eq!((d.len(), d[0].name.as_str(), d[0].reference.as_str()), (1, "Chauffe-eau", "power1"));
    }
}
