//! The charging profiles and what each charger follows (see `charging`).

use super::*;

#[derive(Debug, Clone, FromRow, PartialEq)]
pub struct ChargeProfileRow {
    pub id: i64,
    pub name: String,
    /// Up to this power (`None`: the charger's maximum).
    pub kw: Option<f64>,
    /// A colour for the screens ("#14b8a6").
    pub color: Option<String>,
}

pub async fn list_charge_profiles(db: &Db) -> sqlx::Result<Vec<ChargeProfileRow>> {
    sqlx::query_as("SELECT id, name, kw, color FROM charge_profiles ORDER BY kw IS NULL, kw, name COLLATE NOCASE, id").fetch_all(db).await
}

pub async fn insert_charge_profile(db: &Db, name: &str, kw: Option<f64>, color: Option<&str>) -> sqlx::Result<i64> {
    let at = now();
    let r = sqlx::query("INSERT INTO charge_profiles (name, kw, color, created_at, updated_at) VALUES (?, ?, ?, ?, ?)")
        .bind(name)
        .bind(kw)
        .bind(color)
        .bind(at)
        .bind(at)
        .execute(db)
        .await?;
    Ok(r.last_insert_rowid())
}

pub async fn update_charge_profile(db: &Db, id: i64, name: &str, kw: Option<f64>, color: Option<&str>) -> sqlx::Result<bool> {
    let r =
        sqlx::query("UPDATE charge_profiles SET name = ?, kw = ?, color = ?, updated_at = ? WHERE id = ?").bind(name).bind(kw).bind(color).bind(now()).bind(id).execute(db).await?;
    Ok(r.rows_affected() > 0)
}

/// The chargers that followed it are left alone.
pub async fn delete_charge_profile(db: &Db, id: i64) -> sqlx::Result<bool> {
    let mut tx = db.begin().await?;
    sqlx::query("UPDATE charger_plans SET mode = NULL, profile_id = NULL, automation_id = NULL, updated_at = ? WHERE profile_id = ?")
        .bind(now())
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let gone = sqlx::query("DELETE FROM charge_profiles WHERE id = ?").bind(id).execute(&mut *tx).await?.rows_affected() > 0;
    tx.commit().await?;
    Ok(gone)
}

/// The chargers of the modules: id and name.
pub async fn chargers(db: &Db) -> sqlx::Result<Vec<(i64, String)>> {
    sqlx::query_as("SELECT id, name FROM devices WHERE kind = 'ev.charger' ORDER BY id").fetch_all(db).await
}

/// What a charger follows (`mode`: "profile" with `profile_id`, "off", "solar"; none: nothing), who
/// set it, and a charge asked now.
#[derive(Debug, Clone, Default, FromRow, PartialEq)]
pub struct ChargerPlanRow {
    pub device_id: i64,
    pub mode: Option<String>,
    pub profile_id: Option<i64>,
    /// The automation that set `mode` (`None`: by hand).
    pub automation_id: Option<i64>,
    /// `charging::Solar`, as JSON (`mode` "solar").
    pub solar: Option<String>,
    pub now_amps: Option<i64>,
    pub now_kw: Option<f64>,
    pub now_until: Option<i64>,
    pub now_kwh: Option<f64>,
    pub now_base_energy: Option<f64>,
}

macro_rules! plan_query {
    ($rest:literal) => {
        concat!("SELECT device_id, mode, profile_id, automation_id, solar, now_amps, now_kw, now_until, now_kwh, now_base_energy FROM charger_plans", $rest)
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
        "INSERT INTO charger_plans (device_id, mode, profile_id, automation_id, solar, now_amps, now_kw, now_until, now_kwh, now_base_energy, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT (device_id) DO UPDATE SET mode = excluded.mode, profile_id = excluded.profile_id, automation_id = excluded.automation_id, solar = excluded.solar, \
         now_amps = excluded.now_amps, now_kw = excluded.now_kw, now_until = excluded.now_until, now_kwh = excluded.now_kwh, \
         now_base_energy = excluded.now_base_energy, updated_at = excluded.updated_at",
    )
    .bind(p.device_id)
    .bind(&p.mode)
    .bind(p.profile_id)
    .bind(p.automation_id)
    .bind(&p.solar)
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

/// The charge asked now is over (or called off): the charger follows what it followed again.
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
