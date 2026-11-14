//! Shelly, second generation and later (Plus, Pro, Gen3, Gen4): local RPC API over HTTP
//! (`/rpc/<Method>`), no account and no cloud.
//!
//! Components turned into devices:
//! * `em:N` (Pro 3EM, three phases) → `energy.power@1`, with `energy.meter@1` from `emdata:N`;
//! * `em1:N` (Pro EM, one phase each) → `energy.power@1`, with `energy.meter@1` from `em1data:N`;
//! * `switch:N` (Plus 1PM, Plug S, Pro…) → `switch.relay@1`, with power and energy when measured;
//! * `pm1:N` (Plus PM Mini) → `energy.power@1`, `energy.meter@1`.

use super::{ConfigField, Described, DeviceInfo, Driver, DriverError, DriverFuture, DriverInfo, State};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::time::Duration;

/// Shelly models by firmware application (`app` of `/shelly`): the name shown, and a note when the
/// profile changes what the device exposes. Others of the second generation and later are
/// supported generically, from the components they report.
const MODELS: &[(&str, &str, Option<&str>)] = &[
    ("Pro3EM", "Shelly Pro 3EM", Some("triphasé")),
    ("Pro3EMM", "Shelly Pro 3EM", Some("monophasé : 3 voies indépendantes")),
    ("ProEM", "Shelly Pro EM", None),
    ("EMG3", "Shelly EM Gen3", None),
    ("PlugS", "Shelly Plus Plug S", None),
    ("PlugSG3", "Shelly Plug S Gen3", None),
    ("Plus1", "Shelly Plus 1", None),
    ("Plus1PM", "Shelly Plus 1PM", None),
    ("Plus2PM", "Shelly Plus 2PM", None),
    ("PlusPMMini", "Shelly Plus PM Mini", None),
    ("Pro1", "Shelly Pro 1", None),
    ("Pro1PM", "Shelly Pro 1PM", None),
    ("Pro2PM", "Shelly Pro 2PM", None),
    ("Pro4PM", "Shelly Pro 4PM", None),
    ("S1G3", "Shelly 1 Gen3", None),
    ("S1PMG3", "Shelly 1PM Gen3", None),
    ("PMMiniG3", "Shelly PM Mini Gen3", None),
];

/// A Shelly found on the network (`GET /shelly`, open on every Shelly).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Identified {
    /// Device id ("shellypro3em-8c4f009097c4"): stable, kept to recognise it if its address changes.
    pub id: String,
    pub mac: Option<String>,
    /// Name to show ("Shelly Pro 3EM").
    pub model: String,
    /// Manufacturer code ("SPEM-003CEBEU").
    pub code: Option<String>,
    pub generation: u32,
    pub firmware: Option<String>,
    /// Profile or particularity ("monophasé : 3 voies indépendantes").
    pub note: Option<String>,
    /// This driver can read it (second generation and later).
    pub supported: bool,
    /// The device asks for a password (not supported yet).
    pub auth: bool,
}

/// What a `/shelly` answer says.
pub fn identify(info: &Value) -> Option<Identified> {
    let field = |k: &str| info.get(k).and_then(Value::as_str).map(str::to_string);
    if let Some(app) = field("app") {
        let generation = info["gen"].as_u64().unwrap_or(2) as u32;
        let known = MODELS.iter().find(|(a, ..)| *a == app);
        let auth = info["auth_en"].as_bool().unwrap_or(false);
        return Some(Identified {
            id: field("id")?,
            mac: field("mac"),
            model: known.map(|(_, name, _)| name.to_string()).unwrap_or_else(|| format!("Shelly {app}")),
            code: field("model"),
            generation,
            firmware: field("ver"),
            note: known.and_then(|(_, _, n)| n.map(str::to_string)).or_else(|| auth.then(|| "protégé par mot de passe".into())),
            supported: generation >= 2 && !auth,
            auth,
        });
    }
    // First generation: another API (`/status`), not supported.
    let kind = field("type")?;
    Some(Identified {
        id: field("mac").map(|m| format!("shelly1g-{}", m.to_lowercase())).unwrap_or_else(|| kind.clone()),
        mac: field("mac"),
        model: format!("Shelly {kind}"),
        code: Some(kind),
        generation: 1,
        firmware: field("fw"),
        note: Some("1re génération : pas encore prise en charge".into()),
        supported: false,
        auth: info["auth"].as_bool().unwrap_or(false),
    })
}

/// Asks `host` whether it is a Shelly (scanner).
pub async fn probe(host: &str) -> Option<Identified> {
    let http = reqwest::Client::builder().timeout(Duration::from_secs(2)).build().ok()?;
    let info: Value = http.get(format!("http://{host}/shelly")).send().await.ok()?.json().await.ok()?;
    identify(&info)
}

pub fn info() -> DriverInfo {
    DriverInfo {
        id: "shelly",
        label: "Shelly (Plus, Pro, Gen3, Gen4)",
        provides: vec!["energy.power@1", "energy.meter@1", "switch.relay@1"],
        models: MODELS
            .iter()
            .map(|(_, name, note)| match note {
                Some(n) => format!("{name} ({n})"),
                None => name.to_string(),
            })
            .collect(),
        config: vec![ConfigField { name: "host", kind: "string", optional: false, label: crate::text!("Adresse IP ou nom", "IP address or name", "Dirección IP o nombre") }],
    }
}

pub struct Shelly {
    http: reqwest::Client,
    base: String,
    /// Id of the device expected at this address (from the scanner): another one is refused.
    device_id: Option<String>,
}

/// Host or `host:port` on the local network: letters, digits, dots, dashes, colons.
pub(super) fn valid_host(host: &str) -> bool {
    !host.is_empty() && host.len() <= 253 && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'))
}

impl Shelly {
    pub fn new(config: &Value) -> Result<Self, String> {
        let host = config["host"].as_str().map(str::trim).unwrap_or_default();
        if !valid_host(host) {
            return Err("invalid Shelly address".into());
        }
        let http = reqwest::Client::builder().timeout(Duration::from_secs(5)).build().map_err(|e| e.to_string())?;
        let device_id = config["device_id"].as_str().map(str::trim).filter(|d| !d.is_empty()).map(str::to_string);
        Ok(Self { http, base: format!("http://{host}"), device_id })
    }

    async fn rpc(&self, method: &str, query: &[(&str, String)]) -> Result<Value, DriverError> {
        let resp = self
            .http
            .get(format!("{}/rpc/{method}", self.base))
            .query(query)
            .send()
            .await
            .map_err(|e| DriverError::Unreachable(if e.is_timeout() { "timed out".into() } else { e.to_string() }))?;
        if !resp.status().is_success() {
            return Err(DriverError::Invalid(format!("{method}: HTTP {}", resp.status())));
        }
        resp.json().await.map_err(|e| DriverError::Invalid(format!("{method}: {e}")))
    }
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64()
}

/// Wh → kWh, rounded to the Wh.
fn kwh(v: &Value) -> Option<f64> {
    v.as_f64().map(|wh| (wh / 1000.0 * 1000.0).round() / 1000.0)
}

fn put(state: &mut State, key: &str, value: Option<f64>) {
    if let Some(v) = value {
        state.insert(key.into(), json!(v));
    }
}

/// `"switch:1"` → `("switch", 1)`.
fn component(key: &str) -> Option<(&str, u32)> {
    let (kind, n) = key.split_once(':')?;
    Some((kind, n.parse().ok()?))
}

/// Devices found in a `Shelly.GetStatus` answer (`model`: the name shown, "Shelly Pro 3EM").
fn devices_of(model: &str, status: &Map<String, Value>) -> Vec<DeviceInfo> {
    let app = model.strip_prefix("Shelly ").unwrap_or(model);
    let mut keys: Vec<&String> = status.keys().collect();
    keys.sort();
    let mut out = vec![];
    for key in keys {
        let Some((kind, n)) = component(key) else { continue };
        let c = &status[key];
        let label = |what: &str| if n == 0 { format!("{app} {what}") } else { format!("{app} {what} {}", n + 1) };
        let device = match kind {
            "em" => {
                let mut traits = vec!["energy.power@1".to_string()];
                if status.contains_key(&format!("emdata:{n}")) {
                    traits.push("energy.meter@1".into());
                }
                Some(("energy.power", traits, label("compteur")))
            }
            "em1" => {
                let mut traits = vec!["energy.power@1".to_string()];
                if status.contains_key(&format!("em1data:{n}")) {
                    traits.push("energy.meter@1".into());
                }
                Some(("energy.power", traits, format!("{app} voie {}", n + 1)))
            }
            "switch" => {
                let mut traits = vec!["switch.relay@1".to_string()];
                if c.get("apower").is_some() {
                    traits.push("energy.power@1".into());
                }
                if c.get("aenergy").is_some() {
                    traits.push("energy.meter@1".into());
                }
                Some(("switch.relay", traits, label("relais")))
            }
            "pm1" => Some(("energy.power", vec!["energy.power@1".into(), "energy.meter@1".into()], label("mesure"))),
            _ => None,
        };
        if let Some((kind, traits, name)) = device {
            out.push(DeviceInfo { reference: key.clone(), kind: kind.into(), traits, name });
        }
    }
    out
}

/// State of each device, from a `Shelly.GetStatus` answer.
fn states_of(status: &Map<String, Value>) -> HashMap<String, State> {
    let mut out = HashMap::new();
    for (key, c) in status {
        let Some((kind, n)) = component(key) else { continue };
        let mut s = State::new();
        match kind {
            "em" => {
                put(&mut s, "power", num(&c["total_act_power"]));
                put(&mut s, "power_a", num(&c["a_act_power"]));
                put(&mut s, "power_b", num(&c["b_act_power"]));
                put(&mut s, "power_c", num(&c["c_act_power"]));
                put(&mut s, "voltage", num(&c["a_voltage"]));
                put(&mut s, "current", num(&c["total_current"]));
                if let Some(d) = status.get(&format!("emdata:{n}")) {
                    put(&mut s, "energy_in", kwh(&d["total_act"]));
                    put(&mut s, "energy_out", kwh(&d["total_act_ret"]));
                }
            }
            "em1" => {
                put(&mut s, "power", num(&c["act_power"]));
                put(&mut s, "voltage", num(&c["voltage"]));
                put(&mut s, "current", num(&c["current"]));
                if let Some(d) = status.get(&format!("em1data:{n}")) {
                    put(&mut s, "energy_in", kwh(&d["total_act_energy"]));
                    put(&mut s, "energy_out", kwh(&d["total_act_ret_energy"]));
                }
            }
            "switch" | "pm1" => {
                if kind == "switch" {
                    if let Some(on) = c["output"].as_bool() {
                        s.insert("on".into(), json!(on));
                    }
                }
                put(&mut s, "power", num(&c["apower"]));
                put(&mut s, "voltage", num(&c["voltage"]));
                put(&mut s, "current", num(&c["current"]));
                put(&mut s, "energy_in", kwh(&c["aenergy"]["total"]));
                put(&mut s, "energy_out", kwh(&c["ret_aenergy"]["total"]));
            }
            _ => continue,
        }
        out.insert(key.clone(), s);
    }
    out
}

impl Driver for Shelly {
    fn describe(&self) -> DriverFuture<'_, Described> {
        Box::pin(async move {
            let info = self.rpc("Shelly.GetDeviceInfo", &[]).await?;
            let found = identify(&info).ok_or_else(|| DriverError::Invalid("not a Shelly".into()))?;
            if self.device_id.as_ref().is_some_and(|expected| *expected != found.id) {
                return Err(DriverError::Invalid(format!("another device answers at this address ({}): scan the network again", found.id)));
            }
            let status = self.rpc("Shelly.GetStatus", &[]).await?;
            let status = status.as_object().ok_or_else(|| DriverError::Invalid("Shelly.GetStatus: not an object".into()))?;
            let model = match &found.note {
                Some(n) => format!("{} ({n})", found.model),
                None => found.model.clone(),
            };
            Ok(Described { model, firmware: found.firmware.clone(), devices: devices_of(&found.model, status) })
        })
    }

    fn read(&self) -> DriverFuture<'_, HashMap<String, State>> {
        Box::pin(async move {
            let status = self.rpc("Shelly.GetStatus", &[]).await?;
            let status = status.as_object().ok_or_else(|| DriverError::Invalid("Shelly.GetStatus: not an object".into()))?;
            Ok(states_of(status))
        })
    }

    fn act<'a>(&'a self, reference: &'a str, trait_name: &'a str, action: &'a str, params: &'a State) -> DriverFuture<'a, ()> {
        Box::pin(async move {
            match (component(reference), trait_name, action) {
                (Some(("switch", n)), "switch.relay", "set") => {
                    let on = params.get("on").and_then(Value::as_bool).ok_or_else(|| DriverError::Invalid("`on` is required".into()))?;
                    self.rpc("Switch.Set", &[("id", n.to_string()), ("on", on.to_string())]).await?;
                    Ok(())
                }
                _ => Err(DriverError::Unsupported(format!("{reference}: {trait_name}.{action}"))),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pro3em() -> Map<String, Value> {
        json!({
            "em:0": {"id": 0, "a_act_power": 1120.4, "b_act_power": 640.0, "c_act_power": 650.1, "total_act_power": 2410.5,
                     "a_voltage": 232.1, "total_current": 10.6},
            "emdata:0": {"id": 0, "total_act": 1234567.0, "total_act_ret": 45678.9},
            "sys": {"uptime": 10}
        })
        .as_object()
        .unwrap()
        .clone()
    }

    #[test]
    fn a_pro_3em_is_a_three_phase_meter() {
        let devices = devices_of("Shelly Pro 3EM", &pro3em());
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].reference, "em:0");
        assert_eq!(devices[0].traits, vec!["energy.power@1", "energy.meter@1"]);
        let s = &states_of(&pro3em())["em:0"];
        assert_eq!(s["power"], 2410.5);
        assert_eq!(s["power_b"], 640.0);
        assert_eq!(s["energy_in"], 1234.567);
        assert_eq!(s["energy_out"], 45.679);
    }

    #[test]
    fn a_plug_is_a_relay_that_measures() {
        let status = json!({"switch:0": {"id": 0, "output": true, "apower": 64.2, "voltage": 230.0, "aenergy": {"total": 900.0}}});
        let status = status.as_object().unwrap();
        let devices = devices_of("Shelly Plus Plug S", status);
        assert_eq!(devices[0].kind, "switch.relay");
        assert_eq!(devices[0].traits, vec!["switch.relay@1", "energy.power@1", "energy.meter@1"]);
        assert_eq!(devices[0].name, "Plus Plug S relais");
        let s = &states_of(status)["switch:0"];
        assert_eq!(s["on"], true);
        assert_eq!(s["energy_in"], 0.9);
    }

    #[test]
    fn a_pro_3em_in_single_phase_profile_has_three_channels() {
        // Answers of a real Pro 3EM with the "monophase" profile.
        let info = json!({"name": null, "id": "shellypro3em-8c4f009097c4", "mac": "8C4F009097C4", "model": "SPEM-003CEBEU", "gen": 2,
                          "ver": "1.7.1-pro3emmprod0", "app": "Pro3EMM", "auth_en": false, "profile": "monophase"});
        let found = identify(&info).unwrap();
        assert_eq!(found.model, "Shelly Pro 3EM");
        assert_eq!(found.code.as_deref(), Some("SPEM-003CEBEU"));
        assert!(found.supported && found.note.as_deref().unwrap().contains("monophasé"));
        let status = json!({
            "em1:0": {"id": 0, "voltage": 230.1, "current": 1.94, "act_power": -140.1},
            "em1:1": {"id": 1, "voltage": 230.1, "current": 0.029, "act_power": 0.0},
            "em1:2": {"id": 2, "voltage": 230.1, "current": 1.624, "act_power": -369.2},
            "em1data:0": {"id": 0, "total_act_energy": 1492914.29, "total_act_ret_energy": 91066.74},
            "em1data:1": {"id": 1, "total_act_energy": 380840.47, "total_act_ret_energy": 0.08},
            "em1data:2": {"id": 2, "total_act_energy": 32.15, "total_act_ret_energy": 412330.01}
        });
        let status = status.as_object().unwrap();
        let devices = devices_of(&found.model, status);
        assert_eq!(devices.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(), vec!["Pro 3EM voie 1", "Pro 3EM voie 2", "Pro 3EM voie 3"]);
        assert!(devices.iter().all(|d| d.traits == vec!["energy.power@1", "energy.meter@1"]));
        let s = &states_of(status)["em1:2"];
        assert_eq!(s["power"], -369.2, "negative: the channel returns energy (solar)");
        assert_eq!(s["energy_out"], 412.33);
    }

    #[test]
    fn a_first_generation_shelly_is_found_but_not_supported() {
        let found = identify(&json!({"type": "SHEM-3", "mac": "A4CF12F3AB01", "auth": false, "fw": "20230913-114150/v1.14.0"})).unwrap();
        assert_eq!(found.generation, 1);
        assert!(!found.supported);
        assert!(identify(&json!({"hello": "world"})).is_none(), "not a Shelly");
    }

    #[test]
    fn addresses_are_checked() {
        assert!(Shelly::new(&json!({"host": "192.168.1.40"})).is_ok());
        assert!(Shelly::new(&json!({"host": "shelly-pro3em.local:8080"})).is_ok());
        assert!(Shelly::new(&json!({"host": "evil.example/x?y"})).is_err());
        assert!(Shelly::new(&json!({})).is_err());
    }
}
