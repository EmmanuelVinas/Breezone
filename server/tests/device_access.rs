//! The devices of the modules are given to a user apart: all of them, a room, a type or one
//! device, read-only or with control.

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use breezone_server::config::Config;
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

async fn serve(router: Router) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    port
}

/// A Shelly Pro 3EM: an energy meter and a relay.
async fn fake_shelly() -> u16 {
    let relay = Arc::new(Mutex::new(false));
    let router = Router::new()
        .route("/rpc/Shelly.GetDeviceInfo", get(|| async { Json(json!({"id": "shellypro3em-c3d4", "app": "Pro3EM", "ver": "1.4.4", "gen": 2})) }))
        .route(
            "/rpc/Shelly.GetStatus",
            get(|State(r): State<Arc<Mutex<bool>>>| async move {
                Json(json!({
                    "em:0": {"id": 0, "a_act_power": 100.0, "b_act_power": 100.0, "c_act_power": 100.0, "total_act_power": 300.0},
                    "emdata:0": {"id": 0, "total_act": 1000.0, "total_act_ret": 0.0},
                    "switch:0": {"id": 0, "output": *r.lock().unwrap(), "apower": 0.0, "aenergy": {"total": 0.0}},
                    "sys": {"uptime": 10}
                }))
            }),
        )
        .route(
            "/rpc/Switch.Set",
            get(|State(r): State<Arc<Mutex<bool>>>, Query(q): Query<HashMap<String, String>>| async move {
                *r.lock().unwrap() = q.get("on").map(String::as_str) == Some("true");
                Json(json!({"was_on": false}))
            }),
        )
        .with_state(relay);
    serve(router).await
}

struct Client {
    base: String,
    http: reqwest::Client,
    token: String,
}

impl Client {
    async fn call(&self, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut req = self.http.request(method, format!("{}{}", self.base, path)).bearer_auth(&self.token);
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await.unwrap();
        let status = resp.status();
        (status, serde_json::from_str(&resp.text().await.unwrap()).unwrap_or(Value::Null))
    }
}

async fn token(http: &reqwest::Client, base: &str, path: &str, user: &str) -> String {
    let v: Value = http.post(format!("{base}{path}")).json(&json!({"username": user, "password": "motdepasse1"})).send().await.unwrap().json().await.unwrap();
    v["access_token"].as_str().unwrap_or_else(|| panic!("{v}")).to_string()
}

/// The devices of the modules this user sees: (kind, can_control).
async fn modules(c: &Client) -> Vec<(String, bool)> {
    let (_, v) = c.call(Method::GET, "/api/devices", None).await;
    let mut out: Vec<(String, bool)> =
        v.as_array().unwrap().iter().filter(|d| d["installation_id"].is_null()).map(|d| (d["kind"].as_str().unwrap().to_string(), d["can_control"].as_bool().unwrap())).collect();
    out.sort();
    out
}

#[tokio::test]
async fn the_devices_of_the_modules_are_given_by_type_room_or_device() {
    let dir = std::env::temp_dir().join(format!("breezone-device-access-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut cfg = Config::for_tests(dir);
    cfg.scan_port = 9;
    let state = breezone_server::init_state(cfg).await.unwrap();
    let base = format!("http://127.0.0.1:{}", serve(breezone_server::app(state)).await);
    let http = reqwest::Client::new();
    let admin = Client { token: token(&http, &base, "/api/setup", "admin").await, base: base.clone(), http: http.clone() };
    let port = fake_shelly().await;
    let (s, v) = admin.call(Method::POST, "/api/sources", Some(json!({"driver": "shelly", "name": "Tableau", "config": {"host": format!("127.0.0.1:{port}")}}))).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let (_, devices) = admin.call(Method::GET, "/api/devices", None).await;
    let id_of = |kind: &str| devices.as_array().unwrap().iter().find(|d| d["kind"] == kind).unwrap_or_else(|| panic!("{devices}"))["id"].as_i64().unwrap();
    let relay = id_of("switch.relay");
    let meter = id_of("energy.power");

    let user = |name: &str, devices: Value| {
        let admin = &admin;
        let name = name.to_string();
        async move {
            let (s, v) = admin.call(Method::POST, "/api/users", Some(json!({"username": name, "password": "motdepasse1", "role": "user", "devices": devices}))).await;
            assert_eq!(s, StatusCode::CREATED, "{v}");
            Client { token: token(&admin.http, &admin.base, "/api/auth/login", &name).await, base: admin.base.clone(), http: admin.http.clone() }
        }
    };
    let set = json!({"trait": "switch.relay", "action": "set", "params": {"on": true}});

    // The relays, with control: the relay only, driven; not the energy of the home.
    let lea = user("lea", json!([{"scope": "type", "ref": "switch", "permission": "control"}])).await;
    assert_eq!(modules(&lea).await, vec![("switch.relay".to_string(), true)]);
    let (s, v) = lea.call(Method::POST, &format!("/api/devices/{relay}/actions"), Some(set.clone())).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, _) = lea.call(Method::POST, &format!("/api/devices/{meter}/actions"), Some(set.clone())).await;
    assert_eq!(s, StatusCode::NOT_FOUND, "a device not given is not revealed");
    let (s, _) = lea.call(Method::GET, "/api/energy/live", None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // Everything read-only: seen, not driven; the energy of the home is read.
    let ana = user("ana", json!([{"scope": "all", "permission": "read"}])).await;
    assert_eq!(modules(&ana).await, vec![("energy.power".to_string(), false), ("switch.relay".to_string(), false)]);
    let (s, _) = ana.call(Method::POST, &format!("/api/devices/{relay}/actions"), Some(set.clone())).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = ana.call(Method::GET, "/api/energy/live", None).await;
    assert_eq!(s, StatusCode::OK);

    // One device, with control, over everything read-only: the most a grant gives wins.
    let (s, v) = admin
        .call(
            Method::PUT,
            &format!("/api/users/{}", user_id(&admin, "ana").await),
            Some(json!({"devices": [{"scope": "all", "permission": "read"}, {"scope": "device", "ref": relay.to_string(), "permission": "control"}]})),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(modules(&ana).await, vec![("energy.power".to_string(), false), ("switch.relay".to_string(), true)]);

    // Nothing given: none of the devices of the modules.
    let tom = user("tom", json!([])).await;
    assert!(modules(&tom).await.is_empty());

    // What does not exist is refused.
    let (s, _) = admin
        .call(Method::POST, "/api/users", Some(json!({"username": "zoe", "password": "motdepasse1", "devices": [{"scope": "type", "ref": "toaster", "permission": "read"}]})))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = admin
        .call(Method::POST, "/api/users", Some(json!({"username": "zoe", "password": "motdepasse1", "devices": [{"scope": "room", "ref": "999", "permission": "read"}]})))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

async fn user_id(admin: &Client, name: &str) -> i64 {
    let (_, v) = admin.call(Method::GET, "/api/users", None).await;
    v.as_array().unwrap().iter().find(|u| u["username"] == name).unwrap()["id"].as_i64().unwrap()
}
