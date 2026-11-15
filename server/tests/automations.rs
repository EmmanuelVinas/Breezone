//! Automations: a rule "quand… si… alors…" created through the API, run by the engine.

use breezone_server::automations::{self, Engine};
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

#[tokio::test]
async fn a_rule_drives_a_zone_when_its_measure_crosses_the_threshold() {
    let maison: MockState = Arc::new(Mutex::new(mock::preset("maison")));
    let ws = serve(mock::router(maison.clone())).await;
    let dir = std::env::temp_dir().join(format!("breezone-automations-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let state = breezone_server::init_state(Config::for_tests(dir)).await.unwrap();
    let port = serve(breezone_server::app(state.clone())).await;
    let base = format!("http://127.0.0.1:{port}");
    let http = reqwest::Client::new();
    let v: Value = http.post(format!("{base}/api/setup")).json(&json!({"username":"admin","password":"motdepasse1"})).send().await.unwrap().json().await.unwrap();
    let token = v["access_token"].as_str().unwrap().to_string();
    let call = |m: Method, path: &str, body: Option<Value>| {
        let mut r = http.request(m, format!("{base}{path}")).bearer_auth(&token).header("Accept-Language", "fr");
        if let Some(b) = body {
            r = r.json(&b);
        }
        async move {
            let resp = r.send().await.unwrap();
            let s = resp.status();
            (s, serde_json::from_str::<Value>(&resp.text().await.unwrap()).unwrap_or(Value::Null))
        }
    };
    let (_, inst) = call(Method::POST, "/api/installations", Some(json!({"name":"Maison","host":"127.0.0.1","port":ws}))).await;
    let id = inst["id"].as_i64().unwrap();
    let (_, devices) = call(Method::GET, "/api/devices", None).await;
    let device = |zone: i64| devices.as_array().unwrap().iter().find(|d| d["reference"]["zone_id"] == zone).unwrap().clone();
    let (first, third) = (device(1), device(3));
    let temp = first["state"]["room_temp"].as_f64().unwrap();
    let is_on = |z: i64| maison.lock().unwrap().iter().find(|x| x.zone_id == z).unwrap().is_on();
    assert!(is_on(3));

    // When the first zone is warmer than 1° under what it is, all year: switch the third one off.
    let rule = json!({
        "name": "Trop chaud",
        "when": {"kind": "measure", "measure": {"kind": "device", "device_id": first["id"], "field": "room_temp"}, "op": "above", "value": temp - 1.0},
        "conditions": [{"kind": "months", "from": 1, "to": 12}],
        "actions": [{"kind": "device", "device_id": third["id"], "trait": "climate.zone", "action": "set", "params": {"on": false}}]
    });
    let mut unknown = rule.clone();
    unknown["actions"][0]["params"] = json!({"colour": "red"});
    let (s, _) = call(Method::POST, "/api/automations", Some(unknown)).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "a parameter the trait does not have");
    let (s, created) = call(Method::POST, "/api/automations", Some(rule.clone())).await;
    assert_eq!(s, StatusCode::CREATED, "{created}");
    let rule_id = created["id"].as_i64().unwrap();
    assert_eq!(created["summary"]["conditions"], json!(["de janvier à décembre"]));
    assert_eq!(created["summary"]["actions"][0], format!("{} : éteinte", third["name"].as_str().unwrap()));
    assert_eq!((&created["status"]["holds"], &created["status"]["conditions"]), (&json!(true), &json!([true])));

    // Switched off, it does nothing.
    let (s, _) = call(Method::PUT, &format!("/api/automations/{rule_id}/enabled"), Some(json!({"enabled": false}))).await;
    assert_eq!(s, StatusCode::OK);
    let mut engine = Engine::new(breezone_server::db::now());
    automations::tick(&state, &mut engine).await;
    assert!(is_on(3));
    call(Method::PUT, &format!("/api/automations/{rule_id}/enabled"), Some(json!({"enabled": true}))).await;
    automations::tick(&state, &mut engine).await;
    assert!(!is_on(3), "the rule ran");
    assert!(is_on(1), "other zones untouched");

    let (_, list) = call(Method::GET, "/api/automations", None).await;
    assert!(list[0]["last_run_at"].is_i64());
    assert!(list[0]["last_reason"].as_str().unwrap().contains("Température"), "{}", list[0]["last_reason"]);
    let (_, events) = call(Method::GET, "/api/events", None).await;
    let event = events.as_array().unwrap().iter().find(|e| e["kind"] == "automation" && e["actor"] == "Breezone").unwrap();
    assert!(event["message"].as_str().unwrap().starts_with("Automatisme « Trop chaud » : "), "{event}");

    // Once per episode: switched back on by hand, the zone stays on.
    call(Method::PATCH, &format!("/api/installations/{id}/zones/1/3"), Some(json!({"on": true}))).await;
    sqlx::query("UPDATE automations SET last_run_at = 0").execute(&state.db).await.unwrap();
    automations::tick(&state, &mut engine).await;
    assert!(is_on(3));

    // Run by hand, whatever the trigger says.
    let (s, run) = call(Method::POST, &format!("/api/automations/{rule_id}/run"), None).await;
    assert_eq!((s, &run["results"][0]["ok"]), (StatusCode::OK, &json!(true)));
    assert!(!is_on(3));

    // A user without access to the installation does not see the rule.
    let (s, _) = call(Method::POST, "/api/users", Some(json!({"username":"lea","password":"motdepasse2","role":"user"}))).await;
    assert_eq!(s, StatusCode::CREATED);
    let v: Value =
        http.post(format!("{base}/api/auth/login")).json(&json!({"username":"lea","password":"motdepasse2","device_name":"test"})).send().await.unwrap().json().await.unwrap();
    let lea = v["access_token"].as_str().unwrap().to_string();
    let seen: Value = http.get(format!("{base}/api/automations")).bearer_auth(&lea).send().await.unwrap().json().await.unwrap();
    assert_eq!(seen, json!([]));
    let s = http.post(format!("{base}/api/automations")).bearer_auth(&lea).json(&rule).send().await.unwrap().status();
    assert_eq!(s, StatusCode::NOT_FOUND, "the installation is not revealed");

    let (s, _) = call(Method::DELETE, &format!("/api/automations/{rule_id}"), None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, list) = call(Method::GET, "/api/automations", None).await;
    assert_eq!(list, json!([]));
}
