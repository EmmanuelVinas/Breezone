//! The colour of the Tempo days: read from the relay when a tariff follows it, then used by the
//! price of the day.

use axum::routing::get;
use axum::Json;
use breezone_server::config::Config;
use breezone_server::mock::{self, MockState};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

async fn serve(router: axum::Router) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    port
}

fn local_day(offset: i64) -> String {
    let tz = chrono_tz::Europe::Paris;
    (chrono::Utc::now().with_timezone(&tz).date_naive() + chrono::Duration::days(offset)).format("%Y-%m-%d").to_string()
}

#[tokio::test]
async fn a_red_day_is_priced_as_red() {
    let today = local_day(0);
    let tomorrow = local_day(1);
    let (t1, t2) = (today.clone(), tomorrow.clone());
    let relay = axum::Router::new()
        .route("/api/jourTempo/today", get(move || async move { Json(json!({"dateJour": t1, "codeJour": 3, "periode": "x", "libCouleur": "Rouge"})) }))
        .route("/api/jourTempo/tomorrow", get(move || async move { Json(json!({"dateJour": t2, "codeJour": 0, "periode": "x", "libCouleur": "Inconnu"})) }))
        .route("/api/joursTempo", get(|| async { Json(json!([])) }));
    let relay_port = serve(relay).await;

    let dir = std::env::temp_dir().join(format!("breezone-tempo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut cfg = Config::for_tests(dir);
    cfg.scan_port = 9;
    cfg.tempo_url = Some(format!("http://127.0.0.1:{relay_port}"));
    let state = breezone_server::init_state(cfg).await.unwrap();

    // Nothing is asked while no tariff follows colours.
    breezone_server::tempo::refresh(&state).await.unwrap();
    assert!(breezone_server::db::day_colors(&state.db, "2000-01-01", "2100-01-01").await.unwrap().is_empty());

    let base = format!("http://127.0.0.1:{}", serve(breezone_server::app(state.clone())).await);
    let http = reqwest::Client::new();
    let v: Value = http.post(format!("{base}/api/setup")).json(&json!({"username":"admin","password":"motdepasse1"})).send().await.unwrap().json().await.unwrap();
    let token = v["access_token"].as_str().unwrap().to_string();
    let ws = serve(mock::router(Arc::new(Mutex::new(mock::preset("maison"))) as MockState)).await;
    let v: Value = http
        .post(format!("{base}/api/installations"))
        .bearer_auth(&token)
        .json(&json!({"name": "Maison", "host": "127.0.0.1", "port": ws}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let e = format!("{base}/api/installations/{}/electricity", v["id"]);
    let flat = |id: &str, price: f64| json!({"id": id, "name": id, "slots": [{"start": "00:00", "name": "Base", "price": price}]});
    let tariff = json!({
        "subscription": 16,
        "profiles": [flat("blue", 0.15), flat("red", 0.7)],
        "calendar": {"weekdays": ["blue","blue","blue","blue","blue","blue","blue"], "colors": {"blue": "blue", "red": "red"}}
    });
    let r = http.put(format!("{e}/prices/2020-01-01")).bearer_auth(&token).json(&tariff).send().await.unwrap();
    assert!(r.status().is_success(), "{}", r.text().await.unwrap());
    // A tariff that follows the colours has them read at once, in the background: the day of the
    // week (blue) until they are known, then red.
    let mut price = Value::Null;
    for _ in 0..100 {
        price = http.get(format!("{e}/price")).bearer_auth(&token).send().await.unwrap().json().await.unwrap();
        if price["now"]["profile"] == "red" {
            break;
        }
        assert_eq!(price["now"]["profile"], "blue", "no colour known yet: the day of the week: {price}");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!((price["now"]["profile"].as_str(), price["now"]["price"].as_f64()), (Some("red"), Some(0.7)), "{price}");
    assert_eq!(price["tempo"]["today"], "red");
    assert!(price["tempo"]["tomorrow"].is_null(), "not announced yet: {price}");
    assert_eq!(price["tempo"]["red_left"], 21);
}
