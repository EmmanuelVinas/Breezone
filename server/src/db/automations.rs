//! Automations.

use super::*;

#[derive(Debug, Clone, FromRow)]
pub struct AutomationRow {
    pub id: i64,
    pub name: String,
    pub enabled: bool,
    /// JSON, see `automations::Trigger`.
    pub trigger_def: String,
    /// "all" or "any": how the conditions are joined.
    pub condition_match: String,
    pub conditions: String,
    pub actions: String,
    pub last_run_at: Option<i64>,
    /// What was true at the last run (`Text::store`).
    pub last_reason: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    /// JSON: the actions run when the conditions are false.
    pub else_actions: String,
    /// "single" | "restart".
    pub run_mode: String,
    /// JSON: the actions run when the lasting trigger stops holding.
    pub over_actions: String,
}

pub async fn list_automations(db: &Db) -> sqlx::Result<Vec<AutomationRow>> {
    sqlx::query_as("SELECT * FROM automations ORDER BY name, id").fetch_all(db).await
}

pub async fn automation_by_id(db: &Db, id: i64) -> sqlx::Result<Option<AutomationRow>> {
    sqlx::query_as("SELECT * FROM automations WHERE id = ?").bind(id).fetch_optional(db).await
}

/// What is written of an automation (JSON as described in `automations`).
pub struct AutomationFields<'a> {
    pub name: &'a str,
    pub enabled: bool,
    pub trigger: &'a str,
    /// "all" or "any".
    pub joined: &'a str,
    pub conditions: &'a str,
    pub actions: &'a str,
    pub else_actions: &'a str,
    pub run_mode: &'a str,
    pub over_actions: &'a str,
}

pub async fn insert_automation(db: &Db, f: &AutomationFields<'_>) -> sqlx::Result<AutomationRow> {
    let t = now();
    sqlx::query_as("INSERT INTO automations (name, enabled, trigger_def, condition_match, conditions, actions, else_actions, run_mode, over_actions, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING *")
        .bind(f.name)
        .bind(f.enabled)
        .bind(f.trigger)
        .bind(f.joined)
        .bind(f.conditions)
        .bind(f.actions)
        .bind(f.else_actions)
        .bind(f.run_mode)
        .bind(f.over_actions)
        .bind(t)
        .bind(t)
        .fetch_one(db)
        .await
}

pub async fn update_automation(db: &Db, id: i64, f: &AutomationFields<'_>) -> sqlx::Result<()> {
    sqlx::query("UPDATE automations SET name = ?, enabled = ?, trigger_def = ?, condition_match = ?, conditions = ?, actions = ?, else_actions = ?, run_mode = ?, over_actions = ?, updated_at = ? WHERE id = ?")
        .bind(f.name)
        .bind(f.enabled)
        .bind(f.trigger)
        .bind(f.joined)
        .bind(f.conditions)
        .bind(f.actions)
        .bind(f.else_actions)
        .bind(f.run_mode)
        .bind(f.over_actions)
        .bind(now())
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn set_automation_enabled(db: &Db, id: i64, enabled: bool) -> sqlx::Result<()> {
    sqlx::query("UPDATE automations SET enabled = ?, updated_at = ? WHERE id = ?").bind(enabled).bind(now()).bind(id).execute(db).await?;
    Ok(())
}

pub async fn set_automation_run(db: &Db, id: i64, ts: i64, reason: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE automations SET last_run_at = ?, last_reason = ? WHERE id = ?").bind(ts).bind(reason).bind(id).execute(db).await?;
    Ok(())
}

/// A run of an automation, or a time it did not run.
#[derive(Debug, Clone, FromRow)]
pub struct AutomationRun {
    pub id: i64,
    pub automation_id: i64,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub branch: String,
    /// `Text::store`.
    pub reason: String,
    pub ended: Option<String>,
    /// JSON list of the steps done.
    pub steps: String,
}

/// Runs kept per automation.
const RUNS_KEPT: i64 = 20;

/// Records a run (`finished`: at once, for one that did not happen) and forgets the oldest.
pub async fn insert_automation_run(db: &Db, automation_id: i64, branch: &str, reason: &str, finished: bool) -> sqlx::Result<i64> {
    let t = now();
    let id: i64 = sqlx::query_scalar("INSERT INTO automation_runs (automation_id, started_at, finished_at, branch, reason, ended) VALUES (?, ?, ?, ?, ?, ?) RETURNING id")
        .bind(automation_id)
        .bind(t)
        .bind(finished.then_some(t))
        .bind(branch)
        .bind(reason)
        .bind(finished.then_some("done"))
        .fetch_one(db)
        .await?;
    sqlx::query("DELETE FROM automation_runs WHERE automation_id = ? AND id NOT IN (SELECT id FROM automation_runs WHERE automation_id = ? ORDER BY id DESC LIMIT ?)")
        .bind(automation_id)
        .bind(automation_id)
        .bind(RUNS_KEPT)
        .execute(db)
        .await?;
    Ok(id)
}

pub async fn finish_automation_run(db: &Db, id: i64, ended: &str, steps: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE automation_runs SET finished_at = ?, ended = ?, steps = ? WHERE id = ?").bind(now()).bind(ended).bind(steps).bind(id).execute(db).await?;
    Ok(())
}

/// Ends a run that is cut short (started again), keeping the steps it did.
pub async fn finish_automation_run_ended(db: &Db, id: i64, ended: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE automation_runs SET finished_at = ?, ended = ? WHERE id = ?").bind(now()).bind(ended).bind(id).execute(db).await?;
    Ok(())
}

pub async fn set_automation_run_steps(db: &Db, id: i64, steps: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE automation_runs SET steps = ? WHERE id = ?").bind(steps).bind(id).execute(db).await?;
    Ok(())
}

/// Runs cut short by a stop of the server.
pub async fn interrupt_automation_runs(db: &Db) -> sqlx::Result<()> {
    sqlx::query("UPDATE automation_runs SET finished_at = started_at, ended = 'interrupted' WHERE finished_at IS NULL").execute(db).await?;
    Ok(())
}

pub async fn automation_runs(db: &Db, automation_id: i64) -> sqlx::Result<Vec<AutomationRun>> {
    sqlx::query_as("SELECT * FROM automation_runs WHERE automation_id = ? ORDER BY id DESC").bind(automation_id).fetch_all(db).await
}

/// Whether a user is at home, as they last told it.
#[derive(Debug, Clone, FromRow, serde::Serialize)]
pub struct Presence {
    pub user_id: i64,
    pub username: String,
    pub home: bool,
    pub changed_at: i64,
}

pub async fn list_presence(db: &Db) -> sqlx::Result<Vec<Presence>> {
    sqlx::query_as("SELECT p.user_id, u.username, p.home, p.changed_at FROM presence p JOIN users u ON u.id = p.user_id ORDER BY u.username").fetch_all(db).await
}

/// Records whether a user is at home; `true` when it changed.
pub async fn set_presence(db: &Db, user_id: i64, home: bool) -> sqlx::Result<bool> {
    let changed = sqlx::query("INSERT INTO presence (user_id, home, changed_at) VALUES (?, ?, ?) ON CONFLICT (user_id) DO UPDATE SET home = excluded.home, changed_at = excluded.changed_at WHERE presence.home <> excluded.home")
        .bind(user_id)
        .bind(home)
        .bind(now())
        .execute(db)
        .await?
        .rows_affected();
    Ok(changed > 0)
}

/// A load run on the solar surplus (see `surplus`).
#[derive(Debug, Clone, FromRow, Serialize, serde::Deserialize, PartialEq)]
pub struct SurplusLoadRow {
    pub device_id: i64,
    pub position: i64,
    pub enabled: bool,
    pub min_amps: Option<i64>,
    pub max_amps: Option<i64>,
    pub power: Option<i64>,
    pub guarantee_kwh: Option<f64>,
    pub guarantee_by: Option<String>,
    pub min_on_minutes: i64,
    pub tolerance: i64,
    /// A zone of the air conditioning: the setpoints it is given, and the outdoor temperatures
    /// below which it heats, above which it cools.
    #[serde(default)]
    pub heat_setpoint: Option<f64>,
    #[serde(default)]
    pub cool_setpoint: Option<f64>,
    #[serde(default)]
    pub heat_below: Option<f64>,
    #[serde(default)]
    pub cool_above: Option<f64>,
}

impl SurplusLoadRow {
    /// A charger as the surplus takes it by default (its own limits), for one that follows a
    /// charging profile without being in the list of the surplus.
    pub fn charger(device_id: i64) -> SurplusLoadRow {
        SurplusLoadRow {
            device_id,
            position: i64::MAX,
            enabled: true,
            min_amps: None,
            max_amps: None,
            power: None,
            guarantee_kwh: None,
            guarantee_by: None,
            min_on_minutes: 0,
            tolerance: 0,
            heat_setpoint: None,
            cool_setpoint: None,
            heat_below: None,
            cool_above: None,
        }
    }
}

pub async fn list_surplus_loads(db: &Db) -> sqlx::Result<Vec<SurplusLoadRow>> {
    sqlx::query_as(
        "SELECT device_id, position, enabled, min_amps, max_amps, power, guarantee_kwh, guarantee_by, min_on_minutes, tolerance, heat_setpoint, cool_setpoint, heat_below, cool_above \
         FROM surplus_loads ORDER BY position, device_id",
    )
    .fetch_all(db)
    .await
}

pub async fn upsert_surplus_load(db: &Db, l: &SurplusLoadRow) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO surplus_loads (device_id, position, enabled, min_amps, max_amps, power, guarantee_kwh, guarantee_by, min_on_minutes, tolerance, created_at,
           heat_setpoint, cool_setpoint, heat_below, cool_above)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (device_id) DO UPDATE SET position = excluded.position, enabled = excluded.enabled, min_amps = excluded.min_amps, max_amps = excluded.max_amps,
           power = excluded.power, guarantee_kwh = excluded.guarantee_kwh, guarantee_by = excluded.guarantee_by, min_on_minutes = excluded.min_on_minutes, tolerance = excluded.tolerance,
           heat_setpoint = excluded.heat_setpoint, cool_setpoint = excluded.cool_setpoint, heat_below = excluded.heat_below, cool_above = excluded.cool_above",
    )
    .bind(l.device_id)
    .bind(l.position)
    .bind(l.enabled)
    .bind(l.min_amps)
    .bind(l.max_amps)
    .bind(l.power)
    .bind(l.guarantee_kwh)
    .bind(&l.guarantee_by)
    .bind(l.min_on_minutes)
    .bind(l.tolerance)
    .bind(now())
    .bind(l.heat_setpoint)
    .bind(l.cool_setpoint)
    .bind(l.heat_below)
    .bind(l.cool_above)
    .execute(db)
    .await?;
    Ok(())
}

/// A load of the surplus follows it, or no longer does (`false`: not in the list).
pub async fn set_surplus_load_enabled(db: &Db, device_id: i64, enabled: bool) -> sqlx::Result<bool> {
    Ok(sqlx::query("UPDATE surplus_loads SET enabled = ? WHERE device_id = ?").bind(enabled).bind(device_id).execute(db).await?.rows_affected() > 0)
}

pub async fn delete_surplus_load(db: &Db, device_id: i64) -> sqlx::Result<bool> {
    Ok(sqlx::query("DELETE FROM surplus_loads WHERE device_id = ?").bind(device_id).execute(db).await?.rows_affected() > 0)
}

pub async fn delete_automation(db: &Db, id: i64) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM automations WHERE id = ?").bind(id).execute(db).await?;
    Ok(())
}
