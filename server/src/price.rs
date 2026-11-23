//! The price of electricity (roadmap E32): the contract of the home and its tariffs.
//!
//! A tariff holds the price of the subscription and **profiles of day** ("normal", "red day",
//! "week-end"…), each cut into **slots** of clock time with their price per kWh. Its **calendar**
//! says which profile a day follows: by the day of the week, or by the colour announced for it
//! (Tempo). A tariff is dated: a change of prices adds one from that day, so that the past keeps the
//! prices it was used at. Prices are those of the bill (taxes included), per kWh.
//!
//! The day of a tariff may start later than midnight (Tempo: 06:00): the night from 22:00 to 06:00
//! then belongs to the day before, whose colour it keeps.

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

pub const MAX_PROFILES: usize = 10;
pub const MAX_SLOTS: usize = 24;
/// More than any bill: a price beyond is a typing mistake (a price per MWh, cents…).
const MAX_PRICE: f64 = 10.0;

/// The contract of the home: what the screens say of it, the prices are in its tariffs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Contract {
    /// ISO 3166-1 alpha-2 ("FR").
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub supplier: Option<String>,
    /// "custom": prices set by hand.
    #[serde(default = "custom")]
    pub offer: String,
    /// Subscribed power (kVA).
    #[serde(default)]
    pub power_kva: Option<f64>,
    /// ISO 4217 ("EUR").
    #[serde(default = "euro")]
    pub currency: String,
    /// The off-peak hours, for an "Heures creuses" offer read from the CRE (none: 22:00 to 06:00).
    #[serde(default)]
    pub offpeak: Vec<OffPeak>,
}

/// A stretch of off-peak hours, local time; `from` after `to` goes over midnight.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OffPeak {
    pub from: String,
    pub to: String,
}

fn custom() -> String {
    "custom".into()
}

fn euro() -> String {
    "EUR".into()
}

fn midnight_text() -> String {
    "00:00".into()
}

/// The prices from a day on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tariff {
    /// Price of the subscription per month.
    pub subscription: f64,
    /// Price paid for a kWh returned to the grid (`None`: not sold).
    #[serde(default)]
    pub export_price: Option<f64>,
    pub profiles: Vec<DayProfile>,
    pub calendar: Calendar,
}

/// The prices of a kind of day, slot by slot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DayProfile {
    pub id: String,
    pub name: String,
    /// A colour for the screens ("#2563eb"), chosen by hand.
    #[serde(default)]
    pub color: Option<String>,
    /// From midnight (the first starts at "00:00"), each until the next one.
    pub slots: Vec<PriceSlot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriceSlot {
    /// "HH:MM", local time.
    pub start: String,
    /// "HP", "HC"…
    pub name: String,
    /// Per kWh.
    pub price: f64,
}

/// Which profile a day follows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Calendar {
    /// "HH:MM": when a day of the tariff starts (Tempo: "06:00").
    #[serde(default = "midnight_text")]
    pub day_start: String,
    /// The profile of each day of the week, Monday first.
    pub weekdays: Vec<String>,
    /// The profile of a day of an announced colour, instead of its day of the week.
    #[serde(default)]
    pub colors: ColorProfiles,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ColorProfiles {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blue: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub white: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub red: Option<String>,
}

/// The colour announced for a day (Tempo).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DayColor {
    Blue,
    White,
    Red,
}

/// "HH:MM" → a time of day.
pub fn clock(text: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(text, "%H:%M").ok().filter(|_| text.len() == 5)
}

pub fn validate_contract(c: &Contract) -> Result<(), String> {
    if c.country.as_deref().is_some_and(|x| x.len() != 2 || !x.chars().all(|ch| ch.is_ascii_uppercase())) {
        return Err("country: two capital letters (ISO 3166-1)".into());
    }
    if c.supplier.as_deref().is_some_and(|s| s.chars().count() > 80) || c.offer.is_empty() || c.offer.len() > 40 {
        return Err("supplier: 80 characters at most; offer: 1 to 40".into());
    }
    if c.power_kva.is_some_and(|p| !p.is_finite() || p <= 0.0 || p > 1000.0) {
        return Err("power_kva: between 0 and 1000".into());
    }
    if c.currency.len() != 3 || !c.currency.chars().all(|ch| ch.is_ascii_uppercase()) {
        return Err("currency: three capital letters (ISO 4217)".into());
    }
    if c.offpeak.len() > 3 {
        return Err("offpeak: 3 stretches at most".into());
    }
    for o in &c.offpeak {
        let on_half_hour = |t: &str| clock(t).is_some_and(|t| t.minute() % 30 == 0);
        if !on_half_hour(&o.from) || !on_half_hour(&o.to) || o.from == o.to {
            return Err("offpeak: \"HH:MM\" on the hour or the half hour, from before to".into());
        }
    }
    Ok(())
}

fn price_ok(p: f64) -> bool {
    p.is_finite() && (0.0..=MAX_PRICE).contains(&p)
}

/// Whether a tariff can be used: profiles that cover each day, a calendar that names them.
pub fn validate(t: &Tariff) -> Result<(), String> {
    if !t.subscription.is_finite() || t.subscription < 0.0 || t.subscription > 10_000.0 {
        return Err("subscription: between 0 and 10000 per month".into());
    }
    if t.export_price.is_some_and(|p| !price_ok(p)) {
        return Err(format!("export_price: between 0 and {MAX_PRICE} per kWh"));
    }
    if t.profiles.is_empty() || t.profiles.len() > MAX_PROFILES {
        return Err(format!("profiles: 1 to {MAX_PROFILES}"));
    }
    for (i, p) in t.profiles.iter().enumerate() {
        if p.id.trim().is_empty() || p.id.len() > 40 || t.profiles[..i].iter().any(|o| o.id == p.id) {
            return Err("each profile needs an id of its own (40 characters at most)".into());
        }
        if p.name.trim().is_empty() || p.name.chars().count() > 60 || p.color.as_deref().is_some_and(|c| c.len() > 20) {
            return Err(format!("profile {}: a name of 60 characters at most", p.id));
        }
        if p.slots.is_empty() || p.slots.len() > MAX_SLOTS {
            return Err(format!("profile {}: 1 to {MAX_SLOTS} slots", p.id));
        }
        let mut previous: Option<NaiveTime> = None;
        for s in &p.slots {
            let Some(start) = clock(&s.start) else { return Err(format!("profile {}: start \"HH:MM\" expected, not {}", p.id, s.start)) };
            match previous {
                None if start != NaiveTime::MIN => return Err(format!("profile {}: the first slot starts at 00:00", p.id)),
                Some(before) if start <= before => return Err(format!("profile {}: slots in the order of the day", p.id)),
                _ => {}
            }
            if s.name.chars().count() > 30 || !price_ok(s.price) {
                return Err(format!("profile {}: a slot name of 30 characters at most, a price between 0 and {MAX_PRICE}", p.id));
            }
            previous = Some(start);
        }
    }
    let c = &t.calendar;
    if clock(&c.day_start).is_none() {
        return Err("calendar.day_start: \"HH:MM\" expected".into());
    }
    let known = |id: &String| t.profiles.iter().any(|p| &p.id == id);
    if c.weekdays.len() != 7 || !c.weekdays.iter().all(known) {
        return Err("calendar.weekdays: a profile of the tariff for each of the 7 days".into());
    }
    if ![&c.colors.blue, &c.colors.white, &c.colors.red].into_iter().flatten().all(known) {
        return Err("calendar.colors: profiles of the tariff".into());
    }
    Ok(())
}

/// The tariffs of the home, each from its day on.
#[derive(Debug, Clone, Default)]
pub struct Tariffs(Vec<(NaiveDate, Tariff)>);

impl Tariffs {
    pub fn new(mut list: Vec<(NaiveDate, Tariff)>) -> Tariffs {
        list.sort_by_key(|(from, _)| *from);
        Tariffs(list)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The tariff of a (calendar) day: the last one that starts on it or before.
    fn on(&self, day: NaiveDate) -> Option<&Tariff> {
        self.0.iter().rev().find(|(from, _)| *from <= day).map(|(_, t)| t)
    }
}

/// The price at a moment, and where it comes from.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PriceAt {
    pub price: f64,
    /// The slot ("HP").
    pub slot: String,
    pub profile: String,
    pub profile_name: String,
    /// The colour of the day, when the calendar follows one and it is known.
    pub color: Option<DayColor>,
    /// The day of the tariff ("YYYY-MM-DD"), which may be the day before after midnight (Tempo).
    pub day: String,
}

pub fn local(ts: i64, tz: Tz) -> NaiveDateTime {
    chrono::DateTime::from_timestamp(ts, 0).unwrap_or_default().with_timezone(&tz).naive_local()
}

/// The day of a calendar a local moment belongs to: the day before until `day_start` ("06:00").
pub fn calendar_day(day_start: &str, at: NaiveDateTime) -> NaiveDate {
    let start = clock(day_start).unwrap_or(NaiveTime::MIN);
    (at - Duration::seconds(i64::from(start.num_seconds_from_midnight()))).date()
}

/// The kind of day a calendar gives a day, and the colour it follows (when it has one for it).
pub fn kind_of_day<'a>(cal: &'a Calendar, color: &dyn Fn(NaiveDate) -> Option<DayColor>, day: NaiveDate) -> (&'a str, Option<DayColor>) {
    let colored = |c: DayColor| match c {
        DayColor::Blue => cal.colors.blue.as_deref(),
        DayColor::White => cal.colors.white.as_deref(),
        DayColor::Red => cal.colors.red.as_deref(),
    };
    let day_color = color(day).filter(|c| colored(*c).is_some());
    let weekday = cal.weekdays.get(day.weekday().num_days_from_monday() as usize).map_or("", String::as_str);
    (day_color.and_then(colored).unwrap_or(weekday), day_color)
}

/// The slot of a day that holds `t`: the last one started (slots in the order of the day).
pub fn slot_at<S>(slots: &[S], start: impl Fn(&S) -> &str, t: NaiveTime) -> Option<&S> {
    slots.iter().rev().find(|s| clock(start(s)).is_some_and(|c| c <= t)).or(slots.first())
}

/// The moments from `from` to `until` where a day of slots may change (`times`: the starts of
/// every slot and of the day), `from` first.
pub fn cuts(times: &[&str], tz: Tz, from: i64, until: i64) -> Vec<i64> {
    let mut times: Vec<NaiveTime> = times.iter().copied().filter_map(clock).collect();
    times.sort();
    times.dedup();
    let mut cuts = vec![from];
    let mut day = local(from, tz).date() - Duration::days(1);
    let last = local(until, tz).date() + Duration::days(1);
    while day <= last {
        for t in &times {
            // A time the clocks skip (spring) happens at the first one after.
            let at = tz.from_local_datetime(&day.and_time(*t)).earliest().or_else(|| tz.from_local_datetime(&(day.and_time(*t) + Duration::hours(1))).earliest());
            if let Some(at) = at.map(|a| a.timestamp()).filter(|a| *a > from && *a < until) {
                cuts.push(at);
            }
        }
        day += Duration::days(1);
    }
    cuts.sort();
    cuts.dedup();
    cuts
}

/// The price at `ts` (`None`: no tariff yet). `color` gives the colour announced for a day.
pub fn price_at(tariffs: &Tariffs, color: &dyn Fn(NaiveDate) -> Option<DayColor>, tz: Tz, ts: i64) -> Option<PriceAt> {
    let at = local(ts, tz);
    let day = calendar_day(&tariffs.on(at.date())?.calendar.day_start, at);
    let tariff = tariffs.on(day).or_else(|| tariffs.on(at.date()))?;
    let (id, day_color) = kind_of_day(&tariff.calendar, color, day);
    let profile = tariff.profiles.iter().find(|p| p.id == id)?;
    let slot = slot_at(&profile.slots, |s| s.start.as_str(), at.time())?;
    Some(PriceAt {
        price: slot.price,
        slot: slot.name.clone(),
        profile: profile.id.clone(),
        profile_name: profile.name.clone(),
        color: day_color,
        day: day.format("%Y-%m-%d").to_string(),
    })
}

/// The kind of day of the tariff a day of its calendar follows: its id and its name.
pub fn profile_on(tariffs: &Tariffs, color: &dyn Fn(NaiveDate) -> Option<DayColor>, day: NaiveDate) -> Option<(String, String)> {
    let tariff = tariffs.on(day)?;
    let (id, _) = kind_of_day(&tariff.calendar, color, day);
    tariff.profiles.iter().find(|p| p.id == id).map(|p| (p.id.clone(), p.name.clone()))
}

/// The tariff in force now (the last one started), for the names of its profiles and slots.
pub fn current(tariffs: &Tariffs) -> Option<&Tariff> {
    tariffs.0.last().map(|(_, t)| t)
}

/// A stretch of time at one price.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Period {
    pub start: i64,
    pub end: i64,
    pub price: f64,
    pub slot: String,
    pub profile: String,
    pub profile_name: String,
    pub color: Option<DayColor>,
}

/// The prices from `from` to `until`, as the stretches they hold (empty: no tariff).
pub fn periods(tariffs: &Tariffs, color: &dyn Fn(NaiveDate) -> Option<DayColor>, tz: Tz, from: i64, until: i64) -> Vec<Period> {
    if tariffs.is_empty() || until <= from {
        return vec![];
    }
    // Where the price may change: each start of a slot or of a day of every tariff, each day.
    let times: Vec<&str> =
        tariffs.0.iter().flat_map(|(_, t)| t.profiles.iter().flat_map(|p| p.slots.iter().map(|s| s.start.as_str())).chain([t.calendar.day_start.as_str()])).collect();
    let cuts = cuts(&times, tz, from, until);
    let mut out: Vec<Period> = vec![];
    for (i, start) in cuts.iter().enumerate() {
        let end = cuts.get(i + 1).copied().unwrap_or(until);
        let Some(p) = price_at(tariffs, color, tz, *start) else { continue };
        match out.last_mut() {
            Some(prev) if prev.end == *start && prev.price == p.price && prev.slot == p.slot && prev.profile == p.profile && prev.color == p.color => prev.end = end,
            _ => out.push(Period { start: *start, end, price: p.price, slot: p.slot, profile: p.profile, profile_name: p.profile_name, color: p.color }),
        }
    }
    out
}

/// What a local day cost, in the currency of the contract (taxes included).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DayCost {
    /// The energy drawn from the grid, each kWh at its price.
    pub energy: f64,
    /// The share of the subscription of that day (a month's spread over its days).
    pub subscription: f64,
    /// What the production used on the spot would have cost from the grid.
    pub saved: f64,
    /// What the energy returned to the grid was paid (with a buy-back price).
    pub earned: f64,
}

fn days_in_month(day: NaiveDate) -> i64 {
    let first = day.with_day(1).unwrap_or(day);
    let next = if first.month() == 12 { NaiveDate::from_ymd_opt(first.year() + 1, 1, 1) } else { NaiveDate::from_ymd_opt(first.year(), first.month() + 1, 1) };
    next.map_or(30, |n| (n - first).num_days())
}

/// The cost of a local day from the mean powers of the home over its buckets (`bucket` seconds
/// each, from `points`): `None` when no tariff covers it.
pub fn day_cost(tariffs: &Tariffs, color: &dyn Fn(NaiveDate) -> Option<DayColor>, tz: Tz, day: NaiveDate, points: &[crate::energy::CurvePoint], bucket: i64) -> Option<DayCost> {
    let tariff = tariffs.on(day)?;
    let mut cost = DayCost { subscription: tariff.subscription / days_in_month(day) as f64, ..Default::default() };
    let hours = bucket as f64 / 3600.0;
    for p in points {
        let f = &p.flows;
        let Some(at) = price_at(tariffs, color, tz, p.ts + bucket / 2) else { continue };
        let export = f.grid.map_or(0.0, |g| (-g).max(0.0));
        let solar = f.solar.map_or(0.0, f64::abs);
        // Without a grid meter: what the home used beyond its production.
        let import = f.grid.map(|g| g.max(0.0)).or(f.home.map(|h| (h - solar).max(0.0))).unwrap_or(0.0);
        let kwh = |w: f64| w * hours / 1000.0;
        cost.energy += kwh(import) * at.price;
        cost.saved += kwh((solar - export).max(0.0)) * at.price;
        if let Some(sold) = tariffs.on(day).and_then(|t| t.export_price) {
            cost.earned += kwh(export) * sold;
        }
    }
    let round = |v: f64| (v * 10_000.0).round() / 10_000.0;
    Some(DayCost { energy: round(cost.energy), subscription: round(cost.subscription), saved: round(cost.saved), earned: round(cost.earned) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono_tz::Europe::Paris;

    fn slot(start: &str, name: &str, price: f64) -> PriceSlot {
        PriceSlot { start: start.into(), name: name.into(), price }
    }

    fn profile(id: &str, slots: Vec<PriceSlot>) -> DayProfile {
        DayProfile { id: id.into(), name: id.to_uppercase(), color: None, slots }
    }

    /// Heures creuses from 22:00 to 06:00.
    fn hc() -> Tariff {
        Tariff {
            subscription: 15.0,
            export_price: None,
            profiles: vec![profile("normal", vec![slot("00:00", "HC", 0.2), slot("06:00", "HP", 0.27), slot("22:00", "HC", 0.2)])],
            calendar: Calendar { day_start: "00:00".into(), weekdays: vec!["normal".into(); 7], colors: ColorProfiles::default() },
        }
    }

    /// Tempo-like: blue and red days from 06:00 to 06:00.
    fn tempo() -> Tariff {
        Tariff {
            subscription: 16.0,
            export_price: None,
            profiles: vec![
                profile("blue", vec![slot("00:00", "HC", 0.13), slot("06:00", "HP", 0.16), slot("22:00", "HC", 0.13)]),
                profile("red", vec![slot("00:00", "HC", 0.15), slot("06:00", "HP", 0.7), slot("22:00", "HC", 0.15)]),
            ],
            calendar: Calendar {
                day_start: "06:00".into(),
                weekdays: vec!["blue".into(); 7],
                colors: ColorProfiles { blue: Some("blue".into()), white: None, red: Some("red".into()) },
            },
        }
    }

    fn at(date: &str, time: &str) -> i64 {
        let d = NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap();
        Paris.from_local_datetime(&d.and_time(clock(time).unwrap())).earliest().unwrap().timestamp()
    }

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn no_color(_: NaiveDate) -> Option<DayColor> {
        None
    }

    #[test]
    fn a_tariff_is_checked() {
        assert_eq!(validate(&hc()), Ok(()));
        assert_eq!(validate(&tempo()), Ok(()));
        let mut t = hc();
        t.profiles[0].slots[0].start = "01:00".into();
        assert!(validate(&t).unwrap_err().contains("00:00"));
        let mut t = hc();
        t.profiles[0].slots.swap(1, 2);
        assert!(validate(&t).is_err(), "slots out of order");
        let mut t = hc();
        t.calendar.weekdays[3] = "nope".into();
        assert!(validate(&t).is_err(), "a day follows a profile that does not exist");
        let mut t = hc();
        t.profiles[0].slots[1].price = 270.0;
        assert!(validate(&t).is_err(), "a price per MWh");
        let mut t = hc();
        t.calendar.weekdays.pop();
        assert!(validate(&t).is_err());
    }

    #[test]
    fn the_price_follows_the_slots_of_the_day() {
        let tariffs = Tariffs::new(vec![(day("2026-01-01"), hc())]);
        let p = |time: &str| price_at(&tariffs, &no_color, Paris, at("2026-11-17", time)).unwrap();
        assert_eq!((p("05:59").slot, p("05:59").price), ("HC".into(), 0.2));
        assert_eq!((p("06:00").slot, p("06:00").price), ("HP".into(), 0.27));
        assert_eq!(p("21:59").slot, "HP");
        assert_eq!(p("23:30").slot, "HC");
        assert!(price_at(&tariffs, &no_color, Paris, at("2025-12-31", "12:00")).is_none(), "no tariff before the first one");
    }

    #[test]
    fn a_tempo_night_keeps_the_colour_of_the_day_before() {
        let tariffs = Tariffs::new(vec![(day("2026-01-01"), tempo())]);
        let red_monday = |d: NaiveDate| (d == day("2026-11-16")).then_some(DayColor::Red);
        let p = |date: &str, time: &str| price_at(&tariffs, &red_monday, Paris, at(date, time)).unwrap();
        assert_eq!(p("2026-11-16", "05:00").profile, "blue", "before 06:00, still Sunday");
        assert_eq!((p("2026-11-16", "12:00").price, p("2026-11-16", "12:00").color), (0.7, Some(DayColor::Red)));
        let night = p("2026-11-17", "03:00");
        assert_eq!((night.profile.as_str(), night.price, night.day.as_str()), ("red", 0.15, "2026-11-16"));
        assert_eq!(p("2026-11-17", "07:00").profile, "blue", "the next day has no colour: its day of the week");
    }

    #[test]
    fn a_new_tariff_applies_from_its_day() {
        let mut dearer = hc();
        dearer.profiles[0].slots[1].price = 0.3;
        let tariffs = Tariffs::new(vec![(day("2026-08-01"), dearer), (day("2026-01-01"), hc())]);
        let p = |date: &str| price_at(&tariffs, &no_color, Paris, at(date, "12:00")).unwrap().price;
        assert_eq!(p("2026-07-31"), 0.27);
        assert_eq!(p("2026-08-01"), 0.3);
    }

    #[test]
    fn the_periods_of_a_day_merge_what_has_one_price() {
        let tariffs = Tariffs::new(vec![(day("2026-01-01"), hc())]);
        let list = periods(&tariffs, &no_color, Paris, at("2026-11-17", "00:00"), at("2026-11-18", "00:00"));
        let shape: Vec<(i64, &str)> = list.iter().map(|p| ((p.end - p.start) / 3600, p.slot.as_str())).collect();
        assert_eq!(shape, vec![(6, "HC"), (16, "HP"), (2, "HC")]);
        // Across days, the night is one stretch.
        let list = periods(&tariffs, &no_color, Paris, at("2026-11-17", "12:00"), at("2026-11-18", "12:00"));
        let shape: Vec<(i64, &str)> = list.iter().map(|p| ((p.end - p.start) / 3600, p.slot.as_str())).collect();
        assert_eq!(shape, vec![(10, "HP"), (8, "HC"), (6, "HP")]);
    }

    #[test]
    fn the_periods_hold_across_a_change_of_clocks() {
        let tariffs = Tariffs::new(vec![(day("2026-01-01"), hc())]);
        // 25 October 2026: the clocks go back at 03:00, the day lasts 25 hours.
        let list = periods(&tariffs, &no_color, Paris, at("2026-10-25", "00:00"), at("2026-10-26", "00:00"));
        let hours: Vec<i64> = list.iter().map(|p| (p.end - p.start) / 3600).collect();
        assert_eq!(hours, vec![7, 16, 2]);
    }

    #[test]
    fn a_day_costs_its_energy_at_the_price_of_each_moment() {
        use crate::energy::{CurvePoint, Flows};
        let tariffs = Tariffs::new(vec![(day("2026-01-01"), Tariff { export_price: Some(0.1), ..hc() })]);
        let from = at("2026-11-17", "00:00");
        // 1 kW drawn all day; from 12:00 to 14:00 the sun gives 3 kW, 1 kW used, 2 kW returned.
        let points: Vec<CurvePoint> = (0..96)
            .map(|i| {
                let sunny = (48..56).contains(&i);
                CurvePoint {
                    ts: from + i * 900,
                    flows: Flows { grid: Some(if sunny { -2000.0 } else { 1000.0 }), solar: Some(if sunny { 3000.0 } else { 0.0 }), battery: None, home: Some(1000.0) },
                }
            })
            .collect();
        let c = day_cost(&tariffs, &no_color, Paris, day("2026-11-17"), &points, 900).unwrap();
        // HC: 8 h at 0.2; HP: 16 h less the 2 sunny ones, at 0.27.
        assert!((c.energy - (8.0 * 0.2 + 14.0 * 0.27)).abs() < 1e-6, "{c:?}");
        assert!((c.saved - 2.0 * 0.27).abs() < 1e-6, "{c:?}");
        assert!((c.earned - 4.0 * 0.1).abs() < 1e-6, "{c:?}");
        assert!((c.subscription - 15.0 / 30.0).abs() < 1e-4, "November has 30 days: {c:?}");
        assert!(day_cost(&tariffs, &no_color, Paris, day("2025-06-01"), &points, 900).is_none());
    }
}
