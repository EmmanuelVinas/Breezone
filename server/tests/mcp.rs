//! The access tokens of the users and the MCP server that assistants reach with them.

mod support;

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

async fn call(http: &reqwest::Client, base: &str, token: &str, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut req = http.request(method, format!("{base}{path}")).bearer_auth(token);
    if let Some(b) = body {
        req = req.json(&b);
    }
    let resp = req.send().await.unwrap();
    let status = resp.status();
    (status, serde_json::from_str(&resp.text().await.unwrap()).unwrap_or(Value::Null))
}

async fn rpc(http: &reqwest::Client, base: &str, token: &str, method: &str, params: Value) -> Value {
    let (status, v) = call(http, base, token, Method::POST, "/mcp", Some(json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    v
}

/// The text a tool answered, and whether it is an error.
async fn tool(http: &reqwest::Client, base: &str, token: &str, name: &str, arguments: Value) -> (String, bool) {
    let v = rpc(http, base, token, "tools/call", json!({"name": name, "arguments": arguments})).await;
    let r = &v["result"];
    (r["content"][0]["text"].as_str().unwrap_or_else(|| panic!("{v}")).to_string(), r["isError"].as_bool().unwrap())
}

#[tokio::test]
async fn an_assistant_drives_breezone_with_an_access_token() {
    let dir = std::env::temp_dir().join(format!("breezone-mcp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut cfg = support::config(&dir);
    cfg.scan_port = 9;
    let state = breezone_server::init_state(cfg).await.unwrap();
    let base = format!("http://127.0.0.1:{}", serve(breezone_server::app(state)).await);
    let http = reqwest::Client::new();
    let login: Value = http.post(format!("{base}/api/setup")).json(&json!({"username": "admin", "password": "motdepasse1"})).send().await.unwrap().json().await.unwrap();
    let admin = login["access_token"].as_str().unwrap().to_string();

    let ws = serve(mock::router(Arc::new(Mutex::new(mock::preset("maison"))) as MockState)).await;
    let (status, v) = call(&http, &base, &admin, Method::POST, "/api/installations", Some(json!({"name": "Maison", "host": "127.0.0.1", "port": ws}))).await;
    assert_eq!(status, StatusCode::CREATED, "{v}");

    // Without a token, the assistant is told how to sign in.
    let resp = http.post(format!("{base}/mcp")).json(&json!({"jsonrpc": "2.0", "id": 1, "method": "ping"})).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert!(resp.headers().get("www-authenticate").is_some());

    let (status, created) = call(&http, &base, &admin, Method::POST, "/api/tokens", Some(json!({"name": "Claude"}))).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let token = created["token"].as_str().unwrap().to_string();
    assert!(token.starts_with("bzt_") && created["hint"].as_str().unwrap().starts_with("bzt_"));
    assert!(created.get("token_hash").is_none(), "the hash stays on the server");
    let (_, list) = call(&http, &base, &admin, Method::GET, "/api/tokens", None).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert!(list[0].get("token").is_none(), "a token is shown once only");

    // A token acts as its user on the API, but cannot make another one.
    let (status, _) = call(&http, &base, &token, Method::GET, "/api/devices", None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(&http, &base, &token, Method::POST, "/api/tokens", Some(json!({"name": "more"}))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // The protocol.
    let v = rpc(&http, &base, &token, "initialize", json!({"protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "test", "version": "1"}})).await;
    assert_eq!(v["result"]["protocolVersion"], "2025-03-26");
    assert_eq!(v["result"]["serverInfo"]["name"], "breezone");
    let resp = http.post(format!("{base}/mcp")).bearer_auth(&token).json(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    let v = rpc(&http, &base, &token, "tools/list", json!({})).await;
    let names: Vec<&str> = v["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    for n in ["get_status", "create_automation", "install_module", "api_reference", "api_request"] {
        assert!(names.contains(&n), "{n} in {names:?}");
    }
    let v = rpc(&http, &base, &token, "nope", json!({})).await;
    assert_eq!(v["error"]["code"], -32601);

    // The tools go through the API, with its checks.
    let (text, err) = tool(&http, &base, &token, "list_rooms", json!({})).await;
    assert!(!err && text.trim() == "[]", "{text}");
    let (text, err) =
        tool(&http, &base, &token, "create_timer", json!({"timer": {"delay_minutes": 60, "target": {"kind": "all"}, "command": {"on": false}, "label": "Nuit"}})).await;
    assert!(!err, "{text}");
    let (text, _) = tool(&http, &base, &token, "list_timers", json!({})).await;
    assert!(text.contains("Nuit"), "{text}");
    let (text, err) = tool(&http, &base, &token, "create_automation", json!({"automation": {"name": "x"}})).await;
    assert!(err && text.starts_with("HTTP 4"), "the API refuses it: {text}");
    let (text, err) = tool(&http, &base, &token, "control_zone", json!({"installation_id": "un"})).await;
    assert!(err && text.contains("installation_id"), "{text}");
    let (text, err) = tool(&http, &base, &token, "api_request", json!({"method": "GET", "path": "/api/users"})).await;
    assert!(err && text.contains("not reachable"), "{text}");
    let (text, err) = tool(&http, &base, &token, "api_request", json!({"method": "GET", "path": "/api/energy/live"})).await;
    assert!(!err, "{text}");
    let (text, err) = tool(&http, &base, &token, "api_reference", json!({"section": "automatismes"})).await;
    assert!(!err && text.contains("AutomationInput"), "{text}");

    // A user only gets what they may do.
    call(&http, &base, &admin, Method::POST, "/api/users", Some(json!({"username": "camille", "password": "motdepasse1", "role": "user"}))).await;
    let v: Value = http.post(format!("{base}/api/auth/login")).json(&json!({"username": "camille", "password": "motdepasse1"})).send().await.unwrap().json().await.unwrap();
    let camille = v["access_token"].as_str().unwrap().to_string();
    let (_, created) = call(&http, &base, &camille, Method::POST, "/api/tokens", Some(json!({"name": "Script"}))).await;
    let theirs = created["token"].as_str().unwrap().to_string();
    let (text, err) = tool(&http, &base, &theirs, "install_module", json!({"id": "shelly"})).await;
    assert!(err && text.starts_with("HTTP 403"), "{text}");
    let (status, _) = call(&http, &base, &admin, Method::DELETE, &format!("/api/tokens/{}", created["id"]), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "only its owner revokes a token");

    // Revoked, or the password changed: cut off at once.
    let (status, _) = call(&http, &base, &camille, Method::PUT, "/api/me/password", Some(json!({"current_password": "motdepasse1", "new_password": "motdepasse2"}))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(&http, &base, &theirs, Method::GET, "/api/devices", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "a new password removes the tokens");
    let (status, _) = call(&http, &base, &admin, Method::DELETE, &format!("/api/tokens/{}", list[0]["id"]), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(&http, &base, &token, Method::POST, "/mcp", Some(json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
