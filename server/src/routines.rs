//! Routines ("scènes"): a named list of actions, started on demand, run by the Breezone server.
//! An action drives an Airzone zone or a device of any module (a light, a shutter, a charger…).
//! Each action can wait (`delay_minutes`): it then becomes a timer. Airzone gates its own
//! routines behind its paid cloud plan; Breezone runs these locally, with no subscription.
//!
//! Fixed-time schedules are not routines: they are the schedules of the Airzone app, run by
//! Airzone and edited from Breezone (see `airzone_schedules`).

use crate::db::{self, RoutineRow};
use crate::devices::{self, ActionInput};
use crate::error::{ApiError, ApiResult};
use crate::hub::{Hub, ZoneCommand};
use crate::i18n::Text;
use crate::AppState;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

/// A command applied to one zone (also what a timer and the batch of zone commands run).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Action {
    pub installation_id: i64,
    pub system_id: i64,
    pub zone_id: i64,
    #[serde(flatten)]
    pub command: ZoneCommand,
    /// Run this action that many minutes after the routine starts (a timer).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delay_minutes: Option<i64>,
}

/// An action of a trait of a device, as `POST /api/devices/{id}/actions` takes it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceAction {
    pub device_id: i64,
    #[serde(rename = "trait")]
    pub trait_name: String,
    pub action: String,
    #[serde(default)]
    pub params: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delay_minutes: Option<i64>,
}

impl DeviceAction {
    pub fn input(&self) -> ActionInput {
        ActionInput { trait_name: self.trait_name.clone(), action: self.action.clone(), params: self.params.clone() }
    }
}

/// One action of a routine. The zones keep the shape they were stored with: a routine written by
/// an older version reads the same.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum RoutineAction {
    Device(DeviceAction),
    Zone(Action),
}

impl RoutineAction {
    fn installation_id(&self) -> Option<i64> {
        match self {
            RoutineAction::Zone(z) => Some(z.installation_id),
            RoutineAction::Device(_) => None,
        }
    }

    pub fn delay_minutes(&self) -> Option<i64> {
        match self {
            RoutineAction::Device(d) => d.delay_minutes,
            RoutineAction::Zone(z) => z.delay_minutes,
        }
    }
}

/// The installation a routine acts on, when it acts on one only (its zones): its events go with it
/// in the activity feed. `None` for a routine across installations or with devices of the modules.
pub fn installation_of(actions: &[RoutineAction]) -> Option<i64> {
    let first = actions.first()?.installation_id()?;
    actions.iter().all(|a| a.installation_id() == Some(first)).then_some(first)
}

/// What an action needs the right to drive: an installation, a device of a module, or (a device
/// no longer there) an administrator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Installation(i64),
    Module(crate::access::ModuleDevice),
    Gone,
}

/// Where each device belongs: its installation, or (a device of a module) its room and type.
pub struct DeviceScopes(HashMap<i64, Scope>);

impl DeviceScopes {
    pub async fn load(state: &crate::AppState) -> crate::error::ApiResult<DeviceScopes> {
        Ok(DeviceScopes(
            crate::devices::list(state)
                .await?
                .iter()
                .map(|d| (d.id, d.installation_id.map_or_else(|| Scope::Module(crate::devices::module_device(d)), Scope::Installation)))
                .collect(),
        ))
    }

    pub fn of(&self, actions: &[RoutineAction]) -> Vec<Scope> {
        actions
            .iter()
            .map(|a| match a {
                RoutineAction::Zone(z) => Scope::Installation(z.installation_id),
                RoutineAction::Device(d) => self.0.get(&d.device_id).copied().unwrap_or(Scope::Gone),
            })
            .collect()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RoutineView {
    pub id: i64,
    pub name: String,
    pub enabled: bool,
    pub actions: Vec<RoutineAction>,
    pub last_run_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RoutineInput {
    pub name: String,
    #[serde(default = "crate::yes")]
    pub enabled: bool,
    /// Not supported any more: fixed-time schedules are the Airzone ones.
    #[serde(default)]
    pub time: Option<String>,
    pub actions: Vec<RoutineAction>,
}

/// Validates user input and returns (name, actions JSON). The device actions are checked against
/// the devices by [`check_devices`].
pub fn validate(input: &RoutineInput) -> ApiResult<(String, String)> {
    let name = input.name.trim().to_string();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(ApiError::bad_request("name must be 1-80 characters"));
    }
    if input.time.as_deref().is_some_and(|t| !t.trim().is_empty()) {
        return Err(ApiError::bad_request("les routines se lancent à la demande ; pour une heure fixe, créez une programmation (onglet Programmations)"));
    }
    if input.actions.is_empty() || input.actions.len() > 100 {
        return Err(ApiError::bad_request("a routine needs 1 to 100 actions"));
    }
    if input.actions.iter().any(|a| matches!(a, RoutineAction::Zone(z) if z.command.is_empty())) {
        return Err(ApiError::bad_request("every action must change at least one setting"));
    }
    if input.actions.iter().any(|a| a.delay_minutes().is_some_and(|d| !(0..=crate::timers::MAX_DELAY_MINUTES).contains(&d))) {
        return Err(ApiError::bad_request("délai entre 0 minute et 7 jours"));
    }
    let actions = serde_json::to_string(&input.actions).map_err(|e| ApiError::internal(e.to_string()))?;
    Ok((name, actions))
}

/// Every device action names a device there, and an action of one of its traits with valid
/// parameters.
pub async fn check_devices(state: &AppState, actions: &[RoutineAction]) -> ApiResult<()> {
    let devices: Vec<&DeviceAction> = actions
        .iter()
        .filter_map(|a| match a {
            RoutineAction::Device(d) => Some(d),
            RoutineAction::Zone(_) => None,
        })
        .collect();
    if devices.is_empty() {
        return Ok(());
    }
    let views = devices::list(state).await?;
    for d in devices {
        let view = views.iter().find(|v| v.id == d.device_id).ok_or_else(|| ApiError::bad_request("unknown device"))?;
        devices::check_action(view, &d.input())?;
    }
    Ok(())
}

pub fn to_view(row: &RoutineRow) -> RoutineView {
    RoutineView {
        id: row.id,
        name: row.name.clone(),
        enabled: row.enabled,
        actions: serde_json::from_str(&row.actions).unwrap_or_default(),
        last_run_at: row.last_run_at,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ActionResult {
    pub ok: bool,
    pub error: Option<Text>,
}

impl ActionResult {
    fn of(r: ApiResult<()>) -> ActionResult {
        ActionResult { ok: r.is_ok(), error: r.err().map(|e| e.message) }
    }
}

/// Runs a list of zone commands sequentially and returns one result per action.
pub async fn run_actions(hub: &Hub, actions: &[Action], actor: &str) -> Vec<ActionResult> {
    let mut results = Vec::with_capacity(actions.len());
    for a in actions {
        results.push(ActionResult::of(hub.control(a.installation_id, a.system_id, a.zone_id, &a.command, actor).await.map(|_| ())));
    }
    results
}

/// Runs a routine now. Returns the per-action results.
pub async fn run_routine(state: &AppState, row: &RoutineRow, actor: &str) -> ApiResult<Vec<ActionResult>> {
    let (hub, db) = (&state.hub, &state.db);
    let all: Vec<RoutineAction> = serde_json::from_str(&row.actions).map_err(|e| ApiError::internal(e.to_string()))?;
    let installation = installation_of(&all);
    let (delayed, now): (Vec<RoutineAction>, Vec<RoutineAction>) = all.into_iter().partition(|a| a.delay_minutes().unwrap_or(0) > 0);
    let who = format!("{actor} · routine « {} »", row.name);
    let mut results = Vec::with_capacity(now.len());
    for a in &now {
        let r = match a {
            RoutineAction::Zone(z) => hub.control(z.installation_id, z.system_id, z.zone_id, &z.command, &who).await.map(|_| ()),
            RoutineAction::Device(d) => devices::act(state, d.device_id, &d.input(), &who).await,
        };
        results.push(ActionResult::of(r));
    }
    // Delayed actions become timers.
    let installations = hub.snapshot().await;
    let context = match delayed.iter().any(|a| matches!(a, RoutineAction::Device(_))) {
        true => crate::automations::Context::load(state).await.ok(),
        false => None,
    };
    for a in delayed {
        let delay = a.delay_minutes().unwrap_or(0);
        let (target, command, what) = match a {
            RoutineAction::Zone(z) => {
                let target = crate::timers::Target::Zone { installation_id: z.installation_id, system_id: z.system_id, zone_id: z.zone_id };
                let (place, command) = (target.describe(&installations), z.command.describe());
                (target, z.command, crate::text!("{0} : {1}", "{0}: {1}", "{0}: {1}", place.arg(), command.arg()))
            }
            RoutineAction::Device(d) => {
                let what = match &context {
                    Some(ctx) => ctx.describe_device_action(d.device_id, &d.trait_name, &d.action, &d.params),
                    None => Text::plain(format!("{}.{}", d.trait_name, d.action)),
                };
                let target = crate::timers::Target::Device { device_id: d.device_id, trait_name: d.trait_name, action: d.action, params: d.params };
                (target, ZoneCommand::default(), what)
            }
        };
        let label = crate::text!("{0} — {1}", "{0} — {1}", "{0} — {1}", row.name, what.arg()).store();
        let run_at = db::now() + delay * 60;
        if let Err(e) = db::insert_timer(db, &label, run_at, &json!(target).to_string(), &json!(command).to_string(), &who).await {
            tracing::error!("cannot plan delayed action: {e}");
        }
    }
    db::set_routine_last_run(db, row.id, db::now()).await?;
    let failed = results.iter().filter(|r| !r.ok).count();
    let (level, message) = if failed == 0 {
        ("info", crate::text!("Routine « {0} » exécutée ({1} action(s))", "Routine “{0}” run ({1} action(s))", "Rutina «{0}» ejecutada ({1} acción(es))", row.name, results.len()))
    } else {
        (
            "warn",
            crate::text!(
                "Routine « {0} » exécutée avec {1} échec(s) sur {2}",
                "Routine “{0}” run with {1} failure(s) out of {2}",
                "Rutina «{0}» ejecutada con {1} fallo(s) de {2}",
                row.name,
                failed,
                results.len()
            ),
        )
    };
    hub.log("routine", level, installation, Some(actor), message, Some(json!({ "routine_id": row.id, "results": results }))).await;
    hub.routines_changed();
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone(delay: Option<i64>) -> RoutineAction {
        RoutineAction::Zone(Action { installation_id: 1, system_id: 1, zone_id: 1, command: ZoneCommand { on: Some(true), ..Default::default() }, delay_minutes: delay })
    }

    #[test]
    fn routines_have_no_fixed_time() {
        let ok = RoutineInput { name: "Je pars".into(), enabled: true, time: None, actions: vec![zone(Some(120))] };
        assert!(validate(&ok).is_ok());
        let timed = RoutineInput { time: Some("07:00".into()), ..ok.clone() };
        assert!(validate(&timed).is_err());
        let empty = RoutineInput {
            actions: vec![RoutineAction::Zone(Action { installation_id: 1, system_id: 1, zone_id: 1, command: ZoneCommand::default(), delay_minutes: None })],
            ..ok.clone()
        };
        assert!(validate(&empty).is_err());
        let too_long = RoutineInput { actions: vec![zone(Some(20_000))], ..ok };
        assert!(validate(&too_long).is_err());
    }

    #[test]
    fn reads_the_zones_stored_before_and_the_devices() {
        let stored = r#"[{"installation_id":1,"system_id":1,"zone_id":2,"on":false},{"device_id":7,"trait":"light","action":"set","params":{"on":false},"delay_minutes":30}]"#;
        let actions: Vec<RoutineAction> = serde_json::from_str(stored).unwrap();
        assert!(matches!(&actions[0], RoutineAction::Zone(z) if z.zone_id == 2 && z.command.on == Some(false)));
        assert!(matches!(&actions[1], RoutineAction::Device(d) if d.device_id == 7 && d.trait_name == "light" && d.delay_minutes == Some(30)));
        // Written back, a zone keeps its shape.
        assert_eq!(serde_json::to_value(&actions[0]).unwrap(), json!({"installation_id":1,"system_id":1,"zone_id":2,"on":false}));
    }

    #[test]
    fn a_routine_goes_with_its_installation_in_the_feed() {
        let other =
            RoutineAction::Zone(Action { installation_id: 2, system_id: 1, zone_id: 1, command: ZoneCommand { on: Some(true), ..Default::default() }, delay_minutes: None });
        let device = RoutineAction::Device(DeviceAction { device_id: 7, trait_name: "light".into(), action: "set".into(), params: Map::new(), delay_minutes: None });
        assert_eq!(installation_of(&[zone(None), zone(Some(30))]), Some(1));
        assert_eq!(installation_of(&[zone(None), other]), None, "across installations");
        assert_eq!(installation_of(&[zone(None), device]), None, "with a device of a module");
        assert_eq!(installation_of(&[]), None);
    }

    #[test]
    fn a_device_of_a_module_needs_the_right_to_drive_it() {
        let lamp = crate::access::ModuleDevice { id: 7, room_id: Some(2), kind: "light" };
        let scopes = DeviceScopes(HashMap::from([(7, Scope::Module(lamp)), (8, Scope::Installation(3))]));
        let device = |id| RoutineAction::Device(DeviceAction { device_id: id, trait_name: "light".into(), action: "set".into(), params: Map::new(), delay_minutes: None });
        assert_eq!(scopes.of(&[zone(None), device(7), device(8), device(99)]), vec![Scope::Installation(1), Scope::Module(lamp), Scope::Installation(3), Scope::Gone]);
    }
}
