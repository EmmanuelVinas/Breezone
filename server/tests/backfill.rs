//! The history Breezone missed is filled with what the hardware kept (a Shelly Pro 3EM in single
//! phase keeps a record a minute): only measured values, the counters joining what was recorded.

mod support;

use axum::extract::Query;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use std::collections::HashMap;

async fn serve(router: Router) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    port
}

/// A Pro 3EM in single phase: one channel drawing 60 W, a minute of 1 Wh kept forever; its pages
/// have 30 minutes, as the real one.
async fn fake_pro3emm() -> u16 {
    let page = |Query(q): Query<HashMap<String, i64>>| async move {
        let (from, until) = (q["ts"].div_euclid(60) * 60, q["end_ts"]);
        let rows: Vec<Value> = (0..30).map(|i| from + i * 60).filter(|t| *t < until).map(|_| json!([1.0, 0.0, 230.0, 0.26])).collect();
        let next = from + 30 * 60;
        Json(json!({
            "keys": ["total_act_energy", "total_act_ret_energy", "avg_voltage", "avg_current"],
            "data": [{"ts": from, "period": 60, "values": rows}],
            "next_record_ts": if next < until { json!(next) } else { Value::Null }
        }))
    };
    serve(
        Router::new()
            .route("/rpc/Shelly.GetDeviceInfo", get(|| async { Json(json!({"id": "shellypro3em-c0ffee", "app": "Pro3EMM", "ver": "1.7.1", "gen": 2, "mac": "8C4F00C0FFEE"})) }))
            .route(
                "/rpc/Shelly.GetStatus",
                get(|| async {
                    Json(json!({
                        "em1:0": {"id": 0, "act_power": 60.0, "voltage": 230.0, "current": 0.26},
                        "em1data:0": {"id": 0, "total_act_energy": 10000.0, "total_act_ret_energy": 0.0}
                    }))
                }),
            )
            .route("/rpc/EM1Data.GetData", get(page)),
    )
    .await
}

#[tokio::test]
async fn a_gap_in_the_history_is_filled_with_what_the_meter_kept() {
    let shelly = fake_pro3emm().await;
    let dir = std::env::temp_dir().join(format!("breezone-backfill-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut cfg = support::config(&dir);
    cfg.scan_port = 9;
    cfg.history_interval_secs = 300;
    let state = breezone_server::init_state(cfg).await.unwrap();
    let base = format!("http://127.0.0.1:{}", serve(breezone_server::app(state.clone())).await);
    let http = reqwest::Client::new();
    let v: Value = http.post(format!("{base}/api/setup")).json(&json!({"username":"admin","password":"motdepasse1"})).send().await.unwrap().json().await.unwrap();
    let token = v["access_token"].as_str().unwrap().to_string();
    let source: Value = http
        .post(format!("{base}/api/sources"))
        .bearer_auth(&token)
        .json(&json!({"driver": "shelly", "name": "Pro 3EM", "config": {"host": format!("127.0.0.1:{shelly}")}}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(source["status"]["state"], "online", "{source}");
    let devices: Value = http.get(format!("{base}/api/devices")).bearer_auth(&token).send().await.unwrap().json().await.unwrap();
    let id = devices.as_array().unwrap().iter().find(|d| d["reference"] == "em1:0").expect("the channel")["id"].as_i64().unwrap();

    // Recorded every 5 minutes, then nothing for an hour (the server was stopped).
    let t0 = (breezone_server::db::now() - 4 * 3600).div_euclid(3600) * 3600;
    let point = |ts: i64, field: &str, value: f64| breezone_server::db::DeviceSample { device_id: id, ts, field: field.into(), value };
    let recorded = vec![point(t0, "power", 60.0), point(t0, "energy_in", 9.0), point(t0 + 3600, "power", 60.0), point(t0 + 3600, "energy_in", 9.06)];
    breezone_server::db::insert_device_samples(&state.db, &recorded).await.unwrap();

    let added = breezone_server::backfill::fill(&state, breezone_server::db::now()).await.unwrap();
    assert!(added > 0);
    let rows: Vec<(i64, String, f64)> = sqlx::query_as("SELECT ts, field, value FROM device_samples WHERE device_id = ? AND ts > ? AND ts < ? ORDER BY ts, field")
        .bind(id)
        .bind(t0)
        .bind(t0 + 3600)
        .fetch_all(&state.db)
        .await
        .unwrap();
    let at = |ts: i64, f: &str| rows.iter().find(|(t, field, _)| *t == ts && field == f).map(|r| r.2);
    // Every 5 minutes of the gap, the mean of its minutes.
    assert_eq!(rows.iter().filter(|r| r.1 == "power").count(), 11, "{rows:?}");
    assert_eq!(at(t0 + 300, "power"), Some(60.0));
    assert_eq!(at(t0 + 1800, "voltage"), Some(230.0));
    // The counter, counted back from 9.06 kWh at the end: 1 Wh a minute.
    assert_eq!(at(t0 + 3300, "energy_in"), Some(9.055));
    assert_eq!(at(t0 + 300, "energy_in"), Some(9.005));
    let events: Value = http.get(format!("{base}/api/events")).bearer_auth(&token).header("Accept-Language", "fr").send().await.unwrap().json().await.unwrap();
    assert!(events.to_string().contains("historique repris de l'appareil pour 1 h 00 manquants"), "{events}");

    // Asked once: nothing more the next time.
    assert_eq!(breezone_server::backfill::fill(&state, breezone_server::db::now()).await.unwrap(), 0);
}
