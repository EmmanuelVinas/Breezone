//! Sources of devices (ADR 0013), for administrators: a piece of hardware read by a driver (a
//! Shelly at 192.168.1.40…). Its devices show in `/api/devices`.

use crate::auth::AdminUser;
use crate::db;
use crate::drivers::{DriverInfo, SourceStatus};
use crate::error::{ApiError, ApiResult};
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The drivers this server has, with their configuration form.
pub async fn drivers(_admin: AdminUser) -> Json<Vec<DriverInfo>> {
    Json(crate::drivers::available())
}

#[derive(Serialize)]
pub struct SourceView {
    pub id: i64,
    pub driver: String,
    pub name: String,
    pub config: Value,
    pub enabled: bool,
    pub created_at: i64,
    pub status: SourceStatus,
    /// Devices it provides now.
    pub devices: usize,
}

async fn view(state: &AppState, s: db::Source) -> SourceView {
    let devices = state.drivers.devices().await.iter().filter(|d| d.source_id == s.id).count();
    SourceView {
        status: state.drivers.status(s.id).await.unwrap_or_default(),
        config: serde_json::from_str(&s.config).unwrap_or(Value::Null),
        devices,
        id: s.id,
        driver: s.driver,
        name: s.name,
        enabled: s.enabled,
        created_at: s.created_at,
    }
}

pub async fn list(State(state): State<AppState>, _admin: AdminUser) -> ApiResult<Json<Vec<SourceView>>> {
    let mut out = vec![];
    for s in db::list_sources(&state.db).await? {
        out.push(view(&state, s).await);
    }
    Ok(Json(out))
}

#[derive(Deserialize)]
pub struct SourceInput {
    pub driver: String,
    pub name: String,
    #[serde(default)]
    pub config: Value,
    #[serde(default = "enabled")]
    pub enabled: bool,
}

fn enabled() -> bool {
    true
}

/// Checks the driver and its configuration; returns the trimmed name and the configuration text.
fn checked(input: &SourceInput) -> ApiResult<(String, String)> {
    crate::drivers::create(&input.driver, &input.config).map_err(ApiError::bad_request)?;
    let name: String = input.name.trim().chars().take(60).collect();
    if name.is_empty() {
        return Err(ApiError::bad_request(crate::text!("le nom est vide", "the name is empty", "el nombre está vacío")));
    }
    Ok((name, input.config.to_string()))
}

/// Reads the source at once, so that the answer already tells whether it answers.
async fn polled(state: &AppState, id: i64) -> ApiResult<SourceView> {
    state.drivers.reload().await?;
    state.drivers.poll(id).await;
    let s = db::source_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("source not found"))?;
    Ok(view(state, s).await)
}

pub async fn create(State(state): State<AppState>, _admin: AdminUser, Json(input): Json<SourceInput>) -> ApiResult<(StatusCode, Json<SourceView>)> {
    let (name, config) = checked(&input)?;
    let id = db::insert_source(&state.db, &input.driver, &name, &config).await?;
    Ok((StatusCode::CREATED, Json(polled(&state, id).await?)))
}

pub async fn update(State(state): State<AppState>, _admin: AdminUser, Path(id): Path<i64>, Json(input): Json<SourceInput>) -> ApiResult<Json<SourceView>> {
    let current = db::source_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("source not found"))?;
    if current.driver != input.driver {
        return Err(ApiError::bad_request("the driver of a source cannot change"));
    }
    let (name, config) = checked(&input)?;
    db::update_source(&state.db, id, &name, &config, input.enabled).await?;
    Ok(Json(polled(&state, id).await?))
}

/// Removes the source and its devices.
pub async fn delete(State(state): State<AppState>, _admin: AdminUser, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    if !db::delete_source(&state.db, id).await? {
        return Err(ApiError::not_found("source not found"));
    }
    state.drivers.reload().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Reads the source now (« Tester » / « Actualiser »).
pub async fn refresh(State(state): State<AppState>, _admin: AdminUser, Path(id): Path<i64>) -> ApiResult<Json<SourceView>> {
    db::source_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("source not found"))?;
    Ok(Json(polled(&state, id).await?))
}

#[derive(Deserialize, Default)]
pub struct ScanInput {
    #[serde(default)]
    pub subnet: Option<String>,
}

#[derive(Serialize)]
pub struct FoundProduct {
    #[serde(flatten)]
    pub product: crate::discovery::Product,
    /// Already in Breezone: an installation (Airzone) or a source with this address or id.
    pub added: bool,
}

/// Every supported product on the network (Airzone WebServers, Shelly…), identified down to the
/// model, and whether each one is already added.
pub async fn scan(State(state): State<AppState>, _admin: AdminUser, body: Option<Json<ScanInput>>) -> ApiResult<Json<Value>> {
    let req = body.map(|Json(b)| b).unwrap_or_default();
    let subnet = match req.subnet.filter(|s| !s.trim().is_empty()) {
        Some(s) => Some(s),
        None => crate::discovery::global_subnet(&state.db, state.cfg.scan_subnet.as_deref()).await,
    }
    .ok_or_else(|| ApiError::bad_request("indiquez le réseau à analyser, par exemple 192.168.1.0/24"))?;
    let started = std::time::Instant::now();
    let products = crate::discovery::scan_products(&subnet, state.cfg.scan_port).await.map_err(ApiError::bad_request)?;
    let installations = db::list_installations(&state.db).await?;
    let sources = db::list_sources(&state.db).await?;
    let mac = |m: &Option<String>| m.as_deref().map(crate::cloud::normalize_mac);
    let found: Vec<FoundProduct> = products
        .into_iter()
        .map(|p| {
            let added = match p.driver {
                "airzone" => installations.iter().any(|i| i.host == p.host || (p.mac.is_some() && mac(&i.mac) == mac(&p.mac))),
                driver => sources.iter().any(|s| {
                    let config: Value = serde_json::from_str(&s.config).unwrap_or(Value::Null);
                    s.driver == driver && (config["host"] == p.host.as_str() || (p.device_id.is_some() && config["device_id"].as_str() == p.device_id.as_deref()))
                }),
            };
            FoundProduct { product: p, added }
        })
        .collect();
    Ok(Json(serde_json::json!({ "subnet": subnet, "duration_ms": started.elapsed().as_millis() as u64, "products": found })))
}
