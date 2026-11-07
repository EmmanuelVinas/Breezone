//! A user (role "user") only sees the installations given to them, read-only or with control.

use breezone_server::config::Config;
use breezone_server::mock::{self, MockState};
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

async fn serve(router: axum::Router) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    port
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

fn ids(v: &Value) -> Vec<i64> {
    v.as_array().unwrap().iter().map(|i| i["id"].as_i64().unwrap()).collect()
}

#[tokio::test]
async fn users_only_see_and_drive_the_installations_given_to_them() {
    let dir = std::env::temp_dir().join(format!("breezone-access-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut cfg = Config::for_tests(dir);
    cfg.scan_port = 9;
    let state = breezone_server::init_state(cfg).await.unwrap();
    let base = format!("http://127.0.0.1:{}", serve(breezone_server::app(state)).await);
    let http = reqwest::Client::new();
    let admin = Client { token: token(&http, &base, "/api/setup", "admin").await, base: base.clone(), http: http.clone() };

    let mut inst = vec![];
    for (name, mac) in [("Maison", mock::MOCK_MAC), ("Bureau", "AA:BB:CC:DD:EE:FF")] {
        let ws = serve(mock::router_with_mac(Arc::new(Mutex::new(mock::preset("maison"))) as MockState, mac)).await;
        let (s, v) = admin.call(Method::POST, "/api/installations", Some(json!({"name": name, "host": "127.0.0.1", "port": ws}))).await;
        assert_eq!(s, StatusCode::CREATED, "{v}");
        inst.push(v["id"].as_i64().unwrap());
    }
    let (home, office) = (inst[0], inst[1]);

    // A user who can only look at the home installation.
    let (s, v) = admin
        .call(Method::POST, "/api/users", Some(json!({"username":"camille","password":"motdepasse1","role":"user","access":[{"installation_id": home, "permission":"read"}]})))
        .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    assert_eq!(v["access"][0]["permission"], "read");
    let user_id = v["id"].as_i64().unwrap();
    let user = Client { token: token(&http, &base, "/api/auth/login", "camille").await, base: base.clone(), http: http.clone() };

    let (_, me) = user.call(Method::GET, "/api/me", None).await;
    assert_eq!(me["access"][0]["installation_id"], home, "{me}");
    let (_, me) = admin.call(Method::GET, "/api/me", None).await;
    assert_eq!(me["access"], Value::Null, "administrators see everything");

    let (_, status) = user.call(Method::GET, "/api/status", None).await;
    assert_eq!(ids(&status), vec![home]);
    assert_eq!(status[0]["controllable"], false, "read-only for this user");
    let (_, list) = user.call(Method::GET, "/api/installations", None).await;
    assert_eq!(ids(&list), vec![home]);
    let (s, _) = user.call(Method::GET, &format!("/api/installations/{office}"), None).await;
    assert_eq!(s, StatusCode::NOT_FOUND, "another installation is not revealed");
    let (s, _) = user.call(Method::GET, &format!("/api/installations/{office}/zones/1/1/history"), None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = user.call(Method::GET, &format!("/api/installations/{home}/zones/1/1/history"), None).await;
    assert_eq!(s, StatusCode::OK);

    // Read only: no command, no batch, no timer.
    let (s, v) = user.call(Method::PATCH, &format!("/api/installations/{home}/zones/1/1"), Some(json!({"on": false}))).await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{v}");
    let (s, _) = user.call(Method::POST, "/api/control/batch", Some(json!({"actions":[{"installation_id": home, "system_id":1, "zone_id":1, "on": false}]}))).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = user
        .call(Method::POST, "/api/timers", Some(json!({"delay_minutes": 60, "target": {"kind":"installation","installation_id": home}, "command": {"on": false}})))
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // Events: only those of the installations the user sees (no system events).
    let (_, events) = user.call(Method::GET, "/api/events?limit=100", None).await;
    assert!(events.as_array().unwrap().iter().all(|e| e["installation_id"] == home), "{events}");

    // Control given: commands work; "all installations" stays reserved to administrators.
    let (s, v) = admin.call(Method::PUT, &format!("/api/users/{user_id}"), Some(json!({"access":[{"installation_id": home, "permission":"control"}]}))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, v) = user.call(Method::PATCH, &format!("/api/installations/{home}/zones/1/1"), Some(json!({"on": false}))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, _) = user.call(Method::POST, "/api/timers", Some(json!({"delay_minutes": 60, "target": {"kind":"all"}, "command": {"on": false}}))).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = user
        .call(Method::POST, "/api/timers", Some(json!({"delay_minutes": 60, "target": {"kind":"installation","installation_id": home}, "command": {"on": false}})))
        .await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, _) = user
        .call(Method::POST, "/api/routines", Some(json!({"name":"Bureau","actions":[{"installation_id": office, "system_id":1, "zone_id":1, "on": false}]})))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "a routine cannot drive an installation the user does not see");

    // Routines and timers of other installations stay hidden.
    let (s, _) = admin
        .call(Method::POST, "/api/routines", Some(json!({"name":"Bureau","actions":[{"installation_id": office, "system_id":1, "zone_id":1, "on": false}]})))
        .await;
    assert_eq!(s, StatusCode::CREATED);
    let (_, routines) = user.call(Method::GET, "/api/routines", None).await;
    assert!(routines.as_array().unwrap().is_empty(), "{routines}");
    let (_, routines) = admin.call(Method::GET, "/api/routines", None).await;
    assert_eq!(routines.as_array().unwrap().len(), 1);

    // A new installation is given to nobody.
    let ws = serve(mock::router_with_mac(Arc::new(Mutex::new(mock::preset("bureau"))) as MockState, "11:22:33:44:55:66")).await;
    let (_, v) = admin.call(Method::POST, "/api/installations", Some(json!({"name":"Atelier","host":"127.0.0.1","port": ws}))).await;
    let atelier = v["id"].as_i64().unwrap();
    let (_, status) = user.call(Method::GET, "/api/status", None).await;
    assert_eq!(ids(&status), vec![home]);
    let (_, status) = admin.call(Method::GET, "/api/status", None).await;
    assert!(ids(&status).contains(&atelier));

    // Unknown installations and permissions are refused.
    let (s, _) = admin.call(Method::PUT, &format!("/api/users/{user_id}"), Some(json!({"access":[{"installation_id": 999, "permission":"read"}]}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = admin.call(Method::PUT, &format!("/api/users/{user_id}"), Some(json!({"access":[{"installation_id": home, "permission":"admin"}]}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}
