//! Airzone schedules (mirror of Airzone Cloud).

use super::*;

#[derive(Debug, Clone, FromRow)]
pub struct AirzoneScheduleRow {
    pub cloud_id: String,
    pub sched_id: String,
    pub data: String,
    pub zones: String,
    pub synced_at: i64,
}

pub async fn list_airzone_schedules(db: &Db) -> sqlx::Result<Vec<AirzoneScheduleRow>> {
    sqlx::query_as("SELECT * FROM airzone_schedules").fetch_all(db).await
}

/// Replaces the schedules of one cloud installation: rows = (schedule id, JSON, zones JSON).
pub async fn replace_airzone_schedules(db: &Db, cloud_id: &str, rows: &[(String, String, String)]) -> sqlx::Result<()> {
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM airzone_schedules WHERE cloud_id = ?").bind(cloud_id).execute(&mut *tx).await?;
    for (id, data, zones) in rows {
        sqlx::query("INSERT OR REPLACE INTO airzone_schedules (cloud_id, sched_id, data, zones, synced_at) VALUES (?, ?, ?, ?, ?)")
            .bind(cloud_id)
            .bind(id)
            .bind(data)
            .bind(zones)
            .bind(now())
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Forgets the schedules of cloud installations no longer in the account.
pub async fn retain_airzone_schedules(db: &Db, cloud_ids: &[String]) -> sqlx::Result<()> {
    for row in list_airzone_schedules(db).await? {
        if !cloud_ids.contains(&row.cloud_id) {
            sqlx::query("DELETE FROM airzone_schedules WHERE cloud_id = ?").bind(&row.cloud_id).execute(db).await?;
        }
    }
    Ok(())
}

pub async fn clear_airzone_schedules(db: &Db) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM airzone_schedules").execute(db).await?;
    Ok(())
}

/// Updates the stored organisation (zones, device ids) of a cloud installation.
pub async fn set_cloud_detail(db: &Db, cloud_id: &str, detail: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE cloud_installations SET detail = ? WHERE cloud_id = ?").bind(detail).bind(cloud_id).execute(db).await?;
    Ok(())
}
