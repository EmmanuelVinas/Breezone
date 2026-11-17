//! The home model (ADR 0013): rooms, and devices described by traits. Airzone zones are devices of
//! the trait `climate.zone@1`, recorded automatically with a stable id; the zone routes stay the
//! reference to drive them for now.

use crate::access::Access;
use crate::auth::{AdminUser, AuthUser};
use crate::db::{self, Room};
use crate::devices::{self, ActionInput, DeviceView, ROLES};
use crate::error::{ApiError, ApiResult};
use crate::traits::Trait;
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

/// The catalog of traits: what each kind of device holds and accepts (descriptors, ADR 0012).
pub async fn traits(_user: AuthUser) -> Json<&'static [Trait]> {
    Json(crate::traits::catalog())
}

/// The devices the user can see, each saying whether they may drive it.
pub async fn list_devices(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<DeviceView>>> {
    let access = Access::of(&state.db, &user).await?;
    Ok(Json(devices::for_user(devices::list(&state).await?, &access)))
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
        let measures_energy = devices::get(&state, id).await?.is_some_and(|d| d.traits.iter().any(|t| t.starts_with("energy.power@")));
        if !measures_energy {
            return Err(ApiError::bad_request("only an energy measure has a role"));
        }
    }
    // A consumption measures a device or a room; a production may measure a solar inverter (a meter
    // on its line), so that both count once.
    let consumption = role == Some("consumption");
    let solar = role == Some("solar");
    let measured_device = input.measures_device_id.unwrap_or(device.measures_device_id).filter(|_| consumption || solar);
    let measured_room = input.measures_room_id.unwrap_or(device.measures_room_id).filter(|_| consumption);
    if measured_device == Some(id) {
        return Err(ApiError::bad_request("a measure cannot measure itself"));
    }
    if let Some(d) = measured_device {
        let measured = devices::get(&state, d).await?.ok_or_else(|| ApiError::bad_request("unknown device"))?;
        if solar && !measured.has_trait("solar.inverter@1") {
            return Err(ApiError::bad_request("a production measures a solar inverter"));
        }
    }
    if let Some(r) = measured_room {
        db::room_by_id(&state.db, r).await?.ok_or_else(|| ApiError::bad_request("unknown room"))?;
    }
    db::update_device(&state.db, id, name.as_deref(), room_id, input.position.unwrap_or(device.position)).await?;
    db::set_device_role(&state.db, id, role, measured_device, measured_room).await?;
    viewed_by(&state, &access, id).await
}

/// The device as this user sees it.
async fn viewed_by(state: &AppState, access: &Access, id: i64) -> ApiResult<Json<DeviceView>> {
    let device = devices::get(state, id).await?.ok_or_else(|| ApiError::not_found("device not found"))?;
    devices::for_user(vec![device], access).pop().map(Json).ok_or_else(|| ApiError::not_found("device not found"))
}

pub async fn list_rooms(State(state): State<AppState>, _user: AuthUser) -> ApiResult<Json<Vec<Room>>> {
    Ok(Json(db::list_rooms(&state.db).await?))
}

#[derive(Deserialize)]
pub struct RoomInput {
    pub name: String,
    #[serde(default)]
    pub position: i64,
    /// A Lucide name ("sofa"); empty: guessed from the name again. Unchanged when absent.
    #[serde(default)]
    pub icon: Option<String>,
}

/// The icon given, checked: `Some(None)` clears it.
fn room_icon(input: &RoomInput) -> ApiResult<Option<Option<&str>>> {
    match input.icon.as_deref().map(str::trim) {
        None => Ok(None),
        Some("") => Ok(Some(None)),
        Some(i) if i.len() <= 40 && i.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') => Ok(Some(Some(i))),
        Some(_) => Err(ApiError::bad_request("icon: a Lucide name expected")),
    }
}

fn room_name(input: &RoomInput) -> ApiResult<String> {
    let name: String = input.name.trim().chars().take(60).collect();
    if name.is_empty() {
        return Err(ApiError::bad_request(crate::text!("le nom de la pièce est vide", "the room name is empty", "el nombre de la habitación está vacío")));
    }
    Ok(name)
}

pub async fn create_room(State(state): State<AppState>, _admin: AdminUser, Json(input): Json<RoomInput>) -> ApiResult<(StatusCode, Json<Room>)> {
    let icon = room_icon(&input)?;
    let room = db::insert_room(&state.db, &room_name(&input)?, input.position).await?;
    if let Some(icon) = icon {
        db::set_room_icon(&state.db, room.id, icon).await?;
    }
    let room = db::room_by_id(&state.db, room.id).await?.unwrap_or(room);
    Ok((StatusCode::CREATED, Json(room)))
}

pub async fn update_room(State(state): State<AppState>, _admin: AdminUser, Path(id): Path<i64>, Json(input): Json<RoomInput>) -> ApiResult<Json<Room>> {
    let icon = room_icon(&input)?;
    if !db::update_room(&state.db, id, &room_name(&input)?, input.position).await? {
        return Err(ApiError::not_found("room not found"));
    }
    if let Some(icon) = icon {
        db::set_room_icon(&state.db, id, icon).await?;
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

/// Applies an action of one of the device's traits, whatever its source: the same route drives an
/// Airzone zone (`climate.zone.set`) and a Shelly relay (`switch.relay.set`).
pub async fn act(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>, Json(input): Json<ActionInput>) -> ApiResult<Json<DeviceView>> {
    let access = Access::of(&state.db, &user).await?;
    let view = devices::get(&state, id).await?.ok_or_else(|| ApiError::not_found("device not found"))?;
    match view.installation_id {
        Some(i) => access.view(i)?,
        // Not revealed to whom it was not given.
        None if !access.can_view_module(&devices::module_device(&view)) => return Err(ApiError::not_found("device not found")),
        None => {}
    }
    devices::check_action(&view, &input)?;
    match view.installation_id {
        Some(inst) => access.control(inst)?,
        // A device of a module: given to the user by itself, its room or its type.
        None => access.control_module(&devices::module_device(&view))?,
    }
    devices::apply(&state, &view, &input, &user.username).await?;
    viewed_by(&state, &access, id).await
}
