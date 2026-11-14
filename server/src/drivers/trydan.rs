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

use super::{ConfigField, Described, DeviceInfo, Driver, DriverError, DriverFuture, DriverInfo, State};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::time::Duration;

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

/// A Trydan at `host`, if it is one (the network scanner).
pub async fn probe(host: &str) -> Option<Identified> {
    let http = reqwest::Client::builder().timeout(Duration::from_secs(2)).build().ok()?;
    let data: Value = http.get(format!("http://{host}/RealTimeData")).send().await.ok()?.json().await.ok()?;
    identify(&data)
}

pub fn info() -> DriverInfo {
    DriverInfo {
        id: "trydan",
        label: "V2C Trydan",
        provides: vec!["ev.charger@1", "energy.power@1"],
        models: vec!["V2C Trydan".into()],
        config: vec![ConfigField { name: "host", kind: "string", optional: false, label: crate::text!("Adresse IP ou nom", "IP address or name", "Dirección IP o nombre") }],
    }
}

pub struct Trydan {
    http: reqwest::Client,
    base: String,
    /// Id of the charger expected at this address (from the scanner): another one is refused.
    device_id: Option<String>,
}

impl Trydan {
    pub fn new(config: &Value) -> Result<Self, String> {
        let host = config["host"].as_str().map(str::trim).unwrap_or_default();
        if !super::shelly::valid_host(host) {
            return Err("invalid Trydan address".into());
        }
        let http = reqwest::Client::builder().timeout(Duration::from_secs(5)).build().map_err(|e| e.to_string())?;
        let device_id = config["device_id"].as_str().map(str::trim).filter(|d| !d.is_empty()).map(str::to_string);
        Ok(Self { http, base: format!("http://{host}"), device_id })
    }

    fn unreachable(e: reqwest::Error) -> DriverError {
        DriverError::Unreachable(if e.is_timeout() { "timed out".into() } else { e.to_string() })
    }

    async fn data(&self) -> Result<Map<String, Value>, DriverError> {
        let resp = self.http.get(format!("{}/RealTimeData", self.base)).send().await.map_err(Self::unreachable)?;
        if !resp.status().is_success() {
            return Err(DriverError::Invalid(format!("RealTimeData: HTTP {}", resp.status())));
        }
        match resp.json::<Value>().await {
            Ok(Value::Object(o)) => Ok(o),
            Ok(_) => Err(DriverError::Invalid("RealTimeData: not an object".into())),
            Err(e) => Err(DriverError::Invalid(format!("RealTimeData: {e}"))),
        }
    }

    async fn write(&self, keyword: &str, value: i64) -> Result<(), DriverError> {
        let resp = self.http.get(format!("{}/write/{keyword}={value}", self.base)).send().await.map_err(Self::unreachable)?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() || body.trim() != "OK" {
            return Err(DriverError::Invalid(format!("{keyword}={value}: HTTP {status} {}", body.trim())));
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
    fn describe(&self) -> DriverFuture<'_, Described> {
        Box::pin(async move {
            let data = self.data().await?;
            let found = identify(&Value::Object(data)).ok_or_else(|| DriverError::Invalid("not a V2C Trydan".into()))?;
            if self.device_id.as_ref().is_some_and(|expected| *expected != found.id) {
                return Err(DriverError::Invalid(format!("another charger answers at this address ({}): scan the network again", found.id)));
            }
            let model = match &found.note {
                Some(n) => format!("V2C Trydan ({n})"),
                None => "V2C Trydan".into(),
            };
            Ok(Described {
                model,
                firmware: found.firmware,
                identity: (!found.id.is_empty()).then(|| found.id.clone()),
                devices: vec![DeviceInfo {
                    reference: REFERENCE.into(),
                    kind: "ev.charger".into(),
                    traits: vec!["ev.charger@1".into(), "energy.power@1".into()],
                    name: "Trydan".into(),
                }],
            })
        })
    }

    fn read(&self) -> DriverFuture<'_, HashMap<String, State>> {
        Box::pin(async move { Ok(HashMap::from([(REFERENCE.to_string(), state_of(&self.data().await?))])) })
    }

    fn act<'a>(&'a self, reference: &'a str, trait_name: &'a str, action: &'a str, params: &'a State) -> DriverFuture<'a, ()> {
        Box::pin(async move {
            let flag = |name: &str| params.get(name).and_then(Value::as_bool).map(i64::from).ok_or_else(|| DriverError::Invalid(format!("`{name}` is required")));
            match (reference, trait_name, action) {
                (REFERENCE, "ev.charger", "set_limit") => {
                    let limit = params.get("limit").and_then(Value::as_i64).ok_or_else(|| DriverError::Invalid("`limit` is required".into()))?;
                    self.write("Intensity", limit).await
                }
                (REFERENCE, "ev.charger", "start") => {
                    // A maximum: Breezone sets the current, the charger no longer regulates it.
                    if let Some(limit) = params.get("limit").and_then(Value::as_i64) {
                        self.write("Dynamic", 0).await?;
                        self.write("Intensity", limit).await?;
                    }
                    self.write("Paused", 0).await
                }
                (REFERENCE, "ev.charger", "stop") => self.write("Paused", 1).await,
                (REFERENCE, "ev.charger", "lock") => self.write("Locked", flag("locked")?).await,
                (REFERENCE, "ev.charger", "set_auto_limit") => self.write("Dynamic", flag("on")?).await,
                _ => Err(DriverError::Unsupported(format!("{reference}: {trait_name}.{action}"))),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
