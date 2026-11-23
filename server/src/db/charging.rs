//! The charging profiles and what each charger follows (see `charging`).

use super::*;

#[derive(Debug, Clone, FromRow)]
pub struct ChargeProfileRow {
    pub id: i64,
    pub name: String,
    /// `charging::Plan`, as JSON.
    pub plan: String,
}

pub async fn list_charge_profiles(db: &Db) -> sqlx::Result<Vec<ChargeProfileRow>> {
    sqlx::query_as("SELECT id, name, plan FROM charge_profiles ORDER BY name COLLATE NOCASE, id").fetch_all(db).await
}

pub async fn insert_charge_profile(db: &Db, name: &str, plan: &str) -> sqlx::Result<i64> {
    let at = now();
    let r = sqlx::query("INSERT INTO charge_profiles (name, plan, created_at, updated_at) VALUES (?, ?, ?, ?)").bind(name).bind(plan).bind(at).bind(at).execute(db).await?;
    Ok(r.last_insert_rowid())
}

pub async fn update_charge_profile(db: &Db, id: i64, name: &str, plan: &str) -> sqlx::Result<bool> {
    let r = sqlx::query("UPDATE charge_profiles SET name = ?, plan = ?, updated_at = ? WHERE id = ?").bind(name).bind(plan).bind(now()).bind(id).execute(db).await?;
    Ok(r.rows_affected() > 0)
}

pub async fn delete_charge_profile(db: &Db, id: i64) -> sqlx::Result<bool> {
    Ok(sqlx::query("DELETE FROM charge_profiles WHERE id = ?").bind(id).execute(db).await?.rows_affected() > 0)
}

/// The chargers of the modules: id and name.
pub async fn chargers(db: &Db) -> sqlx::Result<Vec<(i64, String)>> {
    sqlx::query_as("SELECT id, name FROM devices WHERE kind = 'ev.charger' ORDER BY id").fetch_all(db).await
}

/// What a charger follows: a profile, and a charge asked now.
#[derive(Debug, Clone, Default, FromRow, PartialEq)]
pub struct ChargerPlanRow {
    pub device_id: i64,
    pub profile_id: Option<i64>,
    pub now_amps: Option<i64>,
    pub now_kw: Option<f64>,
    pub now_until: Option<i64>,
    pub now_kwh: Option<f64>,
    pub now_base_energy: Option<f64>,
}

macro_rules! plan_query {
    ($rest:literal) => {
        concat!("SELECT device_id, profile_id, now_amps, now_kw, now_until, now_kwh, now_base_energy FROM charger_plans", $rest)
    };
}

pub async fn list_charger_plans(db: &Db) -> sqlx::Result<Vec<ChargerPlanRow>> {
    sqlx::query_as(plan_query!(" ORDER BY device_id")).fetch_all(db).await
}

pub async fn charger_plan(db: &Db, device_id: i64) -> sqlx::Result<Option<ChargerPlanRow>> {
    sqlx::query_as(plan_query!(" WHERE device_id = ?")).bind(device_id).fetch_optional(db).await
}

pub async fn set_charger_plan(db: &Db, p: &ChargerPlanRow) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO charger_plans (device_id, profile_id, now_amps, now_kw, now_until, now_kwh, now_base_energy, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT (device_id) DO UPDATE SET profile_id = excluded.profile_id, now_amps = excluded.now_amps, now_kw = excluded.now_kw, now_until = excluded.now_until, \
         now_kwh = excluded.now_kwh, now_base_energy = excluded.now_base_energy, updated_at = excluded.updated_at",
    )
    .bind(p.device_id)
    .bind(p.profile_id)
    .bind(p.now_amps)
    .bind(p.now_kw)
    .bind(p.now_until)
    .bind(p.now_kwh)
    .bind(p.now_base_energy)
    .bind(now())
    .execute(db)
    .await?;
    Ok(())
}

/// The charge asked now is over (or called off): the charger follows its profile again.
pub async fn clear_charge_now(db: &Db, device_id: i64) -> sqlx::Result<bool> {
    let r = sqlx::query(
        "UPDATE charger_plans SET now_amps = NULL, now_kw = NULL, now_until = NULL, now_kwh = NULL, now_base_energy = NULL, updated_at = ? WHERE device_id = ? AND now_amps IS NOT NULL",
    )
    .bind(now())
    .bind(device_id)
    .execute(db)
    .await?;
    Ok(r.rows_affected() > 0)
}
