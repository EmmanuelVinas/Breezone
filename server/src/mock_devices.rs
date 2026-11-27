//! Fake modules for development, demos and screenshots: one of each kind of device Breezone
//! drives, answering as the real hardware does, with values that follow the time of day.
//!
//! * Shelly Pro EM (`/rpc`): the grid and the solar production, a water heater and a plug relay;
//! * Philips Hue bridge (API v1): three lights, two rooms and their scenes; its button is always
//!   « pressed »;
//! * V2C Trydan: a charger, charging in the afternoon;
//! * Renson Healthbox 3.0: the ventilation and three rooms with their measures;
//! * Tasmota: a lamp with a dimmer, a plug that measures, a temperature and humidity sensor;
//! * a SONOFF ZBBridge Pro under Tasmota: a Zigbee sensor, a plug, a lamp, a 2-channel relay, a door sensor Breezone
//!   does not drive, and a second sensor that joins once the pairing is opened;
//! * ESPHome (native API, in clear): two shutters, a garden valve and an air sensor (CO₂…);
//! * Forecast.Solar: the forecast of the day.
//!
//! [`spawn`] starts them on consecutive ports; [`Module::source`] is what to add in Breezone.

use crate::esphome_wire::{decode, Fields, Frames, Msg};
use axum::extract::{Path, Query, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use chrono::{Local, Timelike};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::f64::consts::PI;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A fake module started: its driver, the name to give it, and its address.
#[derive(Debug, Clone)]
pub struct Module {
    pub driver: &'static str,
    pub name: &'static str,
    pub host: String,
    /// Extra configuration (Forecast.Solar's panels and address).
    pub config: Value,
}

impl Module {
    /// The body of `POST /api/sources` that adds it (a Hue bridge goes through `/api/hue/link`).
    pub fn source(&self) -> Value {
        let mut config = self.config.clone();
        if !self.host.is_empty() {
            config["host"] = json!(self.host);
        }
        json!({"driver": self.driver, "name": self.name, "config": config})
    }
}

/// The hour of the day, local, with its fraction.
fn hour() -> f64 {
    let t = Local::now();
    t.hour() as f64 + t.minute() as f64 / 60.0 + t.second() as f64 / 3600.0
}

/// Solar production (W) at an hour: a bell from 7:30 to 18:30, a few clouds.
fn solar(h: f64) -> f64 {
    if !(7.5..=18.5).contains(&h) {
        return 0.0;
    }
    let bell = ((h - 7.5) / 11.0 * PI).sin();
    let clouds = 1.0 - 0.15 * (h * 2.7).sin().max(0.0);
    (4200.0 * bell * bell * clouds).round()
}

fn heater_on(h: f64) -> bool {
    (12.0..15.0).contains(&h) || (2.0..4.0).contains(&h)
}

fn charging(h: f64) -> bool {
    (9.5..15.5).contains(&h)
}

/// What the house draws (W) apart from the heater and the car.
fn base(h: f64) -> f64 {
    let morning = 900.0 * (-((h - 7.5) * (h - 7.5)) / 1.5).exp();
    let evening = 1500.0 * (-((h - 20.0) * (h - 20.0)) / 3.0).exp();
    (320.0 + morning + evening + 60.0 * (h * 5.0).sin()).round()
}

fn home(h: f64) -> f64 {
    base(h) + if heater_on(h) { 1800.0 } else { 0.0 } + if charging(h) { 3700.0 } else { 0.0 }
}

async fn listen(port: u16) -> std::io::Result<tokio::net::TcpListener> {
    tokio::net::TcpListener::bind(("0.0.0.0", port)).await
}

async fn serve(port: u16, router: Router) -> std::io::Result<()> {
    let listener = listen(port).await?;
    tokio::spawn(async move { axum::serve(listener, router).await });
    Ok(())
}

/// Starts every fake module, from `first_port` on.
pub async fn spawn(first_port: u16) -> std::io::Result<Vec<Module>> {
    let host = |n: u16| format!("127.0.0.1:{}", first_port + n);
    serve(first_port, shelly()).await?;
    serve(first_port + 1, hue()).await?;
    serve(first_port + 2, trydan()).await?;
    serve(first_port + 3, healthbox()).await?;
    serve(first_port + 4, tasmota()).await?;
    let esphome = listen(first_port + 5).await?;
    tokio::spawn(esphome_node(esphome));
    serve(first_port + 6, forecast()).await?;
    serve(first_port + 7, zigbee_bridge()).await?;
    Ok(vec![
        Module { driver: "shelly", name: "Tableau électrique", host: host(0), config: json!({}) },
        Module { driver: "hue", name: "Pont Hue", host: host(1), config: json!({}) },
        Module { driver: "trydan", name: "Borne de recharge", host: host(2), config: json!({}) },
        Module { driver: "renson", name: "VMC", host: host(3), config: json!({}) },
        Module { driver: "tasmota", name: "Salon", host: host(4), config: json!({}) },
        Module { driver: "esphome", name: "Volets et jardin", host: host(5), config: json!({}) },
        Module { driver: "tasmota", name: "Pont Zigbee", host: host(7), config: json!({}) },
        Module {
            driver: "forecast_solar",
            name: "Prévision solaire",
            host: String::new(),
            config: json!({"kwp": 4.5, "declination": 30, "azimuth": 0, "latitude": 45.9, "longitude": 6.13, "url": format!("http://{}", host(6))}),
        },
    ])
}

// ---------------------------------------------------------------------------------------------
// Shelly Pro EM: em1:0 the grid, em1:1 the solar; switch:0 the water heater, switch:1 a plug
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Default)]
struct Relays(Arc<Mutex<HashMap<u32, bool>>>);

fn shelly() -> Router {
    let relays = Relays::default();
    relays.0.lock().unwrap().insert(1, true);
    Router::new()
        .route("/rpc/Shelly.GetDeviceInfo", get(|| async { Json(json!({"id": "shellyproem50-demo01", "app": "ProEM", "ver": "1.4.4", "gen": 2, "mac": "A0DD6C000001"})) }))
        .route(
            "/rpc/Shelly.GetStatus",
            get(|State(r): State<Relays>| async move {
                let h = hour();
                let (sun, house) = (solar(h), home(h));
                let heater = heater_on(h);
                let plug = *r.0.lock().unwrap().get(&1).unwrap_or(&false);
                let day = h * 1000.0;
                Json(json!({
                    "em1:0": {"id": 0, "voltage": 231.2, "current": ((house - sun).abs() / 231.2 * 100.0).round() / 100.0, "act_power": house - sun},
                    "em1:1": {"id": 1, "voltage": 231.2, "current": (sun / 231.2 * 100.0).round() / 100.0, "act_power": -sun},
                    "em1data:0": {"id": 0, "total_act_energy": 8_341_200.0 + day, "total_act_ret_energy": 1_922_600.0},
                    "em1data:1": {"id": 1, "total_act_energy": 0.0, "total_act_ret_energy": 5_120_400.0 + day * 0.6},
                    "switch:0": {"id": 0, "output": heater, "apower": if heater { 1800.0 } else { 0.0 }, "voltage": 231.2, "aenergy": {"total": 921_000.0}},
                    "switch:1": {"id": 1, "output": plug, "apower": if plug { 64.0 } else { 0.0 }, "voltage": 231.2, "aenergy": {"total": 41_200.0}},
                    "sys": {"uptime": 86_400}
                }))
            }),
        )
        .route(
            "/rpc/Switch.Set",
            get(|State(r): State<Relays>, Query(q): Query<HashMap<String, String>>| async move {
                let id = q.get("id").and_then(|i| i.parse().ok()).unwrap_or(0);
                r.0.lock().unwrap().insert(id, q.get("on").map(String::as_str) == Some("true"));
                Json(json!({"was_on": false}))
            }),
        )
        .route("/rpc/Switch.SetConfig", get(|| async { Json(json!({"restart_required": false})) }).post(|| async { Json(json!({"restart_required": false})) }))
        .route(
            "/rpc/Shelly.GetConfig",
            get(|| async {
                Json(json!({
                    "em1:0": {"id": 0, "name": "Réseau"}, "em1:1": {"id": 1, "name": "Panneaux solaires"},
                    "switch:0": {"id": 0, "name": "Chauffe-eau"}, "switch:1": {"id": 1, "name": "Prise du bureau"}
                }))
            }),
        )
        .with_state(relays)
}

// ---------------------------------------------------------------------------------------------
// Philips Hue
// ---------------------------------------------------------------------------------------------

fn hue() -> Router {
    let lights = Arc::new(Mutex::new(json!({
        "1": {"name": "Suspension du salon", "type": "Extended color light", "state": {"on": true, "bri": 180, "ct": 370, "xy": [0.4573, 0.41], "colormode": "ct", "hue": 8418, "sat": 140, "reachable": true},
              "capabilities": {"control": {"ct": {"min": 153, "max": 500}}}},
        "2": {"name": "Ruban LED", "type": "Extended color light", "state": {"on": true, "bri": 120, "xy": [0.18, 0.12], "colormode": "xy", "hue": 46000, "sat": 220, "reachable": true},
              "capabilities": {"control": {"ct": {"min": 153, "max": 500}}}},
        "3": {"name": "Lampe de chevet", "type": "Color temperature light", "state": {"on": false, "bri": 254, "ct": 300, "colormode": "ct", "reachable": true},
              "capabilities": {"control": {"ct": {"min": 153, "max": 454}}}}
    })));
    let config = json!({"name": "Hue Bridge", "bridgeid": "001788FFFE000001", "modelid": "BSB002", "swversion": "1978688000", "apiversion": "1.78.0"});
    let c1 = config.clone();
    Router::new()
        .route("/api/config", get(move || async move { Json(c1) }))
        .route("/api", post(|| async { Json(json!([{"success": {"username": "breezone-demo-key"}}])) }))
        .route("/api/{key}/config", get(move || async move { Json(config) }))
        .route("/api/{key}/lights", get(|State(l): State<Arc<Mutex<Value>>>| async move { Json(l.lock().unwrap().clone()) }))
        .route(
            "/api/{key}/groups",
            get(|| async {
                Json(json!({
                    "1": {"name": "Salon", "type": "Room", "lights": ["1", "2"]},
                    "2": {"name": "Chambre", "type": "Room", "lights": ["3"]}
                }))
            }),
        )
        .route(
            "/api/{key}/scenes",
            get(|| async {
                Json(json!({
                    "s-1": {"name": "Lecture", "type": "GroupScene", "group": "1", "lights": ["1", "2"], "recycle": false},
                    "s-2": {"name": "Cinéma", "type": "GroupScene", "group": "1", "lights": ["1", "2"], "recycle": false},
                    "s-3": {"name": "Veilleuse", "type": "GroupScene", "group": "2", "lights": ["3"], "recycle": false}
                }))
            }),
        )
        .route(
            "/api/{key}/scenes/{id}",
            get(|Path((_, id)): Path<(String, String)>| async move {
                Json(match id.as_str() {
                    "s-3" => json!({"name": "Veilleuse", "type": "GroupScene", "group": "2"}),
                    "s-2" => json!({"name": "Cinéma", "type": "GroupScene", "group": "1"}),
                    _ => json!({"name": "Lecture", "type": "GroupScene", "group": "1"}),
                })
            }),
        )
        .route("/api/{key}/groups/{group}/action", put(|Json(cmd): Json<Value>| async move { Json(json!([{"success": cmd}])) }))
        .route(
            "/api/{key}/lights/{id}/state",
            put(|State(l): State<Arc<Mutex<Value>>>, Path((_, id)): Path<(String, String)>, Json(cmd): Json<Value>| async move {
                let mut lights = l.lock().unwrap();
                for (k, v) in cmd.as_object().into_iter().flatten() {
                    lights[&id]["state"][k] = v.clone();
                    if k == "xy" || k == "ct" {
                        lights[&id]["state"]["colormode"] = json!(k);
                    }
                }
                Json(json!([{"success": cmd}]))
            }),
        )
        .with_state(lights)
}

// ---------------------------------------------------------------------------------------------
// V2C Trydan
// ---------------------------------------------------------------------------------------------

/// The charger alone (also served by [`spawn`]).
pub fn trydan() -> Router {
    let data = Arc::new(Mutex::new(json!({
        "ID": "DEMO01", "FirmwareVersion": "2.5.1", "ChargeState": 1, "ChargeMode": 0, "ChargePower": 0.0, "VoltageInstallation": 231,
        "ChargeEnergy": 0.0, "ChargeTime": 0, "Paused": 0, "Locked": 0, "Intensity": 16, "IntensityMeasure_L1": 0.0, "Dynamic": 1,
        "MinIntensity": 6, "MaxIntensity": 32
    })));
    Router::new()
        .route(
            "/RealTimeData",
            get(|State(d): State<Arc<Mutex<Value>>>| async move {
                let h = hour();
                let mut d = d.lock().unwrap();
                let on = charging(h) && d["Paused"] == 0;
                d["ChargeState"] = json!(if on { 2 } else { 1 });
                d["ChargePower"] = json!(if on { 3700.0 } else { 0.0 });
                d["IntensityMeasure_L1"] = json!(if on { 16.0 } else { 0.0 });
                let since = (h - 9.5).clamp(0.0, 6.0);
                d["ChargeEnergy"] = json!((since * 3.7 * 10.0).round() / 10.0);
                d["ChargeTime"] = json!((since * 3600.0) as i64);
                Json(d.clone())
            }),
        )
        .route(
            "/write/{kv}",
            get(|State(d): State<Arc<Mutex<Value>>>, Path(kv): Path<String>| async move {
                if let Some((k, v)) = kv.split_once('=') {
                    d.lock().unwrap()[k] = json!(v.parse::<i64>().unwrap_or(0));
                }
                "OK"
            }),
        )
        .with_state(data)
}

// ---------------------------------------------------------------------------------------------
// Renson Healthbox 3.0
// ---------------------------------------------------------------------------------------------

fn healthbox() -> Router {
    let boosts: Arc<Mutex<HashMap<String, Value>>> = Arc::default();
    let room = |name: &str, profile: &str, nominal: f64, flow: f64, humidity: f64, co2: Option<f64>| {
        let mut sensors = vec![
            json!({"type": "indoor relative humidity", "parameter": {"humidity": {"value": humidity}}}),
            json!({"type": "indoor temperature", "parameter": {"temperature": {"value": 20.8}}}),
        ];
        if let Some(c) = co2 {
            sensors.push(json!({"type": "indoor CO2", "parameter": {"concentration": {"value": c}}}));
        }
        json!({"name": name, "profile_name": profile, "parameter": {"nominal": {"value": nominal}, "offset": {"value": 0.0}},
               "actuator": [{"parameter": {"flow_rate": {"value": flow}}}], "sensor": sensors})
    };
    let current = move || {
        let h = hour();
        let shower = 55.0 + 25.0 * (-((h - 7.3) * (h - 7.3)) / 0.3).exp();
        json!({
            "device_type": "HEALTHBOX3", "description": "Healthbox 3.0", "serial": "240205P0001",
            "room": {
                "1": room("Salle de bains", "health", 40.0, if shower > 65.0 { 40.0 } else { 18.0 }, (shower * 10.0).round() / 10.0, None),
                "2": room("Cuisine", "health", 75.0, 32.0, 52.0, Some(640.0 + 120.0 * (h * 0.8).sin())),
                "3": room("Toilettes", "eco", 30.0, 10.0, 49.0, None)
            },
            "sensor": [{"type": "global air quality index", "parameter": {"index": {"value": if shower > 65.0 { 92.0 } else { 34.0 }},
                        "main_pollutant": {"value": "indoor relative humidity"}, "room": {"value": "Salle de bains"}}}]
        })
    };
    Router::new()
        .route("/v2/api/data/current", get(move || async move { Json(current()) }))
        .route("/v2/device", get(|| async { Json(json!({"fan": {"flow": 60.0}, "power": 8.0})) }))
        .route("/v1/global", get(|| async { Json(json!({"firmware version": "2.6.9"})) }))
        .route("/v2/api/api_key/status", get(|| async { Json(json!({"state": "valid"})) }))
        .route("/v2/api/api_key", post(|| async { "OK" }))
        .route(
            "/v2/api/boost/{room}",
            get(|State(b): State<Arc<Mutex<HashMap<String, Value>>>>, Path(room): Path<String>| async move {
                let on = b.lock().unwrap().get(&room).is_some_and(|v| v["enable"] == true);
                Json(json!({"default_level": 100.0, "default_timeout": 900, "enable": on, "remaining": if on { 1800 } else { 0 }}))
            })
            .put(|State(b): State<Arc<Mutex<HashMap<String, Value>>>>, Path(room): Path<String>, Json(body): Json<Value>| async move {
                b.lock().unwrap().insert(room, body);
                "OK"
            }),
        )
        .route("/v2/api/data/current/room/{room}/profile_name", put(|| async { "OK" }))
        .with_state(boosts)
}

// ---------------------------------------------------------------------------------------------
// Tasmota: a lamp with a dimmer (POWER1), a measuring plug (POWER2), a BME280
// ---------------------------------------------------------------------------------------------

fn tasmota() -> Router {
    let state = Arc::new(Mutex::new(json!({"POWER1": "ON", "POWER2": "ON", "Dimmer": 60, "CT": 333, "Color": "000000FFFF"})));
    Router::new()
        .route(
            "/cm",
            get(|State(s): State<Arc<Mutex<Value>>>, Query(q): Query<HashMap<String, String>>| async move {
                let cmnd = q.get("cmnd").cloned().unwrap_or_default();
                let mut s = s.lock().unwrap();
                let (name, arg) = cmnd.split_once(' ').unwrap_or((cmnd.as_str(), ""));
                match name {
                    "Power1" | "Power2" => s[name.to_uppercase()] = json!(if arg.eq_ignore_ascii_case("on") { "ON" } else { "OFF" }),
                    "Dimmer" => s["Dimmer"] = json!(arg.parse::<i64>().unwrap_or(50)),
                    "CT" => s["CT"] = json!(arg.parse::<i64>().unwrap_or(300)),
                    "Color" => s["Color"] = json!(arg),
                    _ => {}
                }
                let h = hour();
                let tv = s["POWER2"] == "ON";
                Json(json!({
                    "Status": {"DeviceName": "Salon", "FriendlyName": ["Lampadaire", "Télévision"]},
                    "StatusSTS": s.clone(),
                    "StatusSNS": {
                        "ENERGY": {"Power": if tv { (85.0 + 10.0 * (h * 3.0).sin()).round() } else { 0.0 }, "Voltage": 231, "Current": if tv { 0.38 } else { 0.0 }, "Total": 214.6},
                        "BME280": {"Temperature": ((21.2 + 0.8 * ((h - 15.0) / 24.0 * 2.0 * PI).cos()) * 10.0).round() / 10.0, "Humidity": 47.0, "Pressure": 1016.4},
                        "TempUnit": "C"
                    },
                    "StatusFWR": {"Version": "14.2.0(tasmota)", "Hardware": "ESP32-D0WD-V3"},
                    "StatusNET": {"Mac": "A0:DD:6C:00:00:04"}
                }))
            }),
        )
        .with_state(state)
}

// ---------------------------------------------------------------------------------------------
// SONOFF ZBBridge Pro under Tasmota (Zigbee2Tasmota)
// ---------------------------------------------------------------------------------------------

/// The devices of the network, by short address; the bathroom sensor joins when the pairing opens.
fn zigbee_network() -> Vec<Value> {
    vec![
        json!({"Device": "0x6769", "Name": "Chambre", "IEEEAddr": "0x00124B0026B64D1E", "ModelId": "SNZB-02D", "Manufacturer": "SONOFF", "BatteryPercentage": 87, "Reachable": true}),
        json!({"Device": "0x1D8A", "Name": "Prise de la cuisine", "IEEEAddr": "0x7CB03EBB0A0292DD", "ModelId": "S26R2ZB", "Manufacturer": "SONOFF", "Power": 1, "Reachable": true}),
        json!({"Device": "0x3A10", "Name": "Plafonnier", "IEEEAddr": "0x0017880102AABBCC", "ModelId": "LWB010", "Manufacturer": "Philips", "Power": 0, "Dimmer": 180, "CT": 370, "Reachable": true}),
        json!({"Device": "0x5A01", "Name": "Tableau", "IEEEAddr": "0x001FEE00000052A1", "ModelId": "S2-R", "Manufacturer": "ubisys", "Endpoints": [1, 2, 3], "Power": 1, "Power2": 0, "Reachable": true}),
        json!({"Device": "0x2E01", "Name": "Porte d'entrée", "IEEEAddr": "0x00124B0022AA0001", "ModelId": "SNZB-04", "Manufacturer": "SONOFF", "Contact": 1, "BatteryPercentage": 64, "Reachable": true}),
    ]
}

pub fn zigbee_bridge() -> Router {
    let network = Arc::new(Mutex::new(zigbee_network()));
    Router::new()
        .route(
            "/cm",
            get(|State(n): State<Arc<Mutex<Vec<Value>>>>, Query(q): Query<HashMap<String, String>>| async move {
                let cmnd = q.get("cmnd").cloned().unwrap_or_default();
                let (name, arg) = cmnd.split_once(' ').unwrap_or((cmnd.as_str(), ""));
                let mut n = n.lock().unwrap();
                let h = hour();
                let now = Local::now().timestamp();
                for d in n.iter_mut().filter(|d| d["ModelId"] == "SNZB-02D") {
                    let warm = if d["Name"] == "Chambre" { 19.4 } else { 22.1 };
                    d["Temperature"] = json!(((warm + 0.6 * ((h - 16.0) / 24.0 * 2.0 * PI).cos()) * 10.0).round() / 10.0);
                    d["Humidity"] = json!(((55.0 - 4.0 * ((h - 16.0) / 24.0 * 2.0 * PI).cos()) * 10.0).round() / 10.0);
                    d["LastSeenEpoch"] = json!(now - 90);
                }
                let find = |n: &mut Vec<Value>, device: &str| n.iter_mut().position(|d| d["Device"] == device || d["IEEEAddr"] == device || d["Name"] == device);
                let answer = match name {
                    "ZbStatus1" => json!({"ZbStatus1": n.iter().map(|d| json!({"Device": d["Device"], "Name": d["Name"]})).collect::<Vec<_>>()}),
                    "ZbInfo" => {
                        let all: serde_json::Map<String, Value> =
                            n.iter().filter(|d| arg.is_empty() || d["Device"] == arg).map(|d| (d["Device"].as_str().unwrap_or_default().to_string(), d.clone())).collect();
                        json!({"ZbInfo": all})
                    }
                    "ZbPermitJoin" => {
                        if arg == "1" && !n.iter().any(|d| d["Device"] == "0x4B22") {
                            n.push(json!({"Device": "0x4B22", "IEEEAddr": "0x00124B0026B70011", "ModelId": "SNZB-02D", "Manufacturer": "SONOFF", "BatteryPercentage": 100, "Reachable": true}));
                        }
                        json!({"ZbPermitJoin": "Done"})
                    }
                    "ZbName" => {
                        let (device, new) = arg.split_once(',').unwrap_or((arg, ""));
                        if let Some(i) = find(&mut n, device) {
                            n[i]["Name"] = json!(new);
                        }
                        json!({"ZbName": "Done"})
                    }
                    "ZbSend" => {
                        let v: Value = serde_json::from_str(arg).unwrap_or_default();
                        // The attributes of a second endpoint are suffixed: Power2.
                        let suffix = v["Endpoint"].as_u64().filter(|e| *e > 1).map(|e| e.to_string()).unwrap_or_default();
                        if let Some(i) = find(&mut n, v["Device"].as_str().unwrap_or_default()) {
                            for (k, x) in v["Send"].as_object().into_iter().flatten() {
                                n[i][format!("{k}{suffix}")] = x.clone();
                            }
                        }
                        json!({"ZbSend": "Done"})
                    }
                    _ => json!({
                        "Status": {"DeviceName": "Pont Zigbee", "FriendlyName": ["Tasmota"]},
                        "StatusSTS": {},
                        "StatusSNS": {"TempUnit": "C"},
                        "StatusFWR": {"Version": "16.0.0(release-zbbrdgpro)", "Hardware": "ESP32-D0WD-V3"},
                        "StatusNET": {"Mac": "A0:DD:6C:00:00:08"}
                    }),
                };
                Json(answer)
            }),
        )
        .with_state(network)
}

// ---------------------------------------------------------------------------------------------
// ESPHome node: two covers, a valve (switch), an air sensor
// ---------------------------------------------------------------------------------------------

async fn esphome_node(listener: tokio::net::TcpListener) {
    let covers = Arc::new(Mutex::new(HashMap::from([(1u32, 1.0f32), (2u32, 0.35f32), (8, 0.0), (9, 0.6)])));
    let valve = Arc::new(Mutex::new(false));
    loop {
        let Ok((stream, _)) = listener.accept().await else { continue };
        let (covers, valve) = (covers.clone(), valve.clone());
        tokio::spawn(async move {
            let mut frames = Frames::plain(stream);
            let mut tick = tokio::time::interval(Duration::from_secs(10));
            let mut subscribed = false;
            loop {
                tokio::select! {
                    received = frames.recv() => {
                        let Ok((kind, msg)) = received else { return };
                        let f = Fields(decode(&msg));
                        let sent = match kind {
                            1 => frames.send(2, &Msg::default().varint(1, 1).varint(2, 10).string(4, "volets-jardin").0).await,
                            7 => frames.send(8, &[]).await,
                            5 => { let _ = frames.send(6, &[]).await; return }
                            9 => frames.send(10, &Msg::default().string(2, "volets-jardin").string(3, "A0:DD:6C:00:00:05").string(4, "2025.11.0").string(6, "esp32dev").string(13, "Volets et jardin").0).await,
                            11 => {
                                let list = [
                                    (13u16, Msg::default().string(1, "volet_salon").fixed32(2, 1).string(3, "Volet du salon").bool(6, true).bool(12, true)),
                                    (13, Msg::default().string(1, "volet_chambre").fixed32(2, 2).string(3, "Volet de la chambre").bool(6, true).bool(12, true)),
                                    (13, Msg::default().string(1, "volet_bureau").fixed32(2, 8).string(3, "Volet du bureau").bool(6, true).bool(12, true)),
                                    (13, Msg::default().string(1, "volet_cuisine").fixed32(2, 9).string(3, "Volet de la cuisine").bool(6, true).bool(12, true)),
                                    (17, Msg::default().string(1, "arrosage").fixed32(2, 3).string(3, "Arrosage du jardin")),
                                    (16, Msg::default().string(1, "co2").fixed32(2, 4).string(3, "CO₂").string(6, "ppm").string(9, "carbon_dioxide")),
                                    (16, Msg::default().string(1, "temperature").fixed32(2, 5).string(3, "Température").string(6, "°C").string(9, "temperature")),
                                    (16, Msg::default().string(1, "humidity").fixed32(2, 6).string(3, "Humidité").string(6, "%").string(9, "humidity")),
                                    (16, Msg::default().string(1, "illuminance").fixed32(2, 7).string(3, "Luminosité").string(6, "lx").string(9, "illuminance")),
                                ];
                                let mut r = Ok(());
                                for (k, m) in list {
                                    r = r.and(frames.send(k, &m.0).await);
                                }
                                r.and(frames.send(19, &[]).await)
                            }
                            20 => { subscribed = true; push(&mut frames, &covers, &valve).await }
                            30 => {
                                let key = f.fixed32(1);
                                if f.bool(4) {
                                    covers.lock().unwrap().insert(key, f.float(5).unwrap_or(0.0));
                                }
                                push(&mut frames, &covers, &valve).await
                            }
                            33 => { *valve.lock().unwrap() = f.bool(2); push(&mut frames, &covers, &valve).await }
                            _ => Ok(()),
                        };
                        if sent.is_err() { return }
                    }
                    _ = tick.tick() => {
                        if subscribed && push(&mut frames, &covers, &valve).await.is_err() { return }
                    }
                }
            }
        });
    }
}

/// The states of the node's entities.
async fn push(frames: &mut Frames<tokio::net::TcpStream>, covers: &Mutex<HashMap<u32, f32>>, valve: &Mutex<bool>) -> std::io::Result<()> {
    let h = hour();
    let covers: Vec<(u32, f32)> = covers.lock().unwrap().iter().map(|(k, v)| (*k, *v)).collect();
    for (key, position) in covers {
        frames.send(22, &Msg::default().fixed32(1, key).float(3, position).varint(5, 0).0).await?;
    }
    let on = *valve.lock().unwrap();
    frames.send(26, &Msg::default().fixed32(1, 3).bool(2, on).0).await?;
    let sensors = [(4u32, 620.0 + 180.0 * (h * 0.9).sin()), (5, 20.6 + 0.5 * (h / 3.0).sin()), (6, 51.0 + 4.0 * (h / 2.0).cos()), (7, (solar(h) / 8.0).max(3.0))];
    for (key, v) in sensors {
        frames.send(25, &Msg::default().fixed32(1, key).float(2, ((v * 10.0).round() / 10.0) as f32).0).await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Forecast.Solar
// ---------------------------------------------------------------------------------------------

fn forecast() -> Router {
    Router::new().route(
        "/estimate/{*rest}",
        get(|| async {
            let today = Local::now().date_naive();
            let tomorrow = today.succ_opt().unwrap_or(today);
            let (mut watts, mut wh) = (serde_json::Map::new(), serde_json::Map::new());
            let mut total = 0.0;
            for quarter in 0..96 {
                let h = quarter as f64 / 4.0;
                let w = (solar(h) * 1.05).round();
                total += w / 4.0;
                let at = format!("{today} {:02}:{:02}:00", quarter / 4, (quarter % 4) * 15);
                watts.insert(at.clone(), json!(w));
                wh.insert(at, json!(total.round()));
            }
            Json(json!({
                "result": {"watts": watts, "watt_hours": wh, "watt_hours_day": {today.to_string(): total.round(), tomorrow.to_string(): (total * 0.8).round()}},
                "message": {"type": "success", "code": 0, "text": "", "info": {"timezone": "Europe/Paris"}}
            }))
        }),
    )
}
