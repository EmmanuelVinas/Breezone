//! Solar surplus steering: a charger and a water heater follow what goes back to the grid, read on a
//! Shelly meter.

mod support;

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use reqwest::StatusCode;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

async fn serve(router: Router) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    port
}

/// A Shelly Pro 3EM at the meter (`grid` W: negative goes back to the grid) with one relay.
#[derive(Clone)]
struct Meter {
    grid: Arc<Mutex<f64>>,
    relay: Arc<Mutex<bool>>,
}

async fn fake_meter(m: Meter) -> u16 {
    let router = Router::new()
        .route("/rpc/Shelly.GetDeviceInfo", get(|| async { Json(json!({"id": "shellypro3em-surplus", "app": "Pro3EM", "ver": "1.4.4", "gen": 2})) }))
        .route(
            "/rpc/Shelly.GetStatus",
            get(|State(m): State<Meter>| async move {
                let grid = *m.grid.lock().unwrap();
                Json(json!({
                    "em:0": {"id": 0, "a_act_power": grid, "b_act_power": 0.0, "c_act_power": 0.0, "total_act_power": grid, "a_voltage": 230.0},
                    "emdata:0": {"id": 0, "total_act": 1000.0, "total_act_ret": 500.0},
                    "switch:0": {"id": 0, "output": *m.relay.lock().unwrap(), "apower": 0.0, "aenergy": {"total": 0.0}},
                    "sys": {"uptime": 10}
                }))
            }),
        )
        .route(
            "/rpc/Switch.Set",
            get(|State(m): State<Meter>, Query(q): Query<HashMap<String, String>>| async move {
                *m.relay.lock().unwrap() = q.get("on").map(String::as_str) == Some("true");
                Json(json!({"was_on": false}))
            }),
        )
        .with_state(m);
    serve(router).await
}

/// A V2C Trydan: charging while a car is plugged in and it is not paused.
async fn fake_trydan(data: Arc<Mutex<Value>>) -> u16 {
    let router = Router::new()
        .route(
            "/RealTimeData",
            get(|State(d): State<Arc<Mutex<Value>>>| async move {
                let mut d = d.lock().unwrap();
                let charging = d["ChargeState"] != 0 && d["Paused"] == 0;
                d["ChargeState"] = json!(if d["ChargeState"] == 0 {
                    0
                } else if charging {
                    2
                } else {
                    1
                });
                d["ChargePower"] = json!(if charging { d["Intensity"].as_f64().unwrap() * 230.0 } else { 0.0 });
                Json(d.clone())
            }),
        )
        .route(
            "/write/{kv}",
            get(|State(d): State<Arc<Mutex<Value>>>, Path(kv): Path<String>| async move {
                let (k, v) = kv.split_once('=').unwrap();
                d.lock().unwrap()[k] = json!(v.parse::<i64>().unwrap());
                "OK"
            }),
        )
        .with_state(data);
    serve(router).await
}

#[tokio::test]
async fn a_charger_and_a_heater_follow_the_surplus() {
    let meter = Meter { grid: Arc::new(Mutex::new(0.0)), relay: Arc::new(Mutex::new(false)) };
    let shelly = fake_meter(meter.clone()).await;
    // A car plugged in, the charger paused.
    let charger = Arc::new(Mutex::new(json!({"ID":"SURPLUS1","FirmwareVersion":"2.5.1","ChargeState":1,"ChargeMode":0,"ChargePower":0.0,"VoltageInstallation":230,
        "ChargeEnergy":0.0,"ChargeTime":0,"Paused":1,"Locked":0,"Intensity":6,"IntensityMeasure_L1":0.0,"Dynamic":1,"MinIntensity":6,"MaxIntensity":32})));
    let trydan = fake_trydan(charger.clone()).await;

    let dir = std::env::temp_dir().join(format!("breezone-surplus-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut cfg = support::config(&dir);
    cfg.scan_port = 9;
    let state = breezone_server::init_state(cfg).await.unwrap();
    let base = format!("http://127.0.0.1:{}", serve(breezone_server::app(state.clone())).await);
    let http = reqwest::Client::new();
    let v: Value = http.post(format!("{base}/api/setup")).json(&json!({"username":"admin","password":"motdepasse1"})).send().await.unwrap().json().await.unwrap();
    let token = v["access_token"].as_str().unwrap().to_string();
    let call = |m: &str, path: &str, body: Option<Value>| {
        let mut r = http.request(m.parse().unwrap(), format!("{base}{path}")).bearer_auth(&token).header("Accept-Language", "fr");
        if let Some(b) = body {
            r = r.json(&b);
        }
        async move {
            let resp = r.send().await.unwrap();
            let s = resp.status();
            (s, serde_json::from_str::<Value>(&resp.text().await.unwrap()).unwrap_or(Value::Null))
        }
    };
    for (driver, host) in [("shelly", shelly), ("trydan", trydan)] {
        let (s, src) = call("POST", "/api/sources", Some(json!({"driver": driver, "name": driver, "config": {"host": format!("127.0.0.1:{host}")}}))).await;
        assert_eq!(s, StatusCode::CREATED, "{src}");
    }
    let (_, devices) = call("GET", "/api/devices", None).await;
    let id = |reference: &str| devices.as_array().unwrap().iter().find(|d| d["reference"] == reference).unwrap()["id"].as_i64().unwrap();
    let (grid_id, relay_id, charger_id) = (id("em:0"), id("switch:0"), id("charger"));
    call("PUT", &format!("/api/devices/{grid_id}"), Some(json!({"role": "grid"}))).await;

    // The relay can be added to the list of the surplus; a charger follows it by its profile.
    let (_, view) = call("GET", "/api/surplus", None).await;
    let candidates: Vec<i64> = view["candidates"].as_array().unwrap().iter().map(|c| c["device_id"].as_i64().unwrap()).collect();
    assert_eq!(candidates, vec![relay_id], "{view}");
    let (s, _) = call("PUT", &format!("/api/surplus/loads/{relay_id}"), Some(json!({}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "a relay needs its power");
    let (s, _) = call("PUT", &format!("/api/surplus/loads/{charger_id}"), Some(json!({"max_amps": 16}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "a charger has a charging profile");
    call("PUT", &format!("/api/surplus/loads/{relay_id}"), Some(json!({"power": 2000}))).await;

    // A charger in the list from before the charging profiles (16 A at most, a guarantee): it
    // follows a "Solaire" profile with its currents, leaves the list, and its guarantee is told.
    let legacy = breezone_server::db::SurplusLoadRow {
        max_amps: Some(16),
        guarantee_kwh: Some(20.0),
        guarantee_by: Some("07:00".into()),
        position: -1,
        ..breezone_server::db::SurplusLoadRow::charger(charger_id)
    };
    breezone_server::db::upsert_surplus_load(&state.db, &legacy).await.unwrap();
    breezone_server::charging::adopt_surplus_chargers(&state).await.unwrap();
    let (_, view) = call("GET", "/api/surplus", None).await;
    let order: Vec<i64> = view["loads"].as_array().unwrap().iter().map(|l| l["device_id"].as_i64().unwrap()).collect();
    assert_eq!(order, vec![relay_id]);
    let (_, plan) = call("GET", &format!("/api/devices/{charger_id}/charging"), None).await;
    assert_eq!(plan["profiles"][0]["name"], "Solaire", "{plan}");
    assert_eq!(plan["profile_id"], plan["profiles"][0]["id"]);
    let (_, profiles) = call("GET", "/api/charging/profiles", None).await;
    assert_eq!(profiles[0]["profiles"][0]["slots"], json!([{"start": "00:00", "mode": "solar", "max_amps": 16}]));
    let (_, events) = call("GET", "/api/events", None).await;
    let said: Vec<&str> = events.as_array().unwrap().iter().filter_map(|e| e["message"].as_str()).collect();
    assert!(said.iter().any(|m| m.contains("garantie (+20 kWh avant 07:00) est à refaire")), "{said:?}");
    // Once: nothing more to move.
    breezone_server::charging::adopt_surplus_chargers(&state).await.unwrap();
    assert_eq!(call("GET", "/api/charging/profiles", None).await.1.as_array().unwrap().len(), 1);

    let look = |grid: f64, t: i64| {
        let (state, meter) = (state.clone(), meter.clone());
        async move {
            *meter.grid.lock().unwrap() = grid;
            // Read now, whatever the time their modules ask between two readings.
            for source in [1, 2] {
                state.drivers.poll(source).await;
            }
            breezone_server::surplus::tick(&state, t).await;
            for source in [1, 2] {
                state.drivers.poll(source).await;
            }
        }
    };
    // 2,3 kW go back to the grid: after a minute the charger charges at 10 A.
    let t0 = breezone_server::db::now();
    look(-2300.0, t0).await;
    assert_eq!(charger.lock().unwrap()["Paused"], 1, "a minute first");
    look(-2300.0, t0 + 60).await;
    {
        let c = charger.lock().unwrap();
        assert_eq!((c["Paused"].as_i64(), c["Intensity"].as_i64(), c["Dynamic"].as_i64()), (Some(0), Some(10), Some(0)), "{c}");
    }
    let (_, view) = call("GET", "/api/surplus", None).await;
    assert_eq!(view["export"], 2300.0);
    // The charger on its profile first, then the list.
    let (_, plan) = call("GET", &format!("/api/devices/{charger_id}/charging"), None).await;
    assert_eq!(plan["status"]["reason"], "Charge solaire à 10 A", "{plan}");
    assert_eq!(view["loads"][0]["status"]["text"], "Attend 2 kW de surplus", "{view}");

    // More sun: 4,5 kW still go back with the car at 10 A (6,8 kW in all): 16 A, then the heater.
    look(-4500.0, t0 + 90).await;
    assert_eq!(charger.lock().unwrap()["Intensity"], 16);
    look(-3120.0, t0 + 100).await;
    look(-3120.0, t0 + 160).await;
    assert!(*meter.relay.lock().unwrap(), "the heater is on");

    // Night: the home draws 1 kW, plus the charger (16 A) and the heater: everything stops after 3
    // minutes; the history tells it.
    look(1000.0 + 3680.0 + 2000.0, t0 + 200).await;
    look(1000.0 + 3680.0 + 2000.0, t0 + 400).await;
    assert_eq!(charger.lock().unwrap()["Paused"], 1);
    assert!(!*meter.relay.lock().unwrap());
    let (_, events) = call("GET", "/api/events", None).await;
    let said: Vec<&str> = events.as_array().unwrap().iter().filter_map(|e| e["message"].as_str()).collect();
    assert!(said.contains(&"Trydan : charge solaire à 10 A"), "{said:?}");
    assert!(said.contains(&"Trydan : charge suspendue, plus assez de surplus"), "{said:?}");

    // No longer steered: left as it is.
    let (s, _) = call("DELETE", &format!("/api/surplus/loads/{relay_id}"), None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, view) = call("GET", "/api/surplus", None).await;
    assert!(view["loads"].as_array().unwrap().is_empty());
}

/// A charger that follows a charging profile and is told to charge now, without a grid meter: a
/// profile and a charge asked now act all the same.
#[tokio::test]
async fn a_charger_follows_its_profile_and_charges_now() {
    // A car plugged in, charging as soon as it was.
    let charger = Arc::new(Mutex::new(json!({"ID":"PLAN1","FirmwareVersion":"2.5.1","ChargeState":2,"ChargeMode":0,"ChargePower":0.0,"VoltageInstallation":230,
        "ChargeEnergy":0.0,"ChargeTime":0,"Paused":0,"Locked":0,"Intensity":10,"IntensityMeasure_L1":0.0,"Dynamic":0,"MinIntensity":6,"MaxIntensity":32})));
    let trydan = fake_trydan(charger.clone()).await;
    let dir = std::env::temp_dir().join(format!("breezone-charging-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut cfg = support::config(&dir);
    cfg.scan_port = 9;
    let state = breezone_server::init_state(cfg).await.unwrap();
    let base = format!("http://127.0.0.1:{}", serve(breezone_server::app(state.clone())).await);
    let http = reqwest::Client::new();
    let login = |path: &'static str, body: Value| {
        let (http, base) = (http.clone(), base.clone());
        async move {
            let v: Value = http.post(format!("{base}{path}")).json(&body).send().await.unwrap().json().await.unwrap();
            v["access_token"].as_str().unwrap().to_string()
        }
    };
    let admin = login("/api/setup", json!({"username":"admin","password":"motdepasse1"})).await;
    let call = |token: &str, m: &str, path: &str, body: Option<Value>| {
        let mut r = http.request(m.parse().unwrap(), format!("{base}{path}")).bearer_auth(token).header("Accept-Language", "fr");
        if let Some(b) = body {
            r = r.json(&b);
        }
        async move {
            let resp = r.send().await.unwrap();
            let s = resp.status();
            (s, serde_json::from_str::<Value>(&resp.text().await.unwrap()).unwrap_or(Value::Null))
        }
    };
    let (s, src) = call(&admin, "POST", "/api/sources", Some(json!({"driver": "trydan", "name": "Trydan", "config": {"host": format!("127.0.0.1:{trydan}")}}))).await;
    assert_eq!(s, StatusCode::CREATED, "{src}");
    let (_, devices) = call(&admin, "GET", "/api/devices", None).await;
    let id = devices.as_array().unwrap().iter().find(|d| d["reference"] == "charger").unwrap()["id"].as_i64().unwrap();
    let look = |t: i64| {
        let state = state.clone();
        async move {
            state.drivers.poll(1).await;
            breezone_server::surplus::tick(&state, t).await;
            state.drivers.poll(1).await;
        }
    };

    // A profile that never charges.
    let nothing =
        |slots: Value| json!({"name": "Rien", "profiles": [{"id": "d", "name": "Tous les jours", "slots": slots}], "calendar": {"weekdays": ["d", "d", "d", "d", "d", "d", "d"]}});
    let (s, _) = call(&admin, "POST", "/api/charging/profiles", Some(nothing(json!([{"start": "01:00", "mode": "off"}])))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "the first slot starts at midnight");
    let (s, p) = call(&admin, "POST", "/api/charging/profiles", Some(nothing(json!([{"start": "00:00", "mode": "off"}])))).await;
    assert_eq!(s, StatusCode::CREATED, "{p}");
    let profile = p["id"].as_i64().unwrap();
    let path = format!("/api/devices/{id}/charging");
    let (s, _) = call(&admin, "PUT", &path, Some(json!({"profile_id": profile + 100}))).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, v) = call(&admin, "PUT", &path, Some(json!({"profile_id": profile}))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!((v["profile_id"].as_i64(), v["days"].as_array().unwrap().len(), &v["next"]), (Some(profile), 3, &Value::Null), "{v}");
    assert_eq!(v["days"][0]["periods"][0]["mode"], "off");
    let (_, list) = call(&admin, "GET", "/api/charging/profiles", None).await;
    assert_eq!(list[0]["used_by"][0]["device_id"], id, "{list}");

    // No charge: the charge that started when the car was plugged in is stopped.
    let t0 = breezone_server::db::now();
    look(t0).await;
    assert_eq!(charger.lock().unwrap()["Paused"], 1);
    let (_, v) = call(&admin, "GET", &path, None).await;
    assert_eq!((v["status"]["state"].as_str(), v["status"]["reason"].as_str()), (Some("no_charge"), Some("Profil Rien : pas de charge")), "{v}");

    // Charge now, 3.7 kW (16 A), 2 kWh more.
    let (s, v) = call(&admin, "POST", &format!("{path}/now"), Some(json!({"kw": 3.7, "kwh": 2.0}))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["now"]["amps"], 16);
    look(t0 + 10).await;
    {
        let c = charger.lock().unwrap();
        assert_eq!((c["Paused"].as_i64(), c["Intensity"].as_i64()), (Some(0), Some(16)), "{c}");
    }
    let (_, v) = call(&admin, "GET", &path, None).await;
    assert_eq!(v["status"]["reason"], "Charge maintenant à 3,7 kW, encore 2 kWh", "{v}");
    // The 2 kWh are in: back to the profile, which stops it once the charger has settled.
    charger.lock().unwrap()["ChargeEnergy"] = json!(2.1);
    look(t0 + 20).await;
    let (_, v) = call(&admin, "GET", &path, None).await;
    assert_eq!(v["now"], Value::Null, "{v}");
    look(t0 + 200).await;
    assert_eq!(charger.lock().unwrap()["Paused"], 1);
    let (_, events) = call(&admin, "GET", "/api/events", None).await;
    let said: Vec<&str> = events.as_array().unwrap().iter().filter_map(|e| e["message"].as_str()).collect();
    assert!(said.contains(&"Trydan : énergie ajoutée, fin de la charge immédiate"), "{said:?}");
    assert!(said.contains(&"Trydan : charge maintenant à 16 A"), "{said:?}");

    // Charge now until unplugged: unplugged, it is over.
    call(&admin, "POST", &format!("{path}/now"), Some(json!({}))).await;
    look(t0 + 210).await;
    {
        let c = charger.lock().unwrap();
        assert_eq!((c["Paused"].as_i64(), c["Intensity"].as_i64()), (Some(0), Some(32)), "the charger's maximum");
    }
    charger.lock().unwrap()["ChargeState"] = json!(0);
    look(t0 + 220).await;
    let (_, v) = call(&admin, "GET", &path, None).await;
    assert_eq!((&v["now"], v["status"]["state"].as_str()), (&Value::Null, Some("no_car")), "{v}");
    let (s, _) = call(&admin, "POST", &format!("{path}/now"), Some(json!({"kw": 7}))).await;
    assert_eq!(s, StatusCode::CONFLICT, "no car to charge");

    // Seen read-only: read, not changed; the profiles are the administrators'.
    let (s, _) =
        call(&admin, "POST", "/api/users", Some(json!({"username": "ana", "password": "motdepasse1", "role": "user", "devices": [{"scope": "all", "permission": "read"}]}))).await;
    assert_eq!(s, StatusCode::CREATED);
    let ana = login("/api/auth/login", json!({"username": "ana", "password": "motdepasse1"})).await;
    let (s, v) = call(&ana, "GET", &path, None).await;
    assert_eq!((s, v["can_control"].as_bool()), (StatusCode::OK, Some(false)));
    assert_eq!(call(&ana, "PUT", &path, Some(json!({"profile_id": null}))).await.0, StatusCode::FORBIDDEN);
    assert_eq!(call(&ana, "POST", "/api/charging/profiles", Some(nothing(json!([{"start": "00:00", "mode": "max"}])))).await.0, StatusCode::FORBIDDEN);

    // Deleted: the charger follows nothing.
    assert_eq!(call(&admin, "DELETE", &format!("/api/charging/profiles/{profile}"), None).await.0, StatusCode::NO_CONTENT);
    let (_, v) = call(&admin, "GET", &path, None).await;
    assert_eq!((&v["profile_id"], v["days"].as_array().unwrap().len()), (&Value::Null, 0));
}
