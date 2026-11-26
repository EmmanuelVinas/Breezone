//! Modules in WebAssembly (docs/roadmap/modules.md, lot L0): the Tasmota module drives a fake
//! Tasmota like the native driver, and a module that misbehaves only puts its own source in error.
//!
//! Run with `cargo test --features wasm --test wasm_modules`; the modules are built first
//! (`modules/`, target `wasm32-wasip2`).
#![cfg(feature = "wasm")]

mod support;

use axum::extract::Query;
use axum::response::IntoResponse;
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
    Permissions { lan: true, hosts: vec![], loopback: true, memory_mb: 16, accounts: vec![], oauth_token_url: None, account_fields: vec![] }
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
    let source = module.open(None, &json!({ "host": host }), None).await.unwrap();
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

/// The fake ZBBridge Pro of the demos, on a port of its own.
async fn fake_zigbee_bridge() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, breezone_server::mock_devices::zigbee_bridge()).await.unwrap() });
    let _ = reqwest::Client::new().get(format!("http://{addr}/cm?cmnd=Status%200")).timeout(std::time::Duration::from_secs(30)).send().await;
    addr.to_string()
}

#[tokio::test]
async fn the_tasmota_module_pairs_names_and_drives_zigbee_devices() {
    let _serial = SERIAL.lock().await;
    let host = fake_zigbee_bridge().await;
    let module = Arc::new(WasmModule::load("tasmota", &wasm("tasmota"), local(), None).unwrap().with_optional_calls(&["rename".into()]));
    assert_eq!(module.probe(&host).await.as_deref(), Some("Pont Zigbee"));
    let source = module.open(None, &json!({ "host": host }), None).await.unwrap();
    let d = source.describe().await.unwrap();
    let kinds: Vec<(&str, &str)> = d.devices.iter().map(|x| (x.reference.as_str(), x.kind.as_str())).collect();
    assert_eq!(
        kinds,
        [("zigbee", "zigbee.bridge"), ("zigbee/0x00124B0026B64D1E", "sensor"), ("zigbee/0x7CB03EBB0A0292DD", "switch.relay"), ("zigbee/0x0017880102AABBCC", "light")]
    );
    let s = source.read().await.unwrap();
    assert!(s["zigbee/0x00124B0026B64D1E"]["temperature"].is_number());
    assert_eq!(s["zigbee"]["devices"], 4);

    // Paired: a new sensor is read, which the host then describes.
    source.act("zigbee", "zigbee.bridge", "pair", json!({"on": true}).as_object().unwrap()).await.unwrap();
    let s = source.read().await.unwrap();
    assert_eq!(s["zigbee"]["pairing"], true);
    assert!(s.contains_key("zigbee/0x00124B0026B70011"));

    source.rename("zigbee/0x00124B0026B70011", "Salle de bains").await.unwrap();
    source.act("zigbee/0x0017880102AABBCC", "light", "set", json!({"on": true, "brightness": 50}).as_object().unwrap()).await.unwrap();
    let d = source.describe().await.unwrap();
    assert_eq!(d.devices.iter().find(|x| x.reference == "zigbee/0x00124B0026B70011").map(|x| x.name.as_str()), Some("Salle de bains"));
    let s = source.read().await.unwrap();
    assert_eq!((s["zigbee/0x0017880102AABBCC"]["on"].clone(), s["zigbee/0x0017880102AABBCC"]["brightness"].clone()), (json!(true), json!(50)));
}

#[tokio::test]
async fn a_module_reaches_only_what_it_is_allowed() {
    let _serial = SERIAL.lock().await;
    let (host, _) = fake_tasmota().await;
    let no_loopback = Permissions { loopback: false, ..local() };
    let module = Arc::new(WasmModule::load("tasmota", &wasm("tasmota"), no_loopback, None).unwrap());
    let source = module.open(None, &json!({ "host": host }), None).await.unwrap();
    let err = source.read().await.unwrap_err();
    assert!(err.to_string().contains("not allowed"), "{err}");

    // Neither the Internet unless declared, nor a name that leads to a refused address.
    let misbehave = Arc::new(WasmModule::load("misbehave", &wasm("misbehave"), Permissions { loopback: false, ..local() }, None).unwrap());
    for url in ["http://1.1.1.1/", "http://localhost:9/", "file:///etc/passwd"] {
        let s = misbehave.open(None, &json!(format!("http:{url}")), None).await.unwrap();
        let err = s.read().await.unwrap_err();
        assert!(matches!(err, DriverError::Unreachable(_)), "{url}: {err}");
    }
}

#[tokio::test]
async fn a_module_that_misbehaves_only_fails_its_source() {
    let _serial = SERIAL.lock().await;
    let module = Arc::new(WasmModule::load("misbehave", &wasm("misbehave"), local(), None).unwrap());
    for mode in ["panic", "loop", "memory"] {
        let s = module.open(None, &json!(mode), None).await.unwrap();
        let started = Instant::now();
        let err = s.read().await.unwrap_err();
        eprintln!("{mode}: {err} after {:?}", started.elapsed());
        assert!(matches!(err, DriverError::Invalid(_)), "{mode}: {err}");
        assert!(started.elapsed().as_secs() < 30, "{mode} stopped too late");
        // The instance is made again for the next call: it fails the same way, without harm.
        assert!(s.read().await.is_err());
    }
    let fine = module.open(None, &json!("fine"), None).await.unwrap();
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
        let s = module.open(None, &json!({ "host": host }), None).await.unwrap();
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
    manifest.version = "1.0.0".into();
    let files: Arc<Mutex<HashMap<String, Vec<u8>>>> = Arc::default();
    // Publishes the versions given (manifest, then the component) with an index numbered `seq`.
    let publish = |seq: u64, versions: Vec<breezone_server::modules::manifest::Manifest>| {
        let mut out = files.lock().unwrap();
        let mut published = vec![];
        for m in versions {
            let bytes = package::build(&serde_json::to_string(&m).unwrap(), Some(&wasm("tasmota")), &[]).unwrap();
            let file = format!("tasmotawasm-{}.bzm", m.version);
            out.insert(format!("{file}.sig"), sign(&package::signed_message("tasmotawasm", &m.version, &bytes)));
            published.push(Published { file: file.clone(), size: bytes.len() as u64, sha256: String::new(), published_at: 0, yanked: false, code: true, manifest: m });
            out.insert(file, bytes);
        }
        let index = serde_json::to_vec(&Index { seq, modules: vec![Entry { id: "tasmotawasm".into(), versions: published }] }).unwrap();
        out.insert("index.json.sig".into(), sign(&library::signed_message(seq, &index)));
        out.insert("index.json".into(), index);
    };
    publish(1, vec![manifest.clone()]);
    let served = files.clone();
    let app = Router::new().route(
        "/{file}",
        get(move |axum::extract::Path(f): axum::extract::Path<String>| {
            let files = served.clone();
            async move { files.lock().unwrap().get(&f).cloned().ok_or(axum::http::StatusCode::NOT_FOUND) }
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
    // Named in Breezone: the module takes the name too, without the answer waiting for it.
    let (code, _) = call("PUT", &format!("/api/devices/{}", relay["id"]), Some(json!({"name": "Télévision"})), t()).await;
    assert_eq!(code, 200);
    let renamed = || commands.lock().unwrap().contains(&"FriendlyName2 Télévision".to_string());
    for _ in 0..50 {
        if renamed() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(renamed(), "{:?}", commands.lock().unwrap());
    let (_, modules) = call("GET", "/api/modules", None, t()).await;
    assert!(modules.as_array().unwrap().iter().any(|m| m["id"] == "tasmotawasm" && m["loaded"] == true));

    // Found by the network scan through its own probe (port 80 said open: the fake listens elsewhere).
    // (Other tests of this file may have installed the shipped Tasmota, which knows it too.)
    let found = breezone_server::modules::identify_at(&host, &[80]).await;
    let mine: Vec<_> = found.iter().filter(|p| p.driver == "tasmotawasm").map(|p| (p.model.clone(), p.note.clone())).collect();
    assert_eq!(mine, [("Tasmota".to_string(), Some("« Salon »".to_string()))]);

    // Its log says what went wrong: an address its permissions refuse.
    let (code, _) = call("POST", "/api/sources", Some(json!({"driver": "tasmotawasm", "name": "Dehors", "config": {"host": "1.1.1.1"}})), t()).await;
    assert_eq!(code, 201);
    let (code, log) = call("GET", "/api/modules/tasmotawasm/log", None, t()).await;
    assert_eq!(code, 200);
    assert!(log.as_array().unwrap().iter().any(|l| l["level"] == "warn" && l["message"].as_str().unwrap_or_default().contains("not allowed")), "{log}");
    call("DELETE", "/api/sources/2", None, t()).await;

    // An update whose sources do not read their hardware (1 MB of memory: it cannot even start)
    // goes back to the version before, and is not proposed again.
    let mut broken = manifest.clone();
    broken.version = "1.1.0".into();
    broken.permissions.memory_mb = 1;
    publish(2, vec![broken, manifest.clone()]);
    call("GET", "/api/modules/library?refresh=true", None, t()).await;
    let (code, m) = call("POST", "/api/modules", Some(json!({"id": "tasmotawasm", "version": "1.1.0"})), t()).await;
    assert_eq!(code, 201, "{m}");
    assert_eq!(m["version"], "1.0.0", "back to the version before: {m}");
    assert!(m["last_error"].as_str().unwrap_or_default().contains("1.1.0"), "{m}");
    let (code, _) = call("POST", "/api/modules", Some(json!({"id": "tasmotawasm", "version": "1.1.0"})), t()).await;
    assert_eq!(code, 409, "a version that failed here is not installed again");
    let (_, s) = call("GET", "/api/sources", None, t()).await;
    assert_eq!(s.as_array().unwrap().iter().find(|x| x["driver"] == "tasmotawasm").unwrap()["status"]["state"], "online", "{s}");
}

/// What the hardware says without being asked (lot L5): the host keeps a WebSocket for the module,
/// hands it each message, and passes on the presses and the « read me again » it asks for; the
/// store of a source outlives its instance; an account it does not require stays closed.
#[tokio::test]
async fn subscriptions_store_and_accounts() {
    use breezone_server::drivers::{Heard, Sink};
    use futures_util::{SinkExt, StreamExt};
    let _serial = SERIAL.lock().await;
    // A WebSocket that greets, then answers each frame.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let received = Arc::new(Mutex::new(vec![]));
    let seen = received.clone();
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let seen = seen.clone();
            tokio::spawn(async move {
                let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
                ws.send(tokio_tungstenite::tungstenite::Message::Text("hello".into())).await.unwrap();
                while let Some(Ok(m)) = ws.next().await {
                    seen.lock().unwrap().push(m.to_text().unwrap_or_default().to_string());
                }
            });
        }
    });
    let module = Arc::new(WasmModule::load("misbehave", &wasm("misbehave"), local(), None).unwrap());
    let source = module.open(Some(7), &json!(format!("sub:ws://{addr}/rpc")), None).await.unwrap();
    let (tx, mut heard) = tokio::sync::mpsc::unbounded_channel();
    source.listen(Sink::new(7, tx));
    let mut events = vec![];
    while events.len() < 2 {
        events.push(tokio::time::timeout(std::time::Duration::from_secs(10), heard.recv()).await.expect("the module heard nothing").unwrap());
    }
    assert!(events.iter().any(|h| matches!(h, Heard::Press(p) if p.source_id == 7 && p.reference == "remote" && p.press == "double")), "{events:?}");
    assert!(events.contains(&Heard::Changed(7)), "{events:?}");
    assert_eq!(source.read().await.unwrap()["sub"]["last"], "hello");
    // What it sends goes out on the WebSocket.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(received.lock().unwrap().first().map(String::as_str), Some("ping"));

    // A subscription to an address it may not reach is refused at `open`.
    let strict = Arc::new(WasmModule::load("misbehave", &wasm("misbehave"), Permissions { loopback: false, ..local() }, None).unwrap());
    assert!(strict.open(None, &json!(format!("sub:ws://{addr}/rpc")), None).await.is_err());

    // The store goes on after the instance is made again (a panic in between).
    let counter = module.open(None, &json!("store"), None).await.unwrap();
    assert_eq!(counter.read().await.unwrap()["store"]["n"], 1);
    assert_eq!(counter.read().await.unwrap()["store"]["n"], 2);

    // An account the manifest does not require is not given.
    let curious = module.open(None, &json!("account:netatmo"), None).await.unwrap();
    assert!(matches!(curious.read().await, Err(DriverError::Unsupported(e)) if e.contains("not an account this module requires")));
}

/// A WebSocket in TLS (`wss://`): a device of the local network with its own certificate is
/// heard; the usual checks, for a service on the Internet, refuse that certificate.
#[tokio::test]
async fn a_secure_websocket_is_heard() {
    use breezone_server::drivers::{Heard, Sink};
    use futures_util::SinkExt;
    let _serial = SERIAL.lock().await;
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    let key = rustls::pki_types::PrivateKeyDer::Pkcs8(cert.signing_key.serialize_der().into());
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert.cert.der().clone()], key)
        .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(tls) = acceptor.accept(tcp).await else { return };
                let mut ws = tokio_tungstenite::accept_async(tls).await.unwrap();
                ws.send(tokio_tungstenite::tungstenite::Message::Text("hello".into())).await.unwrap();
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            });
        }
    });
    let module = Arc::new(WasmModule::load("misbehave", &wasm("misbehave"), local(), None).unwrap());
    let source = module.open(Some(8), &json!(format!("sub:wss://localhost:{port}/rpc")), None).await.unwrap();
    let (tx, mut heard) = tokio::sync::mpsc::unbounded_channel();
    source.listen(Sink::new(8, tx));
    loop {
        let h = tokio::time::timeout(std::time::Duration::from_secs(10), heard.recv()).await.expect("the module heard nothing").unwrap();
        if h == Heard::Changed(8) {
            break;
        }
    }
    assert_eq!(source.read().await.unwrap()["sub"]["last"], "hello");
    // The usual checks (a service on the Internet) refuse that certificate.
    let usual = tokio_rustls::TlsConnector::from(Arc::new(breezone_server::tls::client_config(false).unwrap()));
    let tcp = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    assert!(usual.connect(rustls::pki_types::ServerName::try_from("localhost").unwrap(), tcp).await.is_err());
}

/// The appairage of a module (`pair`): waiting for its button, then the source to add.
#[tokio::test]
async fn a_module_pairs() {
    use breezone_server::modules::Paired;
    let _serial = SERIAL.lock().await;
    let module = Arc::new(WasmModule::load("misbehave", &wasm("misbehave"), local(), None).unwrap());
    assert!(matches!(module.pair("link", &json!({"host": "192.168.1.9"})).await, Ok(Paired::Waiting(m)) if m == "press the button"));
    match module.pair("link", &json!({"host": "192.168.1.9", "pressed": true})).await {
        Ok(Paired::Done { config, secret, name }) => assert_eq!((config, secret.as_str(), name.as_str()), (json!("store"), "key", "Misbehave")),
        other => panic!("{other:?}"),
    }
    assert!(matches!(module.pair("finish", &json!({})).await, Err(DriverError::Unsupported(_))));
}

/// A module of the `service` role: its account checked before it is kept, its answers to the
/// queries of a contract checked against the contract.
#[tokio::test]
async fn a_service_module_answers_its_contract() {
    let _serial = SERIAL.lock().await;
    let permissions = Permissions { accounts: vec!["misbehave".into()], ..local() };
    let module = Arc::new(WasmModule::load("misbehave", &wasm("misbehave"), permissions, None).unwrap());
    module.check_account(r#"{"token": "red"}"#).await.unwrap();
    assert!(matches!(module.check_account(r#"{"token": "wrong"}"#).await, Err(DriverError::Invalid(m)) if m == "refused"));
    let colors = module.query("tariff.day_color@1", "colors", &json!({"from": "2026-11-22", "until": "2026-11-22"})).await.unwrap();
    // Its colour depends on the account kept by whichever server another test started last.
    assert_eq!(colors[0]["day"], "2026-11-22", "{colors}");
    // An answer that is not what the contract says is refused.
    assert!(matches!(module.query("tariff.day_color@1", "colors", &json!({"from": "bad"})).await, Err(DriverError::Invalid(m)) if m.contains("color")));
    assert!(matches!(module.query("weather.forecast@1", "daily", &json!({})).await, Err(DriverError::Unsupported(_))));
}

/// The access of an OAuth account (`host.access-token`): renewed by Breezone with the refresh
/// token, the new refresh token kept; one still valid is given as it is.
#[tokio::test]
async fn an_oauth_access_is_renewed_by_the_host() {
    let _serial = SERIAL.lock().await;
    let asked = Arc::new(Mutex::new(vec![]));
    let seen = asked.clone();
    let app = axum::Router::new().route(
        "/token",
        axum::routing::post(move |body: String| {
            let seen = seen.clone();
            async move {
                seen.lock().unwrap().push(body.clone());
                if body.contains("refresh_token=old") {
                    axum::Json(json!({"access_token": "fresh", "refresh_token": "new", "expires_in": 10800})).into_response()
                } else {
                    (axum::http::StatusCode::BAD_REQUEST, axum::Json(json!({"error": "invalid_grant"}))).into_response()
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let permissions = Permissions { accounts: vec!["misbehave".into()], oauth_token_url: Some(format!("http://{addr}/token")), ..local() };
    let module = Arc::new(WasmModule::load("misbehave", &wasm("misbehave"), permissions, None).unwrap());

    let kept: Value = serde_json::from_str(&module.check_account(r#"{"client_id": "id", "client_secret": "s", "refresh_token": "old"}"#).await.unwrap()).unwrap();
    assert_eq!((kept["access_token"].clone(), kept["refresh_token"].clone()), (json!("fresh"), json!("new")), "{kept}");
    assert!(kept["expires_at"].as_i64().unwrap() > breezone_server::db::now() + 10_000);
    assert!(asked.lock().unwrap()[0].contains("grant_type=refresh_token") && asked.lock().unwrap()[0].contains("client_id=id"));
    // Still valid: not renewed.
    let valid = json!({"refresh_token": "new", "access_token": "fresh", "expires_at": breezone_server::db::now() + 3600}).to_string();
    module.check_account(&valid).await.unwrap();
    assert_eq!(asked.lock().unwrap().len(), 1);
    // A refresh token the service refuses: the check fails.
    assert!(module.check_account(r#"{"refresh_token": "revoked"}"#).await.is_err());
}

/// The ESPHome module (lot L8): an encrypted node on a real connection, its handshake and listing
/// read during the call (`host.receive`), its states pushed afterwards (`on-message`), a relay
/// driven.
#[tokio::test]
async fn the_esphome_module_speaks_to_an_encrypted_node() {
    use breezone_server::esphome_wire::{decode, Fields, Msg};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let _serial = SERIAL.lock().await;
    let psk: [u8; 32] = std::array::from_fn(|i| i as u8 + 1);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut tcp, _) = listener.accept().await.unwrap();
        async fn frame(tcp: &mut tokio::net::TcpStream) -> Option<Vec<u8>> {
            let mut head = [0u8; 3];
            tcp.read_exact(&mut head).await.ok()?;
            let mut body = vec![0u8; usize::from(u16::from_be_bytes([head[1], head[2]]))];
            tcp.read_exact(&mut body).await.ok()?;
            Some(body)
        }
        // The empty hello frame, then the client's handshake message.
        let mut hello = [0u8; 3];
        tcp.read_exact(&mut hello).await.unwrap();
        let first = frame(&mut tcp).await.unwrap();
        let params: snow::params::NoiseParams = "Noise_NNpsk0_25519_ChaChaPoly_SHA256".parse().unwrap();
        let mut hs = snow::Builder::new(params).prologue(b"NoiseAPIInit\x00\x00").unwrap().psk(0, &psk).unwrap().build_responder().unwrap();
        let mut buf = vec![0u8; 256];
        hs.read_message(&first[1..], &mut buf).unwrap();
        let server_hello = b"\x01bureau\x00AA:BB:CC:DD:EE:FF\x00";
        tcp.write_all(&[0x01, 0, server_hello.len() as u8]).await.unwrap();
        tcp.write_all(server_hello).await.unwrap();
        let n = hs.write_message(&[], &mut buf).unwrap();
        tcp.write_all(&[0x01, 0, (n + 1) as u8, 0x00]).await.unwrap();
        tcp.write_all(&buf[..n]).await.unwrap();
        let mut t = hs.into_transport_mode().unwrap();
        fn seal(t: &mut snow::TransportState, kind: u16, m: Msg) -> Vec<u8> {
            let mut plain = kind.to_be_bytes().to_vec();
            plain.extend_from_slice(&(m.0.len() as u16).to_be_bytes());
            plain.extend_from_slice(&m.0);
            let mut cipher = vec![0u8; plain.len() + 16];
            let n = t.write_message(&plain, &mut cipher).unwrap();
            let mut f = vec![0x01, (n >> 8) as u8, n as u8];
            f.extend_from_slice(&cipher[..n]);
            f
        }
        let mut out: Vec<u8>;
        loop {
            let Some(cipher) = frame(&mut tcp).await else { return };
            let mut plain = vec![0u8; cipher.len()];
            // The same transport decrypts what comes.
            let n = match t.read_message(&cipher, &mut plain) {
                Ok(n) => n,
                Err(_) => return,
            };
            let kind = u16::from_be_bytes([plain[0], plain[1]]);
            let msg = plain[4..n].to_vec();
            out = vec![];
            match kind {
                1 => out.extend(seal(&mut t, 2, Msg::default().varint(1, 1).varint(2, 10).string(4, "bureau"))),
                9 => out.extend(seal(
                    &mut t,
                    10,
                    Msg::default().string(2, "bureau").string(3, "AA:BB:CC:DD:EE:FF").string(4, "2025.11.0").string(6, "esp32dev").string(13, "Bureau"),
                )),
                11 => {
                    out.extend(seal(&mut t, 17, Msg::default().string(1, "relay").fixed32(2, 1).string(3, "Prise")));
                    out.extend(seal(&mut t, 16, Msg::default().string(1, "t").fixed32(2, 3).string(3, "Température").string(6, "°C").string(9, "temperature")));
                    out.extend(seal(&mut t, 19, Msg::default()));
                }
                20 => {
                    out.extend(seal(&mut t, 26, Msg::default().fixed32(1, 1).bool(2, false)));
                    out.extend(seal(&mut t, 25, Msg::default().fixed32(1, 3).float(2, 19.5)));
                }
                33 => {
                    let f = Fields(decode(&msg));
                    out.extend(seal(&mut t, 26, Msg::default().fixed32(1, f.fixed32(1)).bool(2, f.bool(2))));
                }
                _ => {}
            }
            if !out.is_empty() && tcp.write_all(&out).await.is_err() {
                return;
            }
        }
    });
    let module = Arc::new(WasmModule::load("esphome", &wasm("esphome"), local(), None).unwrap());
    let secret = json!({"key": base64_encode(&psk)}).to_string();
    let node = module.open(Some(42), &json!({"host": addr.to_string()}), Some(&secret)).await.unwrap();
    let d = node.describe().await.unwrap();
    assert_eq!((d.model.as_str(), d.identity.as_deref()), ("ESPHome · esp32dev", Some("AA:BB:CC:DD:EE:FF")));
    assert_eq!(d.devices.iter().map(|x| x.reference.as_str()).collect::<Vec<_>>(), ["switch/relay", "air"]);
    let mut state = HashMap::new();
    for _ in 0..100 {
        state = node.read().await.unwrap();
        if state.contains_key("air") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    }
    assert_eq!((state["switch/relay"]["on"].clone(), state["air"]["temperature"].clone()), (json!(false), json!(19.5)));
    node.act("switch/relay", "switch.relay", "set", &serde_json::Map::from_iter([("on".to_string(), json!(true))])).await.unwrap();
    for _ in 0..100 {
        state = node.read().await.unwrap();
        if state["switch/relay"]["on"] == true {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    }
    assert_eq!(state["switch/relay"]["on"], true, "the node said it switched on");
}

fn base64_encode(b: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(b)
}

/// A module that comes with Breezone (lot L6): beside its web UI, unsigned (the image or the signed
/// archive vouches for it), installed at start, never removed (installed again if missing).
#[tokio::test]
async fn a_module_that_comes_with_breezone_is_installed_with_it() {
    use breezone_server::modules::package;
    let _serial = SERIAL.lock().await;
    breezone_server::modules::wasm::allow_loopback(true);
    let dir = tempfile_dir("builtin");
    std::fs::create_dir_all(dir.join("app/modules")).unwrap();
    let manifest = include_str!("../../modules/tasmota/manifest.json");
    std::fs::write(dir.join("app/modules/tasmota-1.0.1.bzm"), package::build(manifest, Some(&wasm("tasmota")), &[]).unwrap()).unwrap();
    let mut cfg = breezone_server::config::Config::for_tests(dir.join("data"));
    cfg.web_dir = dir.join("app/web");
    let state = breezone_server::init_state(cfg).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, breezone_server::app(state)).await.unwrap() });
    let http = reqwest::Client::new();
    let v: Value = http.post(format!("{base}/api/setup")).json(&json!({"username":"admin","password":"motdepasse1"})).send().await.unwrap().json().await.unwrap();
    let token = v["access_token"].as_str().unwrap().to_string();
    let call = |method: &str, path: &str, body: Option<Value>| {
        let mut req = http.request(method.parse().unwrap(), format!("{base}{path}")).bearer_auth(&token);
        if let Some(b) = body {
            req = req.json(&b);
        }
        async move {
            let resp = req.send().await.unwrap();
            (resp.status().as_u16(), serde_json::from_str::<Value>(&resp.text().await.unwrap()).unwrap_or(Value::Null))
        }
    };
    let tasmota = |modules: &Value| modules.as_array().unwrap().iter().find(|m| m["id"] == "tasmota").unwrap().clone();

    let (_, modules) = call("GET", "/api/modules", None).await;
    assert_eq!(tasmota(&modules)["origin"], "builtin", "{modules}");
    let (host, commands) = fake_tasmota().await;
    let (code, s) = call("POST", "/api/sources", Some(json!({"driver": "tasmota", "name": "Salon", "config": {"host": host}}))).await;
    assert_eq!((code, s["status"]["state"].clone()), (201, json!("online")), "{s}");
    let (_, modules) = call("GET", "/api/modules", None).await;
    assert_eq!(tasmota(&modules)["loaded"], true);

    assert!(commands.lock().unwrap().iter().any(|c| c == "Status 0"));

    // Part of Breezone: not removed, even once no source uses it.
    let (code, _) = call("DELETE", &format!("/api/sources/{}", s["id"]), None).await;
    assert_eq!(code, 204);
    let (code, _) = call("DELETE", "/api/modules/tasmota", None).await;
    assert_eq!(code, 400, "it comes with Breezone");
    // Missing at a start (a damaged install): installed again.
    breezone_server::modules::unregister("tasmota");
    let _ = std::fs::remove_dir_all(dir.join("data/modules/tasmota"));
    let mut cfg = breezone_server::config::Config::for_tests(dir.join("data"));
    cfg.web_dir = dir.join("app/web");
    breezone_server::init_state(cfg).await.unwrap();
    assert_eq!(breezone_server::modules::manifest("tasmota").map(|m| m.version.as_str()), Some("1.1.0"), "shipped with Breezone: installed again");
}

/// A service module that comes with Breezone, with an account (lot L9): its account is connected
/// from the fields of its manifest's form once the module accepts it, then the core asks it for
/// the colour of the days before its own code.
#[tokio::test]
async fn a_service_module_with_an_account_answers_the_core() {
    use breezone_server::modules::package;
    let _serial = SERIAL.lock().await;
    let dir = tempfile_dir("service");
    std::fs::create_dir_all(dir.join("app/modules")).unwrap();
    let manifest = json!({
        "id": "misbehave", "version": "1.0.0",
        "name": {"fr": "Couleurs", "en": "Colours", "es": "Colores"},
        "roles": ["account", "service"], "requires": {"api": "^1.0"},
        "provides": {"services": ["tariff.day_color@1.0", "conditions@1.0"]},
        "config": [{"name": "token", "type": "password", "label": {"fr": "Jeton", "en": "Token", "es": "Token"}}],
        "permissions": {"network": {"lan": false}},
        "conditions": [
            {"name": "tomorrow", "type": "enum", "values": ["blue", "red"],
             "value_labels": {"blue": {"fr": "Bleu", "en": "Blue", "es": "Azul"}, "red": {"fr": "Rouge", "en": "Red", "es": "Rojo"}},
             "label": {"fr": "Couleur de demain", "en": "Tomorrow's colour", "es": "Color de mañana"}},
            {"name": "days_left", "type": "integer", "label": {"fr": "Jours restants", "en": "Days left", "es": "Días restantes"}}
        ]
    });
    std::fs::write(dir.join("app/modules/misbehave-1.0.0.bzm"), package::build(&manifest.to_string(), Some(&wasm("misbehave")), &[]).unwrap()).unwrap();
    let mut cfg = breezone_server::config::Config::for_tests(dir.join("data"));
    cfg.web_dir = dir.join("app/web");
    let state = breezone_server::init_state(cfg).await.unwrap();
    let db = state.db.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, breezone_server::app(state)).await.unwrap() });
    let http = reqwest::Client::new();
    let v: Value = http.post(format!("{base}/api/setup")).json(&json!({"username":"admin","password":"motdepasse1"})).send().await.unwrap().json().await.unwrap();
    let token = v["access_token"].as_str().unwrap().to_string();
    let put = |body: Value| http.put(format!("{base}/api/accounts/misbehave")).bearer_auth(&token).json(&body).send();

    // Not connected: nobody answers, the core does it itself.
    let asked = json!({"from": "2026-11-22", "until": "2026-11-22"});
    assert!(breezone_server::modules::ask_service(&db, "tariff.day_color@1", "colors", &asked).await.is_none());
    assert_eq!(put(json!({})).await.unwrap().status().as_u16(), 400, "the token is required");
    assert_eq!(put(json!({"token": "wrong"})).await.unwrap().status().as_u16(), 400, "refused by the module");
    let resp = put(json!({"token": "red"})).await.unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let view: Value = resp.json().await.unwrap();
    assert_eq!((view["connected"].clone(), view["state"].clone(), view["config"].clone()), (json!(true), json!("ok"), json!({})), "{view}");
    let answer = breezone_server::modules::ask_service(&db, "tariff.day_color@1", "colors", &asked).await.unwrap().unwrap();
    assert_eq!(answer, json!([{"day": "2026-11-22", "color": "red"}]));
    // The module keeps a session in its account, never a field of its form.
    let update = json!({"from": "2026-11-22", "until": "2026-11-22", "update": true});
    let answer = breezone_server::modules::ask_service(&db, "tariff.day_color@1", "colors", &update).await.unwrap().unwrap();
    assert_eq!(answer[0]["color"], "white", "session kept, form field refused");
    let answer = breezone_server::modules::ask_service(&db, "tariff.day_color@1", "colors", &asked).await.unwrap().unwrap();
    assert_eq!(answer[0]["color"], "red", "the token typed is unchanged");
    let tested = http.post(format!("{base}/api/accounts/misbehave/test")).bearer_auth(&token).send().await.unwrap();
    assert_eq!(tested.json::<Value>().await.unwrap()["state"], "ok");

    // Its conditions: read for the automations (what it does not declare left out), offered to
    // the rules, and a rule on them checked and read.
    let conditions: Value = http.get(format!("{base}/api/automations/conditions")).bearer_auth(&token).send().await.unwrap().json().await.unwrap();
    let mine = conditions.as_array().unwrap().iter().find(|c| c["module"] == "misbehave").expect("its conditions");
    assert_eq!(mine["values"], json!({"tomorrow": "red", "days_left": 12}), "{mine}");
    let rule = |conditions: Value| {
        json!({"name": "Demain rouge", "enabled": true, "triggers": [{"kind": "time", "time": "20:00"}], "match": "all", "conditions": conditions,
               "actions": [{"kind": "notify", "message": "Demain est rouge"}], "else": [], "mode": "single", "over": []})
    };
    let post = |body: Value| http.post(format!("{base}/api/automations")).bearer_auth(&token).header("Accept-Language", "fr").json(&body).send();
    let resp = post(rule(json!([{"kind": "module", "module": "misbehave", "field": "tomorrow", "value": "red"},
                                {"kind": "measure", "measure": {"kind": "module", "module": "misbehave", "field": "days_left"}, "op": "above", "value": 5}])))
    .await
    .unwrap();
    assert_eq!(resp.status().as_u16(), 201);
    let created: Value = resp.json().await.unwrap();
    assert_eq!(created["summary"]["conditions"][0], "Couleurs · Couleur de demain : Rouge", "{created}");
    assert_eq!(created["status"]["conditions"], json!([true, true]), "{created}");
    let refused = post(rule(json!([{"kind": "module", "module": "misbehave", "field": "tomorrow", "value": "green"}]))).await.unwrap();
    assert_eq!(refused.status().as_u16(), 400, "not one of its values");
}

/// The schedules a piece of hardware keeps (`schedule.hardware`, lot L9): listed, created and
/// changed through the generic routes of a source, the devices, the action and its parameters
/// checked against the catalog first; one its own app keeps is not changed here.
#[tokio::test]
async fn the_schedules_of_the_hardware() {
    use breezone_server::modules::package;
    let _serial = SERIAL.lock().await;
    let dir = tempfile_dir("schedules");
    std::fs::create_dir_all(dir.join("app/modules")).unwrap();
    let manifest = json!({
        "id": "misbehave", "version": "1.0.0",
        "name": {"fr": "Prise", "en": "Plug", "es": "Enchufe"},
        "roles": ["driver"], "requires": {"api": "^1.0"},
        "provides": {"traits": ["switch.relay@1.0"]},
        "permissions": {"network": {"lan": false}},
        "optional_calls": ["schedules"]
    });
    std::fs::write(dir.join("app/modules/misbehave-1.0.0.bzm"), package::build(&manifest.to_string(), Some(&wasm("misbehave")), &[]).unwrap()).unwrap();
    let mut cfg = breezone_server::config::Config::for_tests(dir.join("data"));
    cfg.web_dir = dir.join("app/web");
    let state = breezone_server::init_state(cfg).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, breezone_server::app(state)).await.unwrap() });
    let http = reqwest::Client::new();
    let v: Value = http.post(format!("{base}/api/setup")).json(&json!({"username":"admin","password":"motdepasse1"})).send().await.unwrap().json().await.unwrap();
    let token = v["access_token"].as_str().unwrap().to_string();
    let call = |method: &str, path: &str, body: Option<Value>| {
        let mut req = http.request(method.parse().unwrap(), format!("{base}{path}")).bearer_auth(&token);
        if let Some(b) = body {
            req = req.json(&b);
        }
        async move {
            let resp = req.send().await.unwrap();
            (resp.status().as_u16(), serde_json::from_str::<Value>(&resp.text().await.unwrap()).unwrap_or(Value::Null))
        }
    };
    let (code, s) = call("POST", "/api/sources", Some(json!({"driver": "misbehave", "name": "Prise", "config": "schedules"}))).await;
    assert_eq!(code, 201, "{s}");
    let id = s["id"].as_i64().unwrap();
    let path = format!("/api/sources/{id}/schedules");
    let (code, list) = call("GET", &path, None).await;
    assert_eq!((code, list[0]["editable"].clone(), list[0]["trait"].clone()), (200, json!(false), json!("switch.relay")), "{list}");

    let morning = json!({"name": "Matin", "days": [5, 1, 1], "time": "07:30", "references": ["plug"], "trait": "switch.relay", "action": "set", "params": {"on": true}, "end": {"time": "08:30", "params": {"on": false}}});
    let (code, list) = call("POST", &path, Some(morning.clone())).await;
    assert_eq!(code, 200, "{list}");
    let created = list.as_array().unwrap().iter().find(|s| s["name"] == "Matin").unwrap().clone();
    assert_eq!((created["days"].clone(), created["end"]["params"].clone()), (json!([1, 5]), json!({"on": false})));
    // What the catalog does not know, or a device of another source, is refused before the hardware.
    let mut bad = morning.clone();
    bad["params"] = json!({"on": "yes"});
    assert_eq!(call("POST", &path, Some(bad)).await.0, 400);
    let mut bad = morning.clone();
    bad["references"] = json!(["other"]);
    assert_eq!(call("POST", &path, Some(bad)).await.0, 400);
    let mut bad = morning.clone();
    bad["time"] = json!("25:00");
    assert_eq!(call("POST", &path, Some(bad)).await.0, 400);
    let mut bad = morning.clone();
    bad["action"] = json!("explode");
    assert_eq!(call("POST", &path, Some(bad)).await.0, 422);

    let sid = created["id"].as_str().unwrap();
    let mut later = morning.clone();
    later["time"] = json!("08:00");
    let (code, list) = call("PUT", &format!("{path}/{sid}"), Some(later)).await;
    assert_eq!((code, list.as_array().unwrap().iter().find(|s| s["id"] == sid).unwrap()["time"].clone()), (200, json!("08:00")));
    assert_eq!(call("PUT", &format!("{path}/app"), Some(morning)).await.0, 409, "kept by its app");
    let (code, list) = call("DELETE", &format!("{path}/{sid}"), None).await;
    assert_eq!((code, list.as_array().unwrap().len()), (200, 1));
}

/// The Shelly module in WebAssembly (lot L8): it replaces the driver compiled in, drives the relay,
/// and the WebSocket it keeps with the Shelly makes a change done by hand show at once (Breezone
/// reads the source again when the Shelly tells it, without waiting for the next reading).
#[tokio::test]
async fn the_shelly_module_hears_its_websocket() {
    use axum::extract::ws::{Message, WebSocketUpgrade};
    use breezone_server::modules::package;
    let _serial = SERIAL.lock().await;
    breezone_server::modules::wasm::allow_loopback(true);
    let relay = Arc::new(Mutex::new(false));
    let (tell, _) = tokio::sync::broadcast::channel::<()>(4);
    let fake = {
        let (r1, r2, r3) = (relay.clone(), relay.clone(), relay.clone());
        let tell = tell.clone();
        Router::new()
            .route("/rpc/Shelly.GetDeviceInfo", get(|| async { Json(json!({"id": "shellyplus1pm-a1b2", "app": "Plus1PM", "ver": "1.4.4", "gen": 2})) }))
            .route(
                "/rpc/Shelly.GetStatus",
                get(move || {
                    let r = r1.clone();
                    async move { Json(json!({"switch:0": {"id": 0, "output": *r.lock().unwrap(), "apower": 12.5}})) }
                }),
            )
            .route(
                "/rpc/Switch.Set",
                get(move |Query(q): Query<HashMap<String, String>>| {
                    let r = r2.clone();
                    async move {
                        *r.lock().unwrap() = q.get("on").map(String::as_str) == Some("true");
                        Json(json!({"was_on": false}))
                    }
                }),
            )
            .route(
                "/rpc",
                get(move |ws: WebSocketUpgrade| {
                    let mut told = tell.subscribe();
                    let _ = r3.clone();
                    async move {
                        ws.on_upgrade(move |mut socket| async move {
                            // Answers the first request, then tells each change.
                            let _ = socket.recv().await;
                            let _ = socket.send(Message::Text(r#"{"id":1,"result":{}}"#.into())).await;
                            while told.recv().await.is_ok() {
                                if socket.send(Message::Text(r#"{"src":"shellyplus1pm","method":"NotifyStatus","params":{}}"#.into())).await.is_err() {
                                    break;
                                }
                            }
                        })
                    }
                }),
            )
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let shelly = listener.local_addr().unwrap().to_string();
    tokio::spawn(async move { axum::serve(listener, fake).await.unwrap() });

    let dir = tempfile_dir("shelly");
    std::fs::create_dir_all(dir.join("app/modules")).unwrap();
    let manifest = include_str!("../../modules/shelly/manifest.json");
    std::fs::write(dir.join("app/modules/shelly-1.0.0.bzm"), package::build(manifest, Some(&wasm("shelly")), &[]).unwrap()).unwrap();
    let mut cfg = breezone_server::config::Config::for_tests(dir.join("data"));
    cfg.web_dir = dir.join("app/web");
    let state = breezone_server::init_state(cfg).await.unwrap();
    state.drivers.clone().hear();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, breezone_server::app(state)).await.unwrap() });
    let http = reqwest::Client::new();
    let v: Value = http.post(format!("{base}/api/setup")).json(&json!({"username":"admin","password":"motdepasse1"})).send().await.unwrap().json().await.unwrap();
    let token = v["access_token"].as_str().unwrap().to_string();
    let call = |method: &str, path: &str, body: Option<Value>| {
        let mut req = http.request(method.parse().unwrap(), format!("{base}{path}")).bearer_auth(&token);
        if let Some(b) = body {
            req = req.json(&b);
        }
        async move {
            let resp = req.send().await.unwrap();
            (resp.status().as_u16(), serde_json::from_str::<Value>(&resp.text().await.unwrap()).unwrap_or(Value::Null))
        }
    };

    let (_, modules) = call("GET", "/api/modules", None).await;
    assert_eq!(modules.as_array().unwrap().iter().find(|m| m["id"] == "shelly").unwrap()["origin"], "builtin");
    let (code, s) = call("POST", "/api/sources", Some(json!({"driver": "shelly", "name": "Chauffe-eau", "config": {"host": shelly}}))).await;
    assert_eq!((code, s["status"]["state"].clone()), (201, json!("online")), "{s}");
    let relay_of = |devices: &Value| devices.as_array().unwrap().iter().find(|d| d["source"] == "shelly").unwrap().clone();
    let (_, devices) = call("GET", "/api/devices", None).await;
    let d = relay_of(&devices);
    assert_eq!((d["name"].clone(), d["state"]["on"].clone()), (json!("Plus 1PM relais"), json!(false)));

    // Driven from Breezone.
    let (code, r) = call("POST", &format!("/api/devices/{}/actions", d["id"]), Some(json!({"trait": "switch.relay", "action": "set", "params": {"on": true}}))).await;
    assert_eq!(code, 200, "{r}");
    assert!(*relay.lock().unwrap());

    // Switched off by hand: the Shelly tells it on its WebSocket, Breezone reads it at once.
    *relay.lock().unwrap() = false;
    let _ = tell.send(());
    let mut off = false;
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let (_, devices) = call("GET", "/api/devices", None).await;
        if relay_of(&devices)["state"]["on"] == false {
            off = true;
            break;
        }
    }
    assert!(off, "the change told by the Shelly shows within 4 s");
}

/// Every module shipped with Breezone (lot L8) compiles against the host, fits its manifest, and
/// opens a source from a configuration like the one its driver compiled in takes.
#[tokio::test]
async fn the_shipped_modules_load() {
    let _serial = SERIAL.lock().await;
    let configs = [
        ("tasmota", json!({"host": "192.168.1.30"})),
        ("shelly", json!({"host": "192.168.1.40"})),
        ("trydan", json!({"host": "192.168.1.37"})),
        ("reolink", json!({"host": "192.168.1.50"})),
        ("hoymiles", json!({"host": "192.168.1.60"})),
        ("anker", json!({"host": "192.168.1.61"})),
        ("deye", json!({"host": "192.168.1.63", "serial": "4123456789"})),
        ("tydom", json!({"host": "192.168.1.64", "mac": "001A25123456"})),
        ("tplink", json!({"host": "192.168.1.31"})),
        ("hue", json!({"host": "192.168.1.20", "device_id": "001788FFFE6DBD26"})),
        ("renson", json!({"host": "192.168.1.70"})),
        // A service: loaded, nothing to open.
        ("rte", json!(null)),
        ("weather", json!(null)),
        ("linky", json!(null)),
        ("deltadore", json!(null)),
        // Its devices come with its account: nothing reached before a read.
        ("netatmo", json!({})),
        ("ezviz", json!({})),
        ("anker_cloud", json!({})),
        // The connection is opened at the first description, not before.
        ("esphome", json!({"host": "192.168.1.62"})),
        ("homekit", json!({"host": "192.168.1.25:5001", "device_id": "AA:BB:CC:DD:EE:FF"})),
        ("forecast_solar", json!({"kwp": 4.5, "declination": 30, "azimuth": 0, "latitude": 43.5, "longitude": 1.5, "timezone": "Europe/Paris"})),
    ];
    let host = breezone_server::capabilities::Host::local();
    for (id, config) in configs {
        let text = std::fs::read_to_string(modules_dir().join(id).join("manifest.json")).unwrap();
        let manifest = breezone_server::modules::manifest::Manifest::parse(&text).unwrap_or_else(|e| panic!("{id}: {e:?}"));
        assert_eq!(manifest.id, id);
        assert_eq!(breezone_server::modules::compat::check(&manifest, &host, env!("CARGO_PKG_VERSION"), |_| true), Ok(()), "{id}");
        let module = Arc::new(WasmModule::load(id, &wasm(id), local(), None).unwrap_or_else(|e| panic!("{id}: {e}")));
        if config.is_null() {
            continue;
        }
        // A Hue bridge is opened with the key it gave (a plain string).
        let pairing = json!({"accessory_id": "AA:BB:CC:DD:EE:FF", "accessory_ltpk": "00".repeat(32), "controller_id": "C", "controller_ltsk": "11".repeat(32)}).to_string();
        let secret = match id {
            "hue" => Some("key-1"),
            // A HomeKit accessory with the pairing the driver compiled in kept.
            "homekit" => Some(pairing.as_str()),
            "tydom" => Some(r#"{"password": "secret"}"#),
            _ => None,
        };
        module.open(None, &config, secret).await.unwrap_or_else(|e| panic!("{id}: {e}"));
    }
}

/// A question and its answer on TCP (`host.exchange`, for the binary protocols of Hoymiles and
/// TP-Link Kasa): the answer is read whole by the length it announces, even in several pieces; an
/// address the module may not reach is refused.
#[tokio::test]
async fn a_tcp_exchange_reads_the_length_announced() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let _serial = SERIAL.lock().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut tcp, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut asked = [0u8; 4];
                tcp.read_exact(&mut asked).await.unwrap();
                assert_eq!(&asked, b"ping");
                // 11 bytes announced, sent in two pieces, then more that is not part of it.
                tcp.write_all(&[0, 0, 0, 11, b'h', b'e', b'l']).await.unwrap();
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                tcp.write_all(b"lo world, and more").await.unwrap();
                let _ = tcp.read(&mut asked).await;
            });
        }
    });
    let module = Arc::new(WasmModule::load("misbehave", &wasm("misbehave"), local(), None).unwrap());
    let s = module.open(None, &json!(format!("exchange:tcp://{addr}")), None).await.unwrap();
    let state = s.read().await.unwrap();
    assert_eq!((state["exchange"]["length"].clone(), state["exchange"]["text"].clone()), (json!(15), json!("hello world")));

    let strict = Arc::new(WasmModule::load("misbehave", &wasm("misbehave"), Permissions { loopback: false, ..local() }, None).unwrap());
    let s = strict.open(None, &json!(format!("exchange:tcp://{addr}")), None).await.unwrap();
    assert!(matches!(s.read().await, Err(DriverError::Unreachable(e)) if e.contains("not allowed")));
}

/// The HomeKit module, as the driver compiled in: paired with the code (typed once the accessory
/// shows it), a shutter read and driven, the remote's buttons heard on a session of their own, and
/// Breezone's own pairing removed with the source.
#[tokio::test]
async fn the_homekit_module_pairs_drives_and_forgets() {
    use breezone_server::modules::package;
    use support::hap::fake;
    let _serial = SERIAL.lock().await;
    breezone_server::modules::wasm::allow_loopback(true);
    let accessory = fake::start().await;
    let dir = tempfile_dir("homekit");
    std::fs::create_dir_all(dir.join("app/modules")).unwrap();
    let manifest = include_str!("../../modules/homekit/manifest.json");
    std::fs::write(dir.join("app/modules/homekit-1.0.0.bzm"), package::build(manifest, Some(&wasm("homekit")), &[]).unwrap()).unwrap();
    let mut cfg = breezone_server::config::Config::for_tests(dir.join("data"));
    cfg.web_dir = dir.join("app/web");
    let state = breezone_server::init_state(cfg).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, breezone_server::app(state)).await.unwrap() });
    let http = reqwest::Client::new();
    let v: Value = http.post(format!("{base}/api/setup")).json(&json!({"username":"admin","password":"motdepasse1"})).send().await.unwrap().json().await.unwrap();
    let token = v["access_token"].as_str().unwrap().to_string();
    let call = |method: &str, path: &str, body: Option<Value>| {
        let mut req = http.request(method.parse().unwrap(), format!("{base}{path}")).bearer_auth(&token);
        if let Some(b) = body {
            req = req.json(&b);
        }
        async move {
            let resp = req.send().await.unwrap();
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap();
            (status, text.clone(), serde_json::from_str::<Value>(&text).unwrap_or(Value::Null))
        }
    };
    let (_, _, modules) = call("GET", "/api/modules", None).await;
    assert_eq!(modules.as_array().unwrap().iter().find(|m| m["id"] == "homekit").unwrap()["origin"], "builtin", "the module, not the driver compiled in");
    let host = format!("127.0.0.1:{}", accessory.port);
    let pair = |step: &str, input: Value| call("POST", "/api/modules/homekit/pairing", Some(json!({"step": step, "input": input, "name": "Legrand"})));

    let (s, _, v) = pair("finish", json!({"host": host, "code": "1234"})).await;
    assert_eq!(s, 422, "not a code: {v}");
    let (s, _, v) = pair("finish", json!({"host": host, "code": "111-11-111", "flags": 1})).await;
    assert_eq!(s, 422);
    assert!(v.to_string().contains("Code incorrect") || v.to_string().contains("Wrong code"), "{v}");
    // With a screen, the code shown changes when a pairing starts: it is typed afterwards.
    accessory.state.lock().unwrap().screen = true;
    let (s, text, _) = pair("start", json!({"host": host, "flags": 1})).await;
    assert_eq!(s, 204, "{text}");
    let shown = accessory.state.lock().unwrap().shown.clone().unwrap();
    // Started again (the dialog reopened): the first one is dropped, a new code shows.
    let (s, _, _) = pair("start", json!({"host": host, "flags": 1})).await;
    assert_eq!(s, 204);
    let (s, _, v) = pair("finish", json!({"host": host, "code": shown})).await;
    assert_eq!(s, 422, "the code of the dropped pairing: {v}");
    let (s, _, _) = pair("start", json!({"host": host, "flags": 1})).await;
    assert_eq!(s, 204);
    let (s, _, _) = pair("cancel", json!({"host": host})).await;
    assert_eq!(s, 204);
    let (s, _, _) = pair("start", json!({"host": host, "flags": 1})).await;
    assert_eq!(s, 204);
    let shown = accessory.state.lock().unwrap().shown.clone().unwrap();
    let (s, text, source) = pair("finish", json!({"host": host, "code": shown, "flags": 1})).await;
    assert_eq!(s, 201, "{text}");
    assert_eq!(source["status"]["state"], "online", "{source}");
    assert_eq!(source["config"]["device_id"], "AA:BB:CC:DD:EE:FF");
    assert!(!text.contains("controller_ltsk") && !text.contains("secret"), "the pairing key is never shown: {text}");

    let (_, _, devices) = call("GET", "/api/devices", None).await;
    let cover = devices.as_array().unwrap().iter().find(|d| d["source"] == "homekit" && d["name"] == "Volet salon").unwrap().clone();
    assert_eq!(cover["traits"], json!(["cover.position@1"]));
    assert_eq!(cover["state"]["position"], 40);
    let (s, text, d) = call("POST", &format!("/api/devices/{}/actions", cover["id"]), Some(json!({"trait":"cover.position","action":"set","params":{"position":75}}))).await;
    assert_eq!(s, 200, "{text}");
    assert_eq!(accessory.state.lock().unwrap().position, 75);
    assert_eq!(d["state"]["position"], 75);

    // The remote: its 4 buttons, heard on a session of their own.
    let remote = devices.as_array().unwrap().iter().find(|d| d["source"] == "homekit" && d["kind"] == "remote").unwrap().clone();
    assert_eq!((remote["name"].clone(), remote["traits"].clone(), remote["state"]["buttons"].clone()), (json!("Télécommande"), json!(["button@1"]), json!(4)));
    for _ in 0..50 {
        if accessory.state.lock().unwrap().subscribed.len() == 4 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(accessory.state.lock().unwrap().subscribed.len(), 4, "the 4 buttons are listened to");
    accessory.press(1, 2);
    accessory.press(2, 0);
    let mut remote = remote;
    for _ in 0..50 {
        let (_, _, devices) = call("GET", "/api/devices", None).await;
        remote = devices.as_array().unwrap().iter().find(|d| d["id"] == remote["id"]).unwrap().clone();
        if remote["state"]["last_button"] == 2 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!((remote["state"]["last_button"].clone(), remote["state"]["last_press"].clone()), (json!(2), json!("single")));
    let (_, _, events) = call("GET", "/api/events?limit=10", None).await;
    let told: Vec<String> = events.as_array().into_iter().flatten().filter(|e| e["kind"] == "button").filter_map(|e| e["message"].as_str().map(str::to_string)).collect();
    assert!(told.iter().any(|m| m.contains("Télécommande") && m.contains("button 1, long press")), "{events}");
    assert_eq!(accessory.state.lock().unwrap().position, 75, "the shutter did not move");

    let (s, _, _) = call("DELETE", &format!("/api/sources/{}", source["id"]), None).await;
    assert_eq!(s, 204);
    assert!(accessory.state.lock().unwrap().controllers.is_empty(), "Breezone removed its own pairing");
}

/// A module built against version 1.0 of the interface, kept as it was: still loaded and driven,
/// whatever the interface added since (a call it does not export is answered for it).
#[tokio::test]
async fn a_module_of_the_first_interface_still_works() {
    let _serial = SERIAL.lock().await;
    let module = Arc::new(WasmModule::load("contract", &wasm("contract_1_0"), local(), None).unwrap().with_optional_calls(&["forget".to_string()]));
    let source = module.open(None, &json!({}), None).await.expect("loaded");
    let described = source.describe().await.unwrap();
    assert_eq!(described.devices.len(), 1);
    let states = source.read().await.unwrap();
    assert_eq!(states["plug"]["on"], true);
    // Added in 1.1, not exported by this module: nothing to forget.
    source.forget().await.expect("no `extras.forget`: nothing done");
}
