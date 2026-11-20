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

/// The password fields of a driver's form, taken out of its configuration: they go to the
/// source's secret, encrypted (an empty one is not given: the one kept stays).
fn split_secrets(driver: &str, config: &Value) -> (Value, serde_json::Map<String, Value>) {
    let fields: Vec<&str> =
        crate::drivers::available().into_iter().find(|d| d.id == driver).map(|d| d.config.iter().filter(|f| f.kind == "password").map(|f| f.name).collect()).unwrap_or_default();
    let mut config = config.clone();
    let mut secrets = serde_json::Map::new();
    if let Some(o) = config.as_object_mut() {
        for f in fields {
            if let Some(v) = o.remove(f).filter(|v| v.as_str().is_some_and(|s| !s.is_empty())) {
                secrets.insert(f.into(), v);
            }
        }
    }
    (config, secrets)
}

/// A forecast of the sun is for a place: the home's (its city) when none is given, in the time
/// zone of this server.
async fn placed(state: &AppState, driver: &str, mut config: Value) -> Value {
    if driver != "forecast_solar" {
        return config;
    }
    if let Some(o) = config.as_object_mut() {
        let missing = |k: &str| o.get(k).is_none_or(|v| v.is_null() || v.as_str().is_some_and(|s| s.trim().is_empty()));
        if missing("latitude") || missing("longitude") {
            if let Some(p) = state.weather.place(None).await {
                o.insert("latitude".into(), Value::from(p.lat));
                o.insert("longitude".into(), Value::from(p.lon));
            }
        }
        o.insert("timezone".into(), Value::from(state.cfg.timezone.name()));
    }
    config
}

/// The secret of a source with the passwords given put in (the others kept).
fn merged_secret(state: &AppState, current: Option<&str>, given: &serde_json::Map<String, Value>) -> Option<String> {
    let mut secret: serde_json::Map<String, Value> =
        current.and_then(|s| crate::crypto::decrypt(&state.data_key, s)).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    secret.extend(given.clone());
    (!secret.is_empty()).then(|| Value::Object(secret).to_string())
}

/// Checks the driver and its configuration (with its secret); returns the trimmed name and the
/// configuration text.
fn checked(input: &SourceInput, secret: Option<&str>) -> ApiResult<(String, String)> {
    // The Netatmo and EZVIZ modules come with their account: only their name and whether they run
    // change here.
    if !crate::accounts::MODULES.contains(&input.driver.as_str()) {
        crate::drivers::create(&input.driver, &input.config, secret).map_err(ApiError::bad_request)?;
    }
    let name: String = input.name.trim().chars().take(60).collect();
    if name.is_empty() {
        return Err(ApiError::bad_request(crate::text!("le nom est vide", "the name is empty", "el nombre está vacío")));
    }
    Ok((name, input.config.to_string()))
}

/// The configuration given, with the choices made on its devices (links, own, ignored) kept when
/// it does not say them: the form of a module only knows its address.
fn with_choices(config: &str, current: &str) -> String {
    let (Ok(Value::Object(mut next)), Ok(Value::Object(before))) = (serde_json::from_str(config), serde_json::from_str::<Value>(current)) else { return config.to_string() };
    for key in ["links", "own", "ignored"] {
        if let (false, Some(v)) = (next.contains_key(key), before.get(key)) {
            next.insert(key.into(), v.clone());
        }
    }
    Value::Object(next).to_string()
}

/// Reads the source at once, so that the answer already tells whether it answers.
async fn polled(state: &AppState, id: i64) -> ApiResult<SourceView> {
    state.drivers.reload().await?;
    state.drivers.poll(id).await;
    let s = db::source_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("source not found"))?;
    Ok(view(state, s).await)
}

pub async fn create(State(state): State<AppState>, _admin: AdminUser, Json(input): Json<SourceInput>) -> ApiResult<(StatusCode, Json<SourceView>)> {
    if crate::accounts::MODULES.contains(&input.driver.as_str()) || crate::accounts::SERVICE_MODULES.contains(&input.driver.as_str()) {
        return Err(ApiError::bad_request("ce module s'ajoute en connectant son compte (Réglages › Comptes et services)"));
    }
    let input = SourceInput { config: placed(&state, &input.driver, input.config.clone()).await, ..input };
    let (config, given) = split_secrets(&input.driver, &input.config);
    let secret = merged_secret(&state, None, &given);
    let driver = input.driver.clone();
    let (name, config) = checked(&SourceInput { config, ..input }, secret.as_deref())?;
    let id = db::insert_source(&state.db, &driver, &name, &config).await?;
    if let Some(s) = secret {
        db::set_source_secret(&state.db, id, Some(&crate::crypto::encrypt(&state.data_key, &s))).await?;
    }
    Ok((StatusCode::CREATED, Json(polled(&state, id).await?)))
}

pub async fn update(State(state): State<AppState>, _admin: AdminUser, Path(id): Path<i64>, Json(input): Json<SourceInput>) -> ApiResult<Json<SourceView>> {
    let current = db::source_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("source not found"))?;
    if current.driver != input.driver {
        return Err(ApiError::bad_request("the driver of a source cannot change"));
    }
    let input = SourceInput { config: placed(&state, &input.driver, input.config.clone()).await, ..input };
    let (config, given) = split_secrets(&input.driver, &input.config);
    let secret = if given.is_empty() { None } else { merged_secret(&state, current.secret.as_deref(), &given) };
    let kept = current.secret.as_deref().and_then(|s| crate::crypto::decrypt(&state.data_key, s));
    let (name, config) = checked(&SourceInput { config, ..input }, secret.as_deref().or(kept.as_deref()))?;
    let config = with_choices(&config, &current.config);
    db::update_source(&state.db, id, &name, &config, input.enabled).await?;
    if let Some(s) = secret {
        db::set_source_secret(&state.db, id, Some(&crate::crypto::encrypt(&state.data_key, &s))).await?;
    }
    Ok(Json(polled(&state, id).await?))
}

/// Removes the source and its devices. For a HomeKit accessory, Breezone first removes its own
/// pairing from it (and nothing else), so that it can be paired again.
pub async fn delete(State(state): State<AppState>, _admin: AdminUser, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    if let Some(source) = db::source_by_id(&state.db, id).await? {
        let pairing = source.secret.as_deref().and_then(|s| crate::crypto::decrypt(&state.data_key, s)).and_then(|s| serde_json::from_str::<crate::homekit::Pairing>(&s).ok());
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
    let secret = crate::crypto::encrypt(&state.data_key, &serde_json::to_string(&pairing).unwrap_or_default());
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
    db::set_source_secret(&state.db, id, Some(&crate::crypto::encrypt(&state.data_key, &key))).await?;
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
    #[serde(flatten)]
    pub located: crate::drivers::Located,
}

/// Looks for the source's hardware on the network by its identity (« Rechercher »), and follows it
/// if its address changed.
pub async fn locate(State(state): State<AppState>, _admin: AdminUser, Path(id): Path<i64>) -> ApiResult<Json<LocateView>> {
    db::source_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("source not found"))?;
    let located = state.drivers.locate(id, state.cfg.scan_subnet.as_deref(), state.cfg.scan_port).await.map_err(ApiError::bad_request)?;
    Ok(Json(LocateView { source: polled(&state, id).await?, located }))
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
    let products = crate::discovery::scan_products(&subnet, state.cfg.scan_port, &state.drivers.quiet_hosts().await).await.map_err(ApiError::bad_request)?;
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

#[derive(Deserialize)]
pub struct KeyInput {
    pub key: String,
}

#[derive(Serialize)]
pub struct KeyView {
    /// The hardware accepts the key: its extra data shows from now on.
    pub accepted: bool,
}

/// Gives a module the key of its maker that unlocks more data (the measures of each room of a
/// Renson Healthbox). The hardware keeps the key; it is not stored nor logged here.
pub async fn unlock(State(state): State<AppState>, _admin: AdminUser, Path(id): Path<i64>, Json(input): Json<KeyInput>) -> ApiResult<Json<KeyView>> {
    let key = input.key.trim();
    if key.is_empty() || key.len() > 200 {
        return Err(ApiError::bad_request("clé invalide"));
    }
    let driver = state.drivers.driver(id).await.ok_or_else(|| ApiError::not_found("source not found"))?;
    let accepted = driver.unlock(key).await.map_err(driver_error)?;
    if accepted {
        state.drivers.poll(id).await;
    }
    Ok(Json(KeyView { accepted }))
}

/// What a module's hardware says about itself, raw (the database of a HomeKit accessory…), with
/// what Breezone reads of it: to understand how a model behaves. No secret is in it.
pub async fn diagnostic(State(state): State<AppState>, _admin: AdminUser, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let source = db::source_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("source not found"))?;
    let driver = state.drivers.driver(id).await.ok_or_else(|| ApiError::not_found("source not configured"))?;
    let hardware = driver.diagnostic().await.map_err(driver_error)?;
    let read = driver.read().await.map_err(driver_error)?;
    let described = driver.describe().await.map_err(driver_error)?;
    Ok(Json(serde_json::json!({
        "breezone": env!("CARGO_PKG_VERSION"),
        "driver": source.driver,
        "model": described.model,
        "firmware": described.firmware,
        "devices": described.devices,
        "states": read,
        "hardware": hardware,
    })))
}

/// A device of a local module a device of the cloud can be the twin of.
#[derive(Serialize)]
pub struct LocalChoice {
    pub source_id: i64,
    pub reference: String,
    /// Its name in Breezone.
    pub name: String,
    pub kind: String,
    /// Its module.
    pub source: String,
}

#[derive(Serialize)]
pub struct TwinsView {
    pub devices: Vec<crate::drivers::CloudDevice>,
    pub locals: Vec<LocalChoice>,
}

/// What became of each device of a module (its own, the twin of a local one, ignored) with its
/// last reading, and for a cloud module the local devices it may be linked to.
pub async fn twins(State(state): State<AppState>, _admin: AdminUser, Path(id): Path<i64>) -> ApiResult<Json<TwinsView>> {
    let source = db::source_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("source not found"))?;
    if !crate::accounts::MODULES.contains(&source.driver.as_str()) {
        return Ok(Json(TwinsView { devices: state.drivers.cloud_view(id).await, locals: vec![] }));
    }
    let names: std::collections::HashMap<String, String> = db::list_devices(&state.db).await?.into_iter().filter_map(|d| Some((d.source_ref, d.name?))).collect();
    let sources: std::collections::HashMap<i64, String> = db::list_sources(&state.db).await?.into_iter().map(|s| (s.id, s.name)).collect();
    let locals = state
        .drivers
        .devices()
        .await
        .into_iter()
        .filter(|d| d.source_id != id && !crate::accounts::MODULES.contains(&d.driver.as_str()))
        .map(|d| LocalChoice {
            name: names.get(&format!("{}/{}", d.source_id, d.info.reference)).cloned().unwrap_or_else(|| d.info.name.clone()),
            source: sources.get(&d.source_id).cloned().unwrap_or_default(),
            source_id: d.source_id,
            reference: d.info.reference,
            kind: d.info.kind,
        })
        .collect();
    Ok(Json(TwinsView { devices: state.drivers.cloud_view(id).await, locals }))
}

#[derive(Deserialize)]
pub struct TwinInput {
    /// The device of the cloud module.
    pub reference: String,
    /// "auto" (found by its name) | "link" | "own" | "ignore"
    pub choice: String,
    /// For "link": the local device it is.
    pub source_id: Option<i64>,
    pub local: Option<String>,
}

/// Chooses what a device of a module is: ignored or not; for a cloud module, also the twin of a
/// local device, a device of its own, or back to what its name finds.
pub async fn set_twin(State(state): State<AppState>, _admin: AdminUser, Path(id): Path<i64>, Json(input): Json<TwinInput>) -> ApiResult<Json<TwinsView>> {
    let source = db::source_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("source not found"))?;
    if !crate::accounts::MODULES.contains(&source.driver.as_str()) && !matches!(input.choice.as_str(), "auto" | "ignore") {
        return Err(ApiError::bad_request("choice: auto or ignore"));
    }
    let mut config: serde_json::Map<String, Value> = serde_json::from_str(&source.config).unwrap_or_default();
    let r = input.reference.as_str();
    let list = |config: &mut serde_json::Map<String, Value>, key: &str, add: bool| {
        let mut items: Vec<Value> = config.get(key).and_then(Value::as_array).cloned().unwrap_or_default();
        items.retain(|v| v.as_str() != Some(r));
        if add {
            items.push(Value::from(r));
        }
        config.insert(key.into(), Value::Array(items));
    };
    let mut links = config.get("links").and_then(Value::as_object).cloned().unwrap_or_default();
    links.remove(r);
    list(&mut config, "own", input.choice == "own");
    list(&mut config, "ignored", input.choice == "ignore");
    match input.choice.as_str() {
        "link" => {
            let (Some(source_id), Some(local)) = (input.source_id, input.local.clone()) else {
                return Err(ApiError::bad_request("l'appareil local à relier est requis"));
            };
            links.insert(r.into(), serde_json::json!({ "source_id": source_id, "reference": local }));
        }
        "auto" | "own" | "ignore" => {}
        _ => return Err(ApiError::bad_request("choice: auto, link, own or ignore")),
    }
    config.insert("links".into(), Value::Object(links));
    db::update_source(&state.db, id, &source.name, &Value::Object(config).to_string(), source.enabled).await?;
    state.drivers.reload().await?;
    state.drivers.poll(id).await;
    twins(State(state), AdminUser(_admin.0), Path(id)).await
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_form_of_a_module_keeps_the_choices_made_on_its_devices() {
        let kept = super::with_choices(r#"{"host":"10.0.0.2"}"#, r#"{"host":"10.0.0.1","ignored":["gas"]}"#);
        assert_eq!(serde_json::from_str::<serde_json::Value>(&kept).unwrap(), serde_json::json!({"host": "10.0.0.2", "ignored": ["gas"]}));
    }
}
