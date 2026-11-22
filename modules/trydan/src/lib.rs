//! V2C Trydan charger, through its local HTTP API (no password: a fixed address is advised).
//!
//! * `GET /RealTimeData`: the whole state, in JSON.
//! * `GET /write/{Keyword}={value}`: changes one setting, answers `OK`.
//!
//! The charger is one device with `ev.charger@1` and `energy.power@1`: it can be given the role of
//! a consumption on the energy page. Its own clamp readings (`HousePower`, `FVPower`) are not used.
//!
//! With `Dynamic` on, the charger sets its current itself from its clamp; Breezone then leaves it
//! (`auto_limit`). Starting with a maximum current turns `Dynamic` off. Charging is started and
//! stopped with `Paused`.

use breezone_module_sdk::{device, host, json, module, Described, Driver, Error, Found, Map, Result, State, Value};

/// The only device of a charger.
pub const REFERENCE: &str = "charger";

/// What answers `/RealTimeData` at an address.
#[derive(Debug, Clone, PartialEq)]
pub struct Identified {
    /// "XY99R6".
    pub id: String,
    pub firmware: Option<String>,
    /// "monophasé · 6–32 A".
    pub note: Option<String>,
}

pub fn identify(data: &Value) -> Option<Identified> {
    let o = data.as_object()?;
    // `ChargeState` and `Intensity` together: a Trydan, not just any JSON.
    if !o.contains_key("ChargeState") || !o.contains_key("Intensity") {
        return None;
    }
    let phases = match o.get("ChargeMode").and_then(Value::as_i64) {
        Some(0) => Some("monophasé"),
        Some(1) => Some("triphasé"),
        _ => None,
    };
    let range = match (o.get("MinIntensity").and_then(Value::as_i64), o.get("MaxIntensity").and_then(Value::as_i64)) {
        (Some(min), Some(max)) => Some(format!("{min}–{max} A")),
        _ => None,
    };
    let note = [phases.map(str::to_string), range].into_iter().flatten().collect::<Vec<_>>().join(" · ");
    Some(Identified {
        id: o.get("ID").and_then(Value::as_str).unwrap_or_default().to_string(),
        firmware: o.get("FirmwareVersion").and_then(Value::as_str).map(str::to_string),
        note: (!note.is_empty()).then_some(note),
    })
}

/// Host or `host:port` on the local network: letters, digits, dots, dashes, colons.
fn valid_host(host: &str) -> bool {
    !host.is_empty() && host.len() <= 253 && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'))
}

pub struct Trydan {
    base: String,
    /// Id of the charger expected at this address (from the scanner): another one is refused.
    device_id: Option<String>,
}

impl Trydan {
    fn data(&self) -> Result<Map<String, Value>> {
        let answer = host::request("GET", &format!("{}/RealTimeData", self.base), &[], None, 5000)?;
        if !(200..300).contains(&answer.status) {
            return Err(Error::Invalid(format!("RealTimeData: HTTP {}", answer.status)));
        }
        match answer.json()? {
            Value::Object(o) => Ok(o),
            _ => Err(Error::Invalid("RealTimeData: not an object".into())),
        }
    }

    fn write(&self, keyword: &str, value: i64) -> Result<()> {
        let answer = host::request("GET", &format!("{}/write/{keyword}={value}", self.base), &[], None, 5000)?;
        let body = String::from_utf8_lossy(&answer.body);
        if !(200..300).contains(&answer.status) || body.trim() != "OK" {
            return Err(Error::Invalid(format!("{keyword}={value}: HTTP {} {}", answer.status, body.trim())));
        }
        Ok(())
    }
}

fn num(o: &Map<String, Value>, key: &str) -> Option<f64> {
    o.get(key).and_then(Value::as_f64)
}

fn int(o: &Map<String, Value>, key: &str) -> Option<i64> {
    o.get(key).and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f.round() as i64)))
}

/// The state of the charger as its traits describe it.
pub fn state_of(o: &Map<String, Value>) -> State {
    let mut s = State::new();
    let status = match int(o, "ChargeState") {
        Some(0) => "available",
        Some(1) => "plugged",
        Some(2) => "charging",
        _ => "fault",
    };
    s.insert("status".into(), json!(status));
    // Unknown during a fault.
    let vehicle = match int(o, "ChargeState") {
        Some(0) => Some(false),
        Some(1 | 2) => Some(true),
        _ => None,
    };
    if let Some(v) = vehicle {
        s.insert("vehicle".into(), json!(v));
    }
    s.insert("paused".into(), json!(int(o, "Paused") == Some(1)));
    s.insert("locked".into(), json!(int(o, "Locked") == Some(1)));
    s.insert("session_energy".into(), json!(num(o, "ChargeEnergy").unwrap_or(0.0)));
    s.insert("session_secs".into(), json!(int(o, "ChargeTime").unwrap_or(0)));
    // 0 while nothing is asked (no car, or the charger regulates itself).
    if let Some(i) = int(o, "Intensity").filter(|i| *i > 0) {
        s.insert("limit".into(), json!(i));
    }
    for (key, field) in [("MinIntensity", "limit_min"), ("MaxIntensity", "limit_max")] {
        if let Some(i) = int(o, key) {
            s.insert(field.into(), json!(i));
        }
    }
    // To turn a power into a current: 1 or 3 phases.
    let phases = match int(o, "ChargeMode") {
        Some(0) => Some(1),
        Some(1) => Some(3),
        _ => None,
    };
    if let Some(p) = phases {
        s.insert("phases".into(), json!(p));
    }
    if let Some(d) = int(o, "Dynamic") {
        s.insert("auto_limit".into(), json!(d == 1));
    }
    s.insert("power".into(), json!(num(o, "ChargePower").unwrap_or(0.0)));
    if let Some(v) = num(o, "VoltageInstallation").filter(|v| *v > 0.0) {
        s.insert("voltage".into(), json!(v));
    }
    // The measured current, for a single-phase charge.
    if int(o, "ChargeMode") == Some(0) {
        if let Some(a) = num(o, "IntensityMeasure_L1") {
            s.insert("current".into(), json!(a));
        }
    }
    s
}

impl Driver for Trydan {
    fn open(config: Value, _secret: Option<Value>) -> Result<Self> {
        let host = config["host"].as_str().map(str::trim).unwrap_or_default();
        if !valid_host(host) {
            return Err(Error::Invalid("invalid Trydan address".into()));
        }
        let device_id = config["device_id"].as_str().map(str::trim).filter(|d| !d.is_empty()).map(str::to_string);
        Ok(Trydan { base: format!("http://{host}"), device_id })
    }

    fn describe(&self) -> Result<Described> {
        let found = identify(&Value::Object(self.data()?)).ok_or_else(|| Error::Invalid("not a V2C Trydan".into()))?;
        if self.device_id.as_ref().is_some_and(|expected| *expected != found.id) {
            return Err(Error::Invalid(format!("another charger answers at this address ({}): scan the network again", found.id)));
        }
        let model = match &found.note {
            Some(n) => format!("V2C Trydan ({n})"),
            None => "V2C Trydan".into(),
        };
        Ok(Described {
            model,
            firmware: found.firmware,
            identity: (!found.id.is_empty()).then(|| found.id.clone()),
            devices: vec![device(REFERENCE, "ev.charger", &["ev.charger@1", "energy.power@1"], "Trydan")],
        })
    }

    fn read(&self) -> Result<Vec<(String, State)>> {
        Ok(vec![(REFERENCE.to_string(), state_of(&self.data()?))])
    }

    fn act(&self, reference: &str, trait_name: &str, action: &str, params: &State) -> Result<()> {
        let flag = |name: &str| params.get(name).and_then(Value::as_bool).map(i64::from).ok_or_else(|| Error::Invalid(format!("`{name}` is required")));
        match (reference, trait_name, action) {
            (REFERENCE, "ev.charger", "set_limit") => {
                let limit = params.get("limit").and_then(Value::as_i64).ok_or_else(|| Error::Invalid("`limit` is required".into()))?;
                self.write("Intensity", limit)
            }
            (REFERENCE, "ev.charger", "start") => {
                // A maximum: Breezone sets the current, the charger no longer regulates it.
                if let Some(limit) = params.get("limit").and_then(Value::as_i64) {
                    self.write("Dynamic", 0)?;
                    self.write("Intensity", limit)?;
                }
                self.write("Paused", 0)
            }
            (REFERENCE, "ev.charger", "stop") => self.write("Paused", 1),
            (REFERENCE, "ev.charger", "lock") => self.write("Locked", flag("locked")?),
            (REFERENCE, "ev.charger", "set_auto_limit") => self.write("Dynamic", flag("on")?),
            _ => Err(Error::Unsupported(format!("{reference}: {trait_name}.{action}"))),
        }
    }

    fn diagnostic(&self) -> Result<Value> {
        let mut d = self.data()?;
        // Where it is, not what it is.
        for k in ["SSID", "IP"] {
            d.remove(k);
        }
        Ok(Value::Object(d))
    }

    fn identify(host_: &str, announced: Option<&Value>) -> Option<Found> {
        if announced.is_some() {
            return None;
        }
        let (_, data) = host::get_json(&format!("http://{host_}/RealTimeData"), 2000).ok()?;
        let t = identify(&data)?;
        Some(Found { model: "V2C Trydan".into(), note: t.note, device_id: (!t.id.is_empty()).then_some(t.id), firmware: t.firmware, ..Found::default() })
    }

    fn probe(host_: &str) -> Option<String> {
        let (_, data) = host::get_json(&format!("http://{host_}/RealTimeData"), 2000).ok()?;
        identify(&data).map(|_| "V2C Trydan".to_string())
    }
}

module!(Trydan);

#[cfg(test)]
mod tests {
    use super::*;
    use breezone_module_sdk::testing;

    const MANIFEST: &str = include_str!("../manifest.json");

    /// A real answer of a Trydan (firmware 2.5.1), no car plugged in.
    fn idle() -> Value {
        json!({"ID":"XY99R6","FirmwareVersion":"2.5.1","ChargeState":0,"ChargeMode":0,"ReadyState":0,"ChargePower":0.00,"VoltageInstallation":222,
            "ChargeEnergy":0.00,"SlaveError":0,"ChargeTime":0,"GridPower":0.00,"HousePower":0.00,"FVPower":0.00,"BatteryPower":0.00,"Paused":0,"Locked":0,
            "Timer":1,"Intensity":0,"IntensityMeasure_L1":0.00,"IntensityMeasure_L2":0.00,"IntensityMeasure_L3":0.00,"VoltageMeasure_L1":0.00,
            "VoltageMeasure_L2":0.00,"VoltageMeasure_L3":0.00,"Dynamic":1,"MinIntensity":6,"MaxIntensity":32,"PauseDynamic":0,"LightLED":5,"LogoLED":0,
            "DynamicPowerMode":0,"ContractedPower":1400,"SSID":"saniv","IP":"192.168.86.37","SignalStatus":2})
    }

    #[test]
    fn a_trydan_is_recognised() {
        let found = identify(&idle()).unwrap();
        assert_eq!(found.id, "XY99R6");
        assert_eq!(found.firmware.as_deref(), Some("2.5.1"));
        assert_eq!(found.note.as_deref(), Some("monophasé · 6–32 A"));
        assert_eq!(identify(&json!({"ChargeState": 0})), None);
        assert_eq!(identify(&json!([1, 2])), None);
    }

    #[test]
    fn an_idle_charger_regulating_itself() {
        let s = state_of(idle().as_object().unwrap());
        assert_eq!(s["status"], "available");
        assert_eq!(s["vehicle"], false);
        assert_eq!(s["auto_limit"], true);
        assert!(!s.contains_key("limit"), "nothing asked");
        assert_eq!((s["limit_min"].clone(), s["limit_max"].clone()), (json!(6), json!(32)));
        assert_eq!(s["power"], 0.0);
        assert_eq!(s["voltage"], 222.0);
    }

    #[test]
    fn a_charging_car() {
        let mut d = idle();
        let o = d.as_object_mut().unwrap();
        for (k, v) in [
            ("ChargeState", json!(2)),
            ("ChargePower", json!(2070.5)),
            ("ChargeEnergy", json!(8.62)),
            ("ChargeTime", json!(11880)),
            ("Intensity", json!(9)),
            ("IntensityMeasure_L1", json!(9.1)),
            ("Paused", json!(0)),
            ("Dynamic", json!(0)),
        ] {
            o.insert(k.into(), v);
        }
        let s = state_of(o);
        assert_eq!(s["status"], "charging");
        assert_eq!(s["vehicle"], true);
        assert_eq!((s["power"].clone(), s["session_energy"].clone(), s["session_secs"].clone()), (json!(2070.5), json!(8.62), json!(11880)));
        assert_eq!((s["limit"].clone(), s["current"].clone(), s["auto_limit"].clone()), (json!(9), json!(9.1), json!(false)));
        o.insert("ChargeState".into(), json!(5));
        assert_eq!(state_of(o)["status"], "fault");
        assert!(!state_of(o).contains_key("vehicle"), "unknown during a fault");
    }

    #[test]
    fn read_checked_and_driven() {
        testing::reset();
        testing::answer_json("GET", "/RealTimeData", &idle());
        testing::answer("GET", "/write/", 200, "OK");
        let t: Trydan = testing::open(json!({"host": "192.168.1.37", "device_id": "XY99R6"}), None);
        let s = testing::check(&t, MANIFEST);
        assert_eq!(s[REFERENCE]["status"], "available");
        t.act(REFERENCE, "ev.charger", "start", json!({"limit": 10}).as_object().unwrap()).unwrap();
        let writes: Vec<String> = testing::requests().into_iter().filter(|r| r.contains("/write/")).collect();
        assert_eq!(writes, ["GET http://192.168.1.37/write/Dynamic=0", "GET http://192.168.1.37/write/Intensity=10", "GET http://192.168.1.37/write/Paused=0"]);
        testing::answer("GET", "/write/", 200, "ERROR");
        assert!(matches!(t.act(REFERENCE, "ev.charger", "stop", &State::new()), Err(Error::Invalid(_))));
        let other: Trydan = testing::open(json!({"host": "192.168.1.37", "device_id": "OTHER"}), None);
        assert!(matches!(other.describe(), Err(Error::Invalid(m)) if m.contains("another charger")));
        assert_eq!(Trydan::probe("192.168.1.37").as_deref(), Some("V2C Trydan"));
        assert!(t.diagnostic().unwrap().get("SSID").is_none());
    }
}
