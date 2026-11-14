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
    crate::drivers::create(&input.driver, &input.config, None).map_err(ApiError::bad_request)?;
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

/// Removes the source and its devices. For a HomeKit accessory, Breezone first removes its own
/// pairing from it (and nothing else), so that it can be paired again.
pub async fn delete(State(state): State<AppState>, _admin: AdminUser, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    if let Some(source) = db::source_by_id(&state.db, id).await? {
        let pairing = source.secret.as_deref().and_then(|s| crate::cloud::decrypt(&state.cloud_key, s)).and_then(|s| serde_json::from_str::<crate::homekit::Pairing>(&s).ok());
        let host: Value = serde_json::from_str(&source.config).unwrap_or(Value::Null);
        if let (Some(pairing), Some(host)) = (pairing, host["host"].as_str()) {
            let removed = tokio::time::timeout(std::time::Duration::from_secs(15), crate::homekit::remove_own_pairing(host, &pairing)).await;
            if !matches!(removed, Ok(Ok(()))) {
                tracing::warn!("cannot remove Breezone's pairing from {host}: {removed:?}; the accessory may still list it");
            }
        }
    }
    if !db::delete_source(&state.db, id).await? {
        return Err(ApiError::not_found("source not found"));
    }
    state.drivers.reload().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct PairInput {
    /// "ip:port", as the scanner found it.
    pub host: String,
    pub name: String,
    /// The accessory's HomeKit setup code (8 digits).
    pub code: String,
    /// Feature flags it announces (1: Apple authentication).
    #[serde(default)]
    pub flags: u32,
}

#[derive(Deserialize)]
pub struct StartInput {
    pub host: String,
    #[serde(default)]
    pub flags: u32,
}

/// Pairings started, by accessory address, until their code is typed.
static STARTED: std::sync::LazyLock<tokio::sync::Mutex<std::collections::HashMap<String, (std::time::Instant, crate::homekit::PairSetup)>>> =
    std::sync::LazyLock::new(Default::default);
const STARTED_FOR: std::time::Duration = std::time::Duration::from_secs(120);

fn accessory_host(host: &str) -> ApiResult<&str> {
    let host = host.trim();
    if host.is_empty() || !host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':')) {
        return Err(ApiError::bad_request("invalid accessory address"));
    }
    Ok(host)
}

/// Starts pairing with a HomeKit accessory: one with a screen shows its code now.
pub async fn start_homekit_pairing(_admin: AdminUser, Json(input): Json<StartInput>) -> ApiResult<StatusCode> {
    let host = accessory_host(&input.host)?;
    // A pairing left half-done keeps the accessory busy: the previous one is dropped first.
    STARTED.lock().await.retain(|h, (at, _)| h != host && at.elapsed() < STARTED_FOR);
    let setup = crate::homekit::PairSetup::start(host, input.flags & 1 == 1).await.map_err(|e| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, e.explain()))?;
    let at = std::time::Instant::now();
    STARTED.lock().await.insert(host.to_string(), (at, setup));
    // Never typed: the accessory is freed, else it refuses any other pairing.
    let host = host.to_string();
    tokio::spawn(async move {
        tokio::time::sleep(STARTED_FOR).await;
        let mut started = STARTED.lock().await;
        if started.get(&host).is_some_and(|(t, _)| *t == at) {
            started.remove(&host);
        }
    });
    Ok(StatusCode::NO_CONTENT)
}

/// Gives up a started pairing: the accessory is free again.
pub async fn cancel_homekit_pairing(_admin: AdminUser, Json(input): Json<StartInput>) -> StatusCode {
    STARTED.lock().await.remove(input.host.trim());
    StatusCode::NO_CONTENT
}

/// Pairs Breezone with a HomeKit accessory with its setup code (the pairing started for it, or a
/// new one), then adds it as a source. Nothing else changes on the accessory: what its own app set
/// up stays as it is.
pub async fn pair_homekit(State(state): State<AppState>, _admin: AdminUser, Json(input): Json<PairInput>) -> ApiResult<(StatusCode, Json<SourceView>)> {
    let code = crate::homekit::setup_code(&input.code).ok_or_else(|| {
        ApiError::bad_request(crate::text!(
            "Le code HomeKit a 8 chiffres (par exemple 123-45-678).",
            "The HomeKit code has 8 digits (for example 123-45-678).",
            "El código HomeKit tiene 8 cifras (por ejemplo 123-45-678)."
        ))
    })?;
    let host = accessory_host(&input.host)?;
    let name: String = input.name.trim().chars().take(60).collect();
    let started = STARTED.lock().await.remove(host).filter(|(at, _)| at.elapsed() < STARTED_FOR);
    let paired = match started {
        Some((_, setup)) => setup.finish(&code).await,
        None => crate::homekit::pair_setup(host, &code, input.flags & 1 == 1).await,
    };
    let pairing = paired.map_err(|e| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, e.explain()))?;
    let config = serde_json::json!({"host": host, "device_id": pairing.accessory_id}).to_string();
    let secret = crate::cloud::encrypt(&state.cloud_key, &serde_json::to_string(&pairing).unwrap_or_default());
    let stored = async {
        let id = db::insert_source(&state.db, "homekit", if name.is_empty() { "HomeKit" } else { &name }, &config).await?;
        db::set_source_secret(&state.db, id, Some(&secret)).await?;
        Ok::<i64, sqlx::Error>(id)
    }
    .await;
    match stored {
        Ok(id) => Ok((StatusCode::CREATED, Json(polled(&state, id).await?))),
        Err(e) => {
            // Paired but not kept: the accessory forgets Breezone, to be paired again.
            let _ = crate::homekit::remove_own_pairing(host, &pairing).await;
            Err(e.into())
        }
    }
}

#[derive(Deserialize)]
pub struct LinkInput {
    pub host: String,
    pub name: String,
}

/// Adds a Hue bridge: it gives Breezone an access key in the 30 seconds after its button is
/// pressed (as for the Hue app).
pub async fn link_hue(State(state): State<AppState>, _admin: AdminUser, Json(input): Json<LinkInput>) -> ApiResult<(StatusCode, Json<SourceView>)> {
    let host = input.host.trim();
    if host.is_empty() || !host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':')) {
        return Err(ApiError::bad_request("invalid bridge address"));
    }
    let (bridge, key) = crate::drivers::hue::link(host).await.map_err(|e| match e {
        crate::drivers::hue::LinkError::ButtonNotPressed => ApiError::new(
            StatusCode::CONFLICT,
            crate::text!(
                "Appuyez sur le bouton rond du pont Hue, puis cliquez sur « Ajouter » dans les 30 secondes.",
                "Press the round button of the Hue bridge, then click « Add » within 30 seconds.",
                "Pulse el botón redondo del puente Hue y haga clic en «Añadir» en 30 segundos."
            ),
        )
        .with_detail("code", "button"),
        crate::drivers::hue::LinkError::Other(m) => {
            ApiError::new(StatusCode::BAD_GATEWAY, crate::text!("Le pont Hue ne répond pas : {0}", "The Hue bridge does not answer: {0}", "El puente Hue no responde: {0}", m))
        }
    })?;
    let name: String = input.name.trim().chars().take(60).collect();
    let config = serde_json::json!({"host": host, "device_id": bridge.id}).to_string();
    let id = db::insert_source(&state.db, "hue", if name.is_empty() { "Philips Hue" } else { &name }, &config).await?;
    db::set_source_secret(&state.db, id, Some(&crate::cloud::encrypt(&state.cloud_key, &key))).await?;
    Ok((StatusCode::CREATED, Json(polled(&state, id).await?)))
}

/// Reads the source now (« Tester » / « Actualiser »).
pub async fn refresh(State(state): State<AppState>, _admin: AdminUser, Path(id): Path<i64>) -> ApiResult<Json<SourceView>> {
    db::source_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("source not found"))?;
    Ok(Json(polled(&state, id).await?))
}

#[derive(Serialize)]
pub struct LocateView {
    #[serde(flatten)]
    pub source: SourceView,
    /// Found at another address, now used.
    pub moved: bool,
}

/// Looks for the source's hardware on the network by its identity (« Rechercher »), and follows it
/// if its address changed.
pub async fn locate(State(state): State<AppState>, _admin: AdminUser, Path(id): Path<i64>) -> ApiResult<Json<LocateView>> {
    db::source_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("source not found"))?;
    let moved = state.drivers.locate(id, state.cfg.scan_subnet.as_deref(), state.cfg.scan_port).await.map_err(ApiError::bad_request)?;
    Ok(Json(LocateView { source: polled(&state, id).await?, moved }))
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

fn driver_error(e: crate::drivers::DriverError) -> ApiError {
    match e {
        crate::drivers::DriverError::Unreachable(m) => ApiError::bad_gateway(m),
        crate::drivers::DriverError::Unsupported(m) => ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, m).with_detail("code", "unsupported"),
        crate::drivers::DriverError::Invalid(m) => ApiError::bad_request(m),
    }
}

#[derive(Debug, Serialize)]
pub struct SceneView {
    pub source_id: i64,
    pub id: String,
    pub name: String,
    /// The room of the module's app it belongs to.
    pub room: Option<String>,
    /// The devices it sets.
    pub device_ids: Vec<i64>,
}

/// The scenes of the modules that have some (Hue), with the devices they set. A module that does
/// not answer is left out.
pub async fn scenes(State(state): State<AppState>, _user: crate::auth::AuthUser) -> ApiResult<Json<Vec<SceneView>>> {
    let ids = db::source_device_ids(&state.db).await?;
    let mut out = vec![];
    for source in db::list_sources(&state.db).await?.into_iter().filter(|s| s.enabled) {
        let Some(driver) = state.drivers.driver(source.id).await else { continue };
        let Ok(scenes) = driver.scenes().await else { continue };
        out.extend(scenes.into_iter().map(|s| SceneView {
            source_id: source.id,
            device_ids: s.references.iter().filter_map(|r| ids.get(&format!("{}/{r}", source.id)).copied()).collect(),
            id: s.id,
            name: s.name,
            room: s.room,
        }));
    }
    Ok(Json(out))
}

/// Activates a scene of a module (administrators, as for the other devices of modules).
pub async fn activate_scene(State(state): State<AppState>, _admin: AdminUser, Path((source_id, scene)): Path<(i64, String)>) -> ApiResult<StatusCode> {
    state.drivers.activate_scene(source_id, &scene).await.map_err(driver_error)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Serialize)]
pub struct RoomsTaken {
    /// Rooms created in Breezone.
    pub created: Vec<String>,
    /// Devices put in a room (those already in one stay where they are).
    pub placed: usize,
}

/// Takes the rooms of the module's own app (Hue): the missing ones are created, by name, and the
/// devices not in a room yet go in theirs.
pub async fn take_rooms(State(state): State<AppState>, _admin: AdminUser, Path(id): Path<i64>) -> ApiResult<Json<RoomsTaken>> {
    let driver = state.drivers.driver(id).await.ok_or_else(|| ApiError::not_found("source not found"))?;
    let rooms = driver.rooms().await.map_err(driver_error)?;
    let ids = db::source_device_ids(&state.db).await?;
    let mut existing = db::list_rooms(&state.db).await?;
    let mut taken = RoomsTaken { created: vec![], placed: 0 };
    for room in rooms {
        let mut devices = vec![];
        for r in &room.references {
            if let Some(device) = match ids.get(&format!("{id}/{r}")) {
                Some(d) => db::device_by_id(&state.db, *d).await?,
                None => None,
            } {
                devices.push(device);
            }
        }
        if devices.is_empty() {
            continue;
        }
        let name = room.name.trim();
        let room_id = match existing.iter().find(|e| e.name.trim().to_lowercase() == name.to_lowercase()) {
            Some(e) => e.id,
            None => {
                let created = db::insert_room(&state.db, name, existing.len() as i64).await?;
                taken.created.push(created.name.clone());
                let id = created.id;
                existing.push(created);
                id
            }
        };
        for d in devices.into_iter().filter(|d| d.room_id.is_none()) {
            db::update_device(&state.db, d.id, d.name.as_deref(), Some(room_id), d.position).await?;
            taken.placed += 1;
        }
    }
    Ok(Json(taken))
}
