//! The overview of an installation: each user's own, over the one its administrators set for
//! everyone, over none (the clients compose one).

use breezone_server::config::Config;
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

async fn token(http: &reqwest::Client, base: &str, path: &str, user: &str) -> String {
    let v: Value = http.post(format!("{base}{path}")).json(&json!({"username": user, "password": "motdepasse1"})).send().await.unwrap().json().await.unwrap();
    v["access_token"].as_str().unwrap_or_else(|| panic!("{v}")).to_string()
}

fn layout(blocks: Value) -> Value {
    json!({"version": 1, "blocks": blocks})
}

#[tokio::test]
async fn each_user_may_have_an_overview_over_the_installations_one() {
    let dir = std::env::temp_dir().join(format!("breezone-dashboard-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut cfg = Config::for_tests(dir);
    cfg.scan_port = 9;
    let state = breezone_server::init_state(cfg).await.unwrap();
    let base = format!("http://127.0.0.1:{}", serve(breezone_server::app(state)).await);
    let http = reqwest::Client::new();
    let admin = Client { token: token(&http, &base, "/api/setup", "admin").await, base: base.clone(), http: http.clone() };
    let ws = serve(mock::router(Arc::new(Mutex::new(mock::preset("maison"))) as MockState)).await;
    let (_, v) = admin.call(Method::POST, "/api/installations", Some(json!({"name": "Maison", "host": "127.0.0.1", "port": ws}))).await;
    let home = v["id"].as_i64().unwrap();
    admin
        .call(Method::POST, "/api/users", Some(json!({"username":"camille","password":"motdepasse1","role":"user","access":[{"installation_id": home, "permission":"control"}]})))
        .await;
    let user = Client { token: token(&http, &base, "/api/auth/login", "camille").await, base: base.clone(), http: http.clone() };
    let path = format!("/api/installations/{home}/dashboard");

    // Nothing set: the clients compose it.
    let (_, v) = user.call(Method::GET, &path, None).await;
    assert_eq!((v["source"].clone(), v["layout"].clone(), v["can_set_default"].clone()), (json!("generated"), Value::Null, json!(false)), "{v}");

    // The administrator sets the installation's overview: everyone sees it.
    let energy = layout(json!([{"id": "e1", "type": "energy", "size": "quarter", "config": {"flow": "solar"}}]));
    let (s, v) = admin.call(Method::PUT, &format!("{path}/default"), Some(energy.clone())).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, v) = user.call(Method::GET, &path, None).await;
    assert_eq!((v["source"].clone(), v["layout"].clone()), (json!("installation"), energy.clone()), "{v}");

    // The user makes their own, for them only; the administrator still sees the installation's.
    let mine = layout(json!([{"id": "w", "type": "weather", "size": "half"}]));
    let (s, v) = user.call(Method::PUT, &path, Some(mine.clone())).await;
    assert_eq!((s, v["source"].clone()), (StatusCode::OK, json!("mine")), "{v}");
    let (_, v) = admin.call(Method::GET, &path, None).await;
    assert_eq!((v["source"].clone(), v["can_set_default"].clone()), (json!("installation"), json!(true)));

    // A user does not set the installation's; a wrong layout is refused.
    let (s, _) = user.call(Method::PUT, &format!("{path}/default"), Some(mine.clone())).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = user.call(Method::PUT, &path, Some(layout(json!([{"id": "x", "type": "clock", "size": "half"}])))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // Back to the installation's.
    let (_, v) = user.call(Method::DELETE, &path, None).await;
    assert_eq!((v["source"].clone(), v["layout"].clone()), (json!("installation"), energy), "{v}");
    let (s, _) = admin.call(Method::DELETE, &format!("{path}/default"), None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, v) = user.call(Method::GET, &path, None).await;
    assert_eq!(v["source"], "generated");
}
