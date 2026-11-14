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
    state: breezone_server::AppState,
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
            tokio::spawn(state.drivers.clone().run(Duration::from_millis(read_every), Duration::from_secs(record_every), None, 9));
        }
        let port = serve(breezone_server::app(state.clone())).await;
        let server = Server { base: format!("http://127.0.0.1:{port}"), http: reqwest::Client::new(), state };
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
    // Added by its address, it learns the identity of its hardware (to find it again if it moves).
    let (_, sources) = server.call("GET", "/api/sources", t, None).await;
    assert_eq!(sources[0]["config"]["device_id"], "shellypro3em-a1b2", "{sources}");

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
    // A production only points to the solar inverter it measures; a battery to nothing.
    let (s, _) = server.call("PUT", &format!("/api/devices/{meter_id}"), t, Some(json!({"role":"solar","measures_device_id": relay_id}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "the relay is not a solar inverter");
    let (_, d) = server.call("PUT", &format!("/api/devices/{meter_id}"), t, Some(json!({"role":"battery","measures_device_id": relay_id}))).await;
    assert_eq!(d["role"], "battery");
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
    // Over a given period, with its own buckets.
    let from = h["series"]["power"][0][0].as_i64().unwrap() - 3600;
    let (s, p) = server.call("GET", &format!("/api/devices/{meter}/history?from={from}&until={}&bucket=900", from + 7200), t, None).await;
    assert_eq!(s, StatusCode::OK, "{p}");
    assert_eq!(p["bucket"], 900);
    assert_eq!(p["series"]["power"][0][1], 2410.5, "{p}");
    let (s, _) = server.call("GET", &format!("/api/devices/{meter}/history?from={from}"), t, None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
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

/// A V2C Trydan answering its local API; while charging, each reading adds 0.5 kWh to the session.
async fn fake_trydan(data: Arc<Mutex<Value>>) -> u16 {
    use axum::extract::Path;
    let router = Router::new()
        .route(
            "/RealTimeData",
            get(|State(d): State<Arc<Mutex<Value>>>| async move {
                let mut d = d.lock().unwrap();
                if d["ChargeState"] == 2 && d["Paused"] == 0 {
                    d["ChargeEnergy"] = json!(d["ChargeEnergy"].as_f64().unwrap() + 0.5);
                }
                Json(d.clone())
            }),
        )
        .route(
            "/write/{kv}",
            get(|State(d): State<Arc<Mutex<Value>>>, Path(kv): Path<String>| async move {
                let (k, v) = kv.split_once('=').unwrap();
                d.lock().unwrap()[k] = json!(v.parse::<i64>().unwrap());
                "OK"
            }),
        )
        .with_state(data);
    serve(router).await
}

#[tokio::test]
async fn a_trydan_charges_with_a_maximum_and_stops_once_its_energy_is_added() {
    let data = Arc::new(Mutex::new(json!({"ID":"XY99R6","FirmwareVersion":"2.5.1","ChargeState":0,"ChargeMode":0,"ChargePower":0.0,"VoltageInstallation":222,
        "ChargeEnergy":0.0,"ChargeTime":0,"Paused":0,"Locked":0,"Intensity":0,"IntensityMeasure_L1":0.0,"Dynamic":1,"MinIntensity":6,"MaxIntensity":32})));
    let trydan = fake_trydan(data.clone()).await;
    let (server, token) = Server::start().await;
    let t = Some(token.as_str());

    let (s, source) =
        server.call("POST", "/api/sources", t, Some(json!({"driver":"trydan","name":"Borne","config":{"host": format!("127.0.0.1:{trydan}"), "device_id": "XY99R6"}}))).await;
    assert_eq!(s, StatusCode::CREATED, "{source}");
    assert_eq!(source["status"]["state"], "online", "{source}");
    assert_eq!(source["status"]["model"], "V2C Trydan (monophasé · 6–32 A)");
    let refresh = format!("/api/sources/{}/refresh", source["id"]);

    let (_, devices) = server.call("GET", "/api/devices", t, None).await;
    let charger = devices.as_array().unwrap().iter().find(|d| d["source"] == "trydan").unwrap().clone();
    assert_eq!(charger["traits"], json!(["ev.charger@1", "energy.power@1"]));
    assert_eq!(charger["state"]["status"], "available");
    assert_eq!(charger["state"]["auto_limit"], true);
    let actions = format!("/api/devices/{}/actions", charger["id"]);

    // Out of range, unknown parameter: nothing is written.
    let (s, _) = server.call("POST", &actions, t, Some(json!({"trait":"ev.charger","action":"start","params":{"limit":40}}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = server.call("POST", &actions, t, Some(json!({"trait":"ev.charger","action":"start","params":{"until":"07:00"}}))).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(data.lock().unwrap()["Dynamic"], 1);

    // Start at 10 A at most, for 1 kWh: Breezone takes the current over from the charger.
    let (s, d) = server.call("POST", &actions, t, Some(json!({"trait":"ev.charger","action":"start","params":{"limit":10,"energy":1.0}}))).await;
    assert_eq!(s, StatusCode::OK, "{d}");
    {
        let d = data.lock().unwrap();
        assert_eq!((d["Dynamic"].clone(), d["Intensity"].clone(), d["Paused"].clone()), (json!(0), json!(10), json!(0)));
    }
    assert_eq!(d["state"]["limit"], 10);
    assert_eq!(d["state"]["energy_goal"], 1.0);

    // The car charges: 0.5 kWh per reading; at 1 kWh Breezone pauses the charger.
    data.lock().unwrap()["ChargeState"] = json!(2);
    for _ in 0..3 {
        server.call("POST", &refresh, t, None).await;
    }
    assert_eq!(data.lock().unwrap()["Paused"], 1, "stopped once 1 kWh is added");
    let (_, devices) = server.call("GET", "/api/devices", t, None).await;
    let charger = devices.as_array().unwrap().iter().find(|d| d["source"] == "trydan").unwrap().clone();
    assert_eq!(charger["state"]["paused"], true);
    assert_eq!(charger["state"]["energy_goal"], Value::Null, "the goal is over");

    // Started again without a goal, then stopped by hand.
    server.call("POST", &actions, t, Some(json!({"trait":"ev.charger","action":"start","params":{}}))).await;
    assert_eq!(data.lock().unwrap()["Paused"], 0);
    let (s, _) = server.call("POST", &actions, t, Some(json!({"trait":"ev.charger","action":"stop","params":{}}))).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(data.lock().unwrap()["Paused"], 1);
    let (s, _) = server.call("POST", &actions, t, Some(json!({"trait":"ev.charger","action":"lock","params":{"locked":true}}))).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(data.lock().unwrap()["Locked"], 1);
}

/// Protobuf fields, encoded the way a Hoymiles DTU does.
fn pb(fields: &[(u32, PbValue)]) -> Vec<u8> {
    fn varint(out: &mut Vec<u8>, mut n: u64) {
        loop {
            let b = (n & 0x7f) as u8;
            n >>= 7;
            if n == 0 {
                out.push(b);
                return;
            }
            out.push(b | 0x80);
        }
    }
    let mut out = vec![];
    for (f, v) in fields {
        match v {
            PbValue::Int(n) => {
                varint(&mut out, u64::from(*f) << 3);
                varint(&mut out, *n as u64);
            }
            PbValue::Bytes(b) => {
                varint(&mut out, (u64::from(*f) << 3) | 2);
                varint(&mut out, b.len() as u64);
                out.extend_from_slice(b);
            }
        }
    }
    out
}

enum PbValue {
    Int(i64),
    Bytes(Vec<u8>),
}

fn hm_frame(command: [u8; 2], payload: &[u8]) -> Vec<u8> {
    let mut crc = 0xffffu16;
    for b in payload {
        crc ^= u16::from(*b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xa001 } else { crc >> 1 };
        }
    }
    let mut out = b"HM".to_vec();
    out.extend_from_slice(&command);
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&crc.to_be_bytes());
    out.extend_from_slice(&(payload.len() as u16 + 10).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

/// A Hoymiles DTU-W100 with one HMS-1600-4T and four panels (the values of a real one). Counts
/// the requests it gets.
async fn fake_dtu(requests: Arc<AtomicU64>) -> u16 {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use PbValue::{Bytes, Int};
    let serial = 19123694208274i64;
    let dtu = pb(&[(2, Int(1537)), (11, Bytes(b"2.3.0.0-dev".to_vec()))]);
    let app_info = pb(&[(1, Bytes(b"4133A0054CBB".to_vec())), (4, Int(4)), (8, Bytes(dtu)), (11, Bytes(pb(&[(2, Int(serial))])))]);
    let sgs = pb(&[(1, Int(serial)), (3, Int(2322)), (4, Int(5000)), (5, Int(4869)), (7, Int(209)), (9, Int(251)), (12, Int(1))]);
    let panel = |port: i64, w: i64, total: i64, day: i64| Bytes(pb(&[(1, Int(serial)), (2, Int(port)), (3, Int(402)), (4, Int(390)), (5, Int(w)), (6, Int(total)), (7, Int(day))]));
    let real = pb(&[
        (1, Bytes(b"4133A0054CBB".to_vec())),
        (3, Int(1)),
        (9, Bytes(sgs)),
        (11, panel(1, 1571, 1176763, 338)),
        (11, panel(2, 1562, 1176453, 337)),
        (11, panel(3, 996, 957769, 224)),
        (11, panel(4, 995, 956530, 224)),
    ]);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else { return };
            requests.fetch_add(1, Ordering::SeqCst);
            let mut buf = [0u8; 512];
            let n = socket.read(&mut buf).await.unwrap_or(0);
            let answer = match buf.get(2..4) {
                Some([0xa3, 0x01]) if n >= 10 => hm_frame([0xa2, 0x01], &app_info),
                Some([0xa3, 0x11]) if n >= 10 => hm_frame([0xa2, 0x11], &real),
                _ => continue,
            };
            let _ = socket.write_all(&answer).await;
        }
    });
    port
}

#[tokio::test]
async fn a_hoymiles_dtu_gives_its_inverter_and_panels() {
    let requests = Arc::new(AtomicU64::new(0));
    let dtu = fake_dtu(requests.clone()).await;
    let (server, token) = Server::start().await;
    let t = Some(token.as_str());

    let (s, source) = server
        .call("POST", "/api/sources", t, Some(json!({"driver":"hoymiles","name":"Solaire","config":{"host": format!("127.0.0.1:{dtu}"), "device_id": "4133A0054CBB"}})))
        .await;
    assert_eq!(s, StatusCode::CREATED, "{source}");
    assert_eq!(source["status"]["state"], "online", "{source}");
    assert_eq!(source["status"]["model"], "Hoymiles DTU-W100/DTU-Lite-S (1 micro-onduleur · 4 panneaux)");
    assert_eq!(source["status"]["firmware"], "2.3.0.0-dev");
    assert_eq!(source["devices"], 5);

    let (_, devices) = server.call("GET", "/api/devices", t, None).await;
    let devices: Vec<Value> = devices.as_array().unwrap().iter().filter(|d| d["source"] == "hoymiles").cloned().collect();
    let inverter = devices.iter().find(|d| d["kind"] == "solar.inverter").unwrap();
    assert_eq!(inverter["name"], "HMS-1600/1800/2000-4T (0512)");
    assert_eq!(inverter["traits"], json!(["energy.power@1", "solar.inverter@1"]));
    assert_eq!(inverter["state"]["power"], 486.9);
    assert_eq!(inverter["state"]["energy_today"], 1.123);
    assert_eq!(inverter["state"]["temperature"], 25.1);
    let panel = devices.iter().find(|d| d["reference"] == "pv:116495000512:1").unwrap();
    assert_eq!(panel["name"], "Panneau 1 (0512)");
    assert_eq!((panel["state"]["power"].clone(), panel["state"]["energy_total"].clone()), (json!(157.1), json!(1176.763)));

    // Read only: nothing to drive.
    let (s, _) = server.call("POST", &format!("/api/devices/{}/actions", inverter["id"]), t, Some(json!({"trait":"switch.relay","action":"set","params":{"on":false}}))).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);

    // An inverter is the solar production of the home without being told.
    let (_, live) = server.call("GET", "/api/energy/live", t, None).await;
    assert_eq!(live["roles"]["solar"], true, "{live}");
    assert_eq!(live["solar"], 486.9, "{live}");

    // A meter on the inverter's line measures it: both count once, through the meter.
    let shelly = fake_shelly(Arc::new(Mutex::new(false))).await;
    server.call("POST", "/api/sources", t, Some(json!({"driver":"shelly","name":"Tableau","config":{"host": format!("127.0.0.1:{shelly}")}}))).await;
    let (_, all) = server.call("GET", "/api/devices", t, None).await;
    let meter = all.as_array().unwrap().iter().find(|d| d["reference"] == "em:0").unwrap()["id"].as_i64().unwrap();
    // A production that does not say what it measures may measure the inverter: counted once.
    server.call("PUT", &format!("/api/devices/{meter}"), t, Some(json!({"role":"solar"}))).await;
    let (_, live) = server.call("GET", "/api/energy/live", t, None).await;
    assert_eq!(live["solar"], 2410.5, "{live}");
    // Unless the inverter is told to count as well (another installation).
    server.call("PUT", &format!("/api/devices/{}", inverter["id"]), t, Some(json!({"role":"solar"}))).await;
    let (_, live) = server.call("GET", "/api/energy/live", t, None).await;
    assert_eq!(live["solar"], 2897.4, "{live}");
    server.call("PUT", &format!("/api/devices/{}", inverter["id"]), t, Some(json!({"role":null}))).await;
    let (s, d) = server.call("PUT", &format!("/api/devices/{meter}"), t, Some(json!({"role":"solar","measures_device_id": inverter["id"]}))).await;
    assert_eq!(s, StatusCode::OK, "{d}");
    assert_eq!(d["measures"]["device_id"], inverter["id"]);
    let (_, live) = server.call("GET", "/api/energy/live", t, None).await;
    assert_eq!(live["solar"], 2410.5, "the meter, not meter + inverter: {live}");
    // Given as the consumption of the inverter, the meter is still its production.
    server.call("PUT", &format!("/api/devices/{meter}"), t, Some(json!({"role":"consumption","measures_device_id": inverter["id"]}))).await;
    let (_, live) = server.call("GET", "/api/energy/live", t, None).await;
    assert_eq!(live["solar"], 2410.5, "{live}");
    assert_eq!(live["consumers"], json!([]), "{live}");

    // The DTU is not read again before its 30 s (the runtime asks every 5 s).
    let before = requests.load(Ordering::SeqCst);
    let (_, sources) = server.call("GET", "/api/sources", t, None).await;
    assert_eq!(sources[0]["status"]["state"], "online");
    assert_eq!(requests.load(Ordering::SeqCst), before);
}

#[tokio::test]
async fn a_lost_source_is_found_again_at_its_new_address() {
    use breezone_server::discovery::Product;
    let shelly = fake_shelly(Arc::new(Mutex::new(false))).await;
    let (server, token) = Server::start().await;
    let t = Some(token.as_str());
    // Its address changed: nothing answers at the old one any more.
    let (_, source) =
        server.call("POST", "/api/sources", t, Some(json!({"driver":"shelly","name":"Tableau","config":{"host":"127.0.0.1:9","device_id":"shellypro3em-a1b2"}}))).await;
    assert_eq!(source["status"]["state"], "offline", "{source}");
    let id = source["id"].as_i64().unwrap();
    let product = |device_id: &str| Product {
        brand: "shelly",
        driver: "shelly",
        model: "Shelly Pro 3EM".into(),
        host: format!("127.0.0.1:{shelly}"),
        port: 80,
        mac: None,
        device_id: Some(device_id.into()),
        firmware: None,
        note: None,
        supported: true,
        flags: 0,
    };

    // Another Shelly on the network is not it.
    assert!(server.state.drivers.relocate(&[product("shellyplus1pm-ffff")], &[id]).await.is_empty());
    // The same one, elsewhere: the source moves there and reads it at once.
    assert_eq!(server.state.drivers.relocate(&[product("shellypro3em-a1b2")], &[id]).await, vec![id]);
    let (_, sources) = server.call("GET", "/api/sources", t, None).await;
    assert_eq!(sources[0]["config"]["host"], format!("127.0.0.1:{shelly}"), "{sources}");
    assert_eq!(sources[0]["status"]["state"], "online", "{sources}");
    let (_, events) = server.call("GET", "/api/events", t, None).await;
    assert!(events.to_string().contains("127.0.0.1:9"), "the move is in the activity: {events}");
}
