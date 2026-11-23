//! Solar surplus steering (roadmap E6): the power that would go back to the grid runs loads of the
//! home instead, in the order of their priority.
//!
//! * The **budget** is what goes back to the grid plus what the loads Breezone runs already use: it
//!   is handed out to the loads in order. A charger takes what it can between its minimum and its
//!   maximum current; a relay (water heater, pool pump…) is switched on when its power fits.
//! * **Steadiness**: a load starts once the surplus has been enough for [`START_DELAY`], stops once
//!   it has been short for [`STOP_DELAY`] (a cloud does not stop it), the current changes at most
//!   every [`ADJUST_EVERY`]; a relay stays on at least its minimum time. A `tolerance` lets a load
//!   draw a little from the grid before Breezone cuts back.
//! * **Guarantee** (a charger): "+20 kWh before 07:00". When the sun can no longer make it, the
//!   charger runs at its maximum current just in time.
//! * Only the loads and the grid meter of this server count: the surplus is physical, it is that of
//!   this site.
//! * A **zone of the air conditioning** (of an installation of this server) heats to its setpoint
//!   while it is cold outside and its system heats, cools while it is hot and its system cools; it
//!   is switched on only if it was off, and put back as it was (off, its setpoint) when the surplus
//!   runs short. A zone switched on, off or set otherwise by hand is left alone.
//! * A charger may follow a **plan** (its charging profile, or a charge asked now, see
//!   `charging`): no charge, charge at some current whatever the sun (taken off the budget first),
//!   or the surplus between other limits. Without a grid meter, only the surplus waits for one.

use crate::db::{self, SurplusLoadRow};
use crate::devices::{self, DeviceView};
use crate::i18n::Text;
use crate::AppState;
use chrono::{NaiveTime, TimeZone};
use chrono_tz::Tz;
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use tokio::sync::RwLock;

/// The surplus must have been enough this long before a load starts (seconds).
pub const START_DELAY: i64 = 60;
/// …and short this long before it stops.
pub const STOP_DELAY: i64 = 180;
/// The current of a charger changes at most this often.
pub const ADJUST_EVERY: i64 = 20;
/// A load stopped by Breezone does not start again before this.
pub const MIN_OFF: i64 = 120;
/// Time a charger takes to show what it was asked (a start, a stop): until then, what it shows is
/// not taken as done by hand.
const SETTLE: i64 = 120;
/// A guarantee starts this much earlier than needed.
const GUARANTEE_MARGIN: i64 = 15 * 60;
/// How often the surplus is looked at.
const EVERY: Duration = Duration::from_secs(10);
/// Voltage when the charger does not tell it.
const VOLTAGE: f64 = 230.0;

/// What a load is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Charger,
    Relay,
    Climate,
}

/// When a zone of the air conditioning runs on the surplus, and to which setpoint.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClimatePlan {
    pub heat_setpoint: f64,
    pub cool_setpoint: f64,
    /// It heats while the outdoor temperature is below this…
    pub heat_below: f64,
    /// …and cools while it is above this.
    pub cool_above: f64,
}

impl Default for ClimatePlan {
    fn default() -> Self {
        Self { heat_setpoint: 20.0, cool_setpoint: 24.0, heat_below: 14.0, cool_above: 25.0 }
    }
}

/// What the plan of a charger says now (see `charging`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Plan {
    /// No charge (until then, when it is known).
    Off { until: Option<i64> },
    /// Charge at this current, from the grid if need be: a slot of its profile, or a charge asked
    /// now (`now`, with what is `left` to add when it was asked for some kWh).
    Max { amps: i64, until: Option<i64>, now: bool, left_kwh: Option<f64> },
    /// The surplus, between these currents.
    Solar { min_amps: i64, max_amps: i64 },
}

/// A load and how it is run.
#[derive(Debug, Clone, PartialEq)]
pub struct Load {
    pub device_id: i64,
    pub kind: Kind,
    pub name: String,
    pub min_amps: i64,
    pub max_amps: i64,
    /// Power of a relay when on (W).
    pub power: f64,
    /// A charger: energy to add (kWh) before a time.
    pub guarantee: Option<(f64, NaiveTime)>,
    pub min_on: i64,
    /// What may be drawn from the grid before cutting back (W).
    pub tolerance: f64,
    /// A zone of the air conditioning.
    pub climate: ClimatePlan,
    /// A charger: what its plan says now (`None`: the surplus, as configured).
    pub plan: Option<Plan>,
    /// The name of the charging profile it follows, for the texts.
    pub profile: Option<String>,
}

/// A load now.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Reading {
    /// The device answers.
    pub online: bool,
    /// A charger: a car is plugged in.
    pub plugged: bool,
    /// It runs (a charger charges, unpaused; a relay is on).
    pub running: bool,
    /// A charger: paused (its own flag, kept by the charger: it does not start by itself then).
    pub paused: bool,
    /// What it draws now (W).
    pub power: f64,
    pub voltage: f64,
    pub phases: u32,
    /// A charger: energy of the session (kWh).
    pub session_energy: f64,
    /// A charger: current asked (A).
    pub amps: Option<i64>,
    /// A charger: it sets its current itself, from its own clamp (load shedding).
    pub auto_limit: bool,
    /// A zone: its system heats (or is in auto), cools (or is in auto).
    pub heats: bool,
    pub cools: bool,
    /// A zone: its setpoint now.
    pub setpoint: Option<f64>,
    /// A zone: the outdoor temperature where it is.
    pub outdoor: Option<f64>,
}

/// What Breezone tells a load.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    /// A charger: charge at this current.
    Start(i64),
    /// A charger: charge, at the current it sets itself.
    Resume,
    /// A charger: change the current.
    Amps(i64),
    Stop,
    On,
    Off,
    /// A zone: on, at this setpoint.
    ClimateOn(f64),
    /// A zone: off, its setpoint put back.
    ClimateOff(Option<f64>),
}

/// What a load does, for the pages.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Status {
    /// "solar" (runs on the surplus) | "guarantee" (runs from the grid to keep the guarantee) |
    /// "waiting" (not enough surplus) | "no_car" | "unavailable" | "no_meter" | "off" (not steered) |
    /// "idle" (a zone: no need now) | "manual" (set by hand, left alone) | "no_charge" (its
    /// profile says no charge now) | "charging" (its profile says charge) | "now" (charge asked now)
    pub state: &'static str,
    /// A charger: the current Breezone asks.
    pub amps: Option<i64>,
    /// What it draws now (W).
    pub power: f64,
    /// In words: "Charge solaire à 9 A", "Attend 1,4 kW de surplus".
    pub text: Text,
}

/// What is remembered of a load between two looks.
#[derive(Debug, Clone, Copy, Default)]
struct Memory {
    /// Since when the surplus is enough to start it.
    enough_since: Option<i64>,
    /// Since when it is short (a running load).
    short_since: Option<i64>,
    /// When Breezone last changed it.
    changed_at: Option<i64>,
    /// When Breezone switched it on.
    on_since: Option<i64>,
    /// The current Breezone last asked.
    amps: Option<i64>,
    /// A zone: Breezone switched it on (else it was on by hand, or is off).
    ours: bool,
    /// A zone: its setpoint before, put back after.
    before: Option<f64>,
    /// A zone: the setpoint Breezone gave it.
    asked: Option<f64>,
    /// A charger stopped, started or set by hand: left alone until its car is unplugged.
    by_hand: bool,
    /// A charger: seen paused since the surplus last started it (a charge going again then was
    /// resumed by someone else, its own app or its button).
    waiting: bool,
}

/// Decides, look after look, what each load does.
#[derive(Debug, Default)]
pub struct Regulator {
    memory: HashMap<i64, Memory>,
}

impl Regulator {
    /// A charger was stopped, started or set by hand: the surplus leaves it until its car is
    /// unplugged (else it would start again what was just stopped).
    pub fn by_hand(&mut self, device_id: i64) {
        self.memory.insert(device_id, Memory { by_hand: true, ..Memory::default() });
    }
}

/// When `at` ("07:00") next comes after `now`, in `tz`.
fn next_time(now: i64, at: NaiveTime, tz: Tz) -> Option<i64> {
    let local = tz.timestamp_opt(now, 0).single()?;
    (0..2).filter_map(|d| (local.date_naive() + chrono::Days::new(d)).and_time(at).and_local_timezone(tz).earliest()).map(|t| t.timestamp()).find(|t| *t > now)
}

fn kw(w: f64) -> String {
    let k = (w / 100.0).round() / 10.0;
    format!("{k}").replace('.', ",")
}

/// A power in kW, with the decimal mark of each language.
fn kw_text(w: f64) -> Text {
    let k = kw(w);
    Text { fr: k.clone(), en: k.replace(',', "."), es: k }
}

/// When a plan ends, in words: "22 h", "demain 6 h", "le 24/11 à 6 h".
fn until_text(now: i64, until: i64, tz: Tz) -> Text {
    let (Some(n), Some(u)) = (tz.timestamp_opt(now, 0).single(), tz.timestamp_opt(until, 0).single()) else { return Text::default() };
    use chrono::Timelike;
    let fr = if u.minute() == 0 { format!("{} h", u.hour()) } else { format!("{} h {:02}", u.hour(), u.minute()) };
    let hm = u.format("%H:%M").to_string();
    let days = (u.date_naive() - n.date_naive()).num_days();
    match days {
        0 => Text { fr, en: hm.clone(), es: hm },
        1 => Text { fr: format!("demain {fr}"), en: format!("tomorrow {hm}"), es: format!("mañana {hm}") },
        _ => {
            let d = u.format("%d/%m").to_string();
            Text { fr: format!("le {d} à {fr}"), en: format!("{} {hm}", u.format("%m/%d")), es: format!("el {d} a las {hm}") }
        }
    }
}

/// What a charger does by its plan, in words.
fn plan_text(plan: Plan, profile: Option<&str>, watts: Option<f64>, now: i64, tz: Tz) -> Text {
    let p = profile.unwrap_or_default().to_string();
    // How it charges: at some power, or as the charger sets it (its own load shedding).
    let how = match watts {
        Some(w) => {
            let k = kw_text(w);
            crate::text!("à {0} kW", "at {0} kW", "a {0} kW", k.arg())
        }
        None => crate::text!("au courant réglé par la borne", "at the current set by the charger", "a la corriente regulada por el cargador"),
    };
    match plan {
        Plan::Off { until: Some(u) } => {
            let t = until_text(now, u, tz);
            crate::text!("Profil {0} : pas de charge jusqu'à {1}", "Profile {0}: no charging until {1}", "Perfil {0}: sin carga hasta {1}", p, t.arg())
        }
        Plan::Off { until: None } => crate::text!("Profil {0} : pas de charge", "Profile {0}: no charging", "Perfil {0}: sin carga", p),
        Plan::Max { now: false, until, .. } => match until {
            Some(u) => {
                let t = until_text(now, u, tz);
                crate::text!("Profil {0} : charge {1} jusqu'à {2}", "Profile {0}: charging {1} until {2}", "Perfil {0}: carga {1} hasta {2}", p, how.arg(), t.arg())
            }
            None => crate::text!("Profil {0} : charge {1}", "Profile {0}: charging {1}", "Perfil {0}: carga {1}", p, how.arg()),
        },
        Plan::Max { now: true, until, left_kwh, .. } => {
            let head = match until {
                Some(u) => {
                    let t = until_text(now, u, tz);
                    crate::text!("Charge maintenant {0} jusqu'à {1}", "Charging now {0} until {1}", "Carga ahora {0} hasta {1}", how.arg(), t.arg())
                }
                None if left_kwh.is_some() => crate::text!("Charge maintenant {0}", "Charging now {0}", "Carga ahora {0}", how.arg()),
                None => crate::text!("Charge maintenant {0} jusqu'au débranchement", "Charging now {0} until unplugged", "Carga ahora {0} hasta desconectar", how.arg()),
            };
            match left_kwh {
                Some(left) => {
                    let l = kw_text(left * 1000.0);
                    crate::text!("{0}, encore {1} kWh", "{0}, {1} kWh to go", "{0}, faltan {1} kWh", head.arg(), l.arg())
                }
                None => head,
            }
        }
        Plan::Solar { .. } => Text::default(),
    }
}

impl Regulator {
    /// One look: `export` is what goes back to the grid (W, negative: drawn from it), `loads` in
    /// order of priority. Returns the commands to send and the status of every load.
    pub fn step(&mut self, now: i64, tz: Tz, export: f64, loads: &[(Load, Reading)]) -> (Vec<(i64, Command)>, HashMap<i64, Status>) {
        self.step_with(now, tz, Some(export), loads)
    }

    /// [`Regulator::step`], with or without a grid meter (`None`): without one, only a charger
    /// whose plan says no charge, or charge whatever the sun, is run.
    pub fn step_with(&mut self, now: i64, tz: Tz, export: Option<f64>, loads: &[(Load, Reading)]) -> (Vec<(i64, Command)>, HashMap<i64, Status>) {
        self.memory.retain(|id, _| loads.iter().any(|(l, _)| l.device_id == *id));
        let meter = export.is_some();
        let export = export.unwrap_or(0.0);
        // What the loads Breezone runs use is part of the surplus.
        // A relay that measures nothing counts for its power.
        let used = |l: &Load, r: &Reading| if matches!(l.kind, Kind::Relay | Kind::Climate) && r.power <= 0.0 { l.power } else { r.power.max(0.0) };
        // A zone counts only when Breezone runs it (one on by hand is not part of the surplus).
        let ours = |l: &Load| l.kind != Kind::Climate || self.memory.get(&l.device_id).is_some_and(|m| m.ours);
        let mut budget = export + loads.iter().filter(|(l, r)| r.online && r.running && ours(l)).map(|(l, r)| used(l, r)).sum::<f64>();
        let mut commands = vec![];
        let mut statuses = HashMap::new();
        for (load, r) in loads {
            let m = self.memory.entry(load.device_id).or_default();
            let status = |state, amps, text| Status { state, amps, power: r.power, text };
            if !r.online {
                *m = Memory::default();
                statuses.insert(load.device_id, status("unavailable", None, crate::text!("Injoignable", "Unreachable", "Inaccesible")));
                continue;
            }
            let planned = load.kind == Kind::Charger && matches!(load.plan, Some(Plan::Off { .. } | Plan::Max { .. }));
            if !meter && !planned {
                let text =
                    crate::text!("Pas de compteur réseau : le surplus est inconnu", "No grid meter: the surplus is unknown", "Sin contador de red: el excedente es desconocido");
                statuses.insert(load.device_id, status("no_meter", None, text));
                continue;
            }
            let rested = m.changed_at.is_none_or(|t| now - t >= MIN_OFF);
            match load.kind {
                Kind::Charger => {
                    if !r.plugged {
                        *m = Memory::default();
                        statuses.insert(load.device_id, status("no_car", None, crate::text!("Pas de voiture branchée", "No car plugged in", "Ningún coche conectado")));
                        continue;
                    }
                    // Stopped (or started, or set) by hand: what was asked holds, the guarantee too.
                    if m.by_hand {
                        let text = crate::text!(
                            "Réglée à la main : le surplus la reprend au prochain branchement",
                            "Set by hand: the surplus takes it back at the next plugging in",
                            "Ajustada a mano: el excedente la retoma en la próxima conexión"
                        );
                        statuses.insert(load.device_id, status("manual", None, text));
                        continue;
                    }
                    // Resumed (seen paused, then charging) or paused (after the surplus started it)
                    // by someone else, its own app or its button: left alone until unplugged. A
                    // charge that starts by itself when the car is plugged in (the charger was not
                    // paused) stays the surplus's, and so does a car that is full.
                    let settled = m.changed_at.is_none_or(|t| now - t >= SETTLE);
                    if settled && ((r.running && m.waiting && !m.ours) || (r.paused && m.ours)) {
                        *m = Memory { by_hand: true, ..Memory::default() };
                        let text = crate::text!(
                            "Réglée à la main : le surplus la reprend au prochain branchement",
                            "Set by hand: the surplus takes it back at the next plugging in",
                            "Ajustada a mano: el excedente la retoma en la próxima conexión"
                        );
                        statuses.insert(load.device_id, status("manual", None, text));
                        continue;
                    }
                    if r.paused {
                        m.waiting = true;
                    }
                    let per_amp = (if r.voltage > 0.0 { r.voltage } else { VOLTAGE }) * r.phases.max(1) as f64;
                    // Its plan: no charge, or a charge whatever the sun; or the surplus within its limits.
                    let solar_limits;
                    let load = match load.plan {
                        Some(plan @ Plan::Off { .. }) => {
                            if r.running {
                                // Still drawing until it shows the stop.
                                budget -= r.power.max(0.0);
                                if settled {
                                    commands.push((load.device_id, Command::Stop));
                                    *m = Memory { changed_at: Some(now), ..Memory::default() };
                                }
                            }
                            statuses.insert(load.device_id, status("no_charge", None, plan_text(plan, load.profile.as_deref(), None, now, tz)));
                            continue;
                        }
                        Some(plan @ Plan::Max { amps, now: asked_now, .. }) => {
                            // A charger that sets its own current (its load shedding) keeps it.
                            let own = r.auto_limit;
                            if !r.running {
                                // Started once: a car that is full, or that waits for its own
                                // schedule, is not asked again (a pause by hand was seen above).
                                if !m.ours && settled {
                                    commands.push((load.device_id, if own { Command::Resume } else { Command::Start(amps) }));
                                    *m = Memory { changed_at: Some(now), on_since: Some(now), ours: true, ..Memory::default() };
                                }
                            } else {
                                // Charging by its plan: a pause now is one by hand.
                                m.ours = true;
                                if !own && r.amps != Some(amps) && m.changed_at.is_none_or(|t| now - t >= ADJUST_EVERY) {
                                    commands.push((load.device_id, Command::Amps(amps)));
                                    m.changed_at = Some(now);
                                }
                            }
                            m.amps = (!own).then_some(amps);
                            m.short_since = None;
                            m.enough_since = None;
                            let watts = if own { r.power.max(0.0) } else { amps as f64 * per_amp };
                            budget -= watts;
                            let text = plan_text(plan, load.profile.as_deref(), (!own).then_some(watts), now, tz);
                            statuses.insert(load.device_id, status(if asked_now { "now" } else { "charging" }, (!own).then_some(amps), text));
                            continue;
                        }
                        Some(Plan::Solar { min_amps, max_amps }) => {
                            solar_limits = Load { min_amps, max_amps, ..load.clone() };
                            &solar_limits
                        }
                        None => load,
                    };
                    // The guarantee first: from the grid if the sun can no longer make it.
                    let forced = load.guarantee.and_then(|(kwh, by)| {
                        let left = kwh - r.session_energy;
                        let deadline = next_time(now, by, tz)?;
                        let needed = (left * 3_600_000.0 / (load.max_amps as f64 * per_amp)).ceil() as i64;
                        (left > 0.0 && now + needed + GUARANTEE_MARGIN >= deadline).then_some(left)
                    });
                    if let Some(left) = forced {
                        let amps = load.max_amps;
                        if !r.running {
                            commands.push((load.device_id, Command::Start(amps)));
                            m.changed_at = Some(now);
                            m.ours = true;
                            m.waiting = false;
                        } else if r.amps != Some(amps) && m.changed_at.is_none_or(|t| now - t >= ADJUST_EVERY) {
                            commands.push((load.device_id, Command::Amps(amps)));
                            m.changed_at = Some(now);
                        }
                        m.amps = Some(amps);
                        m.on_since.get_or_insert(now);
                        budget -= amps as f64 * per_amp;
                        let text = crate::text!(
                            "Garantie : {0} A depuis le réseau, encore {1} kWh",
                            "Guarantee: {0} A from the grid, {1} kWh to go",
                            "Garantía: {0} A de la red, faltan {1} kWh",
                            amps,
                            kw(left * 1000.0)
                        );
                        statuses.insert(load.device_id, status("guarantee", Some(amps), text));
                        continue;
                    }
                    let fits = (((budget + load.tolerance) / per_amp).floor() as i64).min(load.max_amps);
                    let enough = fits >= load.min_amps;
                    if r.running {
                        let amps = if enough {
                            m.short_since = None;
                            fits
                        } else {
                            let since = *m.short_since.get_or_insert(now);
                            if now - since >= STOP_DELAY {
                                commands.push((load.device_id, Command::Stop));
                                *m = Memory { changed_at: Some(now), ..Memory::default() };
                                statuses.insert(load.device_id, status("waiting", None, wait_text(load.min_amps as f64 * per_amp)));
                                continue;
                            }
                            // A cloud: it holds at its minimum a little while.
                            load.min_amps
                        };
                        let current = m.amps.or(r.amps);
                        if current != Some(amps) && m.changed_at.is_none_or(|t| now - t >= ADJUST_EVERY) {
                            commands.push((load.device_id, Command::Amps(amps)));
                            m.changed_at = Some(now);
                            m.amps = Some(amps);
                        }
                        let shown = m.amps.or(r.amps).unwrap_or(amps);
                        budget -= shown as f64 * per_amp;
                        statuses
                            .insert(load.device_id, status("solar", Some(shown), crate::text!("Charge solaire à {0} A", "Solar charging at {0} A", "Carga solar a {0} A", shown)));
                    } else {
                        m.short_since = None;
                        if enough {
                            let since = *m.enough_since.get_or_insert(now);
                            if now - since >= START_DELAY && rested {
                                commands.push((load.device_id, Command::Start(fits)));
                                *m = Memory { changed_at: Some(now), on_since: Some(now), amps: Some(fits), ours: true, ..Memory::default() };
                                budget -= fits as f64 * per_amp;
                                statuses.insert(
                                    load.device_id,
                                    status("solar", Some(fits), crate::text!("Charge solaire à {0} A", "Solar charging at {0} A", "Carga solar a {0} A", fits)),
                                );
                                continue;
                            }
                        } else {
                            m.enough_since = None;
                        }
                        statuses.insert(load.device_id, status("waiting", None, wait_text(load.min_amps as f64 * per_amp)));
                    }
                }
                Kind::Relay => {
                    let need = load.power.max(1.0);
                    if r.running {
                        let used = if r.power > 0.0 { r.power } else { need };
                        let held = m.on_since.is_none_or(|t| now - t >= load.min_on);
                        if budget + load.tolerance >= used.min(need) {
                            m.short_since = None;
                        } else {
                            let since = *m.short_since.get_or_insert(now);
                            if now - since >= STOP_DELAY && held {
                                commands.push((load.device_id, Command::Off));
                                *m = Memory { changed_at: Some(now), ..Memory::default() };
                                statuses.insert(load.device_id, status("waiting", None, wait_text(need)));
                                continue;
                            }
                        }
                        budget -= used;
                        statuses.insert(load.device_id, status("solar", None, crate::text!("En marche sur le surplus", "Running on the surplus", "En marcha con el excedente")));
                    } else {
                        if budget + load.tolerance >= need {
                            let since = *m.enough_since.get_or_insert(now);
                            if now - since >= START_DELAY && rested {
                                commands.push((load.device_id, Command::On));
                                *m = Memory { changed_at: Some(now), on_since: Some(now), ..Memory::default() };
                                budget -= need;
                                statuses.insert(
                                    load.device_id,
                                    status("solar", None, crate::text!("En marche sur le surplus", "Running on the surplus", "En marcha con el excedente")),
                                );
                                continue;
                            }
                        } else {
                            m.enough_since = None;
                        }
                        statuses.insert(load.device_id, status("waiting", None, wait_text(need)));
                    }
                }
                Kind::Climate => {
                    let plan = load.climate;
                    let need = load.power.max(1.0);
                    // What it should do now: heat, cool, or nothing (and why).
                    let want: Result<f64, Text> = match r.outdoor {
                        None => Err(crate::text!("Température extérieure inconnue", "Outdoor temperature unknown", "Temperatura exterior desconocida")),
                        Some(t) if t < plan.heat_below && r.heats => Ok(plan.heat_setpoint),
                        Some(t) if t > plan.cool_above && r.cools => Ok(plan.cool_setpoint),
                        Some(t) if t < plan.heat_below => Err(crate::text!(
                            "Froid dehors ({0}°), mais le système n'est pas en chauffage",
                            "Cold outside ({0}°), but the system does not heat",
                            "Frío fuera ({0}°), pero el sistema no está en calefacción",
                            t.round()
                        )),
                        Some(t) if t > plan.cool_above => Err(crate::text!(
                            "Chaud dehors ({0}°), mais le système n'est pas en froid",
                            "Hot outside ({0}°), but the system does not cool",
                            "Calor fuera ({0}°), pero el sistema no está en frío",
                            t.round()
                        )),
                        Some(t) => Err(crate::text!("Pas besoin : {0}° dehors", "No need: {0}° outside", "Sin necesidad: {0}° fuera", t.round())),
                    };
                    let running_text = |sp: f64| {
                        if r.outdoor.is_some_and(|t| t < plan.heat_below) {
                            crate::text!("Chauffe à {0}° sur le surplus", "Heating to {0}° on the surplus", "Calienta a {0}° con el excedente", sp)
                        } else {
                            crate::text!("Rafraîchit à {0}° sur le surplus", "Cooling to {0}° on the surplus", "Enfría a {0}° con el excedente", sp)
                        }
                    };
                    if m.ours {
                        // Switched off, or set otherwise, by hand: left alone from now on.
                        let moved = matches!((m.asked, r.setpoint), (Some(a), Some(s)) if (a - s).abs() > 0.05);
                        if !r.running || moved {
                            *m = Memory { changed_at: m.changed_at, ..Memory::default() };
                            let text = crate::text!("Réglée à la main : le surplus la laisse", "Set by hand: the surplus leaves it", "Ajustada a mano: el excedente la deja");
                            statuses.insert(load.device_id, status("manual", None, text));
                            continue;
                        }
                        let held = m.on_since.is_none_or(|t| now - t >= load.min_on);
                        if want.is_ok() && budget + load.tolerance >= need {
                            m.short_since = None;
                        } else {
                            let since = *m.short_since.get_or_insert(now);
                            if now - since >= STOP_DELAY && held {
                                commands.push((load.device_id, Command::ClimateOff(m.before)));
                                *m = Memory { changed_at: Some(now), ..Memory::default() };
                                let text = match want {
                                    Ok(_) => wait_text(need),
                                    Err(why) => why,
                                };
                                statuses.insert(load.device_id, status("waiting", None, text));
                                continue;
                            }
                        }
                        budget -= need;
                        statuses.insert(load.device_id, status("solar", None, running_text(m.asked.unwrap_or(plan.heat_setpoint))));
                    } else if r.running {
                        m.enough_since = None;
                        let text = crate::text!("Allumée à la main : le surplus la laisse", "On by hand: the surplus leaves it", "Encendida a mano: el excedente la deja");
                        statuses.insert(load.device_id, status("manual", None, text));
                    } else {
                        match want {
                            Err(why) => {
                                m.enough_since = None;
                                statuses.insert(load.device_id, status("idle", None, why));
                            }
                            Ok(sp) => {
                                if budget + load.tolerance >= need {
                                    let since = *m.enough_since.get_or_insert(now);
                                    if now - since >= START_DELAY && rested {
                                        commands.push((load.device_id, Command::ClimateOn(sp)));
                                        *m = Memory { changed_at: Some(now), on_since: Some(now), ours: true, before: r.setpoint, asked: Some(sp), ..Memory::default() };
                                        budget -= need;
                                        statuses.insert(load.device_id, status("solar", None, running_text(sp)));
                                        continue;
                                    }
                                } else {
                                    m.enough_since = None;
                                }
                                statuses.insert(load.device_id, status("waiting", None, wait_text(need)));
                            }
                        }
                    }
                }
            }
        }
        (commands, statuses)
    }
}

/// "Attend 1,4 kW de surplus".
fn wait_text(watts: f64) -> Text {
    let k = kw(watts);
    Text { fr: format!("Attend {k} kW de surplus"), en: format!("Waits for {} kW of surplus", k.replace(',', ".")), es: format!("Espera {k} kW de excedente") }
}

// ---------------------------------------------------------------------------------------
// Running it
// ---------------------------------------------------------------------------------------

/// What the pages read: the surplus now and what each load does.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Snapshot {
    /// The home has a grid meter: without one there is no surplus to steer.
    pub meter: bool,
    /// What goes back to the grid now (W; negative: drawn from it).
    pub export: Option<f64>,
    pub statuses: HashMap<i64, Status>,
}

#[derive(Default)]
pub struct Surplus {
    regulator: tokio::sync::Mutex<Regulator>,
    snapshot: RwLock<Snapshot>,
}

impl Surplus {
    pub async fn snapshot(&self) -> Snapshot {
        self.snapshot.read().await.clone()
    }

    /// See [`Regulator::by_hand`].
    pub async fn by_hand(&self, device_id: i64) {
        self.regulator.lock().await.by_hand(device_id);
    }

    /// A load given (back) to the surplus: what it remembered of it goes (a stop by hand above all).
    pub async fn release(&self, device_id: i64) {
        self.regulator.lock().await.memory.remove(&device_id);
    }
}

/// The kind of a device that can be steered on the surplus: a charger, a relay of this server, or
/// a zone of an installation of this server (`local`: their ids).
pub fn kind_of(d: &DeviceView, local: &HashSet<i64>) -> Option<Kind> {
    if let Some(inst) = d.installation_id {
        let zone = d.traits.iter().any(|t| t.starts_with("climate.zone@"));
        return (zone && local.contains(&inst)).then_some(Kind::Climate);
    }
    d.source_id?;
    if d.traits.iter().any(|t| t.starts_with("ev.charger@")) {
        Some(Kind::Charger)
    } else if d.traits.iter().any(|t| t.starts_with("switch.relay@")) {
        Some(Kind::Relay)
    } else {
        None
    }
}

fn num(state: &Value, field: &str) -> Option<f64> {
    state.get(field).and_then(Value::as_f64)
}

/// A load as configured, with what its device says of itself.
pub fn load_of(row: &SurplusLoadRow, d: &DeviceView, kind: Kind) -> Load {
    let (lo, hi) = (num(&d.state, "limit_min").unwrap_or(6.0) as i64, num(&d.state, "limit_max").unwrap_or(32.0) as i64);
    Load {
        device_id: d.id,
        kind,
        name: d.name.clone(),
        min_amps: row.min_amps.unwrap_or(lo).clamp(lo, hi),
        max_amps: row.max_amps.unwrap_or(16.min(hi)).clamp(lo, hi).max(row.min_amps.unwrap_or(lo)),
        power: row.power.unwrap_or(0) as f64,
        guarantee: match (row.guarantee_kwh, row.guarantee_by.as_deref().and_then(|t| NaiveTime::parse_from_str(t, "%H:%M").ok())) {
            (Some(k), Some(t)) if k > 0.0 => Some((k, t)),
            _ => None,
        },
        min_on: row.min_on_minutes * 60,
        tolerance: row.tolerance.max(0) as f64,
        climate: {
            let d = ClimatePlan::default();
            ClimatePlan {
                heat_setpoint: row.heat_setpoint.unwrap_or(d.heat_setpoint),
                cool_setpoint: row.cool_setpoint.unwrap_or(d.cool_setpoint),
                heat_below: row.heat_below.unwrap_or(d.heat_below),
                cool_above: row.cool_above.unwrap_or(d.cool_above),
            }
        },
        plan: None,
        profile: None,
    }
}

/// The installations of this server (not those reached through another one): their zones can
/// follow the surplus of this site.
pub async fn local_installations(state: &AppState) -> HashSet<i64> {
    db::list_installations(&state.db).await.unwrap_or_default().into_iter().filter(|i| i.node_id.is_none()).map(|i| i.id).collect()
}

fn reading_of(d: &DeviceView, kind: Kind, outdoor: Option<f64>) -> Reading {
    let s = &d.state;
    let flag = |f: &str| s.get(f).and_then(Value::as_bool);
    match kind {
        Kind::Charger => {
            let status = s.get("status").and_then(Value::as_str);
            Reading {
                online: d.available,
                plugged: matches!(status, Some("plugged" | "charging")),
                running: status == Some("charging") && !flag("paused").unwrap_or(false),
                paused: flag("paused").unwrap_or(false),
                power: num(s, "power").unwrap_or(0.0),
                voltage: num(s, "voltage").unwrap_or(0.0),
                phases: num(s, "phases").unwrap_or(1.0) as u32,
                session_energy: num(s, "session_energy").unwrap_or(0.0),
                amps: num(s, "limit").map(|a| a as i64),
                auto_limit: flag("auto_limit").unwrap_or(false),
                ..Reading::default()
            }
        }
        Kind::Relay => Reading { online: d.available, plugged: true, running: flag("on").unwrap_or(false), power: num(s, "power").unwrap_or(0.0), ..Reading::default() },
        Kind::Climate => {
            let mode = s.get("mode").and_then(Value::as_str);
            Reading {
                online: d.available,
                plugged: true,
                running: flag("on").unwrap_or(false),
                heats: matches!(mode, Some("heating" | "aux_heating" | "auto")),
                cools: matches!(mode, Some("cooling" | "auto")),
                setpoint: num(s, "setpoint"),
                outdoor,
                ..Reading::default()
            }
        }
    }
}

/// Sends a command to a device.
async fn send(state: &AppState, d: &DeviceView, c: Command) -> Result<(), String> {
    // A zone: as any change of a zone, through its installation.
    if let Command::ClimateOn(_) | Command::ClimateOff(_) = c {
        let mut params = Map::new();
        match c {
            Command::ClimateOn(sp) => {
                params.insert("on".into(), json!(true));
                params.insert("setpoint".into(), json!(sp));
            }
            Command::ClimateOff(before) => {
                params.insert("on".into(), json!(false));
                if let Some(sp) = before {
                    params.insert("setpoint".into(), json!(sp));
                }
            }
            _ => {}
        }
        let input = devices::ActionInput { trait_name: "climate.zone".into(), action: "set".into(), params };
        return devices::apply(state, d, &input, "Breezone").await.map_err(|e| e.message.en);
    }
    let source = d.source_id.ok_or("no source")?;
    let reference = d.reference.as_str().unwrap_or_default();
    let params = |v: Value| v.as_object().cloned().unwrap_or_else(Map::new);
    let (trait_name, action, p) = match c {
        Command::Start(a) => ("ev.charger", "start", params(json!({ "limit": a }))),
        Command::Resume => ("ev.charger", "start", Map::new()),
        Command::Amps(a) => ("ev.charger", "set_limit", params(json!({ "limit": a }))),
        Command::Stop => ("ev.charger", "stop", Map::new()),
        Command::On => ("switch.relay", "set", params(json!({ "on": true }))),
        Command::Off => ("switch.relay", "set", params(json!({ "on": false }))),
        Command::ClimateOn(_) | Command::ClimateOff(_) => return Ok(()),
    };
    state.drivers.act(source, reference, trait_name, action, &p).await.map_err(|e| e.to_string())
}

/// Looks at the surplus once (at `now`): sends what the loads must do, keeps their status.
pub async fn tick(state: &AppState, now: i64) {
    let rows = match db::list_surplus_loads(&state.db).await {
        Ok(rows) => rows,
        Err(e) => return tracing::error!("surplus: {e}"),
    };
    let devices = match devices::list(state).await {
        Ok(d) => d,
        Err(e) => return tracing::error!("surplus: {}", e.message.en),
    };
    let grid = match crate::energy::live_of(&state.db, &devices).await {
        Ok(live) => live.live.flows.grid,
        Err(e) => return tracing::error!("surplus: {e}"),
    };
    let local = local_installations(state).await;
    let steered: Vec<(&SurplusLoadRow, &DeviceView, Kind)> =
        rows.iter().filter(|r| r.enabled).filter_map(|r| devices.iter().find(|d| d.id == r.device_id).and_then(|d| kind_of(d, &local).map(|k| (r, d, k)))).collect();
    // The outdoor temperature of each installation of a zone that follows the surplus.
    let mut outdoor: HashMap<i64, Option<f64>> = HashMap::new();
    for (_, d, _) in &steered {
        if let Some(inst) = d.installation_id {
            if let std::collections::hash_map::Entry::Vacant(e) = outdoor.entry(inst) {
                e.insert(state.weather.view(Some(inst)).await.weather.and_then(|w| w.temperature));
            }
        }
    }
    let mut loads: Vec<(Load, Reading)> =
        steered.iter().map(|(r, d, k)| (load_of(r, d, *k), reading_of(d, *k, d.installation_id.and_then(|i| outdoor.get(&i).copied().flatten())))).collect();
    // The chargers that follow a plan, steered on the surplus or not.
    for (id, (plan, profile)) in crate::charging::plans(state, &devices, now).await {
        match loads.iter_mut().find(|(l, _)| l.device_id == id) {
            Some((l, _)) if l.kind == Kind::Charger => {
                l.plan = Some(plan);
                l.profile = profile;
            }
            Some(_) => {}
            None => {
                let Some(d) = devices.iter().find(|d| d.id == id).filter(|d| kind_of(d, &local) == Some(Kind::Charger)) else { continue };
                let mut l = load_of(&SurplusLoadRow::charger(id), d, Kind::Charger);
                l.plan = Some(plan);
                l.profile = profile;
                loads.push((l, reading_of(d, Kind::Charger, None)));
            }
        }
    }
    // What runs whatever the sun first: what is left is the surplus of the others.
    loads.sort_by_key(|(l, _)| !matches!(l.plan, Some(Plan::Off { .. } | Plan::Max { .. })));
    let export = grid.map(|g| -g);
    let (commands, statuses) = state.surplus.regulator.lock().await.step_with(now, state.cfg.timezone, export, &loads);
    for (id, c) in commands {
        let Some(d) = devices.iter().find(|d| d.id == id) else { continue };
        let name = d.name.clone();
        let message = match c {
            Command::Start(a) if statuses.get(&id).is_some_and(|s| s.state == "guarantee") => {
                crate::text!("{0} : charge à {1} A pour tenir la garantie", "{0}: charging at {1} A to keep the guarantee", "{0}: carga a {1} A para cumplir la garantía", name, a)
            }
            Command::Start(a) if statuses.get(&id).is_some_and(|s| s.state == "now") => {
                crate::text!("{0} : charge maintenant à {1} A", "{0}: charging now at {1} A", "{0}: carga ahora a {1} A", name, a)
            }
            Command::Start(a) if statuses.get(&id).is_some_and(|s| s.state == "charging") => {
                crate::text!("{0} : charge à {1} A selon son profil", "{0}: charging at {1} A by its profile", "{0}: carga a {1} A según su perfil", name, a)
            }
            Command::Start(a) => crate::text!("{0} : charge solaire à {1} A", "{0}: solar charging at {1} A", "{0}: carga solar a {1} A", name, a),
            Command::Resume if statuses.get(&id).is_some_and(|s| s.state == "now") => {
                crate::text!(
                    "{0} : charge maintenant, courant réglé par la borne",
                    "{0}: charging now, current set by the charger",
                    "{0}: carga ahora, corriente regulada por el cargador",
                    name
                )
            }
            Command::Resume => crate::text!(
                "{0} : charge selon son profil, courant réglé par la borne",
                "{0}: charging by its profile, current set by the charger",
                "{0}: carga según su perfil, corriente regulada por el cargador",
                name
            ),
            Command::Stop if statuses.get(&id).is_some_and(|s| s.state == "no_charge") => {
                crate::text!("{0} : charge suspendue selon son profil", "{0}: charging paused by its profile", "{0}: carga suspendida según su perfil", name)
            }
            Command::Stop => {
                crate::text!("{0} : charge suspendue, plus assez de surplus", "{0}: charging paused, not enough surplus", "{0}: carga suspendida, excedente insuficiente", name)
            }
            Command::On => crate::text!("{0} : en marche sur le surplus solaire", "{0}: on, on the solar surplus", "{0}: en marcha con el excedente solar", name),
            Command::Off => crate::text!("{0} : arrêté, plus assez de surplus", "{0}: off, not enough surplus", "{0}: apagado, excedente insuficiente", name),
            Command::ClimateOn(sp) => {
                crate::text!("{0} : allumée à {1}° sur le surplus solaire", "{0}: on at {1}° on the solar surplus", "{0}: encendida a {1}° con el excedente solar", name, sp)
            }
            Command::ClimateOff(_) => {
                crate::text!("{0} : remise comme avant, plus de surplus", "{0}: put back as it was, no more surplus", "{0}: devuelta como estaba, sin excedente", name)
            }
            Command::Amps(_) => {
                if let Err(e) = send(state, d, c).await {
                    tracing::warn!("surplus: {name}: {e}");
                }
                continue;
            }
        };
        match send(state, d, c).await {
            Ok(()) => state.hub.log("automation", "info", None, Some("Breezone"), message, None).await,
            Err(e) => {
                let text = crate::text!(
                    "{0} : le pilotage sur surplus n'a pas pu agir ({1})",
                    "{0}: the surplus steering could not act ({1})",
                    "{0}: el pilotaje por excedente no pudo actuar ({1})",
                    name,
                    e
                );
                state.hub.log("automation", "warn", None, Some("Breezone"), text, None).await;
            }
        }
    }
    *state.surplus.snapshot.write().await = Snapshot { meter: export.is_some(), export, statuses };
}

pub async fn run_loop(state: AppState) {
    let mut ticker = tokio::time::interval(EVERY);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticker.tick().await;
        let surplus = db::list_surplus_loads(&state.db).await.is_ok_and(|l| l.iter().any(|l| l.enabled));
        let planned = db::list_charger_plans(&state.db).await.is_ok_and(|l| l.iter().any(|p| p.profile_id.is_some() || p.now_amps.is_some()));
        if !surplus && !planned {
            *state.surplus.snapshot.write().await = Snapshot::default();
            continue;
        }
        tick(&state, db::now()).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PARIS: Tz = chrono_tz::Europe::Paris;

    fn at(h: u32, m: u32) -> i64 {
        PARIS.with_ymd_and_hms(2026, 6, 15, h, m, 0).unwrap().timestamp()
    }

    fn charger(guarantee: Option<(f64, &str)>) -> Load {
        Load {
            device_id: 1,
            kind: Kind::Charger,
            name: "Borne".into(),
            min_amps: 6,
            max_amps: 16,
            power: 0.0,
            guarantee: guarantee.map(|(k, t)| (k, NaiveTime::parse_from_str(t, "%H:%M").unwrap())),
            min_on: 0,
            tolerance: 0.0,
            climate: ClimatePlan::default(),
            plan: None,
            profile: None,
        }
    }

    fn heater() -> Load {
        Load {
            device_id: 2,
            kind: Kind::Relay,
            name: "Chauffe-eau".into(),
            min_amps: 0,
            max_amps: 0,
            power: 2000.0,
            guarantee: None,
            min_on: 600,
            tolerance: 0.0,
            climate: ClimatePlan::default(),
            plan: None,
            profile: None,
        }
    }

    /// A zone of the air conditioning reckoned at 1.5 kW.
    fn zone() -> Load {
        Load { device_id: 3, kind: Kind::Climate, name: "Salon".into(), power: 1500.0, ..heater() }
    }

    /// The zone: on or off, its system heating, its setpoint, the outdoor temperature.
    fn salon(on: bool, setpoint: f64, outdoor: f64) -> Reading {
        Reading { online: true, plugged: true, running: on, heats: true, setpoint: Some(setpoint), outdoor: Some(outdoor), ..Reading::default() }
    }

    fn car(running: bool, amps: i64, energy: f64) -> Reading {
        Reading {
            online: true,
            plugged: true,
            running,
            power: if running { amps as f64 * 230.0 } else { 0.0 },
            voltage: 230.0,
            phases: 1,
            session_energy: energy,
            amps: Some(amps),
            ..Reading::default()
        }
    }

    fn relay(on: bool) -> Reading {
        Reading { online: true, plugged: true, running: on, power: if on { 2000.0 } else { 0.0 }, ..Reading::default() }
    }

    /// What happened on the night of 2026-11-22: the surplus paused the charge, the charge was
    /// resumed from the charger's own app, and the surplus paused it again a few minutes later,
    /// over and over. Resumed by someone else, it is left alone until the car is unplugged.
    #[test]
    fn a_charge_resumed_outside_breezone_is_left_alone() {
        let mut r = Regulator::default();
        let paused = Reading { paused: true, ..car(false, 0, 0.0) };
        // Paused (by the surplus, the day before): seen so.
        let (c, _) = r.step(at(22, 0), PARIS, -500.0, &[(charger(None), paused)]);
        assert!(c.is_empty());
        // Resumed from the charger's app at night: no stop, ever.
        for minutes in [1, 4, 10, 30] {
            let (c, s) = r.step(at(22, minutes), PARIS, -4000.0, &[(charger(None), car(true, 16, 1.0))]);
            assert!(c.is_empty(), "left alone: {c:?}");
            assert_eq!(s[&1].state, "manual");
        }
        // Unplugged: the surplus takes it back.
        r.step(at(23, 0), PARIS, -500.0, &[(charger(None), Reading { plugged: false, ..car(false, 0, 0.0) })]);
        let (_, s) = r.step(at(23, 1), PARIS, -500.0, &[(charger(None), paused)]);
        assert_ne!(s[&1].state, "manual");
    }

    /// A charge that starts by itself when the car is plugged in (the charger was not paused):
    /// the surplus pauses it at night, as before, to wait for the sun.
    #[test]
    fn a_charge_that_starts_when_plugged_in_waits_for_the_sun() {
        let mut r = Regulator::default();
        let charging = [(charger(None), car(true, 16, 0.0))];
        r.step(at(22, 0), PARIS, -3700.0, &charging);
        assert_eq!(r.step(at(22, 4), PARIS, -3700.0, &charging).0, vec![(1, Command::Stop)]);
    }

    /// Paused from its app while the surplus charged it: not started again on the next sun.
    #[test]
    fn a_solar_charge_paused_by_hand_is_not_started_again() {
        let mut r = Regulator::default();
        let idle = [(charger(None), car(false, 0, 0.0))];
        r.step(at(12, 0), PARIS, 3000.0, &idle);
        assert!(matches!(r.step(at(12, 1), PARIS, 3000.0, &idle).0[..], [(1, Command::Start(_))]));
        r.step(at(12, 2), PARIS, 1000.0, &[(charger(None), car(true, 13, 0.1))]);
        // Paused from its app, past the time the charger takes to show what it was asked.
        let paused = [(charger(None), Reading { paused: true, ..car(false, 0, 0.2) })];
        let (c, s) = r.step(at(12, 10), PARIS, 3000.0, &paused);
        assert!(c.is_empty());
        assert_eq!(s[&1].state, "manual");
        assert!(r.step(at(12, 20), PARIS, 5000.0, &paused).0.is_empty(), "not started again");
    }

    /// Stopped by hand (the button of the page Recharge): the surplus does not start it again, nor
    /// the guarantee, until its car is unplugged; then it steers it again.
    #[test]
    fn a_charger_stopped_by_hand_is_left_alone_until_unplugged() {
        let mut r = Regulator::default();
        let paused = [(charger(Some((20.0, "07:00"))), car(false, 0, 0.0))];
        r.by_hand(1);
        for minutes in [0, 1, 5] {
            let (c, s) = r.step(at(23, minutes), PARIS, 5000.0, &paused);
            assert!(c.is_empty(), "never started again: {c:?}");
            assert_eq!(s[&1].state, "manual");
        }
        let unplugged = Reading { plugged: false, ..car(false, 0, 0.0) };
        r.step(at(23, 10), PARIS, 5000.0, &[(charger(None), unplugged)]);
        let plugged = [(charger(None), car(false, 0, 0.0))];
        r.step(at(23, 11), PARIS, 5000.0, &plugged);
        assert_eq!(r.step(at(23, 12), PARIS, 5000.0, &plugged).0, vec![(1, Command::Start(16))], "plugged in again: steered again");
    }

    fn planned(plan: Plan) -> Load {
        Load { plan: Some(plan), profile: Some("Nuit bleue".into()), ..charger(None) }
    }

    /// The blue night: nothing on the peak hours (a charge that starts when the car is plugged in
    /// is stopped), 7 kW (30 A) on the off-peak hours whatever the sun, from the grid.
    #[test]
    fn a_profile_stops_then_charges_whatever_the_sun() {
        let mut r = Regulator::default();
        let off = planned(Plan::Off { until: Some(at(22, 0)) });
        let (c, s) = r.step(at(18, 0), PARIS, 3000.0, &[(off.clone(), car(true, 16, 0.0))]);
        assert_eq!(c, vec![(1, Command::Stop)], "no charge now, even with sun");
        assert_eq!((s[&1].state, s[&1].text.fr.as_str()), ("no_charge", "Profil Nuit bleue : pas de charge jusqu'à 22 h"));
        let paused = Reading { paused: true, ..car(false, 0, 0.0) };
        assert!(r.step(at(18, 1), PARIS, 3000.0, &[(off, paused)]).0.is_empty());
        // 22:00: the off-peak hours, at night.
        let max = planned(Plan::Max { amps: 30, until: Some(at(6, 0) + 86400), now: false, left_kwh: None });
        let (c, s) = r.step(at(22, 0), PARIS, -500.0, &[(max.clone(), paused)]);
        assert_eq!(c, vec![(1, Command::Start(30))]);
        assert_eq!((s[&1].state, s[&1].text.fr.as_str()), ("charging", "Profil Nuit bleue : charge à 6,9 kW jusqu'à demain 6 h"));
        // Charging: nothing more, nothing stops it at night.
        for minutes in [1, 5, 30] {
            assert!(r.step(at(22, minutes), PARIS, -7400.0, &[(max.clone(), car(true, 30, 1.0))]).0.is_empty());
        }
        // The current shown differs (set from its app): put back, at most every 20 s.
        assert_eq!(r.step(at(22, 40), PARIS, -4000.0, &[(max.clone(), car(true, 16, 2.0))]).0, vec![(1, Command::Amps(30))]);
        // Full: not started again.
        let full = car(false, 0, 30.0);
        assert!(r.step(at(23, 30), PARIS, -500.0, &[(max.clone(), full)]).0.is_empty());
        assert!(r.step(at(23, 40), PARIS, -500.0, &[(max, full)]).0.is_empty());
    }

    /// A charger that sets its own current (load shedding by its clamp) keeps it: started without
    /// a current, never set.
    #[test]
    fn a_profile_leaves_its_load_shedding_to_the_charger() {
        let mut r = Regulator::default();
        let max = planned(Plan::Max { amps: 30, until: None, now: false, left_kwh: None });
        let own = |running: bool, amps: i64| Reading { auto_limit: true, paused: !running, ..car(running, amps, 0.0) };
        let (c, s) = r.step(at(22, 0), PARIS, -500.0, &[(max.clone(), own(false, 0))]);
        assert_eq!(c, vec![(1, Command::Resume)]);
        assert_eq!(s[&1].text.fr, "Profil Nuit bleue : charge au courant réglé par la borne");
        assert!(r.step(at(22, 5), PARIS, -4000.0, &[(max, own(true, 12))]).0.is_empty(), "its current is its own");
    }

    /// Paused from its app while its profile charges: left alone until unplugged.
    #[test]
    fn a_pause_by_hand_during_a_profile_holds() {
        let mut r = Regulator::default();
        let max = planned(Plan::Max { amps: 30, until: None, now: false, left_kwh: None });
        r.step(at(23, 0), PARIS, -500.0, &[(max.clone(), car(true, 30, 1.0))]);
        let paused = Reading { paused: true, ..car(false, 0, 1.2) };
        let (c, s) = r.step(at(23, 5), PARIS, -500.0, &[(max.clone(), paused)]);
        assert!(c.is_empty());
        assert_eq!(s[&1].state, "manual");
        assert!(r.step(at(23, 30), PARIS, -500.0, &[(max.clone(), paused)]).0.is_empty(), "not started again");
        // Started from its app during a slot without charge: left alone too.
        let mut r = Regulator::default();
        let off = planned(Plan::Off { until: None });
        r.step(at(12, 0), PARIS, 0.0, &[(off.clone(), Reading { paused: true, ..car(false, 0, 0.0) })]);
        let (c, s) = r.step(at(12, 5), PARIS, 0.0, &[(off, car(true, 16, 0.1))]);
        assert!(c.is_empty(), "{c:?}");
        assert_eq!(s[&1].state, "manual");
    }

    /// Without a grid meter: a profile still stops and charges, only the surplus waits for one.
    #[test]
    fn a_profile_acts_without_a_grid_meter() {
        let mut r = Regulator::default();
        let off = planned(Plan::Off { until: None });
        assert_eq!(r.step_with(at(18, 0), PARIS, None, &[(off, car(true, 16, 0.0))]).0, vec![(1, Command::Stop)]);
        let now = Load { profile: None, ..planned(Plan::Max { amps: 16, until: None, now: true, left_kwh: Some(4.25) }) };
        let paused = Reading { paused: true, ..car(false, 0, 0.0) };
        let (c, s) = r.step_with(at(19, 0), PARIS, None, &[(now, paused), (heater(), relay(false))]);
        assert_eq!(c, vec![(1, Command::Start(16))]);
        assert_eq!((s[&1].state, s[&1].text.fr.as_str()), ("now", "Charge maintenant à 3,7 kW, encore 4,3 kWh"));
        assert_eq!(s[&1].text.en, "Charging now at 3.7 kW, 4.3 kWh to go");
        assert_eq!(s[&2].state, "no_meter");
        let solar = planned(Plan::Solar { min_amps: 6, max_amps: 16 });
        assert_eq!(r.step_with(at(19, 1), PARIS, None, &[(solar, car(false, 0, 0.0))]).1[&1].state, "no_meter");
    }

    /// The surplus slot of a profile: the surplus within its own limits.
    #[test]
    fn a_solar_slot_has_its_own_limits() {
        let mut r = Regulator::default();
        let solar = planned(Plan::Solar { min_amps: 6, max_amps: 10 });
        let idle = [(solar, car(false, 0, 0.0))];
        r.step(at(12, 0), PARIS, 5000.0, &idle);
        assert_eq!(r.step(at(12, 1), PARIS, 5000.0, &idle).0, vec![(1, Command::Start(10))]);
    }

    /// Charging whatever the sun is taken off the surplus first.
    #[test]
    fn a_charge_whatever_the_sun_leaves_the_surplus_to_others() {
        let mut r = Regulator::default();
        let max = planned(Plan::Max { amps: 16, until: None, now: true, left_kwh: None });
        // 3 kW go back with the charger at 16 A: the heater (2 kW) fits, the charger is not counted twice.
        let loads = [(max, car(true, 16, 0.0)), (heater(), relay(false))];
        r.step(at(12, 0), PARIS, 3000.0, &loads);
        assert_eq!(r.step(at(12, 1), PARIS, 3000.0, &loads).0, vec![(2, Command::On)]);
    }

    #[test]
    fn a_charger_follows_the_surplus() {
        let mut r = Regulator::default();
        let t0 = at(12, 0);
        // 2 kW going back to the grid: 8 A fit, after a minute.
        assert!(r.step(t0, PARIS, 2000.0, &[(charger(None), car(false, 0, 0.0))]).0.is_empty(), "a minute first");
        let (c, s) = r.step(t0 + 60, PARIS, 2000.0, &[(charger(None), car(false, 0, 0.0))]);
        assert_eq!(c, vec![(1, Command::Start(8))]);
        assert_eq!(s[&1].text.fr, "Charge solaire à 8 A");
        // Charging at 8 A (1 840 W), 600 W still go back: 10 A, but not before 20 s.
        assert!(r.step(t0 + 70, PARIS, 600.0, &[(charger(None), car(true, 8, 0.0))]).0.is_empty());
        assert_eq!(r.step(t0 + 80, PARIS, 600.0, &[(charger(None), car(true, 8, 0.0))]).0, vec![(1, Command::Amps(10))]);
        // Lots of sun: capped at 16 A.
        assert_eq!(r.step(t0 + 120, PARIS, 5000.0, &[(charger(None), car(true, 10, 0.0))]).0, vec![(1, Command::Amps(16))]);
        // A cloud: drawing 1 kW from the grid at 16 A (3 680 W): only 11 A fit.
        assert_eq!(r.step(t0 + 150, PARIS, -1000.0, &[(charger(None), car(true, 16, 0.0))]).0, vec![(1, Command::Amps(11))]);
        // A big cloud: less than 6 A for 3 minutes holds at 6 A, then stops.
        let mut short = |dt| r.step(t0 + 200 + dt, PARIS, -2600.0, &[(charger(None), car(true, 11, 0.0))]);
        assert_eq!(short(0).0, vec![(1, Command::Amps(6))]);
        assert!(short(100).0.is_empty(), "holds at its minimum");
        assert_eq!(short(180).0, vec![(1, Command::Stop)]);
        // The sun is back at once: it waits 2 minutes since the stop, and a minute of surplus.
        let back = |r: &mut Regulator, dt| r.step(t0 + 380 + dt, PARIS, 3000.0, &[(charger(None), car(false, 6, 0.0))]).0;
        assert!(back(&mut r, 0).is_empty());
        assert!(back(&mut r, 60).is_empty(), "two minutes since the stop");
        assert_eq!(back(&mut r, 130), vec![(1, Command::Start(13))]);
    }

    #[test]
    fn priorities_share_the_surplus() {
        let mut r = Regulator::default();
        let t0 = at(13, 0);
        let loads = |c: Reading, h: Reading| vec![(charger(None), c), (heater(), h)];
        // 5 kW: the charger first (16 A, 3 680 W), not enough left for the 2 kW heater.
        r.step(t0, PARIS, 5000.0, &loads(car(false, 0, 0.0), relay(false)));
        let (c, s) = r.step(t0 + 60, PARIS, 5000.0, &loads(car(false, 0, 0.0), relay(false)));
        assert_eq!(c, vec![(1, Command::Start(16))]);
        assert_eq!(s[&2].state, "waiting");
        assert_eq!(s[&2].text.fr, "Attend 2 kW de surplus");
        // 6 kW of surplus with the charger at 16 A: 2 320 W left, the heater starts after a minute.
        r.step(t0 + 70, PARIS, 2320.0, &loads(car(true, 16, 0.0), relay(false)));
        assert_eq!(r.step(t0 + 130, PARIS, 2320.0, &loads(car(true, 16, 0.0), relay(false))).0, vec![(2, Command::On)]);
        // Less sun: the charger keeps its priority, the heater goes (once on for 10 minutes).
        let less = |r: &mut Regulator, dt| r.step(t0 + 140 + dt, PARIS, -1000.0, &loads(car(true, 16, 0.0), relay(true))).0;
        assert_eq!(less(&mut r, 0), vec![], "the charger still fits; the heater is short");
        assert_eq!(less(&mut r, 180), vec![], "short for 3 minutes, but on for less than 10");
        assert_eq!(less(&mut r, 600), vec![(2, Command::Off)]);
    }

    #[test]
    fn the_guarantee_charges_from_the_grid_in_time() {
        let mut r = Regulator::default();
        // +20 kWh before 07:00 at 16 A single phase (3,68 kW): 5 h 26, plus 15 min of margin.
        let load = charger(Some((20.0, "07:00")));
        let night = |h, m| PARIS.with_ymd_and_hms(2026, 6, 15, h, m, 0).unwrap().timestamp();
        let (c, s) = r.step(night(1, 0), PARIS, -300.0, &[(load.clone(), car(false, 0, 0.0))]);
        assert!(c.is_empty(), "there is still time: {:?}", s[&1]);
        let (c, s) = r.step(night(1, 20), PARIS, -300.0, &[(load.clone(), car(false, 0, 0.0))]);
        assert_eq!(c, vec![(1, Command::Start(16))]);
        assert_eq!(s[&1].text.fr, "Garantie : 16 A depuis le réseau, encore 20 kWh");
        // Reached: back to the surplus (none at night: it stops after 3 minutes).
        let done = car(true, 16, 20.0);
        assert_eq!(r.step(night(6, 50), PARIS, -3680.0, &[(load.clone(), done)]).0, vec![(1, Command::Amps(6))]);
        assert_eq!(r.step(night(6, 54), PARIS, -3680.0, &[(load, done)]).0, vec![(1, Command::Stop)]);
    }

    #[test]
    fn nothing_without_a_car_or_an_answer() {
        let mut r = Regulator::default();
        let unplugged = Reading { plugged: false, ..car(false, 0, 0.0) };
        let (c, s) = r.step(at(12, 0), PARIS, 5000.0, &[(charger(None), unplugged)]);
        assert!(c.is_empty());
        assert_eq!(s[&1].state, "no_car");
        let lost = Reading { online: false, ..car(true, 10, 0.0) };
        assert_eq!(r.step(at(12, 1), PARIS, 5000.0, &[(charger(None), lost)]).1[&1].state, "unavailable");
        // Three-phase: 1 A is 690 W.
        let three = Reading { phases: 3, ..car(false, 0, 0.0) };
        r.step(at(12, 2), PARIS, 5000.0, &[(charger(None), three)]);
        assert_eq!(r.step(at(12, 3), PARIS, 5000.0, &[(charger(None), three)]).0, vec![(1, Command::Start(7))]);
    }

    #[test]
    fn a_zone_heats_on_the_surplus_then_is_put_back() {
        let mut r = Regulator::default();
        let plan = |o: f64| [(zone(), salon(false, 18.0, o))];
        // Mild outside: no need, whatever the surplus.
        let (c, s) = r.step(at(12, 0), PARIS, 3000.0, &plan(16.0));
        assert!(c.is_empty());
        assert_eq!(s[&3].state, "idle");
        // Cold outside, the system heats, 2 kW go back to the grid: on at 20° after a minute.
        assert!(r.step(at(12, 1), PARIS, 2000.0, &plan(9.0)).0.is_empty());
        assert_eq!(r.step(at(12, 2), PARIS, 2000.0, &plan(9.0)).0, vec![(3, Command::ClimateOn(20.0))]);
        // It runs: its 1.5 kW are part of the surplus; still 500 W going back, it stays.
        let on = [(zone(), salon(true, 20.0, 9.0))];
        let (c, s) = r.step(at(12, 3), PARIS, 500.0, &on);
        assert!(c.is_empty());
        assert_eq!(s[&3].state, "solar");
        // The surplus goes: put back off at 18° once its minimum time and the stop delay are over.
        assert!(r.step(at(12, 12), PARIS, -800.0, &on).0.is_empty());
        assert_eq!(r.step(at(12, 16), PARIS, -800.0, &on).0, vec![(3, Command::ClimateOff(Some(18.0)))]);
    }

    #[test]
    fn a_zone_set_by_hand_is_left_alone() {
        let mut r = Regulator::default();
        // On by hand: never touched, and not part of the surplus.
        let by_hand = [(zone(), salon(true, 22.0, 9.0))];
        let (c, s) = r.step(at(12, 0), PARIS, 3000.0, &by_hand);
        assert!(c.is_empty());
        assert_eq!(s[&3].state, "manual");
        // Started by Breezone, then set to 23° by hand: Breezone lets it go, nothing put back.
        let off = [(zone(), salon(false, 18.0, 9.0))];
        r.step(at(13, 0), PARIS, 2000.0, &off);
        assert_eq!(r.step(at(13, 1), PARIS, 2000.0, &off).0, vec![(3, Command::ClimateOn(20.0))]);
        let moved = [(zone(), salon(true, 23.0, 9.0))];
        let (c, s) = r.step(at(13, 2), PARIS, -2000.0, &moved);
        assert!(c.is_empty());
        assert_eq!(s[&3].state, "manual");
        assert!(r.step(at(13, 20), PARIS, -2000.0, &moved).0.is_empty(), "no longer its own");
    }
}
