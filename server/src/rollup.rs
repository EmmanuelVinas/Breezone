//! The long history of the devices: the detail of `device_samples` (every few minutes, kept a few
//! weeks) is summed up per hour (`device_hourly`, about a year) and per local day (`device_daily`,
//! for good). A curve over a long period reads the summary that fits it, so that the database stays
//! small and quick however long it runs.
//!
//! Old rows are deleted in small batches: one big delete would hold the database for seconds,
//! during which the other writes wait (and may give up: « database is locked »).

use crate::db::{self, Db};
use chrono::{Days, NaiveDate, TimeZone};
use chrono_tz::Tz;
// Dynamic SQL here only joins fixed table names and the field names of the trait catalog.
use sqlx::AssertSqlSafe;
use std::time::Duration;

const HOUR: i64 = 3600;
/// Hourly summaries kept (days).
pub const HOURLY_DAYS: i64 = 400;
/// Rows deleted per batch, and the pause between two batches.
const BATCH: i64 = 2000;
const PAUSE: Duration = Duration::from_millis(30);
/// Hours summed up per transaction.
const HOURS_PER_STEP: i64 = 24;

const NEXT_HOUR: &str = "rollup.next_hour";
const NEXT_DAY: &str = "rollup.next_day";
const NEXT_ZONE_DAY: &str = "rollup.next_zone_day";

/// `CASE` picking the last value for a counter, the mean otherwise.
fn value_sql() -> String {
    let counters: Vec<String> =
        crate::traits::catalog().iter().flat_map(|t| &t.state).filter(|f| f.unit.as_deref() == Some("kWh")).map(|f| format!("'{}'", f.name.replace('\'', ""))).collect();
    if counters.is_empty() {
        return "AVG(value)".into();
    }
    format!("CASE WHEN field IN ({}) THEN MAX(value) ELSE AVG(value) END", counters.join(", "))
}

pub fn midnight(day: NaiveDate, tz: Tz) -> i64 {
    day.and_hms_opt(0, 0, 0).and_then(|t| tz.from_local_datetime(&t).earliest()).map_or(0, |t| t.timestamp())
}

pub fn local_day(ts: i64, tz: Tz) -> NaiveDate {
    chrono::DateTime::from_timestamp(ts, 0).unwrap_or_default().with_timezone(&tz).date_naive()
}

/// Sums up what is complete: the hours before the current one, then the local days before today.
pub async fn roll_up(db: &Db, tz: Tz, now: i64) -> sqlx::Result<()> {
    let value = value_sql();
    let current_hour = now.div_euclid(HOUR) * HOUR;
    let mut next = match db::get_setting(db, NEXT_HOUR).await?.and_then(|v| v.parse::<i64>().ok()) {
        Some(h) => h,
        None => match sqlx::query_scalar::<_, Option<i64>>("SELECT MIN(ts) FROM device_samples").fetch_one(db).await? {
            Some(first) => first.div_euclid(HOUR) * HOUR,
            None => current_hour,
        },
    };
    let hourly = format!(
        "INSERT OR REPLACE INTO device_hourly (device_id, field, ts, value)
         SELECT device_id, field, (ts / {HOUR}) * {HOUR}, {value} FROM device_samples WHERE ts >= ? AND ts < ? GROUP BY device_id, field, ts / {HOUR}"
    );
    while next < current_hour {
        let end = (next + HOURS_PER_STEP * HOUR).min(current_hour);
        let mut tx = db.begin().await?;
        sqlx::query(AssertSqlSafe(hourly.as_str())).bind(next).bind(end).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT (key) DO UPDATE SET value = excluded.value")
            .bind(NEXT_HOUR)
            .bind(end.to_string())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        next = end;
        tokio::time::sleep(PAUSE).await;
    }

    // Days: from the hours, once every hour of the day is summed up.
    let today = local_day(now, tz);
    let mut day = match db::get_setting(db, NEXT_DAY).await?.and_then(|v| NaiveDate::parse_from_str(&v, "%Y-%m-%d").ok()) {
        Some(d) => d,
        None => match sqlx::query_scalar::<_, Option<i64>>("SELECT MIN(ts) FROM device_hourly").fetch_one(db).await? {
            Some(first) => local_day(first, tz),
            None => today,
        },
    };
    let daily = format!(
        "INSERT OR REPLACE INTO device_daily (device_id, field, ts, value)
         SELECT device_id, field, ?, {value} FROM device_hourly WHERE ts >= ? AND ts < ? GROUP BY device_id, field"
    );
    while day < today {
        let (start, end) = (midnight(day, tz), midnight(day + Days::new(1), tz));
        if end > current_hour {
            break;
        }
        let mut tx = db.begin().await?;
        sqlx::query(AssertSqlSafe(daily.as_str())).bind(start).bind(start).bind(end).execute(&mut *tx).await?;
        let after = day + Days::new(1);
        sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT (key) DO UPDATE SET value = excluded.value")
            .bind(NEXT_DAY)
            .bind(after.format("%Y-%m-%d").to_string())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        day = after;
        tokio::time::sleep(PAUSE).await;
    }
    roll_up_zones(db, tz, now).await
}

/// The days of the Airzone zones, from their readings, once the day is over.
async fn roll_up_zones(db: &Db, tz: Tz, now: i64) -> sqlx::Result<()> {
    let today = local_day(now, tz);
    let mut day = match db::get_setting(db, NEXT_ZONE_DAY).await?.and_then(|v| NaiveDate::parse_from_str(&v, "%Y-%m-%d").ok()) {
        Some(d) => d,
        None => match sqlx::query_scalar::<_, Option<i64>>("SELECT MIN(ts) FROM readings").fetch_one(db).await? {
            Some(first) => local_day(first, tz),
            None => today,
        },
    };
    while day < today {
        let (start, end) = (midnight(day, tz), midnight(day + Days::new(1), tz));
        let mut tx = db.begin().await?;
        sqlx::query(
            "INSERT OR REPLACE INTO zone_daily (installation_id, system_id, zone_id, ts, room_temp, humidity, setpoint, on_ratio, demand_secs, air_quality)
             SELECT installation_id, system_id, zone_id, ?, AVG(room_temp), AVG(humidity), AVG(CASE WHEN is_on THEN setpoint END),
                    AVG(CASE WHEN is_on THEN 1.0 WHEN is_on IS NULL THEN NULL ELSE 0.0 END), SUM(demand_secs), AVG(air_quality)
             FROM readings WHERE ts >= ? AND ts < ? GROUP BY installation_id, system_id, zone_id",
        )
        .bind(start)
        .bind(start)
        .bind(end)
        .execute(&mut *tx)
        .await?;
        let after = day + Days::new(1);
        sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT (key) DO UPDATE SET value = excluded.value")
            .bind(NEXT_ZONE_DAY)
            .bind(after.format("%Y-%m-%d").to_string())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        day = after;
        tokio::time::sleep(PAUSE).await;
    }
    Ok(())
}

/// A row of `zone_daily`: ts, room temperature, humidity, setpoint, share on, demand, air quality.
type ZoneDay = (i64, Option<f64>, Option<f64>, Option<f64>, Option<f64>, Option<i64>, Option<f64>);

/// The days of a zone since `since`, as readings (on when it was on most of the day).
pub async fn zone_days(db: &Db, installation_id: i64, system_id: i64, zone_id: i64, since: i64) -> sqlx::Result<Vec<db::Reading>> {
    let rows: Vec<ZoneDay> = sqlx::query_as(
        "SELECT ts, room_temp, humidity, setpoint, on_ratio, demand_secs, air_quality FROM zone_daily
         WHERE installation_id = ? AND system_id = ? AND zone_id = ? AND ts >= ? ORDER BY ts",
    )
    .bind(installation_id)
    .bind(system_id)
    .bind(zone_id)
    .bind(since)
    .fetch_all(db)
    .await?;
    let round = |v: Option<f64>| v.map(|v| (v * 10.0).round() / 10.0);
    Ok(rows
        .into_iter()
        .map(|(ts, room_temp, humidity, setpoint, on, demand_secs, air)| db::Reading {
            ts,
            room_temp: round(room_temp),
            humidity: humidity.map(|h| h.round() as i64),
            setpoint: round(setpoint),
            is_on: on.map(|r| r >= 0.5),
            mode: None,
            demand_secs,
            air_quality: air.map(|a| a.round() as i64),
        })
        .collect())
}

/// The oldest point of a zone, its days included.
pub async fn zone_oldest(db: &Db, installation_id: i64, system_id: i64, zone_id: i64) -> sqlx::Result<Option<i64>> {
    let mut oldest: Option<i64> = None;
    for table in ["zone_daily", "readings"] {
        let t: Option<i64> = sqlx::query_scalar(AssertSqlSafe(format!("SELECT MIN(ts) FROM {table} WHERE installation_id = ? AND system_id = ? AND zone_id = ?")))
            .bind(installation_id)
            .bind(system_id)
            .bind(zone_id)
            .fetch_one(db)
            .await?;
        oldest = match (oldest, t) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
    }
    Ok(oldest)
}

/// Deletes the rows of `table` older than `before`, a batch at a time.
async fn delete_before(db: &Db, table: &str, key: &str, before: i64) -> sqlx::Result<u64> {
    let sql = format!("DELETE FROM {table} WHERE ({key}) IN (SELECT {key} FROM {table} WHERE ts < ? LIMIT {BATCH})");
    let mut total = 0;
    loop {
        let n = sqlx::query(AssertSqlSafe(sql.as_str())).bind(before).execute(db).await?.rows_affected();
        total += n;
        if n < BATCH as u64 {
            return Ok(total);
        }
        tokio::time::sleep(PAUSE).await;
    }
}

/// Deletes old history, events and expired sessions, in small batches. The detail is deleted only
/// once summed up.
pub async fn purge(db: &Db, tz: Tz, history_days: i64, events_days: i64) -> sqlx::Result<()> {
    let now = db::now();
    roll_up(db, tz, now).await?;
    let rolled = db::get_setting(db, NEXT_HOUR).await?.and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
    let detail = (now - history_days * 86_400).min(rolled);
    delete_before(db, "device_samples", "device_id, field, ts", detail).await?;
    delete_before(db, "device_hourly", "device_id, field, ts", now - HOURLY_DAYS * 86_400).await?;
    // The readings of a zone go once its day is summed up.
    let zones_rolled = db::get_setting(db, NEXT_ZONE_DAY).await?.and_then(|v| NaiveDate::parse_from_str(&v, "%Y-%m-%d").ok()).map_or(0, |d| midnight(d, tz));
    delete_before(db, "readings", "rowid", (now - history_days * 86_400).min(zones_rolled)).await?;
    delete_before(db, "events", "rowid", now - events_days * 86_400).await?;
    sqlx::query("DELETE FROM sessions WHERE expires_at < ?").bind(now).execute(db).await?;
    Ok(())
}

/// The finest summary that fits a bucket of `bucket` seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Level {
    Detail,
    Hourly,
    Daily,
}

impl Level {
    pub fn for_bucket(bucket: i64) -> Self {
        match bucket {
            b if b >= 86_400 => Level::Daily,
            b if b >= HOUR => Level::Hourly,
            _ => Level::Detail,
        }
    }
}

/// The points of a device from `since` to `until`: from the summary of `level` where it exists,
/// then from finer data for the recent part not summed up yet.
pub async fn device_points(db: &Db, tz: Tz, device: i64, since: i64, until: i64, level: Level) -> sqlx::Result<Vec<db::DeviceSample>> {
    // Where each summary stops: the finer data takes over from there.
    let next_hour = db::get_setting(db, NEXT_HOUR).await?.and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
    let next_day = db::get_setting(db, NEXT_DAY).await?.and_then(|v| NaiveDate::parse_from_str(&v, "%Y-%m-%d").ok()).map_or(0, |d| midnight(d, tz));
    let read = |table: &'static str, from: i64, to: i64| async move {
        if from >= to {
            return Ok(vec![]);
        }
        sqlx::query_as::<_, db::DeviceSample>(AssertSqlSafe(format!("SELECT device_id, ts, field, value FROM {table} WHERE device_id = ? AND ts >= ? AND ts < ? ORDER BY ts")))
            .bind(device)
            .bind(from)
            .bind(to)
            .fetch_all(db)
            .await
    };
    let mut out = vec![];
    let mut from = since;
    if level == Level::Daily {
        out.extend(read("device_daily", from, until.min(next_day)).await?);
        from = from.max(next_day);
    }
    if level != Level::Detail {
        out.extend(read("device_hourly", from, until.min(next_hour)).await?);
        from = from.max(next_hour);
    }
    out.extend(read("device_samples", from, until).await?);
    Ok(out)
}

/// The oldest point of a device, whatever the summary.
pub async fn device_oldest(db: &Db, device: i64) -> sqlx::Result<Option<i64>> {
    let mut oldest: Option<i64> = None;
    for table in ["device_daily", "device_hourly", "device_samples"] {
        let t: Option<i64> = sqlx::query_scalar(AssertSqlSafe(format!("SELECT MIN(ts) FROM {table} WHERE device_id = ?"))).bind(device).fetch_one(db).await?;
        oldest = match (oldest, t) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
    }
    Ok(oldest)
}
