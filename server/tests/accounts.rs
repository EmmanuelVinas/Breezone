//! Linked accounts against fake services: RTE (Tempo), Conso API (Linky), Netatmo.

use axum::extract::{Form, Query};
use axum::http::{HeaderMap, StatusCode as Code};
use axum::routing::{get, post};
use axum::{Json, Router};
use breezone_server::config::Config;
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

struct Client {
    base: String,
    http: reqwest::Client,
    token: Option<String>,
}

impl Client {
    async fn call(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut req = self.http.request(method, format!("{}{}", self.base, path));
        if let Some(t) = &self.token {
            req = req.bearer_auth(t);
        }
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await.unwrap();
        let status = resp.status();
        (status, serde_json::from_str(&resp.text().await.unwrap()).unwrap_or(Value::Null))
    }
}

fn bearer(h: &HeaderMap) -> String {
    h.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string()
}

/// RTE: a token for `breezone:secret`, then the colours of the days asked (blue, but red on the
/// first day asked).
fn rte() -> Router {
    Router::new()
        .route(
            "/token/oauth/",
            post(|h: HeaderMap| async move {
                // base64("breezone:secret")
                if bearer(&h) == "Basic YnJlZXpvbmU6c2VjcmV0" {
                    (Code::OK, Json(json!({"access_token": "rte-token", "expires_in": 7200})))
                } else {
                    (Code::UNAUTHORIZED, Json(json!({"error": "invalid_client", "error_description": "client inconnu"})))
                }
            }),
        )
        .route(
            "/open_api/tempo_like_supply_contract/v1/tempo_like_calendars",
            get(|h: HeaderMap, Query(q): Query<HashMap<String, String>>| async move {
                assert_eq!(bearer(&h), "Bearer rte-token");
                let from = chrono::NaiveDate::parse_from_str(&q["start_date"][..10], "%Y-%m-%d").unwrap();
                let until = chrono::NaiveDate::parse_from_str(&q["end_date"][..10], "%Y-%m-%d").unwrap();
                let values: Vec<Value> = from
                    .iter_days()
                    .take_while(|d| *d < until)
                    .map(|d| json!({"start_date": format!("{d}T00:00:00+01:00"), "value": if d == from { "RED" } else { "BLUE" }}))
                    .collect();
                Json(json!({"tempo_like_calendars": {"values": values}}))
            }),
        )
}

/// Conso API: 10 kWh a day, and a flat 500 W over the half hours of the last week.
fn conso() -> Router {
    let readings = |kind: &'static str| {
        move |h: HeaderMap, Query(q): Query<HashMap<String, String>>| async move {
            if bearer(&h) != "Bearer linky-token" {
                return (Code::UNAUTHORIZED, Json(json!({"message": "jeton invalide"})));
            }
            assert_eq!(q["prm"], "12345678901234");
            let from = chrono::NaiveDate::parse_from_str(&q["start"], "%Y-%m-%d").unwrap();
            let until = chrono::NaiveDate::parse_from_str(&q["end"], "%Y-%m-%d").unwrap();
            let readings: Vec<Value> = match kind {
                "daily" => from.iter_days().take_while(|d| *d < until).map(|d| json!({"value": "10000", "date": d.to_string()})).collect(),
                _ => from
                    .iter_days()
                    .take_while(|d| *d < until)
                    .flat_map(|d| {
                        (1..=48).map(
                            move |i| json!({"value": "500", "date": (d.and_hms_opt(0, 0, 0).unwrap() + chrono::Duration::minutes(30 * i)).format("%Y-%m-%d %H:%M:%S").to_string()}),
                        )
                    })
                    .collect(),
            };
            (Code::OK, Json(json!({"interval_reading": readings})))
        }
    };
    Router::new()
        .route("/api/daily_consumption", get(readings("daily")))
        .route("/api/consumption_load_curve", get(readings("curve")))
        .route("/api/daily_production", get(|| async { (Code::NOT_FOUND, Json(json!({"message": "pas de production"}))) }))
}

/// Netatmo: each refresh token works once and is replaced by the next one.
fn netatmo(used: Arc<Mutex<Vec<String>>>) -> Router {
    Router::new()
        .route(
            "/oauth2/token",
            post(move |Form(f): Form<HashMap<String, String>>| {
                let used = used.clone();
                async move {
                    let mut used = used.lock().unwrap();
                    let token = f.get("refresh_token").cloned().unwrap_or_default();
                    if f.get("client_secret").map(String::as_str) != Some("nsecret") || !token.starts_with("refresh-") || used.contains(&token) {
                        return (Code::BAD_REQUEST, Json(json!({"error": "invalid_grant"})));
                    }
                    used.push(token.clone());
                    let next = format!("refresh-{}", used.len() + 1);
                    (Code::OK, Json(json!({"access_token": "net-access", "refresh_token": next, "expires_in": 10800})))
                }
            }),
        )
        .route(
            "/api/homesdata",
            get(|h: HeaderMap| async move {
                assert_eq!(bearer(&h), "Bearer net-access");
                Json(json!({"body": {"homes": [{"id": "h1", "name": "Maison", "rooms": [{}, {}, {}], "modules": [{}, {}]}]}}))
            }),
        )
}

#[tokio::test]
async fn accounts_are_linked_used_and_removed() {
    let used = Arc::new(Mutex::new(vec![]));
    let dir = std::env::temp_dir().join(format!("breezone-accounts-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut cfg = Config::for_tests(dir);
    cfg.rte_url = format!("http://127.0.0.1:{}", serve(rte()).await);
    cfg.conso_api_url = format!("http://127.0.0.1:{}", serve(conso()).await);
    cfg.netatmo_url = format!("http://127.0.0.1:{}", serve(netatmo(used.clone())).await);
    let state = breezone_server::init_state(cfg).await.unwrap();
    let port = serve(breezone_server::app(state)).await;
    let mut c = Client { base: format!("http://127.0.0.1:{port}"), http: reqwest::Client::new(), token: None };
    let (_, v) = c.call(reqwest::Method::POST, "/api/setup", Some(json!({"username":"admin","password":"motdepasse1"}))).await;
    c.token = Some(v["access_token"].as_str().unwrap().into());

    // Every kind is listed, none connected.
    let (s, list) = c.call(reqwest::Method::GET, "/api/accounts", None).await;
    assert_eq!(s, StatusCode::OK, "{list}");
    let kinds: Vec<&str> = list.as_array().unwrap().iter().map(|a| a["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["airzone", "rte", "linky", "netatmo"]);
    assert!(list.as_array().unwrap().iter().all(|a| a["connected"] == false && a["state"] == "none"));

    // RTE: wrong credentials refused and nothing kept; the right ones read the colours.
    let (s, v) = c.call(reqwest::Method::PUT, "/api/accounts/rte", Some(json!({"client_id": "breezone", "client_secret": "faux"}))).await;
    assert_eq!(s, StatusCode::BAD_GATEWAY, "{v}");
    let (_, list) = c.call(reqwest::Method::GET, "/api/accounts", None).await;
    assert_eq!(list[1]["connected"], false);
    let (s, v) = c.call(reqwest::Method::PUT, "/api/accounts/rte", Some(json!({"client_id": "breezone", "client_secret": "secret"}))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!((v["connected"].clone(), v["state"].clone(), v["config"]["client_id"].clone()), (json!(true), json!("ok"), json!("breezone")));
    assert!(!v.to_string().contains("\"secret\""), "no secret shown: {v}");

    // Linky: a year of days, with their cost when the half hours are known.
    let (s, v) = c.call(reqwest::Method::PUT, "/api/accounts/linky", Some(json!({"token": "linky-token", "prm": "1234 5678 9012 34"}))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["config"]["prm"], "12345678901234");
    let (s, view) = c.call(reqwest::Method::GET, "/api/energy/linky?days=30", None).await;
    assert_eq!(s, StatusCode::OK, "{view}");
    let days = view["days"].as_array().unwrap();
    assert!(days.len() >= 29, "{view}");
    assert_eq!(days.last().unwrap()["consumption"], 10.0);
    assert_eq!(days.last().unwrap()["cost"], Value::Null, "no tariff: no cost");

    // Netatmo: the refresh token given is used once, the one Netatmo gives back is kept.
    let (s, v) = c.call(reqwest::Method::PUT, "/api/accounts/netatmo", Some(json!({"client_id": "nid", "client_secret": "nsecret", "refresh_token": "refresh-1"}))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["config"]["homes"], json!([{"name": "Maison", "rooms": 3, "modules": 2}]));
    let (s, v) = c.call(reqwest::Method::POST, "/api/accounts/netatmo/test", None).await;
    assert_eq!((s, v["state"].clone()), (StatusCode::OK, json!("ok")), "the second refresh token was kept: {v}");
    assert_eq!(*used.lock().unwrap(), ["refresh-1", "refresh-2"]);

    // A user does not see them.
    let (s, _) = c.call(reqwest::Method::POST, "/api/users", Some(json!({"username":"camille","password":"motdepasse1","role":"user"}))).await;
    assert_eq!(s, StatusCode::CREATED);
    let (_, v) = c.call(reqwest::Method::POST, "/api/auth/login", Some(json!({"username":"camille","password":"motdepasse1"}))).await;
    let user = Client { base: c.base.clone(), http: c.http.clone(), token: Some(v["access_token"].as_str().unwrap().into()) };
    let (s, _) = user.call(reqwest::Method::GET, "/api/accounts", None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // Removed: no longer connected.
    let (s, _) = c.call(reqwest::Method::DELETE, "/api/accounts/rte", None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, list) = c.call(reqwest::Method::GET, "/api/accounts", None).await;
    assert_eq!(list[1]["connected"], false);
}
