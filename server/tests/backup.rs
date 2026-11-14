//! Export and import of the configuration: a server restored from its own file keeps its secrets
//! and history; a new server gets the configuration, never a secret.

use breezone_server::config::Config;
use reqwest::StatusCode;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};

struct Server {
    base: String,
    http: reqwest::Client,
    state: breezone_server::AppState,
    token: String,
}

impl Server {
    async fn start() -> Server {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!("breezone-backup-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&dir);
        let mut cfg = Config::for_tests(dir);
        cfg.scan_port = 9;
        let state = breezone_server::init_state(cfg).await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = breezone_server::app(state.clone());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let mut server = Server { base: format!("http://127.0.0.1:{port}"), http: reqwest::Client::new(), state, token: String::new() };
        let (_, v) = server.call("POST", "/api/setup", Some(json!({"username":"admin","password":"motdepasse1"}))).await;
        server.token = v["access_token"].as_str().unwrap().to_string();
        server
    }

    async fn call(&self, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut req = self.http.request(method.parse().unwrap(), format!("{}{path}", self.base));
        if !self.token.is_empty() {
            req = req.bearer_auth(&self.token);
        }
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await.unwrap();
        let status = resp.status();
        (status, serde_json::from_str(&resp.text().await.unwrap()).unwrap_or(Value::Null))
    }

    async fn sql(&self, q: &str) {
        sqlx::query(sqlx::AssertSqlSafe(q.to_string())).execute(&self.state.db).await.unwrap();
    }

    async fn one(&self, q: &str) -> Option<String> {
        sqlx::query_scalar::<_, Option<String>>(sqlx::AssertSqlSafe(q.to_string())).fetch_optional(&self.state.db).await.unwrap().flatten()
    }

    async fn count(&self, q: &str) -> i64 {
        sqlx::query_scalar(sqlx::AssertSqlSafe(q.to_string())).fetch_one(&self.state.db).await.unwrap()
    }
}

#[tokio::test]
async fn configuration_moves_without_secrets_and_restores_in_place() {
    let a = Server::start().await;
    a.sql("INSERT INTO installations (id, name, host, port, enabled, created_at) VALUES (7, 'Maison', '10.0.0.9', 3000, 1, 1)").await;
    a.sql("INSERT INTO zone_meta (installation_id, system_id, zone_id, display_name) VALUES (7, 1, 2, 'Salon')").await;
    a.sql("INSERT INTO readings (installation_id, system_id, zone_id, ts, room_temp) VALUES (7, 1, 2, 100, 21.5)").await;
    a.sql("INSERT INTO rooms (id, name, position, created_at) VALUES (3, 'Salon', 0, 1)").await;
    a.sql("INSERT INTO sources (id, driver, name, config, enabled, created_at, secret) VALUES (5, 'hue', 'Hue', '{\"host\":\"10.0.0.31\",\"device_id\":\"BRIDGE1\"}', 0, 1, 'S3CRET-KEY')").await;
    a.sql("INSERT INTO devices (id, source, source_ref, kind, name, room_id, created_at, source_id) VALUES (11, 'hue', '5:light:1', 'light', 'Lampe', 3, 1, 5)").await;
    a.sql("INSERT INTO device_samples (device_id, ts, field, value) VALUES (11, 100, 'power', 12.0)").await;
    a.sql("INSERT INTO routines (id, name, enabled, days, time, actions, created_at, updated_at) VALUES (4, 'Matin', 1, 31, '07:00', '[{\"installation_id\":7}]', 1, 1)").await;
    a.sql("INSERT INTO settings (key, value) VALUES ('cloud.email', 'moi@exemple.fr'), ('cloud.password', 'CLOUD-PASSWORD'), ('scan.subnet', '10.0.0.0/24')").await;
    let (s, alice) = a.call("POST", "/api/users", Some(json!({"username": "alice", "password": "motdepasse2", "role": "user"}))).await;
    assert!(s.is_success(), "{alice}");
    a.sql(&format!("INSERT OR REPLACE INTO user_installations (user_id, installation_id, permission) VALUES ({}, 7, 'control')", alice["id"])).await;
    let signing_key = a.one("SELECT value FROM settings WHERE key = 'node.signing_key'").await;

    let (s, file) = a.call("GET", "/api/system/config", None).await;
    assert_eq!(s, 200);
    let text = file.to_string();
    for secret in ["S3CRET-KEY", "CLOUD-PASSWORD", "password_hash", "$argon2"] {
        assert!(!text.contains(secret), "{secret} is in the file");
    }
    if let Some(k) = &signing_key {
        assert!(!text.contains(k.as_str()), "the identity of the server is in the file");
    }
    assert!(!text.contains("21.5") && !text.contains("device_samples"), "no history: {text}");
    assert_eq!(file["tables"]["installations"][0]["name"], "Maison");

    // A new server gets everything set up, nothing secret.
    let b = Server::start().await;
    let (s, done) = b.call("POST", "/api/system/config", Some(file.clone())).await;
    assert_eq!(s, 200, "{done}");
    assert_eq!(done["without_password"], json!(["alice"]));
    assert_eq!(done["to_pair"], json!(["Hue"]));
    assert_eq!(done["airzone_password"], true);
    assert_eq!(b.one("SELECT display_name FROM zone_meta WHERE installation_id = 7").await.as_deref(), Some("Salon"));
    assert_eq!(b.one("SELECT name FROM devices WHERE id = 11 AND room_id = 3 AND source_id = 5").await.as_deref(), Some("Lampe"));
    assert_eq!(b.one("SELECT actions FROM routines WHERE id = 4").await.as_deref(), Some("[{\"installation_id\":7}]"));
    assert_eq!(b.one("SELECT secret FROM sources WHERE id = 5").await, None);
    assert_eq!(b.one("SELECT value FROM settings WHERE key = 'cloud.email'").await.as_deref(), Some("moi@exemple.fr"));
    assert_eq!(b.one("SELECT value FROM settings WHERE key = 'cloud.password'").await, None);
    assert_eq!(b.count("SELECT COUNT(*) FROM user_installations ui JOIN users u ON u.id = ui.user_id WHERE u.username = 'alice' AND ui.installation_id = 7").await, 1);
    let (s, _) = b.call("POST", "/api/auth/login", Some(json!({"username": "alice", "password": "motdepasse2", "device_name": "test"}))).await;
    assert_ne!(s, 200, "no password came with the file");
    assert_eq!(b.one("SELECT role FROM users WHERE username = 'admin'").await.as_deref(), Some("admin"));

    // The same server restored from its file: secrets, passwords and history stay.
    a.sql("UPDATE zone_meta SET display_name = 'Changé'").await;
    let (s, done) = a.call("POST", "/api/system/config", Some(file)).await;
    assert_eq!(s, 200, "{done}");
    assert_eq!(done["to_pair"], json!([]));
    assert_eq!(done["airzone_password"], false);
    assert_eq!(a.one("SELECT display_name FROM zone_meta").await.as_deref(), Some("Salon"));
    assert_eq!(a.one("SELECT secret FROM sources WHERE id = 5").await.as_deref(), Some("S3CRET-KEY"));
    assert_eq!(a.one("SELECT value FROM settings WHERE key = 'cloud.password'").await.as_deref(), Some("CLOUD-PASSWORD"));
    assert_eq!(a.count("SELECT COUNT(*) FROM readings WHERE installation_id = 7").await, 1);
    assert_eq!(a.count("SELECT COUNT(*) FROM device_samples WHERE device_id = 11").await, 1);
    assert_eq!(a.one("SELECT value FROM settings WHERE key = 'node.signing_key'").await, signing_key);
    let (s, _) = a.call("POST", "/api/auth/login", Some(json!({"username": "alice", "password": "motdepasse2", "device_name": "test"}))).await;
    assert_eq!(s, 200);
}

#[tokio::test]
async fn refuses_what_is_not_a_configuration() {
    let a = Server::start().await;
    a.sql("INSERT INTO installations (id, name, host, port, enabled, created_at) VALUES (1, 'Maison', '10.0.0.9', 3000, 1, 1)").await;
    let (s, _) = a.call("POST", "/api/system/config", Some(json!({"hello": "world"}))).await;
    assert_eq!(s, 400);
    let (s, _) = a.call("POST", "/api/system/config", Some(json!({"format": "breezone-config", "version": 1, "schema": 9999, "tables": {}}))).await;
    assert_eq!(s, 400, "from a newer Breezone");
    // A broken file changes nothing.
    let (s, _) = a.call("POST", "/api/system/config", Some(json!({"format": "breezone-config", "version": 1, "schema": 1, "tables": {"devices": [{"id": 1, "source": "x", "source_ref": "y", "kind": "light", "created_at": 1, "room_id": 99}]}}))).await;
    assert_eq!(s, 400);
    assert_eq!(a.count("SELECT COUNT(*) FROM installations").await, 1);
    let (s, _) = a.call("POST", "/api/system/config", None).await;
    assert!(s.is_client_error());
}
