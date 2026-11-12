//! Day-by-day comfort of a zone, computed from its history.
//!
//! Each history point stands for the time until the next one (points are written every few
//! minutes and on every change), at most `max_span` seconds: a longer gap is time the
//! installation was not read, and counts for nothing.

use crate::airzone::air_quality_level;
use crate::db::Reading;
use chrono::{TimeZone, Utc};
use chrono_tz::Tz;
use serde::Serialize;

/// A zone is comfortable when its temperature is this close to the setpoint (°).
pub const COMFORT_MARGIN: f64 = 0.5;

/// Operation modes that regulate a temperature: cooling, heating, auxiliary heating, auto.
const REGULATING_MODES: [i64; 4] = [2, 3, 6, 7];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DayStats {
    /// Local date, "YYYY-MM-DD".
    pub day: String,
    pub min_temp: Option<f64>,
    pub max_temp: Option<f64>,
    /// Time-weighted mean.
    pub avg_temp: Option<f64>,
    pub avg_humidity: Option<f64>,
    /// Seconds covered by the history this day.
    pub recorded_secs: i64,
    /// Seconds the zone was on in a mode that regulates the temperature.
    pub on_secs: i64,
    /// Seconds of `on_secs` within `COMFORT_MARGIN` of the setpoint.
    pub comfort_secs: i64,
    /// `comfort_secs / on_secs` (0 to 1), `None` when the zone was not on.
    pub comfort: Option<f64>,
    /// Seconds in demand, `None` when the installation did not record it this day.
    pub demand_secs: Option<i64>,
    /// Seconds of good, regular and bad air quality, `None` when the zone has no sensor.
    pub air_quality_secs: Option<AirQualitySecs>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct AirQualitySecs {
    pub good: i64,
    pub regular: i64,
    pub bad: i64,
}

#[derive(Default)]
struct Acc {
    min: Option<f64>,
    max: Option<f64>,
    temp_sum: f64,
    temp_secs: i64,
    hum_sum: f64,
    hum_secs: i64,
    recorded: i64,
    on: i64,
    comfort: i64,
    demand: Option<i64>,
    air: Option<AirQualitySecs>,
}

fn round(v: f64, digits: i32) -> f64 {
    let f = 10f64.powi(digits);
    (v * f).round() / f
}

/// Groups `readings` (sorted by time) per local day in `tz`, until `now`.
pub fn daily(readings: &[Reading], tz: Tz, max_span: i64, now: i64) -> Vec<DayStats> {
    let mut days: Vec<(String, Acc)> = vec![];
    for (i, r) in readings.iter().enumerate() {
        let end = readings.get(i + 1).map_or(now, |n| n.ts);
        let span = (end - r.ts).clamp(0, max_span);
        let Some(at) = Utc.timestamp_opt(r.ts, 0).single() else { continue };
        let day = at.with_timezone(&tz).format("%Y-%m-%d").to_string();
        if days.last().is_none_or(|(d, _)| *d != day) {
            days.push((day, Acc::default()));
        }
        let acc = &mut days.last_mut().expect("pushed above").1;
        // The demand time is the one counted up to this point (written with it).
        if let Some(d) = r.demand_secs {
            *acc.demand.get_or_insert(0) += d;
        }
        if let Some(t) = r.room_temp {
            acc.min = Some(acc.min.map_or(t, |m| m.min(t)));
            acc.max = Some(acc.max.map_or(t, |m| m.max(t)));
            acc.temp_sum += t * span as f64;
            acc.temp_secs += span;
        }
        if let Some(h) = r.humidity.filter(|h| *h > 0) {
            acc.hum_sum += h as f64 * span as f64;
            acc.hum_secs += span;
        }
        acc.recorded += span;
        if let Some(level) = r.air_quality.and_then(air_quality_level) {
            let air = acc.air.get_or_insert_with(AirQualitySecs::default);
            match level {
                "good" => air.good += span,
                "regular" => air.regular += span,
                _ => air.bad += span,
            }
        }
        let regulating = r.is_on == Some(true) && r.mode.is_none_or(|m| REGULATING_MODES.contains(&m));
        if regulating {
            acc.on += span;
            if let (Some(t), Some(sp)) = (r.room_temp, r.setpoint) {
                if (t - sp).abs() <= COMFORT_MARGIN + 1e-9 {
                    acc.comfort += span;
                }
            }
        }
    }
    days.into_iter()
        .map(|(day, a)| DayStats {
            day,
            min_temp: a.min,
            max_temp: a.max,
            avg_temp: (a.temp_secs > 0).then(|| round(a.temp_sum / a.temp_secs as f64, 1)),
            avg_humidity: (a.hum_secs > 0).then(|| round(a.hum_sum / a.hum_secs as f64, 0)),
            recorded_secs: a.recorded,
            on_secs: a.on,
            comfort_secs: a.comfort,
            comfort: (a.on > 0).then(|| round(a.comfort as f64 / a.on as f64, 3)),
            demand_secs: a.demand,
            air_quality_secs: a.air,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(ts: i64, temp: f64, setpoint: f64, on: bool) -> Reading {
        Reading { ts, room_temp: Some(temp), humidity: Some(50), setpoint: Some(setpoint), is_on: Some(on), mode: Some(3), demand_secs: None, air_quality: None }
    }

    // 2026-11-09 00:00 in Paris (UTC+1).
    const DAY: i64 = 1_794_178_800;

    #[test]
    fn time_weighted_comfort_per_local_day() {
        let readings = vec![
            r(DAY, 18.0, 20.0, true),          // 300 s, too cold
            r(DAY + 300, 19.6, 20.0, true),    // 300 s, comfortable
            r(DAY + 600, 20.4, 20.0, false),   // 300 s, off: not counted
            r(DAY + 86_400, 21.0, 21.0, true), // next day
        ];
        let days = daily(&readings, chrono_tz::Europe::Paris, 600, DAY + 86_400 + 300);
        assert_eq!(days.len(), 2);
        let d = &days[0];
        assert_eq!(d.day, "2026-11-09");
        assert_eq!((d.on_secs, d.comfort_secs, d.comfort), (600, 300, Some(0.5)));
        assert_eq!((d.min_temp, d.max_temp), (Some(18.0), Some(20.4)));
        // The last point of the day stands for 600 s at most (then the installation was not read).
        assert_eq!(d.recorded_secs, 1200);
        assert_eq!(d.avg_temp, Some(19.6));
        assert_eq!(d.demand_secs, None);
        assert_eq!(days[1].day, "2026-11-10");
        assert_eq!(days[1].comfort, Some(1.0));
    }

    #[test]
    fn days_follow_the_time_zone() {
        // 23:30 UTC on the 8th is already the 9th in Paris.
        let readings = vec![r(DAY + 1800, 20.0, 20.0, true)];
        assert_eq!(daily(&readings, chrono_tz::Europe::Paris, 600, DAY + 2400)[0].day, "2026-11-09");
        assert_eq!(daily(&readings, chrono_tz::UTC, 600, DAY + 2400)[0].day, "2026-11-08");
    }

    #[test]
    fn fan_mode_and_unrecorded_demand() {
        let mut fan = r(DAY, 25.0, 20.0, true);
        fan.mode = Some(4);
        let mut heating = r(DAY + 300, 19.0, 20.0, true);
        heating.demand_secs = Some(240);
        let days = daily(&[fan, heating], chrono_tz::UTC, 600, DAY + 600);
        assert_eq!(days[0].on_secs, 300, "fan time is not regulating time");
        assert_eq!(days[0].comfort, Some(0.0));
        assert_eq!(days[0].demand_secs, Some(240));
        assert_eq!(days[0].air_quality_secs, None);
    }

    #[test]
    fn air_quality_time_per_level() {
        let mut readings: Vec<Reading> = (0..4).map(|i| r(DAY + i * 300, 20.0, 20.0, true)).collect();
        for (r, aq) in readings.iter_mut().zip([1, 1, 3, 0]) {
            r.air_quality = Some(aq);
        }
        let days = daily(&readings, chrono_tz::UTC, 600, DAY + 1200);
        // The last point (no measure) counts for nothing.
        assert_eq!(days[0].air_quality_secs, Some(AirQualitySecs { good: 600, regular: 0, bad: 300 }));
    }
}
