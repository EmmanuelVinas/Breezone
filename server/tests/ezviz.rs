//! The EZVIZ account and module against a fake EZVIZ (its requests sent there): two-step
//! verification, the session renewed, a doorbell whose ring is a press, and its encrypted picture.

mod support;

use aes::cipher::{BlockModeEncrypt, KeyIvInit};
use axum::extract::{Form, Query};
use axum::http::{HeaderMap, StatusCode as Code};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use md5::{Digest, Md5};
use reqwest::StatusCode;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const CODE: &str = "ABCDEF";

fn md5_hex(s: &[u8]) -> String {
    hex::encode(Md5::digest(s))
}

/// A picture encrypted as EZVIZ does it, with the verification code of the cameras.
fn encrypted(picture: &[u8]) -> Vec<u8> {
    let mut key = [0u8; 16];
    key[..CODE.len()].copy_from_slice(CODE.as_bytes());
    let mut buf = picture.to_vec();
    buf.resize(picture.len() + 16 - picture.len() % 16, 0);
    let cipher = cbc::Encryptor::<aes::Aes128>::new(&key.into(), b"01234567\0\0\0\0\0\0\0\0".into())
        .encrypt_padded::<cbc::cipher::block_padding::Pkcs7>(&mut buf, picture.len())
        .unwrap()
        .to_vec();
    [b"hikencodepicture".as_slice(), md5_hex(md5_hex(CODE.as_bytes()).as_bytes()).as_bytes(), &cipher].concat()
}

#[derive(Default)]
struct Fake {
    codes_sent: AtomicUsize,
    /// The first session is over: EZVIZ answers 401 until it is renewed.
    expired: AtomicBool,
    session: Mutex<String>,
    messages: Mutex<Vec<Value>>,
    base: Mutex<String>,
}

fn ezviz(f: Arc<Fake>) -> Router {
    let session_ok = |f: &Fake, h: &HeaderMap| h.get("sessionId").and_then(|v| v.to_str().ok()) == Some(f.session.lock().unwrap().as_str()) && !f.expired.load(Ordering::SeqCst);
    let (f1, f2, f3, f4, f5) = (f.clone(), f.clone(), f.clone(), f.clone(), f.clone());
    Router::new()
        .route(
            "/v3/users/login/v5",
            post(move |h: HeaderMap, Form(form): Form<HashMap<String, String>>| {
                let f = f1.clone();
                async move {
                    assert_eq!(h.get("featureCode").unwrap().to_str().unwrap(), form["featureCode"]);
                    if form["password"] != md5_hex(b"motdepasse") {
                        return Json(json!({"meta": {"code": 1014, "message": "password"}}));
                    }
                    if form.get("smsCode").map(String::as_str).unwrap_or("").is_empty() {
                        return Json(json!({"meta": {"code": 6002}}));
                    }
                    assert_eq!((form["smsCode"].as_str(), form["msgType"].as_str(), form["bizType"].as_str()), ("123456", "3", "TERMINAL_BIND"));
                    *f.session.lock().unwrap() = "s1".into();
                    let base = f.base.lock().unwrap().clone();
                    Json(json!({"meta": {"code": 200}, "loginSession": {"sessionId": "s1", "rfSessionId": "r1"}, "loginArea": {"apiDomain": base}}))
                }
            }),
        )
        .route(
            "/v3/sms/nologin/checkcode",
            post(move |Form(form): Form<HashMap<String, String>>| {
                let f = f2.clone();
                async move {
                    assert_eq!((form["from"].as_str(), form["bizType"].as_str()), ("moi@exemple.fr", "TERMINAL_BIND"));
                    f.codes_sent.fetch_add(1, Ordering::SeqCst);
                    Json(json!({"meta": {"code": 200}}))
                }
            }),
        )
        .route(
            "/v3/apigateway/login",
            put(move |Form(form): Form<HashMap<String, String>>| {
                let f = f3.clone();
                async move {
                    assert_eq!(form["refreshSessionId"], "r1");
                    f.expired.store(false, Ordering::SeqCst);
                    *f.session.lock().unwrap() = "s2".into();
                    Json(json!({"meta": {"code": 200}, "sessionInfo": {"sessionId": "s2", "refreshSessionId": "r2"}}))
                }
            }),
        )
        .route(
            "/v3/userdevices/v1/resources/pagelist",
            get(move |h: HeaderMap, Query(q): Query<HashMap<String, String>>| {
                let f = f4.clone();
                async move {
                    if !session_ok(&f, &h) {
                        return (Code::UNAUTHORIZED, Json(json!({"meta": {"code": 401}})));
                    }
                    assert_eq!(q["offset"], "0");
                    (
                        Code::OK,
                        Json(json!({"meta": {"code": 200}, "page": {"hasNext": false}, "deviceInfos": [
                            {"deviceSerial": "BD1", "name": "Sonnette", "deviceCategory": "BDoorBell", "status": 1},
                            {"deviceSerial": "C1", "name": "Jardin", "deviceCategory": "IPC", "status": 1},
                            {"deviceSerial": "P1", "name": "Prise", "deviceCategory": "Socket", "status": 1}
                        ], "CONNECTION": {"BD1": {"localIp": "10.0.0.20"}}})),
                    )
                }
            }),
        )
        .route(
            "/v3/unifiedmsg/list",
            get(move |h: HeaderMap, Query(q): Query<HashMap<String, String>>| {
                let f = f5.clone();
                async move {
                    if !session_ok(&f, &h) {
                        return (Code::UNAUTHORIZED, Json(json!({"meta": {"code": 401}})));
                    }
                    assert_eq!(q["serials"], "BD1,C1");
                    (Code::OK, Json(json!({"meta": {"code": 200}, "message": f.messages.lock().unwrap().clone()})))
                }
            }),
        )
        .route("/pic/1.jpg", get(|| async { encrypted(b"\xff\xd8 la photo \xff\xd9") }))
}

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

#[tokio::test]
async fn an_ezviz_doorbell_rings_and_shows_its_picture() {
    let fake = Arc::new(Fake::default());
    let port = serve(ezviz(fake.clone())).await;
    *fake.base.lock().unwrap() = format!("http://127.0.0.1:{port}");
    let dir = std::env::temp_dir().join(format!("breezone-ezviz-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let cfg = support::config(&dir);
    breezone_server::modules::wasm::redirect_for_tests("apiieu.ezvizlife.com", &format!("127.0.0.1:{port}"));
    let state = breezone_server::init_state(cfg).await.unwrap();
    let port = serve(breezone_server::app(state)).await;
    let mut c = Client { base: format!("http://127.0.0.1:{port}"), http: reqwest::Client::new(), token: None };
    let (_, v) = c.call(reqwest::Method::POST, "/api/setup", Some(json!({"username":"admin","password":"motdepasse1"}))).await;
    c.token = Some(v["access_token"].as_str().unwrap().into());

    // A wrong password; then the right one: EZVIZ sends a code, given at the second try.
    let (s, v) = c.call(reqwest::Method::PUT, "/api/accounts/ezviz", Some(json!({"account": "moi@exemple.fr", "password": "faux"}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    let (s, v) = c.call(reqwest::Method::PUT, "/api/accounts/ezviz", Some(json!({"account": "moi@exemple.fr", "password": "motdepasse"}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert!(v.to_string().contains("code"), "{v}");
    assert_eq!(fake.codes_sent.load(Ordering::SeqCst), 1);
    let (s, v) =
        c.call(reqwest::Method::PUT, "/api/accounts/ezviz", Some(json!({"account": "moi@exemple.fr", "password": "motdepasse", "code": "123456", "image_code": CODE}))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!((v["connected"].clone(), v["config"]["devices"].clone()), (json!(true), json!(["Sonnette", "Jardin", "Prise"])));
    assert!(!v.to_string().contains(CODE) && !v.to_string().contains("motdepasse"), "no secret shown: {v}");

    // The module came with the account: the doorbell and the camera, not the plug.
    let (_, sources) = c.call(reqwest::Method::GET, "/api/sources", None).await;
    let source = sources.as_array().unwrap().iter().find(|s| s["driver"] == "ezviz").cloned().expect("ezviz module added");
    let refresh = format!("/api/sources/{}/refresh", source["id"]);
    let (s, v) = c.call(reqwest::Method::POST, &refresh, None).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, devices) = c.call(reqwest::Method::GET, "/api/devices", None).await;
    let find = |devices: &Value, name: &str| devices.as_array().unwrap().iter().find(|d| d["name"] == name).cloned();
    assert!(find(&devices, "Prise").is_none(), "{devices}");
    let bell = find(&devices, "Sonnette").expect("the doorbell");
    assert_eq!((bell["kind"].clone(), bell["traits"].clone(), bell["state"]["online"].clone()), (json!("doorbell"), json!(["camera@1", "button@1"]), json!(true)));
    assert_eq!(find(&devices, "Jardin").unwrap()["kind"], "camera");

    // The session ends: renewed by its refresh id. Someone rings: a press of the doorbell.
    fake.expired.store(true, Ordering::SeqCst);
    let now = chrono::Utc::now().timestamp_millis();
    fake.messages
        .lock()
        .unwrap()
        .push(json!({"msgId": "m1", "deviceSerial": "BD1", "time": now, "title": "Appel", "subType": "2701", "pic": format!("{}/pic/1.jpg", fake.base.lock().unwrap())}));
    let (s, v) = c.call(reqwest::Method::POST, &refresh, None).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(*fake.session.lock().unwrap(), "s2");
    let mut rang = false;
    for _ in 0..50 {
        let (_, events) = c.call(reqwest::Method::GET, "/api/events?limit=20", None).await;
        if events.as_array().unwrap().iter().any(|e| e["kind"] == "button" && e["message"].to_string().contains("rings")) {
            rang = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let (_, events) = c.call(reqwest::Method::GET, "/api/events?limit=20", None).await;
    let (_, devices) = c.call(reqwest::Method::GET, "/api/devices", None).await;
    assert!(rang, "the ring is in the activity: {events} {devices}");
    let (_, devices) = c.call(reqwest::Method::GET, "/api/devices", None).await;
    let bell = find(&devices, "Sonnette").unwrap();
    assert_eq!((bell["state"]["last_event"].clone(), bell["state"]["last_press"].clone()), (json!("Appel"), json!("single")));

    // Its picture, decrypted with the code of the cameras.
    let resp = c.http.get(format!("{}/api/devices/{}/picture", c.base, bell["id"])).bearer_auth(c.token.as_deref().unwrap()).send().await.unwrap();
    assert_eq!((resp.status(), resp.headers()["content-type"].to_str().unwrap().to_string()), (StatusCode::OK, "image/jpeg".to_string()));
    assert_eq!(resp.bytes().await.unwrap().as_ref(), b"\xff\xd8 la photo \xff\xd9");
    // None yet for the camera.
    let jardin = find(&devices, "Jardin").unwrap();
    let (s, _) = c.call(reqwest::Method::GET, &format!("/api/devices/{}/picture", jardin["id"]), None).await;
    assert_eq!(s, StatusCode::CONFLICT);
}
