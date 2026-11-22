//! The price of electricity: the contract and the tariffs administrators set, the price everyone
//! reads.

mod support;

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

struct Client {
    base: String,
    http: reqwest::Client,
    token: String,
}

impl Client {
    async fn call(&self, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut req = self.http.request(method, format!("{}{}", self.base, path)).bearer_auth(&self.token);
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await.unwrap();
        let status = resp.status();
        (status, serde_json::from_str(&resp.text().await.unwrap()).unwrap_or(Value::Null))
    }
}

/// An installation (a simulated WebServer): the contract and the prices are its own.
async fn installation(admin: &Client) -> i64 {
    let ws = serve(mock::router(Arc::new(Mutex::new(mock::preset("maison"))) as MockState)).await;
    let (_, v) = admin.call(Method::POST, "/api/installations", Some(json!({"name": "Maison", "host": "127.0.0.1", "port": ws}))).await;
    v["id"].as_i64().unwrap_or_else(|| panic!("{v}"))
}

async fn token(http: &reqwest::Client, base: &str, path: &str, user: &str) -> String {
    let v: Value = http.post(format!("{base}{path}")).json(&json!({"username": user, "password": "motdepasse1"})).send().await.unwrap().json().await.unwrap();
    v["access_token"].as_str().unwrap_or_else(|| panic!("{v}")).to_string()
}

/// One price all day long: the price now is known whatever the time of the test.
fn flat(price: f64) -> Value {
    json!({
        "subscription": 12.5,
        "profiles": [{"id": "normal", "name": "Normal", "slots": [{"start": "00:00", "name": "Base", "price": price}]}],
        "calendar": {"weekdays": ["normal", "normal", "normal", "normal", "normal", "normal", "normal"]}
    })
}

#[tokio::test]
async fn administrators_set_the_prices_everyone_reads() {
    let dir = std::env::temp_dir().join(format!("breezone-price-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut cfg = support::config(&dir);
    cfg.scan_port = 9;
    let state = breezone_server::init_state(cfg).await.unwrap();
    let base = format!("http://127.0.0.1:{}", serve(breezone_server::app(state)).await);
    let http = reqwest::Client::new();
    let admin = Client { token: token(&http, &base, "/api/setup", "admin").await, base: base.clone(), http: http.clone() };
    let home = installation(&admin).await;
    let e = format!("/api/installations/{home}/electricity");
    admin
        .call(
            Method::POST,
            "/api/users",
            Some(json!({"username": "camille", "password": "motdepasse1", "role": "user", "access": [{"installation_id": home, "permission": "read"}]})),
        )
        .await;
    let user = Client { token: token(&http, &base, "/api/auth/login", "camille").await, base: base.clone(), http: http.clone() };

    let (status, v) = user.call(Method::GET, &format!("{e}/price"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(v["now"].is_null() && v["periods"] == json!([]), "no tariff yet: {v}");

    let contract = json!({"country": "FR", "supplier": "EDF", "offer": "custom", "power_kva": 9, "currency": "EUR"});
    let (status, _) = user.call(Method::PUT, &format!("{e}/contract"), Some(contract.clone())).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "only an administrator sets the contract");
    let (status, v) = admin.call(Method::PUT, &format!("{e}/contract"), Some(contract)).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["contract"]["supplier"], "EDF");

    let (status, v) = admin.call(Method::PUT, &format!("{e}/prices/2020-01-01"), Some(flat(0.25))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let (status, v) = admin.call(Method::PUT, &format!("{e}/prices/2020-01-01"), Some(json!({"subscription": 1, "profiles": [], "calendar": {"weekdays": []}}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "a tariff without profiles: {v}");
    let (status, _) = admin.call(Method::PUT, &format!("{e}/prices/demain"), Some(flat(0.25))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // A dearer tariff from a day already past: the price now is its own.
    admin.call(Method::PUT, &format!("{e}/prices/2024-02-01"), Some(flat(0.3))).await;
    let (_, v) = user.call(Method::GET, &e, None).await;
    assert_eq!(v["can_edit"], false);
    let from: Vec<&str> = v["versions"].as_array().unwrap().iter().map(|x| x["valid_from"].as_str().unwrap()).collect();
    assert_eq!(from, vec!["2020-01-01", "2024-02-01"]);
    let (_, v) = user.call(Method::GET, &format!("{e}/price?hours=12"), None).await;
    assert_eq!((v["now"]["price"].as_f64(), v["now"]["slot"].as_str(), v["currency"].as_str()), (Some(0.3), Some("Base"), Some("EUR")), "{v}");
    let periods = v["periods"].as_array().unwrap();
    assert_eq!(periods.len(), 1, "one price all along: {v}");
    assert_eq!(periods[0]["end"].as_i64().unwrap() - periods[0]["start"].as_i64().unwrap(), 12 * 3600);

    let (status, _) = admin.call(Method::DELETE, &format!("{e}/prices/2024-02-01"), None).await;
    assert_eq!(status, StatusCode::OK);
    let (_, v) = user.call(Method::GET, &format!("{e}/price"), None).await;
    assert_eq!(v["now"]["price"].as_f64(), Some(0.25), "back to the older one");
    let (status, _) = admin.call(Method::DELETE, &format!("{e}/prices/2024-02-01"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Another place has a contract of its own: none yet.
    let office = installation(&admin).await;
    let (_, v) = admin.call(Method::GET, &format!("/api/installations/{office}/electricity"), None).await;
    assert!(v["contract"].is_null() && v["versions"] == json!([]), "{v}");
    let (_, v) = admin.call(Method::GET, &format!("/api/installations/{office}/electricity/price"), None).await;
    assert!(v["now"].is_null(), "{v}");
    let (status, _) = user.call(Method::GET, &format!("/api/installations/{office}/electricity"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "not an installation of this user");
}

const TEMPO: &str = "DATE_DEBUT;DATE_FIN;P_SOUSCRITE;PART_FIXE_HT;PART_FIXE_TTC;PART_VARIABLE_HCBleu_HT;PART_VARIABLE_HCBleu_TTC;PART_VARIABLE_HPBleu_HT;PART_VARIABLE_HPBleu_TTC;PART_VARIABLE_HCBlanc_HT;PART_VARIABLE_HCBlanc_TTC;PART_VARIABLE_HPBlanc_HT;PART_VARIABLE_HPBlanc_TTC;PART_VARIABLE_HCRouge_HT;PART_VARIABLE_HCRouge_TTC;PART_VARIABLE_HPRouge_HT;PART_VARIABLE_HPRouge_TTC\r\n\
01/02/2026;31/07/2026;9;174,36;233,44;0,0796;0,1325;0,1035;0,1612;0,0941;0,1499;0,1251;0,1871;0,1004;0,1575;0,5575;0,706\r\n\
01/08/2026;;9;177;237,3436191;0,0824;0,1356;0,1072;0,1654;0,0974;0,1536;0,1295;0,1921;0,1040;0,1615;0,5773;0,7295\r\n";

#[tokio::test]
async fn a_regulated_offer_takes_the_prices_of_the_cre() {
    let cre = axum::Router::new().route("/Option_Tempo.csv", axum::routing::get(|| async { TEMPO }));
    let cre_port = serve(cre).await;
    let dir = std::env::temp_dir().join(format!("breezone-cre-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut cfg = support::config(&dir);
    cfg.scan_port = 9;
    cfg.cre_url = Some(format!("http://127.0.0.1:{cre_port}"));
    let state = breezone_server::init_state(cfg).await.unwrap();
    let base = format!("http://127.0.0.1:{}", serve(breezone_server::app(state)).await);
    let http = reqwest::Client::new();
    let admin = Client { token: token(&http, &base, "/api/setup", "admin").await, base: base.clone(), http: http.clone() };
    let home = installation(&admin).await;
    let e = format!("/api/installations/{home}/electricity");

    // A power the CRE has no price for: nothing is changed.
    let (status, v) = admin.call(Method::PUT, &format!("{e}/contract"), Some(json!({"country": "FR", "offer": "tempo", "power_kva": 7}))).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{v}");
    let (_, v) = admin.call(Method::GET, &e, None).await;
    assert!(v["contract"].is_null());

    let (status, v) = admin.call(Method::PUT, &format!("{e}/contract"), Some(json!({"country": "FR", "supplier": "EDF", "offer": "tempo", "power_kva": 9}))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let from: Vec<&str> = v["versions"].as_array().unwrap().iter().map(|x| x["valid_from"].as_str().unwrap()).collect();
    assert_eq!(from, vec!["2026-02-01", "2026-08-01"]);
    let latest = &v["versions"][1]["tariff"];
    assert_eq!(latest["subscription"].as_f64(), Some(19.78));
    assert_eq!(latest["profiles"][2]["slots"][1]["price"].as_f64(), Some(0.7295), "{latest}");
    assert_eq!(latest["calendar"]["day_start"], "06:00");
}

#[tokio::test]
async fn without_internet_a_regulated_offer_is_refused() {
    let dir = std::env::temp_dir().join(format!("breezone-cre-off-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut cfg = support::config(&dir);
    cfg.scan_port = 9;
    let state = breezone_server::init_state(cfg).await.unwrap();
    let base = format!("http://127.0.0.1:{}", serve(breezone_server::app(state)).await);
    let http = reqwest::Client::new();
    let admin = Client { token: token(&http, &base, "/api/setup", "admin").await, base: base.clone(), http: http.clone() };
    let home = installation(&admin).await;
    let e = format!("/api/installations/{home}/electricity");
    let (status, _) = admin.call(Method::PUT, &format!("{e}/contract"), Some(json!({"offer": "base", "power_kva": 6}))).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    let (status, _) = admin.call(Method::PUT, &format!("{e}/contract"), Some(json!({"offer": "custom", "power_kva": 6, "offpeak": [{"from": "22:15", "to": "06:00"}]}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "off-peak hours on the half hour");
}
