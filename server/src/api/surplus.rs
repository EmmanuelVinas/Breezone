//! Solar surplus steering, as the pages show it: the devices automations put on the surplus, in the
//! order of their priority, and what each does now. It is set in the automations only.

use crate::auth::AuthUser;
use crate::charging::{Mode, Solar};
use crate::db;
use crate::devices;
use crate::error::ApiResult;
use crate::surplus::{self, ClimateMode, Kind, Status};
use crate::AppState;
use axum::extract::State;
use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct LoadView {
    pub device_id: i64,
    pub name: String,
    pub kind: Kind,
    /// The automation that put it on the surplus (`None`: the list of before, not yet made into
    /// automations), its name and priority (1 first).
    pub automation_id: Option<i64>,
    pub automation: Option<String>,
    pub priority: Option<i64>,
    /// A relay or a zone: its power (W), how long it stays on at least, what it may draw.
    pub power: Option<i64>,
    pub min_on_minutes: i64,
    pub tolerance: i64,
    /// A zone: heat or cool, to this setpoint.
    pub mode: Option<ClimateMode>,
    pub setpoint: Option<f64>,
    /// A charger: how it charges on the surplus.
    pub solar: Option<Solar>,
    /// What it does now (`None`: not looked at yet).
    pub status: Option<Status>,
}

#[derive(Serialize)]
pub struct SurplusView {
    /// The home has a grid meter (`null`: not looked at yet).
    pub meter: Option<bool>,
    /// What goes back to the grid now (W; negative: drawn from it).
    pub export: Option<f64>,
    /// In the order the surplus is shared.
    pub loads: Vec<LoadView>,
}

async fn view(state: &AppState) -> ApiResult<SurplusView> {
    let devices = devices::list(state).await?;
    let local = surplus::local_installations(state).await;
    let rules: std::collections::HashMap<i64, (String, i64)> = db::list_automations(&state.db).await?.into_iter().map(|r| (r.id, (r.name, r.priority))).collect();
    let snapshot = state.surplus.snapshot().await;
    let looked = !snapshot.statuses.is_empty() || snapshot.meter;
    let mut loads = vec![];
    for row in db::list_surplus_loads(&state.db).await?.into_iter().filter(|r| r.enabled) {
        let Some(d) = devices.iter().find(|d| d.id == row.device_id) else { continue };
        let Some(kind) = surplus::kind_of(d, &local) else { continue };
        let rule = row.automation_id.and_then(|id| rules.get(&id));
        let mode = surplus::climate_mode(&row);
        loads.push(LoadView {
            device_id: d.id,
            name: d.name.clone(),
            kind,
            automation_id: row.automation_id.filter(|_| rule.is_some()),
            automation: rule.map(|(n, _)| n.clone()),
            priority: rule.map(|(_, p)| *p),
            power: row.power,
            min_on_minutes: row.min_on_minutes,
            tolerance: row.tolerance,
            mode,
            setpoint: match mode {
                Some(ClimateMode::Heat) => row.heat_setpoint,
                Some(ClimateMode::Cool) => row.cool_setpoint,
                None => None,
            },
            solar: None,
            status: snapshot.statuses.get(&d.id).cloned(),
        });
    }
    // The chargers an automation told to charge on the surplus.
    for plan in db::list_charger_plans(&state.db).await? {
        if Mode::of_row(&plan) != Mode::Solar {
            continue;
        }
        let Some(d) = devices.iter().find(|d| d.id == plan.device_id) else { continue };
        let rule = plan.automation_id.and_then(|id| rules.get(&id));
        loads.push(LoadView {
            device_id: d.id,
            name: d.name.clone(),
            kind: Kind::Charger,
            automation_id: plan.automation_id.filter(|_| rule.is_some()),
            automation: rule.map(|(n, _)| n.clone()),
            priority: rule.map(|(_, p)| *p),
            power: None,
            min_on_minutes: 0,
            tolerance: 0,
            mode: None,
            setpoint: None,
            solar: Some(Solar::of(&plan)),
            status: snapshot.statuses.get(&d.id).cloned(),
        });
    }
    loads.sort_by_key(|l| l.priority.unwrap_or(i64::MAX));
    Ok(SurplusView { meter: looked.then_some(snapshot.meter), export: snapshot.export, loads })
}

pub async fn get(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<SurplusView>> {
    crate::api::energy::reads_energy(&state, &user).await?;
    Ok(Json(view(&state).await?))
}
