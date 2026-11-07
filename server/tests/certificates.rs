//! Nodes behind HTTPS certificates that fail the usual checks (self-signed, wrong name): the
//! certificate is shown, can be trusted by fingerprint, and only that one is then accepted.

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

/// TLS in front of a plain port, with a self-signed certificate for `name`.
/// Returns the HTTPS port and the certificate fingerprint.
async fn tls_proxy(backend: u16, name: &str) -> (u16, String) {
    let cert = rcgen::generate_simple_self_signed(vec![name.to_string()]).unwrap();
    let der = cert.cert.der().to_vec();
    let fingerprint = breezone_server::tls::fingerprint(&der);
    let key = rustls::pki_types::PrivateKeyDer::Pkcs8(cert.signing_key.serialize_der().into());
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![der.into()], key)
        .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else { return };
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(tcp).await else { return };
                let Ok(mut plain) = tokio::net::TcpStream::connect(("127.0.0.1", backend)).await else { return };
                let _ = tokio::io::copy_bidirectional(&mut tls, &mut plain).await;
            });
        }
    });
    (port, fingerprint)
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("breezone-tls-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

struct Node {
    base: String,
    http: reqwest::Client,
    token: String,
    port: u16,
}

impl Node {
    async fn start(name: &str) -> Node {
        let mut cfg = Config::for_tests(temp_dir(name));
        cfg.scan_port = 9;
        let state = breezone_server::init_state(cfg).await.unwrap();
        let port = serve(breezone_server::app(state)).await;
        let base = format!("http://127.0.0.1:{port}");
        let http = reqwest::Client::new();
        let v: Value = http.post(format!("{base}/api/setup")).json(&json!({"username":"admin","password":"motdepasse1"})).send().await.unwrap().json().await.unwrap();
        Node { base, http, token: v["access_token"].as_str().unwrap().into(), port }
    }

    async fn call(&self, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut req = self.http.request(method, format!("{}{}", self.base, path)).bearer_auth(&self.token);
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await.unwrap();
        let status = resp.status();
        (status, serde_json::from_str(&resp.text().await.unwrap()).unwrap_or(Value::Null))
    }

    async fn installation(&self, name: &str, preset: &str, mac: &str) -> i64 {
        let ws = serve(mock::router_with_mac(Arc::new(Mutex::new(mock::preset(preset))) as MockState, mac)).await;
        let (s, v) = self.call(Method::POST, "/api/installations", Some(json!({"name": name, "host": "127.0.0.1", "port": ws}))).await;
        assert_eq!(s, StatusCode::CREATED, "{v}");
        v["id"].as_i64().unwrap()
    }
}

#[tokio::test]
async fn untrusted_certificates_can_be_reviewed_and_pinned_both_ways() {
    let office = Node::start("office").await;
    let home = Node::start("home").await;
    // Both behind HTTPS with a self-signed certificate for another name.
    let (office_tls, office_fp) = tls_proxy(office.port, "nas.example.org").await;
    let (home_tls, home_fp) = tls_proxy(home.port, "home.example.org").await;
    let office_url = format!("https://127.0.0.1:{office_tls}");
    let home_url = format!("https://127.0.0.1:{home_tls}");
    let office_inst = office.installation("Bureau", "bureau", "AA:BB:CC:DD:EE:FF").await;
    let home_inst = home.installation("Maison", "maison", mock::MOCK_MAC).await;
    office.call(Method::PUT, "/api/node", Some(json!({"name":"Bureau","public_url": office_url}))).await;
    home.call(Method::PUT, "/api/node", Some(json!({"name":"Maison","public_url": home_url}))).await;

    let (_, created) = office
        .call(Method::POST, "/api/shares", Some(json!({"label":"Maison","installation_ids":[office_inst],"permission":"control","reciprocal":true})))
        .await;
    let token = created["token"].as_str().unwrap().to_string();
    let back = json!({"installation_ids": [home_inst], "permission": "read"});

    // 1. Home → office: refused, with the certificate to review (the code is not burnt).
    let (s, v) = home.call(Method::POST, "/api/nodes", Some(json!({"token": token, "reciprocal": back}))).await;
    assert_eq!(s, StatusCode::BAD_GATEWAY, "{v}");
    assert!(v["error"].as_str().unwrap().contains("certificat"), "{v}");
    assert_eq!(v["certificate"]["fingerprint"], office_fp.as_str(), "{v}");
    assert_eq!(v["certificate"]["names"][0], "nas.example.org");
    assert_eq!(v["certificate"]["self_signed"], true);
    // Another certificate's fingerprint does not help.
    let (s, _) = home.call(Method::POST, "/api/nodes", Some(json!({"token": token, "reciprocal": back, "certificate": home_fp}))).await;
    assert_eq!(s, StatusCode::BAD_GATEWAY);
    // Trusting the reviewed one pairs.
    let (s, node) = home.call(Method::POST, "/api/nodes", Some(json!({"token": token, "reciprocal": back, "certificate": office_fp}))).await;
    assert_eq!(s, StatusCode::CREATED, "{node}");
    assert_eq!(node["status"]["state"], "pending");
    assert_eq!(node["certificate_pinned"], true);

    // 2. Office approves; reaching home back fails on home's certificate.
    let share_id = office.call(Method::GET, "/api/shares", None).await.1[0]["id"].as_i64().unwrap();
    let (s, v) = office.call(Method::POST, &format!("/api/shares/{share_id}/approve"), None).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, nodes) = office.call(Method::GET, "/api/nodes", None).await;
    let back_node = nodes[0]["id"].as_i64().unwrap();
    assert_eq!(nodes[0]["status"]["state"], "certificate", "{nodes}");
    let (s, cert) = office.call(Method::GET, &format!("/api/nodes/{back_node}/certificate"), None).await;
    assert_eq!(s, StatusCode::OK, "{cert}");
    assert_eq!(cert["certificate"]["fingerprint"], home_fp.as_str());
    assert_eq!(cert["pinned"], Value::Null);
    let (s, _) = office.call(Method::PUT, &format!("/api/nodes/{back_node}/certificate"), Some(json!({"fingerprint": office_fp}))).await;
    assert_eq!(s, StatusCode::CONFLICT, "only the certificate presented can be trusted");
    let (s, v) = office.call(Method::PUT, &format!("/api/nodes/{back_node}/certificate"), Some(json!({"fingerprint": home_fp}))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["status"]["state"], "online", "{v}");
    assert_eq!(v["installations"][0]["name"], "Maison");

    // Home now reaches the office too.
    let home_node = node["id"].as_i64().unwrap();
    let (_, v) = home.call(Method::POST, &format!("/api/nodes/{home_node}/refresh"), None).await;
    assert_eq!(v["status"]["state"], "online", "{v}");

    // 3. Back to the usual checks: the self-signed certificate is refused again.
    let (s, v) = home.call(Method::DELETE, &format!("/api/nodes/{home_node}/certificate"), None).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["status"]["state"], "certificate", "{v}");
    assert_eq!(v["certificate_pinned"], false);
}
