//! Solar surplus steering: the loads run on the surplus, in order of priority, and what they do now.

use crate::auth::{AdminUser, AuthUser};
use crate::db::{self, SurplusLoadRow};
use crate::devices::{self, DeviceView};
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
    /// The enabled automations that drive it too: they would contradict the steering.
    pub automations: Vec<String>,
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

/// The enabled automations that drive each device (a group expanded), by device.
pub async fn driven_by_automations(state: &AppState) -> ApiResult<std::collections::HashMap<i64, Vec<String>>> {
    let ctx = crate::automations::Context::load(state).await?;
    let mut out: std::collections::HashMap<i64, Vec<String>> = std::collections::HashMap::new();
    for row in db::list_automations(&state.db).await?.into_iter().filter(|r| r.enabled) {
        let Some(rule) = crate::automations::Automation::of(&row) else { continue };
        for id in rule.expanded_devices(&ctx) {
            let names = out.entry(id).or_default();
            if !names.contains(&row.name) {
                names.push(row.name.clone());
            }
        }
    }
    Ok(out)
}

async fn view(state: &AppState) -> ApiResult<SurplusView> {
    let devices = devices::list(state).await?;
    let rows = db::list_surplus_loads(&state.db).await?;
    let local = surplus::local_installations(state).await;
    let mut driven = driven_by_automations(state).await?;
    let snapshot = state.surplus.snapshot().await;
    let looked = !snapshot.statuses.is_empty() || snapshot.meter;
    let loads = rows
        .into_iter()
        .filter_map(|config| {
            let d = devices.iter().find(|d| d.id == config.device_id)?;
            let kind = surplus::kind_of(d, &local)?;
            Some(LoadView {
                device_id: d.id,
                automations: driven.remove(&d.id).unwrap_or_default(),
                name: d.name.clone(),
                kind,
                limit_min: limit(d, "limit_min"),
                limit_max: limit(d, "limit_max"),
                status: snapshot.statuses.get(&d.id).cloned().filter(|_| config.enabled),
                config,
            })
        })
        .collect::<Vec<_>>();
    // A charger follows the surplus by its charging profile (a solar slot), not by this list.
    let candidates = devices
        .iter()
        .filter(|d| !loads.iter().any(|l| l.device_id == d.id))
        .filter(|d| surplus::kind_of(d, &local) != Some(Kind::Charger))
        .filter_map(|d| surplus::kind_of(d, &local).map(|kind| Candidate { device_id: d.id, name: d.name.clone(), kind }))
        .collect();
    Ok(SurplusView { meter: looked.then_some(snapshot.meter), export: snapshot.export, loads, candidates })
}

pub async fn get(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<SurplusView>> {
    crate::api::energy::reads_energy(&state, &user).await?;
    Ok(Json(view(&state).await?))
}

/// How a load is run (all optional: the device's own limits by default).
#[derive(Deserialize)]
pub struct LoadInput {
    #[serde(default = "crate::yes")]
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
    /// A zone of the air conditioning (see `surplus::ClimatePlan`).
    pub heat_setpoint: Option<f64>,
    pub cool_setpoint: Option<f64>,
    pub heat_below: Option<f64>,
    pub cool_above: Option<f64>,
}

/// Adds a load (last in priority) or changes how it is run.
pub async fn save(State(state): State<AppState>, _admin: AdminUser, Path(device_id): Path<i64>, Json(input): Json<LoadInput>) -> ApiResult<Json<SurplusView>> {
    let devices = devices::list(&state).await?;
    let d = devices.iter().find(|d| d.id == device_id).ok_or_else(|| ApiError::not_found("device not found"))?;
    let kind = surplus::kind_of(d, &surplus::local_installations(&state).await).ok_or_else(|| {
        ApiError::bad_request(crate::text!(
            "seules une borne, une prise pilotée ou une zone de climatisation de ce serveur peuvent suivre le surplus",
            "only a charger, a relay or a zone of the air conditioning of this server can follow the surplus",
            "solo un cargador, un relé o una zona de climatización de este servidor pueden seguir el excedente"
        ))
    })?;
    if kind == Kind::Charger && !db::list_surplus_loads(&state.db).await?.iter().any(|r| r.device_id == device_id) {
        return Err(ApiError::bad_request(crate::text!(
            "une borne suit le surplus par son profil de recharge (Recharge › Profil de recharge : « Solaire »)",
            "a charger follows the surplus by its charging profile (Charging › Charging profile: “Solar”)",
            "un cargador sigue el excedente con su perfil de carga (Carga › Perfil de carga: «Solar»)"
        )));
    }
    let bad = |fr: &str, en: &str, es: &str| ApiError::bad_request(crate::i18n::Text { fr: fr.into(), en: en.into(), es: es.into() });
    let (lo, hi) = (limit(d, "limit_min").unwrap_or(6), limit(d, "limit_max").unwrap_or(32));
    if kind == Kind::Charger {
        let (min, max) = (input.min_amps.unwrap_or(lo), input.max_amps.unwrap_or(hi.min(16)));
        if min < lo || max > hi || min > max {
            return Err(bad(&format!("intensité de {lo} à {hi} A"), &format!("current from {lo} to {hi} A"), &format!("intensidad de {lo} a {hi} A")));
        }
    }
    if kind == Kind::Climate {
        let plan = surplus::ClimatePlan::default();
        let (hs, cs) = (input.heat_setpoint.unwrap_or(plan.heat_setpoint), input.cool_setpoint.unwrap_or(plan.cool_setpoint));
        let (hb, ca) = (input.heat_below.unwrap_or(plan.heat_below), input.cool_above.unwrap_or(plan.cool_above));
        if !(10.0..=32.0).contains(&hs) || !(10.0..=32.0).contains(&cs) {
            return Err(bad("consignes de 10 à 32 °C", "setpoints from 10 to 32 °C", "consignas de 10 a 32 °C"));
        }
        if !(-30.0..=45.0).contains(&hb) || !(-30.0..=45.0).contains(&ca) || hb > ca {
            return Err(bad(
                "températures extérieures de -30 à 45 °C, le chauffage sous le froid",
                "outdoor temperatures from -30 to 45 °C, heating below cooling",
                "temperaturas exteriores de -30 a 45 °C, la calefacción por debajo del frío",
            ));
        }
    }
    if matches!(kind, Kind::Relay | Kind::Climate) && !input.power.is_some_and(|p| (1..=20_000).contains(&p)) {
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
            power: input.power.filter(|_| matches!(kind, Kind::Relay | Kind::Climate)),
            guarantee_kwh: input.guarantee_kwh.filter(|_| guarantee),
            guarantee_by: by.filter(|_| guarantee).map(str::to_string),
            min_on_minutes: input.min_on_minutes,
            tolerance: input.tolerance,
            heat_setpoint: input.heat_setpoint.filter(|_| kind == Kind::Climate),
            cool_setpoint: input.cool_setpoint.filter(|_| kind == Kind::Climate),
            heat_below: input.heat_below.filter(|_| kind == Kind::Climate),
            cool_above: input.cool_above.filter(|_| kind == Kind::Climate),
        },
    )
    .await?;
    // Given (back) to the surplus: it steers it again at once, even after a stop by hand.
    state.surplus.release(device_id).await;
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
