//! Routines ("scènes"): a named list of zone actions, started on demand, run by the Breezone
//! server. Each action can wait (`delay_minutes`): it then becomes a timer. Airzone gates its
//! own routines behind its paid cloud plan; Breezone runs these locally, with no subscription.
//!
//! Fixed-time schedules are not routines: they are the schedules of the Airzone app, run by
//! Airzone and edited from Breezone (see `airzone_schedules`).

use crate::db::{self, Db, RoutineRow};
use crate::error::{ApiError, ApiResult};
use crate::hub::{Hub, ZoneCommand};
use crate::i18n::Text;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// One action of a routine: a command applied to one zone.
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

#[derive(Debug, Clone, Serialize)]
pub struct RoutineView {
    pub id: i64,
    pub name: String,
    pub enabled: bool,
    pub actions: Vec<Action>,
    pub last_run_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RoutineInput {
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Not supported any more: fixed-time schedules are the Airzone ones.
    #[serde(default)]
    pub time: Option<String>,
    pub actions: Vec<Action>,
}

fn yes() -> bool {
    true
}

/// Validates user input and returns (name, actions JSON).
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
    if input.actions.iter().any(|a| a.command.is_empty()) {
        return Err(ApiError::bad_request("every action must change at least one setting"));
    }
    if input.actions.iter().any(|a| a.delay_minutes.is_some_and(|d| !(0..=crate::timers::MAX_DELAY_MINUTES).contains(&d))) {
        return Err(ApiError::bad_request("délai entre 0 minute et 7 jours"));
    }
    let actions = serde_json::to_string(&input.actions).map_err(|e| ApiError::internal(e.to_string()))?;
    Ok((name, actions))
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

/// Runs a list of actions sequentially and returns one result per action.
pub async fn run_actions(hub: &Hub, actions: &[Action], actor: &str) -> Vec<ActionResult> {
    let mut results = Vec::with_capacity(actions.len());
    for a in actions {
        let r = hub.control(a.installation_id, a.system_id, a.zone_id, &a.command, actor).await;
        results.push(ActionResult { ok: r.is_ok(), error: r.err().map(|e| e.message) });
    }
    results
}

/// Runs a routine now. Returns the per-action results.
pub async fn run_routine(hub: &Hub, db: &Db, row: &RoutineRow, actor: &str) -> ApiResult<Vec<ActionResult>> {
    let all: Vec<Action> = serde_json::from_str(&row.actions).map_err(|e| ApiError::internal(e.to_string()))?;
    let (delayed, actions): (Vec<Action>, Vec<Action>) = all.into_iter().partition(|a| a.delay_minutes.unwrap_or(0) > 0);
    let results = run_actions(hub, &actions, &format!("{actor} · routine « {} »", row.name)).await;
    // Delayed actions become timers.
    let installations = hub.snapshot().await;
    for a in delayed {
        let target = crate::timers::Target::Zone { installation_id: a.installation_id, system_id: a.system_id, zone_id: a.zone_id };
        let delay = a.delay_minutes.unwrap_or(0);
        let (target, command) = (target.describe(&installations), a.command.describe());
        let label = crate::text!("{0} — {1} : {2}", "{0} — {1}: {2}", "{0} — {1}: {2}", row.name, target.arg(), command.arg()).store();
        let target = crate::timers::Target::Zone { installation_id: a.installation_id, system_id: a.system_id, zone_id: a.zone_id };
        let run_at = db::now() + delay * 60;
        if let Err(e) = db::insert_timer(db, &label, run_at, &json!(target).to_string(), &json!(a.command).to_string(), &format!("{actor} · routine « {} »", row.name)).await {
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
    hub.log("routine", level, None, Some(actor), message, Some(json!({ "routine_id": row.id, "results": results }))).await;
    hub.routines_changed();
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routines_have_no_fixed_time() {
        let action = Action { installation_id: 1, system_id: 1, zone_id: 1, command: ZoneCommand { on: Some(true), ..Default::default() }, delay_minutes: Some(120) };
        let ok = RoutineInput { name: "Je pars".into(), enabled: true, time: None, actions: vec![action.clone()] };
        assert!(validate(&ok).is_ok());
        let timed = RoutineInput { time: Some("07:00".into()), ..ok.clone() };
        assert!(validate(&timed).is_err());
        let empty = RoutineInput { actions: vec![Action { command: ZoneCommand::default(), ..action.clone() }], ..ok.clone() };
        assert!(validate(&empty).is_err());
        let too_long = RoutineInput { actions: vec![Action { delay_minutes: Some(20_000), ..action }], ..ok };
        assert!(validate(&too_long).is_err());
    }
}
