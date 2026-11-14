//! Federation between Breezone nodes (ADR 0008).
//!
//! * Administration (user JWT): identity of this node, shares it exposes (and the access requests
//!   to accept), nodes it consumes.
//! * Node to node (requests signed with the node's Ed25519 key): `/api/federation/*`, limited to
//!   the installations of the share. `POST /api/federation/pair` uses a single-use invitation code.

use crate::auth::{hash_token, AdminUser, AuthUser};
use crate::capabilities::{Capabilities, PeerView};
use crate::db::{self, Share};
use crate::error::{ApiError, ApiResult};
use crate::federation::{self, FedInstallation, FedSnapshot, Identity, Invitation, NodeClient, PairRequest, PairResponse, ACTOR_HEADER};
use crate::hub::{HubMessage, NodeStatus, ZoneCommand};
use crate::i18n::Text;
use crate::tls;
use crate::AppState;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, FromRequest, Path, Query, Request, State};
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
    /// The invitation asks the other node to share back.
    pub reciprocal: bool,
    /// The other node accepted to share back: its installations are added here on approval.
    pub remote_shares_back: bool,
    /// Share given back to a node that invited this one (reciprocal invitation).
    pub reverse: bool,
    /// Version and capabilities of the node reading the share (ADR 0012), once it said hello.
    pub peer: Option<PeerView>,
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
        reverse: s.is_reverse(),
        reciprocal: s.reciprocal && !s.is_reverse(),
        remote_shares_back: s.remote_shares_back,
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
        peer: PeerView::of(s.peer_caps.as_deref(), s.peer_caps_at),
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
    /// Ask the other node to share its installations with this one too.
    #[serde(default)]
    pub reciprocal: bool,
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
        let inst = db::installation_by_id(&state.db, *id).await?.ok_or_else(|| ApiError::bad_request(crate::text!("installation {0} inconnue", "unknown installation {0}", "instalación {0} desconocida", id)))?;
        if !inst.is_local() {
            // Re-sharing installations of another node would allow loops.
            return Err(ApiError::bad_request(crate::text!(
                "« {0} » est jointe via un autre nœud : seules les installations locales sont partageables",
                "“{0}” is joined through another node: only local installations can be shared",
                "«{0}» está unida a través de otro nodo: solo se pueden compartir las instalaciones locales",
                inst.name
            )));
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
    let share = db::insert_share(&state.db, &label, &hash, &req.permission, expires_at, &admin.username, &ids, req.reciprocal).await?;
    let token = federation::encode_token(&Invitation { v: 2, node_id: me.node_id, name: me.name, url, code, reciprocal: req.reciprocal });
    state
        .hub
        .log("system", "info", None, Some(&admin.username), share_created(false, &label, ids.len(), &req.permission), None)
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
    state.hub.log("system", "info", None, Some(&admin.username), crate::text!("Partage « {0} » modifié", "Share “{0}” changed", "Uso compartido «{0}» modificado", label), None).await;
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
    state.hub.log("system", "info", None, Some(&admin.username), crate::text!("Accès accordé au serveur « {0} » (partage « {1} »)", "Access granted to server “{0}” (share “{1}”)", "Acceso concedido al servidor «{0}» (uso compartido «{1}»)", who, share.label), None).await;
    // Reciprocal invitation accepted by the other node: consume what it shares back.
    if let (true, true, Some(url), Some(node_id)) = (share.reciprocal, share.remote_shares_back, share.remote_url.as_deref(), share.remote_node_id.as_deref()) {
        let node = db::upsert_node(&state.db, node_id, &who, url, "", "read").await?;
        state.hub.reload_installations().await?;
        state.hub.poll_node(node.id).await;
        state.hub.log("system", "info", None, Some(&admin.username), crate::text!(
            "Nœud « {0} » ajouté : il partage aussi ses installations avec ce serveur",
            "Node “{0}” added: it also shares its installations with this server",
            "Nodo «{0}» añadido: también comparte sus instalaciones con este servidor",
            who
        ), None).await;
    }
    let share = db::share_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("partage introuvable"))?;
    Ok(Json(share_view(&state, share).await?))
}

/// Revokes a share (or refuses a request): access stops immediately.
pub async fn delete_share(State(state): State<AppState>, AdminUser(admin): AdminUser, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    let share = db::share_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("partage introuvable"))?;
    db::delete_share(&state.db, id).await?;
    state.hub.log("system", "info", None, Some(&admin.username), crate::text!("Partage « {0} » révoqué", "Share “{0}” revoked", "Uso compartido «{0}» revocado", share.label), None).await;
    Ok(StatusCode::NO_CONTENT)
}

/// "Share … created (n installations, read)" for the activity feed.
fn share_created(reciprocal: bool, label: &str, count: usize, permission: &str) -> Text {
    let permission = if permission == "control" { crate::text!("pilotage", "control", "control") } else { crate::text!("lecture", "read", "lectura") };
    if reciprocal {
        crate::text!(
            "Partage réciproque « {0} » créé ({1} installation(s), {2})",
            "Reciprocal share “{0}” created ({1} installation(s), {2})",
            "Uso compartido recíproco «{0}» creado ({1} instalación(es), {2})",
            label,
            count,
            permission.arg()
        )
    } else {
        crate::text!(
            "Partage « {0} » créé ({1} installation(s), {2})",
            "Share “{0}” created ({1} installation(s), {2})",
            "Uso compartido «{0}» creado ({1} instalación(es), {2})",
            label,
            count,
            permission.arg()
        )
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
    /// A certificate failing the usual HTTPS checks was trusted for this node.
    pub certificate_pinned: bool,
    /// Installations the node shares with this one, and whether they are shown here.
    pub offered: Vec<OfferedView>,
    /// Version and capabilities of the node (ADR 0012), once exchanged.
    pub peer: Option<PeerView>,
}

#[derive(Serialize)]
pub struct OfferedView {
    /// Id on the other node.
    pub remote_id: i64,
    pub name: String,
    pub location: Option<String>,
    /// Shown on this server.
    pub included: bool,
}

async fn node_views(state: &AppState) -> ApiResult<Vec<NodeView>> {
    let statuses = state.hub.node_statuses().await;
    let mut offers = state.hub.node_offers().await;
    let installations = db::list_installations(&state.db).await?;
    let mut exclusions = HashMap::new();
    for n in db::list_nodes(&state.db).await? {
        exclusions.insert(n.id, db::node_exclusions(&state.db, n.id).await?);
    }
    Ok(db::list_nodes(&state.db)
        .await?
        .into_iter()
        .map(|n| NodeView {
            offered: offers
                .remove(&n.id)
                .unwrap_or_default()
                .into_iter()
                .map(|o| OfferedView {
                    included: !exclusions.get(&n.id).is_some_and(|e| e.contains(&o.remote_id)),
                    remote_id: o.remote_id,
                    name: o.name,
                    location: o.location,
                })
                .collect(),
            status: statuses.get(&n.id).cloned().unwrap_or(NodeStatus {
                state: "unknown",
                last_seen: n.last_seen,
                last_error: n.last_error.as_deref().map(Text::load),
                latency_ms: None,
                permission: n.permission.clone(),
            }),
            installations: installations
                .iter()
                .filter(|i| i.node_id == Some(n.id))
                .map(|i| ShareInstallation { id: i.id, name: i.name.clone() })
                .collect(),
            certificate_pinned: n.tls_fingerprint.is_some(),
            peer: PeerView::of(n.peer_caps.as_deref(), n.peer_caps_at),
            id: n.id,
            name: n.name,
            url: n.url,
            created_at: n.created_at,
        })
        .collect())
}

pub async fn list_nodes(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<NodeView>>> {
    let access = crate::access::Access::of(&state.db, &user).await?;
    let mut nodes = node_views(&state).await?;
    if !access.is_admin() {
        for n in &mut nodes {
            n.installations.retain(|i| access.can_view(i.id));
            n.offered.clear();
        }
    }
    Ok(Json(nodes))
}

#[derive(Deserialize)]
pub struct AddNodeInput {
    /// Invitation token (`bz2.…`) from the node that shares.
    pub token: String,
    /// Address to use instead of the one in the token (e.g. its local address).
    #[serde(default)]
    pub url: Option<String>,
    /// Answer to a reciprocal invitation: what this node shares back (none = declined).
    #[serde(default)]
    pub reciprocal: Option<ReciprocalInput>,
    /// Fingerprint of a certificate the administrator reviewed and trusts although it fails the
    /// usual HTTPS checks (only that certificate is then accepted for this node).
    #[serde(default)]
    pub certificate: Option<String>,
}

/// Certificate error with what an administrator needs to decide whether to trust it.
async fn certificate_error(url: &str, who: &str, reason: &Text) -> ApiError {
    let err = ApiError::bad_gateway(crate::text!("le serveur « {0} » à {1} : {2}", "server “{0}” at {1}: {2}", "el servidor «{0}» en {1}: {2}", who, url, reason.arg()));
    match tls::fetch_certificate(url).await {
        Some(info) => err.with_detail("certificate", info).with_detail("certificate_error", reason),
        None => err,
    }
}

#[derive(Deserialize)]
pub struct ReciprocalInput {
    pub installation_ids: Vec<i64>,
    #[serde(default = "read")]
    pub permission: String,
    /// Address at which the other node reaches this one (default: the public address).
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
    // Share back (reciprocal invitation): checked before the single-use code is burnt.
    let share_back = match (&req.reciprocal, inv.reciprocal) {
        (Some(r), true) => {
            let input = ShareInput {
                label: inv.name.clone(),
                installation_ids: r.installation_ids.clone(),
                permission: r.permission.clone(),
                expires_in_days: None,
                url: None,
                reciprocal: false,
            };
            let (label, _) = validate_share(&state, &input).await?;
            let my_url = match r.url.as_deref().map(str::trim).filter(|u| !u.is_empty()) {
                Some(u) => federation::normalize_url(u)?,
                None => me.public_url.clone().ok_or_else(|| {
                    ApiError::bad_request("indiquez l'adresse à laquelle l'autre serveur joindra celui-ci (Réglages › Partages › Adresse publique)")
                })?,
            };
            let mut ids = r.installation_ids.clone();
            ids.sort_unstable();
            ids.dedup();
            Some((label, ids, r.permission.clone(), my_url))
        }
        (Some(_), false) => return Err(ApiError::bad_request("ce jeton ne demande pas de partage réciproque")),
        _ => None,
    };
    let pinned = match req.certificate.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
        Some(c) => Some(tls::normalize_fingerprint(c).ok_or_else(|| ApiError::bad_request("empreinte de certificat invalide"))?),
        None => None,
    };
    let key = federation::signing_key(&state.db, &state.cloud_key).await?;
    let client = NodeClient::new(&url, &me.node_id, key, pinned.as_deref());
    let back_url = share_back.as_ref().map(|(_, _, _, u)| u.as_str());
    let paired = match client.pair(&inv.code, &me.name, back_url).await {
        Ok(p) => p,
        Err(federation::NodeError::Certificate(reason)) => return Err(certificate_error(&url, &inv.name, &reason).await),
        Err(federation::NodeError::Unreachable(m)) => return Err(ApiError::bad_gateway(crate::text!(
                "le serveur « {0} » ne répond pas à {1} ({2})",
                "server “{0}” does not answer at {1} ({2})",
                "el servidor «{0}» no responde en {1} ({2})",
                inv.name,
                url,
                m.arg()
            ))),
        Err(other) => return Err(other.into()),
    };
    if paired.node_id != inv.node_id {
        return Err(ApiError::bad_request(crate::text!(
            "{0} n'est pas le serveur « {1} » du jeton",
            "{0} is not the server “{1}” of the token",
            "{0} no es el servidor «{1}» del token",
            url,
            inv.name
        )));
    }
    if let Some((label, ids, permission, _)) = &share_back {
        let public_key = paired.public_key.as_deref().ok_or_else(|| ApiError::bad_gateway("l'autre serveur n'a pas envoyé sa clé publique : mettez-le à jour"))?;
        db::insert_reverse_share(&state.db, label, permission, &admin.username, ids, &paired.node_id, &paired.name, public_key).await?;
        state
            .hub
            .log(
                "system",
                "info",
                None,
                Some(&admin.username),
                share_created(true, label, ids.len(), permission),
                None,
            )
            .await;
    }
    let node = db::upsert_node(&state.db, &paired.node_id, &paired.name, &url, "", "read").await?;
    db::set_node_fingerprint(&state.db, node.id, pinned.as_deref()).await?;
    state.hub.reload_installations().await?;
    state.hub.poll_node(node.id).await;
    if pinned.is_some() {
        state.hub.log("system", "warn", None, Some(&admin.username), certificate_trusted(&paired.name), None).await;
    }
    state
        .hub
        .log(
            "system",
            "info",
            None,
            Some(&admin.username),
            crate::text!(
                "Nœud « {0} » ajouté : en attente d'acceptation sur ce serveur (Réglages › Partages)",
                "Node “{0}” added: waiting for approval on that server (Settings › Shares)",
                "Nodo «{0}» añadido: pendiente de aceptación en ese servidor (Ajustes › Compartidos)",
                paired.name
            ),
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
    state.hub.log("system", "info", None, Some(&admin.username), crate::text!("Nœud « {0} » : adresse {1}", "Node “{0}”: address {1}", "Nodo «{0}»: dirección {1}", node.name, url), None).await;
    node_views(&state).await?.into_iter().find(|n| n.id == id).map(Json).ok_or_else(|| ApiError::not_found("nœud introuvable"))
}

/// Certificate the node presents, to review it before trusting it.
pub async fn node_certificate(State(state): State<AppState>, _admin: AdminUser, Path(id): Path<i64>) -> ApiResult<Json<NodeCertificate>> {
    let node = db::node_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("nœud introuvable"))?;
    let certificate = tls::fetch_certificate(&node.url).await.ok_or_else(|| ApiError::bad_gateway(crate::text!("impossible de lire le certificat de {0}", "cannot read the certificate of {0}", "no se puede leer el certificado de {0}", node.url)))?;
    Ok(Json(NodeCertificate { pinned: node.tls_fingerprint, certificate }))
}

#[derive(Serialize)]
pub struct NodeCertificate {
    /// Certificate trusted so far, if any.
    pub pinned: Option<String>,
    pub certificate: tls::CertificateInfo,
}

#[derive(Deserialize)]
pub struct TrustInput {
    /// Fingerprint of the certificate reviewed (it must still be the one presented).
    pub fingerprint: String,
}

/// Trusts the certificate a node presents although it fails the usual HTTPS checks.
pub async fn trust_node_certificate(State(state): State<AppState>, AdminUser(admin): AdminUser, Path(id): Path<i64>, Json(req): Json<TrustInput>) -> ApiResult<Json<NodeView>> {
    let node = db::node_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("nœud introuvable"))?;
    let wanted = tls::normalize_fingerprint(&req.fingerprint).ok_or_else(|| ApiError::bad_request("empreinte de certificat invalide"))?;
    let current = tls::fetch_certificate(&node.url).await.ok_or_else(|| ApiError::bad_gateway(crate::text!("impossible de lire le certificat de {0}", "cannot read the certificate of {0}", "no se puede leer el certificado de {0}", node.url)))?;
    if current.fingerprint != wanted {
        return Err(ApiError::conflict("le certificat a changé entre-temps : examinez-le de nouveau").with_detail("certificate", current));
    }
    db::set_node_fingerprint(&state.db, id, Some(&wanted)).await?;
    state.hub.reload_installations().await?;
    state.hub.poll_node(id).await;
    state.hub.log("system", "warn", None, Some(&admin.username), certificate_trusted(&node.name), None).await;
    node_views(&state).await?.into_iter().find(|n| n.id == id).map(Json).ok_or_else(|| ApiError::not_found("nœud introuvable"))
}

/// Goes back to the usual HTTPS checks (e.g. once the node has a valid certificate).
pub async fn untrust_node_certificate(State(state): State<AppState>, AdminUser(admin): AdminUser, Path(id): Path<i64>) -> ApiResult<Json<NodeView>> {
    let node = db::node_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("nœud introuvable"))?;
    db::set_node_fingerprint(&state.db, id, None).await?;
    state.hub.reload_installations().await?;
    state.hub.poll_node(id).await;
    state.hub.log("system", "info", None, Some(&admin.username), crate::text!(
        "Serveur « {0} » : vérification HTTPS habituelle rétablie",
        "Server “{0}”: usual HTTPS check restored",
        "Servidor «{0}»: verificación HTTPS habitual restablecida",
        node.name
    ), None).await;
    node_views(&state).await?.into_iter().find(|n| n.id == id).map(Json).ok_or_else(|| ApiError::not_found("nœud introuvable"))
}

#[derive(Deserialize)]
pub struct NodeInstallationsInput {
    /// Installations of the node to show on this server (ids on that node); the others it
    /// shares are hidden. Installations it shares later are shown until excluded.
    pub included: Vec<i64>,
}

/// Chooses which of the installations a node shares are shown on this server.
pub async fn set_node_installations(
    State(state): State<AppState>,
    AdminUser(admin): AdminUser,
    Path(id): Path<i64>,
    Json(req): Json<NodeInstallationsInput>,
) -> ApiResult<Json<NodeView>> {
    let node = db::node_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("nœud introuvable"))?;
    let offered = state.hub.node_offers().await.remove(&id).unwrap_or_default();
    if offered.is_empty() {
        return Err(ApiError::bad_request(crate::text!(
            "« {0} » ne partage encore aucune installation avec ce serveur",
            "“{0}” does not share any installation with this server yet",
            "«{0}» aún no comparte ninguna instalación con este servidor",
            node.name
        )));
    }
    // Exclusions of installations not offered right now are kept.
    let mut excluded: Vec<i64> = db::node_exclusions(&state.db, id).await?.into_iter().filter(|r| !offered.iter().any(|o| o.remote_id == *r)).collect();
    excluded.extend(offered.iter().map(|o| o.remote_id).filter(|r| !req.included.contains(r)));
    db::set_node_exclusions(&state.db, id, &excluded).await?;
    state.hub.poll_node(id).await;
    let shown = offered.iter().filter(|o| req.included.contains(&o.remote_id)).count();
    state
        .hub
        .log(
            "system",
            "info",
            None,
            Some(&admin.username),
            crate::text!(
                "Nœud « {0} » : {1} installation(s) sur {2} affichée(s)",
                "Node “{0}”: {1} of {2} installation(s) shown",
                "Nodo «{0}»: {1} de {2} instalación(es) mostrada(s)",
                node.name,
                shown,
                offered.len()
            ),
            None,
        )
        .await;
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
    state.hub.log("system", "info", None, Some(&admin.username), crate::text!("Nœud « {0} » retiré", "Node “{0}” removed", "Nodo «{0}» eliminado", node.name), None).await;
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
    let mut snap = FedSnapshot {
        node_id: me.node_id,
        name: me.name,
        permission: auth.share.permission.clone(),
        installations: state.hub.federated(&auth.installation_ids).await,
        revision: None,
        caps_rev: Some(Capabilities::local().caps_rev),
    };
    snap.revision = Some(snap.compute_revision());
    Ok(snap)
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
    // Only a reciprocal invitation lets the requesting node share back.
    let shares_back_from = match req.shares_back_from.as_deref().filter(|_| share.reciprocal) {
        Some(u) => Some(federation::normalize_url(u)?),
        None => None,
    };
    if !db::claim_share(&state.db, share.id, &node_id, &name, &req.public_key, signed.address.as_deref(), shares_back_from.as_deref()).await? {
        return Err(ApiError::bad_request("invitation déjà utilisée : créez un nouveau partage"));
    }
    let from = signed.address.as_deref().unwrap_or("?");
    state
        .hub
        .log(
            "system",
            "warn",
            None,
            None,
            crate::text!(
                "Le serveur « {0} » ({2}) demande l'accès au partage « {1} » : acceptez ou refusez dans Réglages › Partages",
                "Server “{0}” ({2}) asks for access to share “{1}”: accept or refuse in Settings › Shares",
                "El servidor «{0}» ({2}) pide acceso al uso compartido «{1}»: acepte o rechace en Ajustes › Compartidos",
                name,
                share.label,
                from
            ),
            None,
        )
        .await;
    let key = federation::signing_key(&state.db, &state.cloud_key).await?;
    Ok(Json(PairResponse { status: "pending".into(), node_id: me.node_id, name: me.name, public_key: Some(federation::public_key_text(&key)) }))
}

/// Exchange of capabilities (ADR 0012): the reading node sends its own, this one answers with its.
/// Called when the link opens, and again whenever either side's `caps_rev` changes.
pub async fn hello(State(state): State<AppState>, auth: ShareAuth) -> ApiResult<Json<Capabilities>> {
    let peer: Capabilities = serde_json::from_slice(&auth.body).map_err(|_| ApiError::bad_request("invalid capabilities"))?;
    let peer = peer.sanitized();
    let before: Option<Capabilities> = auth.share.peer_caps.as_deref().and_then(|c| serde_json::from_str(c).ok());
    let json = serde_json::to_string(&peer).map_err(anyhow::Error::from)?;
    db::set_share_caps(&state.db, auth.share.id, &json).await?;
    db::touch_share(&state.db, auth.share.id, None, None, auth.address.as_deref()).await?;
    let who = auth.share.remote_name.clone().unwrap_or_else(|| auth.share.label.clone());
    if before.is_some_and(|b| b.breezone != peer.breezone) && !peer.breezone.is_empty() {
        state
            .hub
            .log(
                "system",
                "info",
                None,
                None,
                crate::text!("Le serveur « {0} » est passé à Breezone {1}", "Server “{0}” moved to Breezone {1}", "El servidor «{0}» pasó a Breezone {1}", who, peer.breezone),
                None,
            )
            .await;
    }
    Ok(Json(Capabilities::local()))
}

#[derive(Deserialize)]
pub struct WatchQuery {
    /// Seconds to wait for a change (long polling), at most [`federation::WATCH_SECS`].
    wait: Option<u64>,
    /// Revision the reading node already has: answer only once the content differs.
    since: Option<String>,
}

/// What this node shares with the calling one. With `?wait=&since=`, the answer waits until a
/// shared installation changes (or the wait ends), so the other node follows it live.
pub async fn installations(State(state): State<AppState>, Query(q): Query<WatchQuery>, auth: ShareAuth) -> ApiResult<axum::response::Response> {
    use axum::response::IntoResponse;
    db::touch_share(&state.db, auth.share.id, None, None, auth.address.as_deref()).await?;
    // Subscribed before reading, so that no change slips between the read and the wait.
    let mut changes = state.hub.subscribe();
    let mut snap = fed_snapshot(&state, &auth).await?;
    let wait = q.wait.unwrap_or(0).min(federation::WATCH_SECS);
    if wait == 0 || q.since.is_none() || q.since != snap.revision {
        return Ok(Json(snap).into_response());
    }
    let deadline = tokio::time::sleep(std::time::Duration::from_secs(wait));
    tokio::pin!(deadline);
    loop {
        let relevant = tokio::select! {
            _ = &mut deadline => break,
            msg = changes.recv() => match msg {
                Ok(HubMessage::Installation { installation }) => auth.installation_ids.contains(&installation.id),
                Ok(HubMessage::InstallationRemoved { id }) => auth.installation_ids.contains(&id),
                Ok(_) => false,
                // Missed messages: re-read to be sure.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => true,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            },
        };
        if relevant {
            snap = fed_snapshot(&state, &auth).await?;
            if q.since != snap.revision {
                return Ok(Json(snap).into_response());
            }
        }
    }
    // Nothing new: a few bytes instead of the whole content (the other node keeps what it has).
    let snap = fed_snapshot(&state, &auth).await?;
    if q.since != snap.revision {
        return Ok(Json(snap).into_response());
    }
    Ok(StatusCode::NOT_MODIFIED.into_response())
}

pub async fn control(State(state): State<AppState>, Path((id, system_id, zone_id)): Path<(i64, i64, i64)>, auth: ShareAuth) -> ApiResult<Json<FedInstallation>> {
    let cmd = ZoneCommand::parse_strict(&auth.body)?;
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

/// Another node changes the Airzone schedules of an installation this node shares (with control):
/// this node applies it with its own Airzone account.
pub async fn schedule(State(state): State<AppState>, Path(id): Path<i64>, auth: ShareAuth) -> ApiResult<Json<serde_json::Value>> {
    let op: crate::airzone_schedules::ScheduleOp = serde_json::from_slice(&auth.body)
        .map_err(|e| ApiError::bad_request(crate::text!("commande invalide : {0}", "invalid command: {0}", "orden no válida: {0}", e)))?;
    if !auth.installation_ids.contains(&id) {
        return Err(ApiError::not_found("installation non partagée"));
    }
    if auth.share.permission != "control" {
        return Err(ApiError::forbidden("ce partage est en lecture seule"));
    }
    let inst = db::installation_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("installation non partagée"))?;
    let cloud_id = inst.cloud_id.ok_or_else(|| {
        ApiError::bad_request(crate::text!(
            "« {0} » n'est pas reliée au compte Airzone du serveur qui la partage",
            "“{0}” is not linked to the Airzone account of the server sharing it",
            "«{0}» no está vinculada a la cuenta de Airzone del servidor que la comparte",
            inst.name
        ))
    })?;
    db::touch_share(&state.db, auth.share.id, None, None, auth.address.as_deref()).await?;
    let node = auth.share.remote_name.clone().unwrap_or_else(|| auth.share.label.clone());
    let user: String = auth
        .headers
        .get(ACTOR_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.chars().filter(|c| !c.is_control()).take(60).collect())
        .unwrap_or_default();
    let actor = if user.is_empty() { format!("nœud {node}") } else { format!("{user} (nœud {node})") };
    crate::airzone_schedules::apply(&state, &cloud_id, &op, &actor).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

fn certificate_trusted(server: &str) -> Text {
    crate::text!(
        "Certificat HTTPS du serveur « {0} » accepté manuellement",
        "HTTPS certificate of server “{0}” accepted manually",
        "Certificado HTTPS del servidor «{0}» aceptado manualmente",
        server
    )
}
