//! A poll that started before a command must not bring back the previous value (the "setpoint
//! jumps back, then forward" glitch).

use breezone_server::config::Config;
use breezone_server::mock::{self, MockState};
use reqwest::StatusCode;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[tokio::test]
async fn a_poll_in_flight_does_not_undo_a_command() {
    // Fake WebServer whose reads (POST /api/v1/hvac) are slow; writes (PUT) are immediate.
    let state: MockState = Arc::new(Mutex::new(mock::preset("maison")));
    let slow_reads = axum::middleware::from_fn(|req: axum::extract::Request, next: axum::middleware::Next| async move {
        // The state is read first, the answer arrives later: like a slow WebServer.
        let slow = req.method() == axum::http::Method::POST;
        let response = next.run(req).await;
        if slow {
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
        response
    });
    let router = mock::router(state.clone()).layer(slow_reads);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ws = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    let dir = std::env::temp_dir().join(format!("breezone-race-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let app_state = breezone_server::init_state(Config::for_tests(dir)).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let app = breezone_server::app(app_state);
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let http = reqwest::Client::new();
    let v: Value = http.post(format!("{base}/api/setup")).json(&json!({"username":"admin","password":"motdepasse1"})).send().await.unwrap().json().await.unwrap();
    let token = v["access_token"].as_str().unwrap().to_string();
    let inst: Value =
        http.post(format!("{base}/api/installations")).bearer_auth(&token).json(&json!({"name":"Maison","host":"127.0.0.1","port":ws})).send().await.unwrap().json().await.unwrap();
    let id = inst["id"].as_i64().unwrap();
    let before = inst["zones"].as_array().unwrap().iter().find(|z| z["zone_id"] == 2).unwrap()["setpoint"].as_f64().unwrap();
    assert_ne!(before, 22.5);

    // A poll starts (it reads the old setpoint), then a command changes the setpoint meanwhile.
    let poll = {
        let (http, base, token) = (http.clone(), base.clone(), token.clone());
        tokio::spawn(async move { http.post(format!("{base}/api/installations/{id}/refresh")).bearer_auth(&token).send().await.unwrap() })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    let r = http.patch(format!("{base}/api/installations/{id}/zones/1/2")).bearer_auth(&token).json(&json!({"setpoint": 22.5})).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    poll.await.unwrap();

    let inst: Value = http.get(format!("{base}/api/installations/{id}")).bearer_auth(&token).send().await.unwrap().json().await.unwrap();
    let after = inst["zones"].as_array().unwrap().iter().find(|z| z["zone_id"] == 2).unwrap()["setpoint"].as_f64().unwrap();
    assert_eq!(after, 22.5, "the poll started before the command brought back {before}");
}
