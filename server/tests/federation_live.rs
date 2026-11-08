//! Live link between nodes (long polling): changes on the exposing node reach the other one
//! within seconds, without periodic reads, and the link comes back by itself whichever side was
//! down — one node, the other, or both, in either order.
//!
//! Each server sits behind a TCP relay on a fixed port: stopping the relay cuts the server off
//! (open waits included), as a crash or a network loss would; a restart opens a new server on the
//! same data and the same port.

use breezone_server::config::Config;
use breezone_server::mock::{self, MockState};
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

fn temp_dir(name: &str) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("breezone-live-{name}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

async fn serve(router: axum::Router) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    port
}

/// Forwards a fixed port to a server; dropping it closes the port and every open connection.
struct Relay(JoinHandle<()>);

impl Relay {
    async fn start(port: u16, target: u16) -> Relay {
        let listener = loop {
            match TcpListener::bind(("127.0.0.1", port)).await {
                Ok(l) => break l,
                Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        };
        Relay(tokio::spawn(async move {
            let mut conns = tokio::task::JoinSet::new();
            while let Ok((mut inbound, _)) = listener.accept().await {
                conns.spawn(async move {
                    if let Ok(mut outbound) = TcpStream::connect(("127.0.0.1", target)).await {
                        let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
                    }
                });
            }
        }))
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// A Breezone server that can go down and come back on the same data and address.
struct Server {
    dir: PathBuf,
    port: u16,
    base: String,
    http: reqwest::Client,
    token: String,
    state: Option<breezone_server::AppState>,
    relay: Option<Relay>,
    links: Option<JoinHandle<()>>,
}

impl Server {
    async fn new(name: &str) -> Server {
        let port = TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap().port();
        let mut server = Server {
            dir: temp_dir(name),
            port,
            base: format!("http://127.0.0.1:{port}"),
            http: reqwest::Client::new(),
            token: String::new(),
            state: None,
            relay: None,
            links: None,
        };
        server.start().await;
        let v: Value = server
            .http
            .post(format!("{}/api/setup", server.base))
            .json(&json!({"username": "admin", "password": "motdepasse1"}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        server.token = v["access_token"].as_str().unwrap().into();
        server
    }

    /// Starts (or restarts) the server, with its live links to other nodes.
    async fn start(&mut self) {
        let mut cfg = Config::for_tests(self.dir.clone());
        cfg.scan_port = 9;
        let state = breezone_server::init_state(cfg).await.unwrap();
        let app_port = serve(breezone_server::app(state.clone())).await;
        self.relay = Some(Relay::start(self.port, app_port).await);
        self.links = Some(tokio::spawn(state.hub.clone().run_node_links()));
        self.state = Some(state);
    }

    /// Cuts the server off: its address stops answering, its links stop.
    fn stop(&mut self) {
        self.relay = None;
        if let Some(links) = self.links.take() {
            links.abort();
        }
        self.state = None;
    }

    fn hub(&self) -> &Arc<breezone_server::hub::Hub> {
        &self.state.as_ref().expect("server running").hub
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

/// Waits until `check` holds, for at most `secs` seconds.
async fn eventually(secs: u64, mut check: impl AsyncFnMut() -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if check().await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    false
}

/// The office shares its installation with home; home follows it live.
struct Sites {
    office: Server,
    home: Server,
    mock: MockState,
    node_id: i64,
}

impl Sites {
    async fn new(name: &str) -> Sites {
        let mock: MockState = Arc::new(Mutex::new(mock::preset("bureau")));
        let ws = serve(mock::router_with_mac(mock.clone(), "AA:BB:CC:DD:EE:FF")).await;
        let office = Server::new(&format!("{name}-office")).await;
        let home = Server::new(&format!("{name}-home")).await;

        let (_, inst) = office.call(Method::POST, "/api/installations", Some(json!({"name":"Atelier","host":"127.0.0.1","port":ws}))).await;
        let office_inst = inst["id"].as_i64().unwrap();
        office.call(Method::PUT, "/api/node", Some(json!({"name":"Bureau","public_url": office.base}))).await;
        let (_, created) = office
            .call(Method::POST, "/api/shares", Some(json!({"label":"Maison","installation_ids":[office_inst],"permission":"control"})))
            .await;
        let (s, node) = home.call(Method::POST, "/api/nodes", Some(json!({"token": created["token"]}))).await;
        assert_eq!(s, StatusCode::CREATED, "{node}");
        let node_id = node["id"].as_i64().unwrap();
        // Home's link keeps retrying while the office has not accepted yet…
        let (_, shares) = office.call(Method::GET, "/api/shares", None).await;
        let share_id = shares[0]["id"].as_i64().unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;
        office.call(Method::POST, &format!("/api/shares/{share_id}/approve"), None).await;
        office.hub().poll_all().await;
        let sites = Sites { office, home, mock, node_id };
        // …and comes up by itself once accepted.
        assert!(sites.online(25).await, "the link comes up once the share is accepted");
        sites
    }

    async fn node_state(&self) -> String {
        let (_, nodes) = self.home.call(Method::GET, "/api/nodes", None).await;
        nodes.as_array().unwrap().iter().find(|n| n["id"] == self.node_id).map(|n| n["status"]["state"].as_str().unwrap().to_string()).unwrap_or_default()
    }

    async fn online(&self, secs: u64) -> bool {
        eventually(secs, async || self.node_state().await == "online").await
    }

    async fn offline(&self, secs: u64) -> bool {
        eventually(secs, async || self.node_state().await == "offline").await
    }

    /// Setpoint of zone 1 as home sees it.
    async fn home_setpoint(&self) -> Option<f64> {
        let (_, status) = self.home.call(Method::GET, "/api/status", None).await;
        status
            .as_array()?
            .iter()
            .filter(|i| i["access"] == "node")
            .flat_map(|i| i["zones"].as_array().cloned().unwrap_or_default())
            .find(|z| z["zone_id"] == 1)
            .and_then(|z| z["setpoint"].as_f64())
    }

    /// Someone changes the setpoint on the office thermostat; the office reads its WebServer.
    async fn change_at_office(&self, value: f64) {
        if let Some(z) = self.mock.lock().unwrap().iter_mut().find(|z| z.zone_id == 1) {
            z.setpoint = Some(value);
        }
        self.office.hub().poll_all().await;
    }

    /// A change at the office shows at home within seconds (no periodic read in tests).
    async fn live(&self, value: f64) -> bool {
        self.change_at_office(value).await;
        eventually(8, async || self.home_setpoint().await == Some(value)).await
    }
}

#[tokio::test]
async fn an_idle_link_stays_online_and_carries_changes() {
    let sites = Sites::new("idle").await;
    assert!(sites.live(24.5).await, "first change");
    // Several waits end with "nothing new" (2 s each in tests): the node stays online.
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert_eq!(sites.node_state().await, "online");
    assert!(sites.live(23.0).await, "change after idle waits");
}

#[tokio::test]
async fn the_link_comes_back_when_the_exposing_node_returns() {
    let mut sites = Sites::new("office-down").await;
    sites.office.stop();
    assert!(sites.offline(15).await, "home sees the office down");
    sites.office.start().await;
    sites.office.hub().poll_all().await;
    assert!(sites.online(15).await, "home reconnects by itself");
    assert!(sites.live(22.0).await, "changes flow again");
}

#[tokio::test]
async fn the_link_survives_a_restart_of_the_exposing_node_during_a_wait() {
    let mut sites = Sites::new("office-restart").await;
    // Home is waiting for a change when the office restarts.
    sites.office.stop();
    sites.office.start().await;
    assert!(sites.online(15).await);
    assert!(sites.live(25.5).await, "changes flow after the restart");
}

#[tokio::test]
async fn a_restarted_node_catches_up_and_follows_again() {
    let mut sites = Sites::new("home-restart").await;
    sites.home.stop();
    // The office changes while home is down.
    sites.change_at_office(21.0).await;
    sites.home.start().await;
    assert!(sites.online(15).await, "home opens its link again on start");
    assert!(eventually(8, async || sites.home_setpoint().await == Some(21.0)).await, "home catches up on what changed meanwhile");
    assert!(sites.live(19.5).await);
}

#[tokio::test]
async fn both_nodes_down_the_exposing_one_back_first() {
    let mut sites = Sites::new("both-office-first").await;
    sites.home.stop();
    sites.office.stop();
    sites.office.start().await;
    sites.office.hub().poll_all().await;
    sites.home.start().await;
    assert!(sites.online(15).await);
    assert!(sites.live(20.5).await);
}

#[tokio::test]
async fn both_nodes_down_the_following_one_back_first() {
    let mut sites = Sites::new("both-home-first").await;
    sites.home.stop();
    sites.office.stop();
    sites.home.start().await;
    // The office is still down: home keeps trying.
    assert!(sites.offline(15).await, "home sees the office down after its own restart");
    sites.office.start().await;
    sites.office.hub().poll_all().await;
    assert!(sites.online(15).await, "home connects as soon as the office is back");
    assert!(sites.live(26.0).await);
}
