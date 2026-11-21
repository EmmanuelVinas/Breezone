//! Modules in WebAssembly (docs/roadmap/modules.md, lot L0): the Tasmota module drives a fake
//! Tasmota like the native driver, and a module that misbehaves only puts its own source in error.
//!
//! Run with `cargo test --features wasm --test wasm_modules`; the modules are built first
//! (`modules/`, target `wasm32-wasip2`).
#![cfg(feature = "wasm")]

use axum::extract::Query;
use axum::routing::get;
use axum::{Json, Router};
use breezone_server::drivers::{Driver, DriverError};
use breezone_server::modules::wasm::{Permissions, WasmModule};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

fn modules_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../modules")
}

/// Builds the modules once, and gives the bytes of one.
fn wasm(name: &str) -> Vec<u8> {
    static BUILT: OnceLock<()> = OnceLock::new();
    BUILT.get_or_init(|| {
        let status = std::process::Command::new(env!("CARGO"))
            .args(["build", "--release", "--target", "wasm32-wasip2"])
            .current_dir(modules_dir())
            .status()
            .expect("cargo build of the modules");
        assert!(status.success(), "the modules do not build");
    });
    std::fs::read(modules_dir().join(format!("target/wasm32-wasip2/release/breezone_module_{name}.wasm"))).expect("module built")
}

/// The tests of this file run one at a time: one keeps a core busy on purpose (an endless loop of
/// a module, 10 s), which would make the others miss their short network deadlines.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn local() -> Permissions {
    Permissions { lan: true, hosts: vec![], loopback: true, memory_mb: 16 }
}

/// A Tasmota with a dimmer on relay 1, a plug on relay 2, an energy meter and a sensor; it keeps
/// the commands it is sent.
async fn fake_tasmota() -> (String, Arc<Mutex<Vec<String>>>) {
    let commands = Arc::new(Mutex::new(vec![]));
    let seen = commands.clone();
    let app = Router::new().route(
        "/cm",
        get(move |Query(q): Query<HashMap<String, String>>| {
            let seen = seen.clone();
            async move {
                let cmnd = q.get("cmnd").cloned().unwrap_or_default();
                seen.lock().unwrap().push(cmnd);
                Json(json!({
                    "Status": {"DeviceName": "Salon", "FriendlyName": ["Lampe", "Prise"]},
                    "StatusSTS": {"POWER1": "ON", "POWER2": "OFF", "Dimmer": 40, "CT": 250},
                    "StatusSNS": {"ENERGY": {"Power": 52, "Voltage": 231, "Total": 12.5}, "AM2301": {"Temperature": 21.4, "Humidity": 48}},
                    "StatusFWR": {"Version": "14.2.0(tasmota)", "Hardware": "ESP8266EX"},
                    "StatusNET": {"Mac": "AA:BB:CC:DD:EE:FF"}
                }))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    // A first connection, with time to spare: right after a build, the first one to a listener of
    // a new binary can take seconds on macOS, beyond the deadlines of the module.
    let _ = reqwest::Client::new().get(format!("http://{addr}/cm?cmnd=Status%200")).timeout(std::time::Duration::from_secs(30)).send().await;
    commands.lock().unwrap().clear();
    (addr.to_string(), commands)
}

#[tokio::test]
async fn the_tasmota_module_describes_reads_and_drives() {
    let _serial = SERIAL.lock().await;
    let (host, commands) = fake_tasmota().await;
    let cache = tempfile_dir("cache");
    let started = Instant::now();
    let module = Arc::new(WasmModule::load("tasmota", &wasm("tasmota"), local(), Some(&cache)).unwrap());
    let compiled = started.elapsed();
    let started = Instant::now();
    let _again = WasmModule::load("tasmota", &wasm("tasmota"), local(), Some(&cache)).unwrap();
    eprintln!("compiled in {compiled:?}, from the cache in {:?}", started.elapsed());

    assert_eq!(module.probe(&host).await.as_deref(), Some("Salon"));
    let source = module.open(&json!({ "host": host }), None).await.unwrap();
    let d = source.describe().await.unwrap();
    assert_eq!(d.model, "Tasmota · ESP8266EX");
    assert_eq!(d.identity.as_deref(), Some("AA:BB:CC:DD:EE:FF"));
    let kinds: Vec<(&str, &str)> = d.devices.iter().map(|x| (x.reference.as_str(), x.kind.as_str())).collect();
    assert_eq!(kinds, [("power1", "light"), ("power2", "switch.relay"), ("energy", "energy.power"), ("sensor/AM2301", "sensor")]);

    let s = source.read().await.unwrap();
    assert_eq!((s["power1"]["on"].clone(), s["power1"]["brightness"].clone(), s["power1"]["color_temp"].clone()), (json!(true), json!(40), json!(4000)));
    assert_eq!(s["energy"]["energy_in"], json!(12.5));

    let params = json!({ "on": false }).as_object().unwrap().clone();
    source.act("power2", "switch.relay", "set", &params).await.unwrap();
    assert_eq!(commands.lock().unwrap().last().map(String::as_str), Some("Power2 Off"));
    assert!(matches!(source.act("energy", "switch.relay", "set", &params).await, Err(DriverError::Unsupported(_))));
}

#[tokio::test]
async fn a_module_reaches_only_what_it_is_allowed() {
    let _serial = SERIAL.lock().await;
    let (host, _) = fake_tasmota().await;
    let no_loopback = Permissions { loopback: false, ..local() };
    let module = Arc::new(WasmModule::load("tasmota", &wasm("tasmota"), no_loopback, None).unwrap());
    let source = module.open(&json!({ "host": host }), None).await.unwrap();
    let err = source.read().await.unwrap_err();
    assert!(err.to_string().contains("not allowed"), "{err}");

    // Neither the Internet unless declared, nor a name that leads to a refused address.
    let misbehave = Arc::new(WasmModule::load("misbehave", &wasm("misbehave"), Permissions { loopback: false, ..local() }, None).unwrap());
    for url in ["http://1.1.1.1/", "http://localhost:9/", "file:///etc/passwd"] {
        let s = misbehave.open(&json!(format!("http:{url}")), None).await.unwrap();
        let err = s.read().await.unwrap_err();
        assert!(matches!(err, DriverError::Unreachable(_)), "{url}: {err}");
    }
}

#[tokio::test]
async fn a_module_that_misbehaves_only_fails_its_source() {
    let _serial = SERIAL.lock().await;
    let module = Arc::new(WasmModule::load("misbehave", &wasm("misbehave"), local(), None).unwrap());
    for mode in ["panic", "loop", "memory"] {
        let s = module.open(&json!(mode), None).await.unwrap();
        let started = Instant::now();
        let err = s.read().await.unwrap_err();
        eprintln!("{mode}: {err} after {:?}", started.elapsed());
        assert!(matches!(err, DriverError::Invalid(_)), "{mode}: {err}");
        assert!(started.elapsed().as_secs() < 30, "{mode} stopped too late");
        // The instance is made again for the next call: it fails the same way, without harm.
        assert!(s.read().await.is_err());
    }
    let fine = module.open(&json!("fine"), None).await.unwrap();
    assert_eq!(fine.read().await.unwrap()["ok"]["on"], Value::Bool(true));
}

#[tokio::test]
async fn sources_cost_little_memory() {
    let _serial = SERIAL.lock().await;
    let (host, _) = fake_tasmota().await;
    let module = Arc::new(WasmModule::load("tasmota", &wasm("tasmota"), local(), None).unwrap());
    let before = resident_kb();
    let mut sources = vec![];
    for _ in 0..20 {
        let s = module.open(&json!({ "host": host }), None).await.unwrap();
        s.read().await.unwrap();
        sources.push(s);
    }
    let after = resident_kb();
    if let (Some(b), Some(a)) = (before, after) {
        eprintln!("20 sources: {} kB more resident memory ({} kB each)", a.saturating_sub(b), a.saturating_sub(b) / 20);
    }
}

fn resident_kb() -> Option<u64> {
    let out = std::process::Command::new("ps").args(["-o", "rss=", "-p", &std::process::id().to_string()]).output().ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

fn tempfile_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("breezone-wasm-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// From the library to a source (lots L4 and L5): a module in WebAssembly is installed through the
/// API from a signed library, then a source of it reads and drives the hardware like a native one.
#[tokio::test]
async fn an_installed_module_runs_its_sources() {
    let _serial = SERIAL.lock().await;
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use breezone_server::modules::library::{self, Entry, Index, Published};
    use breezone_server::modules::package;
    use ed25519_dalek::{Signer, SigningKey};

    breezone_server::modules::wasm::allow_loopback(true);
    let key = SigningKey::from_bytes(&[5; 32]);
    let sign = |m: &[u8]| STANDARD.encode(key.sign(m).to_bytes()).into_bytes();
    // The Tasmota module under an id of its own (the native one keeps `tasmota`).
    let mut manifest: breezone_server::modules::manifest::Manifest = serde_json::from_str(include_str!("../../modules/tasmota/manifest.json")).unwrap();
    manifest.id = "tasmotawasm".into();
    let bytes = package::build(&serde_json::to_string(&manifest).unwrap(), Some(&wasm("tasmota")), &[]).unwrap();
    let mut files: HashMap<String, Vec<u8>> = HashMap::new();
    files.insert("tasmotawasm-1.0.0.bzm.sig".into(), sign(&package::signed_message("tasmotawasm", "1.0.0", &bytes)));
    let published = Published { manifest, file: "tasmotawasm-1.0.0.bzm".into(), size: bytes.len() as u64, sha256: String::new(), published_at: 0, yanked: false, code: true };
    files.insert("tasmotawasm-1.0.0.bzm".into(), bytes);
    let index = serde_json::to_vec(&Index { seq: 1, modules: vec![Entry { id: "tasmotawasm".into(), versions: vec![published] }] }).unwrap();
    files.insert("index.json.sig".into(), sign(&library::signed_message(1, &index)));
    files.insert("index.json".into(), index);
    let files = Arc::new(files);
    let app = Router::new().route(
        "/{file}",
        get(move |axum::extract::Path(f): axum::extract::Path<String>| {
            let files = files.clone();
            async move { files.get(&f).cloned().ok_or(axum::http::StatusCode::NOT_FOUND) }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let lib = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let dir = tempfile_dir("e2e");
    let mut cfg = breezone_server::config::Config::for_tests(dir);
    cfg.modules_library = Some((lib, STANDARD.encode(key.verifying_key().to_bytes())));
    let state = breezone_server::init_state(cfg).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, breezone_server::app(state)).await.unwrap() });
    let http = reqwest::Client::new();
    let call = |method: &str, path: &str, body: Option<Value>, token: Option<String>| {
        let mut req = http.request(method.parse().unwrap(), format!("{base}{path}"));
        if let Some(t) = token {
            req = req.bearer_auth(t);
        }
        if let Some(b) = body {
            req = req.json(&b);
        }
        async move {
            let resp = req.send().await.unwrap();
            let status = resp.status().as_u16();
            (status, serde_json::from_str::<Value>(&resp.text().await.unwrap()).unwrap_or(Value::Null))
        }
    };
    let (_, v) = call("POST", "/api/setup", Some(json!({"username":"admin","password":"motdepasse1"})), None).await;
    let t = || Some(v["access_token"].as_str().unwrap().to_string());

    let (code, m) = call("POST", "/api/modules", Some(json!({"id": "tasmotawasm"})), t()).await;
    assert_eq!(code, 201, "{m}");
    let (_, drivers) = call("GET", "/api/drivers", None, t()).await;
    assert!(drivers.as_array().unwrap().iter().any(|d| d["id"] == "tasmotawasm"), "its form is offered");

    let (host, commands) = fake_tasmota().await;
    let (code, s) = call("POST", "/api/sources", Some(json!({"driver": "tasmotawasm", "name": "Salon", "config": {"host": host}})), t()).await;
    assert_eq!(code, 201, "{s}");
    assert_eq!(s["status"]["state"], "online", "{s}");
    let (_, devices) = call("GET", "/api/devices", None, t()).await;
    let relay = devices.as_array().unwrap().iter().find(|d| d["source"] == "tasmotawasm" && d["reference"] == "power2").unwrap().clone();
    assert_eq!(relay["state"]["on"], false);
    let (code, r) = call("POST", &format!("/api/devices/{}/actions", relay["id"]), Some(json!({"trait": "switch.relay", "action": "set", "params": {"on": true}})), t()).await;
    assert_eq!(code, 200, "{r}");
    assert!(commands.lock().unwrap().contains(&"Power2 On".to_string()));
    let (_, modules) = call("GET", "/api/modules", None, t()).await;
    assert!(modules.as_array().unwrap().iter().any(|m| m["id"] == "tasmotawasm" && m["loaded"] == true));
}
