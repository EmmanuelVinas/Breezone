//! The home model (ADR 0013): rooms, and devices described by traits. Airzone zones are devices of
//! the trait `climate.zone@1`, recorded automatically with a stable id; the zone routes stay the
//! reference to drive them for now.

use crate::access::Access;
use crate::auth::{AdminUser, AuthUser};
use crate::db::{self, Room};
use crate::error::{ApiError, ApiResult};
use crate::hub::ZoneView;
use crate::traits::Trait;
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

/// The catalog of traits: what each kind of device holds and accepts (descriptors, ADR 0012).
pub async fn traits(_user: AuthUser) -> Json<&'static [Trait]> {
    Json(crate::traits::catalog())
}

#[derive(Serialize)]
pub struct DeviceView {
    pub id: i64,
    /// Main trait ("climate.zone").
    pub kind: String,
    /// `trait@version` the device has.
    pub traits: Vec<String>,
    /// Driver that provides it ("airzone", "shelly").
    pub source: String,
    /// Configured source (drivers other than Airzone).
    pub source_id: Option<i64>,
    pub installation_id: Option<i64>,
    /// Where to find it in its source (Airzone: `{system_id, zone_id}`).
    pub reference: Value,
    /// Custom name, else the one given by the source.
    pub name: String,
    pub room_id: Option<i64>,
    /// For an energy measure: what it stands for ("grid", "solar", "battery", "consumption").
    pub role: Option<String>,
    /// What a "consumption" measure measures.
    pub measures: Measures,
    pub position: i64,
    /// The source currently provides it (a zone removed from its installation is not).
    pub available: bool,
    /// Values of its traits (fields described in the catalog).
    pub state: Value,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Measures {
    pub device_id: Option<i64>,
    pub room_id: Option<i64>,
}

/// Roles of an energy measure.
pub const ROLES: &[&str] = &["grid", "solar", "battery", "consumption"];

fn airzone_ref(installation_id: i64, system_id: i64, zone_id: i64) -> String {
    format!("{installation_id}/{system_id}/{zone_id}")
}

/// State of a zone as the trait `climate.zone@1` describes it.
fn climate_state(z: &ZoneView) -> Value {
    json!({
        "on": z.on,
        "room_temp": z.room_temp,
        "humidity": z.humidity,
        "setpoint": z.setpoint,
        "cool_setpoint": z.cool_setpoint,
        "heat_setpoint": z.heat_setpoint,
        "mode": z.mode,
        "speed": z.speed,
        "sleep": z.sleep,
        "demand": z.demand,
        "air_quality": z.air_quality.as_ref().and_then(|a| a.level),
    })
}

/// Records the zones as devices (new ones only), then lists every device with its state.
pub(crate) async fn devices_of(state: &AppState) -> ApiResult<Vec<DeviceView>> {
    let mut zones: HashMap<String, ZoneView> = HashMap::new();
    for inst in state.hub.snapshot().await {
        for z in inst.zones {
            let reference = airzone_ref(inst.id, z.system_id, z.zone_id);
            db::ensure_device(&state.db, "airzone", &reference, Some(inst.id), "climate.zone").await?;
            zones.insert(reference, z);
        }
    }
    let from_sources: HashMap<String, crate::drivers::SourceDevice> =
        state.drivers.devices().await.into_iter().map(|d| (format!("{}/{}", d.source_id, d.info.reference), d)).collect();
    Ok(db::list_devices(&state.db)
        .await?
        .into_iter()
        .map(|d| {
            let role = d.role.clone();
            let measures = Measures { device_id: d.measures_device_id, room_id: d.measures_room_id };
            if let Some(sd) = d.source_id.and(from_sources.get(&d.source_ref)) {
                return DeviceView {
                    id: d.id,
                    kind: d.kind,
                    traits: sd.info.traits.clone(),
                    source: d.source,
                    source_id: d.source_id,
                    installation_id: None,
                    reference: json!(sd.info.reference),
                    name: d.name.unwrap_or_else(|| sd.info.name.clone()),
                    room_id: d.room_id,
                    role: role.clone(),
                    measures: measures.clone(),
                    position: d.position,
                    available: sd.online,
                    state: sd.state.clone().map(Value::Object).unwrap_or(Value::Null),
                };
            }
            if d.source_id.is_some() {
                // Its source no longer lists it (or is gone): kept, unavailable.
                return DeviceView {
                    traits: vec![format!("{}@1", d.kind)],
                    name: d.name.clone().unwrap_or_else(|| d.source_ref.clone()),
                    reference: json!(d.source_ref.split_once('/').map(|(_, r)| r).unwrap_or(&d.source_ref)),
                    id: d.id,
                    kind: d.kind,
                    source: d.source,
                    source_id: d.source_id,
                    installation_id: None,
                    room_id: d.room_id,
                    role: role.clone(),
                    measures: measures.clone(),
                    position: d.position,
                    available: false,
                    state: Value::Null,
                };
            }
            let zone = (d.source == "airzone").then(|| zones.get(&d.source_ref)).flatten();
            let reference = match d.source_ref.split('/').collect::<Vec<_>>()[..] {
                [_, system, zone] => json!({ "system_id": system.parse::<i64>().ok(), "zone_id": zone.parse::<i64>().ok() }),
                _ => json!(d.source_ref),
            };
            DeviceView {
                traits: vec![format!("{}@1", d.kind)],
                name: d.name.clone().or_else(|| zone.map(|z| z.name.clone())).unwrap_or_else(|| d.source_ref.clone()),
                available: zone.is_some(),
                state: zone.map(climate_state).unwrap_or(Value::Null),
                id: d.id,
                kind: d.kind,
                source: d.source,
                source_id: None,
                installation_id: d.installation_id,
                reference,
                room_id: d.room_id,
                    role: role.clone(),
                    measures: measures.clone(),
                position: d.position,
            }
        })
        .collect())
}

/// The devices the user can see.
pub async fn list_devices(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<DeviceView>>> {
    let access = Access::of(&state.db, &user).await?;
    let mut devices = devices_of(&state).await?;
    devices.retain(|d| d.installation_id.is_none_or(|i| access.can_view(i)));
    Ok(Json(devices))
}

/// A field that may be absent (unchanged), `null` (cleared) or set.
fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<Option<T>>, D::Error> {
    Ok(Some(Option::deserialize(d)?))
}

/// Only the fields given change.
#[derive(Deserialize)]
pub struct DeviceInput {
    /// Custom name (empty or null: the source's).
    #[serde(default, deserialize_with = "present")]
    pub name: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    pub room_id: Option<Option<i64>>,
    #[serde(default)]
    pub position: Option<i64>,
    /// Energy measures only: "grid" | "solar" | "battery" | "consumption" (null: a plain measure).
    #[serde(default, deserialize_with = "present")]
    pub role: Option<Option<String>>,
    /// For "consumption": the device or the room measured.
    #[serde(default, deserialize_with = "present")]
    pub measures_device_id: Option<Option<i64>>,
    #[serde(default, deserialize_with = "present")]
    pub measures_room_id: Option<Option<i64>>,
}

/// Names a device and puts it in a room: like renaming a zone, it needs the right to edit the zones
/// of its installation.
pub async fn update_device(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>, Json(input): Json<DeviceInput>) -> ApiResult<Json<DeviceView>> {
    let device = db::device_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("device not found"))?;
    let access = Access::of(&state.db, &user).await?;
    match device.installation_id {
        Some(i) => access.edit_zones(i)?,
        None if !access.is_admin() => return Err(ApiError::forbidden("administrators only")),
        None => {}
    }
    let room_id = input.room_id.unwrap_or(device.room_id);
    if let Some(room) = room_id {
        db::room_by_id(&state.db, room).await?.ok_or_else(|| ApiError::bad_request("unknown room"))?;
    }
    let name: Option<String> = match &input.name {
        Some(n) => n.as_deref().map(str::trim).filter(|n| !n.is_empty()).map(|n| n.chars().take(60).collect()),
        None => device.name.clone(),
    };
    let role_input = input.role.clone().unwrap_or(device.role.clone());
    let role = role_input.as_deref().filter(|r| !r.is_empty());
    if let Some(r) = role {
        if !ROLES.contains(&r) {
            return Err(ApiError::bad_request(format!("unknown role: {r}")));
        }
        let measures_energy = devices_of(&state).await?.into_iter().find(|d| d.id == id).is_some_and(|d| d.traits.iter().any(|t| t.starts_with("energy.power@")));
        if !measures_energy {
            return Err(ApiError::bad_request("only an energy measure has a role"));
        }
    }
    let consumption = role == Some("consumption");
    let measured_device = input.measures_device_id.unwrap_or(device.measures_device_id).filter(|_| consumption);
    let measured_room = input.measures_room_id.unwrap_or(device.measures_room_id).filter(|_| consumption);
    if measured_device == Some(id) {
        return Err(ApiError::bad_request("a measure cannot measure itself"));
    }
    if let Some(d) = measured_device {
        db::device_by_id(&state.db, d).await?.ok_or_else(|| ApiError::bad_request("unknown device"))?;
    }
    if let Some(r) = measured_room {
        db::room_by_id(&state.db, r).await?.ok_or_else(|| ApiError::bad_request("unknown room"))?;
    }
    db::update_device(&state.db, id, name.as_deref(), room_id, input.position.unwrap_or(device.position)).await?;
    db::set_device_role(&state.db, id, role, measured_device, measured_room).await?;
    devices_of(&state).await?.into_iter().find(|d| d.id == id).map(Json).ok_or_else(|| ApiError::not_found("device not found"))
}

pub async fn list_rooms(State(state): State<AppState>, _user: AuthUser) -> ApiResult<Json<Vec<Room>>> {
    Ok(Json(db::list_rooms(&state.db).await?))
}

#[derive(Deserialize)]
pub struct RoomInput {
    pub name: String,
    #[serde(default)]
    pub position: i64,
}

fn room_name(input: &RoomInput) -> ApiResult<String> {
    let name: String = input.name.trim().chars().take(60).collect();
    if name.is_empty() {
        return Err(ApiError::bad_request(crate::text!("le nom de la pièce est vide", "the room name is empty", "el nombre de la habitación está vacío")));
    }
    Ok(name)
}

pub async fn create_room(State(state): State<AppState>, _admin: AdminUser, Json(input): Json<RoomInput>) -> ApiResult<(StatusCode, Json<Room>)> {
    let room = db::insert_room(&state.db, &room_name(&input)?, input.position).await?;
    Ok((StatusCode::CREATED, Json(room)))
}

pub async fn update_room(State(state): State<AppState>, _admin: AdminUser, Path(id): Path<i64>, Json(input): Json<RoomInput>) -> ApiResult<Json<Room>> {
    if !db::update_room(&state.db, id, &room_name(&input)?, input.position).await? {
        return Err(ApiError::not_found("room not found"));
    }
    db::room_by_id(&state.db, id).await?.map(Json).ok_or_else(|| ApiError::not_found("room not found"))
}

/// Its devices stay, without a room.
pub async fn delete_room(State(state): State<AppState>, _admin: AdminUser, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    if !db::delete_room(&state.db, id).await? {
        return Err(ApiError::not_found("room not found"));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct ActionInput {
    #[serde(rename = "trait")]
    pub trait_name: String,
    pub action: String,
    #[serde(default)]
    pub params: serde_json::Map<String, Value>,
}

/// Checks parameters against the catalog: an unknown one is refused (422, ADR 0012), a missing or
/// mistyped one too (400).
fn check_params(action: &crate::traits::Action, params: &serde_json::Map<String, Value>) -> ApiResult<()> {
    let unknown: Vec<String> = params.keys().filter(|k| !action.params.iter().any(|p| &p.name == *k)).cloned().collect();
    if !unknown.is_empty() {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            crate::text!(
                "Ce serveur (Breezone {0}) ne sait pas appliquer : {1}. Mettez-le à jour.",
                "This server (Breezone {0}) cannot apply: {1}. Update it.",
                "Este servidor (Breezone {0}) no sabe aplicar: {1}. Actualícelo.",
                env!("CARGO_PKG_VERSION"),
                unknown.join(", ")
            ),
        )
        .with_detail("code", "unsupported")
        .with_detail("unsupported", &unknown));
    }
    for p in &action.params {
        match params.get(&p.name) {
            None | Some(Value::Null) if !p.optional => return Err(ApiError::bad_request(format!("`{}` is required", p.name))),
            None | Some(Value::Null) => {}
            Some(v) => {
                let ok = match p.kind.as_str() {
                    "bool" => v.is_boolean(),
                    "integer" => v.is_i64() || v.is_u64(),
                    "number" => v.is_number(),
                    "enum" => v.as_str().is_some_and(|s| p.values.iter().any(|x| x == s)),
                    _ => v.is_string(),
                };
                let in_range = v.as_f64().is_none_or(|x| p.min.is_none_or(|m| x >= m) && p.max.is_none_or(|m| x <= m));
                if !ok || !in_range {
                    return Err(ApiError::bad_request(format!("invalid `{}`", p.name)));
                }
            }
        }
    }
    Ok(())
}

/// Applies an action of one of the device's traits, whatever its source: the same route drives an
/// Airzone zone (`climate.zone.set`) and a Shelly relay (`switch.relay.set`).
pub async fn act(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>, Json(input): Json<ActionInput>) -> ApiResult<Json<DeviceView>> {
    let device = db::device_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("device not found"))?;
    let access = Access::of(&state.db, &user).await?;
    if let Some(i) = device.installation_id {
        access.view(i)?;
    }
    let view = devices_of(&state).await?.into_iter().find(|d| d.id == id).ok_or_else(|| ApiError::not_found("device not found"))?;
    let version = view
        .traits
        .iter()
        .find_map(|t| t.split_once('@').filter(|(n, _)| *n == input.trait_name).and_then(|(_, v)| v.parse::<u32>().ok()));
    let action = version
        .and_then(|v| crate::traits::get(&input.trait_name, v))
        .and_then(|t| t.action(&input.action))
        .ok_or_else(|| {
            ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, format!("{}.{} is not an action of this device", input.trait_name, input.action)).with_detail("code", "unsupported")
        })?;
    check_params(action, &input.params)?;
    match (device.source.as_str(), device.installation_id) {
        ("airzone", Some(inst)) => {
            access.control(inst)?;
            let (system_id, zone_id) = (view.reference["system_id"].as_i64(), view.reference["zone_id"].as_i64());
            let (Some(system_id), Some(zone_id)) = (system_id, zone_id) else {
                return Err(ApiError::not_found("zone not found"));
            };
            let cmd = crate::hub::ZoneCommand::from_value_strict(Value::Object(input.params))?;
            state.hub.control(inst, system_id, zone_id, &cmd, &user.username).await?;
        }
        (_, None) => {
            // Devices of other sources: administrators, until rights per type of device.
            if !access.is_admin() {
                return Err(ApiError::forbidden("administrators only"));
            }
            let source_id = device.source_id.ok_or_else(|| ApiError::not_found("device not found"))?;
            let reference = view.reference.as_str().unwrap_or_default().to_string();
            state.drivers.act(source_id, &reference, &input.trait_name, &input.action, &input.params).await.map_err(|e| match e {
                crate::drivers::DriverError::Unreachable(m) => ApiError::bad_gateway(m),
                crate::drivers::DriverError::Unsupported(m) => ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, m).with_detail("code", "unsupported"),
                crate::drivers::DriverError::Invalid(m) => ApiError::bad_request(m),
            })?;
        }
        _ => return Err(ApiError::not_found("device not found")),
    }
    devices_of(&state).await?.into_iter().find(|d| d.id == id).map(Json).ok_or_else(|| ApiError::not_found("device not found"))
}
