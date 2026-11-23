//! Charging profiles of the chargers (roadmap E39): the plan of the week of a charger.
//!
//! A profile has the shape of a tariff (see `price`): **kinds of day** ("blue day", "week-end"…)
//! cut into **slots** of clock time, and a **calendar** that says which kind a day is, by its day
//! of the week or by its Tempo colour (a day of the calendar may start at 06:00). A slot says what
//! the charger does: nothing (`off`), charge up to some kW whatever the sun (`max`, the charger
//! sharing the power of the home if it can), or charge on the solar surplus only (`solar`).
//!
//! A charger follows one profile, unless it was told to **charge now** (at some kW, until it is
//! unplugged, until a time or until some kWh more). The surplus regulator (`surplus`) does what
//! they say: this module only tells it what the plan is at each moment.

use crate::db::{self, ChargerPlanRow};
use crate::devices::DeviceView;
use crate::price::{self, Calendar, DayColor};
use crate::surplus::Plan;
use crate::AppState;
use chrono::{Duration, NaiveDate, TimeZone};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

pub const MAX_KINDS: usize = 10;
pub const MAX_SLOTS: usize = 24;
/// More than any charger of a home (kW).
pub const MAX_KW: f64 = 50.0;
/// How far the next charge is looked for.
const NEXT_WITHIN: i64 = 48 * 3600;
/// Voltage when the charger does not tell it.
const VOLTAGE: f64 = 230.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Off,
    Max,
    Solar,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Slot {
    /// "HH:MM", local time.
    pub start: String,
    pub mode: Mode,
    /// `max`: up to this power (none: the charger's maximum).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kw: Option<f64>,
    /// `solar`: the current it keeps between (none: the charger's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_amps: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_amps: Option<i64>,
}

/// What a kind of day does, slot by slot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DayKind {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub color: Option<String>,
    /// From midnight (the first starts at "00:00"), each until the next one.
    pub slots: Vec<Slot>,
}

/// A charging profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub profiles: Vec<DayKind>,
    pub calendar: Calendar,
}

/// What is stored of a profile besides its name.
#[derive(Serialize, Deserialize)]
struct Stored {
    profiles: Vec<DayKind>,
    calendar: Calendar,
}

impl Profile {
    /// A stored profile (`None`: written by a newer version, or broken).
    pub fn of(row: &db::ChargeProfileRow) -> Option<Profile> {
        let s: Stored = serde_json::from_str(&row.plan).ok()?;
        Some(Profile { name: row.name.clone(), profiles: s.profiles, calendar: s.calendar })
    }

    pub fn plan_json(&self) -> String {
        serde_json::to_string(&Stored { profiles: self.profiles.clone(), calendar: self.calendar.clone() }).unwrap_or_default()
    }
}

/// Whether a profile can be used: kinds of day that cover each day, a calendar that names them.
pub fn validate(p: &Profile) -> Result<(), String> {
    if p.name.trim().is_empty() || p.name.chars().count() > 60 {
        return Err("name: 1 to 60 characters".into());
    }
    if p.profiles.is_empty() || p.profiles.len() > MAX_KINDS {
        return Err(format!("profiles: 1 to {MAX_KINDS}"));
    }
    for (i, k) in p.profiles.iter().enumerate() {
        if k.id.trim().is_empty() || k.id.len() > 40 || p.profiles[..i].iter().any(|o| o.id == k.id) {
            return Err("each profile needs an id of its own (40 characters at most)".into());
        }
        if k.name.trim().is_empty() || k.name.chars().count() > 60 || k.color.as_deref().is_some_and(|c| c.len() > 20) {
            return Err(format!("profile {}: a name of 60 characters at most", k.id));
        }
        if k.slots.is_empty() || k.slots.len() > MAX_SLOTS {
            return Err(format!("profile {}: 1 to {MAX_SLOTS} slots", k.id));
        }
        let mut previous = None;
        for s in &k.slots {
            let Some(start) = price::clock(&s.start) else { return Err(format!("profile {}: start \"HH:MM\" expected, not {}", k.id, s.start)) };
            match previous {
                None if start != chrono::NaiveTime::MIN => return Err(format!("profile {}: the first slot starts at 00:00", k.id)),
                Some(before) if start <= before => return Err(format!("profile {}: slots in the order of the day", k.id)),
                _ => {}
            }
            if s.kw.is_some_and(|kw| !kw.is_finite() || kw <= 0.0 || kw > MAX_KW) {
                return Err(format!("profile {}: a power between 0 and {MAX_KW} kW", k.id));
            }
            let amps_ok = |a: Option<i64>| a.is_none_or(|a| (1..=80).contains(&a));
            if !amps_ok(s.min_amps) || !amps_ok(s.max_amps) || matches!((s.min_amps, s.max_amps), (Some(lo), Some(hi)) if lo > hi) {
                return Err(format!("profile {}: a current from 1 to 80 A, the minimum below the maximum", k.id));
            }
            previous = Some(start);
        }
    }
    let c = &p.calendar;
    if price::clock(&c.day_start).is_none() {
        return Err("calendar.day_start: \"HH:MM\" expected".into());
    }
    let known = |id: &String| p.profiles.iter().any(|k| &k.id == id);
    if c.weekdays.len() != 7 || !c.weekdays.iter().all(known) {
        return Err("calendar.weekdays: a profile for each of the 7 days".into());
    }
    if ![&c.colors.blue, &c.colors.white, &c.colors.red].into_iter().flatten().all(known) {
        return Err("calendar.colors: profiles of the charging profile".into());
    }
    Ok(())
}

/// The slot of a profile at a moment.
#[derive(Debug, Clone, PartialEq)]
pub struct SlotAt<'a> {
    pub slot: &'a Slot,
    pub kind: &'a DayKind,
    /// The colour of the day, when the calendar follows it.
    pub color: Option<DayColor>,
}

pub fn slot_at<'a>(p: &'a Profile, color: &dyn Fn(NaiveDate) -> Option<DayColor>, tz: Tz, ts: i64) -> Option<SlotAt<'a>> {
    let at = price::local(ts, tz);
    let day = price::calendar_day(&p.calendar.day_start, at);
    let (id, color) = price::kind_of_day(&p.calendar, color, day);
    let kind = p.profiles.iter().find(|k| k.id == id)?;
    let slot = price::slot_at(&kind.slots, |s| s.start.as_str(), at.time())?;
    Some(SlotAt { slot, kind, color })
}

/// A stretch of time with one thing to do.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Period {
    pub start: i64,
    pub end: i64,
    pub mode: Mode,
    pub kw: Option<f64>,
    #[serde(skip)]
    pub min_amps: Option<i64>,
    #[serde(skip)]
    pub max_amps: Option<i64>,
    /// The kind of day.
    pub profile: String,
    pub color: Option<DayColor>,
}

/// What the profile does from `from` to `until`, as the stretches it holds.
pub fn periods(p: &Profile, color: &dyn Fn(NaiveDate) -> Option<DayColor>, tz: Tz, from: i64, until: i64) -> Vec<Period> {
    if until <= from {
        return vec![];
    }
    let times: Vec<&str> = p.profiles.iter().flat_map(|k| k.slots.iter().map(|s| s.start.as_str())).chain([p.calendar.day_start.as_str()]).collect();
    let cuts = price::cuts(&times, tz, from, until);
    let mut out: Vec<Period> = vec![];
    for (i, start) in cuts.iter().enumerate() {
        let end = cuts.get(i + 1).copied().unwrap_or(until);
        let Some(s) = slot_at(p, color, tz, *start) else { continue };
        let same = |prev: &Period| {
            prev.end == *start && prev.mode == s.slot.mode && prev.kw == s.slot.kw && prev.min_amps == s.slot.min_amps && prev.max_amps == s.slot.max_amps && prev.color == s.color
        };
        match out.last_mut() {
            Some(prev) if same(prev) => prev.end = end,
            _ => out.push(Period {
                start: *start,
                end,
                mode: s.slot.mode,
                kw: s.slot.kw,
                min_amps: s.slot.min_amps,
                max_amps: s.slot.max_amps,
                profile: s.kind.id.clone(),
                color: s.color,
            }),
        }
    }
    out
}

/// The next time the charger charges (now, if it does): the first stretch of the next 48 hours
/// that is not `off`.
pub fn next(p: &Profile, color: &dyn Fn(NaiveDate) -> Option<DayColor>, tz: Tz, now: i64) -> Option<Period> {
    periods(p, color, tz, now, now + NEXT_WITHIN).into_iter().find(|p| p.mode != Mode::Off)
}

/// A local day of the profile, from midnight to midnight.
#[derive(Debug, Clone, Serialize)]
pub struct DayView {
    /// "YYYY-MM-DD".
    pub day: String,
    /// Its colour, when the calendar follows it and it is known.
    pub color: Option<DayColor>,
    pub periods: Vec<Period>,
}

fn midnight(day: NaiveDate, tz: Tz) -> i64 {
    tz.from_local_datetime(&day.and_time(chrono::NaiveTime::MIN)).earliest().map_or(0, |t| t.timestamp())
}

/// The `n` local days from `first`.
pub fn days(p: &Profile, color: &dyn Fn(NaiveDate) -> Option<DayColor>, tz: Tz, first: NaiveDate, n: i64) -> Vec<DayView> {
    (0..n)
        .map(|i| {
            let day = first + Duration::days(i);
            let (_, c) = price::kind_of_day(&p.calendar, color, day);
            DayView { day: day.format("%Y-%m-%d").to_string(), color: c, periods: periods(p, color, tz, midnight(day, tz), midnight(day + Duration::days(1), tz)) }
        })
        .collect()
}

/// The current that gives at most `kw` on a charger (between its limits).
pub fn amps_of(kw: f64, voltage: f64, phases: u32, lo: i64, hi: i64) -> i64 {
    let per_amp = (if voltage > 0.0 { voltage } else { VOLTAGE }) * f64::from(phases.max(1));
    ((kw * 1000.0 / per_amp).floor() as i64).clamp(lo, hi.max(lo))
}

/// The power of a current on a charger (kW, to a tenth).
pub fn kw_of(amps: i64, voltage: f64, phases: u32) -> f64 {
    let per_amp = (if voltage > 0.0 { voltage } else { VOLTAGE }) * f64::from(phases.max(1));
    (amps as f64 * per_amp / 100.0).round() / 10.0
}

/// What a charger says of itself that a plan needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Charger {
    pub online: bool,
    pub plugged: bool,
    pub session_energy: f64,
    pub voltage: f64,
    pub phases: u32,
    pub limit_min: i64,
    pub limit_max: i64,
}

impl Charger {
    pub fn of(d: &DeviceView) -> Charger {
        let num = |f: &str| d.state.get(f).and_then(Value::as_f64);
        Charger {
            online: d.available,
            plugged: matches!(d.state.get("status").and_then(Value::as_str), Some("plugged" | "charging")),
            session_energy: num("session_energy").unwrap_or(0.0),
            voltage: num("voltage").unwrap_or(0.0),
            phases: num("phases").unwrap_or(1.0) as u32,
            limit_min: num("limit_min").unwrap_or(6.0) as i64,
            limit_max: num("limit_max").unwrap_or(32.0) as i64,
        }
    }

    pub fn amps(&self, kw: Option<f64>) -> i64 {
        kw.map_or(self.limit_max, |kw| amps_of(kw, self.voltage, self.phases, self.limit_min, self.limit_max))
    }
}

/// Why a charge asked now is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NowOver {
    Unplugged,
    Time,
    Energy,
}

/// Whether the charge asked now is over.
pub fn now_over(row: &ChargerPlanRow, c: &Charger, now: i64) -> Option<NowOver> {
    row.now_amps?;
    if c.online && !c.plugged {
        Some(NowOver::Unplugged)
    } else if row.now_until.is_some_and(|u| now >= u) {
        Some(NowOver::Time)
    } else if let (Some(kwh), Some(base)) = (row.now_kwh, row.now_base_energy) {
        (c.session_energy - base >= kwh).then_some(NowOver::Energy)
    } else {
        None
    }
}

/// What a charger does now by its plan: the charge asked now, else the slot of its profile
/// (`None`: it follows nothing).
pub fn plan_now(row: &ChargerPlanRow, profile: Option<&Profile>, c: &Charger, color: &dyn Fn(NaiveDate) -> Option<DayColor>, tz: Tz, now: i64) -> Option<Plan> {
    if let Some(amps) = row.now_amps.filter(|_| now_over(row, c, now).is_none()) {
        let left = match (row.now_kwh, row.now_base_energy) {
            (Some(kwh), Some(base)) => Some((kwh - (c.session_energy - base)).max(0.0)),
            _ => None,
        };
        return Some(Plan::Max { amps: amps.clamp(c.limit_min, c.limit_max.max(c.limit_min)), until: row.now_until, now: true, left_kwh: left });
    }
    let p = profile?;
    let horizon = now + 7 * 24 * 3600;
    let list = periods(p, color, tz, now, horizon);
    let first = list.first()?.clone();
    // Until it does something else (whatever the colour of the days in between).
    let same = |q: &Period| q.mode == first.mode && q.kw == first.kw && q.min_amps == first.min_amps && q.max_amps == first.max_amps;
    let end = list.iter().take_while(|q| same(q)).last().map_or(first.end, |q| q.end);
    let until = Some(end).filter(|end| *end < horizon);
    Some(match first.mode {
        Mode::Off => Plan::Off { until },
        Mode::Max => Plan::Max { amps: c.amps(first.kw), until, now: false, left_kwh: None },
        Mode::Solar => {
            let lo = first.min_amps.unwrap_or(c.limit_min).clamp(c.limit_min, c.limit_max.max(c.limit_min));
            let hi = first.max_amps.unwrap_or(16.min(c.limit_max)).clamp(lo, c.limit_max.max(lo));
            Plan::Solar { min_amps: lo, max_amps: hi }
        }
    })
}

/// The stored profiles, by id (those this version cannot read are left out).
pub async fn profiles(state: &AppState) -> sqlx::Result<HashMap<i64, Profile>> {
    Ok(db::list_charge_profiles(&state.db).await?.iter().filter_map(|r| Profile::of(r).map(|p| (r.id, p))).collect())
}

/// The Tempo colours known around now (yesterday to the day after tomorrow).
pub async fn colors_around(state: &AppState, now: i64) -> sqlx::Result<HashMap<NaiveDate, DayColor>> {
    let today = crate::rollup::local_day(now, state.cfg.timezone);
    crate::tempo::colors(&state.db, today - Duration::days(1), today + Duration::days(3)).await
}

/// The plan of every charger that follows one now, with the name of its profile. A charge asked
/// now that is over is cleared (it is said in the activity feed).
pub async fn plans(state: &AppState, devices: &[DeviceView], now: i64) -> HashMap<i64, (Plan, Option<String>)> {
    let rows = match db::list_charger_plans(&state.db).await {
        Ok(rows) => rows,
        Err(e) => {
            tracing::error!("charging plans: {e}");
            return HashMap::new();
        }
    };
    if rows.is_empty() {
        return HashMap::new();
    }
    let profiles = profiles(state).await.unwrap_or_default();
    let known = colors_around(state, now).await.unwrap_or_default();
    let color = |d: NaiveDate| known.get(&d).copied();
    let tz = state.cfg.timezone;
    let mut out = HashMap::new();
    for mut row in rows {
        let Some(d) = devices.iter().find(|d| d.id == row.device_id && d.traits.iter().any(|t| t.starts_with("ev.charger@"))) else { continue };
        let c = Charger::of(d);
        if let Some(why) = now_over(&row, &c, now) {
            if db::clear_charge_now(&state.db, row.device_id).await.unwrap_or(false) {
                let name = d.name.clone();
                let text = match why {
                    NowOver::Unplugged => crate::text!(
                        "{0} : voiture débranchée, fin de la charge immédiate",
                        "{0}: car unplugged, the charge now is over",
                        "{0}: coche desconectado, fin de la carga inmediata",
                        name
                    ),
                    NowOver::Time => crate::text!(
                        "{0} : heure atteinte, fin de la charge immédiate",
                        "{0}: time reached, the charge now is over",
                        "{0}: hora alcanzada, fin de la carga inmediata",
                        name
                    ),
                    NowOver::Energy => crate::text!(
                        "{0} : énergie ajoutée, fin de la charge immédiate",
                        "{0}: energy added, the charge now is over",
                        "{0}: energía añadida, fin de la carga inmediata",
                        name
                    ),
                };
                state.hub.log("automation", "info", None, Some("Breezone"), text, None).await;
            }
            row = ChargerPlanRow { now_amps: None, now_kw: None, now_until: None, now_kwh: None, now_base_energy: None, ..row };
        }
        let profile = row.profile_id.and_then(|id| profiles.get(&id));
        if let Some(plan) = plan_now(&row, profile, &c, &color, tz, now) {
            let name = if row.now_amps.is_some() { None } else { profile.map(|p| p.name.clone()) };
            out.insert(row.device_id, (plan, name));
        }
    }
    out
}

/// Gives a charger a profile (`None`: none): it takes it at once, even after a stop by hand.
pub async fn set_profile(state: &AppState, device_id: i64, profile_id: Option<i64>) -> crate::error::ApiResult<()> {
    use crate::error::ApiError;
    if let Some(pid) = profile_id {
        if !db::list_charge_profiles(&state.db).await?.iter().any(|r| r.id == pid) {
            return Err(ApiError::not_found(crate::text!("profil de recharge introuvable", "charging profile not found", "perfil de carga no encontrado")));
        }
    }
    let row = db::charger_plan(&state.db, device_id).await?.unwrap_or(ChargerPlanRow { device_id, ..Default::default() });
    db::set_charger_plan(&state.db, &ChargerPlanRow { profile_id, ..row }).await?;
    state.surplus.release(device_id).await;
    Ok(())
}

/// What "charge now" asks.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NowInput {
    /// Up to this power (none: the charger's maximum).
    #[serde(default)]
    pub kw: Option<f64>,
    /// Until then (none: until it is unplugged).
    #[serde(default)]
    pub until: Option<i64>,
    /// Until this much more energy (kWh).
    #[serde(default)]
    pub kwh: Option<f64>,
}

/// Charges a charger now, without a break: until it is unplugged, a time or some energy more.
pub async fn charge_now(state: &AppState, d: &DeviceView, input: &NowInput) -> crate::error::ApiResult<()> {
    use crate::error::ApiError;
    let now = db::now();
    let bad = |fr: &str, en: &str, es: &str| ApiError::bad_request(crate::i18n::Text { fr: fr.into(), en: en.into(), es: es.into() });
    if input.kw.is_some_and(|k| !k.is_finite() || k <= 0.0 || k > MAX_KW) {
        return Err(bad("puissance de 0 à 50 kW", "power from 0 to 50 kW", "potencia de 0 a 50 kW"));
    }
    if input.until.is_some_and(|u| u <= now || u > now + 7 * 24 * 3600) {
        return Err(bad("une heure dans les 7 jours qui viennent", "a time within the next 7 days", "una hora en los próximos 7 días"));
    }
    if input.kwh.is_some_and(|k| !k.is_finite() || k <= 0.0 || k > 200.0) {
        return Err(bad("énergie de 0 à 200 kWh", "energy from 0 to 200 kWh", "energía de 0 a 200 kWh"));
    }
    let c = Charger::of(d);
    if !c.plugged {
        return Err(ApiError::conflict(crate::text!("Branchez d'abord la voiture", "Plug the car in first", "Conecte primero el coche")));
    }
    let amps = c.amps(input.kw);
    let row = db::charger_plan(&state.db, d.id).await?.unwrap_or(ChargerPlanRow { device_id: d.id, ..Default::default() });
    db::set_charger_plan(
        &state.db,
        &ChargerPlanRow {
            now_amps: Some(amps),
            now_kw: Some(input.kw.unwrap_or_else(|| kw_of(amps, c.voltage, c.phases))),
            now_until: input.until,
            now_kwh: input.kwh,
            now_base_energy: Some(c.session_energy),
            ..row
        },
    )
    .await?;
    // Asked now: what was done by hand before no longer holds.
    state.surplus.release(d.id).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::price::ColorProfiles;
    use chrono_tz::Europe::Paris;

    fn slot(start: &str, mode: Mode, kw: Option<f64>) -> Slot {
        Slot { start: start.into(), mode, kw, min_amps: None, max_amps: None }
    }

    /// The blue night: 7 kW in the off-peak hours (22:00 to 06:00) of a blue day, nothing on the
    /// peak hours, nothing on white and red days; days of Tempo from 06:00 to 06:00.
    fn blue_night() -> Profile {
        Profile {
            name: "Nuit bleue".into(),
            profiles: vec![
                DayKind {
                    id: "blue".into(),
                    name: "Jour bleu".into(),
                    color: None,
                    slots: vec![slot("00:00", Mode::Max, Some(7.0)), slot("06:00", Mode::Off, None), slot("22:00", Mode::Max, Some(7.0))],
                },
                DayKind { id: "other".into(), name: "Blanc et rouge".into(), color: None, slots: vec![slot("00:00", Mode::Off, None)] },
            ],
            calendar: Calendar {
                day_start: "06:00".into(),
                weekdays: vec!["blue".into(); 7],
                colors: ColorProfiles { blue: Some("blue".into()), white: Some("other".into()), red: Some("other".into()) },
            },
        }
    }

    fn at(date: &str, time: &str) -> i64 {
        let d = NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap();
        Paris.from_local_datetime(&d.and_time(price::clock(time).unwrap())).earliest().unwrap().timestamp()
    }

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    /// Monday 2026-11-16 blue, Tuesday red, Wednesday blue.
    fn colors(d: NaiveDate) -> Option<DayColor> {
        match d.format("%d").to_string().as_str() {
            "16" | "18" => Some(DayColor::Blue),
            "17" => Some(DayColor::Red),
            _ => None,
        }
    }

    fn trydan() -> Charger {
        Charger { online: true, plugged: true, session_energy: 0.0, voltage: 230.0, phases: 1, limit_min: 6, limit_max: 32 }
    }

    #[test]
    fn a_profile_is_checked() {
        assert_eq!(validate(&blue_night()), Ok(()));
        let mut p = blue_night();
        p.profiles[0].slots[0].start = "01:00".into();
        assert!(validate(&p).unwrap_err().contains("00:00"));
        let mut p = blue_night();
        p.profiles[0].slots[2].kw = Some(70.0);
        assert!(validate(&p).is_err(), "more than a charger of a home");
        let mut p = blue_night();
        p.calendar.colors.red = Some("nope".into());
        assert!(validate(&p).is_err());
        let mut p = blue_night();
        p.profiles[1].slots[0].min_amps = Some(16);
        p.profiles[1].slots[0].max_amps = Some(10);
        assert!(validate(&p).is_err(), "the minimum above the maximum");
        let mut p = blue_night();
        p.name = " ".into();
        assert!(validate(&p).is_err());
        let json = serde_json::json!({"name": "x", "profiles": [{"id": "a", "name": "A", "slots": [{"start": "00:00", "mode": "sun"}]}], "calendar": {"weekdays": vec!["a"; 7]}});
        assert!(serde_json::from_value::<Profile>(json).is_err(), "an unknown mode");
    }

    #[test]
    fn the_blue_night_charges_in_the_off_peak_hours_of_blue_days_only() {
        let p = blue_night();
        let mode = |date: &str, time: &str| slot_at(&p, &colors, Paris, at(date, time)).map(|s| (s.slot.mode, s.slot.kw)).unwrap();
        assert_eq!(mode("2026-11-16", "12:00"), (Mode::Off, None), "peak hours of a blue day");
        assert_eq!(mode("2026-11-16", "23:00"), (Mode::Max, Some(7.0)), "off-peak hours of a blue day");
        assert_eq!(mode("2026-11-17", "03:00"), (Mode::Max, Some(7.0)), "the night still belongs to the blue Monday");
        assert_eq!(mode("2026-11-17", "07:00"), (Mode::Off, None), "red Tuesday");
        assert_eq!(mode("2026-11-17", "23:00"), (Mode::Off, None), "a red night");
        assert_eq!(mode("2026-11-18", "05:59"), (Mode::Off, None), "still the red day until 06:00");
        assert_eq!(mode("2026-11-18", "22:00"), (Mode::Max, Some(7.0)), "blue again");
    }

    #[test]
    fn the_periods_and_the_next_charge() {
        let p = blue_night();
        let list = periods(&p, &colors, Paris, at("2026-11-16", "06:00"), at("2026-11-17", "06:00"));
        let shape: Vec<(i64, Mode)> = list.iter().map(|p| ((p.end - p.start) / 3600, p.mode)).collect();
        assert_eq!(shape, vec![(16, Mode::Off), (8, Mode::Max)]);
        assert_eq!(list[1].color, Some(DayColor::Blue));
        // From Monday noon: the night of Monday.
        let n = next(&p, &colors, Paris, at("2026-11-16", "12:00")).unwrap();
        assert_eq!((n.start, n.end, n.kw), (at("2026-11-16", "22:00"), at("2026-11-17", "06:00"), Some(7.0)));
        // From Tuesday (red) morning: Wednesday night, the next blue one.
        let n = next(&p, &colors, Paris, at("2026-11-17", "08:00")).unwrap();
        assert_eq!(n.start, at("2026-11-18", "22:00"));
        // In the night: now.
        assert_eq!(next(&p, &colors, Paris, at("2026-11-16", "23:00")).unwrap().start, at("2026-11-16", "23:00"));
        // Three local days, each from midnight.
        let three = days(&p, &colors, Paris, day("2026-11-16"), 3);
        assert_eq!(three.iter().map(|d| d.color).collect::<Vec<_>>(), vec![Some(DayColor::Blue), Some(DayColor::Red), Some(DayColor::Blue)]);
        let monday: Vec<(i64, Mode)> = three[0].periods.iter().map(|p| ((p.end - p.start) / 3600, p.mode)).collect();
        // Sunday's night (no colour: its day of the week, blue) until 06:00, then Monday.
        assert_eq!(monday, vec![(6, Mode::Max), (16, Mode::Off), (2, Mode::Max)]);
    }

    #[test]
    fn kw_become_amps_within_the_limits_of_the_charger() {
        assert_eq!(amps_of(7.0, 230.0, 1, 6, 32), 30);
        assert_eq!(amps_of(7.0, 0.0, 1, 6, 32), 30, "230 V when it does not say");
        assert_eq!(amps_of(11.0, 230.0, 3, 6, 16), 15);
        assert_eq!(amps_of(22.0, 230.0, 1, 6, 32), 32);
        assert_eq!(amps_of(0.5, 230.0, 1, 6, 32), 6);
        assert_eq!(kw_of(30, 230.0, 1), 6.9);
    }

    #[test]
    fn a_charge_now_lasts_until_unplugged_a_time_or_its_energy() {
        let now = at("2026-11-16", "12:00");
        let asked = ChargerPlanRow { device_id: 1, profile_id: None, now_amps: Some(30), now_kw: Some(7.0), now_until: None, now_kwh: None, now_base_energy: Some(2.0) };
        let c = Charger { session_energy: 5.0, ..trydan() };
        assert_eq!(plan_now(&asked, None, &c, &colors, Paris, now), Some(Plan::Max { amps: 30, until: None, now: true, left_kwh: None }));
        assert_eq!(now_over(&asked, &Charger { plugged: false, ..c }, now), Some(NowOver::Unplugged));
        assert_eq!(now_over(&asked, &Charger { online: false, plugged: false, ..c }, now), None, "unreachable is not unplugged");
        let until = ChargerPlanRow { now_until: Some(now + 60), ..asked.clone() };
        assert_eq!(now_over(&until, &c, now), None);
        assert_eq!(now_over(&until, &c, now + 60), Some(NowOver::Time));
        // +5 kWh from 2 kWh: 3 done, 2 to go; done at 7.
        let kwh = ChargerPlanRow { now_kwh: Some(5.0), ..asked.clone() };
        assert_eq!(plan_now(&kwh, None, &c, &colors, Paris, now), Some(Plan::Max { amps: 30, until: None, now: true, left_kwh: Some(2.0) }));
        assert_eq!(now_over(&kwh, &Charger { session_energy: 7.0, ..c }, now), Some(NowOver::Energy));
        // Over: back to the profile.
        let p = blue_night();
        let with_profile = ChargerPlanRow { profile_id: Some(1), ..kwh };
        assert_eq!(plan_now(&with_profile, Some(&p), &Charger { session_energy: 7.0, ..c }, &colors, Paris, now), Some(Plan::Off { until: Some(at("2026-11-16", "22:00")) }));
    }

    #[test]
    fn the_plan_of_a_profile_at_a_moment() {
        let p = blue_night();
        let row = ChargerPlanRow { device_id: 1, profile_id: Some(1), ..Default::default() };
        let plan = |date, time| plan_now(&row, Some(&p), &trydan(), &colors, Paris, at(date, time));
        assert_eq!(plan("2026-11-16", "23:00"), Some(Plan::Max { amps: 30, until: Some(at("2026-11-17", "06:00")), now: false, left_kwh: None }));
        // Red Tuesday then a red night: no charge until Wednesday 22:00.
        assert_eq!(plan("2026-11-17", "07:00"), Some(Plan::Off { until: Some(at("2026-11-18", "22:00")) }));
        let mut solar = p.clone();
        solar.profiles[1].slots[0] = Slot { start: "00:00".into(), mode: Mode::Solar, kw: None, min_amps: Some(8), max_amps: None };
        assert_eq!(plan_now(&row, Some(&solar), &trydan(), &colors, Paris, at("2026-11-17", "12:00")), Some(Plan::Solar { min_amps: 8, max_amps: 16 }));
        assert_eq!(plan_now(&ChargerPlanRow { profile_id: None, ..row }, None, &trydan(), &colors, Paris, at("2026-11-17", "12:00")), None);
    }
}
