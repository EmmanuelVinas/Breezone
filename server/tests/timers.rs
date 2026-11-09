//! Timers ("dans 2 h, éteindre l'étage") and delayed actions of routines.

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
async fn timers_switch_a_group_off_later() {
    let maison: MockState = Arc::new(Mutex::new(mock::preset("maison")));
    let ws = serve(mock::router(maison.clone())).await;
    let dir = std::env::temp_dir().join(format!("breezone-timers-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let state = breezone_server::init_state(Config::for_tests(dir)).await.unwrap();
    let port = serve(breezone_server::app(state.clone())).await;
    let base = format!("http://127.0.0.1:{port}");
    let http = reqwest::Client::new();
    let v: Value = http.post(format!("{base}/api/setup")).json(&json!({"username":"admin","password":"motdepasse1"})).send().await.unwrap().json().await.unwrap();
    let token = v["access_token"].as_str().unwrap().to_string();
    let call = |m: Method, path: &str, body: Option<Value>| {
        let mut r = http.request(m, format!("{base}{path}")).bearer_auth(&token);
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
    // Group "Etage" = zones 3 and 4.
    for z in [3, 4] {
        call(Method::PUT, &format!("/api/installations/{id}/zones/1/{z}/meta"), Some(json!({"group":"Etage"}))).await;
    }
    let is_on = |z: i64| maison.lock().unwrap().iter().find(|x| x.zone_id == z).unwrap().is_on();
    assert!(is_on(3));

    // Invalid: no delay, empty command.
    let (s, _) = call(Method::POST, "/api/timers", Some(json!({"delay_minutes":0,"target":{"kind":"all"},"command":{"on":false}}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, t) = call(
        Method::POST,
        "/api/timers",
        Some(json!({"delay_minutes":120,"target":{"kind":"group","installation_id":id,"group":"Etage"},"command":{"on":false}})),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{t}");
    assert_eq!(t["label"], "Switch off · group Etage (Maison)");
    let (_, list) = call(Method::GET, "/api/timers", None).await;
    assert_eq!(list.as_array().unwrap().len(), 1);

    // Time passes.
    sqlx::query("UPDATE timers SET run_at = 0").execute(&state.db).await.unwrap();
    breezone_server::timers::run_due(&state.hub, &state.db).await;
    assert!(!is_on(3) && !is_on(4), "the group is off");
    assert!(is_on(1), "other zones untouched");
    let (_, list) = call(Method::GET, "/api/timers", None).await;
    assert_eq!(list, json!([]), "a timer runs once");

    // Mode + sleep on the whole installation: the mode once per system, but the sleep timer on
    // every zone (code review 2026-11-06).
    let (s, _) = call(Method::POST, "/api/timers", Some(json!({"delay_minutes":5,"target":{"kind":"installation","installation_id":id},"command":{"mode":"heating","sleep":30}}))).await;
    assert_eq!(s, StatusCode::CREATED);
    sqlx::query("UPDATE timers SET run_at = 0").execute(&state.db).await.unwrap();
    breezone_server::timers::run_due(&state.hub, &state.db).await;
    let sleeps: Vec<Option<i64>> = maison.lock().unwrap().iter().map(|z| z.sleep).collect();
    assert_eq!(sleeps, vec![Some(30); 5], "sleep on every zone");

    // Cancel.
    let (_, t) = call(Method::POST, "/api/timers", Some(json!({"delay_minutes":30,"target":{"kind":"installation","installation_id":id},"command":{"on":false}}))).await;
    let (s, _) = call(Method::DELETE, &format!("/api/timers/{}", t["id"]), None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);

    // A routine: Salon off now, Bureau off in 2 h.
    let (s, r) = call(
        Method::POST,
        "/api/routines",
        Some(json!({"name":"Je pars","actions":[
            {"installation_id":id,"system_id":1,"zone_id":1,"on":false},
            {"installation_id":id,"system_id":1,"zone_id":2,"on":false,"delay_minutes":120}]})),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{r}");
    assert_eq!(r["actions"][1]["delay_minutes"], 120);
    let (s, _) = call(Method::POST, &format!("/api/routines/{}/run", r["id"]), None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(!is_on(1));
    assert!(is_on(2), "delayed");
    let (_, list) = call(Method::GET, "/api/timers", None).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    sqlx::query("UPDATE timers SET run_at = 0").execute(&state.db).await.unwrap();
    breezone_server::timers::run_due(&state.hub, &state.db).await;
    assert!(!is_on(2));
}
