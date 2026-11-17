//! The price of electricity: the contract and the tariffs administrators set, the price everyone
//! reads.

use breezone_server::config::Config;
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};

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
    let mut cfg = Config::for_tests(dir);
    cfg.scan_port = 9;
    let state = breezone_server::init_state(cfg).await.unwrap();
    let base = format!("http://127.0.0.1:{}", serve(breezone_server::app(state)).await);
    let http = reqwest::Client::new();
    let admin = Client { token: token(&http, &base, "/api/setup", "admin").await, base: base.clone(), http: http.clone() };
    admin.call(Method::POST, "/api/users", Some(json!({"username": "camille", "password": "motdepasse1", "role": "user", "access": []}))).await;
    let user = Client { token: token(&http, &base, "/api/auth/login", "camille").await, base: base.clone(), http: http.clone() };

    let (status, v) = user.call(Method::GET, "/api/energy/price", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(v["now"].is_null() && v["periods"] == json!([]), "no tariff yet: {v}");

    let contract = json!({"country": "FR", "supplier": "EDF", "offer": "custom", "power_kva": 9, "currency": "EUR"});
    let (status, _) = user.call(Method::PUT, "/api/energy/contract", Some(contract.clone())).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "only an administrator sets the contract");
    let (status, v) = admin.call(Method::PUT, "/api/energy/contract", Some(contract)).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["contract"]["supplier"], "EDF");

    let (status, v) = admin.call(Method::PUT, "/api/energy/tariffs/2020-01-01", Some(flat(0.25))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let (status, v) = admin.call(Method::PUT, "/api/energy/tariffs/2020-01-01", Some(json!({"subscription": 1, "profiles": [], "calendar": {"weekdays": []}}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "a tariff without profiles: {v}");
    let (status, _) = admin.call(Method::PUT, "/api/energy/tariffs/demain", Some(flat(0.25))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // A dearer tariff from a day already past: the price now is its own.
    admin.call(Method::PUT, "/api/energy/tariffs/2024-02-01", Some(flat(0.3))).await;
    let (_, v) = user.call(Method::GET, "/api/energy/tariff", None).await;
    assert_eq!(v["can_edit"], false);
    let from: Vec<&str> = v["versions"].as_array().unwrap().iter().map(|x| x["valid_from"].as_str().unwrap()).collect();
    assert_eq!(from, vec!["2020-01-01", "2024-02-01"]);
    let (_, v) = user.call(Method::GET, "/api/energy/price?hours=12", None).await;
    assert_eq!((v["now"]["price"].as_f64(), v["now"]["slot"].as_str(), v["currency"].as_str()), (Some(0.3), Some("Base"), Some("EUR")), "{v}");
    let periods = v["periods"].as_array().unwrap();
    assert_eq!(periods.len(), 1, "one price all along: {v}");
    assert_eq!(periods[0]["end"].as_i64().unwrap() - periods[0]["start"].as_i64().unwrap(), 12 * 3600);

    let (status, _) = admin.call(Method::DELETE, "/api/energy/tariffs/2024-02-01", None).await;
    assert_eq!(status, StatusCode::OK);
    let (_, v) = user.call(Method::GET, "/api/energy/price", None).await;
    assert_eq!(v["now"]["price"].as_f64(), Some(0.25), "back to the older one");
    let (status, _) = admin.call(Method::DELETE, "/api/energy/tariffs/2024-02-01", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
