//! Device history.

use super::*;

/// A number of a device's state at a point of its history.
#[derive(Debug, Clone, PartialEq, FromRow)]
pub struct DeviceSample {
    pub device_id: i64,
    pub ts: i64,
    pub field: String,
    pub value: f64,
}

/// Id of each device of the configured sources, by `source_id/reference`.
pub async fn source_device_ids(db: &Db) -> sqlx::Result<HashMap<String, i64>> {
    let rows: Vec<(String, i64)> = sqlx::query_as("SELECT source_ref, id FROM devices WHERE source_id IS NOT NULL").fetch_all(db).await?;
    Ok(rows.into_iter().collect())
}

pub async fn insert_device_samples(db: &Db, samples: &[DeviceSample]) -> sqlx::Result<()> {
    let mut tx = db.begin().await?;
    for s in samples {
        sqlx::query("INSERT OR REPLACE INTO device_samples (device_id, ts, field, value) VALUES (?, ?, ?, ?)")
            .bind(s.device_id)
            .bind(s.ts)
            .bind(&s.field)
            .bind(s.value)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await
}

/// Adds history points where there is none yet (what the hardware kept of a gap): a point
/// Breezone recorded is never replaced.
pub async fn add_device_samples(db: &Db, samples: &[DeviceSample]) -> sqlx::Result<u64> {
    let mut tx = db.begin().await?;
    let mut added = 0;
    for s in samples {
        added += sqlx::query("INSERT OR IGNORE INTO device_samples (device_id, ts, field, value) VALUES (?, ?, ?, ?)")
            .bind(s.device_id)
            .bind(s.ts)
            .bind(&s.field)
            .bind(s.value)
            .execute(&mut *tx)
            .await?
            .rows_affected();
    }
    tx.commit().await?;
    Ok(added)
}

/// History of some devices from `since` to `until` (excluded), sorted by device then time; every
/// field when `fields` is empty.
pub async fn device_samples(db: &Db, devices: &[i64], fields: &[&str], since: i64, until: i64) -> sqlx::Result<Vec<DeviceSample>> {
    let mut out = vec![];
    for d in devices {
        let rows: Vec<DeviceSample> = sqlx::query_as("SELECT device_id, ts, field, value FROM device_samples WHERE device_id = ? AND ts >= ? AND ts < ? ORDER BY ts")
            .bind(d)
            .bind(since)
            .bind(until)
            .fetch_all(db)
            .await?;
        out.extend(rows.into_iter().filter(|r| fields.is_empty() || fields.contains(&r.field.as_str())));
    }
    Ok(out)
}

/// The fields of the devices of the modules that have a history: those summed up by the hour, and
/// those read since `since` (not summed up yet).
pub async fn charted_fields(db: &Db, since: i64) -> sqlx::Result<Vec<(i64, String)>> {
    sqlx::query_as(
        "SELECT device_id, field FROM device_hourly GROUP BY device_id, field \
         UNION SELECT device_id, field FROM device_samples WHERE ts >= ? GROUP BY device_id, field",
    )
    .bind(since)
    .fetch_all(db)
    .await
}
