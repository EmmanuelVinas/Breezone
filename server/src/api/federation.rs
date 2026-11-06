//! Federation between Breezone nodes (ADR 0008).
//!
//! * Administration (user JWT): identity of this node, shares it exposes, nodes it consumes.
//! * Node to node (share key): `/api/federation/*`, limited to the installations of the share.

use crate::auth::{hash_token, AdminUser, AuthUser};
use crate::cloud;
use crate::db::{self, Share};
use crate::error::{ApiError, ApiResult};
use crate::federation::{self, FedInstallation, FedSnapshot, Hello, Identity, Invitation, NodeClient, ACTOR_HEADER, KEY_PREFIX};
use crate::hub::{NodeStatus, ZoneCommand};
use crate::AppState;
use axum::extract::{ConnectInfo, FromRequestParts, Path, State};
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};
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
    /// Invitation token, shown once (only the hash of the key is kept).
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
    let (key, hash) = federation::new_share_key();
    let mut ids = req.installation_ids.clone();
    ids.sort_unstable();
    ids.dedup();
    let share = db::insert_share(&state.db, &label, &hash, &req.permission, expires_at, &admin.username, &ids).await?;
    let token = federation::encode_token(&Invitation { v: 1, node_id: me.node_id, name: me.name, url, key });
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

/// Revokes a share: the key stops working immediately.
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
    /// Invitation token (`bz1.…`) from the node that shares.
    pub token: String,
    /// Address to use instead of the one in the token (e.g. its local address).
    #[serde(default)]
    pub url: Option<String>,
}

/// Adds a node from its invitation token: checks it answers, then imports its installations.
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
    let client = NodeClient::new(&url, &inv.key);
    let snap = client.hello(&me).await.map_err(|e| match e {
        federation::NodeError::Unreachable(m) => ApiError::bad_gateway(format!("le serveur « {} » ne répond pas à {url} ({m})", inv.name)),
        other => other.into(),
    })?;
    if snap.node_id != inv.node_id {
        return Err(ApiError::bad_request(format!("{url} n'est pas le serveur « {} » du jeton", inv.name)));
    }
    let key_enc = cloud::encrypt(&state.cloud_key, &inv.key);
    let node = db::upsert_node(&state.db, &snap.node_id, &snap.name, &url, &key_enc, &snap.permission).await?;
    state.hub.reload_installations().await?;
    state.hub.poll_node(node.id).await;
    state
        .hub
        .log(
            "system",
            "info",
            None,
            Some(&admin.username),
            &format!("Nœud « {} » ajouté ({} installation(s), {})", snap.name, snap.installations.len(), permission_label(&snap.permission)),
            None,
        )
        .await;
    // Installations of the Airzone account reached through this node.
    for i in db::list_installations(&state.db).await?.into_iter().filter(|i| i.node_id == Some(node.id)) {
        if let Err(e) = cloud::auto_link(&state, i.id, &admin.username).await {
            tracing::warn!("cannot link to Airzone Cloud: {}", e.message);
        }
    }
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
// Node to node, authenticated by a share key
// ---------------------------------------------------------------------------------------

/// A call from another node, with a valid share key.
pub struct ShareAuth {
    pub share: Share,
    pub installation_ids: Vec<i64>,
    pub address: Option<String>,
}

impl FromRequestParts<AppState> for ShareAuth {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let key = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer ").or_else(|| h.strip_prefix("bearer ")))
            .map(str::trim)
            .filter(|k| k.starts_with(KEY_PREFIX))
            .ok_or_else(|| ApiError::unauthorized("share key required"))?;
        let share = db::share_by_key_hash(&state.db, &hash_token(key)).await?.ok_or_else(|| ApiError::unauthorized("share revoked"))?;
        if share.expires_at.is_some_and(|e| e <= db::now()) {
            return Err(ApiError::unauthorized("share expired"));
        }
        let installation_ids = db::share_installation_ids(&state.db, share.id).await?;
        let connect = parts.extensions.get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip().to_string());
        let address = client_address(&parts.headers).or(connect);
        Ok(ShareAuth { share, installation_ids, address })
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

/// First call of a node: records who uses the share.
pub async fn hello(State(state): State<AppState>, auth: ShareAuth, Json(req): Json<Hello>) -> ApiResult<Json<FedSnapshot>> {
    let name: String = req.name.trim().chars().take(60).collect();
    let node_id: String = req.node_id.trim().chars().take(64).collect();
    db::touch_share(&state.db, auth.share.id, Some(&node_id), Some(&name), auth.address.as_deref()).await?;
    if auth.share.remote_node_id.as_deref() != Some(node_id.as_str()) {
        state
            .hub
            .log("system", "info", None, None, &format!("Le serveur « {name} » utilise le partage « {} »", auth.share.label), None)
            .await;
    }
    Ok(Json(fed_snapshot(&state, &auth).await?))
}

pub async fn installations(State(state): State<AppState>, auth: ShareAuth) -> ApiResult<Json<FedSnapshot>> {
    db::touch_share(&state.db, auth.share.id, None, None, auth.address.as_deref()).await?;
    Ok(Json(fed_snapshot(&state, &auth).await?))
}

pub async fn control(
    State(state): State<AppState>,
    auth: ShareAuth,
    headers: HeaderMap,
    Path((id, system_id, zone_id)): Path<(i64, i64, i64)>,
    Json(cmd): Json<ZoneCommand>,
) -> ApiResult<Json<FedInstallation>> {
    if !auth.installation_ids.contains(&id) {
        return Err(ApiError::not_found("installation non partagée"));
    }
    if auth.share.permission != "control" {
        return Err(ApiError::forbidden("ce partage est en lecture seule"));
    }
    db::touch_share(&state.db, auth.share.id, None, None, auth.address.as_deref()).await?;
    let node = auth.share.remote_name.clone().unwrap_or_else(|| auth.share.label.clone());
    let user: String = headers
        .get(ACTOR_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.chars().filter(|c| !c.is_control()).take(60).collect())
        .unwrap_or_default();
    let actor = if user.is_empty() { format!("nœud {node}") } else { format!("{user} (nœud {node})") };
    state.hub.control(id, system_id, zone_id, &cmd, &actor).await?;
    state.hub.federated(&[id]).await.into_iter().next().map(Json).ok_or_else(|| ApiError::not_found("installation non partagée"))
}
