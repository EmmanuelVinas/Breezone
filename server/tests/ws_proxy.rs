//! A reverse proxy that drops the WebSocket headers is explained, not silent.

use breezone_server::config::Config;
use serde_json::{json, Value};

#[tokio::test]
async fn a_websocket_request_without_its_headers_is_explained() {
    let dir = std::env::temp_dir().join(format!("breezone-wsproxy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let state = breezone_server::init_state(Config::for_tests(dir)).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, breezone_server::app(state).into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap() });
    let http = reqwest::Client::new();
    let v: Value = http.post(format!("{base}/api/setup")).json(&json!({"username":"admin","password":"motdepasse1"})).send().await.unwrap().json().await.unwrap();
    let token = v["access_token"].as_str().unwrap();

    // What a proxy without the WebSocket headers forwards: a plain GET.
    let resp = http.get(format!("{base}/api/ws")).send().await.unwrap();
    assert_eq!(resp.status(), 400);
    let body: Value = resp.json().await.unwrap();
    assert!(body["error"].as_str().unwrap().contains("en-têtes WebSocket"), "{body}");

    let events: Value = http.get(format!("{base}/api/events?limit=10")).bearer_auth(token).send().await.unwrap().json().await.unwrap();
    assert!(events.as_array().unwrap().iter().any(|e| e["message"].as_str().unwrap_or_default().contains("Temps réel indisponible")), "{events}");
}
