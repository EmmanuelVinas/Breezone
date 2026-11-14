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
    /// Driver that provides it ("airzone").
    pub source: String,
    pub installation_id: Option<i64>,
    /// Where to find it in its source (Airzone: `{system_id, zone_id}`).
    pub reference: Value,
    /// Custom name, else the one given by the source.
    pub name: String,
    pub room_id: Option<i64>,
    pub position: i64,
    /// The source currently provides it (a zone removed from its installation is not).
    pub available: bool,
    /// Values of its traits (fields described in the catalog).
    pub state: Value,
}

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
async fn devices_of(state: &AppState) -> ApiResult<Vec<DeviceView>> {
    let mut zones: HashMap<String, ZoneView> = HashMap::new();
    for inst in state.hub.snapshot().await {
        for z in inst.zones {
            let reference = airzone_ref(inst.id, z.system_id, z.zone_id);
            db::ensure_device(&state.db, "airzone", &reference, Some(inst.id), "climate.zone").await?;
            zones.insert(reference, z);
        }
    }
    Ok(db::list_devices(&state.db)
        .await?
        .into_iter()
        .map(|d| {
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
                installation_id: d.installation_id,
                reference,
                room_id: d.room_id,
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

#[derive(Deserialize)]
pub struct DeviceInput {
    /// Custom name (empty or absent: the source's).
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub room_id: Option<i64>,
    #[serde(default)]
    pub position: i64,
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
    if let Some(room) = input.room_id {
        db::room_by_id(&state.db, room).await?.ok_or_else(|| ApiError::bad_request("unknown room"))?;
    }
    let name: Option<String> = input.name.as_deref().map(str::trim).filter(|n| !n.is_empty()).map(|n| n.chars().take(60).collect());
    db::update_device(&state.db, id, name.as_deref(), input.room_id, input.position).await?;
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
