//! History.

use super::*;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Reading {
    pub ts: i64,
    pub room_temp: Option<f64>,
    pub humidity: Option<i64>,
    pub setpoint: Option<f64>,
    pub is_on: Option<bool>,
    pub mode: Option<i64>,
    /// Seconds in demand since the previous point (`None`: not recorded).
    pub demand_secs: Option<i64>,
    /// Air quality read from the zone (`aq_quality`; `None`: no sensor).
    pub air_quality: Option<i64>,
}

#[allow(clippy::too_many_arguments)]
pub async fn insert_reading(
    db: &Db,
    installation_id: i64,
    system_id: i64,
    zone_id: i64,
    room_temp: Option<f64>,
    humidity: Option<i64>,
    setpoint: Option<f64>,
    is_on: bool,
    mode: Option<i64>,
    demand_secs: Option<i64>,
    air_quality: Option<i64>,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO readings (installation_id, system_id, zone_id, ts, room_temp, humidity, setpoint, is_on, mode, demand_secs, air_quality)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(installation_id)
    .bind(system_id)
    .bind(zone_id)
    .bind(now())
    .bind(room_temp)
    .bind(humidity)
    .bind(setpoint)
    .bind(is_on)
    .bind(mode)
    .bind(demand_secs)
    .bind(air_quality)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn readings(db: &Db, installation_id: i64, system_id: i64, zone_id: i64, since: i64) -> sqlx::Result<Vec<Reading>> {
    sqlx::query_as(
        "SELECT ts, room_temp, humidity, setpoint, is_on, mode, demand_secs, air_quality FROM readings
         WHERE installation_id = ? AND system_id = ? AND zone_id = ? AND ts >= ? ORDER BY ts",
    )
    .bind(installation_id)
    .bind(system_id)
    .bind(zone_id)
    .bind(since)
    .fetch_all(db)
    .await
}

/// Average room temperature of a zone over one time bucket.
#[derive(Debug, Clone, FromRow)]
pub struct TrendPoint {
    pub system_id: i64,
    pub zone_id: i64,
    /// Start of the bucket.
    pub bucket: i64,
    pub room_temp: f64,
}

/// Room temperature of every zone of an installation since `since`, averaged per `bucket` seconds.
pub async fn temperature_trends(db: &Db, installation_id: i64, since: i64, bucket: i64) -> sqlx::Result<Vec<TrendPoint>> {
    sqlx::query_as(
        "SELECT system_id, zone_id, (ts / ?1) * ?1 AS bucket, AVG(room_temp) AS room_temp FROM readings
         WHERE installation_id = ?2 AND ts >= ?3 AND room_temp IS NOT NULL
         GROUP BY system_id, zone_id, bucket ORDER BY system_id, zone_id, bucket",
    )
    .bind(bucket.max(1))
    .bind(installation_id)
    .bind(since)
    .fetch_all(db)
    .await
}
