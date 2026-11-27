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

    // The list of before: the relay (2 kW) and a charger (16 A at most, a guarantee).
    let (_, view) = call("GET", "/api/surplus", None).await;
    assert!(view["loads"].as_array().unwrap().is_empty(), "{view}");
    let relay_row = breezone_server::db::SurplusLoadRow { power: Some(2000), position: 0, ..breezone_server::db::SurplusLoadRow::charger(relay_id) };
    breezone_server::db::upsert_surplus_load(&state.db, &relay_row).await.unwrap();
    let legacy = breezone_server::db::SurplusLoadRow {
        max_amps: Some(16),
        guarantee_kwh: Some(20.0),
        guarantee_by: Some("07:00".into()),
        position: -1,
        ..breezone_server::db::SurplusLoadRow::charger(charger_id)
    };
    breezone_server::db::upsert_surplus_load(&state.db, &legacy).await.unwrap();
    // The charger charges on the surplus by its charging setting, leaves the list, and its
    // guarantee is told.
    breezone_server::charging::adopt_surplus_chargers(&state).await.unwrap();
    let (_, plan) = call("GET", &format!("/api/devices/{charger_id}/charging"), None).await;
    assert_eq!((plan["mode"].as_str(), &plan["automation"]), (Some("solar"), &Value::Null), "{plan}");
    let row = breezone_server::db::charger_plan(&state.db, charger_id).await.unwrap().unwrap();
    let solar: Value = serde_json::from_str(row.solar.as_deref().unwrap()).unwrap();
    assert_eq!(solar["kw"], 3.7, "16 A");
    let (_, events) = call("GET", "/api/events", None).await;
    let said: Vec<&str> = events.as_array().unwrap().iter().filter_map(|e| e["message"].as_str()).collect();
    assert!(said.iter().any(|m| m.contains("garantie (+20 kWh avant 07:00) est à refaire")), "{said:?}");
    // Once: nothing more to move.
    breezone_server::charging::adopt_surplus_chargers(&state).await.unwrap();
    assert!(breezone_server::db::list_surplus_loads(&state.db).await.unwrap().iter().all(|l| l.device_id != charger_id));

    // The relay of the list becomes an automation, first in priority: it follows the surplus while it is daytime.
    breezone_server::surplus::adopt_list(&state).await.unwrap();
    breezone_server::surplus::adopt_list(&state).await.unwrap();
    let (_, rules) = call("GET", "/api/automations", None).await;
    assert_eq!(rules.as_array().unwrap().len(), 1, "made once: {rules}");
    let heater_rule = rules[0]["id"].as_i64().unwrap();
    assert_eq!((rules[0]["priority"].as_i64(), rules[0]["triggers"][0]["kind"].as_str()), (Some(1), Some("daylight")), "{rules}");
    assert_eq!(rules[0]["actions"][0], json!({"kind": "surplus", "device_id": relay_id, "enabled": true, "power": 2000}));
    assert_eq!(rules[0]["over"][0], json!({"kind": "surplus", "device_id": relay_id, "enabled": false}));

    // The charger by an automation, put first in priority.
    let (s, charging) = call(
        "POST",
        "/api/automations",
        Some(json!({"name": "Solaire en journée", "triggers": [{"kind": "daylight", "day": true}],
            "actions": [{"kind": "charger_profile", "device_id": charger_id, "mode": "solar", "solar": {"kw": 3.7}}]})),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{charging}");
    let charging_rule = charging["id"].as_i64().unwrap();
    assert_eq!(charging["priority"], 2, "a new one comes last");
    call("POST", &format!("/api/automations/{charging_rule}/run"), None).await;
    let (s, _) = call("PUT", "/api/automations/order", Some(json!({"ids": [charging_rule]}))).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, view) = call("GET", "/api/surplus", None).await;
    let order: Vec<(i64, i64)> = view["loads"].as_array().unwrap().iter().map(|l| (l["device_id"].as_i64().unwrap(), l["priority"].as_i64().unwrap())).collect();
    assert_eq!(order, vec![(charger_id, 1), (relay_id, 2)], "{view}");

    // A rule of a lower priority leaves the relay to the one that keeps it on the surplus.
    let (_, low) = call(
        "POST",
        "/api/automations",
        Some(json!({"name": "Heures creuses", "triggers": [{"kind": "time", "time": "22:00"}],
            "actions": [{"kind": "device", "device_id": relay_id, "trait": "switch.relay", "action": "set", "params": {"on": true}}]})),
    )
    .await;
    let (_, ran) = call("POST", &format!("/api/automations/{}/run", low["id"]), None).await;
    assert_eq!(ran["results"][0]["what"], "Pro 3EM relais laissé à « Pro 3EM relais sur le surplus », de priorité plus haute", "{ran}");
    assert!(!*meter.relay.lock().unwrap());
    let (_, rules) = call("GET", "/api/automations", None).await;
    let low_view = rules.as_array().unwrap().iter().find(|r| r["id"] == low["id"]).unwrap();
    assert_eq!(low_view["surplus"][0], "Pro 3EM relais (« Pro 3EM relais sur le surplus »)", "{low_view}");

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
    // The charger first, then the relay.
    let (_, plan) = call("GET", &format!("/api/devices/{charger_id}/charging"), None).await;
    assert_eq!(plan["status"]["reason"], "Charge solaire à 10 A", "{plan}");
    assert_eq!(view["loads"][1]["status"]["text"], "Attend 2 kW de surplus", "{view}");

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

    // Its automation deleted: the relay no longer follows the surplus.
    let (s, _) = call("DELETE", &format!("/api/automations/{heater_rule}"), None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, view) = call("GET", "/api/surplus", None).await;
    let left: Vec<i64> = view["loads"].as_array().unwrap().iter().map(|l| l["device_id"].as_i64().unwrap()).collect();
    assert_eq!(left, vec![charger_id]);
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

    // Profiles are powers.
    let (s, _) = call(&admin, "POST", "/api/charging/profiles", Some(json!({"name": "Trop", "kw": 70}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "more than a charger of a home");
    let (s, p) = call(&admin, "POST", "/api/charging/profiles", Some(json!({"name": "Nuit 7 kW", "kw": 7, "color": "#14b8a6"}))).await;
    assert_eq!(s, StatusCode::CREATED, "{p}");
    let profile = p["id"].as_i64().unwrap();
    let (s, _) = call(&admin, "POST", "/api/charging/profiles", Some(json!({"name": "Maximum"}))).await;
    assert_eq!(s, StatusCode::CREATED);

    // An automation says no charge.
    let path = format!("/api/devices/{id}/charging");
    let rule = |action: Value| json!({"name": "Pas de charge", "when": {"kind": "time", "time": "03:00"}, "actions": [action]});
    let (s, _) = call(&admin, "POST", "/api/automations", Some(rule(json!({"kind": "charger_profile", "device_id": id, "mode": "profile", "profile_id": profile + 100})))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "an unknown profile");
    let (s, r) = call(&admin, "POST", "/api/automations", Some(rule(json!({"kind": "charger_profile", "device_id": id, "mode": "off"})))).await;
    assert_eq!(s, StatusCode::CREATED, "{r}");
    let rule_id = r["id"].as_i64().unwrap();
    assert_eq!(r["summary"]["actions"][0], "Trydan : ne pas charger", "{r}");
    let (s, v) = call(&admin, "POST", &format!("/api/automations/{rule_id}/run"), None).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, v) = call(&admin, "GET", &path, None).await;
    assert_eq!((v["mode"].as_str(), v["automation"]["name"].as_str(), v["days"].as_array().unwrap().len()), (Some("off"), Some("Pas de charge"), 3), "{v}");
    assert_eq!(v["days"][1]["segments"][0]["effect"]["mode"], "off", "{v}");
    assert_eq!(v["next"], Value::Null, "nothing charges");
    let (_, plans) = call(&admin, "GET", "/api/charging/automations", None).await;
    assert_eq!(plans[0]["bars"][0]["segments"], json!([{"start": 0, "end": 1440, "effect": {"mode": "off"}}]), "{plans}");

    // No charge: the charge that started when the car was plugged in is stopped.
    let t0 = breezone_server::db::now();
    look(t0).await;
    assert_eq!(charger.lock().unwrap()["Paused"], 1);
    let (_, v) = call(&admin, "GET", &path, None).await;
    assert_eq!((v["status"]["state"].as_str(), v["status"]["reason"].as_str()), (Some("no_charge"), Some("Pas de charge : pas de charge")), "{v}");

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
    // The 2 kWh are in: back to what the automation said, which stops it once the charger has settled.
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
    assert_eq!(call(&ana, "POST", &format!("{path}/now"), Some(json!({}))).await.0, StatusCode::FORBIDDEN);
    assert_eq!(call(&ana, "POST", "/api/charging/profiles", Some(json!({"name": "x"}))).await.0, StatusCode::FORBIDDEN);

    // The 7 kW profile: the automation now gives it.
    let (s, _) = call(
        &admin,
        "PUT",
        &format!("/api/automations/{rule_id}"),
        Some(
            json!({"name": "Nuit", "when": {"kind": "time", "time": "22:00"}, "actions": [{"kind": "charger_profile", "device_id": id, "mode": "profile", "profile_id": profile}]}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, list) = call(&admin, "GET", "/api/charging/profiles", None).await;
    let seven = list.as_array().unwrap().iter().find(|p| p["id"] == profile).unwrap();
    assert_eq!(seven["automations"][0]["name"], "Nuit", "{list}");
    // Its own app decides: the automation does not apply.
    charger.lock().unwrap()["Timer"] = json!(1);
    state.drivers.poll(1).await;
    let (_, v) = call(&admin, "POST", &format!("/api/automations/{rule_id}/run"), None).await;
    assert!(v["results"][0]["ok"] == false && v.to_string().contains("appli de la borne"), "{v}");

    // Deleted: the charger is left alone.
    assert_eq!(call(&admin, "DELETE", &format!("/api/charging/profiles/{profile}"), None).await.0, StatusCode::NO_CONTENT);
}
