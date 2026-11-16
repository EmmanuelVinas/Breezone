//! Timers and Airzone routines.

use super::*;

#[derive(Debug, Clone, FromRow)]
pub struct TimerRow {
    pub id: i64,
    pub label: String,
    pub run_at: i64,
    pub target: String,
    pub command: String,
    pub created_by: Option<String>,
    pub created_at: i64,
}

pub async fn insert_timer(db: &Db, label: &str, run_at: i64, target: &str, command: &str, created_by: &str) -> sqlx::Result<TimerRow> {
    sqlx::query_as("INSERT INTO timers (label, run_at, target, command, created_by, created_at) VALUES (?, ?, ?, ?, ?, ?) RETURNING *")
        .bind(label)
        .bind(run_at)
        .bind(target)
        .bind(command)
        .bind(created_by)
        .bind(now())
        .fetch_one(db)
        .await
}

pub async fn list_timers(db: &Db) -> sqlx::Result<Vec<TimerRow>> {
    sqlx::query_as("SELECT * FROM timers ORDER BY run_at").fetch_all(db).await
}

pub async fn timer_by_id(db: &Db, id: i64) -> sqlx::Result<Option<TimerRow>> {
    sqlx::query_as("SELECT * FROM timers WHERE id = ?").bind(id).fetch_optional(db).await
}

pub async fn due_timers(db: &Db, now: i64) -> sqlx::Result<Vec<TimerRow>> {
    sqlx::query_as("SELECT * FROM timers WHERE run_at <= ? ORDER BY run_at").bind(now).fetch_all(db).await
}

pub async fn delete_timer(db: &Db, id: i64) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM timers WHERE id = ?").bind(id).execute(db).await?;
    Ok(())
}

pub async fn set_airzone_routines(db: &Db, cloud_id: &str, data: &str) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO airzone_routines (cloud_id, data, synced_at) VALUES (?, ?, ?) ON CONFLICT (cloud_id) DO UPDATE SET data = excluded.data, synced_at = excluded.synced_at",
    )
    .bind(cloud_id)
    .bind(data)
    .bind(now())
    .execute(db)
    .await?;
    Ok(())
}

pub async fn list_airzone_routines(db: &Db) -> sqlx::Result<Vec<(String, String)>> {
    sqlx::query_as("SELECT cloud_id, data FROM airzone_routines").fetch_all(db).await
}
