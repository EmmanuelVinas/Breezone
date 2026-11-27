//! Tasmota (Sonoff and many others flashed with it), through its local HTTP API: no MQTT needed.
//!
//!   GET /cm?cmnd=Status%200[&user=admin&password=…] → {Status: {DeviceName, FriendlyName: [..]},
//!       StatusSTS: {POWER | POWER1.., Dimmer, Color, CT}, StatusSNS: {ENERGY: {Power, Total},
//!       <sensor>: {Temperature, Humidity, Pressure, CarbonDioxide, Illuminance}},
//!       StatusFWR: {Version}, StatusNET: {Mac}}
//!   GET /cm?cmnd=Power2%20On, Dimmer%2050, Color%20FF8800, CT%20300 → the new state
//!
//! Its devices: each relay (a light when the module dims), the energy it measures, and each
//! sensor that gives a temperature, a humidity… (`environment.sensor@1`). A Tasmota that is a
//! Zigbee coordinator (the SONOFF ZBBridge Pro) adds itself (`zigbee.bridge@1`: the pairing) and
//! the devices of its network ([`zigbee`]).
//!
//! Tasmota 16 answers `/cm` only to a request with a `Referer`: it is always sent.

pub mod zigbee;

use breezone_module_sdk::{host, json, module, serde_json, Described, DeviceInfo, Driver, Error, Found, Map, Result, State, Value};
use std::cell::{Cell, RefCell};

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

/// A value parsed with the objects of a repeated key merged: Tasmota answers a command that
/// publishes several messages (`ZbInfo` of every device) with their JSONs joined, keys repeated.
struct Merged(Value);

fn merge(into: &mut Map<String, Value>, key: String, value: Value) {
    match (into.get_mut(&key), value) {
        (Some(Value::Object(a)), Value::Object(b)) => {
            for (k, v) in b {
                merge(a, k, v);
            }
        }
        // `{"ZbInfo": {…the devices…}, "ZbInfo": "Done"}`: the word after the data keeps the data.
        (Some(Value::Object(_)), _) => {}
        (_, v) => {
            into.insert(key, v);
        }
    }
}

impl<'de> serde::Deserialize<'de> for Merged {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct Visit;
        impl<'de> serde::de::Visitor<'de> for Visit {
            type Value = Merged;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("JSON")
            }
            fn visit_bool<E>(self, v: bool) -> std::result::Result<Merged, E> {
                Ok(Merged(json!(v)))
            }
            fn visit_i64<E>(self, v: i64) -> std::result::Result<Merged, E> {
                Ok(Merged(json!(v)))
            }
            fn visit_u64<E>(self, v: u64) -> std::result::Result<Merged, E> {
                Ok(Merged(json!(v)))
            }
            fn visit_f64<E>(self, v: f64) -> std::result::Result<Merged, E> {
                Ok(Merged(json!(v)))
            }
            fn visit_str<E>(self, v: &str) -> std::result::Result<Merged, E> {
                Ok(Merged(json!(v)))
            }
            fn visit_unit<E>(self) -> std::result::Result<Merged, E> {
                Ok(Merged(Value::Null))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut a: A) -> std::result::Result<Merged, A::Error> {
                let mut out = vec![];
                while let Some(Merged(v)) = a.next_element()? {
                    out.push(v);
                }
                Ok(Merged(Value::Array(out)))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut a: A) -> std::result::Result<Merged, A::Error> {
                let mut out = Map::new();
                while let Some((k, Merged(v))) = a.next_entry::<String, Merged>()? {
                    merge(&mut out, k, v);
                }
                Ok(Merged(Value::Object(out)))
            }
        }
        d.deserialize_any(Visit)
    }
}

/// An answer of Tasmota: one JSON, its repeated keys merged, or several one after the other.
pub fn parse(body: &[u8]) -> Result<Value> {
    let mut out = Map::new();
    let mut any = false;
    for v in serde_json::Deserializer::from_slice(body).into_iter::<Merged>() {
        match v {
            Ok(Merged(Value::Object(o))) => {
                any = true;
                for (k, v) in o {
                    merge(&mut out, k, v);
                }
            }
            Ok(_) => {}
            Err(e) if any => {
                host::log("warn", &format!("Tasmota : fin de réponse illisible ({e})"));
                break;
            }
            Err(e) => return Err(Error::Invalid(format!("Tasmota : réponse illisible ({e})"))),
        }
    }
    if !any {
        return Err(Error::Invalid("Tasmota : réponse vide".into()));
    }
    Ok(Value::Object(out))
}

/// `GET` of a Tasmota, with the `Referer` Tasmota 16 wants.
fn get(base: &str, path: &str, timeout_ms: u32) -> Result<(u16, Value)> {
    let referer = format!("{base}/");
    let a = host::request("GET", &format!("{base}{path}"), &[("Referer", &referer)], None, timeout_ms)?;
    if a.status == 401 {
        return Ok((401, Value::Null));
    }
    Ok((a.status, parse(&a.body)?))
}

pub struct Tasmota {
    base: String,
    password: Option<String>,
    status: RefCell<Value>,
    /// Whether it is a Zigbee coordinator (found when described, or by a reading after).
    zigbee: Cell<bool>,
    /// When a ZBBridge whose Zigbee did not answer was last asked again.
    zigbee_asked_at: Cell<i64>,
    /// Until when (unix seconds) its Zigbee network is open to new devices.
    pairing_until: Cell<i64>,
}

impl Tasmota {
    fn command(&self, cmnd: &str) -> Result<Value> {
        let mut path = format!("/cm?cmnd={}", encode(cmnd));
        if let Some(p) = &self.password {
            path.push_str(&format!("&user=admin&password={}", encode(p)));
        }
        let (status, v) = get(&self.base, &path, 5000)?;
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

    /// The devices of its Zigbee network, each with what it knows of them; an error for a Tasmota
    /// that is not a coordinator (it says « Unknown » to the commands of Zigbee).
    fn zigbee_devices(&self) -> Result<Vec<Value>> {
        let list = self.command("ZbStatus1")?;
        if !list["ZbStatus1"].is_array() {
            return Err(Error::Unsupported("Tasmota : pas de coordinateur Zigbee".into()));
        }
        // All at once; one by one those the answer missed.
        let all = self.command("ZbInfo").ok();
        let infos = all.as_ref().and_then(|v| v["ZbInfo"].as_object()).cloned().unwrap_or_default();
        let (mut devices, missing) = zigbee::join(&list["ZbStatus1"], &infos);
        for short in missing {
            let Ok(v) = self.command(&format!("ZbInfo {short}")) else { continue };
            if let Some(i) = v["ZbInfo"].get(&short) {
                if let Some(d) = devices.iter_mut().find(|d| d["Device"].as_str() == Some(short.as_str())) {
                    *d = i.clone();
                }
            }
        }
        Ok(devices)
    }

    fn bridge_state(&self, devices: &[Value]) -> State {
        let mut s = Map::new();
        s.insert("pairing".into(), json!(host::now() < self.pairing_until.get()));
        s.insert("devices".into(), json!(devices.len()));
        s
    }
}

/// Whether a status is a Tasmota's ("14.2.0(tasmota)", "16.0.0(release-zbbrdgpro)"), not another JSON.
fn is_tasmota(v: &Value) -> bool {
    v["StatusFWR"]["Version"].is_string() && v["Status"].is_object()
}

/// A firmware for the ZBBridge Pro: Zigbee2Tasmota is in it.
fn zigbee_firmware(v: &Value) -> bool {
    v["StatusFWR"]["Version"].as_str().is_some_and(|f| f.contains("zbbr"))
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
        Ok(Tasmota { base, password, status: RefCell::new(Value::Null), zigbee: Cell::new(false), zigbee_asked_at: Cell::new(0), pairing_until: Cell::new(0) })
    }

    fn describe(&self) -> Result<Described> {
        let v = self.status()?;
        let mut devices = devices_of(&v);
        let zigbee = match self.zigbee_devices() {
            Ok(list) => Some(list),
            // A ZBBridge whose Zigbee is starting (just restarted) or slow: described again later,
            // rather than without its bridge and devices.
            Err(e) if zigbee_firmware(&v) && !matches!(e, Error::Unsupported(_)) => return Err(e),
            Err(_) => None,
        };
        self.zigbee.set(zigbee.is_some());
        self.zigbee_asked_at.set(host::now());
        if let Some(list) = zigbee {
            let name = v["Status"]["DeviceName"].as_str().filter(|n| !n.is_empty()).unwrap_or("Tasmota");
            devices.push(DeviceInfo { reference: zigbee::BRIDGE.into(), kind: "zigbee.bridge".into(), traits: vec!["zigbee.bridge@1".into()], name: format!("{name} · Zigbee") });
            devices.extend(list.iter().filter_map(zigbee::device_of));
        }
        let hardware = if zigbee_firmware(&v) { Some("SONOFF ZBBridge Pro") } else { v["StatusFWR"]["Hardware"].as_str() };
        Ok(Described {
            model: hardware.map(|h| format!("Tasmota · {h}")).unwrap_or_else(|| "Tasmota".into()),
            firmware: v["StatusFWR"]["Version"].as_str().map(str::to_string),
            devices,
            identity: v["StatusNET"]["Mac"].as_str().map(str::to_string),
        })
    }

    fn read(&self) -> Result<Vec<(String, State)>> {
        let status = self.status()?;
        let mut out = states_of(&status);
        // A ZBBridge whose Zigbee was off when described (no template yet, restarting): once a
        // minute, until it answers; its bridge then reaches Breezone, which describes it again.
        let now = host::now();
        if !self.zigbee.get() && zigbee_firmware(&status) && now - self.zigbee_asked_at.get() >= 60 {
            self.zigbee_asked_at.set(now);
            self.zigbee.set(self.zigbee_devices().is_ok());
        }
        if self.zigbee.get() {
            let devices = self.zigbee_devices()?;
            out.push((zigbee::BRIDGE.into(), self.bridge_state(&devices)));
            let now = host::now();
            out.extend(devices.iter().filter(|i| zigbee::device_of(i).is_some()).filter_map(|i| Some((zigbee::reference(i)?, zigbee::state_of(i, now)))));
        }
        Ok(out)
    }

    fn act(&self, reference: &str, trait_name: &str, action: &str, params: &State) -> Result<()> {
        if reference == zigbee::BRIDGE && (trait_name, action) == ("zigbee.bridge", "pair") {
            let on = params.get("on").and_then(Value::as_bool).unwrap_or(true);
            self.command(if on { "ZbPermitJoin 1" } else { "ZbPermitJoin 0" })?;
            self.pairing_until.set(if on { host::now() + zigbee::PAIRING_S } else { 0 });
            return Ok(());
        }
        if let Some(address) = reference.strip_prefix(&format!("{}/", zigbee::BRIDGE)) {
            if !matches!((trait_name, action), ("switch.relay" | "light", "set")) {
                return Err(Error::Unsupported(format!("{reference}: {trait_name}.{action}")));
            }
            for send in zigbee::sends(trait_name, params).map_err(Error::Unsupported)? {
                self.command(&format!("ZbSend {}", json!({"Device": address, "Send": send})))?;
            }
            return Ok(());
        }
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
        let zigbee = match self.zigbee_devices() {
            Ok(devices) => json!({"devices": devices, "unsupported": zigbee::unsupported(&devices)}),
            _ if zigbee_firmware(&v) => json!({"note": "Zigbee inactif : appliquer le modèle du ZBBridge Pro (Configuration › Modèle), puis redémarrer le pont."}),
            _ => Value::Null,
        };
        if let Some(o) = v.as_object_mut().filter(|_| !zigbee.is_null()) {
            o.insert("Zigbee".into(), zigbee);
        }
        Ok(v)
    }

    fn rename(config: Value, secret: Option<Value>, reference: &str, name: &str) -> Result<()> {
        let t = Self::open(config, secret)?;
        let name: String = name.trim().chars().take(32).collect();
        if let Some(address) = reference.strip_prefix(&format!("{}/", zigbee::BRIDGE)) {
            t.command(&format!("ZbName {address},{name}"))?;
            return Ok(());
        }
        match reference.strip_prefix("power").and_then(|n| n.parse::<u32>().ok()) {
            Some(n) => t.command(&format!("FriendlyName{n} {name}")).map(|_| ()),
            None => Err(Error::Unsupported(format!("{reference}: pas de nom sur le module"))),
        }
    }

    fn identify(host: &str, announced: Option<&Value>) -> Option<Found> {
        if announced.is_some() {
            return None;
        }
        Self::probe(host).map(|name| Found { model: "Tasmota".into(), note: Some(format!("« {name} »")), ..Found::default() })
    }

    fn probe(host: &str) -> Option<String> {
        let (_, v) = get(&format!("http://{host}"), "/cm?cmnd=Status%200", 2000).ok()?;
        if !is_tasmota(&v) {
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

    /// The ZBBridge Pro: no relay, a SNZB-02D, a plug and a door sensor on its network.
    fn bridge(seen: i64) {
        testing::reset();
        testing::answer_json(
            "GET",
            "cmnd=Status%200",
            &json!({
                "Status": {"DeviceName": "Pont Zigbee", "FriendlyName": ["Tasmota"]},
                "StatusSTS": {},
                "StatusSNS": {"TempUnit": "C"},
                "StatusFWR": {"Version": "16.0.0(release-zbbrdgpro)", "Hardware": "ESP32-D0WD-V3"},
                "StatusNET": {"Mac": "AA:BB:CC:DD:EE:01"}
            }),
        );
        testing::answer_json(
            "GET",
            "cmnd=ZbStatus1",
            &json!({"ZbStatus1": [{"Device": "0x6769", "Name": "Chambre"}, {"Device": "0x1D8A"}, {"Device": "0x2E01"}, {"Device": "0x3A10", "Name": "Plafonnier"}]}),
        );
        // One JSON, its key repeated: one message for each device.
        let info = |short: &str, more: Value| {
            let mut o = json!({"Device": short, "LinkQuality": 120, "Reachable": true});
            o.as_object_mut().unwrap().extend(more.as_object().unwrap().clone());
            format!("\"ZbInfo\":{{\"{short}\":{o}}}")
        };
        let body = format!(
            "{{{},{},{}}}",
            info(
                "0x6769",
                json!({"Name": "Chambre", "IEEEAddr": "0x00124B0026B64D1E", "ModelId": "SNZB-02D", "Manufacturer": "SONOFF", "Temperature": 19.6, "Humidity": 52.3, "BatteryPercentage": 100, "LastSeenEpoch": seen})
            ),
            info("0x1D8A", json!({"IEEEAddr": "0x7CB03EBB0A0292DD", "ModelId": "S26R2ZB", "Manufacturer": "SONOFF", "Power": 1})),
            info("0x3A10", json!({"Name": "Plafonnier", "IEEEAddr": "0x0017880102AABBCC", "ModelId": "LWB010", "Power": 1, "Dimmer": 127, "CT": 370})),
        );
        testing::answer("GET", "cmnd=ZbInfo", 200, body);
        // The door sensor, missed by the answer for all: asked alone.
        testing::answer_json(
            "GET",
            "cmnd=ZbInfo%200x2E01",
            &json!({"ZbInfo": {"0x2E01": {"Device": "0x2E01", "IEEEAddr": "0x00124B0022AA0001", "ModelId": "SNZB-04", "Manufacturer": "SONOFF", "Contact": 1, "BatteryPercentage": 80}}}),
        );
    }

    #[test]
    fn a_zigbee_bridge_its_sensor_and_its_plug() {
        bridge(host::now() - 60);
        let t: Tasmota = testing::open(json!({"host": "192.168.1.39"}), None);
        let s = testing::check(&t, MANIFEST);
        let d = t.describe().unwrap();
        let names: Vec<(&str, &str, &str)> = d.devices.iter().map(|x| (x.reference.as_str(), x.kind.as_str(), x.name.as_str())).collect();
        assert_eq!(
            names,
            [
                ("zigbee", "zigbee.bridge", "Pont Zigbee · Zigbee"),
                ("zigbee/0x00124B0026B64D1E", "sensor", "Chambre"),
                ("zigbee/0x7CB03EBB0A0292DD", "switch.relay", "SONOFF S26R2ZB"),
                ("zigbee/0x0017880102AABBCC", "light", "Plafonnier"),
            ],
            "the door sensor has nothing of Breezone yet"
        );
        assert_eq!(d.model, "Tasmota · SONOFF ZBBridge Pro");
        let sensor = &s["zigbee/0x00124B0026B64D1E"];
        assert_eq!((sensor["temperature"].clone(), sensor["humidity"].clone(), sensor["battery"].clone()), (json!(19.6), json!(52.3), json!(100.0)));
        assert_eq!(s["zigbee/0x7CB03EBB0A0292DD"]["on"], true);
        assert_eq!((s["zigbee"]["pairing"].clone(), s["zigbee"]["devices"].clone()), (json!(false), json!(4)));
        let lamp = &s["zigbee/0x0017880102AABBCC"];
        assert_eq!((lamp["on"].clone(), lamp["brightness"].clone(), lamp["color_temp"].clone(), lamp["reachable"].clone()), (json!(true), json!(50), json!(2703), json!(true)));
        let diag = t.diagnostic().unwrap();
        assert_eq!(diag["Zigbee"]["unsupported"][0]["model"], "SNZB-04");
        assert!(diag.get("StatusNET").is_none());
    }

    #[test]
    fn an_old_measure_is_not_told_but_the_battery_is() {
        bridge(host::now() - 4 * 3600);
        let t: Tasmota = testing::open(json!({"host": "192.168.1.39"}), None);
        let s = testing::check(&t, MANIFEST);
        let sensor = &s["zigbee/0x00124B0026B64D1E"];
        assert!(sensor.get("temperature").is_none());
        assert_eq!(sensor["battery"], json!(100.0));
    }

    #[test]
    fn it_opens_the_network_and_drives_a_zigbee_plug() {
        bridge(host::now());
        let t: Tasmota = testing::open(json!({"host": "192.168.1.39"}), None);
        t.describe().unwrap();
        testing::answer_json("GET", "cmnd=ZbPermitJoin", &json!({"ZbPermitJoin": "Done"}));
        testing::answer_json("GET", "cmnd=ZbSend", &json!({"ZbSend": "Done"}));
        let on = json!({"on": true});
        t.act("zigbee", "zigbee.bridge", "pair", on.as_object().unwrap()).unwrap();
        let s: std::collections::HashMap<String, State> = t.read().unwrap().into_iter().collect();
        assert_eq!(s["zigbee"]["pairing"], true);
        let off = json!({"on": false});
        t.act("zigbee", "zigbee.bridge", "pair", off.as_object().unwrap()).unwrap();
        t.act("zigbee/0x7CB03EBB0A0292DD", "switch.relay", "set", off.as_object().unwrap()).unwrap();
        let sent: Vec<String> = testing::requests().into_iter().filter(|r| r.contains("PermitJoin") || r.contains("ZbSend")).collect();
        assert_eq!(
            sent,
            [
                "GET http://192.168.1.39/cm?cmnd=ZbPermitJoin%201",
                "GET http://192.168.1.39/cm?cmnd=ZbPermitJoin%200",
                "GET http://192.168.1.39/cm?cmnd=ZbSend%20%7B%22Device%22%3A%220x7CB03EBB0A0292DD%22%2C%22Send%22%3A%7B%22Power%22%3A0%7D%7D",
            ]
        );
        assert!(matches!(t.act("zigbee/0x00124B0026B64D1E", "cover.position", "set", on.as_object().unwrap()), Err(Error::Unsupported(_))));
    }

    #[test]
    fn tasmota_16_is_asked_with_a_referer_and_found_by_its_status() {
        testing::reset();
        testing::respond("GET", "/cm?", |headers, _| {
            let body = if headers.iter().any(|(n, v)| n.eq_ignore_ascii_case("referer") && *v == "http://192.168.1.39/") {
                br#"{"Status": {"DeviceName": "Pont Zigbee"}, "StatusFWR": {"Version": "16.0.0(release-zbbrdgpro)"}}"#.to_vec()
            } else {
                vec![]
            };
            host::Answer { status: 200, headers: vec![], body }
        });
        assert_eq!(Tasmota::probe("192.168.1.39").as_deref(), Some("Pont Zigbee"));
    }

    #[test]
    fn a_zbbridge_whose_zigbee_is_off_says_what_to_do() {
        testing::reset();
        testing::answer_json(
            "GET",
            "cmnd=Status%200",
            &json!({"Status": {"DeviceName": "Pont"}, "StatusSTS": {}, "StatusSNS": {}, "StatusFWR": {"Version": "16.0.0(release-zbbrdgpro)"}}),
        );
        testing::answer_json("GET", "cmnd=Zb", &json!({"Command": "Unknown"}));
        let t: Tasmota = testing::open(json!({"host": "192.168.1.39"}), None);
        assert!(t.describe().unwrap().devices.is_empty());
        assert!(t.read().unwrap().is_empty());
        let asked = || testing::requests().iter().filter(|r| r.contains("cmnd=ZbStatus1")).count();
        assert_eq!(asked(), 1, "no Zigbee asked at each reading");
        assert!(t.diagnostic().unwrap()["Zigbee"]["note"].as_str().unwrap().contains("modèle"));
    }

    #[test]
    fn a_zbbridge_whose_zigbee_starts_later_brings_its_bridge_at_a_reading() {
        testing::reset();
        let status = json!({"Status": {"DeviceName": "Tasmota"}, "StatusSTS": {}, "StatusSNS": {}, "StatusFWR": {"Version": "16.0.0(release-zbbrdgpro)"}});
        testing::answer_json("GET", "cmnd=Status%200", &status);
        testing::answer_json("GET", "cmnd=Zb", &json!({"Command": "Unknown"}));
        let t: Tasmota = testing::open(json!({"host": "192.168.1.39"}), None);
        assert!(t.describe().unwrap().devices.is_empty(), "its template not applied yet");
        // The template applied, the bridge restarted: its Zigbee answers.
        testing::answer_json("GET", "cmnd=ZbStatus1", &json!({"ZbStatus1": []}));
        testing::answer_json("GET", "cmnd=ZbInfo", &json!({"ZbInfo": {}}));
        assert!(t.read().unwrap().is_empty(), "asked again a minute later only");
        t.zigbee_asked_at.set(host::now() - 61);
        let s: std::collections::HashMap<String, State> = t.read().unwrap().into_iter().collect();
        assert_eq!(s["zigbee"]["devices"], 0);
        assert_eq!(t.describe().unwrap().devices.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(), ["Tasmota · Zigbee"]);
    }

    #[test]
    fn a_zbbridge_whose_zigbee_does_not_answer_is_not_described_without_it() {
        testing::reset();
        let status = json!({"Status": {"DeviceName": "Tasmota"}, "StatusSTS": {}, "StatusSNS": {}, "StatusFWR": {"Version": "16.0.0(release-zbbrdgpro)"}});
        testing::answer_json("GET", "cmnd=Status%200", &status);
        testing::answer("GET", "cmnd=ZbStatus1", 200, "");
        let t: Tasmota = testing::open(json!({"host": "192.168.1.39"}), None);
        assert!(t.describe().is_err(), "described again later, its bridge and devices kept");
    }

    #[test]
    fn a_word_after_the_data_under_the_same_key_keeps_the_data() {
        let v = parse(br#"{"ZbInfo":{"0x9B58":{"Device":"0x9B58","ModelId":"SNZB-02D","Temperature":17.9}},"ZbInfo":"Done"}"#).unwrap();
        assert_eq!(v["ZbInfo"]["0x9B58"]["Temperature"], 17.9);
    }

    #[test]
    fn a_sensor_paired_on_tasmota_16_is_described() {
        testing::reset();
        let status = json!({"Status": {"DeviceName": "Tasmota"}, "StatusSTS": {}, "StatusSNS": {}, "StatusFWR": {"Version": "16.0.0(release-zbbrdgpro)"}});
        testing::answer_json("GET", "cmnd=Status%200", &status);
        testing::answer_json("GET", "cmnd=ZbStatus1", &json!({"ZbStatus1": [{"Device": "0x9B58"}]}));
        // As Tasmota 16 answers it, word for word.
        testing::answer(
            "GET",
            "cmnd=ZbInfo",
            200,
            r#"{"ZbInfo":{"0x9B58":{"Device":"0x9B58","IEEEAddr":"0x18690AFFFE1E5ED9","ModelId":"SNZB-02D","Manufacturer":"SONOFF","Endpoints":[1],"Config":["T01"],"Temperature":17.9,"Humidity":57.4,"Reachable":true,"BatteryPercentage":100,"BatteryLastSeenEpoch":1795764526,"LastSeen":29,"LastSeenEpoch":1795764547,"LinkQuality":72}},"ZbInfo":"Done"}"#,
        );
        let t: Tasmota = testing::open(json!({"host": "192.168.1.39"}), None);
        let d = t.describe().unwrap();
        let sensor = d.devices.iter().find(|x| x.reference == "zigbee/0x18690AFFFE1E5ED9").expect("the sensor is described");
        assert_eq!((sensor.name.as_str(), sensor.kind.as_str()), ("SONOFF SNZB-02D", "sensor"));
        assert!(sensor.traits.iter().any(|t| t.starts_with("environment.sensor@")));
    }

    #[test]
    fn answers_joined_one_after_the_other_are_merged() {
        let v = parse(br#"{"ZbInfo":{"0x1":{"Temperature":20}}}{"ZbInfo":{"0x2":{"Power":0}}}"#).unwrap();
        assert_eq!((v["ZbInfo"]["0x1"]["Temperature"].clone(), v["ZbInfo"]["0x2"]["Power"].clone()), (json!(20), json!(0)));
        assert!(matches!(parse(b""), Err(Error::Invalid(_))));
    }

    #[test]
    fn a_zigbee_lamp_is_dimmed_but_its_colours_are_not_driven() {
        bridge(host::now());
        let t: Tasmota = testing::open(json!({"host": "192.168.1.39"}), None);
        testing::answer_json("GET", "cmnd=ZbSend", &json!({"ZbSend": "Done"}));
        let set = json!({"on": true, "brightness": 100, "color_temp": 2700});
        t.act("zigbee/0x0017880102AABBCC", "light", "set", set.as_object().unwrap()).unwrap();
        let sent: Vec<String> = testing::requests().into_iter().filter(|r| r.contains("ZbSend")).map(|r| r.split("ZbSend%20").nth(1).unwrap().to_string()).collect();
        let decoded: Vec<Value> = sent.iter().map(|s| serde_json::from_str::<Value>(&decode(s)).unwrap()["Send"].clone()).collect();
        assert_eq!(decoded, [json!({"CT": 370}), json!({"Dimmer": 254}), json!({"Power": 1})]);
        let colour = json!({"color": "#ff8800"});
        assert!(matches!(t.act("zigbee/0x0017880102AABBCC", "light", "set", colour.as_object().unwrap()), Err(Error::Unsupported(_))));
    }

    /// `%7B` → `{`, for reading the commands sent.
    fn decode(s: &str) -> String {
        let mut out = vec![];
        let b = s.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'%' {
                out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap());
                i += 3;
            } else {
                out.push(b[i]);
                i += 1;
            }
        }
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn a_name_given_in_breezone_goes_to_the_bridge_and_to_a_relay() {
        testing::reset();
        testing::answer_json("GET", "/cm?", &json!({"ZbName": "Done"}));
        let config = || json!({"host": "192.168.1.39"});
        Tasmota::rename(config(), None, "zigbee/0x00124B0026B64D1E", "  Chambre d'amis ").unwrap();
        Tasmota::rename(config(), Some(json!({"password": "pw"})), "power2", "Télé").unwrap();
        assert!(matches!(Tasmota::rename(config(), None, "energy", "Mesure"), Err(Error::Unsupported(_))));
        assert_eq!(
            testing::requests(),
            [
                "GET http://192.168.1.39/cm?cmnd=ZbName%200x00124B0026B64D1E%2CChambre%20d%27amis",
                "GET http://192.168.1.39/cm?cmnd=FriendlyName2%20T%C3%A9l%C3%A9&user=admin&password=pw",
            ]
        );
    }
}
