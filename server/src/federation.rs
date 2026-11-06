//! Federation between Breezone nodes (ADR 0008).
//!
//! A node only reaches the Airzone WebServers of its own network. To drive several sites from
//! one instance, a node **exposes** some of its local installations to another node through a
//! *share* (a secret key limited to these installations, read-only or with control), and
//! **consumes** the installations other nodes expose to it.
//!
//! * The exposing node creates the share and gives an invitation token
//!   `bz1.<base64url(JSON {v, node_id, name, url, key})>` (also shown as a QR code). Only the
//!   SHA-256 of the key is stored.
//! * The consuming node is given the token, stores the URL and the key (encrypted), calls
//!   `POST /api/federation/hello` and then polls `GET /api/federation/installations`. Commands go
//!   through `PATCH /api/federation/installations/{id}/zones/{system_id}/{zone_id}`.
//! * The exposing node stays the authority for its installations; the consuming node keeps a
//!   local copy of the history, like for its own installations.
//!
//! Only servers talk to each other: the key never reaches the phones.

use crate::airzone::ZoneData;
use crate::auth::{hash_token, random_bytes};
use crate::db::{self, Db};
use crate::error::ApiError;
use crate::hub::ZoneCommand;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::time::Duration;

const TOKEN_PREFIX: &str = "bz1.";
pub const KEY_PREFIX: &str = "bzk_";
const SETTING_NODE_ID: &str = "node.id";
const SETTING_NODE_NAME: &str = "node.name";
const SETTING_PUBLIC_URL: &str = "node.public_url";
/// Header carrying the user who sent a command through a node (for the activity feed).
pub const ACTOR_HEADER: &str = "x-breezone-actor";

// ---------------------------------------------------------------------------------------
// Identity of this node
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct Identity {
    /// Random, stable id of this node (generated at first use).
    pub node_id: String,
    pub name: String,
    /// Address other nodes use to reach this one (Cloudflare Tunnel, or a local address).
    pub public_url: Option<String>,
}

pub async fn identity(db: &Db) -> sqlx::Result<Identity> {
    let node_id = match db::get_setting(db, SETTING_NODE_ID).await? {
        Some(id) => id,
        None => {
            let id = hex::encode(random_bytes::<16>());
            db::set_setting(db, SETTING_NODE_ID, &id).await?;
            id
        }
    };
    Ok(Identity {
        node_id,
        name: db::get_setting(db, SETTING_NODE_NAME).await?.unwrap_or_else(|| "Breezone".into()),
        public_url: db::get_setting(db, SETTING_PUBLIC_URL).await?,
    })
}

pub async fn save_identity(db: &Db, name: &str, public_url: Option<&str>) -> Result<Identity, ApiError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 60 {
        return Err(ApiError::bad_request("le nom du serveur doit faire de 1 à 60 caractères"));
    }
    db::set_setting(db, SETTING_NODE_NAME, name).await?;
    match public_url.map(str::trim).filter(|u| !u.is_empty()) {
        Some(url) => db::set_setting(db, SETTING_PUBLIC_URL, &normalize_url(url)?).await?,
        None => db::delete_setting(db, SETTING_PUBLIC_URL).await?,
    }
    Ok(identity(db).await?)
}

// ---------------------------------------------------------------------------------------
// Invitation token and keys
// ---------------------------------------------------------------------------------------

/// Content of an invitation token.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Invitation {
    pub v: u8,
    pub node_id: String,
    pub name: String,
    pub url: String,
    pub key: String,
}

pub fn encode_token(inv: &Invitation) -> String {
    let json = serde_json::to_vec(inv).expect("serializable invitation");
    format!("{TOKEN_PREFIX}{}", URL_SAFE_NO_PAD.encode(json))
}

pub fn parse_token(token: &str) -> Result<Invitation, ApiError> {
    let invalid = || ApiError::bad_request("jeton d'invitation invalide : copiez-le en entier depuis Réglages › Partages du serveur qui partage");
    let compact: String = token.chars().filter(|c| !c.is_whitespace()).collect();
    let body = compact.strip_prefix(TOKEN_PREFIX).ok_or_else(invalid)?;
    let raw = URL_SAFE_NO_PAD.decode(body.trim_end_matches('=')).map_err(|_| invalid())?;
    let inv: Invitation = serde_json::from_slice(&raw).map_err(|_| invalid())?;
    if inv.v != 1 {
        return Err(ApiError::bad_request("version de jeton non prise en charge : mettez Breezone à jour"));
    }
    if !inv.key.starts_with(KEY_PREFIX) || inv.node_id.is_empty() {
        return Err(invalid());
    }
    Ok(Invitation { url: normalize_url(&inv.url)?, ..inv })
}

/// A new share key and its hash (the only thing stored by the exposing node).
pub fn new_share_key() -> (String, String) {
    let key = format!("{KEY_PREFIX}{}", URL_SAFE_NO_PAD.encode(random_bytes::<32>()));
    let hash = hash_token(&key);
    (key, hash)
}

// ---------------------------------------------------------------------------------------
// Addresses (ADR 0010: plain HTTP only towards private addresses)
// ---------------------------------------------------------------------------------------

/// Validates a node URL: `https://…`, or `http://…` towards a private address only.
pub fn normalize_url(url: &str) -> Result<String, ApiError> {
    let url = url.trim().trim_end_matches('/').to_string();
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| ApiError::bad_request("adresse invalide : elle doit commencer par https:// (ou http:// sur le réseau local)"))?;
    let authority = rest.split('/').next().unwrap_or_default();
    let host = host_of(authority);
    if host.is_empty() || rest.contains([' ', '?', '#']) {
        return Err(ApiError::bad_request("adresse invalide"));
    }
    match scheme {
        "https" => Ok(url),
        "http" if is_private_host(host) => Ok(url),
        "http" => Err(ApiError::bad_request("HTTP n'est accepté que vers une adresse locale : utilisez l'adresse https:// du serveur")),
        _ => Err(ApiError::bad_request("adresse invalide : https:// ou http:// attendu")),
    }
}

fn host_of(authority: &str) -> &str {
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    if let Some(v6) = authority.strip_prefix('[') {
        return v6.split(']').next().unwrap_or_default();
    }
    authority.split(':').next().unwrap_or_default()
}

/// Loopback, private, link-local addresses and local host names (`nas`, `nas.local`…).
pub fn is_private_host(host: &str) -> bool {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => ip.is_private() || ip.is_loopback() || ip.is_link_local(),
        Ok(IpAddr::V6(ip)) => {
            let first = ip.segments()[0];
            ip.is_loopback() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
        }
        Err(_) => {
            host == "localhost"
                || !host.contains('.')
                || [".local", ".lan", ".home", ".home.arpa", ".internal", ".localhost"].iter().any(|s| host.ends_with(s))
        }
    }
}

// ---------------------------------------------------------------------------------------
// Wire format
// ---------------------------------------------------------------------------------------

/// What a node exposes to one share.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FedSnapshot {
    pub node_id: String,
    pub name: String,
    /// "read" | "control"
    pub permission: String,
    pub installations: Vec<FedInstallation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FedInstallation {
    /// Id on the exposing node.
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub mac: Option<String>,
    pub connection: FedConnection,
    /// Zones as read from the WebServer (Airzone format).
    #[serde(default)]
    pub zones: Vec<ZoneData>,
    /// Names and groups set on the exposing node.
    #[serde(default)]
    pub meta: Vec<FedZoneMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FedConnection {
    /// "online" | "offline" | "unknown"
    pub state: String,
    #[serde(default)]
    pub last_seen: Option<i64>,
    #[serde(default)]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FedZoneMeta {
    pub system_id: i64,
    pub zone_id: i64,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub position: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hello {
    pub node_id: String,
    pub name: String,
}

// ---------------------------------------------------------------------------------------
// Client (consuming side)
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, thiserror::Error)]
pub enum NodeError {
    /// The node answered with an error (401: key revoked or expired, 403, 400…).
    #[error("{message}")]
    Rejected { status: u16, message: String },
    #[error("injoignable : {0}")]
    Unreachable(String),
}

impl NodeError {
    /// Connection state shown for the node.
    pub fn state(&self) -> &'static str {
        match self {
            NodeError::Rejected { status: 401, message } if message.contains("expired") => "expired",
            NodeError::Rejected { status: 401, .. } => "revoked",
            NodeError::Rejected { .. } => "error",
            NodeError::Unreachable(_) => "offline",
        }
    }
}

impl From<NodeError> for ApiError {
    fn from(e: NodeError) -> Self {
        match e {
            NodeError::Rejected { status: 400, message } => ApiError::bad_request(message),
            NodeError::Rejected { status: 401, message } => ApiError::bad_gateway(format!("le nœud a refusé la clé ({message})")),
            NodeError::Rejected { status: 403, message } => ApiError::forbidden(message),
            NodeError::Rejected { status: 404, message } => ApiError::not_found(message),
            other => ApiError::bad_gateway(other.to_string()),
        }
    }
}

#[derive(Clone)]
pub struct NodeClient {
    http: reqwest::Client,
    url: String,
    key: String,
}

impl std::fmt::Debug for NodeClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the key.
        f.debug_struct("NodeClient").field("url", &self.url).finish()
    }
}

impl NodeClient {
    pub fn new(url: &str, key: &str) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(4))
            .timeout(Duration::from_secs(10))
            .user_agent(concat!("Breezone/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default();
        Self { http, url: url.trim_end_matches('/').to_string(), key: key.to_string() }
    }

    async fn send<T: serde::de::DeserializeOwned>(&self, req: reqwest::RequestBuilder) -> Result<T, NodeError> {
        let resp = req.bearer_auth(&self.key).send().await.map_err(|e| NodeError::Unreachable(describe(&e)))?;
        let status = resp.status();
        if !status.is_success() {
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            let message = body["error"].as_str().map(String::from).unwrap_or_else(|| format!("HTTP {status}"));
            return Err(NodeError::Rejected { status: status.as_u16(), message });
        }
        resp.json::<T>().await.map_err(|e| NodeError::Unreachable(format!("réponse inattendue ({e}) : est-ce bien un serveur Breezone ?")))
    }

    pub async fn hello(&self, me: &Identity) -> Result<FedSnapshot, NodeError> {
        let hello = Hello { node_id: me.node_id.clone(), name: me.name.clone() };
        self.send(self.http.post(format!("{}/api/federation/hello", self.url)).json(&hello)).await
    }

    pub async fn snapshot(&self) -> Result<FedSnapshot, NodeError> {
        self.send(self.http.get(format!("{}/api/federation/installations", self.url))).await
    }

    pub async fn control(&self, remote_id: i64, system_id: i64, zone_id: i64, cmd: &ZoneCommand, actor: &str) -> Result<FedInstallation, NodeError> {
        let url = format!("{}/api/federation/installations/{remote_id}/zones/{system_id}/{zone_id}", self.url);
        let actor: String = actor.chars().filter(|c| !c.is_control()).take(80).collect();
        self.send(self.http.patch(url).header(ACTOR_HEADER, actor).json(cmd)).await
    }
}

fn describe(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "délai dépassé".into()
    } else if e.is_connect() {
        "connexion impossible".into()
    } else {
        e.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_round_trip() {
        let inv = Invitation { v: 1, node_id: "abc".into(), name: "Bureau".into(), url: "https://fz.example.org".into(), key: "bzk_secret".into() };
        let token = encode_token(&inv);
        assert!(token.starts_with("bz1."));
        // Pasted with line breaks.
        let wrapped = format!("{}\n{}", &token[..10], &token[10..]);
        assert_eq!(parse_token(&wrapped).unwrap(), inv);
        assert!(parse_token("bz1.garbage").is_err());
        assert!(parse_token("hello").is_err());
    }

    #[test]
    fn http_only_to_private_addresses() {
        assert!(normalize_url("http://192.168.1.20:8080/").is_ok());
        assert!(normalize_url("http://nas.local:8080").is_ok());
        assert!(normalize_url("http://nas:8080").is_ok());
        assert!(normalize_url("http://[fd00::1]:8080").is_ok());
        assert!(normalize_url("https://breezone.example.org").is_ok());
        assert!(normalize_url("http://breezone.example.org").is_err());
        assert!(normalize_url("http://8.8.8.8").is_err());
        assert!(normalize_url("ftp://nas").is_err());
        assert_eq!(normalize_url("https://x.example.org/").unwrap(), "https://x.example.org");
    }

    #[test]
    fn errors_map_to_states() {
        assert_eq!(NodeError::Rejected { status: 401, message: "share expired".into() }.state(), "expired");
        assert_eq!(NodeError::Rejected { status: 401, message: "share revoked".into() }.state(), "revoked");
        assert_eq!(NodeError::Unreachable("x".into()).state(), "offline");
    }
}
