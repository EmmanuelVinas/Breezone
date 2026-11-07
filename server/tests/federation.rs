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
    state: breezone_server::AppState,
}

impl Node {
    /// A Breezone server with its administrator.
    async fn start(name: &str, configure: impl FnOnce(&mut Config)) -> Node {
        let mut cfg = Config::for_tests(temp_dir(name));
        configure(&mut cfg);
        let state = breezone_server::init_state(cfg).await.unwrap();
        let port = serve(breezone_server::app(state.clone())).await;
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
        Node { base, http, token: v["access_token"].as_str().unwrap().into(), state }
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
    assert!(token.starts_with("bz2."));
    assert_eq!(created["share"]["connected"], false);
    assert_eq!(created["share"]["status"], "invited");

    // A node cannot add its own token; a garbled or old-format token is refused.
    let (s, _) = office.call(Method::POST, "/api/nodes", Some(json!({"token": token}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = home.call(Method::POST, "/api/nodes", Some(json!({"token":"bz2.nope"}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, v) = home.call(Method::POST, "/api/nodes", Some(json!({"token":"bz1.eyJ2IjoxfQ"}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(v["error"].as_str().unwrap().contains("ancien format"), "{v}");

    // Home: the Airzone account first (the office installation is not reachable from home)…
    let (s, v) = home
        .call(Method::PUT, "/api/cloud", Some(json!({"email": mock::MOCK_CLOUD_EMAIL, "password": mock::MOCK_CLOUD_PASSWORD})))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    // …then the office node, from its token.
    let (s, node) = home.call(Method::POST, "/api/nodes", Some(json!({"token": token}))).await;
    assert_eq!(s, StatusCode::CREATED, "{node}");
    assert_eq!(node["name"], "Bureau");
    assert_eq!(node["status"]["state"], "pending", "waits for the office to accept: {node}");
    let node_id = node["id"].as_i64().unwrap();
    let (_, status) = home.call(Method::GET, "/api/status", None).await;
    assert!(status.as_array().unwrap().iter().all(|i| i["access"] == "local"), "nothing shared before approval");

    // The invitation is single-use.
    let (s, v) = home.call(Method::POST, "/api/nodes", Some(json!({"token": token}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");

    // The office sees the request and accepts it.
    let (_, shares) = office.call(Method::GET, "/api/shares", None).await;
    assert_eq!(shares[0]["status"], "pending");
    assert_eq!(shares[0]["remote_name"], "Breezone");
    assert!(shares[0]["requested_at"].is_i64());
    let (_, events) = office.call(Method::GET, "/api/events?limit=5", None).await;
    assert!(events.as_array().unwrap().iter().any(|e| e["message"].as_str().unwrap_or_default().contains("demande l'accès")));
    let share_id = shares[0]["id"].as_i64().unwrap();
    let (s, v) = office.call(Method::POST, &format!("/api/shares/{share_id}/approve"), None).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "active");
    let (_, node) = home.call(Method::POST, &format!("/api/nodes/{node_id}/refresh"), None).await;
    assert_eq!(node["status"]["state"], "online", "{node}");
    assert_eq!(node["status"]["permission"], "control");
    // The Airzone link of installations received from a node runs in the background (every 5
    // minutes); trigger it now.
    breezone_server::cloud::auto_link_all(&home.state).await;

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

/// A fake node: its key, and signed calls made by hand.
struct FakeNode {
    id: String,
    key: ed25519_dalek::SigningKey,
}

impl FakeNode {
    fn new(id: &str, seed: u8) -> FakeNode {
        FakeNode { id: id.into(), key: ed25519_dalek::SigningKey::from_bytes(&[seed; 32]) }
    }

    fn request(&self, http: &reqwest::Client, base: &str, method: Method, path: &str, body: &[u8]) -> reqwest::RequestBuilder {
        let mut req = http.request(method.clone(), format!("{base}{path}"));
        for (name, value) in breezone_server::federation::signed_headers(&self.key, &self.id, method.as_str(), path, body) {
            req = req.header(name, value);
        }
        if !body.is_empty() {
            req = req.header("content-type", "application/json").body(body.to_vec());
        }
        req
    }

    async fn pair(&self, http: &reqwest::Client, base: &str, token: &str) -> reqwest::Response {
        use base64::Engine;
        let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(token.trim_start_matches("bz2.")).unwrap();
        let inv: Value = serde_json::from_slice(&raw).unwrap();
        let body = serde_json::to_vec(&json!({
            "code": inv["code"], "node_id": self.id, "name": "Voisin",
            "public_key": breezone_server::federation::public_key_text(&self.key),
        }))
        .unwrap();
        self.request(http, base, Method::POST, "/api/federation/pair", &body).send().await.unwrap()
    }
}

#[tokio::test]
async fn pairing_and_signed_requests_are_enforced() {
    let a_mock: MockState = Arc::new(Mutex::new(mock::preset("maison")));
    let a_ws = serve(mock::router(a_mock)).await;
    let b_mock: MockState = Arc::new(Mutex::new(mock::preset("bureau")));
    let b_ws = serve(mock::router_with_mac(b_mock, "AA:BB:CC:DD:EE:FF")).await;
    let office = Node::start("scope", |_| {}).await;
    let (_, a) = office.call(Method::POST, "/api/installations", Some(json!({"name":"A","host":"127.0.0.1","port":a_ws}))).await;
    let (_, b) = office.call(Method::POST, "/api/installations", Some(json!({"name":"B","host":"127.0.0.1","port":b_ws}))).await;
    let share = |label: &str| json!({"label": label, "installation_ids": [a["id"]], "permission": "control", "url": office.base});
    let http = reqwest::Client::new();
    let neighbour = FakeNode::new("neighbour", 1);
    let list = "/api/federation/installations";

    // An expired invitation is refused.
    let (_, old) = office.call(Method::POST, "/api/shares", Some(share("Ancien"))).await;
    sqlx::query("UPDATE shares SET pairing_expires_at = 0").execute(&office.state.db).await.unwrap();
    let r = neighbour.pair(&http, &office.base, old["token"].as_str().unwrap()).await;
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    assert!(r.text().await.unwrap().contains("expirée"));

    // Pairing with a valid invitation: pending until approved.
    let (_, created) = office.call(Method::POST, "/api/shares", Some(share("Voisin"))).await;
    let token = created["token"].as_str().unwrap();
    // The request must be signed with the key it presents.
    let impostor = FakeNode { id: "neighbour".into(), key: ed25519_dalek::SigningKey::from_bytes(&[9; 32]) };
    {
        use base64::Engine;
        let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(token.trim_start_matches("bz2.")).unwrap();
        let inv: Value = serde_json::from_slice(&raw).unwrap();
        // Presents the neighbour's public key but signs with another key.
        let body = serde_json::to_vec(&json!({"code": inv["code"], "node_id": "neighbour", "name": "x",
            "public_key": breezone_server::federation::public_key_text(&neighbour.key)})).unwrap();
        let r = impostor.request(&http, &office.base, Method::POST, "/api/federation/pair", &body).send().await.unwrap();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    }
    let r = neighbour.pair(&http, &office.base, token).await;
    assert_eq!(r.status(), StatusCode::OK);
    let r = neighbour.request(&http, &office.base, Method::GET, list, b"").send().await.unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN, "pending approval");
    let share_id = created["share"]["id"].as_i64().unwrap();
    office.call(Method::POST, &format!("/api/shares/{share_id}/approve"), None).await;

    // Only installation A is visible; B cannot be driven.
    let snap: Value = neighbour.request(&http, &office.base, Method::GET, list, b"").send().await.unwrap().json().await.unwrap();
    let ids: Vec<i64> = snap["installations"].as_array().unwrap().iter().map(|i| i["id"].as_i64().unwrap()).collect();
    assert_eq!(ids, vec![a["id"].as_i64().unwrap()]);
    let path_b = format!("/api/federation/installations/{}/zones/1/1", b["id"]);
    let r = neighbour.request(&http, &office.base, Method::PATCH, &path_b, br#"{"on":false}"#).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::NOT_FOUND);

    // A captured request cannot be replayed.
    let path_a = format!("/api/federation/installations/{}/zones/1/1", a["id"]);
    let headers = breezone_server::federation::signed_headers(&neighbour.key, &neighbour.id, "PATCH", &path_a, br#"{"on":true}"#);
    let send = |body: &'static [u8]| {
        let mut req = http.patch(format!("{}{path_a}", office.base)).header("content-type", "application/json").body(body);
        for (n, v) in &headers {
            req = req.header(*n, v);
        }
        req.send()
    };
    assert_eq!(send(br#"{"on":true}"#).await.unwrap().status(), StatusCode::OK);
    assert_eq!(send(br#"{"on":true}"#).await.unwrap().status(), StatusCode::UNAUTHORIZED, "replay");
    // A modified body breaks the signature (fresh nonce, same signed body).
    let headers2 = breezone_server::federation::signed_headers(&neighbour.key, &neighbour.id, "PATCH", &path_a, br#"{"on":true}"#);
    let mut req = http.patch(format!("{}{path_a}", office.base)).header("content-type", "application/json").body(br#"{"on":false}"#.to_vec());
    for (n, v) in &headers2 {
        req = req.header(*n, v);
    }
    assert_eq!(req.send().await.unwrap().status(), StatusCode::UNAUTHORIZED, "tampered body");

    // Unsigned, a user token, another key: refused. The federation API gives no user API access.
    let r = http.get(format!("{}{list}", office.base)).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    let r = http.get(format!("{}{list}", office.base)).bearer_auth(&office.token).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    let r = impostor.request(&http, &office.base, Method::GET, list, b"").send().await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    let r = neighbour.request(&http, &office.base, Method::GET, "/api/status", b"").send().await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);

    // The invitation cannot be used twice.
    let r = FakeNode::new("other", 3).pair(&http, &office.base, token).await;
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);

    // Revoked: access stops.
    office.call(Method::DELETE, &format!("/api/shares/{share_id}"), None).await;
    let r = neighbour.request(&http, &office.base, Method::GET, list, b"").send().await.unwrap();
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
    // must not prevent linking it to the office installation of the account, and the zone names,
    // groups and history of the previous WebServer must not stick to the new one.
    let (_, before) = n.call(Method::GET, &format!("/api/installations/{id}"), None).await;
    assert!(before["zones"].as_array().unwrap().iter().any(|z| z["group"] == "Etage"));
    let office: MockState = Arc::new(Mutex::new(mock::preset("bureau")));
    let office_ws = serve(mock::router_with_mac(office, "AA:BB:CC:DD:EE:FF")).await;
    let (s, v) = n.call(Method::PUT, &format!("/api/installations/{id}"), Some(json!({"name":"Bureau","host":"127.0.0.1","port":office_ws}))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, v) = n.call(Method::PUT, &format!("/api/installations/{id}/cloud"), Some(json!({"cloud_id":"inst-office"}))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, list) = n.call(Method::GET, "/api/installations", None).await;
    assert_eq!(list[0]["mac"], "AA:BB:CC:DD:EE:FF");
    assert_eq!(list[0]["cloud_id"], "inst-office");
    let (_, after) = n.call(Method::GET, &format!("/api/installations/{id}"), None).await;
    let zones = after["zones"].as_array().unwrap();
    assert!(zones.iter().all(|z| z["group"] != "Etage" && z["group"] != "Rez de chaussée"), "groups of the home kept: {after}");
    assert_eq!(zones.iter().find(|z| z["zone_id"] == 1).unwrap()["name"], "Open space", "zone names of the office");
    let (_, history) = n.call(Method::GET, &format!("/api/installations/{id}/zones/1/1/history?hours=24"), None).await;
    assert!(history.as_array().unwrap().iter().all(|r| r["room_temp"] != 19.5), "history of the home kept: {history}");
    let (_, events) = n.call(Method::GET, "/api/events?limit=20", None).await;
    assert!(events.as_array().unwrap().iter().any(|e| e["message"].as_str().unwrap_or_default().contains("pointe vers un autre WebServer")));
}

#[tokio::test]
async fn a_reciprocal_invitation_links_both_ways_once_accepted() {
    let home_mock: MockState = Arc::new(Mutex::new(mock::preset("maison")));
    let home_ws = serve(mock::router_with_mac(home_mock.clone(), mock::MOCK_MAC)).await;
    let office_mock: MockState = Arc::new(Mutex::new(mock::preset("bureau")));
    let office_ws = serve(mock::router_with_mac(office_mock.clone(), "AA:BB:CC:DD:EE:FF")).await;
    let office = Node::start("rec-office", |c| c.scan_port = 9).await;
    let home = Node::start("rec-home", |c| c.scan_port = 9).await;
    for (node, name, port) in [(&office, "Bureau", office_ws), (&home, "Maison", home_ws)] {
        let (s, v) = node.call(Method::POST, "/api/installations", Some(json!({"name": name, "host": "127.0.0.1", "port": port}))).await;
        assert_eq!(s, StatusCode::CREATED, "{v}");
        let (s, v) = node.call(Method::PUT, "/api/node", Some(json!({"name": name, "public_url": node.base}))).await;
        assert_eq!(s, StatusCode::OK, "{v}");
    }
    let office_inst = office.call(Method::GET, "/api/installations", None).await.1[0]["id"].as_i64().unwrap();
    let home_inst = home.call(Method::GET, "/api/installations", None).await.1[0]["id"].as_i64().unwrap();

    // The office invites home and asks for a reciprocal share.
    let (s, created) = office
        .call(Method::POST, "/api/shares", Some(json!({"label":"Maison","installation_ids":[office_inst],"permission":"control","reciprocal":true})))
        .await;
    assert_eq!(s, StatusCode::CREATED, "{created}");
    assert_eq!(created["share"]["reciprocal"], true);
    let token = created["token"].as_str().unwrap().to_string();

    // Home cannot share an installation it does not drive (checked before the single-use code is
    // burnt, so the token still works below).
    let (s, v) = home
        .call(Method::POST, "/api/nodes", Some(json!({"token": token, "reciprocal": {"installation_ids": [9999], "permission": "read"}})))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");

    // Home accepts and shares its installation back (read only).
    let (s, node) = home
        .call(Method::POST, "/api/nodes", Some(json!({"token": token, "reciprocal": {"installation_ids": [home_inst], "permission": "read"}})))
        .await;
    assert_eq!(s, StatusCode::CREATED, "{node}");
    assert_eq!(node["status"]["state"], "pending");
    let (_, home_shares) = home.call(Method::GET, "/api/shares", None).await;
    assert_eq!(home_shares[0]["reverse"], true, "{home_shares}");
    assert_eq!(home_shares[0]["status"], "active");
    assert_eq!(home_shares[0]["remote_name"], "Bureau");

    // The office sees that home shares back; nothing is consumed before approval.
    let (_, shares) = office.call(Method::GET, "/api/shares", None).await;
    assert_eq!(shares[0]["status"], "pending");
    assert_eq!(shares[0]["remote_shares_back"], true, "{shares}");
    let (_, nodes) = office.call(Method::GET, "/api/nodes", None).await;
    assert!(nodes.as_array().unwrap().is_empty());

    // Approval links both ways.
    let share_id = shares[0]["id"].as_i64().unwrap();
    let (s, v) = office.call(Method::POST, &format!("/api/shares/{share_id}/approve"), None).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, nodes) = office.call(Method::GET, "/api/nodes", None).await;
    assert_eq!(nodes[0]["name"], "Maison", "{nodes}");
    assert_eq!(nodes[0]["status"]["state"], "online", "{nodes}");
    assert_eq!(nodes[0]["status"]["permission"], "read");
    assert_eq!(nodes[0]["installations"][0]["name"], "Maison");
    let node_id = node["id"].as_i64().unwrap();
    let (_, node) = home.call(Method::POST, &format!("/api/nodes/{node_id}/refresh"), None).await;
    assert_eq!(node["status"]["state"], "online", "{node}");
    assert_eq!(node["installations"][0]["name"], "Bureau");

    // Read only: the office cannot drive the home installation.
    let (_, status) = office.call(Method::GET, "/api/status", None).await;
    let remote = status.as_array().unwrap().iter().find(|i| i["access"] == "node").expect("home installation on the office node").clone();
    let (s, _) = office
        .call(Method::PATCH, &format!("/api/installations/{}/zones/1/1", remote["id"]), Some(json!({"setpoint": 21.5})))
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn sharing_back_needs_a_reciprocal_invitation() {
    let office_mock: MockState = Arc::new(Mutex::new(mock::preset("bureau")));
    let office_ws = serve(mock::router_with_mac(office_mock, "AA:BB:CC:DD:EE:FF")).await;
    let home_mock: MockState = Arc::new(Mutex::new(mock::preset("maison")));
    let home_ws = serve(mock::router(home_mock)).await;
    let office = Node::start("norec-office", |c| c.scan_port = 9).await;
    let home = Node::start("norec-home", |c| c.scan_port = 9).await;
    office.call(Method::POST, "/api/installations", Some(json!({"name":"Bureau","host":"127.0.0.1","port":office_ws}))).await;
    home.call(Method::POST, "/api/installations", Some(json!({"name":"Maison","host":"127.0.0.1","port":home_ws}))).await;
    office.call(Method::PUT, "/api/node", Some(json!({"name":"Bureau","public_url": office.base}))).await;
    let office_inst = office.call(Method::GET, "/api/installations", None).await.1[0]["id"].as_i64().unwrap();
    let home_inst = home.call(Method::GET, "/api/installations", None).await.1[0]["id"].as_i64().unwrap();
    let (_, created) = office.call(Method::POST, "/api/shares", Some(json!({"label":"Maison","installation_ids":[office_inst]}))).await;
    let token = created["token"].as_str().unwrap();
    let (s, v) = home
        .call(Method::POST, "/api/nodes", Some(json!({"token": token, "reciprocal": {"installation_ids": [home_inst], "url": home.base}})))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    // The code was not burnt: pairing without sharing back still works.
    let (s, v) = home.call(Method::POST, "/api/nodes", Some(json!({"token": token}))).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let (_, shares) = home.call(Method::GET, "/api/shares", None).await;
    assert!(shares.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn a_shared_installation_replaces_an_unreachable_local_copy() {
    // The office WebServer is shared by the office node…
    let office_mock: MockState = Arc::new(Mutex::new(mock::preset("bureau")));
    let office_ws = serve(mock::router_with_mac(office_mock, "AA:BB:CC:DD:EE:FF")).await;
    let office = Node::start("dup-office", |c| c.scan_port = 9).await;
    let home = Node::start("dup-home", |c| c.scan_port = 9).await;
    let (_, inst) = office.call(Method::POST, "/api/installations", Some(json!({"name":"Atelier","host":"127.0.0.1","port":office_ws}))).await;
    let office_inst = inst["id"].as_i64().unwrap();
    office.call(Method::PUT, "/api/node", Some(json!({"name":"Bureau","public_url": office.base}))).await;

    // …and home already knows it as a local installation it cannot reach from its network.
    let (s, local) = home.call(Method::POST, "/api/installations", Some(json!({"name":"Atelier (local)","host":"127.0.0.1","port":9}))).await;
    assert_eq!(s, StatusCode::CREATED, "{local}");
    let local_id = local["id"].as_i64().unwrap();
    breezone_server::db::set_installation_mac(&home.state.db, local_id, "AA:BB:CC:DD:EE:FF").await.unwrap();
    for _ in 0..3 {
        home.call(Method::POST, &format!("/api/installations/{local_id}/refresh"), None).await;
    }

    let (_, created) = office.call(Method::POST, "/api/shares", Some(json!({"label":"Maison","installation_ids":[office_inst]}))).await;
    let (_, node) = home.call(Method::POST, "/api/nodes", Some(json!({"token": created["token"]}))).await;
    let share_id = office.call(Method::GET, "/api/shares", None).await.1[0]["id"].as_i64().unwrap();
    office.call(Method::POST, &format!("/api/shares/{share_id}/approve"), None).await;
    let node_id = node["id"].as_i64().unwrap();
    let (_, node) = home.call(Method::POST, &format!("/api/nodes/{node_id}/refresh"), None).await;
    assert_eq!(node["status"]["state"], "online", "{node}");
    assert_eq!(node["installations"][0]["name"], "Atelier", "the shared copy is used: {node}");
    let (_, events) = home.call(Method::GET, "/api/events?limit=20", None).await;
    assert!(
        events.as_array().unwrap().iter().any(|e| e["message"].as_str().unwrap_or_default().contains("existe aussi sur ce serveur")),
        "{events}"
    );
}
