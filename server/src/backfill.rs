//! The history Breezone could not record (it was stopped, a source was unreachable), filled with
//! what the hardware kept of it (`history` of a module): only what it measured, never an
//! estimate.
//!
//! A gap is more than [`GAP_POINTS`] points missed in the power of a device, in the last
//! [`LOOK_BACK`]. What the hardware gives (a point a minute, say) is brought to the pace of the
//! history: each number the mean of the points of the interval, each counter (kWh) counted back
//! from its value right after the gap, so that it joins what was recorded. A gap is asked once per
//! start of the server.

use crate::db::{self, DeviceSample};
use crate::AppState;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Mutex;
use std::time::Duration;

/// How far back gaps are looked for.
const LOOK_BACK: i64 = 7 * 86_400;
/// Points missed that make a gap.
const GAP_POINTS: i64 = 3;
/// Calls to the hardware for one gap, at most.
const MAX_CALLS: usize = 400;
/// After the start (the sources read and a first point recorded), then every hour.
const FIRST: Duration = Duration::from_secs(120);
const EVERY: Duration = Duration::from_secs(3600);

/// The field whose points tell the gaps.
const PACE: &str = "power";

static ASKED: Mutex<Option<HashSet<(i64, i64)>>> = Mutex::new(None);

pub async fn run_loop(state: AppState) {
    tokio::time::sleep(FIRST).await;
    loop {
        if let Err(e) = fill(&state, db::now()).await {
            tracing::warn!("history not filled: {e:#}");
        }
        tokio::time::sleep(EVERY).await;
    }
}

/// A point the hardware kept: its start, its length, its numbers.
#[derive(Debug, Clone, PartialEq)]
pub struct Kept {
    pub ts: i64,
    pub period: i64,
    pub values: BTreeMap<String, f64>,
}

/// The points of a `history` answer, and where to ask the rest.
pub fn parse(answer: &Value) -> (Vec<Kept>, Option<i64>) {
    let points = answer["points"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let o = p.as_object()?;
            let ts = o.get("ts")?.as_i64()?;
            let period = o.get("period")?.as_i64().filter(|p| *p > 0)?;
            let values = o.iter().filter(|(k, _)| *k != "ts" && *k != "period").filter_map(|(k, v)| Some((k.clone(), v.as_f64().filter(|x| x.is_finite())?))).collect();
            Some(Kept { ts, period, values })
        })
        .collect();
    (points, answer["more"].as_i64())
}

/// The gaps of a device's points (sorted times): from one point to the next, when more than
/// `GAP_POINTS` were missed.
pub fn gaps(times: &[i64], interval: i64) -> Vec<(i64, i64)> {
    times.windows(2).filter(|w| w[1] - w[0] > GAP_POINTS * interval).map(|w| (w[0], w[1])).collect()
}

/// The history points of a gap `(after, before)` from what the hardware kept: one every
/// `interval` from `after`, each the mean of the numbers of its points (those that started in the
/// interval before it); a counter of `counters` counted back from its value at `before`, when the
/// points cover the end of the gap.
pub fn points_of(device_id: i64, kept: &[Kept], gap: (i64, i64), interval: i64, counters: &HashMap<String, f64>) -> Vec<DeviceSample> {
    let (after, before) = gap;
    let covers_end = kept.last().is_some_and(|k| k.ts + k.period >= before - interval);
    let mut out = vec![];
    let mut at = after + interval;
    while at <= before - interval / 2 {
        let inside: Vec<&Kept> = kept.iter().filter(|k| k.ts >= at - interval && k.ts < at).collect();
        if !inside.is_empty() {
            let mut fields: BTreeMap<&str, (f64, usize)> = BTreeMap::new();
            for k in &inside {
                for (f, v) in &k.values {
                    if !crate::traits::is_counter(f) {
                        let e = fields.entry(f.as_str()).or_insert((0.0, 0));
                        e.0 += v;
                        e.1 += 1;
                    }
                }
            }
            out.extend(fields.into_iter().map(|(f, (sum, n))| DeviceSample { device_id, ts: at, field: f.to_string(), value: sum / n as f64 }));
            if covers_end {
                for (f, end) in counters {
                    // What it rose by from this point to the end of the gap.
                    let later: f64 = kept.iter().filter(|k| k.ts >= at && k.ts < before).filter_map(|k| k.values.get(f)).sum();
                    out.push(DeviceSample { device_id, ts: at, field: f.clone(), value: ((end - later) * 1000.0).round() / 1000.0 });
                }
            }
        }
        at += interval;
    }
    out
}

/// Looks for the gaps of the devices whose hardware keeps a history, and fills them. The number of
/// points added.
pub async fn fill(state: &AppState, now: i64) -> anyhow::Result<u64> {
    let interval = state.cfg.history_interval_secs.max(1);
    let ids = db::source_device_ids(&state.db).await?;
    let names: HashMap<i64, String> =
        sqlx::query_as::<_, (i64, String)>("SELECT id, name FROM devices WHERE source_id IS NOT NULL").fetch_all(&state.db).await?.into_iter().collect();
    let mut added = 0;
    for source in db::list_sources(&state.db).await? {
        let Some(driver) = state.drivers.driver(source.id).await else { continue };
        if !driver.keeps_history() {
            continue;
        }
        let prefix = format!("{}/", source.id);
        for (key, device_id) in ids.iter().filter(|(k, _)| k.starts_with(&prefix)) {
            let reference = &key[prefix.len()..];
            let times: Vec<i64> = sqlx::query_scalar("SELECT ts FROM device_samples WHERE device_id = ? AND field = ? AND ts >= ? ORDER BY ts")
                .bind(device_id)
                .bind(PACE)
                .bind(now - LOOK_BACK)
                .fetch_all(&state.db)
                .await?;
            for gap in gaps(&times, interval) {
                {
                    let mut asked = ASKED.lock().unwrap_or_else(|e| e.into_inner());
                    if !asked.get_or_insert_with(HashSet::new).insert((*device_id, gap.0)) {
                        continue;
                    }
                }
                let mut kept = vec![];
                let mut since = gap.0;
                for _ in 0..MAX_CALLS {
                    let answer = match driver.history(reference, since, gap.1).await {
                        Ok(a) => a,
                        Err(e) => {
                            tracing::info!("{}: no history of {reference}: {e}", source.name);
                            break;
                        }
                    };
                    let (points, more) = parse(&answer);
                    kept.extend(points);
                    match more.filter(|m| *m > since && *m < gap.1) {
                        Some(m) => since = m,
                        None => break,
                    }
                }
                kept.retain(|k| k.ts >= gap.0 && k.ts < gap.1);
                kept.sort_by_key(|k| k.ts);
                kept.dedup_by_key(|k| k.ts);
                if kept.is_empty() {
                    continue;
                }
                // The counters right after the gap, to count back from.
                let ends: Vec<(String, f64)> =
                    sqlx::query_as("SELECT field, value FROM device_samples WHERE device_id = ? AND ts = ?").bind(device_id).bind(gap.1).fetch_all(&state.db).await?;
                let counters: HashMap<String, f64> = ends.into_iter().filter(|(f, _)| crate::traits::is_counter(f) && kept.iter().any(|k| k.values.contains_key(f))).collect();
                let samples = points_of(*device_id, &kept, gap, interval, &counters);
                let n = db::add_device_samples(&state.db, &samples).await?;
                if n == 0 {
                    continue;
                }
                added += n;
                crate::rollup::redo(&state.db, state.cfg.timezone, gap.0, gap.1).await?;
                let name = names.get(device_id).cloned().unwrap_or_default();
                let span = duration_text(gap.1 - gap.0);
                let text = crate::text!(
                    "{0} : historique repris de l'appareil pour {1} manquants",
                    "{0}: history taken back from the device for {1} missed",
                    "{0}: historial recuperado del dispositivo para {1} perdidos",
                    name,
                    span.arg()
                );
                state.hub.log("connection", "info", None, Some("Breezone"), text, None).await;
            }
        }
    }
    Ok(added)
}

/// "3 h 20", "45 min".
fn duration_text(secs: i64) -> crate::i18n::Text {
    let (h, m) = (secs / 3600, (secs % 3600) / 60);
    let text = if h > 0 { format!("{h} h {m:02}") } else { format!("{m} min") };
    crate::i18n::Text { fr: text.clone(), en: text.clone(), es: text }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn minute(ts: i64, power: f64, energy_in: f64) -> Kept {
        Kept { ts, period: 60, values: BTreeMap::from([("power".to_string(), power), ("energy_in".to_string(), energy_in)]) }
    }

    #[test]
    fn a_gap_is_more_than_a_few_points_missed() {
        assert_eq!(gaps(&[0, 300, 600, 3600, 3900], 300), vec![(600, 3600)]);
        assert!(gaps(&[0, 300, 1200], 300).is_empty(), "three points missed: not yet");
    }

    #[test]
    fn an_answer_is_read() {
        let (points, more) = parse(&json!({"points": [{"ts": 60, "period": 60, "power": 75.0, "energy_in": 0.00125}, {"ts": 120, "power": 3}], "more": 180}));
        assert_eq!(points, vec![minute(60, 75.0, 0.00125)], "a point without its period is left out");
        assert_eq!(more, Some(180));
    }

    #[test]
    fn the_minutes_of_a_gap_become_points_of_the_history() {
        // Recorded at 0 and at 1200 (every 300 s); the hardware kept the minutes from 0 to 1200:
        // 60 W then 120 W, so 1 Wh then 2 Wh a minute.
        let kept: Vec<Kept> = (0..20).map(|i| if i < 10 { minute(i * 60, 60.0, 0.001) } else { minute(i * 60, 120.0, 0.002) }).collect();
        let counters = HashMap::from([("energy_in".to_string(), 10.0)]);
        let points = points_of(7, &kept, (0, 1200), 300, &counters);
        let at = |ts: i64, f: &str| points.iter().find(|p| p.ts == ts && p.field == f).map(|p| p.value);
        assert_eq!(points.iter().map(|p| p.ts).collect::<HashSet<_>>(), HashSet::from([300, 600, 900]), "not on the points recorded");
        assert_eq!(at(300, "power"), Some(60.0));
        assert_eq!(at(600, "power"), Some(60.0), "the minutes from 300 to 600");
        assert_eq!(at(900, "power"), Some(120.0));
        // Counted back from 10 kWh at 1200: 900 → 10 - 5 × 2 Wh, 300 → 10 - (5 × 1 + 10 × 2) Wh.
        assert_eq!(at(900, "energy_in"), Some(9.99));
        assert_eq!(at(300, "energy_in"), Some(9.975));
        // Without the end of the gap, no counter (it could not join what was recorded).
        let partial = points_of(7, &kept[..8], (0, 1200), 300, &counters);
        assert!(partial.iter().all(|p| p.field != "energy_in"));
        assert!(partial.iter().any(|p| p.field == "power"));
    }
}
