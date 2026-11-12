//! Installations, zones, history, batch control, discovery and activity feed.

use crate::access::Access;
use crate::airzone::AirzoneClient;
use crate::auth::{AdminUser, AuthUser};
use crate::db::{self, Event, Installation, Reading, ZoneMeta};
use crate::cloud;
use crate::discovery;
use crate::error::{ApiError, ApiResult};
use crate::hub::{InstallationView, ZoneCommand, ZoneView};
use crate::routines::{self, Action, ActionResult};
use crate::stats::{self, DayStats};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub async fn status(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<InstallationView>>> {
    let access = Access::of(&state.db, &user).await?;
    Ok(Json(access.filter(state.hub.snapshot().await)))
}

pub async fn get_installation(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<Json<InstallationView>> {
    let access = Access::of(&state.db, &user).await?;
    access.view(id)?;
    state.hub.installation(id).await.map(|i| Json(access.apply(i))).ok_or_else(|| ApiError::not_found("installation not found"))
}

/// Forces an immediate poll of the installation.
pub async fn refresh_installation(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<Json<InstallationView>> {
    let access = Access::of(&state.db, &user).await?;
    access.view(id)?;
    state.hub.poll_one(id).await;
    state.hub.installation(id).await.map(|i| Json(access.apply(i))).ok_or_else(|| ApiError::not_found("installation not found"))
}

pub async fn list_installations(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<Installation>>> {
    let access = Access::of(&state.db, &user).await?;
    Ok(Json(db::list_installations(&state.db).await?.into_iter().filter(|i| access.can_view(i.id)).collect()))
}

#[derive(Deserialize)]
pub struct InstallationInput {
    pub name: String,
    pub host: String,
    #[serde(default = "default_port")]
    pub port: i64,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Record the time each zone spends in demand (installations of this server only).
    /// Unchanged when absent.
    #[serde(default)]
    pub record_demand: Option<bool>,
}

fn default_port() -> i64 {
    3000
}

fn yes() -> bool {
    true
}

fn validate_installation(i: &InstallationInput) -> ApiResult<(String, String)> {
    let name = i.name.trim().to_string();
    let host = i.host.trim().trim_start_matches("http://").trim_end_matches('/').to_string();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(ApiError::bad_request("name must be 1-80 characters"));
    }
    if host.is_empty() || host.contains(['/', ' ', '?', '#']) {
        return Err(ApiError::bad_request("host must be an IP address or a hostname"));
    }
    if !(1..=65535).contains(&i.port) {
        return Err(ApiError::bad_request("invalid port"));
    }
    Ok((name, host))
}

pub async fn create_installation(State(state): State<AppState>, AdminUser(admin): AdminUser, Json(req): Json<InstallationInput>) -> ApiResult<(StatusCode, Json<InstallationView>)> {
    let (name, host) = validate_installation(&req)?;
    let inst = db::insert_installation(&state.db, &name, &host, req.port, req.enabled).await?;
    if let Some(record) = req.record_demand {
        db::set_installation_record_demand(&state.db, inst.id, record).await?;
    }
    state.hub.reload_installations().await?;
    state.hub.log("system", "info", Some(inst.id), Some(&admin.username), crate::text!("Installation « {0} » ajoutée ({1}:{2})", "Installation “{0}” added ({1}:{2})", "Instalación «{0}» añadida ({1}:{2})", name, host, req.port), None).await;
    state.hub.poll_one(inst.id).await;
    // Same WebServer as an installation of the Airzone account: link them (no duplicate).
    if let Err(e) = cloud::auto_link(&state, inst.id, &admin.username).await {
        tracing::warn!("cannot link to Airzone Cloud: {}", e.message.en);
    }
    let view = state.hub.installation(inst.id).await.ok_or_else(|| ApiError::internal("installation not loaded"))?;
    Ok((StatusCode::CREATED, Json(Access::of(&state.db, &admin).await?.apply(view))))
}

pub async fn update_installation(State(state): State<AppState>, admin: AuthUser, Path(id): Path<i64>, Json(req): Json<InstallationInput>) -> ApiResult<Json<InstallationView>> {
    let access = Access::of(&state.db, &admin).await?;
    access.manage(id)?;
    let current = db::installation_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("installation not found"))?;
    let (name, host, port) = if current.is_local() {
        let (name, host) = validate_installation(&req)?;
        (name, host, req.port)
    } else {
        // Reached through another node: only the name and the enabled flag can change here.
        let name = req.name.trim().to_string();
        if name.is_empty() || name.chars().count() > 80 {
            return Err(ApiError::bad_request("name must be 1-80 characters"));
        }
        (name, current.host.clone(), current.port)
    };
    db::update_installation(&state.db, id, &name, &host, port, req.enabled).await?;
    // A shared installation records the demand time if the server that owns it does.
    if let Some(record) = req.record_demand.filter(|_| current.is_local()) {
        db::set_installation_record_demand(&state.db, id, record).await?;
    }
    state.hub.reload_installations().await?;
    state.hub.log("system", "info", Some(id), Some(&admin.username), crate::text!("Installation « {0} » modifiée", "Installation “{0}” changed", "Instalación «{0}» modificada", name), None).await;
    if req.enabled {
        state.hub.poll_one(id).await;
        if current.host != host || current.port != port {
            // Another WebServer (another MAC address) is another installation: its zones only share
            // numbers with the previous ones. Forget the previous names, groups, history and link.
            let new_mac = state.hub.current_webserver_mac(id).await;
            if let (Some(old), Some(new)) = (&current.mac, &new_mac) {
                if cloud::normalize_mac(old) != cloud::normalize_mac(new) {
                    db::reset_installation_for_new_webserver(&state.db, id).await?;
                    state.hub.reload_installations().await?;
                    state
                        .hub
                        .log(
                            "system",
                            "info",
                            Some(id),
                            Some(&admin.username),
                            crate::text!(
                                "« {0} » pointe vers un autre WebServer ({1}) : noms et groupes des zones, historique et lien Airzone remis à zéro",
                                "“{0}” points to another WebServer ({1}): zone names and groups, history and Airzone link reset",
                                "«{0}» apunta a otro WebServer ({1}): nombres y grupos de zonas, historial y vínculo Airzone restablecidos",
                                name,
                                new
                            ),
                            None,
                        )
                        .await;
                }
            }
            if let Err(e) = cloud::auto_link(&state, id, &admin.username).await {
                tracing::warn!("cannot link to Airzone Cloud: {}", e.message.en);
            }
        }
    }
    state.hub.installation(id).await.map(|v| Json(access.apply(v))).ok_or_else(|| ApiError::not_found("installation not found"))
}

pub async fn delete_installation(State(state): State<AppState>, admin: AuthUser, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    Access::of(&state.db, &admin).await?.manage(id)?;
    let inst = db::installation_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("installation not found"))?;
    db::delete_installation(&state.db, id).await?;
    state.hub.reload_installations().await?;
    state.hub.log("system", "info", None, Some(&admin.username), crate::text!("Installation « {0} » supprimée", "Installation “{0}” deleted", "Instalación «{0}» eliminada", inst.name), None).await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct CloudLinkInput {
    /// Airzone Cloud installation this installation is (null = none).
    #[serde(default)]
    pub cloud_id: Option<String>,
}

/// Says which installation of the Airzone account an installation is.
pub async fn link_cloud(
    State(state): State<AppState>,
    admin: AuthUser,
    Path(id): Path<i64>,
    Json(req): Json<CloudLinkInput>,
) -> ApiResult<Json<InstallationView>> {
    let access = Access::of(&state.db, &admin).await?;
    access.manage(id)?;
    cloud::link(&state, id, req.cloud_id.as_deref(), &admin.username).await?;
    state.hub.installation(id).await.map(|v| Json(access.apply(v))).ok_or_else(|| ApiError::not_found("installation not found"))
}

#[derive(Deserialize)]
pub struct TestRequest {
    pub host: String,
    #[serde(default = "default_port")]
    pub port: i64,
}

/// Checks that a host answers the Airzone local API, before saving it.
pub async fn test_installation(State(state): State<AppState>, user: AuthUser, Json(req): Json<TestRequest>) -> ApiResult<Json<Value>> {
    if !Access::of(&state.db, &user).await?.manages_any() {
        return Err(ApiError::forbidden("administrator role required"));
    }
    if !(1..=65535).contains(&req.port) {
        return Err(ApiError::bad_request("invalid port"));
    }
    let host = req.host.trim().trim_start_matches("http://").trim_end_matches('/').to_string();
    let client = AirzoneClient::new(&host, req.port as u16);
    match client.read_all().await {
        Ok(resp) => {
            let zones: Vec<Value> = resp
                .zones()
                .iter()
                .map(|z| json!({ "system_id": z.system_id, "zone_id": z.zone_id, "name": z.name, "room_temp": z.room_temp }))
                .collect();
            let info = client.webserver_info().await.ok();
            Ok(Json(json!({ "ok": true, "zones": zones, "webserver": info })))
        }
        Err(e) => Ok(Json(json!({ "ok": false, "error": e.to_string() }))),
    }
}

pub async fn control_zone(
    State(state): State<AppState>,
    user: AuthUser,
    Path((id, system_id, zone_id)): Path<(i64, i64, i64)>,
    Json(cmd): Json<ZoneCommand>,
) -> ApiResult<Json<ZoneView>> {
    Access::of(&state.db, &user).await?.control(id)?;
    Ok(Json(state.hub.control(id, system_id, zone_id, &cmd, &user.username).await?))
}

#[derive(Deserialize)]
pub struct MetaInput {
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub position: Option<i64>,
}

pub async fn update_zone_meta(
    State(state): State<AppState>,
    user: AuthUser,
    Path((id, system_id, zone_id)): Path<(i64, i64, i64)>,
    Json(req): Json<MetaInput>,
) -> ApiResult<Json<Option<ZoneView>>> {
    Access::of(&state.db, &user).await?.edit_zones(id)?;
    db::installation_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("installation not found"))?;
    let clean = |s: Option<String>| s.map(|v| v.trim().chars().take(60).collect::<String>()).filter(|v| !v.is_empty());
    let meta = ZoneMeta {
        installation_id: id,
        system_id,
        zone_id,
        display_name: clean(req.display_name),
        group_name: clean(req.group),
        position: req.position.unwrap_or(0),
    };
    Ok(Json(state.hub.set_zone_meta(meta).await?))
}

#[derive(Deserialize)]
pub struct HistoryQuery {
    #[serde(default = "default_hours")]
    pub hours: i64,
}

fn default_hours() -> i64 {
    24
}

pub async fn zone_history(
    State(state): State<AppState>,
    user: AuthUser,
    Path((id, system_id, zone_id)): Path<(i64, i64, i64)>,
    Query(q): Query<HistoryQuery>,
) -> ApiResult<Json<Vec<Reading>>> {
    Access::of(&state.db, &user).await?.view(id)?;
    let hours = q.hours.clamp(1, 24 * 90);
    Ok(Json(db::readings(&state.db, id, system_id, zone_id, db::now() - hours * 3600).await?))
}

#[derive(Deserialize)]
pub struct TrendQuery {
    #[serde(default = "default_hours")]
    pub hours: i64,
    /// Points per zone.
    #[serde(default = "default_points")]
    pub points: i64,
}

fn default_points() -> i64 {
    48
}

#[derive(Serialize)]
pub struct ZoneTrend {
    pub system_id: i64,
    pub zone_id: i64,
    /// Average room temperature per time slice, oldest first: `[ts, temperature]`.
    pub points: Vec<(i64, f64)>,
}

/// Room temperature of every zone of an installation, a few points each (zone cards).
pub async fn trends(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
    Query(q): Query<TrendQuery>,
) -> ApiResult<Json<Vec<ZoneTrend>>> {
    Access::of(&state.db, &user).await?.view(id)?;
    let span = q.hours.clamp(1, 24 * 7) * 3600;
    let bucket = span / q.points.clamp(6, 200);
    let mut trends: Vec<ZoneTrend> = vec![];
    for p in db::temperature_trends(&state.db, id, db::now() - span, bucket).await? {
        let point = (p.bucket + bucket / 2, (p.room_temp * 10.0).round() / 10.0);
        match trends.last_mut() {
            Some(t) if t.system_id == p.system_id && t.zone_id == p.zone_id => t.points.push(point),
            _ => trends.push(ZoneTrend { system_id: p.system_id, zone_id: p.zone_id, points: vec![point] }),
        }
    }
    Ok(Json(trends))
}

#[derive(Deserialize)]
pub struct DailyQuery {
    #[serde(default = "default_days")]
    pub days: i64,
}

fn default_days() -> i64 {
    14
}

/// Comfort of a zone day by day (local days of the server's time zone), oldest first.
pub async fn zone_daily(
    State(state): State<AppState>,
    user: AuthUser,
    Path((id, system_id, zone_id)): Path<(i64, i64, i64)>,
    Query(q): Query<DailyQuery>,
) -> ApiResult<Json<Vec<DayStats>>> {
    Access::of(&state.db, &user).await?.view(id)?;
    let days = q.days.clamp(1, 90);
    let tz = state.cfg.timezone;
    let now = db::now();
    // Midnight, local time, `days - 1` days ago.
    let first = chrono::Utc::now().with_timezone(&tz).date_naive() - chrono::Days::new(days as u64 - 1);
    let since = first
        .and_hms_opt(0, 0, 0)
        .and_then(|t| t.and_local_timezone(tz).earliest())
        .map_or(now - days * 86_400, |t| t.timestamp());
    let readings = db::readings(&state.db, id, system_id, zone_id, since).await?;
    Ok(Json(stats::daily(&readings, tz, state.cfg.history_interval_secs * 2, now)))
}

#[derive(Deserialize)]
pub struct BatchRequest {
    pub actions: Vec<Action>,
}

/// Applies several zone commands at once (e.g. switch a whole floor off).
pub async fn batch(State(state): State<AppState>, user: AuthUser, Json(req): Json<BatchRequest>) -> ApiResult<Json<Value>> {
    if req.actions.is_empty() || req.actions.len() > 100 {
        return Err(ApiError::bad_request("1 to 100 actions expected"));
    }
    Access::of(&state.db, &user).await?.control_actions(&req.actions)?;
    let results: Vec<ActionResult> = routines::run_actions(&state.hub, &req.actions, &user.username).await;
    Ok(Json(json!({ "results": results })))
}

#[derive(Deserialize, Default)]
pub struct ScanRequest {
    #[serde(default)]
    pub subnet: Option<String>,
}

pub async fn scan(State(state): State<AppState>, _admin: AdminUser, body: Option<Json<ScanRequest>>) -> ApiResult<Json<Value>> {
    let req = body.map(|Json(b)| b).unwrap_or_default();
    let subnet = match req.subnet.filter(|s| !s.trim().is_empty()) {
        Some(s) => Some(s),
        None => discovery::global_subnet(&state.db, state.cfg.scan_subnet.as_deref()).await,
    }
    .ok_or_else(|| ApiError::bad_request("indiquez le réseau à analyser, par exemple 192.168.1.0/24"))?;
    let found = discovery::scan(Some(&subnet), state.cfg.scan_port).await.map_err(ApiError::bad_request)?;
    Ok(Json(json!({ "subnet": subnet, "found": found })))
}

/// Default subnet scanned to find WebServers (and the server's own guess).
pub async fn discovery_settings(State(state): State<AppState>, _user: AuthUser) -> ApiResult<Json<Value>> {
    let saved = db::get_setting(&state.db, "scan.subnet").await?;
    Ok(Json(json!({
        "subnet": saved,
        "effective": discovery::global_subnet(&state.db, state.cfg.scan_subnet.as_deref()).await,
        "in_container": discovery::in_container(),
    })))
}

#[derive(Deserialize)]
pub struct DiscoverySettings {
    #[serde(default)]
    pub subnet: Option<String>,
}

pub async fn save_discovery_settings(State(state): State<AppState>, _admin: AdminUser, Json(req): Json<DiscoverySettings>) -> ApiResult<Json<Value>> {
    discovery::save_global_subnet(&state.db, req.subnet.as_deref()).await.map_err(ApiError::bad_request)?;
    Ok(Json(json!({
        "subnet": db::get_setting(&state.db, "scan.subnet").await?,
        "effective": discovery::global_subnet(&state.db, state.cfg.scan_subnet.as_deref()).await,
        "in_container": discovery::in_container(),
    })))
}

#[derive(Deserialize)]
pub struct EventsQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub before: Option<i64>,
    #[serde(default)]
    pub installation_id: Option<i64>,
}

fn default_limit() -> i64 {
    100
}

pub async fn events(State(state): State<AppState>, user: AuthUser, Query(q): Query<EventsQuery>) -> ApiResult<Json<Vec<Event>>> {
    let visible = Access::of(&state.db, &user).await?.visible();
    Ok(Json(db::list_visible_events(&state.db, q.limit.clamp(1, 500), q.before, q.installation_id, visible.as_deref()).await?))
}
