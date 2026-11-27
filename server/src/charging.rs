//! Charging profiles of the chargers (roadmap E39).
//!
//! A **profile** is a power: charge up to some kW, or up to what the charger can. **When** a charger
//! charges is up to the automations: the action "charging of the charger" tells it to follow a
//! profile, not to charge, to charge on the solar surplus (following it, or at a fixed power once it
//! is high enough), or to let the charger decide by its own app ([`Mode`]). A charger whose own
//! schedule is on (`timer` of `ev.charger`) is left to it: the automations do not drive it.
//!
//! A charger may also be told to **charge now** (at some kW, until it is unplugged, until a time or
//! until some kWh more), which comes first. The surplus regulator (`surplus`) does what they say:
//! this module only tells it what the plan is at each moment.

use crate::db::{self, ChargeProfileRow, ChargerPlanRow};
use crate::devices::DeviceView;
use crate::surplus::Plan;
use crate::AppState;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

pub const MAX_PROFILES: usize = 20;
/// More than any charger of a home (kW).
pub const MAX_KW: f64 = 50.0;
/// Voltage when the charger does not tell it.
const VOLTAGE: f64 = 230.0;

/// A profile as it is written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileInput {
    pub name: String,
    /// Up to this power (none: the charger's maximum).
    #[serde(default)]
    pub kw: Option<f64>,
    /// A colour for the screens ("#14b8a6").
    #[serde(default)]
    pub color: Option<String>,
}

fn kw_ok(kw: Option<f64>) -> bool {
    kw.is_none_or(|kw| kw.is_finite() && kw > 0.0 && kw <= MAX_KW)
}

pub fn validate(p: &ProfileInput) -> Result<(), String> {
    if p.name.trim().is_empty() || p.name.chars().count() > 60 {
        return Err("name: 1 to 60 characters".into());
    }
    if !kw_ok(p.kw) {
        return Err(format!("kw: a power between 0 and {MAX_KW} kW"));
    }
    if p.color.as_deref().is_some_and(|c| c.len() > 20) {
        return Err("color: 20 characters at most".into());
    }
    Ok(())
}

/// What a charger is told to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Charge whenever the car asks, up to the power of a profile (none: the charger's maximum).
    Profile,
    /// No charge.
    Off,
    /// The solar surplus only (see [`Solar`]).
    Solar,
    /// Breezone leaves it to the charger and its app (its own schedule is switched on).
    Free,
}

impl Mode {
    /// The mode of an action: written, or the form of before (a profile, or none).
    pub fn of_step(mode: Option<Mode>, profile_id: Option<i64>) -> Mode {
        mode.unwrap_or(if profile_id.is_some() { Mode::Profile } else { Mode::Free })
    }

    fn stored(self) -> Option<&'static str> {
        match self {
            Mode::Profile => Some("profile"),
            Mode::Off => Some("off"),
            Mode::Solar => Some("solar"),
            Mode::Free => None,
        }
    }

    /// What a charger follows by its stored row.
    pub fn of_row(row: &ChargerPlanRow) -> Mode {
        match row.mode.as_deref() {
            Some("profile") => Mode::Profile,
            Some("off") => Mode::Off,
            Some("solar") => Mode::Solar,
            _ => Mode::Free,
        }
    }
}

/// How a charger charges on the solar surplus.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Solar {
    /// `fixed`: charge at this power once the surplus is over `start_kw` (the rest from the grid);
    /// else follow the surplus up to it (none: the charger's maximum).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kw: Option<f64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fixed: bool,
    /// Start when the surplus is over it, stop below it (kW; none: the charger's minimum current).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_kw: Option<f64>,
}

impl Solar {
    pub fn validate(&self) -> Result<(), String> {
        if !kw_ok(self.kw) || !kw_ok(self.start_kw) {
            return Err(format!("solar: powers between 0 and {MAX_KW} kW"));
        }
        if self.fixed && self.kw.is_none() {
            return Err("solar: a fixed power needs its kW".into());
        }
        Ok(())
    }

    pub fn of(row: &ChargerPlanRow) -> Solar {
        row.solar.as_deref().and_then(|s| serde_json::from_str(s).ok()).unwrap_or_default()
    }

    /// The plan on a charger: the currents it keeps between, and the surplus that starts it.
    pub fn plan(&self, c: &Charger) -> Plan {
        let (lo, hi) = (c.limit_min, c.limit_max.max(c.limit_min));
        let max_amps = self.kw.map_or(hi, |kw| amps_of(kw, c.voltage, c.phases, lo, hi));
        let min_amps = if self.fixed { max_amps } else { lo };
        Plan::Solar { min_amps, max_amps, start_w: self.start_kw.map(|k| k * 1000.0) }
    }
}

/// What a charger is told, as an action gives it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Choice {
    pub mode: Mode,
    pub profile_id: Option<i64>,
    pub solar: Option<Solar>,
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
    /// Its own schedule (the app of the charger) decides when it charges.
    pub own_schedule: bool,
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
            own_schedule: d.state.get("timer").and_then(Value::as_bool).unwrap_or(false),
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

/// What a charger does now: the charge asked now, else what it was told (`None`: it is left to
/// itself, or to its own schedule).
pub fn plan_now(row: &ChargerPlanRow, profile: Option<&ChargeProfileRow>, c: &Charger, now: i64) -> Option<Plan> {
    if let Some(amps) = row.now_amps.filter(|_| now_over(row, c, now).is_none()) {
        let left = match (row.now_kwh, row.now_base_energy) {
            (Some(kwh), Some(base)) => Some((kwh - (c.session_energy - base)).max(0.0)),
            _ => None,
        };
        return Some(Plan::Max { amps: amps.clamp(c.limit_min, c.limit_max.max(c.limit_min)), until: row.now_until, now: true, left_kwh: left });
    }
    if c.own_schedule {
        return None;
    }
    match Mode::of_row(row) {
        // Without a profile: the charger's maximum; a profile deleted since: nothing.
        Mode::Profile => match (row.profile_id, profile) {
            (None, _) => Some(Plan::Max { amps: c.amps(None), until: None, now: false, left_kwh: None }),
            (Some(_), p) => p.map(|p| Plan::Max { amps: c.amps(p.kw), until: None, now: false, left_kwh: None }),
        },
        Mode::Off => Some(Plan::Off { until: None }),
        Mode::Solar => Some(Solar::of(row).plan(c)),
        Mode::Free => None,
    }
}

/// The plan of every charger that follows one now, with who set it (the automation, else the
/// profile) for the texts. A charge asked now that is over is cleared (it is said in the activity
/// feed).
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
    let profiles: HashMap<i64, ChargeProfileRow> = db::list_charge_profiles(&state.db).await.unwrap_or_default().into_iter().map(|p| (p.id, p)).collect();
    let rules: HashMap<i64, String> = db::list_automations(&state.db).await.unwrap_or_default().into_iter().map(|r| (r.id, r.name)).collect();
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
        if let Some(plan) = plan_now(&row, profile, &c, now) {
            let by = if row.now_amps.is_some() { None } else { row.automation_id.and_then(|id| rules.get(&id).cloned()).or_else(|| profile.map(|p| p.name.clone())) };
            out.insert(row.device_id, (plan, by));
        }
    }
    out
}

/// Tells a charger what to do (`automation`: the one that does, `None`: by hand). It takes it at
/// once, even after a stop by hand. `Free` switches its own schedule on.
pub async fn set(state: &AppState, device_id: i64, choice: Choice, automation: Option<i64>) -> crate::error::ApiResult<()> {
    use crate::error::ApiError;
    if choice.mode == Mode::Profile {
        // No profile: up to the charger's maximum.
        let known = match choice.profile_id {
            Some(pid) => db::list_charge_profiles(&state.db).await?.iter().any(|r| r.id == pid),
            None => true,
        };
        if !known {
            return Err(ApiError::not_found(crate::text!("profil de recharge introuvable", "charging profile not found", "perfil de carga no encontrado")));
        }
    }
    if let Some(s) = &choice.solar {
        s.validate().map_err(ApiError::bad_request)?;
    }
    let row = db::charger_plan(&state.db, device_id).await?.unwrap_or(ChargerPlanRow { device_id, ..Default::default() });
    let solar = (choice.mode == Mode::Solar).then(|| serde_json::to_string(&choice.solar.unwrap_or_default()).unwrap_or_default());
    let profile_id = choice.profile_id.filter(|_| choice.mode == Mode::Profile);
    db::set_charger_plan(&state.db, &ChargerPlanRow { mode: choice.mode.stored().map(Into::into), profile_id, automation_id: automation, solar, ..row }).await?;
    if choice.mode == Mode::Free {
        own_schedule(state, device_id, true).await?;
    }
    state.surplus.release(device_id).await;
    Ok(())
}

/// Switches the schedule of the charger's own app on or off, when it has one.
pub async fn own_schedule(state: &AppState, device_id: i64, on: bool) -> crate::error::ApiResult<()> {
    let Some(d) = crate::devices::get(state, device_id).await? else { return Ok(()) };
    if d.state.get("timer").and_then(Value::as_bool).is_none() || d.state.get("timer").and_then(Value::as_bool) == Some(on) {
        return Ok(());
    }
    let input =
        crate::devices::ActionInput { trait_name: "ev.charger".into(), action: "set_timer".into(), params: serde_json::Map::from_iter([("on".to_string(), Value::Bool(on))]) };
    crate::devices::act(state, d.id, &input, "Breezone").await
}

/// The chargers still in the list of the solar surplus (from before the charging profiles) charge
/// on the surplus by their plan instead, with their currents, and leave the list. Their guarantee
/// goes (an automation does it now), and the activity feed says so. A charger told something else
/// keeps it.
pub async fn adopt_surplus_chargers(state: &AppState) -> anyhow::Result<()> {
    let chargers = db::chargers(&state.db).await?;
    for row in db::list_surplus_loads(&state.db).await? {
        let Some((_, name)) = chargers.iter().find(|(id, _)| *id == row.device_id) else { continue };
        let plan = db::charger_plan(&state.db, row.device_id).await?.unwrap_or(ChargerPlanRow { device_id: row.device_id, ..Default::default() });
        let given = row.enabled && Mode::of_row(&plan) == Mode::Free;
        if given {
            // Its currents as powers (230 V, one phase: what it said is not known here).
            let solar = Solar { kw: row.max_amps.map(|a| kw_of(a, VOLTAGE, 1)), fixed: false, start_kw: row.min_amps.map(|a| kw_of(a, VOLTAGE, 1)) };
            db::set_charger_plan(&state.db, &ChargerPlanRow { mode: Some("solar".into()), solar: serde_json::to_string(&solar).ok(), automation_id: None, ..plan }).await?;
        }
        db::delete_surplus_load(&state.db, row.device_id).await?;
        let name = name.clone();
        let text = match (&row.guarantee_kwh, &row.guarantee_by) {
            (Some(kwh), Some(by)) if given => crate::text!(
                "{0} charge désormais sur le surplus solaire par son réglage de recharge ; sa garantie (+{1} kWh avant {2}) est à refaire avec un automatisme",
                "{0} now charges on the solar surplus by its charging setting; its guarantee (+{1} kWh before {2}) is to be made again with an automation",
                "{0} carga ahora con el excedente solar según su ajuste de carga; su garantía (+{1} kWh antes de las {2}) hay que rehacerla con una automatización",
                name,
                kwh,
                by
            ),
            _ if given => crate::text!(
                "{0} charge désormais sur le surplus solaire par son réglage de recharge",
                "{0} now charges on the solar surplus by its charging setting",
                "{0} carga ahora con el excedente solar según su ajuste de carga",
                name
            ),
            _ => crate::text!(
                "{0} quitte la liste du surplus solaire : une borne le suit par un automatisme",
                "{0} leaves the list of the solar surplus: a charger follows it by an automation",
                "{0} sale de la lista del excedente solar: un cargador lo sigue con una automatización",
                name
            ),
        };
        state.hub.log("automation", "info", None, Some("Breezone"), text, None).await;
    }
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
    if !kw_ok(input.kw) {
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

    fn trydan() -> Charger {
        Charger { online: true, plugged: true, session_energy: 0.0, voltage: 230.0, phases: 1, limit_min: 6, limit_max: 32, own_schedule: false }
    }

    fn profile(kw: Option<f64>) -> ChargeProfileRow {
        ChargeProfileRow { id: 1, name: "Nuit 7 kW".into(), kw, color: None }
    }

    fn told(mode: &str) -> ChargerPlanRow {
        ChargerPlanRow { device_id: 1, mode: Some(mode.into()), profile_id: (mode == "profile").then_some(1), ..Default::default() }
    }

    #[test]
    fn a_profile_is_checked() {
        let p = |name: &str, kw| ProfileInput { name: name.into(), kw, color: None };
        assert_eq!(validate(&p("Nuit 7 kW", Some(7.0))), Ok(()));
        assert_eq!(validate(&p("Maximum", None)), Ok(()));
        assert!(validate(&p(" ", Some(7.0))).is_err());
        assert!(validate(&p("Trop", Some(70.0))).is_err(), "more than a charger of a home");
        assert!(validate(&p("Rien", Some(0.0))).is_err());
        assert!(Solar { fixed: true, ..Default::default() }.validate().is_err(), "a fixed power needs its kW");
        assert_eq!(Solar { kw: Some(2.0), fixed: true, start_kw: Some(1.0) }.validate(), Ok(()));
    }

    #[test]
    fn the_mode_of_an_action_reads_the_form_of_before() {
        assert_eq!(Mode::of_step(None, Some(3)), Mode::Profile);
        assert_eq!(Mode::of_step(None, None), Mode::Free);
        assert_eq!(Mode::of_step(Some(Mode::Solar), None), Mode::Solar);
        assert_eq!(serde_json::to_value(Mode::Off).unwrap(), "off");
    }

    #[test]
    fn what_a_charger_does_by_what_it_was_told() {
        let c = trydan();
        assert_eq!(plan_now(&told("profile"), Some(&profile(Some(7.0))), &c, 0), Some(Plan::Max { amps: 30, until: None, now: false, left_kwh: None }));
        assert_eq!(plan_now(&told("profile"), Some(&profile(None)), &c, 0), Some(Plan::Max { amps: 32, until: None, now: false, left_kwh: None }), "the charger's maximum");
        assert_eq!(plan_now(&told("profile"), None, &c, 0), None, "a profile gone");
        let max = ChargerPlanRow { profile_id: None, ..told("profile") };
        assert_eq!(plan_now(&max, None, &c, 0), Some(Plan::Max { amps: 32, until: None, now: false, left_kwh: None }), "no profile: the charger's maximum");
        assert_eq!(plan_now(&told("off"), None, &c, 0), Some(Plan::Off { until: None }));
        assert_eq!(plan_now(&ChargerPlanRow { device_id: 1, ..Default::default() }, None, &c, 0), None, "left alone");
        // Its own schedule on: the automations do not drive it.
        assert_eq!(plan_now(&told("off"), None, &Charger { own_schedule: true, ..c }, 0), None);
    }

    #[test]
    fn the_solar_surplus_followed_or_at_a_fixed_power() {
        let c = trydan();
        // Followed, up to the charger's maximum, from its minimum.
        assert_eq!(Solar::default().plan(&c), Plan::Solar { min_amps: 6, max_amps: 32, start_w: None });
        // Followed up to 3 kW.
        assert_eq!(Solar { kw: Some(3.0), ..Default::default() }.plan(&c), Plan::Solar { min_amps: 6, max_amps: 13, start_w: None });
        // 2 kW once the surplus is over 1 kW.
        assert_eq!(Solar { kw: Some(2.0), fixed: true, start_kw: Some(1.0) }.plan(&c), Plan::Solar { min_amps: 8, max_amps: 8, start_w: Some(1000.0) });
        let row = ChargerPlanRow { solar: Some(r#"{"kw":2,"fixed":true,"start_kw":1}"#.into()), ..told("solar") };
        assert_eq!(plan_now(&row, None, &c, 0), Some(Plan::Solar { min_amps: 8, max_amps: 8, start_w: Some(1000.0) }));
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
        let now = 1_000_000;
        let asked = ChargerPlanRow { device_id: 1, now_amps: Some(30), now_kw: Some(7.0), now_base_energy: Some(2.0), ..Default::default() };
        let c = Charger { session_energy: 5.0, ..trydan() };
        assert_eq!(plan_now(&asked, None, &c, now), Some(Plan::Max { amps: 30, until: None, now: true, left_kwh: None }));
        assert_eq!(
            plan_now(&asked, None, &Charger { own_schedule: true, ..c }, now),
            Some(Plan::Max { amps: 30, until: None, now: true, left_kwh: None }),
            "asked now, whatever its own schedule"
        );
        assert_eq!(now_over(&asked, &Charger { plugged: false, ..c }, now), Some(NowOver::Unplugged));
        assert_eq!(now_over(&asked, &Charger { online: false, plugged: false, ..c }, now), None, "unreachable is not unplugged");
        let until = ChargerPlanRow { now_until: Some(now + 60), ..asked.clone() };
        assert_eq!(now_over(&until, &c, now), None);
        assert_eq!(now_over(&until, &c, now + 60), Some(NowOver::Time));
        // +5 kWh from 2 kWh: 3 done, 2 to go; done at 7.
        let kwh = ChargerPlanRow { now_kwh: Some(5.0), ..asked.clone() };
        assert_eq!(plan_now(&kwh, None, &c, now), Some(Plan::Max { amps: 30, until: None, now: true, left_kwh: Some(2.0) }));
        assert_eq!(now_over(&kwh, &Charger { session_energy: 7.0, ..c }, now), Some(NowOver::Energy));
        // Over: back to what it was told.
        let off = ChargerPlanRow { mode: Some("off".into()), ..kwh };
        assert_eq!(plan_now(&off, None, &Charger { session_energy: 7.0, ..c }, now), Some(Plan::Off { until: None }));
    }
}
