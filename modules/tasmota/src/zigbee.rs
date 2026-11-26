//! Zigbee2Tasmota: a Tasmota that is itself the coordinator of a Zigbee network (the SONOFF
//! ZBBridge Pro flashed with `tasmota32-zbbrdgpro`), its devices read over the same HTTP API.
//!
//!   ZbStatus1 → {ZbStatus1: [{Device: "0x6769", Name}]}
//!   ZbInfo [<device>] → {ZbInfo: {"0x6769": {Device, Name, IEEEAddr, ModelId, Manufacturer,
//!       Temperature, Humidity, BatteryPercentage, Power, LinkQuality, LastSeenEpoch…}}}
//!   ZbPermitJoin 1 | 0 → pairing open for 60 s, or closed
//!   ZbSend {"Device": "<IEEE>", "Send": {"Power": 1}} (or Dimmer 0–254, CT in mireds)
//!   ZbName <IEEE>,<name>

use breezone_module_sdk::{json, DeviceInfo, Map, Value};

/// The reference of the bridge itself.
pub const BRIDGE: &str = "zigbee";

/// How long `ZbPermitJoin 1` keeps the network open (Tasmota's own duration).
pub const PAIRING_S: i64 = 60;

/// A measure older than this is no longer told (a sensor out of battery or out of range).
const STALE_S: i64 = 3 * 3600;

const MEASURES: [(&str, &str); 4] = [("Temperature", "temperature"), ("Humidity", "humidity"), ("Pressure", "pressure"), ("Illuminance", "illuminance")];

/// The address to give Tasmota: the IEEE one, which a new pairing does not change.
pub fn address(info: &Value) -> Option<&str> {
    info["IEEEAddr"].as_str().or(info["Device"].as_str())
}

pub fn reference(info: &Value) -> Option<String> {
    address(info).map(|a| format!("{BRIDGE}/{a}"))
}

fn name(info: &Value) -> String {
    if let Some(n) = info["Name"].as_str().filter(|n| !n.is_empty()) {
        return n.to_string();
    }
    match (info["Manufacturer"].as_str(), info["ModelId"].as_str()) {
        (Some(m), Some(id)) => format!("{m} {id}"),
        (None, Some(id)) => id.to_string(),
        _ => format!("Zigbee {}", info["Device"].as_str().unwrap_or("?")),
    }
}

fn is_relay(info: &Value) -> bool {
    info["Power"].is_number() || info["Power"].is_boolean()
}

/// A lamp: it dims (its colours are not driven yet).
fn is_light(info: &Value) -> bool {
    info["Dimmer"].is_number()
}

fn power(info: &Value) -> bool {
    info["Power"].as_bool().unwrap_or_else(|| info["Power"].as_f64().unwrap_or(0.0) > 0.0)
}

/// The colour temperatures of Tasmota's lights: 153–500 mireds.
const KELVIN_MIN: i64 = 2000;
const KELVIN_MAX: i64 = 6500;

fn is_sensor(info: &Value) -> bool {
    MEASURES.iter().any(|(from, _)| info[from].is_number())
}

/// What Breezone makes of a Zigbee device; `None` for one it cannot translate (yet).
pub fn device_of(info: &Value) -> Option<DeviceInfo> {
    let reference = reference(info)?;
    let light = is_light(info);
    let traits: Vec<String> = [(light, "light@1"), (is_relay(info) && !light, "switch.relay@1"), (is_sensor(info), "environment.sensor@1")]
        .iter()
        .filter(|(yes, _)| *yes)
        .map(|(_, t)| t.to_string())
        .collect();
    let kind = if light {
        "light"
    } else if is_relay(info) {
        "switch.relay"
    } else {
        "sensor"
    };
    (!traits.is_empty()).then(|| DeviceInfo { reference, kind: kind.into(), traits, name: name(info) })
}

/// The state of a Zigbee device, at `now` (unix seconds).
pub fn state_of(info: &Value, now: i64) -> Map<String, Value> {
    let mut s = Map::new();
    if is_relay(info) || is_light(info) {
        s.insert("on".into(), json!(power(info)));
    }
    if let Some(d) = info["Dimmer"].as_f64() {
        s.insert("brightness".into(), json!((d / 254.0 * 100.0).round().clamp(0.0, 100.0) as i64));
        if let Some(ct) = info["CT"].as_f64().filter(|c| *c > 0.0) {
            s.insert("color_temp".into(), json!(((1_000_000.0 / ct).round() as i64).clamp(KELVIN_MIN, KELVIN_MAX)));
            s.insert("color_temp_min".into(), json!(KELVIN_MIN));
            s.insert("color_temp_max".into(), json!(KELVIN_MAX));
        }
        if let Some(r) = info["Reachable"].as_bool() {
            s.insert("reachable".into(), json!(r));
        }
    }
    if is_sensor(info) {
        let fresh = info["LastSeenEpoch"].as_i64().is_none_or(|seen| now - seen < STALE_S);
        for (from, to) in MEASURES.iter().filter(|_| fresh) {
            if let Some(x) = info[from].as_f64() {
                s.insert(to.to_string(), json!(x));
            }
        }
        if let Some(b) = info["BatteryPercentage"].as_f64() {
            s.insert("battery".into(), json!(b.clamp(0.0, 100.0)));
        }
    }
    s
}

/// What to send a device for `set` of `switch.relay` or `light`, in order: one `ZbSend` each.
pub fn sends(trait_name: &str, params: &Map<String, Value>) -> Result<Vec<Value>, String> {
    if params.get("color").is_some_and(|c| !c.is_null()) {
        return Err("les couleurs d'une lampe Zigbee ne sont pas encore pilotées".into());
    }
    let mut out = vec![];
    if trait_name == "light" {
        if let Some(k) = params.get("color_temp").and_then(Value::as_f64) {
            out.push(json!({"CT": (1_000_000.0 / k.max(1.0)).round().clamp(153.0, 500.0) as i64}));
        }
        if let Some(b) = params.get("brightness").and_then(Value::as_f64) {
            out.push(json!({"Dimmer": (b.clamp(0.0, 100.0) * 254.0 / 100.0).round() as i64}));
        }
    }
    if let Some(on) = params.get("on").and_then(Value::as_bool) {
        out.push(json!({"Power": u8::from(on)}));
    }
    Ok(out)
}

/// The devices of `ZbStatus1`, each with what `ZbInfo` knows of it (`infos`, by short address).
pub fn join(list: &Value, infos: &Map<String, Value>) -> (Vec<Value>, Vec<String>) {
    let mut out = vec![];
    let mut missing = vec![];
    for e in list.as_array().into_iter().flatten() {
        let Some(short) = e["Device"].as_str() else { continue };
        match infos.get(short) {
            Some(i) => out.push(i.clone()),
            None => {
                missing.push(short.to_string());
                out.push(e.clone());
            }
        }
    }
    (out, missing)
}

/// The devices nothing of Breezone fits yet (a contact, a motion sensor…), for the diagnostic.
pub fn unsupported(devices: &[Value]) -> Vec<Value> {
    devices
        .iter()
        .filter(|i| device_of(i).is_none())
        .map(|i| {
            let fields: Vec<&String> = i.as_object().into_iter().flatten().map(|(k, _)| k).collect();
            json!({"name": name(i), "model": i["ModelId"], "manufacturer": i["Manufacturer"], "fields": fields})
        })
        .collect()
}
