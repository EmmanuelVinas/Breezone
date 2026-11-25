//! Shelly, second generation and later (Plus, Pro, Gen3, Gen4): local RPC API over HTTP
//! (`/rpc/<Method>`), no account and no cloud.
//!
//! Components turned into devices:
//! * `em:N` (Pro 3EM, three phases) → `energy.power@1`, with `energy.meter@1` from `emdata:N`;
//! * `em1:N` (Pro EM, one phase each) → `energy.power@1`, with `energy.meter@1` from `em1data:N`;
//! * `switch:N` (Plus 1PM, Plug S, Pro…) → `switch.relay@1`, with power and energy when measured;
//! * `pm1:N` (Plus PM Mini) → `energy.power@1`, `energy.meter@1`.
//!
//! Besides the readings, the module keeps the Shelly's WebSocket (`/rpc`): each `NotifyStatus`
//! makes Breezone read it at once (a relay switched by hand shows without waiting).
//!
//! History: a channel `em1:N` keeps a record a minute for weeks (`EM1Data.GetData`), which fills
//! what Breezone could not record (`history`).

use breezone_module_sdk::{device, host, json, module, unsupported, Described, DeviceInfo, Driver, Error, Found, History, HistoryPoint, Map, Result, State, Value};
use std::cell::Cell;

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

/// What a `/shelly` (or `Shelly.GetDeviceInfo`) answer says.
struct Identified {
    id: String,
    /// The name shown ("Shelly Pro 3EM").
    model: String,
    /// Profile or particularity ("monophasé : 3 voies indépendantes").
    note: Option<String>,
    firmware: Option<String>,
    /// Second generation or later, without a password.
    supported: bool,
}

fn identify(info: &Value) -> Option<Identified> {
    let field = |k: &str| info.get(k).and_then(Value::as_str).map(str::to_string);
    let app = field("app")?;
    let generation = info["gen"].as_u64().unwrap_or(2);
    let known = MODELS.iter().find(|(a, ..)| *a == app);
    let auth = info["auth_en"].as_bool().unwrap_or(false);
    let model = known.map(|(_, name, _)| name.to_string()).unwrap_or_else(|| format!("Shelly {app}"));
    let note = known.and_then(|(_, _, n)| n.map(str::to_string)).or_else(|| auth.then(|| "protégé par mot de passe".into()));
    Some(Identified { id: field("id")?, model, note, firmware: field("ver"), supported: generation >= 2 && !auth })
}

/// Host or `host:port` on the local network: letters, digits, dots, dashes, colons.
fn valid_host(host: &str) -> bool {
    !host.is_empty() && host.len() <= 253 && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'))
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
        let found: Option<(&str, Vec<&str>, String)> = match kind {
            "em" => {
                let mut traits = vec!["energy.power@1"];
                if status.contains_key(&format!("emdata:{n}")) {
                    traits.push("energy.meter@1");
                }
                Some(("energy.power", traits, label("compteur")))
            }
            "em1" => {
                let mut traits = vec!["energy.power@1"];
                if status.contains_key(&format!("em1data:{n}")) {
                    traits.push("energy.meter@1");
                }
                Some(("energy.power", traits, format!("{app} voie {}", n + 1)))
            }
            "switch" => {
                let mut traits = vec!["switch.relay@1"];
                if c.get("apower").is_some() {
                    traits.push("energy.power@1");
                }
                if c.get("aenergy").is_some() {
                    traits.push("energy.meter@1");
                }
                Some(("switch.relay", traits, label("relais")))
            }
            "pm1" => Some(("energy.power", vec!["energy.power@1", "energy.meter@1"], label("mesure"))),
            _ => None,
        };
        if let Some((kind, traits, name)) = found {
            out.push(device(key.clone(), kind, &traits, name));
        }
    }
    out
}

/// State of each device, from a `Shelly.GetStatus` answer.
fn states_of(status: &Map<String, Value>) -> Vec<(String, State)> {
    let mut out = vec![];
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
        out.push((key.clone(), s));
    }
    out
}

/// The minutes of an `EM1Data.GetData` answer before `until`: the energy drawn and given back
/// (Wh, turned into kWh) and the mean power, voltage and current of each.
fn history_of(page: &Value, until: i64) -> History {
    let keys: Vec<&str> = page["keys"].as_array().map(|k| k.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    let at = |name: &str| keys.iter().position(|k| *k == name);
    let (drawn, given, voltage, current) = (at("total_act_energy"), at("total_act_ret_energy"), at("avg_voltage"), at("avg_current"));
    let mut points = vec![];
    for block in page["data"].as_array().into_iter().flatten() {
        let (Some(start), Some(period)) = (block["ts"].as_i64(), block["period"].as_i64().filter(|p| *p > 0)) else { continue };
        for (i, row) in block["values"].as_array().into_iter().flatten().enumerate() {
            let ts = start + i as i64 * period;
            if ts >= until {
                break;
            }
            let v = |k: Option<usize>| k.and_then(|k| row.get(k)).and_then(Value::as_f64);
            let mut values = Map::new();
            if let (Some(d), Some(g)) = (v(drawn), v(given)) {
                values.insert("energy_in".into(), json!(d / 1000.0));
                values.insert("energy_out".into(), json!(g / 1000.0));
                values.insert("power".into(), json!(((d - g) * 3600.0 / period as f64 * 10.0).round() / 10.0));
            }
            if let Some(x) = v(voltage) {
                values.insert("voltage".into(), json!(x));
            }
            if let Some(x) = v(current) {
                values.insert("current".into(), json!(x));
            }
            if !values.is_empty() {
                points.push(HistoryPoint { ts, period, values });
            }
        }
    }
    let more = page["next_record_ts"].as_i64().filter(|n| *n < until && points.last().is_none_or(|p| *n > p.ts));
    History { points, more }
}

pub struct Shelly {
    host: String,
    /// Id of the device expected at this address (from the scanner): another one is refused.
    device_id: Option<String>,
    /// The WebSocket kept with the Shelly, once asked (`None`: not yet, or it refused).
    socket: Cell<Option<u32>>,
    socket_tried: Cell<bool>,
}

impl Shelly {
    fn rpc(&self, method: &str, query: &str) -> Result<Value> {
        let url = if query.is_empty() { format!("http://{}/rpc/{method}", self.host) } else { format!("http://{}/rpc/{method}?{query}", self.host) };
        let answer = host::request("GET", &url, &[], None, 5000)?;
        if !(200..300).contains(&answer.status) {
            return Err(Error::Invalid(format!("{method}: HTTP {}", answer.status)));
        }
        answer.json().map_err(|e| match e {
            Error::Invalid(m) => Error::Invalid(format!("{method}: {m}")),
            other => other,
        })
    }

    fn status(&self) -> Result<Map<String, Value>> {
        match self.rpc("Shelly.GetStatus", "")? {
            Value::Object(m) => Ok(m),
            _ => Err(Error::Invalid("Shelly.GetStatus: not an object".into())),
        }
    }

    /// Opens the WebSocket once, and asks the status on it: a Shelly sends its notifications to
    /// the clients that asked it something. Without it, the readings go on as before.
    fn listen(&self) {
        if self.socket_tried.replace(true) {
            return;
        }
        match host::subscribe(&format!("ws://{}/rpc", self.host)) {
            Ok(s) => {
                let hello = json!({"id": 1, "src": "breezone", "method": "Shelly.GetStatus"}).to_string();
                if host::send(s, hello.as_bytes()).is_ok() {
                    self.socket.set(Some(s));
                } else {
                    host::unsubscribe(s);
                }
            }
            Err(e) => host::log("info", &format!("no WebSocket ({e:?}): readings only")),
        }
    }
}

impl Driver for Shelly {
    fn open(config: Value, _secret: Option<Value>) -> Result<Self> {
        let host = config["host"].as_str().map(str::trim).unwrap_or_default().to_string();
        if !valid_host(&host) {
            return Err(Error::Invalid("invalid Shelly address".into()));
        }
        let device_id = config["device_id"].as_str().map(str::trim).filter(|d| !d.is_empty()).map(str::to_string);
        Ok(Shelly { host, device_id, socket: Cell::new(None), socket_tried: Cell::new(false) })
    }

    fn describe(&self) -> Result<Described> {
        let info = self.rpc("Shelly.GetDeviceInfo", "")?;
        let Identified { id, model, note, firmware, .. } = identify(&info).ok_or_else(|| Error::Invalid("not a Shelly".into()))?;
        if self.device_id.as_ref().is_some_and(|expected| *expected != id) {
            return Err(Error::Invalid(format!("another device answers at this address ({id}): scan the network again")));
        }
        let status = self.status()?;
        let shown = match &note {
            Some(n) => format!("{model} ({n})"),
            None => model.clone(),
        };
        Ok(Described { model: shown, firmware, devices: devices_of(&model, &status), identity: Some(id) })
    }

    fn history(config: Value, secret: Option<Value>, reference: &str, since: i64, until: i64) -> Result<History> {
        let Some(("em1", n)) = component(reference) else { return unsupported("history") };
        let shelly = Shelly::open(config, secret)?;
        let page = shelly.rpc("EM1Data.GetData", &format!("id={n}&ts={since}&end_ts={until}"))?;
        Ok(history_of(&page, until))
    }

    fn read(&self) -> Result<Vec<(String, State)>> {
        let states = states_of(&self.status()?);
        self.listen();
        Ok(states)
    }

    fn act(&self, reference: &str, trait_name: &str, action: &str, params: &State) -> Result<()> {
        match (component(reference), trait_name, action) {
            (Some(("switch", n)), "switch.relay", "set") => {
                let on = params.get("on").and_then(Value::as_bool).ok_or_else(|| Error::Invalid("`on` is required".into()))?;
                self.rpc("Switch.Set", &format!("id={n}&on={on}"))?;
                Ok(())
            }
            _ => Err(Error::Unsupported(format!("{reference}: {trait_name}.{action}"))),
        }
    }

    fn on_message(&self, _subscription: u32, data: &[u8]) -> Result<()> {
        // A change told by the Shelly: read it now rather than at the next reading.
        if breezone_module_sdk::serde_json::from_slice::<Value>(data).is_ok_and(|m| m["method"] == "NotifyStatus") {
            host::changed();
        }
        Ok(())
    }

    fn diagnostic(&self) -> Result<Value> {
        let mut info = self.rpc("Shelly.GetDeviceInfo", "")?;
        if let Some(o) = info.as_object_mut() {
            o.remove("mac");
        }
        Ok(json!({"info": info, "status": self.status()?}))
    }

    fn close(&self) {
        if let Some(s) = self.socket.take() {
            host::unsubscribe(s);
        }
    }

    fn identify(host_: &str, announced: Option<&Value>) -> Option<Found> {
        if announced.is_some() {
            return None;
        }
        let (_, info) = host::get_json(&format!("http://{host_}/shelly"), 2000).ok()?;
        let i = identify(&info)?;
        let mac = info["mac"].as_str().map(str::to_string);
        Some(Found { model: i.model, note: i.note, device_id: Some(i.id), firmware: i.firmware, mac, unsupported: !i.supported, ..Found::default() })
    }

    fn probe(host_: &str) -> Option<String> {
        let (_, info) = host::get_json(&format!("http://{host_}/shelly"), 2000).ok()?;
        identify(&info).filter(|i| i.supported).map(|i| i.model)
    }
}

module!(Shelly);

#[cfg(test)]
mod history_tests {
    use super::*;
    use breezone_module_sdk::testing;

    #[test]
    fn a_channel_gives_its_minutes_back() {
        testing::reset();
        let page = json!({
            "keys": ["total_act_energy", "total_act_ret_energy", "lag_react_energy", "lead_react_energy", "max_act_power", "min_act_power", "max_aprt_power", "min_aprt_power", "max_voltage", "min_voltage", "avg_voltage", "max_current", "min_current", "avg_current"],
            "data": [{"ts": 1000, "period": 60, "values": [
                [1.25, 0.0, 0.0, 0.0, 85.8, 68.8, 422.5, 404.1, 227.0, 222.5, 224.1, 1.86, 1.81, 1.83],
                [0.0, 5.0, 0.0, 0.0, 0.0, -300.0, 0.0, 0.0, 227.0, 222.5, 225.0, 1.3, 1.3, 1.3],
                [0.5, 0.0, 0.0, 0.0, 30.0, 30.0, 0.0, 0.0, 227.0, 222.5, 225.0, 0.2, 0.2, 0.2]
            ]}],
            "next_record_ts": 1180
        });
        testing::answer_json("GET", "/rpc/EM1Data.GetData?id=1&ts=1000&end_ts=1120", &page);
        let h = <Shelly as Driver>::history(json!({"host": "192.168.1.40"}), None, "em1:1", 1000, 1120).unwrap();
        // The third minute starts at `until`: left out, and nothing more to ask.
        assert_eq!(h.points.len(), 2);
        assert_eq!(h.more, None);
        assert_eq!((h.points[0].ts, h.points[0].period), (1000, 60));
        assert_eq!(h.points[0].values["power"], json!(75.0), "1.25 Wh in a minute");
        assert_eq!(h.points[0].values["energy_in"], json!(0.00125));
        assert_eq!(h.points[1].values["power"], json!(-300.0), "given back");
        assert_eq!(h.points[1].values["voltage"], json!(225.0));
        // Further on, the rest is asked.
        testing::answer_json("GET", "/rpc/EM1Data.GetData?id=1&ts=1000&end_ts=5000", &page);
        assert_eq!(<Shelly as Driver>::history(json!({"host": "192.168.1.40"}), None, "em1:1", 1000, 5000).unwrap().more, Some(1180));
        // Only the channels of one phase keep their minutes here.
        assert!(<Shelly as Driver>::history(json!({"host": "192.168.1.40"}), None, "switch:0", 1000, 5000).is_err());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use breezone_module_sdk::testing;

    const MANIFEST: &str = include_str!("../manifest.json");

    fn pro3em() -> Value {
        json!({
            "em:0": {"id": 0, "a_act_power": 1120.4, "b_act_power": 640.0, "c_act_power": 650.1, "total_act_power": 2410.5,
                     "a_voltage": 232.1, "total_current": 10.6},
            "emdata:0": {"id": 0, "total_act": 1234567.0, "total_act_ret": 45678.9},
            "sys": {"uptime": 10}
        })
    }

    fn info(app: &str) -> Value {
        json!({"id": "shellypro3em-a1b2", "app": app, "ver": "1.4.4", "gen": 2, "model": "SPEM-003CEBEU", "auth_en": false})
    }

    fn opened() -> Shelly {
        testing::open(json!({"host": "192.168.1.40"}), None)
    }

    #[test]
    fn a_pro_3em_is_a_three_phase_meter() {
        testing::reset();
        testing::answer_json("GET", "/rpc/Shelly.GetDeviceInfo", &info("Pro3EM"));
        testing::answer_json("GET", "/rpc/Shelly.GetStatus", &pro3em());
        let s = testing::check(&opened(), MANIFEST);
        let d = opened().describe().unwrap();
        assert_eq!((d.devices.len(), d.devices[0].reference.as_str()), (1, "em:0"));
        assert_eq!(d.devices[0].traits, vec!["energy.power@1", "energy.meter@1"]);
        assert_eq!(d.model, "Shelly Pro 3EM (triphasé)");
        let s = &s["em:0"];
        assert_eq!((s["power"].clone(), s["power_b"].clone()), (json!(2410.5), json!(640.0)));
        assert_eq!((s["energy_in"].clone(), s["energy_out"].clone()), (json!(1234.567), json!(45.679)));
    }

    #[test]
    fn a_plug_is_a_relay_that_measures_and_is_driven() {
        testing::reset();
        testing::answer_json("GET", "/rpc/Shelly.GetDeviceInfo", &info("PlugS"));
        testing::answer_json("GET", "/rpc/Shelly.GetStatus", &json!({"switch:0": {"id": 0, "output": true, "apower": 64.2, "voltage": 230.0, "aenergy": {"total": 900.0}}}));
        testing::answer_json("GET", "/rpc/Switch.Set", &json!({"was_on": true}));
        let plug = opened();
        let s = testing::check(&plug, MANIFEST);
        let d = plug.describe().unwrap();
        assert_eq!((d.devices[0].kind.as_str(), d.devices[0].name.as_str()), ("switch.relay", "Plus Plug S relais"));
        assert_eq!(d.devices[0].traits, vec!["switch.relay@1", "energy.power@1", "energy.meter@1"]);
        assert_eq!((s["switch:0"]["on"].clone(), s["switch:0"]["energy_in"].clone()), (json!(true), json!(0.9)));
        plug.act("switch:0", "switch.relay", "set", json!({"on": false}).as_object().unwrap()).unwrap();
        assert!(testing::requests().contains(&"GET http://192.168.1.40/rpc/Switch.Set?id=0&on=false".to_string()));
    }

    #[test]
    fn a_pro_3em_in_single_phase_profile_has_three_channels() {
        let found = identify(&json!({"name": null, "id": "shellypro3em-8c4f009097c4", "mac": "8C4F009097C4", "model": "SPEM-003CEBEU", "gen": 2,
                                     "ver": "1.7.1-pro3emmprod0", "app": "Pro3EMM", "auth_en": false, "profile": "monophase"}))
        .unwrap();
        assert_eq!(found.model, "Shelly Pro 3EM");
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
        let states: std::collections::HashMap<_, _> = states_of(status).into_iter().collect();
        assert_eq!(states["em1:2"]["power"], -369.2, "negative: the channel returns energy (solar)");
        assert_eq!(states["em1:2"]["energy_out"], 412.33);
    }

    #[test]
    fn another_device_at_the_address_and_a_first_generation_are_refused() {
        testing::reset();
        testing::answer_json("GET", "/rpc/Shelly.GetDeviceInfo", &info("Pro3EM"));
        let s: Shelly = testing::open(json!({"host": "192.168.1.40", "device_id": "shellyplus1-ffff"}), None);
        assert!(matches!(s.describe(), Err(Error::Invalid(m)) if m.contains("another device")));
        assert!(identify(&json!({"type": "SHEM-3", "mac": "A4CF12F3AB01"})).is_none(), "first generation: another API");
        for bad in [json!({"host": "evil.example/x?y"}), json!({})] {
            assert!(Shelly::open(bad, None).is_err());
        }
        assert!(Shelly::open(json!({"host": "shelly-pro3em.local:8080"}), None).is_ok());
    }

    #[test]
    fn its_websocket_makes_breezone_read_it_at_once() {
        testing::reset();
        testing::answer_json("GET", "/rpc/Shelly.GetStatus", &pro3em());
        let s = opened();
        s.read().unwrap();
        // Asked once on the WebSocket, so that the Shelly tells its changes.
        assert!(String::from_utf8_lossy(&testing::sent()[0].1).contains("Shelly.GetStatus"));
        s.read().unwrap();
        assert_eq!(testing::sent().len(), 1, "opened once");
        s.on_message(1, br#"{"src":"shellypro3em","method":"NotifyStatus","params":{}}"#).unwrap();
        s.on_message(1, br#"{"id":1,"result":{}}"#).unwrap();
        assert_eq!(testing::changed_count(), 1);
    }
}
