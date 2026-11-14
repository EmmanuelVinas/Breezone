//! Drivers (ADR 0013): a source of devices is configured, read through its driver, and its devices
//! are listed and driven through their traits like the Airzone zones.

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use breezone_server::config::Config;
use reqwest::StatusCode;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

async fn serve(router: Router) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    port
}

/// A Shelly Pro 3EM with one relay, answering its local RPC API.
async fn fake_shelly(relay: Arc<Mutex<bool>>) -> u16 {
    let router = Router::new()
        .route("/rpc/Shelly.GetDeviceInfo", get(|| async { Json(json!({"id": "shellypro3em-a1b2", "app": "Pro3EM", "ver": "1.4.4", "gen": 2})) }))
        .route(
            "/rpc/Shelly.GetStatus",
            get(|State(relay): State<Arc<Mutex<bool>>>| async move {
                Json(json!({
                    "em:0": {"id": 0, "a_act_power": 1120.4, "b_act_power": 640.0, "c_act_power": 650.1, "total_act_power": 2410.5, "a_voltage": 232.1},
                    "emdata:0": {"id": 0, "total_act": 1234567.0, "total_act_ret": 45678.0},
                    "switch:0": {"id": 0, "output": *relay.lock().unwrap(), "apower": 0.0, "aenergy": {"total": 900.0}},
                    "sys": {"uptime": 10}
                }))
            }),
        )
        .route(
            "/rpc/Switch.Set",
            get(|State(relay): State<Arc<Mutex<bool>>>, Query(q): Query<HashMap<String, String>>| async move {
                *relay.lock().unwrap() = q.get("on").map(String::as_str) == Some("true");
                Json(json!({"was_on": false}))
            }),
        )
        .with_state(relay);
    serve(router).await
}

struct Server {
    base: String,
    http: reqwest::Client,
}

impl Server {
    async fn start() -> (Server, String) {
        let dir = std::env::temp_dir().join(format!("breezone-drivers-{}-{}", std::process::id(), rand_suffix()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut cfg = Config::for_tests(dir);
        cfg.scan_port = 9;
        let state = breezone_server::init_state(cfg).await.unwrap();
        let port = serve(breezone_server::app(state)).await;
        let server = Server { base: format!("http://127.0.0.1:{port}"), http: reqwest::Client::new() };
        let (_, v) = server.call("POST", "/api/setup", None, Some(json!({"username":"admin","password":"motdepasse1"}))).await;
        (server, v["access_token"].as_str().unwrap().to_string())
    }

    async fn call(&self, method: &str, path: &str, token: Option<&str>, body: Option<Value>) -> (StatusCode, Value) {
        let mut req = self.http.request(method.parse().unwrap(), format!("{}{path}", self.base));
        if let Some(t) = token {
            req = req.bearer_auth(t);
        }
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await.unwrap();
        let status = resp.status();
        (status, serde_json::from_str(&resp.text().await.unwrap()).unwrap_or(Value::Null))
    }
}

fn rand_suffix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64
}

#[tokio::test]
async fn a_shelly_source_provides_devices_driven_through_their_traits() {
    let relay = Arc::new(Mutex::new(false));
    let shelly = fake_shelly(relay.clone()).await;
    let (server, token) = Server::start().await;
    let t = Some(token.as_str());

    // The drivers and their configuration form.
    let (_, drivers) = server.call("GET", "/api/drivers", t, None).await;
    let d = drivers.as_array().unwrap().iter().find(|d| d["id"] == "shelly").unwrap();
    assert_eq!(d["config"][0]["name"], "host");

    // A source is checked, then read at once.
    let (s, _) = server.call("POST", "/api/sources", t, Some(json!({"driver":"shelly","name":"Tableau","config":{"host":"evil.example/x"}}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = server.call("POST", "/api/sources", t, Some(json!({"driver":"nope","name":"x","config":{}}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, source) = server.call("POST", "/api/sources", t, Some(json!({"driver":"shelly","name":"Tableau","config":{"host": format!("127.0.0.1:{shelly}")}}))).await;
    assert_eq!(s, StatusCode::CREATED, "{source}");
    assert_eq!(source["status"]["state"], "online", "{source}");
    assert_eq!(source["status"]["model"], "Shelly Pro 3EM (triphasé)");
    assert_eq!(source["devices"], 2);

    // Its devices: a three-phase meter and a relay, with their state.
    let (_, devices) = server.call("GET", "/api/devices", t, None).await;
    let devices = devices.as_array().unwrap().clone();
    let meter = devices.iter().find(|d| d["reference"] == "em:0").unwrap().clone();
    assert_eq!(meter["source"], "shelly");
    assert_eq!(meter["traits"], json!(["energy.power@1", "energy.meter@1"]));
    assert_eq!(meter["state"]["power"], 2410.5);
    assert_eq!(meter["state"]["energy_in"], 1234.567);
    assert_eq!(meter["name"], "Pro 3EM compteur");
    let relay_dev = devices.iter().find(|d| d["reference"] == "switch:0").unwrap().clone();
    assert_eq!(relay_dev["state"]["on"], false);
    let relay_id = relay_dev["id"].as_i64().unwrap();
    let actions = format!("/api/devices/{relay_id}/actions");

    // The generic action switches the relay on the hardware.
    let (s, d) = server.call("POST", &actions, t, Some(json!({"trait":"switch.relay","action":"set","params":{"on":true}}))).await;
    assert_eq!(s, StatusCode::OK, "{d}");
    assert!(*relay.lock().unwrap());
    assert_eq!(d["state"]["on"], true, "read again after the action");

    // Never a half-applied action: unknown parameter, missing or mistyped one, foreign action.
    let (s, v) = server.call("POST", &actions, t, Some(json!({"trait":"switch.relay","action":"set","params":{"on":true,"delay":5}}))).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(v["code"], "unsupported");
    let (s, _) = server.call("POST", &actions, t, Some(json!({"trait":"switch.relay","action":"set","params":{}}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = server.call("POST", &actions, t, Some(json!({"trait":"switch.relay","action":"set","params":{"on":"yes"}}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = server.call("POST", &format!("/api/devices/{}/actions", meter["id"]), t, Some(json!({"trait":"switch.relay","action":"set","params":{"on":true}}))).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "the meter has no relay");

    // A user sees the devices but neither drives them nor manages the sources (for now).
    server.call("POST", "/api/users", t, Some(json!({"username":"bob","password":"motdepasse1","role":"user"}))).await;
    let (_, v) = server.call("POST", "/api/auth/login", None, Some(json!({"username":"bob","password":"motdepasse1"}))).await;
    let bob = v["access_token"].as_str().unwrap().to_string();
    let (_, seen) = server.call("GET", "/api/devices", Some(&bob), None).await;
    assert_eq!(seen.as_array().unwrap().len(), 2);
    let (s, _) = server.call("POST", &actions, Some(&bob), Some(json!({"trait":"switch.relay","action":"set","params":{"on":false}}))).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = server.call("GET", "/api/sources", Some(&bob), None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // A source that does not answer is reported, and its devices go with it when removed.
    let (_, dead) = server.call("POST", "/api/sources", t, Some(json!({"driver":"shelly","name":"Garage","config":{"host":"127.0.0.1:9"}}))).await;
    assert_eq!(dead["status"]["state"], "offline", "{dead}");
    let id = source["id"].as_i64().unwrap();
    let (s, _) = server.call("DELETE", &format!("/api/sources/{id}"), t, None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, devices) = server.call("GET", "/api/devices", t, None).await;
    assert!(devices.as_array().unwrap().iter().all(|d| d["source_id"] != id), "{devices}");
}
