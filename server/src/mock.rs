//! A fake Airzone WebServer implementing the subset of the local API that Breezone uses.
//! Used by the `mock-airzone` binary (develop/demo without hardware) and by the tests.

use crate::airzone::ZoneData;
use axum::extract::State;
use axum::routing::{post, put};
use axum::{Json, Router};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub type MockState = Arc<Mutex<Vec<ZoneData>>>;

const AMBIENT: f64 = 14.0;

fn zone(id: i64, name: &str, on: bool, temp: f64, humidity: i64, setpoint: f64, master: bool) -> ZoneData {
    ZoneData {
        system_id: 1,
        zone_id: id,
        name: Some(name.into()),
        on: Some(on as i64),
        room_temp: Some(temp),
        humidity: Some(humidity),
        setpoint: Some(setpoint),
        max_temp: Some(30.0),
        min_temp: Some(15.0),
        temp_step: Some(0.5),
        units: Some(0),
        mode: Some(3),
        modes: master.then(|| vec![1, 2, 3, 4, 5]),
        master_zone_id: Some(1),
        speed: Some(0),
        speeds: Some(3),
        sleep: Some(0),
        air_demand: Some(0),
        floor_demand: Some(0),
        errors: vec![],
        ..Default::default()
    }
}

/// Zones of a preset. "maison" mirrors a real two-floor installation.
pub fn preset(name: &str) -> Vec<ZoneData> {
    match name {
        "bureau" => vec![
            zone(1, "Open space", true, 20.5, 41, 21.0, true),
            zone(2, "Salle de réunion", false, 18.0, 44, 20.0, false),
            zone(3, "Accueil", true, 19.0, 46, 19.5, false),
        ],
        _ => vec![
            zone(1, "Salon", true, 19.5, 39, 19.0, true),
            zone(2, "Bureau", true, 19.5, 47, 16.0, false),
            zone(3, "Chambre 1", true, 18.4, 45, 17.5, false),
            zone(4, "Chambre 2", false, 11.7, 60, 17.0, false),
            zone(5, "Dressing", false, 15.2, 56, 17.0, false),
        ],
    }
}

/// MAC address of the default fake WebServer.
pub const MOCK_MAC: &str = "A8:03:2A:00:FE:ED";

/// MAC address of a preset's WebServer: "bureau" is the « Atelier » installation of the fake
/// Airzone Cloud account, so that two fake sites never look like the same WebServer.
pub fn preset_mac(name: &str) -> &'static str {
    match name {
        "bureau" => "AA:BB:CC:DD:EE:FF",
        _ => MOCK_MAC,
    }
}

pub fn router(state: MockState) -> Router {
    router_with_mac(state, MOCK_MAC)
}

/// A fake WebServer with a given MAC address (to simulate several installations).
pub fn router_with_mac(state: MockState, mac: &str) -> Router {
    let mac = mac.to_string();
    Router::new()
        .route("/api/v1/hvac", post(read).put(write))
        .route("/api/v1/webserver", post(move || webserver(mac.clone())))
        .route("/api/v1/version", post(|| async { Json(json!({ "version": "1.76" })) }))
        .route("/api/v1/integration", put(|| async { Json(json!({})) }).post(|| async { Json(json!({})) }))
        .with_state(state)
}

fn int(body: &Value, key: &str) -> i64 {
    body.get(key).and_then(Value::as_i64).unwrap_or(0)
}

fn error(key: &str, msg: &str) -> Json<Value> {
    Json(json!({ "errors": [{ key: msg }] }))
}

async fn read(State(state): State<MockState>, Json(body): Json<Value>) -> Json<Value> {
    let (sys, zone) = (int(&body, "systemID"), int(&body, "zoneID"));
    let zones = state.lock().unwrap().clone();
    if sys == 0 {
        return Json(json!({ "systems": [{ "data": zones }] }));
    }
    if sys != 1 {
        return error("systemID", "systemid not avaiable");
    }
    if zone == 0 {
        return Json(json!({ "data": zones }));
    }
    match zones.into_iter().find(|z| z.zone_id == zone) {
        Some(z) => Json(json!({ "data": [z] })),
        None => error("zoneID", "zoneid not avaiable"),
    }
}

async fn write(State(state): State<MockState>, Json(body): Json<Value>) -> Json<Value> {
    let (sys, zone_id) = (int(&body, "systemID"), int(&body, "zoneID"));
    if sys != 1 {
        return error("systemID", "systemid not avaiable");
    }
    let mut zones = state.lock().unwrap();
    let Some(idx) = zones.iter().position(|z| z.zone_id == zone_id) else {
        return error("zoneID", "zoneid not avaiable");
    };
    if let Some(mode) = body.get("mode").and_then(Value::as_i64) {
        if !zones[idx].is_master() {
            return error("mode", "Method not provided or not supported");
        }
        for z in zones.iter_mut() {
            z.mode = Some(mode);
        }
    }
    let z = &mut zones[idx];
    if let Some(on) = body.get("on").and_then(Value::as_i64) {
        z.on = Some(on);
    }
    if let Some(sp) = body.get("setpoint").and_then(Value::as_f64) {
        if !(15.0..=30.0).contains(&sp) {
            return error("setpoint", "setpoint out of range");
        }
        z.setpoint = Some(sp);
    }
    if let Some(speed) = body.get("speed").and_then(Value::as_i64) {
        z.speed = Some(speed);
    }
    if let Some(sleep) = body.get("sleep").and_then(Value::as_i64) {
        z.sleep = Some(sleep);
    }
    if let Some(name) = body.get("name").and_then(Value::as_str) {
        z.name = Some(name.into());
    }
    Json(json!({ "data": [z.clone()] }))
}

async fn webserver(mac: String) -> Json<Value> {
    Json(json!({
        "mac": mac,
        "wifi_rssi": -52,
        "wifi_channel": 6,
        "interface": "wifi",
        "ws_type": "ws_az",
        "ws_firmware": "3.44"
    }))
}

/// Makes temperatures drift towards the setpoint (zone heating) or the ambient temperature.
pub fn spawn_simulation(state: MockState, period: Duration) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(period);
        loop {
            ticker.tick().await;
            let mut zones = state.lock().unwrap();
            for z in zones.iter_mut() {
                let temp = z.room_temp.unwrap_or(AMBIENT);
                let sp = z.setpoint.unwrap_or(20.0);
                let heating = z.is_on() && matches!(z.mode, Some(3) | Some(6) | Some(7));
                let cooling = z.is_on() && z.mode == Some(2);
                let target = if (heating && temp < sp) || (cooling && temp > sp) { sp } else { AMBIENT };
                let next = temp + (target - temp) * 0.08;
                z.room_temp = Some((next * 10.0).round() / 10.0);
                let demanding = (heating && temp < sp - 0.2) || (cooling && temp > sp + 0.2);
                z.air_demand = Some(demanding as i64);
            }
        }
    });
}

// ---------------------------------------------------------------------------------------
// Fake Airzone Cloud (only the read endpoints used by the synchronisation)
// ---------------------------------------------------------------------------------------

pub const MOCK_CLOUD_EMAIL: &str = "demo@breezone.local";
pub const MOCK_CLOUD_PASSWORD: &str = "demo-password";
const MOCK_CLOUD_TOKEN: &str = "mock-cloud-token";

fn cloud_zone(sys: i64, zone: i64, name: &str, ws: &str) -> Value {
    json!({ "device_id": format!("dev-{sys}-{zone}"), "type": "az_zone", "ws_id": ws, "name": name,
            "meta": { "system_number": sys, "zone_number": zone, "units": 0 } })
}

/// Account with two installations: "Maison" whose WebServer MAC is `local_mac`
/// (two groups, like the official app), and "Atelier" on another network.
pub fn cloud_account(local_mac: &str) -> Value {
    let ws = local_mac.replace(':', "");
    json!({
        "summaries": [
            { "installation_id": "inst-home", "name": "Maison", "ws_ids": [ws],
              "location_text": { "city": { "fr": "Annecy, Haute-Savoie", "en": "Annecy" } } },
            { "installation_id": "inst-office", "name": "Atelier", "ws_ids": ["AABBCCDDEEFF"],
              "location_text": { "city": { "fr": "Lyon, Rhône" } } }
        ],
        "details": {
            "inst-home": { "name": "Maison", "groups": [
                { "name": "Etage", "devices": [
                    { "device_id": "sys-1", "type": "az_system", "ws_id": ws, "meta": { "system_number": 1 } },
                    cloud_zone(1, 4, "Chambre 2", &ws), cloud_zone(1, 3, "Chambre 1", &ws), cloud_zone(1, 5, "Dressing", &ws) ] },
                { "name": "Rez de chaussée", "devices": [ cloud_zone(1, 1, "Salon", &ws), cloud_zone(1, 2, "Bureau", &ws) ] }
            ] },
            "inst-office": { "name": "Atelier", "groups": [
                { "name": "Atelier", "devices": [ cloud_zone(1, 1, "Open space", "AABBCCDDEEFF") ] }
            ] }
        },
        // Schedules as returned by GET installations/{id}/schedules (days: 0 = Monday).
        "schedules": {
            "inst-home": [
                { "_id": "sched-matin", "name": "Réveil", "type": "week", "prog_enabled": true,
                  "device_ids": ["dev-1-3", "dev-1-4"],
                  "start_conf": { "days": [0, 1, 2, 3, 4], "hour": 6, "minutes": 30, "power": true, "setpoint": { "celsius": 20.5, "fahrenheit": 69 } },
                  "end_conf": { "hour": 8, "minutes": 0, "power": false } },
                { "_id": "sched-nuit", "name": "Nuit", "type": "week", "prog_enabled": false,
                  "device_ids": ["sys-1"],
                  "start_conf": { "days": [6], "hour": 23, "minutes": 0, "mode": 1 } },
                { "_id": "sched-vacances", "name": "Vacances", "type": "calendar", "device_ids": ["sys-1"],
                  "start_conf": { "start_month": 7, "start_day": 1, "end_month": 7, "end_day": 31, "power": false } }
            ],
            "inst-office": [
                { "_id": "sched-bureau", "name": "Ouverture", "type": "week", "device_ids": ["dev-1-1"],
                  "start_conf": { "days": [0, 1, 2, 3, 4], "hour": 8, "minutes": 0, "power": true } }
            ]
        },
        "routines": {}
    })
}

fn rand_id() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
}

fn authorized(headers: &axum::http::HeaderMap) -> bool {
    headers.get("authorization").and_then(|v| v.to_str().ok()) == Some(&format!("Bearer {MOCK_CLOUD_TOKEN}"))
}

pub fn cloud_router(account: Value) -> Router {
    use axum::extract::Path;
    use axum::http::{HeaderMap, StatusCode};
    use axum::response::IntoResponse;
    use axum::routing::get;
    let account = Arc::new(account);
    let a1 = account.clone();
    let a2 = account.clone();
    // Schedules are editable, like on Airzone Cloud.
    let sched = Arc::new(Mutex::new(account["schedules"].clone()));
    let a4 = account.clone();
    Router::new()
        .route(
            "/api/v1/auth/login",
            post(|Json(body): Json<Value>| async move {
                if body["email"] == MOCK_CLOUD_EMAIL && body["password"] == MOCK_CLOUD_PASSWORD {
                    Json(json!({ "token": MOCK_CLOUD_TOKEN, "refreshToken": "mock-refresh" })).into_response()
                } else {
                    (StatusCode::UNAUTHORIZED, Json(json!({ "_id": "userNotExist" }))).into_response()
                }
            }),
        )
        .route(
            "/api/v1/installations",
            get(move |headers: HeaderMap| async move {
                if !authorized(&headers) {
                    return StatusCode::UNAUTHORIZED.into_response();
                }
                Json(json!({ "installations": a1["summaries"].clone() })).into_response()
            }),
        )
        .route(
            "/api/v1/installations/{id}",
            get(move |headers: HeaderMap, Path(id): Path<String>| async move {
                if !authorized(&headers) {
                    return StatusCode::UNAUTHORIZED.into_response();
                }
                match a2["details"].get(&id) {
                    Some(d) => Json(d.clone()).into_response(),
                    None => StatusCode::NOT_FOUND.into_response(),
                }
            }),
        )
        .route(
            "/api/v1/installations/{id}/schedules",
            get({
                let sched = sched.clone();
                move |headers: HeaderMap, Path(id): Path<String>| async move {
                    if !authorized(&headers) {
                        return StatusCode::UNAUTHORIZED.into_response();
                    }
                    let list = sched.lock().unwrap().get(&id).cloned().unwrap_or(json!([]));
                    Json(json!({ "data": list, "max_sched_num": 24 })).into_response()
                }
            })
            .post({
                let sched = sched.clone();
                move |headers: HeaderMap, Path(id): Path<String>, Json(body): Json<Value>| async move {
                    if !authorized(&headers) {
                        return StatusCode::UNAUTHORIZED.into_response();
                    }
                    let mut s = body["schedule"].clone();
                    let new_id = format!("sched-{}", rand_id());
                    s["_id"] = json!(new_id);
                    let mut all = sched.lock().unwrap();
                    let list = all.as_object_mut().unwrap().entry(id).or_insert(json!([]));
                    list.as_array_mut().unwrap().push(s);
                    Json(json!({ "_id": new_id })).into_response()
                }
            }),
        )
        .route(
            "/api/v1/installations/{id}/schedule/{sid}",
            axum::routing::patch({
                let sched = sched.clone();
                move |headers: HeaderMap, Path((id, sid)): Path<(String, String)>, Json(body): Json<Value>| async move {
                    if !authorized(&headers) {
                        return StatusCode::UNAUTHORIZED.into_response();
                    }
                    let mut all = sched.lock().unwrap();
                    let Some(list) = all.get_mut(&id).and_then(Value::as_array_mut) else { return StatusCode::NOT_FOUND.into_response() };
                    let Some(s) = list.iter_mut().find(|s| s["_id"] == sid.as_str()) else { return StatusCode::NOT_FOUND.into_response() };
                    let mut new = body["schedule"].clone();
                    new["_id"] = json!(sid);
                    *s = new;
                    Json(json!({})).into_response()
                }
            })
            .delete({
                let sched = sched.clone();
                move |headers: HeaderMap, Path((id, sid)): Path<(String, String)>| async move {
                    if !authorized(&headers) {
                        return StatusCode::UNAUTHORIZED.into_response();
                    }
                    let mut all = sched.lock().unwrap();
                    if let Some(list) = all.get_mut(&id).and_then(Value::as_array_mut) {
                        list.retain(|s| s["_id"] != sid.as_str());
                    }
                    Json(json!({})).into_response()
                }
            }),
        )
        .route(
            "/api/v1/installations/{id}/routines",
            get(move |headers: HeaderMap, Path(id): Path<String>| async move {
                if !authorized(&headers) {
                    return StatusCode::UNAUTHORIZED.into_response();
                }
                Json(json!({ "data": { "routines": a4["routines"].get(&id).cloned().unwrap_or(json!([])) } })).into_response()
            }),
        )
        .route("/api/v1/user/logout", get(|| async { Json(json!({})) }))
}
