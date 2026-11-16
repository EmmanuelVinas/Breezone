//! Automations: a rule "quand… si… alors…" created through the API, run by the engine.

use breezone_server::automations::{self, Engine};
use breezone_server::config::Config;
use breezone_server::mock::{self, MockState};
use chrono::Datelike;
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

/// Waits for the runs of a rule (in the background) to end.
async fn settle(state: &breezone_server::AppState, id: i64) {
    for _ in 0..100 {
        if !state.automations.is_running(id) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("automation {id} still runs");
}

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
    assert_eq!((&created["match"], &created["summary"]["joined"]), (&json!("all"), &json!("et")));
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
    settle(&state, rule_id).await;
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

    // Wait until zone 1 is off, then put zone 4 back as it was.
    let (s, back) = call(
        Method::POST,
        "/api/automations",
        Some(json!({
            "name": "Fenêtre",
            "when": {"kind": "time", "time": "03:00"},
            "actions": [
                {"kind": "device", "device_id": device(4)["id"], "trait": "climate.zone", "action": "set", "params": {"on": false}},
                {"kind": "wait_until", "condition": {"kind": "state", "device_id": first["id"], "field": "on", "value": false}, "timeout_seconds": 60},
                {"kind": "restore"}
            ]
        })),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{back}");
    let back_id = back["id"].as_i64().unwrap();
    assert_eq!(back["summary"]["actions"][2], "remettre comme avant");
    for z in [4, 5] {
        call(Method::PATCH, &format!("/api/installations/{id}/zones/1/{z}"), Some(json!({"on": true}))).await;
    }
    assert!(is_on(4));
    let switch_off = {
        let (http, base, token) = (http.clone(), base.clone(), token.clone());
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            http.patch(format!("{base}/api/installations/{id}/zones/1/1")).bearer_auth(&token).json(&json!({"on": false})).send().await.unwrap();
        })
    };
    let (s, run) = call(Method::POST, &format!("/api/automations/{back_id}/run"), None).await;
    switch_off.await.unwrap();
    assert_eq!((s, &run["running"]), (StatusCode::OK, &json!(false)), "{run}");
    assert_eq!(run["results"].as_array().unwrap().len(), 4, "off, wait, put back: {run}");
    assert!(is_on(4), "put back as before");
    let (_, runs) = call(Method::GET, &format!("/api/automations/{back_id}/runs"), None).await;
    assert_eq!((&runs[0]["branch"], &runs[0]["ended"]), (&json!("then"), &json!("done")), "{runs}");
    assert!(runs[0]["steps"][3]["what"].as_str().unwrap().starts_with("remettre "), "{runs}");

    // "Else": the conditions are false when the time comes.
    let paris = chrono::Utc::now().with_timezone(&chrono_tz::Europe::Paris);
    let minute_ago = (paris - chrono::Duration::minutes(1)).format("%H:%M").to_string();
    let other_month = paris.month() % 12 + 1;
    let (s, otherwise) = call(
        Method::POST,
        "/api/automations",
        Some(json!({
            "name": "Sinon",
            "triggers": [{"kind": "time", "time": minute_ago}],
            "conditions": [{"kind": "months", "from": other_month, "to": other_month}],
            "actions": [{"kind": "scene", "routine_id": 0}],
            "else": [{"kind": "device", "device_id": device(5)["id"], "trait": "climate.zone", "action": "set", "params": {"on": false}}]
        })),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "unknown scene: {otherwise}");
    let (s, otherwise) = call(
        Method::POST,
        "/api/automations",
        Some(json!({
            "name": "Sinon",
            "triggers": [{"kind": "time", "time": minute_ago}],
            "conditions": [{"kind": "months", "from": other_month, "to": other_month}],
            "actions": [{"kind": "device", "device_id": device(5)["id"], "trait": "climate.zone", "action": "set", "params": {"setpoint": 25}}],
            "else": [{"kind": "device", "device_id": device(5)["id"], "trait": "climate.zone", "action": "set", "params": {"on": false}}]
        })),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{otherwise}");
    let otherwise_id = otherwise["id"].as_i64().unwrap();
    assert!(is_on(5));
    let mut engine = Engine::new(breezone_server::db::now() - 120);
    automations::tick(&state, &mut engine).await;
    settle(&state, otherwise_id).await;
    assert!(!is_on(5), "the else actions ran");
    let (_, runs) = call(Method::GET, &format!("/api/automations/{otherwise_id}/runs"), None).await;
    assert_eq!(runs[0]["branch"], "else", "{runs}");

    // Switched off while it waits: the run ends, and the history says so.
    let (_, waiting) =
        call(Method::POST, "/api/automations", Some(json!({"name": "Attente", "when": {"kind": "time", "time": "03:00"}, "actions": [{"kind": "wait", "seconds": 600}]}))).await;
    let waiting_id = waiting["id"].as_i64().unwrap();
    let row = breezone_server::db::automation_by_id(&state.db, waiting_id).await.unwrap().unwrap();
    let waits = automations::Automation::of(&row).unwrap();
    let ctx = automations::Context::load(&state).await.unwrap();
    automations::start(
        &state,
        &row,
        &waits,
        automations::Due { id: waiting_id, branch: automations::Branch::Then, reason: breezone_server::i18n::Text::plain("test"), lasting: None },
        ctx,
        "test",
    )
    .await
    .unwrap();
    assert!(state.automations.is_running(waiting_id));
    let (_, listed) = call(Method::GET, &format!("/api/automations/{waiting_id}"), None).await;
    assert_eq!(listed["status"]["running"], true);
    call(Method::PUT, &format!("/api/automations/{waiting_id}/enabled"), Some(json!({"enabled": false}))).await;
    assert!(!state.automations.is_running(waiting_id));
    let (_, runs) = call(Method::GET, &format!("/api/automations/{waiting_id}/runs"), None).await;
    assert_eq!(runs[0]["ended"], "cancelled", "{runs}");

    // Presence, a notification to ntfy, and the holiday pause.
    let received: Arc<Mutex<Vec<(String, String)>>> = Arc::default();
    let ntfy = {
        let received = received.clone();
        serve(axum::Router::new().route(
            "/breezone",
            axum::routing::post(move |query: axum::extract::Query<std::collections::HashMap<String, String>>, body: String| {
                let received = received.clone();
                async move {
                    let title = query.get("title").cloned().unwrap_or_default();
                    received.lock().unwrap().push((title, body));
                }
            }),
        ))
        .await
    };
    let (s, saved) = call(Method::PUT, "/api/automations/settings", Some(json!({"ntfy_url": format!("http://127.0.0.1:{ntfy}/breezone")}))).await;
    assert_eq!(s, StatusCode::OK, "{saved}");
    let (s, _) = call(Method::POST, "/api/automations/notify-test", None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    assert_eq!(received.lock().unwrap().len(), 1, "the test notification");
    let (_, presence) = call(Method::GET, "/api/presence", None).await;
    assert_eq!((&presence["me"], &presence["someone_home"]), (&Value::Null, &Value::Null), "nobody told yet");
    call(Method::PUT, "/api/presence", Some(json!({"home": true}))).await;
    let (s, empty) = call(
        Method::POST,
        "/api/automations",
        Some(json!({
            "name": "Maison vide",
            "triggers": [{"kind": "presence", "home": false}],
            "actions": [{"kind": "notify", "message": "Tout le monde est parti"}]
        })),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{empty}");
    let empty_id = empty["id"].as_i64().unwrap();
    let mut engine = Engine::new(breezone_server::db::now());
    automations::tick(&state, &mut engine).await;
    // On holiday: the last one leaves, nothing starts.
    let (_, paused) = call(Method::PUT, "/api/automations/settings", Some(json!({"paused_until": 0}))).await;
    assert_eq!(paused["paused_until"], 0);
    let (_, presence) = call(Method::PUT, "/api/presence", Some(json!({"home": false}))).await;
    assert_eq!(presence["someone_home"], false);
    automations::tick(&state, &mut engine).await;
    settle(&state, empty_id).await;
    assert_eq!(received.lock().unwrap().len(), 1, "paused");
    // Back from holiday, the next departure starts it.
    call(Method::PUT, "/api/automations/settings", Some(json!({"paused_until": null}))).await;
    call(Method::PUT, "/api/presence", Some(json!({"home": true}))).await;
    automations::tick(&state, &mut engine).await;
    call(Method::PUT, "/api/presence", Some(json!({"home": false}))).await;
    automations::tick(&state, &mut engine).await;
    settle(&state, empty_id).await;
    let got = received.lock().unwrap().clone();
    assert_eq!(got.len(), 2, "{got:?}");
    assert_eq!(got[1], ("Breezone · Maison vide".to_string(), "Tout le monde est parti".to_string()));
    let (_, events) = call(Method::GET, "/api/events", None).await;
    assert!(events.as_array().unwrap().iter().any(|e| e["kind"] == "notification" && e["message"] == "Maison vide : Tout le monde est parti"), "{events}");

    // A user without access to the installation does not see the rule.
    let (s, _) = call(Method::POST, "/api/users", Some(json!({"username":"lea","password":"motdepasse2","role":"user"}))).await;
    assert_eq!(s, StatusCode::CREATED);
    let v: Value =
        http.post(format!("{base}/api/auth/login")).json(&json!({"username":"lea","password":"motdepasse2","device_name":"test"})).send().await.unwrap().json().await.unwrap();
    let lea = v["access_token"].as_str().unwrap().to_string();
    let seen: Value = http.get(format!("{base}/api/automations")).bearer_auth(&lea).send().await.unwrap().json().await.unwrap();
    let ids: Vec<i64> = seen.as_array().unwrap().iter().map(|a| a["id"].as_i64().unwrap()).collect();
    assert!(![rule_id, back_id, otherwise_id].iter().any(|id| ids.contains(id)), "the rules of the installation are hidden: {ids:?}");
    let s = http.post(format!("{base}/api/automations")).bearer_auth(&lea).json(&rule).send().await.unwrap().status();
    assert_eq!(s, StatusCode::NOT_FOUND, "the installation is not revealed");

    let (s, _) = call(Method::DELETE, &format!("/api/automations/{rule_id}"), None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, list) = call(Method::GET, "/api/automations", None).await;
    assert!(list.as_array().unwrap().iter().all(|a| a["id"] != rule_id), "deleted");
}

#[tokio::test]
async fn while_a_trigger_holds_then_as_before() {
    let maison: MockState = Arc::new(Mutex::new(mock::preset("maison")));
    let ws = serve(mock::router(maison.clone())).await;
    let dir = std::env::temp_dir().join(format!("breezone-automations-over-{}", std::process::id()));
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
    let is_on = |z: i64| maison.lock().unwrap().iter().find(|x| x.zone_id == z).unwrap().is_on();
    for z in [1, 2] {
        call(Method::PATCH, &format!("/api/installations/{id}/zones/1/{z}"), Some(json!({"on": true}))).await;
    }

    // While zone 1 is off, zone 2 is off too; then it is put back.
    let switch_off_2 = json!([{"kind": "device", "device_id": device(2)["id"], "trait": "climate.zone", "action": "set", "params": {"on": false}}]);
    let (s, _) =
        call(Method::POST, "/api/automations", Some(json!({"name": "Heure", "when": {"kind": "time", "time": "03:00"}, "actions": switch_off_2, "over": [{"kind": "restore"}]})))
            .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "a time does not last");
    let (s, created) = call(
        Method::POST,
        "/api/automations",
        Some(json!({
            "name": "Ensemble",
            "when": {"kind": "state", "device_id": device(1)["id"], "field": "on", "value": false},
            "actions": switch_off_2,
            "over": [{"kind": "restore"}]
        })),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{created}");
    assert_eq!(created["summary"]["over"], json!(["remettre comme avant"]));
    let rule_id = created["id"].as_i64().unwrap();

    let mut engine = Engine::new(breezone_server::db::now());
    automations::tick(&state, &mut engine).await;
    call(Method::PATCH, &format!("/api/installations/{id}/zones/1/1"), Some(json!({"on": false}))).await;
    automations::tick(&state, &mut engine).await;
    for _ in 0..40 {
        if !is_on(2) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(!is_on(2), "the rule ran");
    assert!(state.automations.is_running(rule_id), "it waits for the end");

    call(Method::PATCH, &format!("/api/installations/{id}/zones/1/1"), Some(json!({"on": true}))).await;
    for _ in 0..300 {
        if !state.automations.is_running(rule_id) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(is_on(2), "put back once zone 1 is on again");
    let (_, runs) = call(Method::GET, &format!("/api/automations/{rule_id}/runs"), None).await;
    let steps = runs[0]["steps"].as_array().unwrap();
    assert_eq!(runs[0]["ended"], "done", "{runs}");
    assert!(steps[1]["what"].as_str().unwrap().starts_with("ce n'est plus vrai : "), "{runs}");
}
