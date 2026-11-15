//! Solar surplus steering: the loads run on the surplus, in order of priority, and what they do now.

use super::home::{devices_of, DeviceView};
use crate::auth::{AdminUser, AuthUser};
use crate::db::{self, SurplusLoadRow};
use crate::error::{ApiError, ApiResult};
use crate::surplus::{self, Kind, Status};
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct LoadView {
    pub device_id: i64,
    pub name: String,
    pub kind: Kind,
    #[serde(flatten)]
    pub config: SurplusLoadRow,
    /// The limits of the charger (A), for the settings.
    pub limit_min: Option<i64>,
    pub limit_max: Option<i64>,
    /// What it does now (`None`: not steered, or not looked at yet).
    pub status: Option<Status>,
}

#[derive(Serialize)]
pub struct Candidate {
    pub device_id: i64,
    pub name: String,
    pub kind: Kind,
}

#[derive(Serialize)]
pub struct SurplusView {
    /// The home has a grid meter (`null`: not looked at yet).
    pub meter: Option<bool>,
    /// What goes back to the grid now (W; negative: drawn from it).
    pub export: Option<f64>,
    pub loads: Vec<LoadView>,
    /// Devices that could be added (a charger, a relay of this server).
    pub candidates: Vec<Candidate>,
}

fn limit(d: &DeviceView, field: &str) -> Option<i64> {
    d.state.get(field).and_then(serde_json::Value::as_i64)
}

async fn view(state: &AppState) -> ApiResult<SurplusView> {
    let devices = devices_of(state).await?;
    let rows = db::list_surplus_loads(&state.db).await?;
    let snapshot = state.surplus.snapshot().await;
    let looked = !snapshot.statuses.is_empty() || snapshot.meter;
    let loads = rows
        .into_iter()
        .filter_map(|config| {
            let d = devices.iter().find(|d| d.id == config.device_id)?;
            let kind = surplus::kind_of(d)?;
            Some(LoadView {
                device_id: d.id,
                name: d.name.clone(),
                kind,
                limit_min: limit(d, "limit_min"),
                limit_max: limit(d, "limit_max"),
                status: snapshot.statuses.get(&d.id).cloned().filter(|_| config.enabled),
                config,
            })
        })
        .collect::<Vec<_>>();
    let candidates = devices
        .iter()
        .filter(|d| !loads.iter().any(|l| l.device_id == d.id))
        .filter_map(|d| surplus::kind_of(d).map(|kind| Candidate { device_id: d.id, name: d.name.clone(), kind }))
        .collect();
    Ok(SurplusView { meter: looked.then_some(snapshot.meter), export: snapshot.export, loads, candidates })
}

pub async fn get(State(state): State<AppState>, _user: AuthUser) -> ApiResult<Json<SurplusView>> {
    Ok(Json(view(&state).await?))
}

/// How a load is run (all optional: the device's own limits by default).
#[derive(Deserialize)]
pub struct LoadInput {
    #[serde(default = "yes")]
    pub enabled: bool,
    pub min_amps: Option<i64>,
    pub max_amps: Option<i64>,
    /// A relay: its power when on (W), needed.
    pub power: Option<i64>,
    pub guarantee_kwh: Option<f64>,
    /// "HH:MM".
    pub guarantee_by: Option<String>,
    #[serde(default)]
    pub min_on_minutes: i64,
    #[serde(default)]
    pub tolerance: i64,
}

fn yes() -> bool {
    true
}

/// Adds a load (last in priority) or changes how it is run.
pub async fn save(State(state): State<AppState>, _admin: AdminUser, Path(device_id): Path<i64>, Json(input): Json<LoadInput>) -> ApiResult<Json<SurplusView>> {
    let devices = devices_of(&state).await?;
    let d = devices.iter().find(|d| d.id == device_id).ok_or_else(|| ApiError::not_found("device not found"))?;
    let kind = surplus::kind_of(d).ok_or_else(|| {
        ApiError::bad_request(crate::text!(
            "seules une borne ou une prise pilotée de ce serveur peuvent suivre le surplus",
            "only a charger or a relay of this server can follow the surplus",
            "solo un cargador o un relé de este servidor pueden seguir el excedente"
        ))
    })?;
    let bad = |fr: &str, en: &str, es: &str| ApiError::bad_request(crate::i18n::Text { fr: fr.into(), en: en.into(), es: es.into() });
    let (lo, hi) = (limit(d, "limit_min").unwrap_or(6), limit(d, "limit_max").unwrap_or(32));
    if kind == Kind::Charger {
        let (min, max) = (input.min_amps.unwrap_or(lo), input.max_amps.unwrap_or(hi.min(16)));
        if min < lo || max > hi || min > max {
            return Err(bad(&format!("intensité de {lo} à {hi} A"), &format!("current from {lo} to {hi} A"), &format!("intensidad de {lo} a {hi} A")));
        }
    }
    if kind == Kind::Relay && !input.power.is_some_and(|p| (1..=20_000).contains(&p)) {
        return Err(bad("donnez la puissance de l'appareil (W)", "give the power of the device (W)", "indique la potencia del aparato (W)"));
    }
    let by = input.guarantee_by.as_deref().map(str::trim).filter(|t| !t.is_empty());
    if by.is_some_and(|t| chrono::NaiveTime::parse_from_str(t, "%H:%M").is_err()) || input.guarantee_kwh.is_some_and(|k| !(0.0..=200.0).contains(&k)) {
        return Err(bad("garantie : de 0 à 200 kWh, heure HH:MM", "guarantee: 0 to 200 kWh, time HH:MM", "garantía: de 0 a 200 kWh, hora HH:MM"));
    }
    if !(0..=240).contains(&input.min_on_minutes) || !(0..=5000).contains(&input.tolerance) {
        return Err(bad("durée de 0 à 240 min, tolérance de 0 à 5 000 W", "duration 0 to 240 min, tolerance 0 to 5,000 W", "duración de 0 a 240 min, tolerancia de 0 a 5000 W"));
    }
    let rows = db::list_surplus_loads(&state.db).await?;
    let position = rows.iter().find(|r| r.device_id == device_id).map(|r| r.position).unwrap_or_else(|| rows.iter().map(|r| r.position + 1).max().unwrap_or(0));
    let guarantee = kind == Kind::Charger && by.is_some() && input.guarantee_kwh.is_some_and(|k| k > 0.0);
    db::upsert_surplus_load(
        &state.db,
        &SurplusLoadRow {
            device_id,
            position,
            enabled: input.enabled,
            min_amps: input.min_amps.filter(|_| kind == Kind::Charger),
            max_amps: input.max_amps.filter(|_| kind == Kind::Charger),
            power: input.power.filter(|_| kind == Kind::Relay),
            guarantee_kwh: input.guarantee_kwh.filter(|_| guarantee),
            guarantee_by: by.filter(|_| guarantee).map(str::to_string),
            min_on_minutes: input.min_on_minutes,
            tolerance: input.tolerance,
        },
    )
    .await?;
    Ok(Json(view(&state).await?))
}

/// Breezone no longer runs it on the surplus (it is left as it is).
pub async fn remove(State(state): State<AppState>, _admin: AdminUser, Path(device_id): Path<i64>) -> ApiResult<StatusCode> {
    if !db::delete_surplus_load(&state.db, device_id).await? {
        return Err(ApiError::not_found("device not found"));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct OrderInput {
    /// Every load, the first one first.
    pub device_ids: Vec<i64>,
}

/// Sets the priorities.
pub async fn order(State(state): State<AppState>, _admin: AdminUser, Json(input): Json<OrderInput>) -> ApiResult<Json<SurplusView>> {
    for mut row in db::list_surplus_loads(&state.db).await? {
        let Some(position) = input.device_ids.iter().position(|id| *id == row.device_id) else { continue };
        row.position = position as i64;
        db::upsert_surplus_load(&state.db, &row).await?;
    }
    Ok(Json(view(&state).await?))
}
