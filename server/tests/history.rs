//! The long history of the devices: the detail is summed up per hour and per local day, curves over
//! long periods read the summaries, and the detail is deleted only once summed up.

use breezone_server::db;
use breezone_server::rollup::{self, Level};
use chrono::TimeZone;
use chrono_tz::Europe::Paris;

async fn database() -> db::Db {
    let dir = std::env::temp_dir().join(format!("breezone-history-{}-{}", std::process::id(), rand_suffix()));
    let _ = std::fs::remove_dir_all(&dir);
    db::connect(&dir.join("breezone.db")).await.unwrap()
}

fn rand_suffix() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64
}

fn at(day: u32, hour: u32, minute: u32) -> i64 {
    Paris.with_ymd_and_hms(2026, 3, day, hour, minute, 0).unwrap().timestamp()
}

#[tokio::test]
async fn hours_and_days_are_summed_up_and_read_by_period() {
    let db = database().await;
    let source = db::insert_source(&db, "shelly", "Compteur", "{}").await.unwrap();
    db::ensure_source_device(&db, source, "shelly", "em1:0", "energy.power").await.unwrap();
    let device: i64 = sqlx::query_scalar("SELECT id FROM devices").fetch_one(&db).await.unwrap();
    // Two days (the clocks change on the night of the 29th of March), a point every 5 minutes:
    // the power is the hour of the day × 100 W; the counter grows by 1 kWh per hour.
    let mut samples = vec![];
    for day in [28u32, 29] {
        for hour in 0..24u32 {
            for minute in (0..60).step_by(5) {
                let Some(t) = Paris.with_ymd_and_hms(2026, 3, day, hour, minute, 0).single() else { continue };
                let ts = t.timestamp();
                samples.push(db::DeviceSample { device_id: device, ts, field: "power".into(), value: hour as f64 * 100.0 });
                samples.push(db::DeviceSample { device_id: device, ts, field: "energy_in".into(), value: ((ts - at(28, 0, 0)) / 3600) as f64 });
            }
        }
    }
    db::insert_device_samples(&db, &samples).await.unwrap();

    rollup::roll_up(&db, Paris, at(30, 10, 0)).await.unwrap();
    let hour: f64 =
        sqlx::query_scalar("SELECT value FROM device_hourly WHERE device_id = ? AND field = 'power' AND ts = ?").bind(device).bind(at(28, 14, 0)).fetch_one(&db).await.unwrap();
    assert_eq!(hour, 1400.0, "the mean of the hour");
    let days: Vec<(i64, f64)> = sqlx::query_as("SELECT ts, value FROM device_daily WHERE device_id = ? AND field = 'power' ORDER BY ts").bind(device).fetch_all(&db).await.unwrap();
    assert_eq!(days.iter().map(|d| d.0).collect::<Vec<_>>(), vec![at(28, 0, 0), at(29, 0, 0)], "a point at each local midnight");
    assert_eq!(days[0].1, 1150.0, "the mean of 0 to 2300 W");
    let counter: f64 =
        sqlx::query_scalar("SELECT value FROM device_daily WHERE device_id = ? AND field = 'energy_in' AND ts = ?").bind(device).bind(at(28, 0, 0)).fetch_one(&db).await.unwrap();
    assert_eq!(counter, 23.0, "a counter keeps its last value of the day");

    // A curve over days reads the daily summary, one over hours the hourly one.
    let daily = rollup::device_points(&db, Paris, device, at(28, 0, 0), at(30, 0, 0), Level::Daily).await.unwrap();
    assert_eq!(daily.iter().filter(|p| p.field == "power").count(), 2);
    let hourly = rollup::device_points(&db, Paris, device, at(28, 0, 0), at(29, 0, 0), Level::Hourly).await.unwrap();
    assert_eq!(hourly.iter().filter(|p| p.field == "power").count(), 24);
    assert_eq!(rollup::device_oldest(&db, device).await.unwrap(), Some(at(28, 0, 0)));

    // Run again: nothing summed twice, nothing missed.
    rollup::roll_up(&db, Paris, at(30, 10, 0)).await.unwrap();
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM device_hourly WHERE field = 'power'").fetch_one(&db).await.unwrap();
    assert_eq!(n, 47, "47 hours: the 29th of March has 23");

    // The detail goes after its retention, the summaries stay.
    rollup::purge(&db, Paris, 1, 30).await.unwrap();
    let detail: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM device_samples").fetch_one(&db).await.unwrap();
    assert_eq!(detail, 0, "summed up and older than a day: deleted, in batches");
    let kept: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM device_daily").fetch_one(&db).await.unwrap();
    assert_eq!(kept, 4);
}
