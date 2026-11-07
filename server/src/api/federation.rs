//! Federation between Breezone nodes (ADR 0008).
//!
//! * Administration (user JWT): identity of this node, shares it exposes (and the access requests
//!   to accept), nodes it consumes.
//! * Node to node (requests signed with the node's Ed25519 key): `/api/federation/*`, limited to
//!   the installations of the share. `POST /api/federation/pair` uses a single-use invitation code.

use crate::auth::{hash_token, AdminUser, AuthUser};
use crate::db::{self, Share};
use crate::error::{ApiError, ApiResult};
use crate::federation::{self, FedInstallation, FedSnapshot, Identity, Invitation, NodeClient, PairRequest, PairResponse, ACTOR_HEADER};
use crate::hub::{NodeStatus, ZoneCommand};
use crate::AppState;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, FromRequest, Path, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;

// ---------------------------------------------------------------------------------------
// Identity of this node
// ---------------------------------------------------------------------------------------

pub async fn get_identity(State(state): State<AppState>, _user: AuthUser) -> ApiResult<Json<Identity>> {
    Ok(Json(federation::identity(&state.db).await?))
}

#[derive(Deserialize)]
pub struct IdentityInput {
    pub name: String,
    #[serde(default)]
    pub public_url: Option<String>,
}

pub async fn save_identity(State(state): State<AppState>, _admin: AdminUser, Json(req): Json<IdentityInput>) -> ApiResult<Json<Identity>> {
    Ok(Json(federation::save_identity(&state.db, &req.name, req.public_url.as_deref()).await?))
}

// ---------------------------------------------------------------------------------------
// Shares exposed by this node
// ---------------------------------------------------------------------------------------

#[derive(Serialize)]
pub struct ShareView {
    pub id: i64,
    pub label: String,
    /// "read" | "control"
    pub permission: String,
    pub installations: Vec<ShareInstallation>,
    pub expires_at: Option<i64>,
    pub expired: bool,
    pub created_at: i64,
    pub created_by: Option<String>,
    /// Node using the share (known after its first call).
    pub remote_name: Option<String>,
    pub last_seen: Option<i64>,
    pub last_address: Option<String>,
    /// The node called recently.
    pub connected: bool,
    /// "invited" (code not used yet) | "pending" (a node asks for access) | "active"
    pub status: String,
    /// End of validity of the invitation code (status "invited").
    pub pairing_expires_at: Option<i64>,
    /// The invitation code can no longer be used.
    pub pairing_expired: bool,
    /// When the node asked for access (status "pending").
    pub requested_at: Option<i64>,
}

#[derive(Serialize)]
pub struct ShareInstallation {
    pub id: i64,
    pub name: String,
}

async fn share_view(state: &AppState, s: Share) -> ApiResult<ShareView> {
    let ids = db::share_installation_ids(&state.db, s.id).await?;
    let mut installations = vec![];
    for id in ids {
        if let Some(i) = db::installation_by_id(&state.db, id).await? {
            installations.push(ShareInstallation { id: i.id, name: i.name });
        }
    }
    let now = db::now();
    // A consuming node polls at its own poll interval (10 s by default).
    let window = (state.cfg.poll_interval_secs as i64 * 3).max(90);
    Ok(ShareView {
        id: s.id,
        expired: s.expires_at.is_some_and(|e| e <= now),
        connected: s.last_seen.is_some_and(|t| now - t <= window),
        label: s.label,
        permission: s.permission,
        installations,
        expires_at: s.expires_at,
        created_at: s.created_at,
        created_by: s.created_by,
        remote_name: s.remote_name,
        last_seen: s.last_seen,
        last_address: s.last_address,
        pairing_expired: s.status == "invited" && s.pairing_expires_at.is_some_and(|e| e <= now),
        status: s.status,
        pairing_expires_at: s.pairing_expires_at,
        requested_at: s.requested_at,
    })
}

pub async fn list_shares(State(state): State<AppState>, _admin: AdminUser) -> ApiResult<Json<Vec<ShareView>>> {
    let mut out = vec![];
    for s in db::list_shares(&state.db).await? {
        out.push(share_view(&state, s).await?);
    }
    Ok(Json(out))
}

#[derive(Deserialize)]
pub struct ShareInput {
    pub label: String,
    pub installation_ids: Vec<i64>,
    #[serde(default = "read")]
    pub permission: String,
    /// Validity in days (none = until revoked).
    #[serde(default)]
    pub expires_in_days: Option<i64>,
    /// Address the other node will use (default: the public address of this node).
    #[serde(default)]
    pub url: Option<String>,
}

fn read() -> String {
    "read".into()
}

async fn validate_share(state: &AppState, req: &ShareInput) -> ApiResult<(String, Option<i64>)> {
    let label = req.label.trim().to_string();
    if label.is_empty() || label.chars().count() > 60 {
        return Err(ApiError::bad_request("le libellé doit faire de 1 à 60 caractères"));
    }
    if !["read", "control"].contains(&req.permission.as_str()) {
        return Err(ApiError::bad_request("droit attendu : read ou control"));
    }
    if req.installation_ids.is_empty() {
        return Err(ApiError::bad_request("choisissez au moins une installation"));
    }
    for id in &req.installation_ids {
        let inst = db::installation_by_id(&state.db, *id).await?.ok_or_else(|| ApiError::bad_request(format!("installation {id} inconnue")))?;
        if !inst.is_local() {
            // Re-sharing installations of another node would allow loops.
            return Err(ApiError::bad_request(format!("« {} » est jointe via un autre nœud : seules les installations locales sont partageables", inst.name)));
        }
    }
    let expires_at = match req.expires_in_days {
        Some(d) if !(1..=3650).contains(&d) => return Err(ApiError::bad_request("durée de validité invalide")),
        Some(d) => Some(db::now() + d * 86_400),
        None => None,
    };
    Ok((label, expires_at))
}

#[derive(Serialize)]
pub struct CreatedShare {
    pub share: ShareView,
    /// Invitation token, shown once: single-use pairing code, valid 15 minutes.
    pub token: String,
}

pub async fn create_share(State(state): State<AppState>, AdminUser(admin): AdminUser, Json(req): Json<ShareInput>) -> ApiResult<(StatusCode, Json<CreatedShare>)> {
    let (label, expires_at) = validate_share(&state, &req).await?;
    let me = federation::identity(&state.db).await?;
    let url = match req.url.as_deref().map(str::trim).filter(|u| !u.is_empty()) {
        Some(u) => federation::normalize_url(u)?,
        None => me.public_url.clone().ok_or_else(|| {
            ApiError::bad_request("indiquez l'adresse à laquelle l'autre serveur joindra celui-ci (Réglages › Partages › Adresse publique)")
        })?,
    };
    let (code, hash) = federation::new_pairing_code();
    let mut ids = req.installation_ids.clone();
    ids.sort_unstable();
    ids.dedup();
    let share = db::insert_share(&state.db, &label, &hash, &req.permission, expires_at, &admin.username, &ids).await?;
    let token = federation::encode_token(&Invitation { v: 2, node_id: me.node_id, name: me.name, url, code });
    state
        .hub
        .log("system", "info", None, Some(&admin.username), &format!("Partage « {label} » créé ({} installation(s), {})", ids.len(), permission_label(&req.permission)), None)
        .await;
    Ok((StatusCode::CREATED, Json(CreatedShare { share: share_view(&state, share).await?, token })))
}

/// Changes the installations, the permission or the expiry of a share (same key).
pub async fn update_share(State(state): State<AppState>, AdminUser(admin): AdminUser, Path(id): Path<i64>, Json(req): Json<ShareInput>) -> ApiResult<Json<ShareView>> {
    let share = db::share_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("partage introuvable"))?;
    let (label, expires_at) = validate_share(&state, &req).await?;
    let expires_at = if req.expires_in_days.is_some() { expires_at } else { share.expires_at };
    let mut ids = req.installation_ids.clone();
    ids.sort_unstable();
    ids.dedup();
    db::update_share(&state.db, id, &label, &req.permission, expires_at).await?;
    db::set_share_installations(&state.db, id, &ids).await?;
    state.hub.log("system", "info", None, Some(&admin.username), &format!("Partage « {label} » modifié"), None).await;
    let share = db::share_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("partage introuvable"))?;
    Ok(Json(share_view(&state, share).await?))
}

/// Accepts the access request of the node that used the invitation.
pub async fn approve_share(State(state): State<AppState>, AdminUser(admin): AdminUser, Path(id): Path<i64>) -> ApiResult<Json<ShareView>> {
    let share = db::share_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("partage introuvable"))?;
    if !db::approve_share(&state.db, id).await? {
        return Err(ApiError::bad_request("aucune demande d'accès en attente pour ce partage"));
    }
    let who = share.remote_name.clone().unwrap_or_else(|| "?".into());
    state.hub.log("system", "info", None, Some(&admin.username), &format!("Accès accordé au serveur « {who} » (partage « {} »)", share.label), None).await;
    let share = db::share_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("partage introuvable"))?;
    Ok(Json(share_view(&state, share).await?))
}

/// Revokes a share (or refuses a request): access stops immediately.
pub async fn delete_share(State(state): State<AppState>, AdminUser(admin): AdminUser, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    let share = db::share_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("partage introuvable"))?;
    db::delete_share(&state.db, id).await?;
    state.hub.log("system", "info", None, Some(&admin.username), &format!("Partage « {} » révoqué", share.label), None).await;
    Ok(StatusCode::NO_CONTENT)
}

fn permission_label(p: &str) -> &'static str {
    if p == "control" {
        "pilotage"
    } else {
        "lecture"
    }
}

// ---------------------------------------------------------------------------------------
// Nodes consumed by this node
// ---------------------------------------------------------------------------------------

#[derive(Serialize)]
pub struct NodeView {
    pub id: i64,
    pub name: String,
    pub url: String,
    pub created_at: i64,
    pub status: NodeStatus,
    pub installations: Vec<ShareInstallation>,
}

async fn node_views(state: &AppState) -> ApiResult<Vec<NodeView>> {
    let statuses = state.hub.node_statuses().await;
    let installations = db::list_installations(&state.db).await?;
    Ok(db::list_nodes(&state.db)
        .await?
        .into_iter()
        .map(|n| NodeView {
            status: statuses.get(&n.id).cloned().unwrap_or(NodeStatus {
                state: "unknown",
                last_seen: n.last_seen,
                last_error: n.last_error.clone(),
                latency_ms: None,
                permission: n.permission.clone(),
            }),
            installations: installations
                .iter()
                .filter(|i| i.node_id == Some(n.id))
                .map(|i| ShareInstallation { id: i.id, name: i.name.clone() })
                .collect(),
            id: n.id,
            name: n.name,
            url: n.url,
            created_at: n.created_at,
        })
        .collect())
}

pub async fn list_nodes(State(state): State<AppState>, _user: AuthUser) -> ApiResult<Json<Vec<NodeView>>> {
    Ok(Json(node_views(&state).await?))
}

#[derive(Deserialize)]
pub struct AddNodeInput {
    /// Invitation token (`bz2.…`) from the node that shares.
    pub token: String,
    /// Address to use instead of the one in the token (e.g. its local address).
    #[serde(default)]
    pub url: Option<String>,
}

/// Adds a node from its invitation token: pairs with it (single-use code + this node's public
/// key). The node then waits until an administrator over there accepts the request.
pub async fn add_node(State(state): State<AppState>, AdminUser(admin): AdminUser, Json(req): Json<AddNodeInput>) -> ApiResult<(StatusCode, Json<NodeView>)> {
    let inv = federation::parse_token(&req.token)?;
    let url = match req.url.as_deref().map(str::trim).filter(|u| !u.is_empty()) {
        Some(u) => federation::normalize_url(u)?,
        None => inv.url.clone(),
    };
    let me = federation::identity(&state.db).await?;
    if inv.node_id == me.node_id {
        return Err(ApiError::bad_request("ce jeton a été créé par ce serveur : utilisez-le sur l'autre serveur"));
    }
    let key = federation::signing_key(&state.db, &state.cloud_key).await?;
    let client = NodeClient::new(&url, &me.node_id, key);
    let paired = client.pair(&inv.code, &me.name).await.map_err(|e| match e {
        federation::NodeError::Unreachable(m) => ApiError::bad_gateway(format!("le serveur « {} » ne répond pas à {url} ({m})", inv.name)),
        other => other.into(),
    })?;
    if paired.node_id != inv.node_id {
        return Err(ApiError::bad_request(format!("{url} n'est pas le serveur « {} » du jeton", inv.name)));
    }
    let node = db::upsert_node(&state.db, &paired.node_id, &paired.name, &url, "", "read").await?;
    state.hub.reload_installations().await?;
    state.hub.poll_node(node.id).await;
    state
        .hub
        .log(
            "system",
            "info",
            None,
            Some(&admin.username),
            &format!("Nœud « {} » ajouté : en attente d'acceptation sur ce serveur (Réglages › Partages)", paired.name),
            None,
        )
        .await;
    let view = node_views(&state).await?.into_iter().find(|n| n.id == node.id).ok_or_else(|| ApiError::internal("node not stored"))?;
    Ok((StatusCode::CREATED, Json(view)))
}

#[derive(Deserialize)]
pub struct NodeInput {
    pub url: String,
}

/// Changes the address used to reach a node.
pub async fn update_node(State(state): State<AppState>, AdminUser(admin): AdminUser, Path(id): Path<i64>, Json(req): Json<NodeInput>) -> ApiResult<Json<NodeView>> {
    let node = db::node_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("nœud introuvable"))?;
    let url = federation::normalize_url(&req.url)?;
    db::set_node_url(&state.db, id, &url).await?;
    state.hub.reload_installations().await?;
    state.hub.poll_node(id).await;
    state.hub.log("system", "info", None, Some(&admin.username), &format!("Nœud « {} » : adresse {url}", node.name), None).await;
    node_views(&state).await?.into_iter().find(|n| n.id == id).map(Json).ok_or_else(|| ApiError::not_found("nœud introuvable"))
}

pub async fn refresh_node(State(state): State<AppState>, _user: AuthUser, Path(id): Path<i64>) -> ApiResult<Json<NodeView>> {
    state.hub.poll_node(id).await;
    node_views(&state).await?.into_iter().find(|n| n.id == id).map(Json).ok_or_else(|| ApiError::not_found("nœud introuvable"))
}

/// Removes a node and the installations reached through it.
pub async fn delete_node(State(state): State<AppState>, AdminUser(admin): AdminUser, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    let node = db::node_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("nœud introuvable"))?;
    db::delete_node(&state.db, id).await?;
    state.hub.reload_installations().await?;
    state.hub.log("system", "info", None, Some(&admin.username), &format!("Nœud « {} » retiré", node.name), None).await;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------------------
// Node to node: signed requests
// ---------------------------------------------------------------------------------------

/// Nonces already seen (with their timestamp), to refuse replayed requests.
static NONCES: std::sync::Mutex<Option<HashMap<String, i64>>> = std::sync::Mutex::new(None);

/// A request from another node: signature headers and the raw body (checked against the key of
/// the node it claims to come from, see [`ShareAuth`] and [`pair`]).
pub struct Signed {
    pub node_id: String,
    message: Vec<u8>,
    signature: String,
    pub body: Bytes,
    pub headers: HeaderMap,
    pub address: Option<String>,
}

impl Signed {
    fn verify(&self, public_key: &str) -> bool {
        federation::verify_signature(public_key, &self.message, &self.signature)
    }
}

impl FromRequest<AppState> for Signed {
    type Rejection = ApiError;

    async fn from_request(req: Request, _state: &AppState) -> Result<Self, Self::Rejection> {
        let (parts, body) = req.into_parts();
        let body = axum::body::to_bytes(body, 64 * 1024).await.map_err(|_| ApiError::bad_request("body too large"))?;
        let header = |name: &str| parts.headers.get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
        let (Some(node_id), Some(timestamp), Some(nonce), Some(signature)) = (
            header(federation::HEADER_NODE),
            header(federation::HEADER_TIMESTAMP).and_then(|t| t.parse::<i64>().ok()),
            header(federation::HEADER_NONCE),
            header(federation::HEADER_SIGNATURE),
        ) else {
            return Err(ApiError::unauthorized("signed request required"));
        };
        let now = db::now();
        if (now - timestamp).abs() > federation::SIGNATURE_WINDOW_SECS {
            return Err(ApiError::unauthorized("request too old: check the clocks of both servers"));
        }
        {
            let mut guard = NONCES.lock().unwrap();
            let seen = guard.get_or_insert_with(HashMap::new);
            seen.retain(|_, t| now - *t <= 2 * federation::SIGNATURE_WINDOW_SECS);
            if seen.insert(format!("{node_id}:{nonce}"), timestamp).is_some() {
                return Err(ApiError::unauthorized("replayed request"));
            }
        }
        let path = parts.uri.path_and_query().map(|p| p.as_str()).unwrap_or_else(|| parts.uri.path()).to_string();
        let message = federation::signing_message(parts.method.as_str(), &path, timestamp, &nonce, &body);
        let connect = parts.extensions.get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip().to_string());
        let address = client_address(&parts.headers).or(connect);
        Ok(Signed { node_id, message, signature, body, headers: parts.headers, address })
    }
}

/// A signed call from a paired node whose access was accepted.
pub struct ShareAuth {
    pub share: Share,
    pub installation_ids: Vec<i64>,
    pub address: Option<String>,
    pub body: Bytes,
    pub headers: HeaderMap,
}

impl FromRequest<AppState> for ShareAuth {
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &AppState) -> Result<Self, Self::Rejection> {
        let signed = Signed::from_request(req, state).await?;
        let share = db::share_of_node(&state.db, &signed.node_id).await?.ok_or_else(|| ApiError::unauthorized("share revoked"))?;
        if !share.remote_public_key.as_deref().is_some_and(|k| signed.verify(k)) {
            return Err(ApiError::unauthorized("invalid signature"));
        }
        if share.expires_at.is_some_and(|e| e <= db::now()) {
            return Err(ApiError::unauthorized("share expired"));
        }
        if share.status != "active" {
            return Err(ApiError::forbidden("pairing pending approval"));
        }
        let installation_ids = db::share_installation_ids(&state.db, share.id).await?;
        Ok(ShareAuth { share, installation_ids, address: signed.address, body: signed.body, headers: signed.headers })
    }
}

/// Address of the calling node, as seen behind Cloudflare Tunnel or a reverse proxy.
fn client_address(headers: &HeaderMap) -> Option<String> {
    let get = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).map(str::trim).filter(|v| !v.is_empty());
    get("cf-connecting-ip")
        .or_else(|| get("x-forwarded-for").and_then(|v| v.split(',').next()).map(str::trim))
        .or_else(|| get("x-real-ip"))
        .map(|v| v.chars().take(64).collect())
}

async fn fed_snapshot(state: &AppState, auth: &ShareAuth) -> ApiResult<FedSnapshot> {
    let me = federation::identity(&state.db).await?;
    Ok(FedSnapshot {
        node_id: me.node_id,
        name: me.name,
        permission: auth.share.permission.clone(),
        installations: state.hub.federated(&auth.installation_ids).await,
    })
}

/// A node uses an invitation code: it proves it holds the key it presents (the request is signed
/// with it), the code is burnt, and an administrator of this node must accept the request.
pub async fn pair(State(state): State<AppState>, signed: Signed) -> ApiResult<Json<PairResponse>> {
    let req: PairRequest = serde_json::from_slice(&signed.body).map_err(|_| ApiError::bad_request("invalid pairing request"))?;
    if req.node_id != signed.node_id || !signed.verify(&req.public_key) {
        return Err(ApiError::unauthorized("invalid signature"));
    }
    let me = federation::identity(&state.db).await?;
    if req.node_id == me.node_id {
        return Err(ApiError::bad_request("ce jeton a été créé par ce serveur : utilisez-le sur l'autre serveur"));
    }
    let share = db::invited_share(&state.db, &hash_token(req.code.trim()))
        .await?
        .ok_or_else(|| ApiError::bad_request("invitation inconnue ou déjà utilisée : créez un nouveau partage"))?;
    if share.pairing_expires_at.is_some_and(|e| e <= db::now()) {
        return Err(ApiError::bad_request("invitation expirée (15 minutes) : créez un nouveau partage"));
    }
    let name: String = req.name.trim().chars().take(60).collect();
    let node_id: String = req.node_id.trim().chars().take(64).collect();
    if !db::claim_share(&state.db, share.id, &node_id, &name, &req.public_key, signed.address.as_deref()).await? {
        return Err(ApiError::bad_request("invitation déjà utilisée : créez un nouveau partage"));
    }
    let from = signed.address.as_deref().map(|a| format!(" depuis {a}")).unwrap_or_default();
    state
        .hub
        .log(
            "system",
            "warn",
            None,
            None,
            &format!("Le serveur « {name} » demande l'accès au partage « {} »{from} : acceptez ou refusez dans Réglages › Partages", share.label),
            None,
        )
        .await;
    Ok(Json(PairResponse { status: "pending".into(), node_id: me.node_id, name: me.name }))
}

pub async fn installations(State(state): State<AppState>, auth: ShareAuth) -> ApiResult<Json<FedSnapshot>> {
    db::touch_share(&state.db, auth.share.id, None, None, auth.address.as_deref()).await?;
    Ok(Json(fed_snapshot(&state, &auth).await?))
}

pub async fn control(State(state): State<AppState>, Path((id, system_id, zone_id)): Path<(i64, i64, i64)>, auth: ShareAuth) -> ApiResult<Json<FedInstallation>> {
    let cmd: ZoneCommand = serde_json::from_slice(&auth.body).map_err(|e| ApiError::bad_request(format!("invalid command: {e}")))?;
    if !auth.installation_ids.contains(&id) {
        return Err(ApiError::not_found("installation non partagée"));
    }
    if auth.share.permission != "control" {
        return Err(ApiError::forbidden("ce partage est en lecture seule"));
    }
    db::touch_share(&state.db, auth.share.id, None, None, auth.address.as_deref()).await?;
    let node = auth.share.remote_name.clone().unwrap_or_else(|| auth.share.label.clone());
    let user: String = auth
        .headers
        .get(ACTOR_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.chars().filter(|c| !c.is_control()).take(60).collect())
        .unwrap_or_default();
    let actor = if user.is_empty() { format!("nœud {node}") } else { format!("{user} (nœud {node})") };
    state.hub.control(id, system_id, zone_id, &cmd, &actor).await?;
    state.hub.federated(&[id]).await.into_iter().next().map(Json).ok_or_else(|| ApiError::not_found("installation non partagée"))
}
