//! Routines.

use super::*;

#[derive(Debug, Clone, FromRow)]
pub struct RoutineRow {
    pub id: i64,
    pub name: String,
    pub enabled: bool,
    pub days: i64,
    pub time: Option<String>,
    pub actions: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_run_at: Option<i64>,
    /// "airzone:…" when imported from an Airzone schedule.
    pub origin: Option<String>,
}

pub async fn list_routines(db: &Db) -> sqlx::Result<Vec<RoutineRow>> {
    sqlx::query_as("SELECT * FROM routines ORDER BY time IS NULL, time, name").fetch_all(db).await
}

pub async fn routine_by_id(db: &Db, id: i64) -> sqlx::Result<Option<RoutineRow>> {
    sqlx::query_as("SELECT * FROM routines WHERE id = ?").bind(id).fetch_optional(db).await
}

pub async fn insert_routine(db: &Db, name: &str, enabled: bool, days: i64, time: Option<&str>, actions: &str) -> sqlx::Result<RoutineRow> {
    let t = now();
    sqlx::query_as("INSERT INTO routines (name, enabled, days, time, actions, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING *")
        .bind(name)
        .bind(enabled)
        .bind(days)
        .bind(time)
        .bind(actions)
        .bind(t)
        .bind(t)
        .fetch_one(db)
        .await
}

pub async fn update_routine(db: &Db, id: i64, name: &str, enabled: bool, days: i64, time: Option<&str>, actions: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE routines SET name = ?, enabled = ?, days = ?, time = ?, actions = ?, updated_at = ? WHERE id = ?")
        .bind(name)
        .bind(enabled)
        .bind(days)
        .bind(time)
        .bind(actions)
        .bind(now())
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn set_routine_last_run(db: &Db, id: i64, ts: i64) -> sqlx::Result<()> {
    sqlx::query("UPDATE routines SET last_run_at = ? WHERE id = ?").bind(ts).bind(id).execute(db).await?;
    Ok(())
}

pub async fn delete_routine(db: &Db, id: i64) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM routines WHERE id = ?").bind(id).execute(db).await?;
    Ok(())
}
