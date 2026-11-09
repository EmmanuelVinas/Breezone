//! Optional, read-only synchronisation with the user's Airzone Cloud account.
//!
//! The zones themselves (ids, names, temperatures, setpoints) live in the local WebServer and
//! are read by Breezone directly: no cloud is needed to drive them. What only exists in the
//! Airzone Cloud is the user's *organisation*: the list of installations with their name and
//! city, the groups of zones ("Étage", "Rez-de-chaussée") and their order.
//!
//! When the user enters their Airzone credentials once, Breezone:
//!   1. reads the installations, groups and zones from Airzone Cloud;
//!   2. scans the LAN and recognises each WebServer by its MAC address (`ws_id` in the cloud);
//!   3. creates the missing installations automatically, with their name, city and groups;
//!   4. lists the cloud installations whose WebServer is not on this network (another site);
//!   5. re-synchronises every day (if the user chose to remember the credentials). Daily syncs
//!      never override what the user customised in Breezone (names, groups).
//!
//! Credentials are optional and, when remembered, stored encrypted (ChaCha20-Poly1305).
//!
//! Cloud endpoints (from `aioairzone-cloud`):
//!   POST /api/v1/auth/login {email, password}  → {token, refreshToken}
//!   GET  /api/v1/installations                  → {installations: [{installation_id, name, ws_ids, location_text}]}
//!   GET  /api/v1/installations/{id}             → {groups: [{name, devices: [{type:"az_zone", ws_id, meta:{system_number, zone_number}, name}]}]}
//!   GET  /api/v1/user/logout

use crate::auth::random_bytes;
use crate::db::{self, CloudInstallationRow, InstallationAccess, ZoneMeta};
use crate::discovery;
use crate::i18n::Text;
use crate::error::{ApiError, ApiResult};
use crate::hub::InstallationView;
use crate::AppState;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

const KEY_EMAIL: &str = "cloud.email";
const KEY_PASSWORD: &str = "cloud.password";
const KEY_LAST_SYNC: &str = "cloud.last_sync";
const KEY_LAST_ERROR: &str = "cloud.last_error";
const AUTO_SYNC_PERIOD: Duration = Duration::from_secs(24 * 3600);

// ---------------------------------------------------------------------------------------
// Cloud model
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudZone {
    pub system_id: i64,
    pub zone_id: i64,
    pub name: Option<String>,
    pub group: Option<String>,
    pub position: i64,
    /// Airzone Cloud device id (schedules target devices).
    #[serde(default)]
    pub device_id: Option<String>,
}

/// A system device of a cloud installation (schedules may target a whole system).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudSystem {
    pub device_id: String,
    pub system_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudInstallation {
    pub cloud_id: String,
    pub name: String,
    pub location: Option<String>,
    pub ws_ids: Vec<String>,
    pub groups: Vec<String>,
    pub zones: Vec<CloudZone>,
    #[serde(default)]
    pub systems: Vec<CloudSystem>,
    /// Raw schedules and routines of the installation (read only when importing them).
    #[serde(skip)]
    pub schedules: Option<Value>,
    #[serde(skip)]
    pub routines: Option<Value>,
}

pub fn normalize_mac(mac: &str) -> String {
    mac.chars().filter(|c| c.is_ascii_hexdigit()).collect::<String>().to_uppercase()
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(String::from).filter(|x| !x.trim().is_empty())
}

/// Reads the account's installations, groups and zones from Airzone Cloud.
pub async fn fetch(base_url: &str, email: &str, password: &str) -> ApiResult<Vec<CloudInstallation>> {
    let session = CloudSession::login(base_url, base_url, email, password).await?;
    let result = fetch_account(&session, false).await;
    session.logout().await;
    result
}

/// A logged-in Airzone Cloud session. `base` serves the account data (`m.airzonecloud.com`),
/// `ctrl` the control endpoints used by the official app for schedules (`prodctrl…`).
pub struct CloudSession {
    http: reqwest::Client,
    base: String,
    ctrl: String,
    token: String,
}

fn cloud_err(e: reqwest::Error) -> ApiError {
    ApiError::bad_gateway(crate::text!("Airzone Cloud injoignable : {0}", "Airzone Cloud unreachable: {0}", "Airzone Cloud inaccesible: {0}", e))
}

impl CloudSession {
    pub async fn login(base_url: &str, ctrl_url: &str, email: &str, password: &str) -> ApiResult<CloudSession> {
        let base = base_url.trim_end_matches('/').to_string();
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let login = http
            .post(format!("{base}/api/v1/auth/login"))
            .json(&json!({ "email": email.trim(), "password": password }))
            .send()
            .await
            .map_err(cloud_err)?;
        let status = login.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::BAD_REQUEST || status == reqwest::StatusCode::NOT_FOUND {
            return Err(ApiError::bad_request("Airzone Cloud a refusé ces identifiants"));
        }
        if !status.is_success() {
            return Err(ApiError::bad_gateway(crate::text!(
                "connexion Airzone Cloud impossible (HTTP {0})",
                "cannot sign in to Airzone Cloud (HTTP {0})",
                "no se puede conectar con Airzone Cloud (HTTP {0})",
                status
            )));
        }
        let login: Value = login.json().await.map_err(cloud_err)?;
        let token = s(&login, "token").ok_or_else(|| ApiError::bad_gateway("Airzone Cloud n'a pas renvoyé de jeton"))?;
        Ok(CloudSession { http, base, ctrl: ctrl_url.trim_end_matches('/').to_string(), token })
    }

    async fn send(&self, req: reqwest::RequestBuilder, what: &str) -> ApiResult<Value> {
        let resp = req.bearer_auth(&self.token).send().await.map_err(cloud_err)?;
        let status = resp.status();
        let text = resp.text().await.map_err(cloud_err)?;
        if !status.is_success() {
            let detail: Value = serde_json::from_str(&text).unwrap_or_default();
            let msg = s(&detail, "msg").or_else(|| s(&detail, "_id")).or_else(|| s(&detail, "error")).unwrap_or_default();
            return Err(ApiError::bad_gateway(crate::text!(
                "Airzone Cloud a répondu HTTP {0} pour {1} {2}",
                "Airzone Cloud answered HTTP {0} for {1} {2}",
                "Airzone Cloud respondió HTTP {0} para {1} {2}",
                status,
                what,
                msg
            )));
        }
        Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    /// GET on the account API.
    pub async fn get(&self, path: &str) -> ApiResult<Value> {
        self.send(self.http.get(format!("{}/api/v1/{path}", self.base)), path).await
    }

    /// Request on the control API (schedules), like the official app.
    pub async fn ctrl(&self, method: reqwest::Method, path: &str, body: Option<&Value>) -> ApiResult<Value> {
        let mut req = self.http.request(method, format!("{}/api/v1/{path}", self.ctrl));
        if let Some(b) = body {
            req = req.json(b);
        }
        self.send(req, path).await
    }

    /// Best effort: close the cloud session.
    pub async fn logout(&self) {
        let _ = self.http.get(format!("{}/api/v1/user/logout", self.base)).bearer_auth(&self.token).send().await;
    }
}

/// Reads the account's installations (and, with `schedules`, their schedules: control API
/// first, account API as a fallback).
pub async fn fetch_account(session: &CloudSession, schedules: bool) -> ApiResult<Vec<CloudInstallation>> {
    let get = |path: String| async move { session.get(&path).await };

    let list = get("installations".into()).await?;
    let mut result = vec![];
    for inst in list["installations"].as_array().cloned().unwrap_or_default() {
        let Some(cloud_id) = s(&inst, "installation_id").or_else(|| s(&inst, "_id")) else { continue };
        let detail = get(format!("installations/{cloud_id}")).await?;
        let city = &inst["location_text"]["city"];
        let location = city["fr"].as_str().or_else(|| city["en"].as_str()).or_else(|| city.as_str()).map(String::from);
        let ws_ids: Vec<String> = inst["ws_ids"]
            .as_array()
            .or_else(|| detail["ws_ids"].as_array())
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        let mut groups = vec![];
        let mut zones: Vec<CloudZone> = vec![];
        let mut systems: Vec<CloudSystem> = vec![];
        let mut ws_from_devices = HashSet::new();
        for group in detail["groups"].as_array().cloned().unwrap_or_default() {
            let group_name = s(&group, "name");
            if let Some(g) = &group_name {
                groups.push(g.clone());
            }
            for (position, dev) in group["devices"].as_array().cloned().unwrap_or_default().iter().enumerate() {
                if let Some(ws) = s(dev, "ws_id") {
                    ws_from_devices.insert(ws);
                }
                let device_id = s(dev, "device_id").or_else(|| s(dev, "_id"));
                if dev["type"] == "az_system" {
                    if let (Some(id), Some(sys)) = (&device_id, dev["meta"]["system_number"].as_i64()) {
                        if !systems.iter().any(|x| &x.device_id == id) {
                            systems.push(CloudSystem { device_id: id.clone(), system_id: sys });
                        }
                    }
                }
                if dev["type"] != "az_zone" {
                    continue;
                }
                let (Some(sys), Some(zone)) = (dev["meta"]["system_number"].as_i64(), dev["meta"]["zone_number"].as_i64()) else {
                    continue;
                };
                // A zone may appear in several groups: keep its first group.
                if zones.iter().any(|z| z.system_id == sys && z.zone_id == zone) {
                    continue;
                }
                zones.push(CloudZone { system_id: sys, zone_id: zone, name: s(dev, "name"), group: group_name.clone(), position: position as i64, device_id });
            }
        }
        let mut ws_ids = ws_ids;
        for ws in ws_from_devices {
            if !ws_ids.contains(&ws) {
                ws_ids.push(ws);
            }
        }
        // Schedules and routines: optional (an error is reported by the import, not fatal).
        let (sched, routines) = if schedules {
            let path = format!("installations/{cloud_id}/schedules");
            let sched = match session.ctrl(reqwest::Method::GET, &path, None).await {
                Ok(v) => Some(v),
                Err(_) => get(path).await.ok(),
            };
            // Routines: same endpoints as the official app (`…/routines?user_units=0`).
            let mut routines = None;
            for path in [format!("installations/{cloud_id}/routines?user_units=0"), format!("installations/{cloud_id}/routines")] {
                if let Ok(v) = session.ctrl(reqwest::Method::GET, &path, None).await {
                    routines = Some(v);
                    break;
                }
                match session.get(&path).await {
                    Ok(v) => {
                        routines = Some(v);
                        break;
                    }
                    Err(e) => tracing::info!("Airzone routines of {cloud_id} not readable: {}", e.message.en),
                }
            }
            (sched, routines)
        } else {
            (None, None)
        };
        result.push(CloudInstallation {
            systems,
            schedules: sched,
            routines,
            cloud_id,
            name: s(&detail, "name").or_else(|| s(&inst, "name")).unwrap_or_else(|| "Installation".into()),
            location,
            ws_ids,
            groups,
            zones,
        });
    }
    Ok(result)
}

// ---------------------------------------------------------------------------------------
// Credentials (encrypted at rest)
// ---------------------------------------------------------------------------------------

pub struct Credentials {
    pub email: String,
    pub password: String,
}

/// Encryption key derived from the server secret (stored in the data directory).
pub fn derive_key(server_secret: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"breezone/airzone-cloud-credentials/");
    h.update(server_secret);
    let mut key = [0u8; 32];
    key.copy_from_slice(&h.finalize());
    key
}

pub fn encrypt(key: &[u8; 32], plain: &str) -> String {
    let cipher = ChaCha20Poly1305::new_from_slice(key).expect("32-byte key");
    let nonce_bytes = random_bytes::<12>();
    let nonce = Nonce::from(nonce_bytes);
    let ct = cipher.encrypt(&nonce, plain.as_bytes()).expect("encryption");
    let mut out = nonce_bytes.to_vec();
    out.extend(ct);
    STANDARD.encode(out)
}

pub fn decrypt(key: &[u8; 32], encoded: &str) -> Option<String> {
    let raw = STANDARD.decode(encoded).ok()?;
    if raw.len() < 13 {
        return None;
    }
    let cipher = ChaCha20Poly1305::new_from_slice(key).ok()?;
    let nonce_bytes: [u8; 12] = raw[..12].try_into().ok()?;
    let plain = cipher.decrypt(&Nonce::from(nonce_bytes), &raw[12..]).ok()?;
    String::from_utf8(plain).ok()
}

pub async fn load_credentials(state: &AppState) -> Option<Credentials> {
    let email = db::get_setting(&state.db, KEY_EMAIL).await.ok()??;
    let enc = db::get_setting(&state.db, KEY_PASSWORD).await.ok()??;
    let password = decrypt(&state.cloud_key, &enc)?;
    Some(Credentials { email, password })
}

async fn save_credentials(state: &AppState, c: &Credentials) -> ApiResult<()> {
    db::set_setting(&state.db, KEY_EMAIL, c.email.trim()).await?;
    db::set_setting(&state.db, KEY_PASSWORD, &encrypt(&state.cloud_key, &c.password)).await?;
    Ok(())
}

pub async fn forget(state: &AppState) -> ApiResult<()> {
    for k in [KEY_EMAIL, KEY_PASSWORD, KEY_LAST_SYNC, KEY_LAST_ERROR] {
        db::delete_setting(&state.db, k).await?;
    }
    db::clear_cloud_installations(&state.db).await?;
    db::clear_airzone_schedules(&state.db).await?;
    Ok(())
}

// ---------------------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------------------

/// Fixed local address of an installation's WebServer (otherwise found by the network scan).
#[derive(Debug, Clone, Serialize)]
pub struct AccessView {
    pub host: String,
    pub port: i64,
}

#[derive(Debug, Serialize)]
pub struct CloudInstallationView {
    pub cloud_id: String,
    pub name: String,
    pub location: Option<String>,
    pub zones: i64,
    /// Local Breezone installation driving it, or null if its WebServer was not reached.
    pub installation_id: Option<i64>,
    pub reachable: bool,
    /// MAC address(es) of its WebServer, as known by Airzone Cloud.
    pub ws_ids: Vec<String>,
    /// Fixed local address of its WebServer (null = found by scanning the server's network).
    pub access: Option<AccessView>,
}

#[derive(Debug, Serialize)]
pub struct CloudStatus {
    /// Credentials are remembered: automatic daily synchronisation is on.
    pub auto_sync: bool,
    pub email: Option<String>,
    pub last_sync: Option<i64>,
    pub last_error: Option<Text>,
    pub installations: Vec<CloudInstallationView>,
    /// Network scanned by this server to find the WebServers.
    pub default_subnet: Option<String>,
}

pub async fn status(state: &AppState, include_email: bool) -> ApiResult<CloudStatus> {
    let email = db::get_setting(&state.db, KEY_EMAIL).await?;
    let auto_sync = email.is_some() && db::get_setting(&state.db, KEY_PASSWORD).await?.is_some();
    let rows = db::list_cloud_installations(&state.db).await?;
    let access: HashMap<String, InstallationAccess> = db::list_access(&state.db).await?.into_iter().map(|a| (a.cloud_id.clone(), a)).collect();
    Ok(CloudStatus {
        auto_sync,
        email: email.filter(|_| include_email),
        last_sync: db::get_setting(&state.db, KEY_LAST_SYNC).await?.and_then(|v| v.parse().ok()),
        last_error: db::get_setting(&state.db, KEY_LAST_ERROR).await?.as_deref().map(Text::load),
        installations: rows
            .into_iter()
            .map(|r| CloudInstallationView {
                reachable: r.installation_id.is_some(),
                ws_ids: serde_json::from_str(&r.ws_ids).unwrap_or_default(),
                access: access.get(&r.cloud_id).map(|a| AccessView { host: a.host.clone(), port: a.port }),
                cloud_id: r.cloud_id,
                name: r.name,
                location: r.location,
                zones: r.zones,
                installation_id: r.installation_id,
            })
            .collect(),
        default_subnet: discovery::global_subnet(&state.db, state.cfg.scan_subnet.as_deref()).await,
    })
}

// ---------------------------------------------------------------------------------------
// Synchronisation
// ---------------------------------------------------------------------------------------

#[derive(Debug, Default, Serialize)]
pub struct SyncReport {
    /// Installations created automatically (WebServer found).
    pub created: Vec<String>,
    /// Existing installations updated with their cloud organisation.
    pub updated: Vec<String>,
    /// Cloud installations whose WebServer was not reached from this server.
    pub unreachable: Vec<String>,
    /// Why some installations were not reached, and what to configure.
    pub notes: Vec<Text>,
}

#[derive(Debug, Clone, Copy)]
pub struct SyncOptions {
    /// Apply cloud names and groups even over Breezone customisations (explicit import).
    pub overwrite: bool,
}

#[derive(Deserialize)]
pub struct ConnectRequest {
    pub email: String,
    pub password: String,
    /// Remember the credentials (encrypted) for the automatic daily synchronisation.
    #[serde(default = "yes")]
    pub remember: bool,
}

fn yes() -> bool {
    true
}

/// Connects the Airzone Cloud account: synchronises now and optionally remembers credentials.
pub async fn connect(state: &AppState, req: &ConnectRequest, actor: &str) -> ApiResult<SyncReport> {
    let creds = Credentials { email: req.email.trim().to_string(), password: req.password.clone() };
    if creds.email.is_empty() || creds.password.is_empty() {
        return Err(ApiError::bad_request("e-mail et mot de passe Airzone requis"));
    }
    let report = sync(state, &creds, SyncOptions { overwrite: true }, actor).await?;
    if req.remember {
        save_credentials(state, &creds).await?;
        crate::airzone_schedules::sync_soon(state);
    } else {
        db::set_setting(&state.db, KEY_EMAIL, &creds.email).await?;
        db::delete_setting(&state.db, KEY_PASSWORD).await?;
    }
    Ok(report)
}

/// Synchronises with the remembered credentials.
pub async fn sync_stored(state: &AppState, actor: &str) -> ApiResult<SyncReport> {
    let creds = load_credentials(state).await.ok_or_else(|| ApiError::bad_request("aucun compte Airzone mémorisé"))?;
    sync(state, &creds, SyncOptions { overwrite: false }, actor).await
}

pub async fn sync(state: &AppState, creds: &Credentials, opts: SyncOptions, actor: &str) -> ApiResult<SyncReport> {
    let result = async {
        let cloud = fetch(&state.cfg.airzone_cloud_url, &creds.email, &creds.password).await?;
        reconcile(state, &cloud, opts, actor).await
    }
    .await;
    match &result {
        Ok(_) => {
            db::set_setting(&state.db, KEY_LAST_SYNC, &db::now().to_string()).await?;
            db::delete_setting(&state.db, KEY_LAST_ERROR).await?;
        }
        Err(e) => {
            db::set_setting(&state.db, KEY_LAST_ERROR, &e.message.store()).await?;
            let text = crate::text!(
                "Synchronisation Airzone Cloud échouée : {0}",
                "Airzone Cloud synchronisation failed: {0}",
                "Sincronización con Airzone Cloud fallida: {0}",
                e.message.arg()
            );
            state.hub.log("system", "error", None, Some(actor), text, None).await;
        }
    }
    result
}

/// The cloud organisation saved at the last synchronisation.
async fn snapshot(state: &AppState) -> ApiResult<Vec<CloudInstallation>> {
    Ok(db::list_cloud_installations(&state.db)
        .await?
        .into_iter()
        .filter_map(|r| r.detail.and_then(|d| serde_json::from_str(&d).ok()))
        .collect())
}

#[derive(Debug, Deserialize)]
pub struct AccessInput {
    /// Fixed local address of the WebServer ("192.168.1.25"); empty = found by the network scan.
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub port: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct AttachResult {
    pub attached: bool,
    pub installation_id: Option<i64>,
    pub message: Text,
    pub report: SyncReport,
}

/// Sets (or clears) the fixed local address of a cloud installation's WebServer and tries to
/// reach it right away, using the organisation saved at the last synchronisation.
pub async fn set_access(state: &AppState, cloud_id: &str, input: &AccessInput, actor: &str) -> ApiResult<AttachResult> {
    let host = input
        .host
        .as_deref()
        .map(|h| h.trim().trim_start_matches("http://").trim_end_matches('/').to_string())
        .filter(|h| !h.is_empty());
    if let Some(h) = &host {
        if h.contains(['/', ' ', '?', '#']) {
            return Err(ApiError::bad_request("adresse invalide : indiquez une IP ou un nom d'hôte"));
        }
    }
    let port = input.port.unwrap_or(3000);
    if !(1..=65535).contains(&port) {
        return Err(ApiError::bad_request("port invalide"));
    }
    let cloud = snapshot(state).await?;
    let target = cloud
        .iter()
        .find(|c| c.cloud_id == cloud_id)
        .ok_or_else(|| ApiError::not_found("installation Airzone inconnue : synchronisez d'abord le compte Airzone"))?
        .name
        .clone();
    match &host {
        Some(h) => db::set_access(&state.db, cloud_id, h, port).await?,
        None => db::delete_access(&state.db, cloud_id).await?,
    }
    let report = reconcile(state, &cloud, SyncOptions { overwrite: false }, actor).await?;
    crate::airzone_schedules::sync_soon(state);
    let installation_id = db::list_cloud_installations(&state.db)
        .await?
        .into_iter()
        .find(|r| r.cloud_id == cloud_id)
        .and_then(|r| r.installation_id);
    let message = match installation_id {
        Some(_) => crate::text!("« {0} » est pilotée par ce serveur.", "“{0}” is driven by this server.", "«{0}» la controla este servidor.", target),
        None => report.notes.iter().find(|n| n.fr.starts_with(&format!("{target} :"))).cloned().unwrap_or_else(|| {
            crate::text!("« {0} » n'a pas été trouvée.", "“{0}” was not found.", "«{0}» no se encontró.", target)
        }),
    };
    Ok(AttachResult { attached: installation_id.is_some(), installation_id, message, report })
}

/// Matches the cloud installations with WebServers reachable from this server, creates or
/// updates the local installations, applies the cloud organisation and saves the snapshot.
async fn reconcile(state: &AppState, cloud: &[CloudInstallation], opts: SyncOptions, actor: &str) -> ApiResult<SyncReport> {
    let mut report = SyncReport::default();
    let access: HashMap<String, InstallationAccess> = db::list_access(&state.db).await?.into_iter().map(|a| (a.cloud_id.clone(), a)).collect();
    let cloud_macs: Vec<Vec<String>> = cloud.iter().map(|c| c.ws_ids.iter().map(|m| normalize_mac(m)).collect()).collect();

    // MAC addresses of the local WebServers.
    let locals = db::list_installations(&state.db).await?;
    let macs = futures_util::future::join_all(locals.iter().map(|l| async {
        let mac = if l.enabled { state.hub.webserver_mac(l.id).await } else { l.mac.clone() };
        (l.id, mac.map(|m| normalize_mac(&m)))
    }))
    .await;

    // 1. Already linked, or same MAC address.
    let mut matches: Vec<Option<i64>> = cloud
        .iter()
        .enumerate()
        .map(|(i, c)| {
            locals.iter().find(|l| l.cloud_id.as_deref() == Some(c.cloud_id.as_str())).map(|l| l.id).or_else(|| {
                macs.iter().find(|(_, m)| m.as_ref().is_some_and(|m| cloud_macs[i].contains(m))).map(|(id, _)| *id)
            })
        })
        .collect();
    let mut created_ids: HashSet<i64> = HashSet::new();
    let mut notes: HashMap<usize, Text> = HashMap::new();
    let mut known_hosts: HashSet<(String, i64)> = locals.iter().map(|l| (l.host.clone(), l.port)).collect();

    // 2. Installations with a fixed address: check it is the right WebServer (MAC address).
    for (i, c) in cloud.iter().enumerate() {
        let Some(a) = access.get(&c.cloud_id) else { continue };
        let host = a.host.clone();
        match discovery::probe(&host, a.port as u16).await {
            None => {
                notes.insert(
                    i,
                    crate::text!(
                        "{0} : aucun WebServer Airzone ne répond à {1}:{2}",
                        "{0}: no Airzone WebServer answers at {1}:{2}",
                        "{0}: ningún WebServer de Airzone responde en {1}:{2}",
                        c.name,
                        host,
                        a.port
                    ),
                );
            }
            Some(found) => {
                let mac = found.mac.as_deref().map(normalize_mac);
                if mac.as_ref().is_some_and(|m| !cloud_macs[i].is_empty() && !cloud_macs[i].contains(m)) {
                    notes.insert(
                        i,
                        crate::text!(
                            "{0} : le WebServer de {1}:{2} (MAC {3}) n'est pas celui de cette installation (attendu {4})",
                            "{0}: the WebServer at {1}:{2} (MAC {3}) is not the one of this installation (expected {4})",
                            "{0}: el WebServer de {1}:{2} (MAC {3}) no es el de esta instalación (se esperaba {4})",
                            c.name,
                            host,
                            a.port,
                            found.mac.clone().unwrap_or_default(),
                            c.ws_ids.join(", ")
                        ),
                    );
                    continue;
                }
                match matches[i] {
                    Some(id) => {
                        // Address changed: update the existing installation.
                        if let Some(l) = locals.iter().find(|l| l.id == id) {
                            if l.host != host || l.port != a.port {
                                db::update_installation(&state.db, id, &l.name, &host, a.port, l.enabled).await?;
                            }
                        }
                    }
                    None => {
                        let inst = db::insert_installation(&state.db, &c.name, &host, a.port, true).await?;
                        if let Some(m) = &found.mac {
                            db::set_installation_mac(&state.db, inst.id, m).await?;
                        }
                        known_hosts.insert((host.clone(), a.port));
                        matches[i] = Some(inst.id);
                        created_ids.insert(inst.id);
                        report.created.push(c.name.clone());
                    }
                }
            }
        }
    }

    // 3. Others: look for their WebServer (MAC address) on this server's network. A server only
    //    scans its own network; installations elsewhere are reached through another node.
    let pending: Vec<usize> = (0..cloud.len()).filter(|i| matches[*i].is_none() && !access.contains_key(&cloud[*i].cloud_id)).collect();
    if !pending.is_empty() {
        match discovery::global_subnet(&state.db, state.cfg.scan_subnet.as_deref()).await {
            None => {
                for i in pending {
                    notes.insert(
                        i,
                        crate::text!(
                            "{0} : indiquez le réseau local du serveur, ou l'adresse de son WebServer",
                            "{0}: enter the local network of the server, or the address of its WebServer",
                            "{0}: indique la red local del servidor, o la dirección de su WebServer",
                            cloud[i].name
                        ),
                    );
                }
            }
            Some(subnet) => match discovery::scan(Some(&subnet), state.cfg.scan_port).await {
                Err(e) => {
                    for i in pending {
                        notes.insert(
                            i,
                            crate::text!("{0} : analyse de {1} impossible ({2})", "{0}: cannot scan {1} ({2})", "{0}: no se puede analizar {1} ({2})", cloud[i].name, subnet, e),
                        );
                    }
                }
                Ok(found) => {
                    for i in pending {
                        let c = &cloud[i];
                        let hit = found.iter().find(|f| {
                            f.mac.as_deref().map(normalize_mac).is_some_and(|m| cloud_macs[i].contains(&m))
                                && !known_hosts.contains(&(f.host.clone(), f.port as i64))
                        });
                        match hit {
                            Some(f) => {
                                let inst = db::insert_installation(&state.db, &c.name, &f.host, f.port as i64, true).await?;
                                if let Some(mac) = &f.mac {
                                    db::set_installation_mac(&state.db, inst.id, mac).await?;
                                }
                                known_hosts.insert((f.host.clone(), f.port as i64));
                                matches[i] = Some(inst.id);
                                created_ids.insert(inst.id);
                                report.created.push(c.name.clone());
                            }
                            None => {
                                notes.insert(
                                    i,
                                    crate::text!(
                                        "{0} : WebServer {1} absent du réseau {2} (autre site ?)",
                                        "{0}: WebServer {1} not on network {2} (another site?)",
                                        "{0}: WebServer {1} ausente de la red {2} (¿otro sitio?)",
                                        c.name,
                                        c.ws_ids.first().cloned().unwrap_or_default(),
                                        subnet
                                    ),
                                );
                            }
                        }
                    }
                }
            },
        }
    }
    if !created_ids.is_empty() {
        state.hub.reload_installations().await?;
        for id in &created_ids {
            state.hub.poll_one(*id).await;
        }
    }

    // 4. Fallback for installations whose MAC could not be read: zone names.
    let snapshot = state.hub.snapshot().await;
    for (i, c) in cloud.iter().enumerate() {
        if matches[i].is_some() {
            continue;
        }
        let taken: HashSet<i64> = matches.iter().flatten().copied().collect();
        let best = snapshot
            .iter()
            .filter(|l| !taken.contains(&l.id))
            .map(|l| {
                let score = c
                    .zones
                    .iter()
                    .filter(|cz| {
                        l.zones.iter().any(|lz| {
                            lz.system_id == cz.system_id
                                && lz.zone_id == cz.zone_id
                                && lz.airzone_name.as_deref().map(str::to_lowercase) == cz.name.as_deref().map(str::to_lowercase)
                        })
                    })
                    .count();
                (l.id, score)
            })
            .max_by_key(|(_, score)| *score);
        if let Some((id, score)) = best {
            if score > 0 && score * 2 >= c.zones.len() {
                matches[i] = Some(id);
            }
        }
    }

    // 5. Apply the cloud organisation to the matched installations.
    for (i, c) in cloud.iter().enumerate() {
        let Some(id) = matches[i] else {
            report.unreachable.push(c.name.clone());
            if let Some(n) = notes.remove(&i) {
                report.notes.push(n);
            }
            continue;
        };
        let is_new = created_ids.contains(&id);
        let overwrite = opts.overwrite || is_new;
        let Some(local) = snapshot.iter().find(|l| l.id == id) else { continue };

        apply_organisation(state, c, local, overwrite).await?;
        if !is_new {
            report.updated.push(c.name.clone());
        }
    }
    state.hub.reload_installations().await?;

    // 6. Save the cloud installations and their organisation.
    let rows: Vec<CloudInstallationRow> = cloud
        .iter()
        .enumerate()
        .map(|(i, c)| CloudInstallationRow {
            cloud_id: c.cloud_id.clone(),
            name: c.name.clone(),
            location: c.location.clone(),
            ws_ids: serde_json::to_string(&c.ws_ids).unwrap_or_default(),
            zones: c.zones.len() as i64,
            installation_id: matches[i],
            synced_at: db::now(),
            detail: serde_json::to_string(c).ok(),
        })
        .collect();
    db::replace_cloud_installations(&state.db, &rows).await?;

    state
        .hub
        .log(
            "system",
            "info",
            None,
            Some(actor),
            crate::text!(
                "Installations Airzone : {0} créée(s), {1} mise(s) à jour, {2} non jointe(s)",
                "Airzone installations: {0} created, {1} updated, {2} not joined",
                "Instalaciones de Airzone: {0} creada(s), {1} actualizada(s), {2} no unida(s)",
                report.created.len(),
                report.updated.len(),
                report.unreachable.len()
            ),
            Some(json!(report)),
        )
        .await;
    Ok(report)
}

/// Applies the cloud organisation to a Breezone installation and links them: location,
/// groups, zone names (and the installation name if `overwrite`).
async fn apply_organisation(state: &AppState, c: &CloudInstallation, local: &InstallationView, overwrite: bool) -> ApiResult<()> {
    let id = local.id;
    db::set_installation_cloud_id(&state.db, id, &c.cloud_id).await?;
    db::set_cloud_installation_link(&state.db, &c.cloud_id, Some(id)).await?;
    db::set_installation_location(&state.db, id, c.location.as_deref()).await?;
    if overwrite && c.name.trim() != local.name {
        db::update_installation(&state.db, id, c.name.trim(), &local.host, local.port, local.enabled).await?;
    }
    // A single group (Airzone's default one, named like the installation) is not useful.
    let meaningful_groups = c.groups.len() > 1;
    for cz in &c.zones {
        let Some(lz) = local.zones.iter().find(|z| z.system_id == cz.system_id && z.zone_id == cz.zone_id) else { continue };
        let customised_name = lz.airzone_name.as_deref().is_some_and(|a| a != lz.name);
        let display_name = if customised_name {
            Some(lz.name.clone())
        } else if lz.airzone_name.is_none() {
            cz.name.clone()
        } else {
            None
        };
        let group = if meaningful_groups && (overwrite || lz.group.is_none()) { cz.group.clone() } else { lz.group.clone() };
        let position = if overwrite || lz.group.is_none() { cz.position } else { lz.position };
        state
            .hub
            .set_zone_meta(ZoneMeta { installation_id: id, system_id: cz.system_id, zone_id: cz.zone_id, display_name, group_name: group, position })
            .await?;
    }
    Ok(())
}

/// Links (or unlinks, with `None`) a Breezone installation to an installation of the Airzone
/// account, chosen by the user. Uses the organisation saved at the last synchronisation: no
/// Airzone login needed. Refused if the WebServer MAC address proves it is another installation.
pub async fn link(state: &AppState, installation_id: i64, cloud_id: Option<&str>, actor: &str) -> ApiResult<Option<String>> {
    let inst = db::installation_by_id(&state.db, installation_id).await?.ok_or_else(|| ApiError::not_found("installation not found"))?;
    let Some(cloud_id) = cloud_id.filter(|c| !c.is_empty()) else {
        db::clear_installation_cloud_link(&state.db, installation_id).await?;
        state.hub.reload_installations().await?;
        state.hub.log("system", "info", Some(installation_id), Some(actor), crate::text!(
            "« {0} » n'est plus associée au compte Airzone",
            "“{0}” is no longer linked to the Airzone account",
            "«{0}» ya no está asociada a la cuenta de Airzone",
            inst.name
        ), None).await;
        return Ok(None);
    };
    let cloud = snapshot(state).await?;
    let c = cloud
        .iter()
        .find(|c| c.cloud_id == cloud_id)
        .ok_or_else(|| ApiError::not_found("installation Airzone inconnue : synchronisez d'abord le compte Airzone"))?;
    if let Some(mac) = state.hub.current_webserver_mac(installation_id).await {
        let expected: Vec<String> = c.ws_ids.iter().map(|m| normalize_mac(m)).collect();
        if !expected.is_empty() && !expected.contains(&normalize_mac(&mac)) {
            return Err(ApiError::bad_request(crate::text!(
                "le WebServer de « {0} » (MAC {1}) n'est pas celui de « {2} » (attendu {3})",
                "the WebServer of “{0}” (MAC {1}) is not the one of “{2}” (expected {3})",
                "el WebServer de «{0}» (MAC {1}) no es el de «{2}» (se esperaba {3})",
                inst.name,
                mac,
                c.name,
                c.ws_ids.join(", ")
            )));
        }
    }
    // The cloud installation may have been linked to another (duplicate) installation.
    db::clear_cloud_link_of(&state.db, &c.cloud_id).await?;
    let local = state.hub.installation(installation_id).await.ok_or_else(|| ApiError::not_found("installation not found"))?;
    apply_organisation(state, c, &local, true).await?;
    state.hub.reload_installations().await?;
    crate::airzone_schedules::sync_soon(state);
    state
        .hub
        .log(
            "system",
            "info",
            Some(installation_id),
            Some(actor),
            crate::text!(
                "« {0} » associée à l'installation Airzone « {1} »",
                "“{0}” linked to the Airzone installation “{1}”",
                "«{0}» asociada a la instalación de Airzone «{1}»",
                inst.name,
                c.name
            ),
            None,
        )
        .await;
    Ok(Some(c.name.clone()))
}

/// Links an installation to the cloud installation with the same WebServer MAC address, if any
/// and not linked yet. Called when an installation is added or its address changes, so that it
/// does not show twice (once as the local installation, once as the cloud one).
pub async fn auto_link(state: &AppState, installation_id: i64, actor: &str) -> ApiResult<Option<String>> {
    let Some(inst) = db::installation_by_id(&state.db, installation_id).await? else { return Ok(None) };
    if inst.cloud_id.is_some() {
        return Ok(None);
    }
    let Some(mac) = state.hub.current_webserver_mac(installation_id).await.map(|m| normalize_mac(&m)) else { return Ok(None) };
    let rows = db::list_cloud_installations(&state.db).await?;
    let linked: HashSet<String> = db::list_installations(&state.db).await?.into_iter().filter_map(|i| i.cloud_id).collect();
    let cloud = snapshot(state).await?;
    let Some(c) = cloud.iter().find(|c| {
        c.ws_ids.iter().any(|m| normalize_mac(m) == mac)
            && !linked.contains(&c.cloud_id)
            && rows.iter().any(|r| r.cloud_id == c.cloud_id && r.installation_id.is_none_or(|i| i == installation_id))
    }) else {
        return Ok(None);
    };
    let Some(local) = state.hub.installation(installation_id).await else { return Ok(None) };
    apply_organisation(state, c, &local, false).await?;
    state.hub.reload_installations().await?;
    crate::airzone_schedules::sync_soon(state);
    state
        .hub
        .log(
            "system",
            "info",
            Some(installation_id),
            Some(actor),
            crate::text!(
                "« {0} » reconnue comme l'installation Airzone « {1} » (même WebServer)",
                "“{0}” recognised as the Airzone installation “{1}” (same WebServer)",
                "«{0}» reconocida como la instalación de Airzone «{1}» (mismo WebServer)",
                inst.name,
                c.name
            ),
            None,
        )
        .await;
    Ok(Some(c.name.clone()))
}

/// Links the installations added before the last synchronisation (or by hand) to the cloud.
pub async fn auto_link_all(state: &AppState) {
    let Ok(installations) = db::list_installations(&state.db).await else { return };
    for i in installations.into_iter().filter(|i| i.cloud_id.is_none() && i.enabled) {
        if let Err(e) = auto_link(state, i.id, "Breezone").await {
            tracing::warn!("cannot link installation {} to Airzone Cloud: {}", i.id, e.message.en);
        }
    }
}

/// Daily synchronisation when credentials are remembered.
pub async fn run_auto_sync(state: AppState) {
    tokio::time::sleep(Duration::from_secs(20)).await;
    auto_link_all(&state).await;
    // Installations shared by another node appear once it accepted the pairing: link them to
    // the Airzone account too (cheap: MAC addresses are known).
    {
        let state = state.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(300));
            loop {
                ticker.tick().await;
                auto_link_all(&state).await;
            }
        });
    }
    let mut ticker = tokio::time::interval(AUTO_SYNC_PERIOD);
    loop {
        ticker.tick().await;
        if load_credentials(&state).await.is_some() {
            if let Err(e) = sync_stored(&state, "Synchronisation automatique").await {
                tracing::warn!("automatic Airzone Cloud sync failed: {}", e.message.en);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_macs() {
        assert_eq!(normalize_mac("a8:03:2a:00:fe:ed"), "A8032A00FEED");
        assert_eq!(normalize_mac("A8-03-2A-00-FE-ED"), "A8032A00FEED");
    }

    #[test]
    fn credentials_encryption_roundtrip() {
        let key = derive_key(b"server-secret");
        let enc = encrypt(&key, "mon mot de passe");
        assert_ne!(enc, "mon mot de passe");
        assert_eq!(decrypt(&key, &enc).as_deref(), Some("mon mot de passe"));
        assert_eq!(decrypt(&derive_key(b"other"), &enc), None);
        assert_ne!(encrypt(&key, "x"), encrypt(&key, "x"), "random nonce");
    }
}
