//! Philips Hue bridge, through its local API (version 1, HTTP on the local network): its lights
//! (`light@1`), and the rooms and scenes of the Hue app. The bridge holds the Zigbee network;
//! Breezone needs no Hue account.
//!
//! Breezone gets its access key the way the Hue app does: the bridge's button is pressed, then
//! `POST /api` gives a key (the source's secret, stored encrypted): the step `link` of `pair`.

use breezone_module_sdk::{device, host, json, module, Described, DeviceInfo, Driver, Error, Found, HardwareRoom, Map, Paired, PairedSource, Result, Scene, State, Value};

/// What answers `/api/config` without a key: a Hue bridge.
#[derive(Debug, Clone, PartialEq)]
pub struct Identified {
    /// "001788FFFE6DBD26".
    pub id: String,
    pub name: String,
    /// "BSB002".
    pub model: String,
    pub firmware: Option<String>,
}

pub fn identify(config: &Value) -> Option<Identified> {
    let id = config["bridgeid"].as_str().filter(|s| !s.is_empty())?.to_string();
    Some(Identified {
        id,
        name: config["name"].as_str().unwrap_or("Hue Bridge").to_string(),
        model: config["modelid"].as_str().unwrap_or("").to_string(),
        firmware: config["swversion"].as_str().map(str::to_string),
    })
}

/// An address as typed: a name or an IP, with a port maybe.
fn valid_host(host_: &str) -> bool {
    !host_.is_empty() && host_.len() <= 253 && host_.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'))
}

// ---------------------------------------------------------------------------------------
// Colors (formulas published by Philips for Hue: sRGB, wide gamut D65)
// ---------------------------------------------------------------------------------------

fn to_linear(v: f64) -> f64 {
    if v > 0.04045 {
        ((v + 0.055) / 1.055).powf(2.4)
    } else {
        v / 12.92
    }
}

fn from_linear(v: f64) -> f64 {
    let v = if v <= 0.0031308 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
    v.clamp(0.0, 1.0)
}

/// "#ff8800" → the CIE xy the bridge takes.
pub fn hex_to_xy(hex: &str) -> Option<[f64; 2]> {
    let h = hex.strip_prefix('#').unwrap_or(hex);
    if h.len() != 6 {
        return None;
    }
    let c = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok().map(|v| to_linear(v as f64 / 255.0));
    let (r, g, b) = (c(0)?, c(2)?, c(4)?);
    let x = r * 0.664511 + g * 0.154324 + b * 0.162028;
    let y = r * 0.283881 + g * 0.668433 + b * 0.047685;
    let z = r * 0.000088 + g * 0.072310 + b * 0.986039;
    let sum = x + y + z;
    if sum <= 0.0 {
        return Some([0.3127, 0.3290]);
    }
    Some([(x / sum * 10000.0).round() / 10000.0, (y / sum * 10000.0).round() / 10000.0])
}

/// The bridge's xy → "#rrggbb", at full brightness (the brightness is a field of its own).
pub fn xy_to_hex(x: f64, y: f64) -> String {
    if y <= 0.0 {
        return "#ffffff".into();
    }
    let (cx, cy, cz) = (x / y, 1.0, (1.0 - x - y) / y);
    let r = cx * 1.656492 - cy * 0.354851 - cz * 0.255038;
    let g = -cx * 0.707196 + cy * 1.655397 + cz * 0.036152;
    let b = cx * 0.051713 - cy * 0.121364 + cz * 1.011530;
    let max = r.max(g).max(b).max(1e-9);
    let (r, g, b) = if max > 1.0 { (r / max, g / max, b / max) } else { (r, g, b) };
    let c = |v: f64| (from_linear(v.max(0.0)) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", c(r), c(g), c(b))
}

/// The bridge's hue (0–65535) and saturation (0–254) → "#rrggbb".
fn hs_to_hex(hue: f64, sat: f64) -> String {
    let h = (hue / 65535.0 * 360.0) % 360.0;
    let s = (sat / 254.0).clamp(0.0, 1.0);
    let c = s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = 1.0 - c;
    let (r, g, b) = match h as u32 {
        0..=59 => (c, x, 0.0),
        60..=119 => (x, c, 0.0),
        120..=179 => (0.0, c, x),
        180..=239 => (0.0, x, c),
        240..=299 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let v = |v: f64| ((v + m) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", v(r), v(g), v(b))
}

fn kelvin(mired: f64) -> i64 {
    (1_000_000.0 / mired.max(1.0)).round() as i64
}

/// The state of a light (`/lights/<id>`) as `light@1` describes it.
pub fn state_of(light: &Value) -> State {
    let st = &light["state"];
    let mut s = Map::new();
    s.insert("on".into(), json!(st["on"].as_bool().unwrap_or(false)));
    if let Some(bri) = st["bri"].as_f64() {
        s.insert("brightness".into(), json!(((bri / 254.0) * 100.0).round().clamp(1.0, 100.0) as i64));
    }
    let ct_caps = &light["capabilities"]["control"]["ct"];
    if let Some(ct) = st["ct"].as_f64() {
        s.insert("color_temp".into(), json!(kelvin(ct)));
        // Mireds: the highest is the warmest.
        if let (Some(min), Some(max)) = (ct_caps["min"].as_f64(), ct_caps["max"].as_f64()) {
            s.insert("color_temp_min".into(), json!(kelvin(max)));
            s.insert("color_temp_max".into(), json!(kelvin(min)));
        }
    }
    let xy = || st["xy"].as_array().and_then(|xy| Some(xy_to_hex(xy.first()?.as_f64()?, xy.get(1)?.as_f64()?)));
    let color = match st["colormode"].as_str() {
        Some("xy") => xy(),
        Some("hs") => Some(hs_to_hex(st["hue"].as_f64().unwrap_or(0.0), st["sat"].as_f64().unwrap_or(0.0))),
        _ => None,
    };
    if st.get("xy").is_some() || st.get("hue").is_some() {
        if let Some(c) = color.clone().or_else(xy) {
            s.insert("color".into(), json!(c));
        }
        s.insert("mode".into(), json!(if color.is_some() { "color" } else { "white" }));
    } else if st.get("ct").is_some() {
        s.insert("mode".into(), json!("white"));
    }
    if let Some(r) = st["reachable"].as_bool() {
        s.insert("reachable".into(), json!(r));
    }
    s
}

/// The command for `set`: only what is given.
pub fn command(params: &State) -> Result<Value> {
    let mut c = Map::new();
    if let Some(on) = params.get("on").and_then(Value::as_bool) {
        c.insert("on".into(), json!(on));
    }
    if let Some(b) = params.get("brightness").and_then(Value::as_i64) {
        c.insert("bri".into(), json!(((b.clamp(1, 100) as f64) / 100.0 * 254.0).round().max(1.0) as i64));
        c.entry("on").or_insert(json!(true));
    }
    if let Some(k) = params.get("color_temp").and_then(Value::as_i64) {
        c.insert("ct".into(), json!((1_000_000 / k.clamp(1500, 9000)).clamp(153, 500)));
        c.entry("on").or_insert(json!(true));
    }
    if let Some(hex) = params.get("color").and_then(Value::as_str) {
        let xy = hex_to_xy(hex).ok_or_else(|| Error::Invalid("`color`: #rrggbb expected".into()))?;
        c.insert("xy".into(), json!(xy));
        c.entry("on").or_insert(json!(true));
    }
    if c.is_empty() {
        return Err(Error::Invalid("nothing to set".into()));
    }
    Ok(Value::Object(c))
}

fn references(lights: &Value) -> Vec<String> {
    lights.as_array().map(|l| l.iter().filter_map(Value::as_str).map(|id| format!("light:{id}")).collect()).unwrap_or_default()
}

/// The rooms of the Hue app (`/groups` of type "Room"), by name.
pub fn rooms_of(groups: &Value) -> Vec<HardwareRoom> {
    let mut rooms: Vec<(u64, HardwareRoom)> = groups
        .as_object()
        .map(|o| {
            o.iter()
                .filter(|(_, g)| g["type"] == "Room")
                .map(|(id, g)| (id.parse().unwrap_or(0), HardwareRoom { name: g["name"].as_str().unwrap_or("Pièce").to_string(), references: references(&g["lights"]) }))
                .collect()
        })
        .unwrap_or_default();
    rooms.sort_by_key(|(n, _)| *n);
    rooms.into_iter().map(|(_, r)| r).collect()
}

/// The scenes of the Hue app (`/scenes`), without the temporary ones apps leave behind.
pub fn scenes_of(scenes: &Value, groups: &Value) -> Vec<Scene> {
    let mut out: Vec<Scene> = scenes
        .as_object()
        .map(|o| {
            o.iter()
                .filter(|(_, s)| s["recycle"] != true)
                .map(|(id, s)| Scene {
                    id: id.clone(),
                    name: s["name"].as_str().unwrap_or("Scène").to_string(),
                    room: s["group"].as_str().and_then(|g| groups[g]["name"].as_str()).map(str::to_string),
                    references: references(&s["lights"]),
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| (&a.room, &a.name).cmp(&(&b.room, &b.name)));
    out
}

fn answer_json(a: &host::Answer) -> Result<Value> {
    serde_json_from(&a.body)
}

fn serde_json_from(body: &[u8]) -> Result<Value> {
    breezone_module_sdk::serde_json::from_slice(body).map_err(|e| Error::Invalid(e.to_string()))
}

pub struct Hue {
    base: String,
    device_id: Option<String>,
}

impl Hue {
    fn get(&self, path: &str) -> Result<Value> {
        let v = answer_json(&host::request("GET", &format!("{}{path}", self.base), &[], None, 5000)?)?;
        if let Some(e) = v.as_array().and_then(|a| a.first()).and_then(|f| f.get("error")) {
            return Err(Error::Invalid(match e["type"].as_i64() {
                Some(1) => "the bridge no longer knows Breezone's key: add it again".into(),
                _ => e["description"].as_str().unwrap_or("error").to_string(),
            }));
        }
        Ok(v)
    }

    fn put(&self, path: &str, body: &Value) -> Result<()> {
        let a = host::request("PUT", &format!("{}{path}", self.base), &[("Content-Type", "application/json")], Some(body.to_string().as_bytes()), 5000)?;
        let v = answer_json(&a)?;
        match v.as_array().and_then(|a| a.iter().find_map(|r| r.get("error"))) {
            Some(e) => Err(Error::Invalid(e["description"].as_str().unwrap_or("error").to_string())),
            None => Ok(()),
        }
    }
}

impl Driver for Hue {
    fn open(config: Value, secret: Option<Value>) -> Result<Self> {
        let host_ = config["host"].as_str().map(str::trim).unwrap_or_default();
        if !valid_host(host_) {
            return Err(Error::Invalid("invalid Hue bridge address".into()));
        }
        let key = secret
            .as_ref()
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|k| !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
            .ok_or_else(|| Error::Invalid("a Hue bridge is added by pressing its button".into()))?;
        let device_id = config["device_id"].as_str().map(str::trim).filter(|d| !d.is_empty()).map(str::to_string);
        Ok(Self { base: format!("http://{host_}/api/{key}"), device_id })
    }

    fn describe(&self) -> Result<Described> {
        let config = self.get("/config")?;
        let found = identify(&config).ok_or_else(|| Error::Invalid("not a Hue bridge".into()))?;
        if self.device_id.as_ref().is_some_and(|expected| !expected.eq_ignore_ascii_case(&found.id)) {
            return Err(Error::Invalid(format!("another bridge answers at this address ({}): scan the network again", found.id)));
        }
        let lights = self.get("/lights")?;
        let mut devices: Vec<(u64, DeviceInfo)> = lights
            .as_object()
            .map(|o| o.iter().map(|(id, l)| (id.parse().unwrap_or(0), device(format!("light:{id}"), "light", &["light@1"], l["name"].as_str().unwrap_or("Lumière")))).collect())
            .unwrap_or_default();
        devices.sort_by_key(|(n, _)| *n);
        let n = devices.len();
        Ok(Described {
            model: format!("Philips Hue {} ({} lumière{})", if found.model.is_empty() { "Bridge".into() } else { found.model.clone() }, n, if n > 1 { "s" } else { "" }),
            firmware: found.firmware,
            devices: devices.into_iter().map(|(_, d)| d).collect(),
            identity: Some(found.id),
        })
    }

    fn read(&self) -> Result<Vec<(String, State)>> {
        let lights = self.get("/lights")?;
        Ok(lights.as_object().map(|o| o.iter().map(|(id, l)| (format!("light:{id}"), state_of(l))).collect()).unwrap_or_default())
    }

    fn act(&self, reference: &str, trait_name: &str, action: &str, params: &State) -> Result<()> {
        let (Some(id), "light", "set") = (reference.strip_prefix("light:"), trait_name, action) else {
            return Err(Error::Unsupported(format!("{reference}: {trait_name}.{action}")));
        };
        self.put(&format!("/lights/{id}/state"), &command(params)?)
    }

    fn rooms(&self) -> Result<Vec<HardwareRoom>> {
        Ok(rooms_of(&self.get("/groups")?))
    }

    fn scenes(&self) -> Result<Vec<Scene>> {
        let groups = self.get("/groups")?;
        Ok(scenes_of(&self.get("/scenes")?, &groups))
    }

    fn activate_scene(&self, id: &str) -> Result<()> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(Error::Invalid("invalid scene".into()));
        }
        let scene = self.get(&format!("/scenes/{id}"))?;
        // A scene of a room is recalled on that room; any other on all the lights (group 0).
        let group = scene["group"].as_str().filter(|g| g.chars().all(|c| c.is_ascii_digit())).unwrap_or("0");
        self.put(&format!("/groups/{group}/action"), &json!({ "scene": id }))
    }

    fn identify(host_: &str, announced: Option<&Value>) -> Option<Found> {
        if announced.is_some() {
            return None;
        }
        let config = answer_json(&host::request("GET", &format!("http://{host_}/api/config"), &[], None, 2000).ok()?).ok()?;
        let b = identify(&config)?;
        let model = format!("Philips Hue {}", if b.model.is_empty() { "Bridge".to_string() } else { format!("Bridge ({})", b.model) });
        let note = match config["apiversion"].as_str() {
            Some(api) => format!("« {} » · API {api} · appuyez sur son bouton pour l'ajouter", b.name),
            None => format!("« {} » · appuyez sur son bouton pour l'ajouter", b.name),
        };
        Some(Found { model, note: Some(note), device_id: Some(b.id), firmware: b.firmware, ..Found::default() })
    }

    fn probe(host_: &str) -> Option<String> {
        let a = host::request("GET", &format!("http://{host_}/api/config"), &[], None, 2000).ok()?;
        identify(&answer_json(&a).ok()?).map(|b| b.name)
    }

    /// `link`: the bridge gives a key in the 30 seconds after its button is pressed.
    fn pair(step: &str, input: Value) -> Result<Paired> {
        if step != "link" {
            return Err(Error::Unsupported(format!("unknown step {step}")));
        }
        let host_ = input["host"].as_str().map(str::trim).filter(|h| valid_host(h)).ok_or_else(|| Error::Invalid("invalid address".into()))?;
        let config = answer_json(&host::request("GET", &format!("http://{host_}/api/config"), &[], None, 5000)?)?;
        let bridge = identify(&config).ok_or_else(|| Error::Invalid("not a Hue bridge".into()))?;
        let ask = json!({"devicetype": "breezone#server"}).to_string();
        let answer = answer_json(&host::request("POST", &format!("http://{host_}/api"), &[("Content-Type", "application/json")], Some(ask.as_bytes()), 5000)?)?;
        if let Some(key) = answer[0]["success"]["username"].as_str() {
            return Ok(Paired::Done(PairedSource { config: json!({"host": host_, "device_id": bridge.id}).to_string(), secret: key.to_string(), name: "Philips Hue".into() }));
        }
        match answer[0]["error"]["type"].as_i64() {
            Some(101) => Ok(Paired::Waiting(
                match input["lang"].as_str() {
                    Some("en") => "Press the round button of the Hue bridge, then click « Add » within 30 seconds.",
                    Some("es") => "Pulse el botón redondo del puente Hue y haga clic en «Añadir» en 30 segundos.",
                    _ => "Appuyez sur le bouton rond du pont Hue, puis cliquez sur « Ajouter » dans les 30 secondes.",
                }
                .into(),
            )),
            _ => Err(Error::Unreachable(answer[0]["error"]["description"].as_str().unwrap_or("unexpected answer").to_string())),
        }
    }
}

module!(Hue);

#[cfg(test)]
mod tests {
    use super::*;
    use breezone_module_sdk::testing;

    const MANIFEST: &str = include_str!("../manifest.json");

    #[test]
    fn colors_both_ways() {
        // Pure colors keep their dominant channel after a round trip through xy.
        for hex in ["#ff0000", "#00ff00", "#0000ff", "#ff8800"] {
            let [x, y] = hex_to_xy(hex).unwrap();
            let back = xy_to_hex(x, y);
            assert_eq!(&back[..3], &hex[..3], "{hex} → ({x}, {y}) → {back}");
        }
        assert_eq!(hex_to_xy("#fff0"), None);
        assert_eq!(hs_to_hex(0.0, 254.0), "#ff0000");
    }

    #[test]
    fn a_color_light_and_a_white_one() {
        let color = json!({"name": "Plafonnier", "state": {"on": true, "bri": 127, "ct": 366, "xy": [0.4573, 0.41], "colormode": "ct", "hue": 8418, "sat": 140, "reachable": true},
            "capabilities": {"control": {"ct": {"min": 153, "max": 500}, "colorgamut": [[0.6915, 0.3083], [0.17, 0.7], [0.1532, 0.0475]]}}});
        let s = state_of(&color);
        assert_eq!((s["on"].clone(), s["brightness"].clone()), (json!(true), json!(50)));
        assert_eq!((s["color_temp"].clone(), s["color_temp_min"].clone(), s["color_temp_max"].clone()), (json!(2732), json!(2000), json!(6536)));
        assert_eq!(s["mode"], "white", "in white mode, its color is the white's");
        assert!(s["color"].as_str().unwrap().starts_with('#'));
        let dimmable = json!({"name": "Lampe", "state": {"on": false, "bri": 254, "reachable": false}});
        let s = state_of(&dimmable);
        assert_eq!((s["brightness"].clone(), s["reachable"].clone()), (json!(100), json!(false)));
        assert!(!s.contains_key("color") && !s.contains_key("color_temp"));
    }

    #[test]
    fn commands_only_what_is_asked() {
        let p = |v: Value| v.as_object().unwrap().clone();
        assert_eq!(command(&p(json!({"on": false}))).unwrap(), json!({"on": false}));
        assert_eq!(command(&p(json!({"brightness": 50}))).unwrap(), json!({"bri": 127, "on": true}));
        assert_eq!(command(&p(json!({"color_temp": 2700}))).unwrap(), json!({"ct": 370, "on": true}));
        assert_eq!(command(&p(json!({"color": "#ff0000"}))).unwrap()["xy"].as_array().map(|a| a.len()), Some(2));
        assert!(command(&p(json!({}))).is_err());
        assert!(command(&p(json!({"color": "rouge"}))).is_err());
    }

    fn bridge() {
        testing::answer_json("GET", "/api/config", &json!({"bridgeid": "001788FFFE6DBD26", "name": "Hue Bridge", "modelid": "BSB002", "swversion": "1967054020"}));
        testing::answer_json("GET", "/api/key-1/config", &json!({"bridgeid": "001788FFFE6DBD26", "name": "Hue Bridge", "modelid": "BSB002", "swversion": "1967054020"}));
        testing::answer_json(
            "GET",
            "/api/key-1/lights",
            &json!({
                "1": {"name": "Plafonnier", "state": {"on": true, "bri": 254, "ct": 366, "xy": [0.4573, 0.41], "colormode": "ct", "reachable": true}, "capabilities": {"control": {"ct": {"min": 153, "max": 500}}}},
                "2": {"name": "Lampe", "state": {"on": false, "bri": 127, "reachable": true}}
            }),
        );
        testing::answer_json(
            "GET",
            "/api/key-1/groups",
            &json!({"1": {"name": "Salon", "type": "Room", "lights": ["1", "2"]}, "2": {"name": "Tout", "type": "LightGroup", "lights": ["1"]}}),
        );
        testing::answer_json("GET", "/api/key-1/scenes", &json!({"abc": {"name": "Lecture", "group": "1", "lights": ["1"]}, "tmp": {"name": "x", "recycle": true, "lights": []}}));
        testing::answer_json("GET", "/api/key-1/scenes/abc", &json!({"name": "Lecture", "group": "1", "lights": ["1"]}));
        testing::answer_json("PUT", "/api/key-1/", &json!([{"success": {}}]));
    }

    #[test]
    fn a_bridge_lists_drives_and_recalls() {
        testing::reset();
        bridge();
        // The key the native driver stored: a plain string, not JSON.
        let hue: Hue = testing::open(json!({"host": "192.168.1.20", "device_id": "001788fffe6dbd26"}), Some(json!("key-1")));
        let d = hue.describe().unwrap();
        assert_eq!((d.model.as_str(), d.identity.as_deref(), d.devices.len()), ("Philips Hue BSB002 (2 lumières)", Some("001788FFFE6DBD26"), 2));
        let state = testing::check(&hue, MANIFEST);
        assert_eq!((state["light:1"]["brightness"].clone(), state["light:2"]["on"].clone()), (json!(100), json!(false)));
        hue.act("light:2", "light", "set", &json!({"on": true}).as_object().unwrap().clone()).unwrap();
        assert_eq!(hue.rooms().unwrap()[0].references, ["light:1", "light:2"]);
        let scenes = hue.scenes().unwrap();
        assert_eq!((scenes.len(), scenes[0].room.as_deref()), (1, Some("Salon")));
        hue.activate_scene("abc").unwrap();
        let asked = testing::requests();
        assert!(asked.contains(&"PUT http://192.168.1.20/api/key-1/lights/2/state".to_string()), "{asked:?}");
        assert!(asked.contains(&"PUT http://192.168.1.20/api/key-1/groups/1/action".to_string()), "{asked:?}");
        // Another bridge at the address is told.
        let other: Hue = testing::open(json!({"host": "192.168.1.20", "device_id": "0017880000000000"}), Some(json!("key-1")));
        assert!(other.describe().is_err());
    }

    #[test]
    fn pairing_waits_for_the_button() {
        testing::reset();
        bridge();
        testing::answer_json("POST", "192.168.1.20/api", &json!([{"error": {"type": 101, "description": "link button not pressed"}}]));
        assert!(matches!(Hue::pair("link", json!({"host": "192.168.1.20"})), Ok(Paired::Waiting(m)) if m.contains("bouton")));
        assert!(matches!(Hue::pair("link", json!({"host": "192.168.1.20", "lang": "en"})), Ok(Paired::Waiting(m)) if m.contains("button")));
        testing::answer_json("POST", "192.168.1.20/api", &json!([{"success": {"username": "key-1"}}]));
        match Hue::pair("link", json!({"host": "192.168.1.20"})) {
            Ok(Paired::Done(s)) => {
                assert_eq!(s.secret, "key-1");
                assert_eq!(breezone_module_sdk::serde_json::from_str::<Value>(&s.config).unwrap(), json!({"host": "192.168.1.20", "device_id": "001788FFFE6DBD26"}));
            }
            other => panic!("{other:?}"),
        }
        assert!(Hue::pair("finish", json!({})).is_err());
    }
}
