//! The library of modules (docs/roadmap/modules.md, lot L4): a fake library signed with a key of
//! the tests, from which modules are installed, updated, rolled back and removed through the API;
//! what is not signed, or older than what was seen, is refused.

use axum::extract::{Path, State};
use axum::http::StatusCode as Code;
use axum::routing::get;
use axum::Router;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use breezone_server::config::Config;
use breezone_server::modules::library::{self, Entry, Index, Published};
use breezone_server::modules::package;
use ed25519_dalek::{Signer, SigningKey};
use reqwest::StatusCode;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

type Files = Arc<Mutex<HashMap<String, Vec<u8>>>>;

fn key() -> SigningKey {
    SigningKey::from_bytes(&[3; 32])
}

fn sign(message: &[u8]) -> Vec<u8> {
    STANDARD.encode(key().sign(message).to_bytes()).into_bytes()
}

/// A module that is only data (no code), at `version`.
fn manifest(version: &str, api: &str) -> breezone_server::modules::manifest::Manifest {
    let mut m: breezone_server::modules::manifest::Manifest = serde_json::from_str(include_str!("../contracts/module.example.json")).unwrap();
    m.id = "plugdemo".into();
    m.version = version.into();
    m.requires.api = api.into();
    m.requires.breezone = None;
    m
}

/// Publishes the versions given in `files`, with an index numbered `seq`.
fn publish(files: &Files, seq: u64, versions: &[(&str, &str)]) {
    let mut published = vec![];
    let mut out = files.lock().unwrap();
    for (version, api) in versions {
        let m = manifest(version, api);
        let bytes = package::build(&serde_json::to_string(&m).unwrap(), None, &[("i18n/fr.json", b"{}")]).unwrap();
        let file = format!("plugdemo-{version}.bzm");
        out.insert(format!("{file}.sig"), sign(&package::signed_message("plugdemo", version, &bytes)));
        published.push(Published { manifest: m, file: file.clone(), size: bytes.len() as u64, sha256: String::new(), published_at: 0, yanked: false, code: false });
        out.insert(file, bytes);
    }
    let index = serde_json::to_vec(&Index { seq, modules: vec![Entry { id: "plugdemo".into(), versions: published }] }).unwrap();
    out.insert("index.json.sig".into(), sign(&library::signed_message(seq, &index)));
    out.insert("index.json".into(), index);
}

async fn serve_library(files: Files) -> String {
    let app = Router::new()
        .route("/{file}", get(|State(f): State<Files>, Path(file): Path<String>| async move { f.lock().unwrap().get(&file).cloned().ok_or(Code::NOT_FOUND) }))
        .with_state(files);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

struct Server {
    base: String,
    http: reqwest::Client,
    token: String,
}

impl Server {
    async fn start(library: String) -> Server {
        let dir = std::env::temp_dir().join(format!("breezone-library-{}-{}", std::process::id(), library.rsplit(':').next().unwrap()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut cfg = Config::for_tests(dir);
        cfg.modules_library = Some((library, STANDARD.encode(key().verifying_key().to_bytes())));
        let state = breezone_server::init_state(cfg).await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, breezone_server::app(state)).await.unwrap() });
        let http = reqwest::Client::new();
        let v: Value = http.post(format!("{base}/api/setup")).json(&json!({"username":"admin","password":"motdepasse1"})).send().await.unwrap().json().await.unwrap();
        Server { base, http, token: v["access_token"].as_str().unwrap().to_string() }
    }

    async fn call(&self, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut req = self.http.request(method.parse().unwrap(), format!("{}{path}", self.base)).bearer_auth(&self.token);
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await.unwrap();
        let status = resp.status();
        (status, serde_json::from_str(&resp.text().await.unwrap()).unwrap_or(Value::Null))
    }
}

#[tokio::test]
async fn modules_come_from_the_signed_library() {
    let files: Files = Arc::default();
    // 2.0 needs an interface no server has: 1.1 is the one that fits.
    publish(&files, 3, &[("2.0.0", "^9.0"), ("1.1.0", "^1.0"), ("1.0.0", "^1.0")]);
    let server = Server::start(serve_library(files.clone()).await).await;

    let (code, lib) = server.call("GET", "/api/modules/library", None).await;
    assert_eq!(code, StatusCode::OK, "{lib}");
    assert_eq!(lib[0]["id"], "plugdemo");
    assert_eq!(lib[0]["manifest"]["version"], "2.0.0");
    assert_eq!(lib[0]["fits"], "1.1.0");
    assert_eq!(lib[0]["reasons"][0]["kind"], "api");
    assert_eq!(lib[0]["installed"], Value::Null);

    // An older version asked by name, then the one that fits.
    let (code, m) = server.call("POST", "/api/modules", Some(json!({"id": "plugdemo", "version": "1.0.0"}))).await;
    assert_eq!(code, StatusCode::CREATED, "{m}");
    assert_eq!((m["version"].clone(), m["origin"].clone(), m["loaded"].clone()), (json!("1.0.0"), json!("library"), json!(false)));
    let (code, m) = server.call("POST", "/api/modules", Some(json!({"id": "plugdemo"}))).await;
    assert_eq!(code, StatusCode::CREATED, "{m}");
    assert_eq!(m["version"], "1.1.0");
    let (code, _) = server.call("POST", "/api/modules", Some(json!({"id": "plugdemo", "version": "2.0.0"}))).await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "an interface this server does not host");

    // Announced to the clients and the other nodes.
    let (_, caps) = server.call("GET", "/api/capabilities", None).await;
    assert!(caps["modules"].as_array().unwrap().iter().any(|m| m["id"] == "plugdemo" && m["version"] == "1.1.0"), "{caps}");

    // Before updating Breezone: a version that lacks the traits it gives would not run it.
    let host = breezone_server::capabilities::Host::local();
    files.lock().unwrap().insert("0.99.0-host.json".into(), serde_json::to_vec(&host).unwrap());
    let mut poorer = host.clone();
    poorer.traits.remove("switch.relay");
    files.lock().unwrap().insert("0.98.0-host.json".into(), serde_json::to_vec(&poorer).unwrap());
    let (code, next) = server.call("GET", "/api/modules/next?version=0.99.0", None).await;
    assert_eq!(code, StatusCode::OK, "{next}");
    assert_eq!((next[0]["id"].clone(), next[0]["fits"].clone()), (json!("plugdemo"), json!(true)));
    let (_, next) = server.call("GET", "/api/modules/next?version=0.98.0", None).await;
    assert_eq!((next[0]["fits"].clone(), next[0]["reasons"][0]["kind"].clone(), next[0]["replacement"].clone()), (json!(false), json!("trait"), Value::Null), "{next}");

    let (code, m) = server.call("POST", "/api/modules/plugdemo/rollback", None).await;
    assert_eq!((code, m["version"].clone()), (StatusCode::OK, json!("1.0.0")));

    // A package sent by hand is signed like the others.
    let m2 = manifest("1.2.0", "^1.0");
    let bytes = package::build(&serde_json::to_string(&m2).unwrap(), None, &[]).unwrap();
    let forged = STANDARD.encode(SigningKey::from_bytes(&[4; 32]).sign(&package::signed_message("plugdemo", "1.2.0", &bytes)).to_bytes());
    let (code, _) = server.call("POST", "/api/modules/upload", Some(json!({"package": STANDARD.encode(&bytes), "signature": forged}))).await;
    assert_eq!(code, StatusCode::FORBIDDEN);
    let good = String::from_utf8(sign(&package::signed_message("plugdemo", "1.2.0", &bytes))).unwrap();
    let (code, m) = server.call("POST", "/api/modules/upload", Some(json!({"package": STANDARD.encode(&bytes), "signature": good}))).await;
    assert_eq!((code, m["origin"].clone(), m["version"].clone()), (StatusCode::CREATED, json!("file"), json!("1.2.0")), "{m}");

    // An index older than the one seen is refused; a tampered one too.
    publish(&files, 2, &[("1.1.0", "^1.0")]);
    let (code, _) = server.call("GET", "/api/modules/library?refresh=true", None).await;
    assert_eq!(code, StatusCode::BAD_GATEWAY);
    publish(&files, 4, &[("1.1.0", "^1.0")]);
    files.lock().unwrap().get_mut("index.json").unwrap().extend_from_slice(b" ");
    let (code, _) = server.call("GET", "/api/modules/library?refresh=true", None).await;
    assert_eq!(code, StatusCode::BAD_GATEWAY);

    let (code, _) = server.call("DELETE", "/api/modules/plugdemo", None).await;
    assert_eq!(code, StatusCode::NO_CONTENT);
    let (_, list) = server.call("GET", "/api/modules", None).await;
    assert!(!list.as_array().unwrap().iter().any(|m| m["id"] == "plugdemo"));
    let (code, _) = server.call("DELETE", "/api/modules/shelly", None).await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "compiled in");
}
