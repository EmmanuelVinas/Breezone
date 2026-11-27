//! The devices of the home (ADR 0013), whatever their source: an Airzone zone (trait
//! `climate.zone@1`) or a device of a module (a Shelly relay, a Hue light…). What the API, the
//! automations, the routines, the timers and the surplus steering read and drive.
//!
//! The zones are recorded as devices by the hub when it learns them ([`zone_refs`]); the devices of
//! the modules by their source when it is read. Listing them writes nothing.

use crate::db::{self, Db};
use crate::drivers::{DriverError, SourceDevice};
use crate::error::{ApiError, ApiResult};
use crate::hub::{InstallationView, ZoneView};
use crate::AppState;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
pub struct DeviceView {
    pub id: i64,
    /// Main trait ("climate.zone").
    pub kind: String,
    /// `trait@version` the device has.
    pub traits: Vec<String>,
    /// Driver that provides it: "airzone", or the id of a module.
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
    /// The user who asked may drive it (set by [`for_user`]; `true` inside the server).
    pub can_control: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Measures {
    pub device_id: Option<i64>,
    pub room_id: Option<i64>,
}

impl DeviceView {
    pub fn has_trait(&self, id: &str) -> bool {
        self.traits.iter().any(|t| t == id)
    }
}

/// Roles of an energy measure.
pub const ROLES: &[&str] = &["grid", "solar", "battery", "consumption"];

fn zone_ref(installation_id: i64, system_id: i64, zone_id: i64) -> String {
    format!("{installation_id}/{system_id}/{zone_id}")
}

/// The zones of an installation, as the hub records them as devices (`source_ref`, installation,
/// kind).
pub fn zone_refs(installation: &InstallationView) -> Vec<(String, Option<i64>, &'static str)> {
    installation.zones.iter().map(|z| (zone_ref(installation.id, z.system_id, z.zone_id), Some(installation.id), "climate.zone")).collect()
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

/// What a device is now, from its row and what its source says of it (the zone of its
/// installation, or the device of its module).
fn view(d: db::Device, zone: Option<&ZoneView>, from_source: Option<&SourceDevice>) -> DeviceView {
    let mut v = DeviceView {
        traits: vec![format!("{}@1", d.kind)],
        name: d.name.clone().unwrap_or_else(|| d.source_ref.clone()),
        reference: json!(d.source_ref),
        available: false,
        state: Value::Null,
        id: d.id,
        source_id: d.source_id,
        installation_id: None,
        room_id: d.room_id,
        role: d.role.clone(),
        measures: Measures { device_id: d.measures_device_id, room_id: d.measures_room_id },
        position: d.position,
        can_control: true,
        kind: d.kind,
        source: d.source,
    };
    match (d.source_id, from_source) {
        (Some(_), Some(sd)) => {
            v.traits = sd.info.traits.clone();
            v.reference = json!(sd.info.reference);
            v.name = d.name.unwrap_or_else(|| sd.info.name.clone());
            v.available = sd.online;
            v.state = sd.state.clone().map(Value::Object).unwrap_or(Value::Null);
        }
        // Its source no longer lists it (or is gone): kept, unavailable.
        (Some(_), None) => v.reference = json!(d.source_ref.split_once('/').map(|(_, r)| r).unwrap_or(&d.source_ref)),
        (None, _) => {
            v.installation_id = d.installation_id;
            if let [_, system, zone_id] = d.source_ref.split('/').collect::<Vec<_>>()[..] {
                v.reference = json!({ "system_id": system.parse::<i64>().ok(), "zone_id": zone_id.parse::<i64>().ok() });
            }
            if let Some(z) = zone {
                v.name = d.name.unwrap_or_else(|| z.name.clone());
                v.available = true;
                v.state = climate_state(z);
            }
        }
    }
    v
}

fn source_key(source_id: i64, reference: &str) -> String {
    format!("{source_id}/{reference}")
}

/// Every device with its state.
pub async fn list(state: &AppState) -> ApiResult<Vec<DeviceView>> {
    let zones: HashMap<String, ZoneView> =
        state.hub.snapshot().await.into_iter().flat_map(|i| i.zones.into_iter().map(move |z| (zone_ref(i.id, z.system_id, z.zone_id), z))).collect();
    let sources: HashMap<String, SourceDevice> = state.drivers.devices().await.into_iter().map(|d| (source_key(d.source_id, &d.info.reference), d)).collect();
    Ok(db::list_devices(&state.db)
        .await?
        .into_iter()
        .map(|d| {
            let (zone, from_source) = (zones.get(&d.source_ref), d.source_id.and(sources.get(&d.source_ref)));
            let zone = zone.filter(|_| d.source == "airzone");
            view(d, zone, from_source)
        })
        .collect())
}

/// One device with its state.
pub async fn get(state: &AppState, id: i64) -> ApiResult<Option<DeviceView>> {
    let Some(d) = db::device_by_id(&state.db, id).await? else { return Ok(None) };
    let from_source = match d.source_id {
        Some(_) => state.drivers.devices().await.into_iter().find(|s| source_key(s.source_id, &s.info.reference) == d.source_ref),
        None => None,
    };
    let parts: Vec<&str> = d.source_ref.split('/').collect();
    let zone = match (d.source.as_str(), d.installation_id, parts.as_slice()) {
        ("airzone", Some(inst), [_, system, zone_id]) => match (system.parse(), zone_id.parse()) {
            (Ok(system), Ok(zone_id)) => state.hub.zone(inst, system, zone_id).await,
            _ => None,
        },
        _ => None,
    };
    Ok(Some(view(d, zone.as_ref(), from_source.as_ref())))
}

/// The type of a device, as the clients group them (`deviceType` of the web, keep them alike): one
/// of [`TYPES`].
pub fn type_of(kind: &str, traits: &[String]) -> &'static str {
    let has = |prefix: &str| kind.starts_with(prefix) || traits.iter().any(|t| t.starts_with(prefix));
    match kind {
        "shutter" => return "shutter",
        "window" => return "window",
        "door" => return "door",
        _ => {}
    }
    if has("climate.") {
        "climate"
    } else if has("ev.charger") {
        "charger"
    } else if has("light") {
        "light"
    } else if has("cover.") {
        "shutter"
    } else if has("ventilation.unit") {
        "ventilation"
    } else if has("ventilation.room") {
        "room-air"
    } else if has("switch.") {
        "switch"
    } else if has("energy.battery") {
        "battery"
    } else if has("solar.") {
        "solar"
    } else if has("energy.") {
        "energy"
    } else if has("environment.") {
        "sensor"
    } else if has("camera") {
        "camera"
    } else if kind == "remote" || has("button") {
        "remote"
    } else {
        "other"
    }
}

/// The types a grant may name.
pub const TYPES: &[&str] =
    &["climate", "charger", "light", "shutter", "window", "door", "ventilation", "room-air", "switch", "solar", "battery", "energy", "sensor", "camera", "remote", "other"];

/// A device of a module, as an access to it is judged.
pub fn module_device(d: &DeviceView) -> crate::access::ModuleDevice {
    crate::access::ModuleDevice { id: d.id, room_id: d.room_id, kind: type_of(&d.kind, &d.traits) }
}

/// The devices as a user sees them: those of the installations they see, and the devices of the
/// modules they were given, each saying whether they may drive it.
pub fn for_user(devices: Vec<DeviceView>, access: &crate::access::Access) -> Vec<DeviceView> {
    devices
        .into_iter()
        .filter(|d| match d.installation_id {
            Some(i) => access.can_view(i),
            None => access.can_view_module(&module_device(d)),
        })
        .map(|mut d| {
            d.can_control = match d.installation_id {
                Some(i) => access.can_control(i),
                None => access.can_control_module(&module_device(&d)),
            };
            d
        })
        .collect()
}

/// The installation each device belongs to (`None`: a device of a module).
pub async fn installations(db: &Db) -> sqlx::Result<HashMap<i64, Option<i64>>> {
    Ok(db::list_devices(db).await?.into_iter().map(|d| (d.id, d.installation_id)).collect())
}

#[derive(Debug, Clone, Deserialize)]
pub struct ActionInput {
    #[serde(rename = "trait")]
    pub trait_name: String,
    pub action: String,
    #[serde(default)]
    pub params: Map<String, Value>,
}

/// Checks parameters against the catalog: an unknown one is refused (422, ADR 0012), a missing or
/// mistyped one too (400), and one outside the bounds or the choices the device tells in its
/// state (a generic trait), when it tells them.
fn check_params(action: &crate::traits::Action, params: &Map<String, Value>, state: &Value) -> ApiResult<()> {
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
                let told = |from: &Option<String>| from.as_ref().and_then(|f| state.get(f)).filter(|v| !v.is_null());
                let min = told(&p.min_from).and_then(Value::as_f64).or(p.min);
                let max = told(&p.max_from).and_then(Value::as_f64).or(p.max);
                let in_range = v.as_f64().is_none_or(|x| min.is_none_or(|m| x >= m) && max.is_none_or(|m| x <= m));
                let a_choice = told(&p.values_from).and_then(Value::as_array).is_none_or(|choices| choices.contains(v));
                if !ok || !in_range || !a_choice {
                    return Err(ApiError::bad_request(format!("invalid `{}`", p.name)));
                }
            }
        }
    }
    Ok(())
}

/// The action of a trait the device has, its parameters checked against the catalog.
pub fn check_action(view: &DeviceView, input: &ActionInput) -> ApiResult<()> {
    let version = view.traits.iter().find_map(|t| t.split_once('@').filter(|(n, _)| *n == input.trait_name).and_then(|(_, v)| v.parse::<u32>().ok()));
    let action = version.and_then(|v| crate::traits::get(&input.trait_name, v)).and_then(|t| t.action(&input.action)).ok_or_else(|| {
        ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, format!("{}.{} is not an action of this device", input.trait_name, input.action)).with_detail("code", "unsupported")
    })?;
    check_params(action, &input.params, &view.state)
}

/// Applies a checked action to a device, whatever its source. The rights of `actor` were checked
/// by the caller.
pub async fn apply(state: &AppState, view: &DeviceView, input: &ActionInput, actor: &str) -> ApiResult<()> {
    match (view.installation_id, view.source_id) {
        (Some(inst), _) => {
            let (Some(system_id), Some(zone_id)) = (view.reference["system_id"].as_i64(), view.reference["zone_id"].as_i64()) else {
                return Err(ApiError::not_found("zone not found"));
            };
            let cmd = crate::hub::ZoneCommand::from_value_strict(Value::Object(input.params.clone()))?;
            state.hub.control(inst, system_id, zone_id, &cmd, actor).await?;
            Ok(())
        }
        (None, Some(source_id)) => {
            let reference = view.reference.as_str().unwrap_or_default();
            state.drivers.act(source_id, reference, &input.trait_name, &input.action, &input.params).await.map_err(|e| match e {
                DriverError::Unreachable(m) => ApiError::bad_gateway(m),
                DriverError::Unsupported(m) => ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, m).with_detail("code", "unsupported"),
                DriverError::Invalid(m) => ApiError::bad_request(m),
            })
        }
        (None, None) => Err(ApiError::not_found("device not found")),
    }
}

/// Checks and applies an action to a device (a routine, a timer). The rights of `actor` were
/// checked by the caller.
pub async fn act(state: &AppState, id: i64, input: &ActionInput, actor: &str) -> ApiResult<()> {
    let view = get(state, id).await?.ok_or_else(|| ApiError::not_found("device not found"))?;
    check_action(&view, input)?;
    apply(state, &view, input, actor).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn params(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn a_generic_setting_stays_within_what_the_device_tells() {
        let number = crate::traits::get("generic.number", 1).unwrap().action("set").unwrap();
        let state = json!({"value": 20, "min": 10, "max": 30, "unit": "°C"});
        assert!(check_params(number, &params(json!({"value": 25})), &state).is_ok());
        assert!(check_params(number, &params(json!({"value": 31})), &state).is_err());
        assert!(check_params(number, &params(json!({"value": 99})), &json!({})).is_ok(), "no bounds told: any number");

        let select = crate::traits::get("generic.select", 1).unwrap().action("set").unwrap();
        let state = json!({"value": "eco", "options": ["eco", "confort"]});
        assert!(check_params(select, &params(json!({"value": "confort"})), &state).is_ok());
        assert!(check_params(select, &params(json!({"value": "turbo"})), &state).is_err());
    }
}
