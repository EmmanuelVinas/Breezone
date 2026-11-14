//! Drivers (ADR 0013): a source of devices is configured, read through its driver, and its devices
//! are listed and driven through their traits like the Airzone zones.

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use breezone_server::config::Config;
use reqwest::StatusCode;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

async fn serve(router: Router) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    port
}

#[derive(Clone)]
struct Fake {
    relay: Arc<Mutex<bool>>,
    /// Wh the meter counts at each reading.
    step: u64,
    counted: Arc<AtomicU64>,
}

/// A Shelly Pro 3EM with one relay, answering its local RPC API.
async fn fake_shelly(relay: Arc<Mutex<bool>>) -> u16 {
    fake_shelly_counting(relay, 0).await
}

async fn fake_shelly_counting(relay: Arc<Mutex<bool>>, step: u64) -> u16 {
    let router = Router::new()
        .route("/rpc/Shelly.GetDeviceInfo", get(|| async { Json(json!({"id": "shellypro3em-a1b2", "app": "Pro3EM", "ver": "1.4.4", "gen": 2})) }))
        .route(
            "/rpc/Shelly.GetStatus",
            get(|State(f): State<Fake>| async move {
                let wh = f.counted.fetch_add(f.step, Ordering::SeqCst) as f64;
                Json(json!({
                    "em:0": {"id": 0, "a_act_power": 1120.4, "b_act_power": 640.0, "c_act_power": 650.1, "total_act_power": 2410.5, "a_voltage": 232.1},
                    "emdata:0": {"id": 0, "total_act": 1234567.0 + wh, "total_act_ret": 45678.0},
                    "switch:0": {"id": 0, "output": *f.relay.lock().unwrap(), "apower": 0.0, "aenergy": {"total": 900.0}},
                    "sys": {"uptime": 10}
                }))
            }),
        )
        .route(
            "/rpc/Switch.Set",
            get(|State(f): State<Fake>, Query(q): Query<HashMap<String, String>>| async move {
                *f.relay.lock().unwrap() = q.get("on").map(String::as_str) == Some("true");
                Json(json!({"was_on": false}))
            }),
        )
        .with_state(Fake { relay, step, counted: Arc::new(AtomicU64::new(0)) });
    serve(router).await
}

struct Server {
    base: String,
    http: reqwest::Client,
}

impl Server {
    async fn start() -> (Server, String) {
        Self::start_with(|_| {}, false).await
    }

    /// `read`: the sources are read and their history written in the background.
    async fn start_with(tune: impl FnOnce(&mut Config), read: bool) -> (Server, String) {
        let dir = std::env::temp_dir().join(format!("breezone-drivers-{}-{}", std::process::id(), rand_suffix()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut cfg = Config::for_tests(dir);
        cfg.scan_port = 9;
        tune(&mut cfg);
        let (read_every, record_every) = (cfg.device_read_ms, cfg.history_interval_secs as u64);
        let state = breezone_server::init_state(cfg).await.unwrap();
        if read {
            use std::time::Duration;
            tokio::spawn(state.drivers.clone().run(Duration::from_millis(read_every), Duration::from_secs(record_every)));
        }
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

fn rand_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!("{}-{}", SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(), NEXT.fetch_add(1, Ordering::SeqCst))
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

    // Roles of the energy measures (energy page): the meter is the whole home, the relay's own
    // measure is the consumption of a device.
    let meter_id = meter["id"].as_i64().unwrap();
    let (s, d) = server.call("PUT", &format!("/api/devices/{meter_id}"), t, Some(json!({"role":"grid"}))).await;
    assert_eq!(s, StatusCode::OK, "{d}");
    assert_eq!(d["role"], "grid");
    let (s, d) = server.call("PUT", &format!("/api/devices/{relay_id}"), t, Some(json!({"role":"consumption","measures_device_id": meter_id}))).await;
    assert_eq!(s, StatusCode::OK, "{d}");
    assert_eq!(d["measures"]["device_id"], meter_id);
    let (s, _) = server.call("PUT", &format!("/api/devices/{relay_id}"), t, Some(json!({"role":"consumption","measures_device_id": relay_id}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "not itself");
    let (s, _) = server.call("PUT", &format!("/api/devices/{meter_id}"), t, Some(json!({"role":"magic"}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // Only consumption points to what it measures.
    let (_, d) = server.call("PUT", &format!("/api/devices/{meter_id}"), t, Some(json!({"role":"solar","measures_device_id": relay_id}))).await;
    assert_eq!(d["role"], "solar");
    assert_eq!(d["measures"]["device_id"], Value::Null);

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

#[tokio::test]
async fn the_measures_have_a_history_and_make_the_energy_of_the_home() {
    let shelly = fake_shelly_counting(Arc::new(Mutex::new(false)), 1).await;
    // Read often, a history point every second.
    let tune = |cfg: &mut Config| {
        cfg.device_read_ms = 200;
        cfg.history_interval_secs = 1;
    };
    let (server, token) = Server::start_with(tune, true).await;
    let t = Some(token.as_str());
    let (s, _) = server.call("POST", "/api/sources", t, Some(json!({"driver":"shelly","name":"Tableau","config":{"host": format!("127.0.0.1:{shelly}")}}))).await;
    assert_eq!(s, StatusCode::CREATED);

    // Nothing is shown until the measures have a role.
    let (_, live) = server.call("GET", "/api/energy/live", t, None).await;
    assert_eq!(live["roles"]["grid"], false, "{live}");
    assert_eq!(live["home"], Value::Null);

    let (_, devices) = server.call("GET", "/api/devices", t, None).await;
    let id = |reference: &str| devices.as_array().unwrap().iter().find(|d| d["reference"] == reference).unwrap()["id"].as_i64().unwrap();
    let (meter, relay) = (id("em:0"), id("switch:0"));
    server.call("PUT", &format!("/api/devices/{meter}"), t, Some(json!({"role":"grid"}))).await;
    server.call("PUT", &format!("/api/devices/{relay}"), t, Some(json!({"role":"consumption","name":"Pompe"}))).await;

    let (_, live) = server.call("GET", "/api/energy/live", t, None).await;
    assert_eq!((live["roles"]["grid"].clone(), live["roles"]["solar"].clone()), (json!(true), json!(false)), "{live}");
    assert_eq!(live["grid"], 2410.5);
    assert_eq!(live["home"], 2410.5);
    assert_eq!(live["solar"], Value::Null);
    assert_eq!(live["consumers"][0]["name"], "Pompe");
    assert_eq!(live["other"], 2410.5);

    tokio::time::sleep(std::time::Duration::from_millis(3500)).await;

    // The history of the meter: its power (mean), its counter (last value).
    let (s, h) = server.call("GET", &format!("/api/devices/{meter}/history?hours=1&points=60"), t, None).await;
    assert_eq!(s, StatusCode::OK, "{h}");
    assert_eq!(h["bucket"], 60);
    assert_eq!(h["series"]["power"][0][1], 2410.5, "{h}");
    assert!(h["series"]["energy_in"][0][1].as_f64().unwrap() > 1234.567, "{h}");
    let (s, _) = server.call("GET", "/api/devices/999999/history", t, None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // Its counter went up: energy drawn from the grid today, used by the home.
    let (_, days) = server.call("GET", "/api/energy/days?days=3", t, None).await;
    let days = days.as_array().unwrap();
    assert_eq!(days.len(), 3);
    let today = days.last().unwrap();
    assert!(today["imported"].as_f64().unwrap() > 0.0, "{today}");
    assert_eq!(today["consumption"], today["imported"]);
    assert_eq!(today["production"], Value::Null, "no solar measure");
    assert_eq!(today["consumers"][0]["name"], "Pompe");
    assert_eq!(days[0]["imported"], 0.0, "no reading two days ago");

    let (s, day) = server.call("GET", "/api/energy/day", t, None).await;
    assert_eq!(s, StatusCode::OK, "{day}");
    assert_eq!(day["bucket"], 900);
    let points = day["points"].as_array().unwrap();
    assert!((92..=100).contains(&points.len()));
    assert!(points.iter().any(|p| p["grid"] == 2410.5), "{day}");
    assert_eq!(day["totals"], *today);
    let (s, _) = server.call("GET", "/api/energy/day?date=demain", t, None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}
