//! Zigbee2Tasmota: a Tasmota that is itself the coordinator of a Zigbee network (the SONOFF
//! ZBBridge Pro flashed with `tasmota32-zbbrdgpro`), its devices read over the same HTTP API.
//!
//!   ZbStatus1 → {ZbStatus1: [{Device: "0x6769", Name}]}
//!   ZbInfo [<device>] → {ZbInfo: {"0x6769": {Device, Name, IEEEAddr, ModelId, Manufacturer,
//!       Temperature, Humidity, BatteryPercentage, Power, LinkQuality, LastSeenEpoch…}}}
//!   ZbPermitJoin 1 | 0 → pairing open for 60 s, or closed
//!   ZbSend {"Device": "<IEEE>", "Send": {"Power": 1}} (or Dimmer 0–254, CT in mireds), with
//!       "Endpoint": n for one relay of a device that has several (Power2… in its ZbInfo)
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

/// The relays of a device, by endpoint, each with the key of its state: `Power` (the first
/// endpoint, or the only one), `Power2`, `Power3`… for those after it (Tasmota suffixes the
/// attributes of the other endpoints of a device that has several).
fn relays(info: &Value) -> Vec<(u8, String)> {
    let first = info["Endpoints"][0].as_u64().and_then(|e| u8::try_from(e).ok()).unwrap_or(1);
    let mut out: Vec<(u8, String)> = info
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, v)| v.is_number() || v.is_boolean())
        .filter_map(|(k, _)| match k.strip_prefix("Power")? {
            "" => Some((first, k.clone())),
            n => n.parse::<u8>().ok().filter(|n| *n > 0).map(|n| (n, k.clone())),
        })
        .collect();
    out.sort();
    out.dedup_by_key(|(ep, _)| *ep);
    out
}

/// The key of another attribute of the same endpoint: `Dimmer` beside `Power`, `Dimmer2` beside `Power2`.
fn beside(key: &str, attribute: &str) -> String {
    format!("{attribute}{}", key.strip_prefix("Power").unwrap_or_default())
}

/// A lamp: it dims (its colours are not driven yet).
fn dims(info: &Value, key: &str) -> bool {
    info[beside(key, "Dimmer")].is_number()
}

fn power(info: &Value, key: &str) -> bool {
    info[key].as_bool().unwrap_or_else(|| info[key].as_f64().unwrap_or(0.0) > 0.0)
}

/// The colour temperatures of Tasmota's lights: 153–500 mireds.
const KELVIN_MIN: i64 = 2000;
const KELVIN_MAX: i64 = 6500;

fn is_sensor(info: &Value) -> bool {
    MEASURES.iter().any(|(from, _)| info[from].is_number())
}

/// A device of several relays (a 2- or 4-channel module, the ubisys S2-R): one Breezone device
/// each, `zigbee/<IEEE>/<endpoint>`; the one of a single relay stays `zigbee/<IEEE>`.
fn channels(info: &Value) -> Option<Vec<(u8, String)>> {
    Some(relays(info)).filter(|r| r.len() > 1)
}

/// The address and endpoint of a reference after `zigbee/`: `0x…` or `0x…/2`.
pub fn target(reference: &str) -> (&str, Option<u8>) {
    match reference.split_once('/') {
        Some((address, ep)) => (address, ep.parse().ok()),
        None => (reference, None),
    }
}

/// What Breezone makes of a Zigbee device: none for one it cannot translate (yet).
pub fn devices_of(info: &Value) -> Vec<DeviceInfo> {
    let Some(reference) = reference(info) else { return vec![] };
    let device =
        |reference: String, kind: &str, traits: &[&str], name: String| DeviceInfo { reference, kind: kind.into(), traits: traits.iter().map(|t| t.to_string()).collect(), name };
    let name = name(info);
    let sensor = is_sensor(info);
    if let Some(channels) = channels(info) {
        let mut out: Vec<DeviceInfo> = channels
            .iter()
            .enumerate()
            .map(|(i, (ep, key))| {
                let (kind, t) = if dims(info, key) { ("light", "light@1") } else { ("switch.relay", "switch.relay@1") };
                device(format!("{reference}/{ep}"), kind, &[t], format!("{name} {}", i + 1))
            })
            .collect();
        if sensor {
            out.push(device(reference, "sensor", &["environment.sensor@1"], name));
        }
        return out;
    }
    let relay = relays(info).into_iter().next();
    let light = relay.as_ref().is_some_and(|(_, k)| dims(info, k));
    let traits: Vec<&str> =
        [(light, "light@1"), (relay.is_some() && !light, "switch.relay@1"), (sensor, "environment.sensor@1")].iter().filter(|(yes, _)| *yes).map(|(_, t)| *t).collect();
    let kind = if light {
        "light"
    } else if relay.is_some() {
        "switch.relay"
    } else {
        "sensor"
    };
    if traits.is_empty() {
        return vec![];
    }
    vec![device(reference, kind, &traits, name)]
}

/// The on/off, brightness and colour temperature of the relay of `key`.
fn relay_state(info: &Value, key: &str, s: &mut Map<String, Value>) {
    s.insert("on".into(), json!(power(info, key)));
    if let Some(d) = info[beside(key, "Dimmer")].as_f64() {
        s.insert("brightness".into(), json!((d / 254.0 * 100.0).round().clamp(0.0, 100.0) as i64));
        if let Some(ct) = info[beside(key, "CT")].as_f64().filter(|c| *c > 0.0) {
            s.insert("color_temp".into(), json!(((1_000_000.0 / ct).round() as i64).clamp(KELVIN_MIN, KELVIN_MAX)));
            s.insert("color_temp_min".into(), json!(KELVIN_MIN));
            s.insert("color_temp_max".into(), json!(KELVIN_MAX));
        }
        if let Some(r) = info["Reachable"].as_bool() {
            s.insert("reachable".into(), json!(r));
        }
    }
}

fn sensor_state(info: &Value, now: i64, s: &mut Map<String, Value>) {
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

/// The state of each Breezone device of a Zigbee device, at `now` (unix seconds).
pub fn states_of(info: &Value, now: i64) -> Vec<(String, Map<String, Value>)> {
    let Some(reference) = reference(info) else { return vec![] };
    if devices_of(info).is_empty() {
        return vec![];
    }
    let sensor = is_sensor(info);
    if let Some(channels) = channels(info) {
        let mut out: Vec<(String, Map<String, Value>)> = channels
            .iter()
            .map(|(ep, key)| {
                let mut s = Map::new();
                relay_state(info, key, &mut s);
                (format!("{reference}/{ep}"), s)
            })
            .collect();
        if sensor {
            let mut s = Map::new();
            sensor_state(info, now, &mut s);
            out.push((reference, s));
        }
        return out;
    }
    let mut s = Map::new();
    if let Some((_, key)) = relays(info).first() {
        relay_state(info, key, &mut s);
    }
    if sensor {
        sensor_state(info, now, &mut s);
    }
    vec![(reference, s)]
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
        .filter(|i| devices_of(i).is_empty())
        .map(|i| {
            let fields: Vec<&String> = i.as_object().into_iter().flatten().map(|(k, _)| k).collect();
            json!({"name": name(i), "model": i["ModelId"], "manufacturer": i["Manufacturer"], "fields": fields})
        })
        .collect()
}
