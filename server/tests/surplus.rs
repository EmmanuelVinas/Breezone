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

    // Nothing steered yet: the charger and the relay can be added.
    let (_, view) = call("GET", "/api/surplus", None).await;
    let candidates: Vec<i64> = view["candidates"].as_array().unwrap().iter().map(|c| c["device_id"].as_i64().unwrap()).collect();
    assert!(candidates.contains(&charger_id) && candidates.contains(&relay_id), "{view}");
    let (s, _) = call("PUT", &format!("/api/surplus/loads/{relay_id}"), Some(json!({}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "a relay needs its power");
    let (s, _) = call("PUT", &format!("/api/surplus/loads/{charger_id}"), Some(json!({"max_amps": 40}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "above the charger's maximum");
    call("PUT", &format!("/api/surplus/loads/{charger_id}"), Some(json!({"max_amps": 16}))).await;
    let (_, view) = call("PUT", &format!("/api/surplus/loads/{relay_id}"), Some(json!({"power": 2000}))).await;
    let order: Vec<i64> = view["loads"].as_array().unwrap().iter().map(|l| l["device_id"].as_i64().unwrap()).collect();
    assert_eq!(order, vec![charger_id, relay_id], "added last in priority");

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
    assert_eq!(view["loads"][0]["status"]["text"], "Charge solaire à 10 A", "{view}");
    assert_eq!(view["loads"][1]["status"]["text"], "Attend 2 kW de surplus");

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

    // Priorities: the heater first.
    let (_, view) = call("PUT", "/api/surplus/order", Some(json!({"device_ids": [relay_id, charger_id]}))).await;
    assert_eq!(view["loads"][0]["device_id"], relay_id);
    // No longer steered: left as it is.
    let (s, _) = call("DELETE", &format!("/api/surplus/loads/{charger_id}"), None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, view) = call("GET", "/api/surplus", None).await;
    assert_eq!(view["loads"].as_array().unwrap().len(), 1);
}
