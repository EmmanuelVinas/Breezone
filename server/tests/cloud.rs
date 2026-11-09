//! Airzone Cloud synchronisation: automatic creation of installations found on the LAN,
//! groups import, installations on other sites, non-destructive daily sync.

use breezone_server::config::Config;
use breezone_server::mock;
use reqwest::StatusCode;
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
async fn cloud_sync_creates_installations_and_groups() {
    // Fake local WebServer (MAC A8:03:2A:00:FE:ED) and fake Airzone Cloud.
    let airzone_port = serve(mock::router(Arc::new(Mutex::new(mock::preset("maison"))))).await;
    // The office WebServer, on "another network": not found by the default scan.
    let office_port = serve(mock::router_with_mac(Arc::new(Mutex::new(mock::preset("bureau"))), "AA:BB:CC:DD:EE:FF")).await;
    let cloud_port = serve(mock::cloud_router(mock::cloud_account("A8:03:2A:00:FE:ED"))).await;

    let dir = std::env::temp_dir().join(format!("breezone-cloud-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut cfg = Config::for_tests(dir);
    cfg.airzone_cloud_url = format!("http://127.0.0.1:{cloud_port}");
    cfg.scan_subnet = Some("127.0.0.0/30".into());
    cfg.scan_port = airzone_port;
    let state = breezone_server::init_state(cfg).await.unwrap();
    let port = serve(breezone_server::app(state)).await;

    let mut c = Client { base: format!("http://127.0.0.1:{port}"), http: reqwest::Client::new(), token: None };
    let (_, v) = c.call(reqwest::Method::POST, "/api/setup", Some(json!({"username":"admin","password":"motdepasse1"}))).await;
    c.token = Some(v["access_token"].as_str().unwrap().into());

    // Wrong Airzone credentials.
    let (s, v) = c.call(reqwest::Method::PUT, "/api/cloud", Some(json!({"email": mock::MOCK_CLOUD_EMAIL, "password": "nope"}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");

    // Connect: the WebServer is found on the LAN by its MAC and created automatically.
    let (s, v) = c
        .call(reqwest::Method::PUT, "/api/cloud", Some(json!({"email": mock::MOCK_CLOUD_EMAIL, "password": mock::MOCK_CLOUD_PASSWORD})))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["report"]["created"], json!(["Maison"]));
    assert_eq!(v["report"]["unreachable"], json!(["Atelier"]));
    let note = v["report"]["notes"][0].as_str().unwrap();
    assert!(note.starts_with("Atelier: WebServer AABBCCDDEEFF not on network 127.0.0.0/30"), "English without Accept-Language: {note}");
    assert_eq!(v["status"]["auto_sync"], true);

    let (_, status) = c.call(reqwest::Method::GET, "/api/status", None).await;
    let insts = status.as_array().unwrap();
    assert_eq!(insts.len(), 1);
    let maison = &insts[0];
    assert_eq!(maison["name"], "Maison");
    assert_eq!(maison["location"], "Annecy, Haute-Savoie");
    assert_eq!(maison["connection"]["state"], "online");
    let zone = |name: &str| maison["zones"].as_array().unwrap().iter().find(|z| z["name"] == name).cloned().unwrap();
    assert_eq!(zone("Chambre 2")["group"], "Etage");
    assert_eq!(zone("Salon")["group"], "Rez de chaussée");
    // Zones are ordered by group then cloud order.
    let names: Vec<&str> = maison["zones"].as_array().unwrap().iter().map(|z| z["name"].as_str().unwrap()).collect();
    assert_eq!(&names[..3], &["Chambre 2", "Chambre 1", "Dressing"]);

    // The other site is listed, not reachable from this server.
    let (_, cloud) = c.call(reqwest::Method::GET, "/api/cloud", None).await;
    let list = cloud["installations"].as_array().unwrap();
    assert_eq!(list.len(), 2);
    let mkp = list.iter().find(|i| i["name"] == "Atelier").unwrap();
    assert_eq!(mkp["reachable"], false);
    assert_eq!(mkp["location"], "Lyon, Rhône");
    assert_eq!(cloud["email"], mock::MOCK_CLOUD_EMAIL);

    // Per-installation access point. A WebServer with the wrong MAC address is refused…
    let (s, v) = c
        .call(reqwest::Method::PUT, "/api/cloud/installations/inst-office/access", Some(json!({"host":"127.0.0.1","port":airzone_port})))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["result"]["attached"], false);
    assert!(v["result"]["message"].as_str().unwrap().contains("is not the one of this installation"), "{v}");
    // …the right address attaches the installation, with its cloud organisation (no new login).
    let (s, v) = c
        .call(reqwest::Method::PUT, "/api/cloud/installations/inst-office/access", Some(json!({"host":"127.0.0.1","port":office_port})))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["result"]["attached"], true, "{v}");
    let mkp = v["status"]["installations"].as_array().unwrap().iter().find(|i| i["cloud_id"] == "inst-office").unwrap().clone();
    assert_eq!(mkp["reachable"], true);
    assert_eq!(mkp["access"]["port"], office_port);
    let (_, all) = c.call(reqwest::Method::GET, "/api/status", None).await;
    let office = all.as_array().unwrap().iter().find(|i| i["name"] == "Atelier").unwrap();
    assert_eq!(office["location"], "Lyon, Rhône");
    assert_eq!(office["zones"].as_array().unwrap().len(), 3);
    // Unknown cloud installation / invalid input.
    let (s, _) = c.call(reqwest::Method::PUT, "/api/cloud/installations/nope/access", Some(json!({"host":"127.0.0.1"}))).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = c.call(reqwest::Method::PUT, "/api/cloud/installations/inst-office/access", Some(json!({"host":"bad host/"}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // A Breezone customisation survives the daily (non-overwriting) synchronisation.
    let id = maison["id"].as_i64().unwrap();
    let (s, _) = c.call(reqwest::Method::PUT, &format!("/api/installations/{id}/zones/1/1/meta"), Some(json!({"group":"RDC"}))).await;
    assert_eq!(s, StatusCode::OK);
    let (s, v) = c.call(reqwest::Method::POST, "/api/cloud/sync", None).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["report"]["created"], json!([]));
    let mut updated: Vec<String> = serde_json::from_value(v["report"]["updated"].clone()).unwrap();
    updated.sort();
    assert_eq!(updated, vec!["Atelier".to_string(), "Maison".to_string()], "both stay linked");
    let (_, inst) = c.call(reqwest::Method::GET, &format!("/api/installations/{id}"), None).await;
    let salon = inst["zones"].as_array().unwrap().iter().find(|z| z["name"] == "Salon").unwrap();
    assert_eq!(salon["group"], "RDC");

    // Forget the account.
    let (s, _) = c.call(reqwest::Method::DELETE, "/api/cloud", None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, cloud) = c.call(reqwest::Method::GET, "/api/cloud", None).await;
    assert_eq!(cloud["auto_sync"], false);
    assert_eq!(cloud["installations"], json!([]));
    let (s, _) = c.call(reqwest::Method::POST, "/api/cloud/sync", None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn airzone_schedules_are_mirrored_and_edited_on_airzone_cloud() {
    let airzone_port = serve(mock::router(Arc::new(Mutex::new(mock::preset("maison"))))).await;
    let cloud_port = serve(mock::cloud_router(mock::cloud_account("A8:03:2A:00:FE:ED"))).await;
    let dir = std::env::temp_dir().join(format!("breezone-sched-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut cfg = Config::for_tests(dir);
    cfg.airzone_cloud_url = format!("http://127.0.0.1:{cloud_port}");
    cfg.scan_port = airzone_port;
    let state = breezone_server::init_state(cfg).await.unwrap();
    let port = serve(breezone_server::app(state)).await;
    let mut c = Client { base: format!("http://127.0.0.1:{port}"), http: reqwest::Client::new(), token: None };
    let (_, v) = c.call(reqwest::Method::POST, "/api/setup", Some(json!({"username":"admin","password":"motdepasse1"}))).await;
    c.token = Some(v["access_token"].as_str().unwrap().into());

    // No Airzone account yet.
    let (s, _) = c.call(reqwest::Method::POST, "/api/airzone/schedules/sync", None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, v) = c
        .call(reqwest::Method::PUT, "/api/cloud", Some(json!({"email": mock::MOCK_CLOUD_EMAIL, "password": mock::MOCK_CLOUD_PASSWORD})))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");

    let (s, r) = c.call(reqwest::Method::POST, "/api/airzone/schedules/sync", None).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["account"], true);
    let list = r["schedules"].as_array().unwrap();
    assert_eq!(list.len(), 4);
    let find = |r: &Value, name: &str| r["schedules"].as_array().unwrap().iter().find(|s| s["name"] == name).cloned();
    let wake = find(&r, "Réveil").unwrap();
    assert_eq!(wake["days"], json!([1, 2, 3, 4, 5]));
    assert_eq!(wake["time"], "06:30");
    assert_eq!(wake["zones"], json!([{"system_id":1,"zone_id":3},{"system_id":1,"zone_id":4}]));
    assert_eq!(wake["action"]["on"], true);
    assert_eq!(wake["action"]["setpoint"], 20.5);
    assert_eq!(wake["editable"], true);
    assert!(wake["installation_id"].is_i64(), "linked to the local installation");
    assert_eq!(find(&r, "Vacances").unwrap()["editable"], false);
    assert_eq!(find(&r, "Ouverture").unwrap()["installation_id"], Value::Null, "other site, still listed");
    // Breezone does not run them: no Breezone routine.
    let (_, routines) = c.call(reqwest::Method::GET, "/api/routines", None).await;
    assert_eq!(routines, json!([]));

    // Edit → written on Airzone Cloud, then read back.
    let id = wake["id"].as_str().unwrap();
    let (s, r) = c
        .call(
            reqwest::Method::PUT,
            &format!("/api/airzone/schedules/inst-home/{id}"),
            Some(json!({ "name": "Réveil tard", "days": [6, 7], "time": "08:45",
                         "zones": [{"system_id":1,"zone_id":1}], "action": {"mode":"heating","setpoint":21} })),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let edited = find(&r, "Réveil tard").unwrap();
    assert_eq!(edited["days"], json!([6, 7]));
    assert_eq!(edited["time"], "08:45");
    assert_eq!(edited["action"]["mode"], "heating");
    assert_eq!(edited["zones"], json!([{"system_id":1,"zone_id":1}]));
    assert!(edited.get("end").is_some());

    // Disable, create, delete.
    let (_, r) = c.call(reqwest::Method::PUT, &format!("/api/airzone/schedules/inst-home/{id}/enabled"), Some(json!({"enabled": false}))).await;
    assert_eq!(find(&r, "Réveil tard").unwrap()["enabled"], false);
    let (s, r) = c
        .call(
            reqwest::Method::POST,
            "/api/airzone/schedules",
            Some(json!({ "cloud_id": "inst-home", "name": "Extinction", "days": [1,2,3,4,5], "time": "23:00",
                         "zones": [{"system_id":1,"zone_id":2}], "action": {"on": false} })),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let off = find(&r, "Extinction").unwrap();
    assert_eq!(off["action"]["on"], false);
    let (s, r) = c.call(reqwest::Method::DELETE, &format!("/api/airzone/schedules/inst-home/{}", off["id"].as_str().unwrap()), None).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert!(find(&r, "Extinction").is_none());
    // Unknown zone refused.
    let (s, _) = c
        .call(
            reqwest::Method::POST,
            "/api/airzone/schedules",
            Some(json!({ "cloud_id": "inst-home", "name": "X", "days": [1], "time": "10:00", "zones": [{"system_id":9,"zone_id":9}], "action": {"on": true} })),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // A user who drives the installation but may not edit schedules: switching on and off only.
    let home = wake["installation_id"].as_i64().unwrap();
    let (s, v) = c
        .call(
            reqwest::Method::POST,
            "/api/users",
            Some(json!({"username":"camille","password":"motdepasse1","access":[{"installation_id": home, "permission":"control",
                        "rights": {"zones": true, "routines": true, "schedules": false}}]})),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let (_, v) = c.call(reqwest::Method::POST, "/api/auth/login", Some(json!({"username":"camille","password":"motdepasse1"}))).await;
    let user = Client { base: c.base.clone(), http: c.http.clone(), token: Some(v["access_token"].as_str().unwrap().into()) };
    let (s, v) = user.call(reqwest::Method::PUT, &format!("/api/airzone/schedules/inst-home/{id}"), Some(json!({ "name": "X", "days": [1], "time": "10:00", "zones": [{"system_id":1,"zone_id":1}], "action": {"on": true} }))).await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{v}");
    let (s, _) = user.call(reqwest::Method::DELETE, &format!("/api/airzone/schedules/inst-home/{id}"), None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, v) = user.call(reqwest::Method::PUT, &format!("/api/airzone/schedules/inst-home/{id}/enabled"), Some(json!({"enabled": true}))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
}
