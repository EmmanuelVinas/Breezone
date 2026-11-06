//! Federation between two Breezone nodes (ADR 0008), and linking installations added by hand
//! to the Airzone account.

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

fn temp_dir(name: &str) -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("breezone-fed-{name}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

struct Node {
    base: String,
    http: reqwest::Client,
    token: String,
}

impl Node {
    /// A Breezone server with its administrator.
    async fn start(name: &str, configure: impl FnOnce(&mut Config)) -> Node {
        let mut cfg = Config::for_tests(temp_dir(name));
        configure(&mut cfg);
        let state = breezone_server::init_state(cfg).await.unwrap();
        let port = serve(breezone_server::app(state)).await;
        let base = format!("http://127.0.0.1:{port}");
        let http = reqwest::Client::new();
        let v: Value = http
            .post(format!("{base}/api/setup"))
            .json(&json!({"username": "admin", "password": "motdepasse1"}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        Node { base, http, token: v["access_token"].as_str().unwrap().into() }
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
}

fn mock_setpoint(mock: &MockState, zone_id: i64) -> Option<f64> {
    mock.lock().unwrap().iter().find(|z| z.zone_id == zone_id).and_then(|z| z.setpoint)
}

#[tokio::test]
async fn a_node_drives_the_installations_another_node_shares() {
    // The office WebServer, reachable only from the office node.
    let office_mock: MockState = Arc::new(Mutex::new(mock::preset("bureau")));
    let office_ws = serve(mock::router_with_mac(office_mock.clone(), "AA:BB:CC:DD:EE:FF")).await;
    // The home node knows the Airzone account (the office installation is "inst-office").
    let cloud_port = serve(mock::cloud_router(mock::cloud_account(mock::MOCK_MAC))).await;

    let office = Node::start("office", |_| {}).await;
    let home = Node::start("home", |c| {
        c.airzone_cloud_url = format!("http://127.0.0.1:{cloud_port}");
        c.scan_port = 9; // nothing on the home network in this test
    })
    .await;

    // Office: its installation, its identity, and a share for home.
    let (s, inst) = office.call(Method::POST, "/api/installations", Some(json!({"name":"Atelier","host":"127.0.0.1","port":office_ws}))).await;
    assert_eq!(s, StatusCode::CREATED, "{inst}");
    let office_inst = inst["id"].as_i64().unwrap();
    let (s, v) = office.call(Method::PUT, "/api/node", Some(json!({"name":"Bureau","public_url": office.base}))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, v) = office.call(Method::PUT, "/api/node", Some(json!({"name":"Bureau","public_url":"http://breezone.example.org"}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "plain HTTP towards a public address: {v}");
    let (s, created) = office
        .call(Method::POST, "/api/shares", Some(json!({"label":"Maison","installation_ids":[office_inst],"permission":"control"})))
        .await;
    assert_eq!(s, StatusCode::CREATED, "{created}");
    let token = created["token"].as_str().unwrap().to_string();
    assert!(token.starts_with("bz1."));
    assert_eq!(created["share"]["connected"], false);

    // A node cannot add its own token; a garbled token is refused.
    let (s, _) = office.call(Method::POST, "/api/nodes", Some(json!({"token": token}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = home.call(Method::POST, "/api/nodes", Some(json!({"token":"bz1.nope"}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // Home: the Airzone account first (the office installation is not reachable from home)…
    let (s, v) = home
        .call(Method::PUT, "/api/cloud", Some(json!({"email": mock::MOCK_CLOUD_EMAIL, "password": mock::MOCK_CLOUD_PASSWORD})))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    // …then the office node, from its token.
    let (s, node) = home.call(Method::POST, "/api/nodes", Some(json!({"token": token}))).await;
    assert_eq!(s, StatusCode::CREATED, "{node}");
    assert_eq!(node["name"], "Bureau");
    assert_eq!(node["status"]["state"], "online");
    assert_eq!(node["status"]["permission"], "control");
    let node_id = node["id"].as_i64().unwrap();

    // The shared installation is driven from home like a local one.
    let (_, status) = home.call(Method::GET, "/api/status", None).await;
    let remote = status.as_array().unwrap().iter().find(|i| i["access"] == "node").cloned().expect("remote installation");
    assert_eq!(remote["node"]["name"], "Bureau");
    assert_eq!(remote["connection"]["state"], "online");
    assert_eq!(remote["zones"].as_array().unwrap().len(), 3);
    assert_eq!(remote["controllable"], true);
    let remote_id = remote["id"].as_i64().unwrap();
    // Recognised as the "Atelier" installation of the account (same WebServer).
    let (_, cloud) = home.call(Method::GET, "/api/cloud", None).await;
    let mkp = cloud["installations"].as_array().unwrap().iter().find(|c| c["cloud_id"] == "inst-office").unwrap().clone();
    assert_eq!(mkp["reachable"], true, "{cloud}");
    assert_eq!(mkp["installation_id"], remote_id);

    // Names set on the office node show at home; saving them unchanged does not freeze them.
    let (_, o) = office.call(Method::PUT, &format!("/api/installations/{office_inst}/zones/1/2/meta"), Some(json!({"display_name":"Réunion"}))).await;
    assert_eq!(o["name"], "Réunion");
    home.call(Method::POST, &format!("/api/nodes/{node_id}/refresh"), None).await;
    let (_, z) = home.call(Method::PUT, &format!("/api/installations/{remote_id}/zones/1/2/meta"), Some(json!({"display_name":"Réunion","group":"Bureaux"}))).await;
    assert_eq!(z["name"], "Réunion");
    office.call(Method::PUT, &format!("/api/installations/{office_inst}/zones/1/2/meta"), Some(json!({"display_name":"Salle Ada"}))).await;
    home.call(Method::POST, &format!("/api/nodes/{node_id}/refresh"), None).await;
    let (_, inst) = home.call(Method::GET, &format!("/api/installations/{remote_id}"), None).await;
    let z2 = inst["zones"].as_array().unwrap().iter().find(|z| z["zone_id"] == 2).unwrap().clone();
    assert_eq!(z2["name"], "Salle Ada", "renamed on the office node: {z2}");
    assert_eq!(z2["group"], "Bureaux", "local group kept");

    let (s, z) = home.call(Method::PATCH, &format!("/api/installations/{remote_id}/zones/1/1"), Some(json!({"setpoint": 22.5}))).await;
    assert_eq!(s, StatusCode::OK, "{z}");
    assert_eq!(z["setpoint"], 22.5);
    assert_eq!(mock_setpoint(&office_mock, 1), Some(22.5), "applied on the office WebServer");
    // Out of range: validated by the office node.
    let (s, _) = home.call(Method::PATCH, &format!("/api/installations/{remote_id}/zones/1/1"), Some(json!({"setpoint": 50}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // The office knows who uses its share, and who sent the command.
    let (_, shares) = office.call(Method::GET, "/api/shares", None).await;
    let share = &shares[0];
    assert_eq!(share["connected"], true);
    assert_eq!(share["remote_name"], "Breezone");
    assert_eq!(share["installations"][0]["name"], "Atelier");
    let (_, events) = office.call(Method::GET, "/api/events?limit=20", None).await;
    let cmd = events.as_array().unwrap().iter().find(|e| e["kind"] == "command").unwrap();
    assert_eq!(cmd["actor"], "admin (nœud Breezone)", "{cmd}");

    // Read-only share: commands refused, state still visible.
    let share_id = share["id"].as_i64().unwrap();
    let (s, v) = office
        .call(Method::PUT, &format!("/api/shares/{share_id}"), Some(json!({"label":"Maison","installation_ids":[office_inst],"permission":"read"})))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, node) = home.call(Method::POST, &format!("/api/nodes/{node_id}/refresh"), None).await;
    assert_eq!(node["status"]["permission"], "read");
    let (s, _) = home.call(Method::PATCH, &format!("/api/installations/{remote_id}/zones/1/1"), Some(json!({"on": false}))).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (_, inst) = home.call(Method::GET, &format!("/api/installations/{remote_id}"), None).await;
    assert_eq!(inst["controllable"], false);

    // Revoked: the home node shows it, the installation goes offline.
    let (s, _) = office.call(Method::DELETE, &format!("/api/shares/{share_id}"), None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, node) = home.call(Method::POST, &format!("/api/nodes/{node_id}/refresh"), None).await;
    assert_eq!(node["status"]["state"], "revoked", "{node}");
    let (_, inst) = home.call(Method::GET, &format!("/api/installations/{remote_id}"), None).await;
    assert_eq!(inst["connection"]["state"], "offline");
    assert_eq!(inst["node"]["state"], "revoked");

    // Removing the node removes its installations.
    let (s, _) = home.call(Method::DELETE, &format!("/api/nodes/{node_id}"), None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, status) = home.call(Method::GET, "/api/status", None).await;
    assert!(status.as_array().unwrap().iter().all(|i| i["access"] == "local"));
}

#[tokio::test]
async fn shares_only_expose_their_installations() {
    let a_mock: MockState = Arc::new(Mutex::new(mock::preset("maison")));
    let a_ws = serve(mock::router(a_mock)).await;
    let b_mock: MockState = Arc::new(Mutex::new(mock::preset("bureau")));
    let b_ws = serve(mock::router_with_mac(b_mock, "AA:BB:CC:DD:EE:FF")).await;
    let office = Node::start("scope", |_| {}).await;
    let (_, a) = office.call(Method::POST, "/api/installations", Some(json!({"name":"A","host":"127.0.0.1","port":a_ws}))).await;
    let (_, b) = office.call(Method::POST, "/api/installations", Some(json!({"name":"B","host":"127.0.0.1","port":b_ws}))).await;
    let (s, created) = office
        .call(Method::POST, "/api/shares", Some(json!({"label":"Voisin","installation_ids":[a["id"]],"permission":"control","url": office.base})))
        .await;
    assert_eq!(s, StatusCode::CREATED, "{created}");
    let token = created["token"].as_str().unwrap();
    let inv: Value = {
        use base64::Engine;
        let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(token.trim_start_matches("bz1.")).unwrap();
        serde_json::from_slice(&raw).unwrap()
    };
    let key = inv["key"].as_str().unwrap();
    let http = reqwest::Client::new();

    // Only installation A is visible; B cannot be driven with this key.
    let snap: Value = http.get(format!("{}/api/federation/installations", office.base)).bearer_auth(key).send().await.unwrap().json().await.unwrap();
    let ids: Vec<i64> = snap["installations"].as_array().unwrap().iter().map(|i| i["id"].as_i64().unwrap()).collect();
    assert_eq!(ids, vec![a["id"].as_i64().unwrap()]);
    let r = http
        .patch(format!("{}/api/federation/installations/{}/zones/1/1", office.base, b["id"]))
        .bearer_auth(key)
        .json(&json!({"on": false}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
    // No key, a user token, or a wrong key: refused.
    let r = http.get(format!("{}/api/federation/installations", office.base)).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    let r = http.get(format!("{}/api/federation/installations", office.base)).bearer_auth(&office.token).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    let r = http.get(format!("{}/api/federation/installations", office.base)).bearer_auth("bzk_wrong").send().await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    // The federation API does not give access to the user API.
    let r = http.get(format!("{}/api/status", office.base)).bearer_auth(key).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn an_installation_added_by_hand_is_linked_to_the_airzone_account() {
    let maison: MockState = Arc::new(Mutex::new(mock::preset("maison")));
    let maison_ws = serve(mock::router(maison)).await;
    let cloud_port = serve(mock::cloud_router(mock::cloud_account(mock::MOCK_MAC))).await;
    let n = Node::start("link", |c| {
        c.airzone_cloud_url = format!("http://127.0.0.1:{cloud_port}");
        c.scan_port = 9; // the scan does not find it
    })
    .await;
    let (s, v) = n.call(Method::PUT, "/api/cloud", Some(json!({"email": mock::MOCK_CLOUD_EMAIL, "password": mock::MOCK_CLOUD_PASSWORD}))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["report"]["created"], json!([]));

    // Added by hand afterwards, under another name: recognised by its MAC address.
    let (s, inst) = n.call(Method::POST, "/api/installations", Some(json!({"name":"Maison","host":"127.0.0.1","port":maison_ws}))).await;
    assert_eq!(s, StatusCode::CREATED, "{inst}");
    let id = inst["id"].as_i64().unwrap();
    assert_eq!(inst["location"], "Annecy, Haute-Savoie");
    let (_, cloud) = n.call(Method::GET, "/api/cloud", None).await;
    let home = cloud["installations"].as_array().unwrap().iter().find(|c| c["cloud_id"] == "inst-home").unwrap().clone();
    assert_eq!(home["installation_id"], id, "no duplicate: {cloud}");
    let salon = inst["zones"].as_array().unwrap().iter().find(|z| z["name"] == "Salon").unwrap();
    assert_eq!(salon["group"], "Rez de chaussée");

    // Explicit choice: another installation of the account is refused (different WebServer)…
    let (s, v) = n.call(Method::PUT, &format!("/api/installations/{id}/cloud"), Some(json!({"cloud_id":"inst-office"}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(v["error"].as_str().unwrap().contains("n'est pas celui de"), "{v}");
    // …unlinking and linking again works, and takes the Airzone name.
    let (s, _) = n.call(Method::PUT, &format!("/api/installations/{id}/cloud"), Some(json!({"cloud_id": null}))).await;
    assert_eq!(s, StatusCode::OK);
    let (_, list) = n.call(Method::GET, "/api/installations", None).await;
    assert_eq!(list[0]["cloud_id"], Value::Null);
    let (s, v) = n.call(Method::PUT, &format!("/api/installations/{id}/cloud"), Some(json!({"cloud_id":"inst-home"}))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["name"], "Maison");
    let (_, list) = n.call(Method::GET, "/api/installations", None).await;
    assert_eq!(list[0]["cloud_id"], "inst-home");

    // The installation now points to another WebServer (the office one): its old MAC address
    // must not prevent linking it to the office installation of the account.
    let office: MockState = Arc::new(Mutex::new(mock::preset("bureau")));
    let office_ws = serve(mock::router_with_mac(office, "AA:BB:CC:DD:EE:FF")).await;
    let (s, v) = n.call(Method::PUT, &format!("/api/installations/{id}"), Some(json!({"name":"Bureau","host":"127.0.0.1","port":office_ws}))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, v) = n.call(Method::PUT, &format!("/api/installations/{id}/cloud"), Some(json!({"cloud_id":"inst-office"}))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, list) = n.call(Method::GET, "/api/installations", None).await;
    assert_eq!(list[0]["mac"], "AA:BB:CC:DD:EE:FF");
    assert_eq!(list[0]["cloud_id"], "inst-office");
}
