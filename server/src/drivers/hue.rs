//! Philips Hue bridge, through its local API (version 1, HTTP on the local network): its lights
//! (`light@1`). The bridge holds the Zigbee network; Breezone needs no Hue account.
//!
//! Breezone gets its access key the way the Hue app does: the bridge's button is pressed, then
//! `POST /api` gives a key (the source's secret, stored encrypted).

use super::{ConfigField, Described, DeviceInfo, Driver, DriverError, DriverFuture, DriverInfo, HardwareRoom, Scene, State};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::time::Duration;

/// What answers `/api/config` without a key: a Hue bridge.
#[derive(Debug, Clone, PartialEq)]
pub struct Identified {
    /// "001788FFFE6DBD26".
    pub id: String,
    pub name: String,
    /// "BSB002".
    pub model: String,
    pub firmware: Option<String>,
    pub api: Option<String>,
}

pub fn identify(config: &Value) -> Option<Identified> {
    let id = config["bridgeid"].as_str().filter(|s| !s.is_empty())?.to_string();
    Some(Identified {
        id,
        name: config["name"].as_str().unwrap_or("Hue Bridge").to_string(),
        model: config["modelid"].as_str().unwrap_or("").to_string(),
        firmware: config["swversion"].as_str().map(str::to_string),
        api: config["apiversion"].as_str().map(str::to_string),
    })
}

fn client() -> reqwest::Client {
    reqwest::Client::builder().timeout(Duration::from_secs(5)).build().expect("http client")
}

/// A Hue bridge at `host`, if it is one (the network scanner).
pub async fn probe(host: &str) -> Option<Identified> {
    let http = reqwest::Client::builder().timeout(Duration::from_secs(2)).build().ok()?;
    let config: Value = http.get(format!("http://{host}/api/config")).send().await.ok()?.json().await.ok()?;
    identify(&config)
}

#[derive(Debug, Clone, PartialEq)]
pub enum LinkError {
    /// The bridge's button was not pressed in the last 30 seconds.
    ButtonNotPressed,
    Other(String),
}

/// Asks the bridge for an access key: it gives one in the 30 seconds after its button is pressed.
pub async fn link(host: &str) -> Result<(Identified, String), LinkError> {
    let http = client();
    let config: Value =
        http.get(format!("http://{host}/api/config")).send().await.map_err(|e| LinkError::Other(e.to_string()))?.json().await.map_err(|e| LinkError::Other(e.to_string()))?;
    let bridge = identify(&config).ok_or_else(|| LinkError::Other("not a Hue bridge".into()))?;
    let answer: Value = http
        .post(format!("http://{host}/api"))
        .json(&json!({"devicetype": "breezone#server"}))
        .send()
        .await
        .map_err(|e| LinkError::Other(e.to_string()))?
        .json()
        .await
        .map_err(|e| LinkError::Other(e.to_string()))?;
    if let Some(key) = answer[0]["success"]["username"].as_str() {
        return Ok((bridge, key.to_string()));
    }
    match answer[0]["error"]["type"].as_i64() {
        Some(101) => Err(LinkError::ButtonNotPressed),
        _ => Err(LinkError::Other(answer[0]["error"]["description"].as_str().unwrap_or("unexpected answer").to_string())),
    }
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
    let color = match st["colormode"].as_str() {
        Some("xy") => st["xy"].as_array().and_then(|xy| Some(xy_to_hex(xy.first()?.as_f64()?, xy.get(1)?.as_f64()?))),
        Some("hs") => Some(hs_to_hex(st["hue"].as_f64().unwrap_or(0.0), st["sat"].as_f64().unwrap_or(0.0))),
        _ => None,
    };
    if st.get("xy").is_some() || st.get("hue").is_some() {
        let current = color.clone().or_else(|| st["xy"].as_array().and_then(|xy| Some(xy_to_hex(xy.first()?.as_f64()?, xy.get(1)?.as_f64()?))));
        if let Some(c) = current {
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
pub fn command(params: &State) -> Result<Value, DriverError> {
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
        let xy = hex_to_xy(hex).ok_or_else(|| DriverError::Invalid("`color`: #rrggbb expected".into()))?;
        c.insert("xy".into(), json!(xy));
        c.entry("on").or_insert(json!(true));
    }
    if c.is_empty() {
        return Err(DriverError::Invalid("nothing to set".into()));
    }
    Ok(Value::Object(c))
}

pub fn info() -> DriverInfo {
    DriverInfo {
        id: "hue",
        label: "Philips Hue (pont)",
        provides: vec!["light@1"],
        models: vec!["Hue Bridge (BSB002)".into()],
        config: vec![ConfigField { name: "host", kind: "string", optional: false, label: crate::text!("Adresse IP ou nom", "IP address or name", "Dirección IP o nombre") }],
    }
}

pub struct Hue {
    http: reqwest::Client,
    base: String,
    key: String,
    device_id: Option<String>,
}

impl Hue {
    pub fn new(config: &Value, secret: Option<&str>) -> Result<Self, String> {
        let host = config["host"].as_str().map(str::trim).unwrap_or_default();
        if !super::shelly::valid_host(host) {
            return Err("invalid Hue bridge address".into());
        }
        let key =
            secret.map(str::trim).filter(|k| !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')).ok_or("a Hue bridge is added by pressing its button")?;
        let device_id = config["device_id"].as_str().map(str::trim).filter(|d| !d.is_empty()).map(str::to_string);
        Ok(Self { http: client(), base: format!("http://{host}/api/{key}"), key: key.to_string(), device_id })
    }

    async fn get(&self, path: &str) -> Result<Value, DriverError> {
        let resp =
            self.http.get(format!("{}{path}", self.base)).send().await.map_err(|e| DriverError::Unreachable(if e.is_timeout() { "timed out".into() } else { e.to_string() }))?;
        let v: Value = resp.json().await.map_err(|e| DriverError::Invalid(e.to_string()))?;
        if let Some(e) = v.as_array().and_then(|a| a.first()).and_then(|f| f.get("error")) {
            return Err(DriverError::Invalid(match e["type"].as_i64() {
                Some(1) => "the bridge no longer knows Breezone's key: add it again".into(),
                _ => e["description"].as_str().unwrap_or("error").to_string(),
            }));
        }
        Ok(v)
    }
}

impl Hue {
    async fn put(&self, path: &str, body: &Value) -> Result<(), DriverError> {
        let resp = self.http.put(format!("{}{path}", self.base)).json(body).send().await.map_err(|e| DriverError::Unreachable(e.to_string()))?;
        let v: Value = resp.json().await.map_err(|e| DriverError::Invalid(e.to_string()))?;
        match v.as_array().and_then(|a| a.iter().find_map(|r| r.get("error"))) {
            Some(e) => Err(DriverError::Invalid(e["description"].as_str().unwrap_or("error").to_string())),
            None => Ok(()),
        }
    }
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

impl Driver for Hue {
    fn describe(&self) -> DriverFuture<'_, Described> {
        Box::pin(async move {
            let config = self.get("/config").await?;
            let found = identify(&config).ok_or_else(|| DriverError::Invalid("not a Hue bridge".into()))?;
            if self.device_id.as_ref().is_some_and(|expected| !expected.eq_ignore_ascii_case(&found.id)) {
                return Err(DriverError::Invalid(format!("another bridge answers at this address ({}): scan the network again", found.id)));
            }
            let lights = self.get("/lights").await?;
            let mut devices: Vec<(u64, DeviceInfo)> = lights
                .as_object()
                .map(|o| {
                    o.iter()
                        .map(|(id, l)| {
                            (
                                id.parse().unwrap_or(0),
                                DeviceInfo {
                                    reference: format!("light:{id}"),
                                    kind: "light".into(),
                                    traits: vec!["light@1".into()],
                                    name: l["name"].as_str().unwrap_or("Lumière").to_string(),
                                },
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            devices.sort_by_key(|(n, _)| *n);
            let n = devices.len();
            Ok(Described {
                model: format!("Philips Hue {} ({} lumière{})", if found.model.is_empty() { "Bridge".into() } else { found.model.clone() }, n, if n > 1 { "s" } else { "" }),
                firmware: found.firmware,
                devices: devices.into_iter().map(|(_, d)| d).collect(),
                identity: Some(found.id),
            })
        })
    }

    fn read(&self) -> DriverFuture<'_, HashMap<String, State>> {
        Box::pin(async move {
            let lights = self.get("/lights").await?;
            Ok(lights.as_object().map(|o| o.iter().map(|(id, l)| (format!("light:{id}"), state_of(l))).collect()).unwrap_or_default())
        })
    }

    fn act<'a>(&'a self, reference: &'a str, trait_name: &'a str, action: &'a str, params: &'a State) -> DriverFuture<'a, ()> {
        Box::pin(async move {
            let (Some(id), "light", "set") = (reference.strip_prefix("light:"), trait_name, action) else {
                return Err(DriverError::Unsupported(format!("{reference}: {trait_name}.{action}")));
            };
            self.put(&format!("/lights/{id}/state"), &command(params)?).await
        })
    }

    fn rooms(&self) -> DriverFuture<'_, Vec<HardwareRoom>> {
        Box::pin(async move { Ok(rooms_of(&self.get("/groups").await?)) })
    }

    fn scenes(&self) -> DriverFuture<'_, Vec<Scene>> {
        Box::pin(async move {
            let groups = self.get("/groups").await?;
            Ok(scenes_of(&self.get("/scenes").await?, &groups))
        })
    }

    fn activate_scene<'a>(&'a self, id: &'a str) -> DriverFuture<'a, ()> {
        Box::pin(async move {
            if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                return Err(DriverError::Invalid("invalid scene".into()));
            }
            let scene = self.get(&format!("/scenes/{id}")).await?;
            // A scene of a room is recalled on that room; any other on all the lights (group 0).
            let group = scene["group"].as_str().filter(|g| g.chars().all(|c| c.is_ascii_digit())).unwrap_or("0");
            self.put(&format!("/groups/{group}/action"), &json!({ "scene": id })).await
        })
    }
}

impl std::fmt::Debug for Hue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the key.
        f.debug_struct("Hue").field("base", &self.base.replace(&self.key, "…")).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
