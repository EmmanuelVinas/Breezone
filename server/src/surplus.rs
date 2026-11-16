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

use crate::db::{self, SurplusLoadRow};
use crate::devices::{self, DeviceView};
use crate::i18n::Text;
use crate::AppState;
use chrono::{NaiveTime, TimeZone};
use chrono_tz::Tz;
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
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
    /// What it draws now (W).
    pub power: f64,
    pub voltage: f64,
    pub phases: u32,
    /// A charger: energy of the session (kWh).
    pub session_energy: f64,
    /// A charger: current asked (A).
    pub amps: Option<i64>,
}

/// What Breezone tells a load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// A charger: charge at this current.
    Start(i64),
    /// A charger: change the current.
    Amps(i64),
    Stop,
    On,
    Off,
}

/// What a load does, for the pages.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Status {
    /// "solar" (runs on the surplus) | "guarantee" (runs from the grid to keep the guarantee) |
    /// "waiting" (not enough surplus) | "no_car" | "unavailable" | "no_meter" | "off" (not steered)
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
}

/// Decides, look after look, what each load does.
#[derive(Debug, Default)]
pub struct Regulator {
    memory: HashMap<i64, Memory>,
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

impl Regulator {
    /// One look: `export` is what goes back to the grid (W, negative: drawn from it), `loads` in
    /// order of priority. Returns the commands to send and the status of every load.
    pub fn step(&mut self, now: i64, tz: Tz, export: f64, loads: &[(Load, Reading)]) -> (Vec<(i64, Command)>, HashMap<i64, Status>) {
        self.memory.retain(|id, _| loads.iter().any(|(l, _)| l.device_id == *id));
        // What the loads Breezone runs use is part of the surplus.
        // A relay that measures nothing counts for its power.
        let used = |l: &Load, r: &Reading| if l.kind == Kind::Relay && r.power <= 0.0 { l.power } else { r.power.max(0.0) };
        let mut budget = export + loads.iter().filter(|(_, r)| r.online && r.running).map(|(l, r)| used(l, r)).sum::<f64>();
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
            let rested = m.changed_at.is_none_or(|t| now - t >= MIN_OFF);
            match load.kind {
                Kind::Charger => {
                    if !r.plugged {
                        *m = Memory::default();
                        statuses.insert(load.device_id, status("no_car", None, crate::text!("Pas de voiture branchée", "No car plugged in", "Ningún coche conectado")));
                        continue;
                    }
                    let per_amp = (if r.voltage > 0.0 { r.voltage } else { VOLTAGE }) * r.phases.max(1) as f64;
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
                                *m = Memory { changed_at: Some(now), on_since: Some(now), amps: Some(fits), ..Memory::default() };
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
}

/// The kind of a device that can be steered on the surplus: a charger, or a relay of this server.
pub fn kind_of(d: &DeviceView) -> Option<Kind> {
    if d.installation_id.is_some() || d.source_id.is_none() {
        return None;
    }
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
    }
}

fn reading_of(d: &DeviceView, kind: Kind) -> Reading {
    let s = &d.state;
    let flag = |f: &str| s.get(f).and_then(Value::as_bool);
    match kind {
        Kind::Charger => {
            let status = s.get("status").and_then(Value::as_str);
            Reading {
                online: d.available,
                plugged: matches!(status, Some("plugged" | "charging")),
                running: status == Some("charging") && !flag("paused").unwrap_or(false),
                power: num(s, "power").unwrap_or(0.0),
                voltage: num(s, "voltage").unwrap_or(0.0),
                phases: num(s, "phases").unwrap_or(1.0) as u32,
                session_energy: num(s, "session_energy").unwrap_or(0.0),
                amps: num(s, "limit").map(|a| a as i64),
            }
        }
        Kind::Relay => Reading { online: d.available, plugged: true, running: flag("on").unwrap_or(false), power: num(s, "power").unwrap_or(0.0), ..Reading::default() },
    }
}

/// Sends a command to a device.
async fn send(state: &AppState, d: &DeviceView, c: Command) -> Result<(), String> {
    let source = d.source_id.ok_or("no source")?;
    let reference = d.reference.as_str().unwrap_or_default();
    let params = |v: Value| v.as_object().cloned().unwrap_or_else(Map::new);
    let (trait_name, action, p) = match c {
        Command::Start(a) => ("ev.charger", "start", params(json!({ "limit": a }))),
        Command::Amps(a) => ("ev.charger", "set_limit", params(json!({ "limit": a }))),
        Command::Stop => ("ev.charger", "stop", Map::new()),
        Command::On => ("switch.relay", "set", params(json!({ "on": true }))),
        Command::Off => ("switch.relay", "set", params(json!({ "on": false }))),
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
    let steered: Vec<(&SurplusLoadRow, &DeviceView, Kind)> =
        rows.iter().filter(|r| r.enabled).filter_map(|r| devices.iter().find(|d| d.id == r.device_id).and_then(|d| kind_of(d).map(|k| (r, d, k)))).collect();
    let Some(grid) = grid else {
        let statuses = steered
            .iter()
            .map(|(_, d, _)| {
                let text =
                    crate::text!("Pas de compteur réseau : le surplus est inconnu", "No grid meter: the surplus is unknown", "Sin contador de red: el excedente es desconocido");
                (d.id, Status { state: "no_meter", amps: None, power: num(&d.state, "power").unwrap_or(0.0), text })
            })
            .collect();
        *state.surplus.snapshot.write().await = Snapshot { meter: false, export: None, statuses };
        return;
    };
    let loads: Vec<(Load, Reading)> = steered.iter().map(|(r, d, k)| (load_of(r, d, *k), reading_of(d, *k))).collect();
    let (commands, statuses) = state.surplus.regulator.lock().await.step(now, state.cfg.timezone, -grid, &loads);
    for (id, c) in commands {
        let Some(d) = devices.iter().find(|d| d.id == id) else { continue };
        let name = d.name.clone();
        let message = match c {
            Command::Start(a) if statuses.get(&id).is_some_and(|s| s.state == "guarantee") => {
                crate::text!("{0} : charge à {1} A pour tenir la garantie", "{0}: charging at {1} A to keep the guarantee", "{0}: carga a {1} A para cumplir la garantía", name, a)
            }
            Command::Start(a) => crate::text!("{0} : charge solaire à {1} A", "{0}: solar charging at {1} A", "{0}: carga solar a {1} A", name, a),
            Command::Stop => {
                crate::text!("{0} : charge suspendue, plus assez de surplus", "{0}: charging paused, not enough surplus", "{0}: carga suspendida, excedente insuficiente", name)
            }
            Command::On => crate::text!("{0} : en marche sur le surplus solaire", "{0}: on, on the solar surplus", "{0}: en marcha con el excedente solar", name),
            Command::Off => crate::text!("{0} : arrêté, plus assez de surplus", "{0}: off, not enough surplus", "{0}: apagado, excedente insuficiente", name),
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
    *state.surplus.snapshot.write().await = Snapshot { meter: true, export: Some(-grid), statuses };
}

pub async fn run_loop(state: AppState) {
    let mut ticker = tokio::time::interval(EVERY);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticker.tick().await;
        if !db::list_surplus_loads(&state.db).await.is_ok_and(|l| l.iter().any(|l| l.enabled)) {
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
        }
    }

    fn heater() -> Load {
        Load { device_id: 2, kind: Kind::Relay, name: "Chauffe-eau".into(), min_amps: 0, max_amps: 0, power: 2000.0, guarantee: None, min_on: 600, tolerance: 0.0 }
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
        }
    }

    fn relay(on: bool) -> Reading {
        Reading { online: true, plugged: true, running: on, power: if on { 2000.0 } else { 0.0 }, ..Reading::default() }
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
}
