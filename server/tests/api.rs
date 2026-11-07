//! End-to-end test: a real Breezone server talking to the fake Airzone WebServer.

use breezone_server::config::Config;
use breezone_server::mock::{self, MockState};
use futures_util::{SinkExt, StreamExt};
use reqwest::StatusCode;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn next_json(ws: &mut Ws) -> Value {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(5), ws.next()).await.unwrap().unwrap().unwrap();
        if let Message::Text(t) = msg {
            return serde_json::from_str::<Value>(&t).unwrap();
        }
    }
}

struct Env {
    base: String,
    http: reqwest::Client,
    mock: MockState,
    mock_port: u16,
}

async fn start() -> Env {
    // Fake Airzone WebServer.
    let mock_state: MockState = Arc::new(Mutex::new(mock::preset("maison")));
    let mock_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_port = mock_listener.local_addr().unwrap().port();
    let router = mock::router(mock_state.clone());
    tokio::spawn(async move { axum::serve(mock_listener, router).await.unwrap() });

    // Breezone server with a throw-away data directory.
    let dir = std::env::temp_dir().join(format!("breezone-test-{}-{}", std::process::id(), rand_suffix()));
    let state = breezone_server::init_state(Config::for_tests(dir)).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = breezone_server::app(state);
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    Env { base: format!("http://127.0.0.1:{port}"), http: reqwest::Client::new(), mock: mock_state, mock_port }
}

/// Unique per test in this process (tests run in parallel: a clock-based name could collide).
fn rand_suffix() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64;
    t.wrapping_add(n << 48)
}

impl Env {
    async fn call(&self, method: &str, path: &str, token: Option<&str>, body: Option<Value>) -> (StatusCode, Value) {
        let url = format!("{}{}", self.base, path);
        let mut req = match method {
            "GET" => self.http.get(url),
            "POST" => self.http.post(url),
            "PUT" => self.http.put(url),
            "PATCH" => self.http.patch(url),
            "DELETE" => self.http.delete(url),
            _ => unreachable!(),
        };
        if let Some(t) = token {
            req = req.bearer_auth(t);
        }
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await.unwrap();
        let status = resp.status();
        let text = resp.text().await.unwrap();
        (status, serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    fn mock_zone(&self, zone_id: i64) -> breezone_server::airzone::ZoneData {
        self.mock.lock().unwrap().iter().find(|z| z.zone_id == zone_id).cloned().unwrap()
    }
}

#[tokio::test]
async fn full_flow() {
    let env = start().await;

    // --- First start: setup creates the admin -------------------------------------------
    let (s, v) = env.call("GET", "/api/setup", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["needs_setup"], true);

    let (s, v) = env.call("POST", "/api/setup", None, Some(json!({"username":"admin","password":"motdepasse1","device_name":"Test"}))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let token = v["access_token"].as_str().unwrap().to_string();
    let refresh = v["refresh_token"].as_str().unwrap().to_string();
    assert_eq!(v["user"]["role"], "admin");

    let (s, _) = env.call("POST", "/api/setup", None, Some(json!({"username":"x","password":"motdepasse1"}))).await;
    assert_eq!(s, StatusCode::CONFLICT);

    // --- Auth is required --------------------------------------------------------------
    let (s, _) = env.call("GET", "/api/status", None, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = env.call("GET", "/api/status", Some("garbage"), None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, v) = env.call("GET", "/api/nope", Some(&token), None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert_eq!(v["error"], "not found");

    // --- Test connection then add the installation ---------------------------------------
    let (s, v) = env.call("POST", "/api/installations/test", Some(&token), Some(json!({"host":"127.0.0.1","port":env.mock_port}))).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["ok"], true);
    assert_eq!(v["zones"].as_array().unwrap().len(), 5);

    let (s, v) = env
        .call("POST", "/api/installations", Some(&token), Some(json!({"name":"Maison","host":"127.0.0.1","port":env.mock_port})))
        .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let inst = v["id"].as_i64().unwrap();
    assert_eq!(v["connection"]["state"], "online");
    let zones = v["zones"].as_array().unwrap();
    assert_eq!(zones.len(), 5);
    let salon = zones.iter().find(|z| z["name"] == "Salon").unwrap();
    assert_eq!(salon["is_master"], true);
    assert_eq!(salon["mode"], "heating");
    assert_eq!(salon["modes"].as_array().unwrap().len(), 5);
    let bureau = zones.iter().find(|z| z["name"] == "Bureau").unwrap();
    assert_eq!(bureau["is_master"], false);
    assert_eq!(bureau["modes"].as_array().unwrap().len(), 5, "slave zones expose the system modes");

    // --- WebSocket: auth, hello, snapshot, live update -----------------------------------
    let ws_url = env.base.replace("http://", "ws://") + "/api/ws";
    let (mut ws, _) = tokio_tungstenite::connect_async(ws_url.as_str()).await.unwrap();
    ws.send(Message::Text(json!({"type":"auth","token":token}).to_string().into())).await.unwrap();
    assert_eq!(next_json(&mut ws).await["type"], "hello");
    let snap = next_json(&mut ws).await;
    assert_eq!(snap["type"], "snapshot");
    assert_eq!(snap["installations"][0]["zones"].as_array().unwrap().len(), 5);

    // --- Commands -------------------------------------------------------------------------
    let (s, v) = env
        .call("PATCH", &format!("/api/installations/{inst}/zones/1/4"), Some(&token), Some(json!({"on":true,"setpoint":20.5})))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["on"], true);
    assert_eq!(v["setpoint"], 20.5);
    let m = env.mock_zone(4);
    assert_eq!(m.on, Some(1));
    assert_eq!(m.setpoint, Some(20.5));

    // The WebSocket receives the change (skip activity-feed events).
    let mut got_update = false;
    for _ in 0..10 {
        let msg = next_json(&mut ws).await;
        if msg["type"] == "installation" {
            let z = msg["installation"]["zones"].as_array().unwrap().iter().find(|z| z["zone_id"] == 4).cloned().unwrap();
            assert_eq!(z["setpoint"], 20.5);
            got_update = true;
            break;
        }
    }
    assert!(got_update, "websocket did not push the update");

    // Mode change requested on a slave zone is redirected to the master zone.
    let (s, v) = env
        .call("PATCH", &format!("/api/installations/{inst}/zones/1/2"), Some(&token), Some(json!({"mode":"cooling"})))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["mode"], "cooling");
    assert!(env.mock.lock().unwrap().iter().all(|z| z.mode == Some(2)));

    // Validation.
    let (s, _) = env
        .call("PATCH", &format!("/api/installations/{inst}/zones/1/4"), Some(&token), Some(json!({"setpoint":45})))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = env.call("PATCH", &format!("/api/installations/{inst}/zones/1/99"), Some(&token), Some(json!({"on":true}))).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // Zone metadata (group by floor, custom name).
    let (s, v) = env
        .call("PUT", &format!("/api/installations/{inst}/zones/1/4/meta"), Some(&token), Some(json!({"group":"Étage","display_name":"Chambre d'amis"})))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["group"], "Étage");
    assert_eq!(v["name"], "Chambre d'amis");
    assert_eq!(v["airzone_name"], "Chambre 2");

    // Batch (switch a whole floor off).
    let (s, v) = env
        .call(
            "POST",
            "/api/control/batch",
            Some(&token),
            Some(json!({"actions":[{"installation_id":inst,"system_id":1,"zone_id":3,"on":false},{"installation_id":inst,"system_id":1,"zone_id":4,"on":false}]})),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(v["results"].as_array().unwrap().iter().all(|r| r["ok"] == true));
    assert_eq!(env.mock_zone(3).on, Some(0));

    // History has been recorded.
    let (s, v) = env.call("GET", &format!("/api/installations/{inst}/zones/1/4/history?hours=1"), Some(&token), None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(!v.as_array().unwrap().is_empty());

    // --- Routines (local, no subscription) -------------------------------------------------
    let (s, v) = env
        .call(
            "POST",
            "/api/routines",
            Some(&token),
            Some(json!({
                "name": "Tout éteindre",
                "actions": (1..=5).map(|z| json!({"installation_id":inst,"system_id":1,"zone_id":z,"on":false})).collect::<Vec<_>>()
            })),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let routine = v["id"].as_i64().unwrap();

    let (s, v) = env.call("POST", &format!("/api/routines/{routine}/run"), Some(&token), None).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(env.mock.lock().unwrap().iter().all(|z| z.on == Some(0)));

    // A fixed time is refused: schedules are the Airzone ones.
    let (s, _) = env
        .call(
            "PUT",
            &format!("/api/routines/{routine}"),
            Some(&token),
            Some(json!({"name":"Matin","time":"06:45","actions":[{"installation_id":inst,"system_id":1,"zone_id":1,"on":true}]})),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // --- Activity feed -------------------------------------------------------------------
    let (s, v) = env.call("GET", "/api/events?limit=50", Some(&token), None).await;
    assert_eq!(s, StatusCode::OK);
    let kinds: Vec<&str> = v.as_array().unwrap().iter().map(|e| e["kind"].as_str().unwrap()).collect();
    assert!(kinds.contains(&"command"));
    assert!(kinds.contains(&"routine"));
    assert!(kinds.contains(&"connection"));

    // --- Users & permissions -----------------------------------------------------------------
    let (s, _) = env.call("POST", "/api/users", Some(&token), Some(json!({"username":"marie","password":"motdepasse2"}))).await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, v) = env.call("POST", "/api/auth/login", None, Some(json!({"username":"Marie","password":"motdepasse2","device_name":"iPhone de Marie"}))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let marie = v["access_token"].as_str().unwrap().to_string();
    let (s, _) = env.call("POST", "/api/installations", Some(&marie), Some(json!({"name":"X","host":"1.2.3.4"}))).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = env.call("PATCH", &format!("/api/installations/{inst}/zones/1/1"), Some(&marie), Some(json!({"on":true}))).await;
    assert_eq!(s, StatusCode::NOT_FOUND, "a new user sees no installation until one is given");
    let (_, users) = env.call("GET", "/api/users", Some(&token), None).await;
    let marie_id = users.as_array().unwrap().iter().find(|u| u["username"] == "marie").unwrap()["id"].as_i64().unwrap();
    let (s, _) = env
        .call("PUT", &format!("/api/users/{marie_id}"), Some(&token), Some(json!({"access":[{"installation_id": inst, "permission": "control"}]})))
        .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = env.call("PATCH", &format!("/api/installations/{inst}/zones/1/1"), Some(&marie), Some(json!({"on":true}))).await;
    assert_eq!(s, StatusCode::OK, "regular users drive the installations given to them");
    let (s, _) = env.call("POST", "/api/auth/login", None, Some(json!({"username":"marie","password":"wrong-password"}))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // --- Refresh tokens: rotation, grace window, revocation ---------------------------------
    let (s, v) = env.call("POST", "/api/auth/refresh", None, Some(json!({"refresh_token":refresh}))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let refresh2 = v["refresh_token"].as_str().unwrap().to_string();
    assert_ne!(refresh2, refresh);
    let token2 = v["access_token"].as_str().unwrap().to_string();
    let (s, _) = env.call("GET", "/api/me", Some(&token2), None).await;
    assert_eq!(s, StatusCode::OK);

    // A client that lost the response retries with the previous token: still accepted.
    let (s, v) = env.call("POST", "/api/auth/refresh", None, Some(json!({"refresh_token":refresh}))).await;
    assert_eq!(s, StatusCode::OK, "grace window: {v}");
    let refresh3 = v["refresh_token"].as_str().unwrap().to_string();

    let (s, _) = env.call("POST", "/api/auth/refresh", None, Some(json!({"refresh_token":"not-a-token"}))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // Devices list, then revoke this device: access token and refresh token stop working.
    let (s, v) = env.call("GET", "/api/sessions", Some(&token2), None).await;
    assert_eq!(s, StatusCode::OK);
    let current = v.as_array().unwrap().iter().find(|s| s["current"] == true).unwrap();
    assert_eq!(current["device_name"], "Test");
    let (s, _) = env.call("POST", "/api/auth/logout", None, Some(json!({"refresh_token":refresh3}))).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _) = env.call("GET", "/api/me", Some(&token2), None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "revoked device is cut off immediately");
    let (s, _) = env.call("POST", "/api/auth/refresh", None, Some(json!({"refresh_token":refresh3}))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn offline_installation_is_reported() {
    let env = start().await;
    let (_, v) = env.call("POST", "/api/setup", None, Some(json!({"username":"admin","password":"motdepasse1"}))).await;
    let token = v["access_token"].as_str().unwrap().to_string();
    // Nothing listens on port 9 → unreachable.
    let (s, v) = env.call("POST", "/api/installations", Some(&token), Some(json!({"name":"Absente","host":"127.0.0.1","port":9}))).await;
    assert_eq!(s, StatusCode::CREATED);
    let id = v["id"].as_i64().unwrap();
    assert!(v["connection"]["last_error"].is_string());
    let (_, v) = env.call("POST", &format!("/api/installations/{id}/refresh"), Some(&token), None).await;
    assert_eq!(v["connection"]["state"], "offline");
    let (s, _) = env.call("PATCH", &format!("/api/installations/{id}/zones/1/1"), Some(&token), Some(json!({"on":true}))).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}
