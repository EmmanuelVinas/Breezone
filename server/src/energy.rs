//! Energy of the home, from the history of its energy measures and their role (ADR 0013).
//!
//! * `grid`: the whole home at the meter. Positive power is drawn from the grid, `energy_in` is
//!   imported, `energy_out` exported.
//! * `solar`: a production. Its sign depends on how the clamp is fitted, so its size is used.
//! * `battery`: positive power charges it, `energy_in` is charged, `energy_out` discharged.
//! * `consumption`: what a device or a room uses (size, like the production).
//!
//! The home uses what the grid gives, plus the production and the battery discharge, minus the
//! battery charge. Without a grid meter it is the sum of the consumption measures.
//!
//! Each history point stands for the time since the previous one of the same device (its power is
//! the mean over that time), at most `max_span` seconds: a longer gap is time the device was not
//! read, and counts for nothing. The energy of a span comes from the meter's counters when the
//! device has them, else from the mean power.

use crate::db::DeviceSample;
use chrono::{TimeZone, Utc};
use chrono_tz::Tz;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Grid,
    Solar,
    Battery,
    Consumption,
}

impl Role {
    pub fn parse(s: &str) -> Option<Role> {
        match s {
            "grid" => Some(Role::Grid),
            "solar" => Some(Role::Solar),
            "battery" => Some(Role::Battery),
            "consumption" => Some(Role::Consumption),
            _ => None,
        }
    }
}

/// What a consumption measure measures, as shown in the breakdown.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Consumer {
    pub name: String,
    /// The measured device, or room; neither for a measure that stands for itself.
    pub device_id: Option<i64>,
    pub room_id: Option<i64>,
}

/// An energy measure of the home.
#[derive(Debug, Clone, PartialEq)]
pub struct Measure {
    pub device_id: i64,
    pub role: Role,
    /// For a consumption measure.
    pub consumer: Option<Consumer>,
}

/// Fields of the history read here.
pub const FIELDS: &[&str] = &["power", "energy_in", "energy_out"];

/// The time a device was read between two history points, and what it gave.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    pub device_id: i64,
    pub start: i64,
    pub end: i64,
    /// Mean power (W), signed as the device gives it.
    pub power: f64,
    /// kWh in and out.
    pub energy_in: f64,
    pub energy_out: f64,
}

impl Span {
    fn mid(&self) -> i64 {
        self.start + (self.end - self.start) / 2
    }
}

#[derive(Default, Clone, Copy)]
struct Point {
    ts: i64,
    power: Option<f64>,
    energy_in: Option<f64>,
    energy_out: Option<f64>,
}

/// Counters going down (a reset) or jumping beyond this give nothing for the span (kWh/h).
const MAX_KWH_PER_HOUR: f64 = 100.0;

fn delta(prev: Option<f64>, cur: Option<f64>, hours: f64) -> Option<f64> {
    let d = cur? - prev?;
    Some(if d >= 0.0 && d <= MAX_KWH_PER_HOUR * hours { d } else { 0.0 })
}

/// The spans of each device, from its history (sorted by device then time).
pub fn spans(samples: &[DeviceSample], max_span: i64) -> Vec<Span> {
    let mut points: Vec<(i64, Point)> = vec![];
    for s in samples {
        if points.last().is_none_or(|(d, p)| *d != s.device_id || p.ts != s.ts) {
            points.push((s.device_id, Point { ts: s.ts, ..Default::default() }));
        }
        let p = &mut points.last_mut().expect("pushed above").1;
        match s.field.as_str() {
            "power" => p.power = Some(s.value),
            "energy_in" => p.energy_in = Some(s.value),
            "energy_out" => p.energy_out = Some(s.value),
            _ => {}
        }
    }
    let mut out = vec![];
    for w in points.windows(2) {
        let ((d0, a), (d1, b)) = (w[0], w[1]);
        let secs = b.ts - a.ts;
        if d0 != d1 || secs <= 0 || secs > max_span {
            continue;
        }
        let hours = secs as f64 / 3600.0;
        let counted_in = delta(a.energy_in, b.energy_in, hours);
        let counted_out = delta(a.energy_out, b.energy_out, hours);
        let (energy_in, energy_out) = match (counted_in, counted_out, b.power) {
            (Some(i), o, _) => (i, o.unwrap_or(0.0)),
            (None, _, Some(p)) => {
                let kwh = p * hours / 1000.0;
                (kwh.max(0.0), (-kwh).max(0.0))
            }
            _ => continue,
        };
        let power = b.power.unwrap_or((energy_in - energy_out) * 1000.0 / hours);
        out.push(Span { device_id: d1, start: a.ts, end: b.ts, power, energy_in, energy_out });
    }
    out
}

fn round(v: f64, digits: i32) -> f64 {
    let f = 10f64.powi(digits);
    // `+ 0.0` turns −0 into 0.
    (v * f).round() / f + 0.0
}

/// Power of the home at a moment (W), each role summed over its measures; `None` for a role the
/// home has no measure of.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Flows {
    /// Drawn from the grid (negative: returned to it).
    pub grid: Option<f64>,
    pub solar: Option<f64>,
    /// Charging (negative: discharging).
    pub battery: Option<f64>,
    /// Used by the home.
    pub home: Option<f64>,
}

fn flows(measures: &[Measure], power: impl Fn(i64) -> Option<f64>) -> Flows {
    let sum = |role: Role, size: bool| -> Option<f64> {
        let values: Vec<f64> = measures.iter().filter(|m| m.role == role).filter_map(|m| power(m.device_id)).collect();
        (!values.is_empty()).then(|| values.iter().map(|v| if size { v.abs() } else { *v }).sum())
    };
    let grid = sum(Role::Grid, false);
    let solar = sum(Role::Solar, true);
    let battery = sum(Role::Battery, false);
    let home = match grid {
        Some(g) => Some((g + solar.unwrap_or(0.0) - battery.unwrap_or(0.0)).max(0.0)),
        None => sum(Role::Consumption, true),
    };
    let r = |v: Option<f64>| v.map(|v| round(v, 1));
    Flows { grid: r(grid), solar: r(solar), battery: r(battery), home: r(home) }
}

/// The power of one consumer now (W).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ConsumerPower {
    #[serde(flatten)]
    pub consumer: Consumer,
    pub power: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Live {
    #[serde(flatten)]
    pub flows: Flows,
    /// Largest first.
    pub consumers: Vec<ConsumerPower>,
    /// What the home uses beyond its consumption measures (with a grid meter).
    pub other: Option<f64>,
}

/// The consumers, each once (two measures of the same room add up).
fn group<T>(measures: &[Measure], value: impl Fn(&Measure) -> Option<f64>, make: impl Fn(Consumer, f64) -> T) -> Vec<T> {
    let mut groups: Vec<(Consumer, f64)> = vec![];
    for m in measures.iter().filter(|m| m.role == Role::Consumption) {
        let (Some(c), Some(v)) = (&m.consumer, value(m)) else {
            continue;
        };
        match groups.iter_mut().find(|(g, _)| (g.device_id.is_some() || g.room_id.is_some()) && g.device_id == c.device_id && g.room_id == c.room_id) {
            Some((_, total)) => *total += v,
            None => groups.push((c.clone(), v)),
        }
    }
    groups.sort_by(|a, b| b.1.total_cmp(&a.1));
    groups.into_iter().map(|(c, v)| make(c, v)).collect()
}

/// The home now, from the current power of its measures.
pub fn live(measures: &[Measure], power: impl Fn(i64) -> Option<f64>) -> Live {
    let flows = flows(measures, &power);
    let consumers = group(measures, |m| power(m.device_id).map(f64::abs), |consumer, p| ConsumerPower { consumer, power: round(p, 1) });
    let other = flows.grid.and(flows.home).map(|h| round((h - consumers.iter().map(|c| c.power).sum::<f64>()).max(0.0), 1));
    Live { flows, consumers, other }
}

/// A point of the curve of a day: the mean power of each role over its bucket.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CurvePoint {
    /// Start of the bucket.
    pub ts: i64,
    #[serde(flatten)]
    pub flows: Flows,
}

/// Mean power of each role per bucket of `bucket` seconds from `from`, `count` buckets. A bucket
/// without any reading has no values.
pub fn curve(measures: &[Measure], spans: &[Span], from: i64, bucket: i64, count: usize) -> Vec<CurvePoint> {
    // Per bucket and device: (power × seconds, seconds).
    let mut acc: Vec<BTreeMap<i64, (f64, f64)>> = vec![BTreeMap::new(); count];
    for s in spans {
        let i = (s.mid() - from).div_euclid(bucket);
        if i < 0 || i as usize >= count {
            continue;
        }
        let secs = (s.end - s.start) as f64;
        let e = acc[i as usize].entry(s.device_id).or_default();
        e.0 += s.power * secs;
        e.1 += secs;
    }
    acc.into_iter()
        .enumerate()
        .map(|(i, devices)| CurvePoint { ts: from + i as i64 * bucket, flows: flows(measures, |id| devices.get(&id).filter(|(_, secs)| *secs > 0.0).map(|(ws, secs)| ws / secs)) })
        .collect()
}

/// Energy of one consumer over a day (kWh).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ConsumerEnergy {
    #[serde(flatten)]
    pub consumer: Consumer,
    pub energy: f64,
}

/// Energy of one measure over a day (kWh), as its counters give it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DeviceEnergy {
    pub device_id: i64,
    pub energy_in: f64,
    pub energy_out: f64,
}

/// Energy of the home over a local day (kWh); `None` for what the home has no measure of.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct DayEnergy {
    /// "YYYY-MM-DD".
    pub day: String,
    pub imported: Option<f64>,
    pub exported: Option<f64>,
    pub production: Option<f64>,
    pub charged: Option<f64>,
    pub discharged: Option<f64>,
    /// Used by the home.
    pub consumption: Option<f64>,
    /// Production used on the spot (not returned to the grid).
    pub self_consumed: Option<f64>,
    /// Largest first.
    pub consumers: Vec<ConsumerEnergy>,
    /// What the home used beyond its consumption measures (with a grid meter).
    pub other: Option<f64>,
    pub devices: Vec<DeviceEnergy>,
}

fn day_of(ts: i64, tz: Tz) -> Option<String> {
    Utc.timestamp_opt(ts, 0).single().map(|t| t.with_timezone(&tz).format("%Y-%m-%d").to_string())
}

/// Energy of each local day in `days` (in this order), from the spans of the measures.
pub fn daily(measures: &[Measure], spans: &[Span], tz: Tz, days: &[String]) -> Vec<DayEnergy> {
    // Per day and device: (in, out).
    let mut per_day: BTreeMap<String, BTreeMap<i64, (f64, f64)>> = BTreeMap::new();
    for s in spans {
        let Some(day) = day_of(s.mid(), tz) else {
            continue;
        };
        let e = per_day.entry(day).or_default().entry(s.device_id).or_default();
        e.0 += s.energy_in;
        e.1 += s.energy_out;
    }
    let has = |role: Role| measures.iter().any(|m| m.role == role);
    let r3 = |v: f64| round(v, 3);
    days.iter()
        .map(|day| {
            let devices = per_day.get(day).cloned().unwrap_or_default();
            let total = |role: Role, pick: &dyn Fn((f64, f64)) -> f64| -> Option<f64> {
                has(role).then(|| measures.iter().filter(|m| m.role == role).filter_map(|m| devices.get(&m.device_id)).map(|e| pick(*e)).sum())
            };
            let imported = total(Role::Grid, &|e| e.0);
            let exported = total(Role::Grid, &|e| e.1);
            let production = total(Role::Solar, &|e| e.0.max(e.1));
            let charged = total(Role::Battery, &|e| e.0);
            let discharged = total(Role::Battery, &|e| e.1);
            let consumers = group(measures, |m| devices.get(&m.device_id).map(|e| e.0.max(e.1)), |consumer, kwh| ConsumerEnergy { consumer, energy: r3(kwh) });
            let measured: f64 = consumers.iter().map(|c| c.energy).sum();
            let consumption = match imported {
                Some(i) => Some((i - exported.unwrap_or(0.0) + production.unwrap_or(0.0) + discharged.unwrap_or(0.0) - charged.unwrap_or(0.0)).max(0.0)),
                None => has(Role::Consumption).then_some(measured),
            };
            DayEnergy {
                day: day.clone(),
                imported: imported.map(r3),
                exported: exported.map(r3),
                self_consumed: production.map(|p| r3((p - exported.unwrap_or(0.0)).max(0.0))),
                production: production.map(r3),
                charged: charged.map(r3),
                discharged: discharged.map(r3),
                other: imported.and(consumption).map(|c| r3((c - measured).max(0.0))),
                consumption: consumption.map(r3),
                consumers,
                devices: measures
                    .iter()
                    .filter_map(|m| devices.get(&m.device_id).map(|e| DeviceEnergy { device_id: m.device_id, energy_in: r3(e.0), energy_out: r3(e.1) }))
                    .collect(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(device_id: i64, ts: i64, field: &str, value: f64) -> DeviceSample {
        DeviceSample { device_id, ts, field: field.into(), value }
    }

    fn measure(device_id: i64, role: Role) -> Measure {
        Measure { device_id, role, consumer: None }
    }

    fn consumer(device_id: i64, name: &str, room_id: Option<i64>) -> Measure {
        Measure { device_id, role: Role::Consumption, consumer: Some(Consumer { name: name.into(), device_id: None, room_id }) }
    }

    // 2026-11-09 00:00 in Paris (UTC+1).
    const DAY: i64 = 1_794_178_800;

    #[test]
    fn counters_give_the_energy_else_the_mean_power() {
        let samples = vec![
            // A meter: 0.5 kWh in, 0.1 kWh out over 300 s.
            sample(1, DAY, "energy_in", 10.0),
            sample(1, DAY, "energy_out", 2.0),
            sample(1, DAY, "power", 100.0),
            sample(1, DAY + 300, "energy_in", 10.5),
            sample(1, DAY + 300, "energy_out", 2.1),
            sample(1, DAY + 300, "power", 4800.0),
            // A gap: not read for an hour, counts for nothing.
            sample(1, DAY + 3900, "energy_in", 20.0),
            // A plug without counters: 1200 W for 600 s = 0.2 kWh, then a reading back.
            sample(2, DAY, "power", 0.0),
            sample(2, DAY + 600, "power", 1200.0),
            sample(2, DAY + 900, "power", -400.0),
        ];
        let s = spans(&samples, 600);
        assert_eq!(s.len(), 3, "{s:?}");
        assert_eq!((s[0].energy_in, round(s[0].energy_out, 3), s[0].power), (0.5, 0.1, 4800.0));
        assert_eq!((round(s[1].energy_in, 3), s[1].energy_out), (0.2, 0.0));
        assert_eq!((s[2].energy_in, round(s[2].energy_out, 4)), (0.0, 0.0333));
    }

    #[test]
    fn a_counter_reset_gives_nothing() {
        let samples = vec![sample(1, DAY, "energy_in", 900.0), sample(1, DAY + 300, "energy_in", 0.2)];
        assert_eq!(spans(&samples, 600)[0].energy_in, 0.0);
    }

    #[test]
    fn the_home_from_the_grid_the_sun_and_the_battery() {
        let measures = vec![measure(1, Role::Grid), measure(2, Role::Solar), measure(3, Role::Battery), consumer(4, "Pompe", None)];
        // Returning 600 W, producing 4 800 W (clamp reversed), charging 1 200 W, pool pump 750 W.
        let power = |id| [(1, -600.0), (2, -4800.0), (3, 1200.0), (4, 750.0)].into_iter().find(|(d, _)| *d == id).map(|(_, p)| p);
        let l = live(&measures, power);
        assert_eq!(l.flows, Flows { grid: Some(-600.0), solar: Some(4800.0), battery: Some(1200.0), home: Some(3000.0) });
        assert_eq!(l.consumers[0].power, 750.0);
        assert_eq!(l.other, Some(2250.0));

        // Without a grid meter, the home is what its measures use; no "other".
        let only = vec![consumer(4, "Pompe", None), consumer(5, "Salon", Some(1)), consumer(6, "Salon 2", Some(1))];
        let l = live(&only, |id| Some(id as f64 * 100.0));
        assert_eq!(l.flows.home, Some(1500.0));
        assert_eq!(l.flows.grid, None);
        assert_eq!(l.other, None);
        assert_eq!(l.consumers.len(), 2, "the two measures of the room add up");
        assert_eq!((l.consumers[0].consumer.name.as_str(), l.consumers[0].power), ("Salon", 1100.0));
    }

    #[test]
    fn days_add_up_per_role() {
        let measures = vec![measure(1, Role::Grid), measure(2, Role::Solar), consumer(3, "Borne", None)];
        let span = |device_id, start: i64, energy_in, energy_out| Span { device_id, start, end: start + 300, power: 0.0, energy_in, energy_out };
        let spans = vec![
            span(1, DAY + 3600, 2.0, 0.0),
            span(1, DAY + 43_200, 0.0, 1.5),
            span(2, DAY + 43_200, 0.0, 6.0),
            span(3, DAY + 43_200, 1.0, 0.0),
            // The next local day (23:58 UTC on the 9th is 00:58 on the 10th in Paris).
            span(1, DAY + 86_400 + 3480, 0.4, 0.0),
        ];
        let days = daily(&measures, &spans, chrono_tz::Europe::Paris, &["2026-11-09".into(), "2026-11-10".into(), "2026-11-11".into()]);
        let d = &days[0];
        assert_eq!((d.imported, d.exported, d.production), (Some(2.0), Some(1.5), Some(6.0)));
        assert_eq!(d.consumption, Some(6.5), "2 imported − 1.5 exported + 6 produced");
        assert_eq!(d.self_consumed, Some(4.5));
        assert_eq!((d.consumers[0].energy, d.other), (1.0, Some(5.5)));
        assert_eq!((d.charged, d.discharged), (None, None), "no battery");
        assert_eq!(days[1].imported, Some(0.4));
        assert_eq!(days[2].consumption, Some(0.0), "a day without readings");
    }

    #[test]
    fn the_curve_of_a_day() {
        let measures = vec![measure(1, Role::Grid)];
        let spans = vec![
            Span { device_id: 1, start: DAY, end: DAY + 300, power: 1000.0, energy_in: 0.0, energy_out: 0.0 },
            Span { device_id: 1, start: DAY + 300, end: DAY + 600, power: 2000.0, energy_in: 0.0, energy_out: 0.0 },
        ];
        let c = curve(&measures, &spans, DAY, 900, 2);
        assert_eq!(c[0].flows.grid, Some(1500.0));
        assert_eq!(c[0].flows.home, Some(1500.0));
        assert_eq!(c[1].flows.grid, None);
    }
}
