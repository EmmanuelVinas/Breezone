//! What the automations plan for a charger, as friezes (roadmap E39).
//!
//! The rules are played over a stretch of time: their triggers at a time (a time of day, the sun,
//! a stretch of the tariff that starts or ends), their conditions that can be told ahead (hours,
//! the tariff, day or night; and, on real days, the days of the week and the months), and what
//! their steps tell the charger ("profile", "no charge", "the surplus", "its own app", "charge
//! now"). A condition that cannot be told ahead (a measure, a device, presence…) is taken as true,
//! and said so. The last rule started wins.

use super::{on_day, time_of, Automation, Condition, Match, Step, SunEvent, TempoDay, Trigger};
use crate::charging::Mode;
use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Timelike, Utc};
use chrono_tz::Tz;
use serde::Serialize;

/// What a rule tells a charger.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum Effect {
    Profile {
        profile_id: Option<i64>,
    },
    Off,
    Solar,
    Free,
    /// "Charge now" (until it is unplugged, a time or some energy: shown until the next change).
    Now {
        kw: Option<f64>,
    },
}

/// A stretch with one thing told (`effect` `None`: nothing told yet).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Segment {
    pub start: i64,
    pub end: i64,
    pub effect: Option<Effect>,
    /// The rule that told it.
    pub automation_id: Option<i64>,
}

/// The tariff as the plan sees it.
pub trait TariffAt {
    /// Its kind of day and its slot at `ts`.
    fn at(&self, ts: i64) -> Option<(String, String)>;
    /// The kind of day of the day after the day of `ts` (`None`: not known).
    fn tomorrow(&self, ts: i64) -> Option<String>;
    /// The moments of `from..until` where the kind or the slot may change.
    fn cuts(&self, from: i64, until: i64) -> Vec<i64>;
}

/// The place, the clock and the tariff.
pub struct World<'a> {
    pub tz: Tz,
    pub coords: Option<(f64, f64)>,
    pub tariff: &'a dyn TariffAt,
    /// Real days: the days of the week and the months count. Else a day of a kind, any day.
    pub real: bool,
}

impl World<'_> {
    fn local(&self, ts: i64) -> DateTime<Tz> {
        DateTime::<Utc>::from_timestamp(ts, 0).unwrap_or_default().with_timezone(&self.tz)
    }

    fn at_time(&self, day: NaiveDate, t: chrono::NaiveTime) -> Option<i64> {
        let at = day.and_time(t);
        self.tz.from_local_datetime(&at).earliest().or_else(|| self.tz.from_local_datetime(&(at + Duration::hours(1))).earliest()).map(|d| d.timestamp())
    }

    fn sun(&self, day: NaiveDate) -> Option<crate::weather::SunTimes> {
        self.coords.map(|(lat, lon)| crate::weather::sun(day, lat, lon))
    }
}

/// A trigger can be placed on the clock.
pub fn placed(t: &Trigger) -> bool {
    matches!(t, Trigger::Time { .. } | Trigger::Sun { .. } | Trigger::Tariff { .. })
}

/// The rule tells this charger something.
pub fn drives(rule: &Automation, device_id: i64) -> bool {
    rule.steps().any(|s| matches!(s, Step::ChargerProfile { device_id: d, .. } | Step::ChargerNow { device_id: d, .. } if *d == device_id))
}

/// What the steps tell the charger, each with its delay (the waits before it).
fn effects(steps: &[Step], device_id: i64) -> Vec<(i64, Effect)> {
    let mut delay = 0;
    let mut out = vec![];
    for s in steps {
        match s {
            Step::Wait { seconds } => delay += seconds,
            // How long it waits is not known ahead.
            Step::WaitUntil { .. } => break,
            Step::ChargerProfile { device_id: d, profile_id, mode, .. } if *d == device_id => out.push((
                delay,
                match Mode::of_step(*mode, *profile_id) {
                    Mode::Profile => Effect::Profile { profile_id: *profile_id },
                    Mode::Off => Effect::Off,
                    Mode::Solar => Effect::Solar,
                    Mode::Free => Effect::Free,
                },
            )),
            Step::ChargerNow { device_id: d, kw, .. } if *d == device_id => out.push((delay, Effect::Now { kw: *kw })),
            _ => {}
        }
    }
    out
}

/// Whether the conditions hold at `ts`, and whether one of them had to be taken as true.
fn met(world: &World, joined: Match, conditions: &[Condition], ts: i64, assumed: &mut bool) -> bool {
    let each = conditions.iter().map(|c| one(world, c, ts, assumed));
    match joined {
        Match::All => each.fold(true, |a, b| a && b),
        Match::Any => conditions.is_empty() || each.fold(false, |a, b| a || b),
    }
}

fn one(world: &World, c: &Condition, ts: i64, assumed: &mut bool) -> bool {
    let at = world.local(ts);
    let mut guess = || {
        *assumed = true;
        true
    };
    match c {
        Condition::Hours { from, to } => match (time_of(from), time_of(to), at.time().with_second(0)) {
            (Some(from), Some(to), Some(t)) => {
                if from < to {
                    t >= from && t < to
                } else {
                    from == to || t >= from || t < to
                }
            }
            _ => guess(),
        },
        Condition::Days { days } if world.real => on_day(days, at),
        Condition::Months { from, to } if world.real => {
            let m = at.month();
            if from <= to {
                (*from..=*to).contains(&m)
            } else {
                m >= *from || m <= *to
            }
        }
        Condition::Daylight { day } => match world.sun(at.date_naive()) {
            Some(crate::weather::SunTimes { sunrise: Some(up), sunset: Some(down) }) => (ts >= up && ts < down) == *day,
            _ => guess(),
        },
        Condition::Tariff { day: TempoDay::Today, profile, slot } => match world.tariff.at(ts) {
            Some((p, s)) => profile.as_deref().is_none_or(|x| x == p) && slot.as_deref().is_none_or(|x| x == s),
            None => guess(),
        },
        Condition::Tariff { day: TempoDay::Tomorrow, profile, .. } => match world.tariff.tomorrow(ts) {
            Some(p) => profile.as_deref().is_none_or(|x| x == p),
            None => guess(),
        },
        Condition::Group { joined, conditions } => met(world, *joined, conditions, ts, assumed),
        _ => guess(),
    }
}

/// When a time or sun trigger happens on a day.
fn occurrence(world: &World, t: &Trigger, day: NaiveDate) -> Option<i64> {
    let (ts, days) = match t {
        Trigger::Time { time, days } => (world.at_time(day, time_of(time)?)?, days),
        Trigger::Sun { event, offset_minutes, days } => {
            let (lat, lon) = world.coords?;
            let at = match event {
                SunEvent::Sunrise => crate::weather::sun(day, lat, lon).sunrise,
                SunEvent::Sunset => crate::weather::sun(day, lat, lon).sunset,
                SunEvent::Dawn => crate::weather::twilight(day, lat, lon).sunrise,
                SunEvent::Dusk => crate::weather::twilight(day, lat, lon).sunset,
            }?;
            (at + offset_minutes * 60, days)
        }
        _ => return None,
    };
    (!world.real || on_day(days, world.local(ts))).then_some(ts)
}

/// One rule played: what it tells the charger and when (sorted), and whether a condition was
/// taken as true.
pub fn events(world: &World, rule: &Automation, device_id: i64, from: i64, until: i64) -> (Vec<(i64, Effect)>, bool) {
    let mut assumed = false;
    let mut out: Vec<(i64, Effect)> = vec![];
    let fire = |ts: i64, steps: &[Step], out: &mut Vec<(i64, Effect)>| out.extend(effects(steps, device_id).into_iter().map(|(d, e)| (ts + d, e)));
    let first = world.local(from).date_naive() - Duration::days(1);
    let last = world.local(until).date_naive();
    for t in &rule.triggers {
        match t {
            Trigger::Time { .. } | Trigger::Sun { .. } => {
                let mut day = first;
                while day <= last {
                    if let Some(ts) = occurrence(world, t, day).filter(|ts| *ts >= from && *ts < until) {
                        let steps = if met(world, rule.joined, &rule.conditions, ts, &mut assumed) { &rule.actions } else { &rule.otherwise };
                        fire(ts, steps, &mut out);
                    }
                    day += Duration::days(1);
                }
            }
            Trigger::Tariff { profile, slot } => {
                let holds = |ts: i64| world.tariff.at(ts).is_some_and(|(p, s)| profile.as_deref().is_none_or(|x| x == p) && slot.as_deref().is_none_or(|x| x == s));
                let mut before = holds(from - 60);
                for c in world.tariff.cuts(from, until) {
                    let now = holds(c);
                    if now && !before {
                        let steps = if met(world, rule.joined, &rule.conditions, c, &mut assumed) { &rule.actions } else { &rule.otherwise };
                        fire(c, steps, &mut out);
                    } else if before && !now {
                        fire(c, &rule.over, &mut out);
                    }
                    before = now;
                }
            }
            _ => {}
        }
    }
    out.sort_by_key(|(ts, _)| *ts);
    (out, assumed)
}

/// The rules played together over `from..until`, the last started winning; what they told before
/// `from` (a day before) holds at `from`.
pub fn segments(world: &World, rules: &[(i64, &Automation)], device_id: i64, from: i64, until: i64) -> (Vec<Segment>, bool) {
    let warm = from - 24 * 3600;
    let mut all: Vec<(i64, Effect, i64)> = vec![];
    let mut assumed = false;
    for (id, rule) in rules {
        let (list, guessed) = events(world, rule, device_id, warm, until);
        assumed |= guessed;
        all.extend(list.into_iter().map(|(ts, e)| (ts, e, *id)));
    }
    all.sort_by_key(|(ts, _, _)| *ts);
    let mut current: (Option<Effect>, Option<i64>) = (None, None);
    let mut out: Vec<Segment> = vec![];
    let mut start = from;
    for (ts, e, id) in all {
        if ts < from {
            current = (Some(e), Some(id));
            continue;
        }
        if ts >= until {
            break;
        }
        if (Some(e), Some(id)) == current {
            continue;
        }
        if ts > start {
            out.push(Segment { start, end: ts, effect: current.0, automation_id: current.1 });
        }
        current = (Some(e), Some(id));
        start = ts;
    }
    out.push(Segment { start, end: until, effect: current.0, automation_id: current.1 });
    // Side by side the same thing told by the same rule: one stretch.
    out.dedup_by(|b, a| {
        let same = a.effect == b.effect && a.automation_id == b.automation_id && a.end == b.start;
        if same {
            a.end = b.end;
        }
        same
    });
    (out, assumed)
}

/// The tariff of a real home, from its prices and the colours known.
pub struct RealTariff<'a> {
    pub tariffs: &'a crate::price::Tariffs,
    pub colors: &'a dyn Fn(NaiveDate) -> Option<crate::price::DayColor>,
    pub tz: Tz,
}

impl TariffAt for RealTariff<'_> {
    fn at(&self, ts: i64) -> Option<(String, String)> {
        crate::price::price_at(self.tariffs, self.colors, self.tz, ts).map(|p| (p.profile, p.slot))
    }

    fn tomorrow(&self, ts: i64) -> Option<String> {
        let p = crate::price::price_at(self.tariffs, self.colors, self.tz, ts)?;
        let day = NaiveDate::parse_from_str(&p.day, "%Y-%m-%d").ok()?;
        crate::price::profile_on(self.tariffs, self.colors, day + Duration::days(1)).map(|(id, _)| id)
    }

    fn cuts(&self, from: i64, until: i64) -> Vec<i64> {
        let Some(t) = crate::price::current(self.tariffs) else { return vec![] };
        let times: Vec<&str> = t.profiles.iter().flat_map(|p| p.slots.iter().map(|s| s.start.as_str())).chain([t.calendar.day_start.as_str()]).collect();
        crate::price::cuts(&times, self.tz, from, until)
    }
}

/// A day of one kind of the tariff, every day ("a blue day"): its slots, from midnight.
pub struct KindTariff<'a> {
    pub kind: Option<&'a crate::price::DayProfile>,
    pub tz: Tz,
}

impl TariffAt for KindTariff<'_> {
    fn at(&self, ts: i64) -> Option<(String, String)> {
        let k = self.kind?;
        let t = crate::price::local(ts, self.tz).time();
        let slot = crate::price::slot_at(&k.slots, |s| s.start.as_str(), t)?;
        Some((k.id.clone(), slot.name.clone()))
    }

    fn tomorrow(&self, _ts: i64) -> Option<String> {
        self.kind.map(|k| k.id.clone())
    }

    fn cuts(&self, from: i64, until: i64) -> Vec<i64> {
        let Some(k) = self.kind else { return vec![] };
        let times: Vec<&str> = k.slots.iter().map(|s| s.start.as_str()).collect();
        crate::price::cuts(&times, self.tz, from, until)
    }
}

/// The kinds of day of the tariff a rule names (in its triggers and conditions).
pub fn kinds_named(rule: &Automation) -> Vec<String> {
    fn walk(c: &Condition, out: &mut Vec<String>) {
        match c {
            Condition::Tariff { profile: Some(p), .. } => out.push(p.clone()),
            Condition::Group { conditions, .. } => conditions.iter().for_each(|c| walk(c, out)),
            _ => {}
        }
    }
    let mut out = vec![];
    for t in &rule.triggers {
        if let Trigger::Tariff { profile: Some(p), .. } = t {
            out.push(p.clone());
        }
    }
    rule.conditions.iter().for_each(|c| walk(c, &mut out));
    out.dedup();
    let mut seen = vec![];
    out.retain(|p| {
        let new = !seen.contains(p);
        seen.push(p.clone());
        new
    });
    out
}

/// A stretch of a day of a frieze, in minutes from midnight.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DaySegment {
    pub start: u32,
    pub end: u32,
    pub effect: Option<Effect>,
}

/// A frieze of a rule: a day of a kind of the tariff, or any day.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Bar {
    /// The kind of day of the tariff (`None`: every day, or the other days with `others`).
    pub kind: Option<String>,
    pub label: String,
    pub color: Option<String>,
    /// The days of the kinds it does not name.
    pub others: bool,
    pub segments: Vec<DaySegment>,
}

/// The friezes of one rule for a charger: a bar per kind of day of the tariff it names, and the
/// other days; else one bar, every day. Its waits and conditions that cannot be told ahead are
/// said by `assumed`.
pub fn bars(
    rule: &Automation,
    device_id: i64,
    tariff: Option<&crate::price::Tariff>,
    today_kind: Option<&str>,
    coords: Option<(f64, f64)>,
    tz: Tz,
    day: NaiveDate,
) -> (Vec<Bar>, bool) {
    let named = kinds_named(rule);
    let kinds: Vec<&crate::price::DayProfile> = tariff.map(|t| t.profiles.iter().collect()).unwrap_or_default();
    let mut wanted: Vec<(Option<&crate::price::DayProfile>, bool)> = named.iter().filter_map(|id| kinds.iter().find(|k| &k.id == id).map(|k| (Some(*k), false))).collect();
    if wanted.is_empty() {
        let k = today_kind.and_then(|id| kinds.iter().find(|k| k.id == id)).or(kinds.first()).copied();
        wanted.push((k, false));
    } else if let Some(other) = kinds.iter().find(|k| !named.contains(&k.id)) {
        wanted.push((Some(*other), true));
    }
    let mid = |d: NaiveDate| tz.from_local_datetime(&d.and_time(chrono::NaiveTime::MIN)).earliest().map_or(0, |t| t.timestamp());
    let (from, until) = (mid(day), mid(day + Duration::days(1)));
    let mut assumed = false;
    let mut out = vec![];
    for (kind, others) in wanted {
        let tariff = KindTariff { kind, tz };
        let world = World { tz, coords, tariff: &tariff, real: false };
        let (segs, guessed) = segments(&world, &[(0, rule)], device_id, from, until);
        assumed |= guessed;
        let minutes = |ts: i64| (((ts - from).max(0) / 60) as u32).min(24 * 60);
        let label = match (kind, others) {
            (_, true) => "".to_string(),
            (Some(k), false) if !named.is_empty() => k.name.clone(),
            _ => "".to_string(),
        };
        out.push(Bar {
            kind: kind.filter(|_| !others && !named.is_empty()).map(|k| k.id.clone()),
            label,
            color: kind.filter(|_| !others && !named.is_empty()).and_then(|k| k.color.clone()),
            others,
            segments: segs.into_iter().map(|s| DaySegment { start: minutes(s.start), end: minutes(s.end), effect: s.effect }).collect(),
        });
    }
    (out, assumed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::price::{Calendar, ColorProfiles, DayProfile, PriceSlot, Tariff};
    use chrono_tz::Europe::Paris;

    fn slot(start: &str, name: &str) -> PriceSlot {
        PriceSlot { start: start.into(), name: name.into(), price: 0.1 }
    }

    /// Tempo: off-peak 22:00 to 06:00, three kinds.
    fn tempo() -> Tariff {
        let kind = |id: &str, name: &str| DayProfile { id: id.into(), name: name.into(), color: None, slots: vec![slot("00:00", "HC"), slot("06:00", "HP"), slot("22:00", "HC")] };
        Tariff {
            subscription: 0.0,
            export_price: None,
            profiles: vec![kind("blue", "Jour bleu"), kind("white", "Jour blanc"), kind("red", "Jour rouge")],
            calendar: Calendar {
                day_start: "06:00".into(),
                weekdays: vec!["blue".into(); 7],
                colors: ColorProfiles { blue: Some("blue".into()), white: Some("white".into()), red: Some("red".into()) },
            },
        }
    }

    fn charger(mode: Mode, profile_id: Option<i64>) -> Step {
        Step::ChargerProfile { device_id: 9, profile_id, mode: Some(mode), solar: None }
    }

    fn rule(triggers: Vec<Trigger>, conditions: Vec<Condition>, actions: Vec<Step>, otherwise: Vec<Step>, over: Vec<Step>) -> Automation {
        Automation { triggers, joined: Match::All, conditions, actions, otherwise, mode: Default::default(), over }
    }

    /// While the off-peak hours last, a blue day: the 7 kW profile, else no charge; then no charge.
    fn blue_night() -> Automation {
        rule(
            vec![Trigger::Tariff { profile: None, slot: Some("HC".into()) }],
            vec![Condition::Tariff { day: TempoDay::Today, profile: Some("blue".into()), slot: None }],
            vec![charger(Mode::Profile, Some(1))],
            vec![charger(Mode::Off, None)],
            vec![charger(Mode::Off, None)],
        )
    }

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn shape(b: &Bar) -> Vec<(u32, u32, Option<Effect>)> {
        b.segments.iter().map(|s| (s.start / 60, s.end / 60, s.effect)).collect()
    }

    #[test]
    fn the_blue_night_as_friezes_of_a_blue_day_and_the_others() {
        let t = tempo();
        let (bars, assumed) = bars(&blue_night(), 9, Some(&t), Some("blue"), None, Paris, day("2026-11-16"));
        assert!(!assumed);
        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0].label, "Jour bleu");
        let seven = Some(Effect::Profile { profile_id: Some(1) });
        assert_eq!(shape(&bars[0]), vec![(0, 6, seven), (6, 22, Some(Effect::Off)), (22, 24, seven)]);
        assert!(bars[1].others);
        assert_eq!(shape(&bars[1]), vec![(0, 24, Some(Effect::Off))]);
    }

    #[test]
    fn the_sun_and_a_time_of_day() {
        let solar = rule(
            vec![Trigger::Time { time: "08:00".into(), days: vec![] }, Trigger::Time { time: "17:00".into(), days: vec![] }],
            vec![Condition::Hours { from: "07:00".into(), to: "12:00".into() }],
            vec![charger(Mode::Solar, None)],
            vec![charger(Mode::Off, None)],
            vec![],
        );
        let (bars, assumed) = bars(&solar, 9, None, None, None, Paris, day("2026-11-16"));
        assert!(!assumed);
        assert_eq!(bars.len(), 1);
        assert_eq!(shape(&bars[0]), vec![(0, 8, Some(Effect::Off)), (8, 17, Some(Effect::Solar)), (17, 24, Some(Effect::Off))], "17:00 is out of the hours: else");
        // Another charger: nothing told.
        let (bars, _) = super::bars(&solar, 3, None, None, None, Paris, day("2026-11-16"));
        assert_eq!(shape(&bars[0]), vec![(0, 24, None)]);
    }

    #[test]
    fn a_condition_that_cannot_be_told_is_taken_as_true() {
        let plugged = rule(
            vec![Trigger::Time { time: "04:00".into(), days: vec![1, 2, 3, 4, 5] }],
            vec![Condition::Presence { home: true }],
            vec![charger(Mode::Profile, Some(2)), Step::Wait { seconds: 3 * 3600 }, charger(Mode::Off, None)],
            vec![],
            vec![],
        );
        let (bars, assumed) = bars(&plugged, 9, None, None, None, Paris, day("2026-11-21"));
        assert!(assumed);
        // A Saturday, but a frieze is a day of a kind: the days of the week do not count.
        assert_eq!(shape(&bars[0]), vec![(0, 4, Some(Effect::Off)), (4, 7, Some(Effect::Profile { profile_id: Some(2) })), (7, 24, Some(Effect::Off))]);
    }

    #[test]
    fn real_days_follow_the_colours_and_the_last_rule_wins() {
        let t = crate::price::Tariffs::new(vec![(day("2026-01-01"), tempo())]);
        // Monday blue, Tuesday red.
        let colors = |d: NaiveDate| match d.day() {
            16 => Some(crate::price::DayColor::Blue),
            17 => Some(crate::price::DayColor::Red),
            _ => None,
        };
        let real = RealTariff { tariffs: &t, colors: &colors, tz: Paris };
        let world = World { tz: Paris, coords: None, tariff: &real, real: true };
        let at = |d: &str, h: u32| Paris.from_local_datetime(&day(d).and_hms_opt(h, 0, 0).unwrap()).earliest().unwrap().timestamp();
        let solar = rule(vec![Trigger::Time { time: "10:00".into(), days: vec![] }], vec![], vec![charger(Mode::Solar, None)], vec![], vec![]);
        let night = blue_night();
        let rules = [(1, &night), (2, &solar)];
        let (segs, _) = segments(&world, &rules, 9, at("2026-11-16", 0), at("2026-11-18", 0));
        let got: Vec<(i64, Option<Effect>, Option<i64>)> = segs.iter().map(|s| ((s.start - at("2026-11-16", 0)) / 3600, s.effect, s.automation_id)).collect();
        let seven = Some(Effect::Profile { profile_id: Some(1) });
        assert_eq!(
            got,
            vec![
                // Sunday night (no colour: its day of the week, blue) still charges.
                (0, seven, Some(1)),
                (6, Some(Effect::Off), Some(1)),
                (10, Some(Effect::Solar), Some(2)),
                (22, seven, Some(1)),
                (30, Some(Effect::Off), Some(1)),
                (34, Some(Effect::Solar), Some(2)),
                // Tuesday is red: the night does not charge.
                (46, Some(Effect::Off), Some(1)),
            ]
        );
    }
}
