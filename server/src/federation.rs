//! Federation between Breezone nodes (ADR 0008).
//!
//! A node only reaches the Airzone WebServers of its own network. To drive several sites from
//! one instance, a node **exposes** some of its local installations to another node through a
//! *share* (limited to these installations, read-only or with control), and **consumes** the
//! installations other nodes expose to it.
//!
//! Pairing (simple for the user, no secret ever sent twice):
//!
//! 1. The exposing node creates a share and shows an invitation token (and QR code)
//!    `bz2.<base64url(JSON {v, node_id, name, url, code})>`. The `code` is **single-use** and
//!    valid **15 minutes**; only its SHA-256 is stored.
//! 2. The consuming node is given the token. It has its own **Ed25519 key pair** (the private
//!    key never leaves it) and calls `POST /api/federation/pair` with the code and its public key,
//!    signed with that key. The code is then burnt.
//! 3. The exposing node shows the request ("Maison asks for access, from 82.x.x.x") and an
//!    administrator **accepts** or refuses it.
//! 4. Every later call is **signed** (method, path, timestamp, nonce, body hash): no bearer
//!    secret travels; a captured request cannot be replayed (5-minute window, nonces remembered).
//!
//! The exposing node stays the authority for its installations; the consuming node keeps a local
//! copy of the history, like for its own installations. Only servers talk to each other.

use crate::airzone::ZoneData;
use crate::auth::{hash_token, random_bytes};
use crate::db::{self, Db};
use crate::error::ApiError;
use crate::i18n::Text;
use crate::hub::ZoneCommand;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

const TOKEN_PREFIX: &str = "bz2.";
const LEGACY_TOKEN_PREFIX: &str = "bz1.";
/// An invitation code can be used once, within this delay.
pub const PAIRING_TTL_SECS: i64 = 15 * 60;
/// Signed requests older (or newer) than this are refused.
pub const SIGNATURE_WINDOW_SECS: i64 = 5 * 60;
const SETTING_NODE_ID: &str = "node.id";
const SETTING_SIGNING_KEY: &str = "node.signing_key";
pub const HEADER_NODE: &str = "x-breezone-node";
pub const HEADER_TIMESTAMP: &str = "x-breezone-timestamp";
pub const HEADER_NONCE: &str = "x-breezone-nonce";
pub const HEADER_SIGNATURE: &str = "x-breezone-signature";
const SETTING_NODE_NAME: &str = "node.name";
const SETTING_PUBLIC_URL: &str = "node.public_url";
/// Header carrying the user who sent a command through a node (for the activity feed).
pub const ACTOR_HEADER: &str = "x-breezone-actor";
/// Longest a node holds a "tell me when it changes" read open (long polling) before answering
/// "nothing new" (304, no body). Below the 60 s read timeout of common reverse proxies (Synology).
pub const WATCH_SECS: u64 = 45;

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

/// The Ed25519 key of this node, created at first use and stored encrypted with the server
/// secret. Its public half is what other nodes use to check this node's requests.
pub async fn signing_key(db: &Db, secret: &[u8; 32]) -> sqlx::Result<SigningKey> {
    if let Some(enc) = db::get_setting(db, SETTING_SIGNING_KEY).await? {
        let bytes = crate::cloud::decrypt(secret, &enc).and_then(|b64| URL_SAFE_NO_PAD.decode(b64).ok());
        if let Some(bytes) = bytes.and_then(|b| <[u8; 32]>::try_from(b).ok()) {
            return Ok(SigningKey::from_bytes(&bytes));
        }
        tracing::warn!("node key unreadable (server secret changed?): a new one is created, nodes must pair again");
    }
    let key = SigningKey::from_bytes(&random_bytes::<32>());
    let enc = crate::cloud::encrypt(secret, &URL_SAFE_NO_PAD.encode(key.to_bytes()));
    db::set_setting(db, SETTING_SIGNING_KEY, &enc).await?;
    Ok(key)
}

pub fn public_key_text(key: &SigningKey) -> String {
    URL_SAFE_NO_PAD.encode(key.verifying_key().to_bytes())
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
    /// Single-use pairing code (valid 15 minutes).
    pub code: String,
    /// The inviting node asks the invited one to share its installations too.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reciprocal: bool,
}

pub fn encode_token(inv: &Invitation) -> String {
    let json = serde_json::to_vec(inv).expect("serializable invitation");
    format!("{TOKEN_PREFIX}{}", URL_SAFE_NO_PAD.encode(json))
}

pub fn parse_token(token: &str) -> Result<Invitation, ApiError> {
    let invalid = || ApiError::bad_request("jeton d'invitation invalide : copiez-le en entier depuis Réglages › Partages du serveur qui partage");
    let compact: String = token.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.starts_with(LEGACY_TOKEN_PREFIX) {
        return Err(ApiError::bad_request("ancien format de jeton : créez un nouveau partage sur l'autre serveur (mis à jour)"));
    }
    let body = compact.strip_prefix(TOKEN_PREFIX).ok_or_else(invalid)?;
    let raw = URL_SAFE_NO_PAD.decode(body.trim_end_matches('=')).map_err(|_| invalid())?;
    let inv: Invitation = serde_json::from_slice(&raw).map_err(|_| invalid())?;
    if inv.v != 2 {
        return Err(ApiError::bad_request("version de jeton non prise en charge : mettez Breezone à jour"));
    }
    if inv.code.len() < 20 || inv.node_id.is_empty() {
        return Err(invalid());
    }
    Ok(Invitation { url: normalize_url(&inv.url)?, ..inv })
}

/// A new single-use pairing code and its hash (the only thing stored by the exposing node).
pub fn new_pairing_code() -> (String, String) {
    let code = URL_SAFE_NO_PAD.encode(random_bytes::<24>());
    let hash = hash_token(&code);
    (code, hash)
}

// ---------------------------------------------------------------------------------------
// Signed requests between nodes
// ---------------------------------------------------------------------------------------

/// What is signed: method, path with query, timestamp, nonce and the SHA-256 of the body.
pub fn signing_message(method: &str, path: &str, timestamp: i64, nonce: &str, body: &[u8]) -> Vec<u8> {
    format!("{}\n{path}\n{timestamp}\n{nonce}\n{}", method.to_ascii_uppercase(), hex::encode(Sha256::digest(body))).into_bytes()
}

/// Headers to add to a request from node `node_id`.
pub fn signed_headers(key: &SigningKey, node_id: &str, method: &str, path: &str, body: &[u8]) -> Vec<(&'static str, String)> {
    let timestamp = db::now();
    let nonce = URL_SAFE_NO_PAD.encode(random_bytes::<16>());
    let signature = key.sign(&signing_message(method, path, timestamp, &nonce, body));
    vec![
        (HEADER_NODE, node_id.to_string()),
        (HEADER_TIMESTAMP, timestamp.to_string()),
        (HEADER_NONCE, nonce),
        (HEADER_SIGNATURE, URL_SAFE_NO_PAD.encode(signature.to_bytes())),
    ]
}

/// Checks a signature made with the public key `public_key` (base64url).
pub fn verify_signature(public_key: &str, message: &[u8], signature: &str) -> bool {
    let Some(key) = URL_SAFE_NO_PAD.decode(public_key).ok().and_then(|b| <[u8; 32]>::try_from(b).ok()) else { return false };
    let Ok(key) = VerifyingKey::from_bytes(&key) else { return false };
    let Some(sig) = URL_SAFE_NO_PAD.decode(signature).ok().and_then(|b| Signature::from_slice(&b).ok()) else { return false };
    key.verify_strict(message, &sig).is_ok()
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
    /// Fingerprint of the content, to wait for the next change (`?since=`). Absent from nodes up
    /// to 0.5.x, which do not support waiting: they are read periodically instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// `caps_rev` of the exposing node (ADR 0012): when it changes (update, rollback, module), the
    /// reading node asks for its capabilities again. Absent from nodes up to 0.10.0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caps_rev: Option<String>,
}

impl FedSnapshot {
    /// Fingerprint of what matters to the reading node: the "last seen" times change at every
    /// read of the WebServers and are left out, so only real changes give a new revision.
    pub fn compute_revision(&self) -> String {
        let mut stable = self.clone();
        stable.revision = None;
        for inst in &mut stable.installations {
            inst.connection.last_seen = None;
        }
        let json = serde_json::to_vec(&stable).expect("serializable snapshot");
        hex::encode(&Sha256::digest(&json)[..12])
    }
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
    /// The owning node records the demand time of the zones: the consuming node does too.
    #[serde(default)]
    pub record_demand: bool,
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

/// Pairing request of a consuming node (`POST /api/federation/pair`), signed with `public_key`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairRequest {
    pub code: String,
    pub node_id: String,
    pub name: String,
    pub public_key: String,
    /// Reciprocal invitation accepted: address at which the inviting node reaches this one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shares_back_from: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairResponse {
    /// "pending": an administrator of the exposing node must accept the request.
    pub status: String,
    pub node_id: String,
    pub name: String,
    /// Public key of the exposing node (to bind a reciprocal share to it).
    #[serde(default)]
    pub public_key: Option<String>,
}

// ---------------------------------------------------------------------------------------
// Client (consuming side)
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, thiserror::Error)]
pub enum NodeError {
    /// The node answered with an error (401: key revoked or expired, 403, 400…).
    #[error("{}", message.current())]
    Rejected { status: u16, message: Text },
    #[error("{}", self.text().current())]
    Unreachable(Text),
    /// The node's HTTPS certificate is refused (it can be reviewed and trusted).
    #[error("{}", _0.current())]
    Certificate(Text),
}

impl NodeError {
    /// What went wrong, for people.
    pub fn text(&self) -> Text {
        match self {
            NodeError::Rejected { message, .. } => message.clone(),
            NodeError::Unreachable(reason) => crate::text!("injoignable : {0}", "unreachable: {0}", "inaccesible: {0}", reason.arg()),
            NodeError::Certificate(reason) => reason.clone(),
        }
    }

    /// Connection state shown for the node.
    pub fn state(&self) -> &'static str {
        match self {
            NodeError::Rejected { status: 401, message } if message.en.contains("expired") => "expired",
            NodeError::Rejected { status: 401, .. } => "revoked",
            NodeError::Rejected { status: 403, message } if message.en.contains("pending") => "pending",
            NodeError::Rejected { .. } => "error",
            NodeError::Unreachable(_) => "offline",
            NodeError::Certificate(_) => "certificate",
        }
    }
}

impl From<NodeError> for ApiError {
    fn from(e: NodeError) -> Self {
        match e {
            NodeError::Rejected { status: 400, message } => ApiError::bad_request(message),
            NodeError::Rejected { status: 401, message } => ApiError::bad_gateway(crate::text!(
                "le nœud a refusé l'accès ({0})",
                "the node refused access ({0})",
                "el nodo rechazó el acceso ({0})",
                message.arg()
            )),
            NodeError::Rejected { status: 403, message } => ApiError::forbidden(message),
            NodeError::Rejected { status: 404, message } => ApiError::not_found(message),
            // A command the other node does not know (ADR 0012): said as such, with its words.
            NodeError::Rejected { status: 422, message } => ApiError::new(axum::http::StatusCode::UNPROCESSABLE_ENTITY, message).with_detail("code", "unsupported"),
            other => ApiError::bad_gateway(other.text()),
        }
    }
}

#[derive(Clone)]
pub struct NodeClient {
    http: reqwest::Client,
    url: String,
    /// Id of this (consuming) node and its signing key.
    node_id: String,
    key: SigningKey,
}

impl std::fmt::Debug for NodeClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the key.
        f.debug_struct("NodeClient").field("url", &self.url).finish()
    }
}

impl NodeClient {
    /// `pinned`: certificate trusted by an administrator (only that one is then accepted).
    pub fn new(url: &str, node_id: &str, key: SigningKey, pinned: Option<&str>) -> Self {
        let http = crate::tls::node_http_client(pinned);
        Self { http, url: url.trim_end_matches('/').to_string(), node_id: node_id.to_string(), key }
    }

    /// Sends a signed request (path relative to the node URL, e.g. `/api/federation/installations`).
    async fn send<T: serde::de::DeserializeOwned>(&self, method: reqwest::Method, path: &str, body: Option<Vec<u8>>, actor: Option<&str>) -> Result<T, NodeError> {
        self.send_with(method, path, body, actor, None).await
    }

    async fn send_with<T: serde::de::DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Vec<u8>>,
        actor: Option<&str>,
        timeout: Option<std::time::Duration>,
    ) -> Result<T, NodeError> {
        let resp = self.request(method, path, body, actor, timeout).await?;
        Self::decode(resp).await
    }

    /// Sends a signed request; network and certificate failures become [`NodeError`]s.
    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Vec<u8>>,
        actor: Option<&str>,
        timeout: Option<std::time::Duration>,
    ) -> Result<reqwest::Response, NodeError> {
        let body = body.unwrap_or_default();
        let mut req = self.http.request(method.clone(), format!("{}{path}", self.url));
        // Its errors in the language of the person waiting for them (English in the background).
        req = req.header(reqwest::header::ACCEPT_LANGUAGE, crate::i18n::current().code());
        if let Some(timeout) = timeout {
            req = req.timeout(timeout);
        }
        for (name, value) in signed_headers(&self.key, &self.node_id, method.as_str(), path, &body) {
            req = req.header(name, value);
        }
        if let Some(actor) = actor {
            req = req.header(ACTOR_HEADER, actor.chars().filter(|c| !c.is_control()).take(80).collect::<String>());
        }
        if !body.is_empty() {
            req = req.header(reqwest::header::CONTENT_TYPE, "application/json").body(body);
        }
        req.send().await.map_err(|e| {
            let (text, certificate) = describe(&e);
            if certificate {
                NodeError::Certificate(text)
            } else {
                NodeError::Unreachable(text)
            }
        })
    }

    async fn decode<T: serde::de::DeserializeOwned>(resp: reqwest::Response) -> Result<T, NodeError> {
        let status = resp.status();
        if !status.is_success() {
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            // The route does not exist there (the router's own 404): an older node.
            if status == reqwest::StatusCode::NOT_FOUND && body["error"] == "not found" {
                return Err(NodeError::Rejected {
                    status: 404,
                    message: crate::text!(
                        "fonction absente sur ce serveur : mettez-le à jour",
                        "feature missing on this server: update it",
                        "función ausente en este servidor: actualícelo"
                    ),
                });
            }
            let message = body["error"].as_str().map(String::from).unwrap_or_else(|| format!("HTTP {status}"));
            return Err(NodeError::Rejected { status: status.as_u16(), message: Text::plain(message) });
        }
        resp.json::<T>().await.map_err(|e| {
            NodeError::Unreachable(crate::text!(
                "réponse inattendue ({0}) : est-ce bien un serveur Breezone ?",
                "unexpected answer ({0}): is it a Breezone server?",
                "respuesta inesperada ({0}): ¿es realmente un servidor Breezone?",
                e
            ))
        })
    }

    /// Uses the invitation code: the exposing node records this node's public key.
    pub async fn pair(&self, code: &str, name: &str, shares_back_from: Option<&str>) -> Result<PairResponse, NodeError> {
        let req = PairRequest {
            code: code.to_string(),
            node_id: self.node_id.clone(),
            name: name.to_string(),
            public_key: public_key_text(&self.key),
            shares_back_from: shares_back_from.map(str::to_string),
        };
        let body = serde_json::to_vec(&req).expect("serializable request");
        self.send(reqwest::Method::POST, "/api/federation/pair", Some(body), None).await
    }

    /// Exchanges capabilities with the node (ADR 0012): sends this node's, receives its own.
    /// `None`: the node predates the exchange (up to 0.10.0): it gets the legacy profile.
    pub async fn hello(&self, mine: &crate::capabilities::Capabilities) -> Result<Option<crate::capabilities::Capabilities>, NodeError> {
        let body = serde_json::to_vec(mine).expect("serializable capabilities");
        let resp = self.request(reqwest::Method::POST, "/api/federation/hello", Some(body), None, None).await?;
        let status = resp.status();
        // Unknown route (404, or 405 on some proxies): an older node.
        if status == reqwest::StatusCode::NOT_FOUND || status == reqwest::StatusCode::METHOD_NOT_ALLOWED {
            return Ok(None);
        }
        if !status.is_success() {
            return Self::decode::<crate::capabilities::Capabilities>(resp).await.map(Some);
        }
        // Very old nodes answer their web page to unknown addresses: legacy as well.
        Ok(resp.json::<crate::capabilities::Capabilities>().await.ok().map(crate::capabilities::Capabilities::sanitized))
    }

    pub async fn snapshot(&self) -> Result<FedSnapshot, NodeError> {
        self.send(reqwest::Method::GET, "/api/federation/installations", None, None).await
    }

    /// Like [`Self::snapshot`], but the node answers only when its content differs from `since`,
    /// or `None` ("nothing new", a few bytes) after `wait_secs` (at most [`WATCH_SECS`]). Without
    /// `since`, it answers
    /// at once.
    pub async fn watch(&self, since: Option<&str>, wait_secs: u64) -> Result<Option<FedSnapshot>, NodeError> {
        let wait = wait_secs.clamp(1, WATCH_SECS);
        let path = match since {
            Some(since) => format!("/api/federation/installations?wait={wait}&since={since}"),
            None => "/api/federation/installations".to_string(),
        };
        let timeout = std::time::Duration::from_secs(wait + 15);
        let resp = self.request(reqwest::Method::GET, &path, None, None, Some(timeout)).await?;
        if resp.status() == reqwest::StatusCode::NOT_MODIFIED {
            return Ok(None);
        }
        Self::decode(resp).await.map(Some)
    }

    pub async fn control(&self, remote_id: i64, system_id: i64, zone_id: i64, cmd: &ZoneCommand, actor: &str) -> Result<FedInstallation, NodeError> {
        let path = format!("/api/federation/installations/{remote_id}/zones/{system_id}/{zone_id}");
        let body = serde_json::to_vec(cmd).expect("serializable command");
        self.send(reqwest::Method::PATCH, &path, Some(body), Some(actor)).await
    }

    /// Asks the node to change the Airzone schedules of one of its installations.
    pub async fn schedule(&self, remote_id: i64, op: &crate::airzone_schedules::ScheduleOp, actor: &str) -> Result<serde_json::Value, NodeError> {
        let path = format!("/api/federation/installations/{remote_id}/schedules");
        let body = serde_json::to_vec(op).expect("serializable operation");
        self.send(reqwest::Method::POST, &path, Some(body), Some(actor)).await
    }
}

/// What failed, for people, and whether it is the HTTPS certificate.
fn describe(e: &reqwest::Error) -> (Text, bool) {
    // The useful part (TLS, DNS, refused) is deep in the source chain.
    let mut chain = String::new();
    let mut source: Option<&dyn std::error::Error> = Some(e);
    while let Some(err) = source {
        chain.push_str(&err.to_string().to_lowercase());
        chain.push(' ');
        source = err.source();
    }
    if e.is_timeout() {
        (crate::text!("délai dépassé", "timed out", "tiempo de espera agotado"), false)
    } else if chain.contains(crate::tls::PIN_MISMATCH) {
        (
            crate::text!(
                "certificat HTTPS différent de celui que vous aviez accepté",
                "HTTPS certificate different from the one you had accepted",
                "certificado HTTPS distinto del que había aceptado"
            ),
            true,
        )
    } else if chain.contains("certificate") {
        let reason = if chain.contains("not valid for name") || chain.contains("notvalidforname") {
            crate::text!(
                "certificat HTTPS refusé : il ne couvre pas ce nom de domaine",
                "HTTPS certificate refused: it does not cover this domain name",
                "certificado HTTPS rechazado: no cubre este nombre de dominio"
            )
        } else if chain.contains("expired") {
            crate::text!("certificat HTTPS expiré", "HTTPS certificate expired", "certificado HTTPS caducado")
        } else {
            crate::text!(
                "certificat HTTPS refusé (non reconnu ou auto-signé)",
                "HTTPS certificate refused (unknown or self-signed)",
                "certificado HTTPS rechazado (desconocido o autofirmado)"
            )
        };
        (reason, true)
    } else if chain.contains("dns error") || chain.contains("failed to lookup") {
        (crate::text!("nom de domaine introuvable", "domain name not found", "nombre de dominio no encontrado"), false)
    } else if chain.contains("refused") {
        (
            crate::text!(
                "connexion refusée (port fermé ou serveur arrêté)",
                "connection refused (port closed or server stopped)",
                "conexión rechazada (puerto cerrado o servidor detenido)"
            ),
            false,
        )
    } else if e.is_connect() {
        (crate::text!("connexion impossible", "cannot connect", "conexión imposible"), false)
    } else {
        (Text::plain(e.to_string()), false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_round_trip() {
        let inv = Invitation { v: 2, node_id: "abc".into(), name: "Bureau".into(), url: "https://fz.example.org".into(), code: new_pairing_code().0, reciprocal: false };
        let token = encode_token(&inv);
        assert!(token.starts_with("bz2."));
        // Pasted with line breaks.
        let wrapped = format!("{}\n{}", &token[..10], &token[10..]);
        assert_eq!(parse_token(&wrapped).unwrap(), inv);
        assert!(parse_token("bz2.garbage").is_err());
        assert!(parse_token("bz1.eyJ2IjoxfQ").unwrap_err().message.fr.contains("ancien format"));
        assert!(parse_token("hello").is_err());
    }

    #[test]
    fn http_only_to_private_addresses() {
        assert!(normalize_url("http://192.168.1.20:8427/").is_ok());
        assert!(normalize_url("http://nas.local:8427").is_ok());
        assert!(normalize_url("http://nas:8427").is_ok());
        assert!(normalize_url("http://[fd00::1]:8427").is_ok());
        assert!(normalize_url("https://breezone.example.org").is_ok());
        assert!(normalize_url("http://breezone.example.org").is_err());
        assert!(normalize_url("http://8.8.8.8").is_err());
        assert!(normalize_url("ftp://nas").is_err());
        assert_eq!(normalize_url("https://x.example.org/").unwrap(), "https://x.example.org");
    }

    #[test]
    fn signatures_cover_method_path_time_nonce_and_body() {
        let key = SigningKey::from_bytes(&[7u8; 32]);
        let public = public_key_text(&key);
        let headers = signed_headers(&key, "home", "PATCH", "/api/x", b"{\"on\":true}");
        let get = |n: &str| headers.iter().find(|(h, _)| *h == n).unwrap().1.clone();
        let ts: i64 = get(HEADER_TIMESTAMP).parse().unwrap();
        let (nonce, sig) = (get(HEADER_NONCE), get(HEADER_SIGNATURE));
        assert!(verify_signature(&public, &signing_message("PATCH", "/api/x", ts, &nonce, b"{\"on\":true}"), &sig));
        assert!(!verify_signature(&public, &signing_message("PATCH", "/api/x", ts, &nonce, b"{\"on\":false}"), &sig), "body");
        assert!(!verify_signature(&public, &signing_message("PATCH", "/api/y", ts, &nonce, b"{\"on\":true}"), &sig), "path");
        assert!(!verify_signature(&public, &signing_message("PATCH", "/api/x", ts + 1, &nonce, b"{\"on\":true}"), &sig), "time");
        let other = public_key_text(&SigningKey::from_bytes(&[8u8; 32]));
        assert!(!verify_signature(&other, &signing_message("PATCH", "/api/x", ts, &nonce, b"{\"on\":true}"), &sig), "key");
    }

    #[test]
    fn errors_map_to_states() {
        assert_eq!(NodeError::Rejected { status: 403, message: Text::plain("pairing pending approval") }.state(), "pending");
        assert_eq!(NodeError::Rejected { status: 401, message: Text::plain("share expired") }.state(), "expired");
        assert_eq!(NodeError::Rejected { status: 401, message: Text::plain("share revoked") }.state(), "revoked");
        assert_eq!(NodeError::Unreachable(Text::plain("x")).state(), "offline");
    }
}
