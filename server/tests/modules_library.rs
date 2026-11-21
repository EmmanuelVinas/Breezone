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

fn sign_with(key: &SigningKey, message: &[u8]) -> Vec<u8> {
    STANDARD.encode(key.sign(message).to_bytes()).into_bytes()
}

fn sign(message: &[u8]) -> Vec<u8> {
    sign_with(&key(), message)
}

/// A module `id` that is only data (no code), at `version`.
fn manifest_of(id: &str, version: &str, api: &str) -> breezone_server::modules::manifest::Manifest {
    let mut m: breezone_server::modules::manifest::Manifest = serde_json::from_str(include_str!("../contracts/module.example.json")).unwrap();
    m.id = id.into();
    m.version = version.into();
    m.requires.api = api.into();
    m.requires.breezone = None;
    m
}

fn manifest(version: &str, api: &str) -> breezone_server::modules::manifest::Manifest {
    manifest_of("plugdemo", version, api)
}

/// Publishes in `files` the modules given (id, version, interface), signed by `key`, with an index
/// numbered `seq`.
fn publish_with(files: &Files, key: &SigningKey, seq: u64, modules: &[(&str, &str, &str)]) {
    let mut entries: Vec<Entry> = vec![];
    let mut out = files.lock().unwrap();
    for (id, version, api) in modules {
        let m = manifest_of(id, version, api);
        let bytes = package::build(&serde_json::to_string(&m).unwrap(), None, &[("i18n/fr.json", b"{}")]).unwrap();
        let file = format!("{id}-{version}.bzm");
        out.insert(format!("{file}.sig"), sign_with(key, &package::signed_message(id, version, &bytes)));
        let published = Published { manifest: m, file: file.clone(), size: bytes.len() as u64, sha256: String::new(), published_at: 0, yanked: false, code: false };
        match entries.iter_mut().find(|e| e.id == *id) {
            Some(e) => e.versions.push(published),
            None => entries.push(Entry { id: id.to_string(), versions: vec![published] }),
        }
        out.insert(file, bytes);
    }
    let index = serde_json::to_vec(&Index { seq, modules: entries }).unwrap();
    out.insert("index.json.sig".into(), sign_with(key, &library::signed_message(seq, &index)));
    out.insert("index.json".into(), index);
}

/// Publishes versions of `plugdemo` in the official library.
fn publish(files: &Files, seq: u64, versions: &[(&str, &str)]) {
    let modules: Vec<(&str, &str, &str)> = versions.iter().map(|(v, a)| ("plugdemo", *v, *a)).collect();
    publish_with(files, &key(), seq, &modules);
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
    let lib = lib["modules"].clone();
    assert_eq!(lib[0]["id"], "plugdemo");
    assert_eq!(lib[0]["repository"]["official"], true);
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
    let (code, lib) = server.call("GET", "/api/modules/library?refresh=true", None).await;
    assert_eq!(code, StatusCode::OK);
    assert!(lib["unreadable"][0]["error"].as_str().unwrap().contains("older"), "{lib}");
    publish(&files, 4, &[("1.1.0", "^1.0")]);
    files.lock().unwrap().get_mut("index.json").unwrap().extend_from_slice(b" ");
    let (_, lib) = server.call("GET", "/api/modules/library?refresh=true", None).await;
    assert!(lib["unreadable"][0]["error"].as_str().unwrap().contains("signature"), "{lib}");

    let (code, _) = server.call("DELETE", "/api/modules/plugdemo", None).await;
    assert_eq!(code, StatusCode::NO_CONTENT);
    let (_, list) = server.call("GET", "/api/modules", None).await;
    assert!(!list.as_array().unwrap().iter().any(|m| m["id"] == "plugdemo"));
    let (code, _) = server.call("DELETE", "/api/modules/shelly", None).await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "compiled in");
}

/// Sources of modules besides Breezone's (a developer's, a community's): each signs with its own
/// key, lists its modules in its index, and may neither replace a module of Breezone nor one that
/// came from another source.
#[tokio::test]
async fn other_sources_of_modules() {
    let official: Files = Arc::default();
    publish(&official, 1, &[("1.0.0", "^1.0")]);
    let server = Server::start(serve_library(official.clone()).await).await;
    let other_key = SigningKey::from_bytes(&[8; 32]);
    let other: Files = Arc::default();
    publish_with(&other, &other_key, 1, &[("chauffeau", "0.3.0", "^1.0"), ("plugdemo", "9.0.0", "^1.0"), ("shelly", "9.0.0", "^1.0")]);
    let other_url = serve_library(other.clone()).await;
    let other_public = STANDARD.encode(other_key.verifying_key().to_bytes());

    // Refused: not on the local network in HTTP, a key that is not one, a key that does not sign it.
    for (url, key) in [("http://modules.example.com", other_public.as_str()), (other_url.as_str(), "abc"), (other_url.as_str(), &STANDARD.encode([1u8; 32]))] {
        let (code, _) = server.call("POST", "/api/modules/repositories", Some(json!({"name": "Autre", "url": url, "public_key": key}))).await;
        assert!(code == StatusCode::BAD_REQUEST || code == StatusCode::UNPROCESSABLE_ENTITY, "{url} {key}: {code}");
    }
    let (code, repos) = server.call("POST", "/api/modules/repositories", Some(json!({"name": "Atelier", "url": other_url, "public_key": other_public}))).await;
    assert_eq!(code, StatusCode::CREATED, "{repos}");
    let atelier = repos.as_array().unwrap().iter().find(|r| r["name"] == "Atelier").unwrap()["id"].as_i64().unwrap();

    // Its modules are listed with it, apart from the official ones.
    let (_, lib) = server.call("GET", &format!("/api/modules/library?repository={atelier}"), None).await;
    let ids: Vec<&str> = lib["modules"].as_array().unwrap().iter().map(|m| m["id"].as_str().unwrap()).collect();
    assert_eq!(ids.len(), 3, "{lib}");
    assert!(lib["modules"].as_array().unwrap().iter().all(|m| m["repository"]["official"] == false));

    let (code, m) = server.call("POST", "/api/modules", Some(json!({"id": "chauffeau", "repository": atelier}))).await;
    assert_eq!(code, StatusCode::CREATED, "{m}");
    // A module of Breezone, or one installed from Breezone's library, is not taken by another source.
    let (code, _) = server.call("POST", "/api/modules", Some(json!({"id": "shelly", "repository": atelier}))).await;
    assert_eq!(code, StatusCode::CONFLICT);
    let (code, _) = server.call("POST", "/api/modules", Some(json!({"id": "plugdemo"}))).await;
    assert_eq!(code, StatusCode::CREATED);
    let (code, _) = server.call("POST", "/api/modules", Some(json!({"id": "plugdemo", "repository": atelier}))).await;
    assert_eq!(code, StatusCode::CONFLICT);

    // A source a module comes from is not removed; switched off, it is no longer read.
    let (code, _) = server.call("DELETE", &format!("/api/modules/repositories/{atelier}"), None).await;
    assert_eq!(code, StatusCode::CONFLICT);
    let (code, _) = server.call("PATCH", &format!("/api/modules/repositories/{atelier}"), Some(json!({"enabled": false}))).await;
    assert_eq!(code, StatusCode::OK);
    let (_, lib) = server.call("GET", "/api/modules/library", None).await;
    assert!(lib["modules"].as_array().unwrap().iter().all(|m| m["repository"]["official"] == true), "{lib}");
    let (code, _) = server.call("DELETE", "/api/modules/chauffeau", None).await;
    assert_eq!(code, StatusCode::NO_CONTENT);
    let (code, _) = server.call("DELETE", &format!("/api/modules/repositories/{atelier}"), None).await;
    assert_eq!(code, StatusCode::NO_CONTENT);
    let (code, _) = server.call("DELETE", "/api/modules/repositories/1", None).await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "the library of Breezone stays");
}
