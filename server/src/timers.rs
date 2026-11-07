//! Timers ("minuteries"): one action to run later on a zone, a group, an installation or
//! everything — e.g. "in 2 hours, switch the first floor off". Also used for the delayed
//! actions of routines. Run by the server; the target is resolved when the timer fires, so a
//! group or an installation includes the zones it has at that moment.

use crate::db::{self, Db};
use crate::error::{ApiError, ApiResult};
use crate::hub::{Hub, InstallationView, ZoneCommand};
use crate::routines::{run_actions, Action};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

/// Longest delay: one week.
pub const MAX_DELAY_MINUTES: i64 = 7 * 24 * 60;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Target {
    Zone { installation_id: i64, system_id: i64, zone_id: i64 },
    Group { installation_id: i64, group: String },
    Installation { installation_id: i64 },
    All,
}

impl Target {
    /// Installation targeted (`None` for all installations).
    pub fn installation_id(&self) -> Option<i64> {
        match self {
            Target::Zone { installation_id, .. } | Target::Group { installation_id, .. } | Target::Installation { installation_id } => Some(*installation_id),
            Target::All => None,
        }
    }

    /// Zones targeted, in the current state of the installations.
    pub fn zones(&self, installations: &[InstallationView]) -> Vec<(i64, i64, i64)> {
        installations
            .iter()
            .flat_map(|i| i.zones.iter().map(move |z| (i, z)))
            .filter(|(i, z)| match self {
                Target::Zone { installation_id, system_id, zone_id } => i.id == *installation_id && z.system_id == *system_id && z.zone_id == *zone_id,
                Target::Group { installation_id, group } => i.id == *installation_id && z.group.as_deref() == Some(group.as_str()),
                Target::Installation { installation_id } => i.id == *installation_id,
                Target::All => true,
            })
            .filter(|(i, _)| i.controllable)
            .map(|(i, z)| (i.id, z.system_id, z.zone_id))
            .collect()
    }

    /// "Salon (Maison)", "groupe Étage (Maison)", "Maison", "toutes les installations".
    pub fn describe(&self, installations: &[InstallationView]) -> String {
        let inst = |id: i64| installations.iter().find(|i| i.id == id);
        let inst_name = |id: i64| inst(id).map(|i| i.name.clone()).unwrap_or_else(|| format!("installation {id}"));
        match self {
            Target::Zone { installation_id, system_id, zone_id } => {
                let zone = inst(*installation_id)
                    .and_then(|i| i.zones.iter().find(|z| z.system_id == *system_id && z.zone_id == *zone_id))
                    .map(|z| z.name.clone())
                    .unwrap_or_else(|| format!("zone {zone_id}"));
                format!("{zone} ({})", inst_name(*installation_id))
            }
            Target::Group { installation_id, group } => format!("groupe {group} ({})", inst_name(*installation_id)),
            Target::Installation { installation_id } => inst_name(*installation_id),
            Target::All => "toutes les installations".into(),
        }
    }
}

/// The actions of a timer: one per zone; a mode change once per system.
pub fn actions(target: &Target, command: &ZoneCommand, installations: &[InstallationView]) -> Vec<Action> {
    let mut zones = target.zones(installations);
    zones.sort_unstable();
    zones.dedup();
    // The mode is system-wide: once per system is enough, but only when nothing else (setpoint,
    // sleep, speed…) has to reach every zone.
    if command.mode.is_some() && *command == (ZoneCommand { mode: command.mode, ..Default::default() }) {
        zones.dedup_by_key(|(i, s, _)| (*i, *s));
    }
    zones
        .into_iter()
        .map(|(installation_id, system_id, zone_id)| Action { installation_id, system_id, zone_id, command: command.clone(), delay_minutes: None })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct TimerView {
    pub id: i64,
    pub label: String,
    pub run_at: i64,
    pub target: Target,
    pub command: ZoneCommand,
    pub created_by: Option<String>,
    pub created_at: i64,
}

fn view(row: db::TimerRow) -> Option<TimerView> {
    Some(TimerView {
        id: row.id,
        label: row.label,
        run_at: row.run_at,
        target: serde_json::from_str(&row.target).ok()?,
        command: serde_json::from_str(&row.command).ok()?,
        created_by: row.created_by,
        created_at: row.created_at,
    })
}

pub async fn list(db: &Db) -> ApiResult<Vec<TimerView>> {
    Ok(db::list_timers(db).await?.into_iter().filter_map(view).collect())
}

#[derive(Debug, Deserialize)]
pub struct TimerInput {
    /// In how many minutes.
    pub delay_minutes: i64,
    pub target: Target,
    pub command: ZoneCommand,
    #[serde(default)]
    pub label: Option<String>,
}

/// Plans an action. Returns the timer.
pub async fn create(hub: &Hub, db: &Db, input: &TimerInput, actor: &str) -> ApiResult<TimerView> {
    if !(1..=MAX_DELAY_MINUTES).contains(&input.delay_minutes) {
        return Err(ApiError::bad_request("délai entre 1 minute et 7 jours"));
    }
    if input.command.is_empty() {
        return Err(ApiError::bad_request("choisissez une action"));
    }
    let installations = hub.snapshot().await;
    if input.target.zones(&installations).is_empty() {
        return Err(ApiError::bad_request("aucune zone pilotable dans cette cible"));
    }
    let label = input
        .label
        .as_deref()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| l.chars().take(80).collect())
        .unwrap_or_else(|| format!("{} · {}", verb(&input.command), input.target.describe(&installations)));
    let run_at = db::now() + input.delay_minutes * 60;
    let row = db::insert_timer(db, &label, run_at, &json!(input.target).to_string(), &json!(input.command).to_string(), actor).await?;
    hub.log("routine", "info", None, Some(actor), &format!("Minuterie dans {} : {label}", human_delay(input.delay_minutes)), None).await;
    hub.routines_changed();
    view(row).ok_or_else(|| ApiError::internal("timer not stored"))
}

pub async fn cancel(hub: &Hub, db: &Db, id: i64, actor: &str) -> ApiResult<()> {
    let row = db::timer_by_id(db, id).await?.ok_or_else(|| ApiError::not_found("minuterie introuvable"))?;
    db::delete_timer(db, id).await?;
    hub.log("routine", "info", None, Some(actor), &format!("Minuterie annulée : {}", row.label), None).await;
    hub.routines_changed();
    Ok(())
}

/// "Éteindre", "Allumer à 20°", "Consigne 19°", "Mode chaud".
pub fn verb(cmd: &ZoneCommand) -> String {
    let mut parts = vec![];
    match cmd.on {
        Some(false) => return "Éteindre".into(),
        Some(true) => parts.push("Allumer".to_string()),
        None => {}
    }
    if let Some(m) = cmd.mode {
        parts.push(format!("Mode {}", crate::hub::mode_label(m)));
    }
    if let Some(sp) = cmd.setpoint {
        parts.push(if parts.is_empty() { format!("Consigne {sp}°") } else { format!("{sp}°") });
    }
    if parts.is_empty() {
        cmd.describe()
    } else {
        parts.join(" ")
    }
}

pub fn human_delay(minutes: i64) -> String {
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m:02}"),
    }
}

/// Runs the timers whose time has come.
pub async fn run_due(hub: &Hub, db: &Db) {
    let due = match db::due_timers(db, db::now()).await {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("timers: {e}");
            return;
        }
    };
    if due.is_empty() {
        return;
    }
    let installations = hub.snapshot().await;
    for row in due {
        // Delete first: a timer runs at most once, even if the server stops during the run.
        if let Err(e) = db::delete_timer(db, row.id).await {
            tracing::error!("timers: {e}");
            continue;
        }
        let actor = row.created_by.clone().unwrap_or_else(|| "Minuterie".into());
        let Some(t) = view(row) else { continue };
        let actions = actions(&t.target, &t.command, &installations);
        let results = run_actions(hub, &actions, &format!("{actor} · minuterie")).await;
        let failed = results.iter().filter(|r| !r.ok).count();
        let (level, message) = if failed == 0 {
            ("info", format!("Minuterie exécutée : {}", t.label))
        } else {
            ("warn", format!("Minuterie exécutée avec {failed} échec(s) sur {} : {}", results.len(), t.label))
        };
        hub.log("routine", level, None, Some(&actor), &message, Some(json!({ "results": results }))).await;
    }
    hub.routines_changed();
}

pub async fn run_loop(hub: Arc<Hub>, db: Db) {
    let mut ticker = tokio::time::interval(Duration::from_secs(5));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticker.tick().await;
        run_due(&hub, &db).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_delays() {
        assert_eq!(human_delay(30), "30 min");
        assert_eq!(human_delay(120), "2 h");
        assert_eq!(human_delay(95), "1 h 35");
    }
}
