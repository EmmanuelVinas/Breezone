//! Regression tests for the code review of 2026-11-06: races on accounts and input checks.

use breezone_server::config::Config;
use reqwest::StatusCode;
use serde_json::{json, Value};

async fn start(name: &str) -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("breezone-review-{name}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let state = breezone_server::init_state(Config::for_tests(dir)).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = breezone_server::app(state);
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://127.0.0.1:{port}")
}

async fn post(http: &reqwest::Client, url: String, token: Option<&str>, body: Value) -> (StatusCode, Value) {
    let mut req = http.post(url).json(&body);
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let resp = req.send().await.unwrap();
    let s = resp.status();
    (s, resp.json().await.unwrap_or(Value::Null))
}

/// Two first-start setups at the same time: only one administrator may be created.
#[tokio::test]
async fn concurrent_setup_creates_one_admin() {
    let base = start("setup").await;
    let http = reqwest::Client::new();
    let calls = (0..8).map(|i| {
        let http = http.clone();
        let base = base.clone();
        async move { post(&http, format!("{base}/api/setup"), None, json!({"username": format!("admin{i}"), "password": "motdepasse1"})).await.0 }
    });
    let results = futures_util::future::join_all(calls).await;
    let ok = results.iter().filter(|s| **s == StatusCode::OK).count();
    assert_eq!(ok, 1, "{results:?}");
}

/// Parallel wrong passwords: the lockout must stop them after a handful.
#[tokio::test]
async fn concurrent_logins_are_throttled() {
    let base = start("throttle").await;
    let http = reqwest::Client::new();
    post(&http, format!("{base}/api/setup"), None, json!({"username":"admin","password":"motdepasse1"})).await;
    let calls = (0..40).map(|_| {
        let http = http.clone();
        let base = base.clone();
        async move { post(&http, format!("{base}/api/auth/login"), None, json!({"username":"admin","password":"wrong-password"})).await.0 }
    });
    let results = futures_util::future::join_all(calls).await;
    let tried = results.iter().filter(|s| **s == StatusCode::UNAUTHORIZED).count();
    assert!(tried <= 5, "{tried} passwords tried, at most 5 expected");
}

/// Two administrators demoting each other at the same time: one admin must remain.
#[tokio::test]
async fn concurrent_demotions_keep_an_admin() {
    let base = start("admins").await;
    let http = reqwest::Client::new();
    let (_, a) = post(&http, format!("{base}/api/setup"), None, json!({"username":"alice","password":"motdepasse1"})).await;
    let ta = a["access_token"].as_str().unwrap().to_string();
    let (_, b) = post(&http, format!("{base}/api/users"), Some(&ta), json!({"username":"bob","password":"motdepasse1","role":"admin"})).await;
    let (_, lb) = post(&http, format!("{base}/api/auth/login"), None, json!({"username":"bob","password":"motdepasse1"})).await;
    let tb = lb["access_token"].as_str().unwrap().to_string();
    let a_id = a["user"]["id"].as_i64().unwrap();
    let b_id = b["id"].as_i64().unwrap();
    for _ in 0..10 {
        let r1 = http.put(format!("{base}/api/users/{b_id}")).bearer_auth(&ta).json(&json!({"role":"user"})).send();
        let r2 = http.put(format!("{base}/api/users/{a_id}")).bearer_auth(&tb).json(&json!({"role":"user"})).send();
        let (r1, r2) = tokio::join!(r1, r2);
        let (r1, r2) = (r1.unwrap().status(), r2.unwrap().status());
        assert!(!(r1.is_success() && r2.is_success()), "both administrators were demoted");
        // Restore for the next round (whoever is still admin promotes the other).
        let _ = http.put(format!("{base}/api/users/{b_id}")).bearer_auth(&ta).json(&json!({"role":"admin"})).send().await;
        let _ = http.put(format!("{base}/api/users/{a_id}")).bearer_auth(&tb).json(&json!({"role":"admin"})).send().await;
    }
}

/// The test endpoint must reject the ports that saving rejects (no silent wrap-around).
#[tokio::test]
async fn test_installation_rejects_invalid_ports() {
    let base = start("port").await;
    let http = reqwest::Client::new();
    let (_, a) = post(&http, format!("{base}/api/setup"), None, json!({"username":"admin","password":"motdepasse1"})).await;
    let t = a["access_token"].as_str().unwrap();
    let (s, _) = post(&http, format!("{base}/api/installations/test"), Some(t), json!({"host":"127.0.0.1","port":70000})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}
