//! Linked accounts against fake services: RTE (Tempo), Conso API (Linky), Netatmo, through the
//! modules shipped with Breezone (their requests sent to the fakes).
#![cfg(feature = "wasm")]

mod support;

use axum::extract::{Form, Query};
use axum::http::{HeaderMap, StatusCode as Code};
use axum::routing::{get, post};
use axum::{Json, Router};
use reqwest::StatusCode;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// The tests of this file run one at a time: the modules reach the database of the last server
/// started (one per process), where the accounts of the other test are not.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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

/// Netatmo: each refresh token works once and is replaced by the next one (each access lasts 30 s,
/// renewed at each use); a home with its
/// gateway, a shutter and a line of an Ecometer; the changes asked are kept in `set`.
fn netatmo(used: Arc<Mutex<Vec<String>>>, set: Arc<Mutex<Vec<Value>>>) -> Router {
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
                    // About to end at once: Breezone renews it at each use.
                    (Code::OK, Json(json!({"access_token": "net-access", "refresh_token": next, "expires_in": 30})))
                }
            }),
        )
        .route(
            "/api/homesdata",
            get(|h: HeaderMap| async move {
                assert_eq!(bearer(&h), "Bearer net-access");
                Json(json!({"body": {"homes": [{"id": "h1", "name": "Maison", "rooms": [{}, {}, {}], "modules": [
                    {"id": "g1", "type": "NLG", "name": "Passerelle"},
                    {"id": "v1", "type": "NLV", "name": "Volet bureau", "bridge": "g1"},
                    {"id": "e1", "type": "NLE", "name": "Chauffage", "bridge": "g1"},
                    {"id": "e2", "type": "NLE", "name": "Institut", "bridge": "g1"}
                ]}]}}))
            }),
        )
        .route(
            "/api/homestatus",
            get(|Query(q): Query<HashMap<String, String>>| async move {
                assert_eq!(q["home_id"], "h1");
                Json(json!({"body": {"home": {"id": "h1", "modules": [
                    {"id": "g1", "type": "NLG"},
                    {"id": "v1", "type": "NLV", "current_position": 30, "target_position": 30, "reachable": true},
                    {"id": "e1", "type": "NLE", "power": 812, "reachable": true},
                    {"id": "e2", "type": "NLE"}
                ]}}}))
            }),
        )
        .route(
            // A line of the Ecometer without a power in its status: 300 Wh in the last whole half hour.
            "/api/getmeasure",
            post(|h: HeaderMap, Form(f): Form<HashMap<String, String>>| async move {
                assert_eq!(bearer(&h), "Bearer net-access");
                assert_eq!((f["device_id"].as_str(), f["module_id"].as_str(), f["scale"].as_str()), ("g1", "e2", "30min"));
                assert!(f["type"].starts_with("sum_energy_elec"));
                let end: i64 = f["date_end"].parse().unwrap();
                let start = end - end % 1800 - 3600;
                Json(json!({"body": [{"beg_time": start + 900, "step_time": 1800, "value": [[null, null, 100, null], [null, null, 200, 100]]}]}))
            }),
        )
        .route(
            "/api/setstate",
            post(move |Json(body): Json<Value>| {
                let set = set.clone();
                async move {
                    set.lock().unwrap().push(body);
                    Json(json!({"status": "ok"}))
                }
            }),
        )
}

#[tokio::test]
async fn accounts_are_linked_used_and_removed() {
    let _serial = SERIAL.lock().await;
    let used = Arc::new(Mutex::new(vec![]));
    let set = Arc::new(Mutex::new(vec![]));
    let dir = std::env::temp_dir().join(format!("breezone-accounts-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let cfg = support::config(&dir);
    let redirect = breezone_server::modules::wasm::redirect_for_tests;
    redirect("digital.iservices.rte-france.com", &format!("127.0.0.1:{}", serve(rte()).await));
    redirect("conso.boris.sh", &format!("127.0.0.1:{}", serve(conso()).await));
    redirect("api.netatmo.com", &format!("127.0.0.1:{}", serve(netatmo(used.clone(), set.clone())).await));
    let state = breezone_server::init_state(cfg).await.unwrap();
    let port = serve(breezone_server::app(state)).await;
    let mut c = Client { base: format!("http://127.0.0.1:{port}"), http: reqwest::Client::new(), token: None };
    let (_, v) = c.call(reqwest::Method::POST, "/api/setup", Some(json!({"username":"admin","password":"motdepasse1"}))).await;
    c.token = Some(v["access_token"].as_str().unwrap().into());

    // Every kind is listed, none connected; the weather, a service without credentials, is on.
    let (s, list) = c.call(reqwest::Method::GET, "/api/accounts", None).await;
    assert_eq!(s, StatusCode::OK, "{list}");
    let kinds: Vec<&str> = list.as_array().unwrap().iter().map(|a| a["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["airzone", "anker_cloud", "deltadore", "ezviz", "forecast_solar", "linky", "weather", "netatmo", "rte"], "the Airzone account, then the modules by name");
    let account = |list: &Value, kind: &str| list.as_array().unwrap().iter().find(|a| a["kind"] == kind).cloned().unwrap();
    assert!(["airzone", "deltadore", "ezviz", "linky", "netatmo", "rte"].iter().all(|k| {
        let a = account(&list, k);
        a["connected"] == false && a["state"] == "none" && a["enabled"] == true
    }));
    let weather = account(&list, "weather");
    assert_eq!((weather["connected"].clone(), weather["service"].clone(), weather["enabled"].clone()), (json!(true), json!(true), json!(true)));
    let (s, v) = c.call(reqwest::Method::PUT, "/api/accounts/weather/enabled", Some(json!({"enabled": false}))).await;
    assert_eq!((s, v["enabled"].clone()), (StatusCode::OK, json!(false)), "{v}");
    let (_, v) = c.call(reqwest::Method::GET, "/api/weather", None).await;
    assert_eq!(v["online"], false, "switched off, the weather is not read: {v}");
    let (s, _) = c.call(reqwest::Method::DELETE, "/api/accounts/weather", None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "a service is switched off, not removed");

    // RTE: wrong credentials refused and nothing kept; the right ones read the colours.
    let (s, v) = c.call(reqwest::Method::PUT, "/api/accounts/rte", Some(json!({"client_id": "breezone", "client_secret": "faux"}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    let (_, list) = c.call(reqwest::Method::GET, "/api/accounts", None).await;
    assert_eq!(account(&list, "rte")["connected"], false);
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
    assert_eq!(v["config"]["homes"], json!([{"name": "Maison", "rooms": 3, "modules": 4}]));
    let (s, v) = c.call(reqwest::Method::POST, "/api/accounts/netatmo/test", None).await;
    assert_eq!((s, v["state"].clone()), (StatusCode::OK, json!("ok")), "the second refresh token was kept: {v}");
    assert_eq!(used.lock().unwrap()[..2], ["refresh-1", "refresh-2"]);

    // The Netatmo module came with the account: a shutter (no local one here) and the Ecometer's line.
    let (_, sources) = c.call(reqwest::Method::GET, "/api/sources", None).await;
    let source = sources.as_array().unwrap().iter().find(|s| s["driver"] == "netatmo").cloned().expect("netatmo module added");
    let (s, v) = c.call(reqwest::Method::POST, &format!("/api/sources/{}/refresh", source["id"]), None).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, devices) = c.call(reqwest::Method::GET, "/api/devices", None).await;
    let find = |name: &str| devices.as_array().unwrap().iter().find(|d| d["name"] == name).cloned();
    assert!(find("Passerelle").is_none(), "the gateway is not a device: {devices}");
    let line = find("Chauffage").expect("the line of the Ecometer");
    assert_eq!(line["state"]["power"], 812.0);
    let institut = find("Institut").expect("a line without a power in its status");
    assert_eq!(institut["state"]["power"], 600.0, "300 Wh in its last whole half hour: {institut}");
    let shutter = find("Volet bureau").expect("the shutter");
    assert_eq!((shutter["state"]["position"].clone(), shutter["state"]["stoppable"].clone()), (json!(30), json!(true)));
    // Stopped where it is: Legrand's -1, through its gateway.
    let (s, v) = c.call(reqwest::Method::POST, &format!("/api/devices/{}/actions", shutter["id"]), Some(json!({"trait": "cover.position", "action": "stop", "params": {}}))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(set.lock().unwrap().last().cloned(), Some(json!({"home": {"id": "h1", "modules": [{"id": "v1", "bridge": "g1", "target_position": -1}]}})));

    // What became of each device of the cloud module, and the line of the Ecometer left out by hand.
    let path = format!("/api/sources/{}/twins", source["id"]);
    let (s, v) = c.call(reqwest::Method::GET, &path, None).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let status = |v: &Value, r: &str| v["devices"].as_array().unwrap().iter().find(|d| d["reference"] == r).unwrap()["status"].clone();
    assert_eq!((status(&v, "h1/v1"), status(&v, "h1/e1")), (json!("own"), json!("own")));
    let (s, v) = c.call(reqwest::Method::PUT, &path, Some(json!({"reference": "h1/e1", "choice": "ignore"}))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(status(&v, "h1/e1"), "ignored");
    let (_, devices) = c.call(reqwest::Method::GET, "/api/devices", None).await;
    assert!(!devices.as_array().unwrap().iter().any(|d| d["name"] == "Chauffage"), "an ignored device is no longer one: {devices}");

    // The account switched off: its module stops, and comes back with it.
    let (s, v) = c.call(reqwest::Method::PUT, "/api/accounts/netatmo/enabled", Some(json!({"enabled": false}))).await;
    assert_eq!((s, v["enabled"].clone(), v["connected"].clone()), (StatusCode::OK, json!(false), json!(true)), "{v}");
    let (_, sources) = c.call(reqwest::Method::GET, "/api/sources", None).await;
    assert_eq!(sources.as_array().unwrap().iter().find(|s| s["driver"] == "netatmo").unwrap()["enabled"], false);
    let (s, _) = c.call(reqwest::Method::PUT, "/api/accounts/netatmo/enabled", Some(json!({"enabled": true}))).await;
    assert_eq!(s, StatusCode::OK);
    let (_, sources) = c.call(reqwest::Method::GET, "/api/sources", None).await;
    assert_eq!(sources.as_array().unwrap().iter().find(|s| s["driver"] == "netatmo").unwrap()["enabled"], true);

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
    assert_eq!(account(&list, "rte")["connected"], false);
}

/// Delta Dore: its login (Azure B2C, a multipart form), then the sites of the account, each with
/// the MAC and the password of its Tydom gateway.
fn deltadore() -> Router {
    Router::new()
        .route(
            "/deltadoreadb2ciot.onmicrosoft.com/v2.0/.well-known/openid-configuration",
            get(|| async { Json(json!({"token_endpoint": "https://deltadoreadb2ciot.b2clogin.com/deltadoreadb2ciot.onmicrosoft.com/oauth2/v2.0/token?p=b2c"})) }),
        )
        .route(
            "/deltadoreadb2ciot.onmicrosoft.com/oauth2/v2.0/token",
            post(|h: HeaderMap, body: String| async move {
                let multipart = h.get("content-type").and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("multipart/form-data"));
                if multipart && body.contains("\r\n\r\nguillaume@example.com\r\n") && body.contains("\r\n\r\nbon\r\n") {
                    (Code::OK, Json(json!({"access_token": "dd-token"})))
                } else {
                    (Code::BAD_REQUEST, Json(json!({"error": "invalid_grant", "error_description": "AADB2C90225: The username or password provided in the request are invalid."})))
                }
            }),
        )
        .route(
            "/sitesmanagement/api/v1/sites",
            get(|h: HeaderMap| async move {
                if bearer(&h) != "Bearer dd-token" {
                    return (Code::UNAUTHORIZED, Json(json!({})));
                }
                (Code::OK, Json(json!({"sites": [{"id": "s1", "gateway": {"mac": "001A25123456", "password": "gateway-secret"}}]})))
            }),
        )
}

#[tokio::test]
async fn the_delta_dore_account_learns_the_passwords_of_its_gateways() {
    let _serial = SERIAL.lock().await;
    let dir = std::env::temp_dir().join(format!("breezone-deltadore-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let cfg = support::config(&dir);
    let port = serve(deltadore()).await;
    for host in ["deltadoreadb2ciot.b2clogin.com", "prod.iotdeltadore.com"] {
        breezone_server::modules::wasm::redirect_for_tests(host, &format!("127.0.0.1:{port}"));
    }
    let state = breezone_server::init_state(cfg).await.unwrap();
    let port = serve(breezone_server::app(state)).await;
    let mut c = Client { base: format!("http://127.0.0.1:{port}"), http: reqwest::Client::new(), token: None };
    let (_, v) = c.call(reqwest::Method::POST, "/api/setup", Some(json!({"username":"admin","password":"motdepasse1"}))).await;
    c.token = Some(v["access_token"].as_str().unwrap().into());

    let (s, v) = c.call(reqwest::Method::PUT, "/api/accounts/deltadore", Some(json!({"email": "guillaume@example.com", "password": "faux"}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "a wrong password is the user's to fix: {v}");
    let (s, v) = c.call(reqwest::Method::PUT, "/api/accounts/deltadore", Some(json!({"email": "guillaume@example.com", "password": "bon"}))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!((v["connected"].clone(), v["state"].clone()), (json!(true), json!("ok")));
    assert_eq!(v["config"]["email"], "guillaume@example.com");
    assert_eq!(v["config"]["gateways"], json!(["001A25123456"]), "the gateways reached are shown: {v}");
    assert!(!v.to_string().contains("gateway-secret") && !v.to_string().contains("\"bon\""), "no password shown: {v}");
}
