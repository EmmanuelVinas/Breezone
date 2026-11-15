//! Automations (roadmap E15): rules that read like a sentence — *when* something happens, *if* a few
//! things are true, *then* act — run by this server, with no cloud and no script to write.
//!
//! * **When** (one trigger): a measure crossing a threshold for some time (the power returned to
//!   the grid, the temperature of a room…), a fixed time, sunrise or sunset.
//! * **If** (up to [`MAX_CONDITIONS`] conditions, all true): a measure, the state of a device, a
//!   period of the year, hours, days, day or night.
//! * **Then** (1 to [`MAX_ACTIONS`] actions): an action of a trait of a device (ADR 0013: a zone
//!   and a charger are driven the same way), or a scene.
//!
//! A measure trigger runs its rule once per episode: when the measure has held past the threshold
//! for the time asked *and* the conditions are true (they are tried again as long as it holds).
//! It is armed again once the measure has come back past the threshold by a margin, so that a
//! value hovering around it does not run the rule over and over.

use crate::api::home::{self, ActionInput, DeviceView};
use crate::db::{self, AutomationRow};
use crate::energy::Flows;
use crate::error::{ApiError, ApiResult};
use crate::hub::{join, ZoneCommand};
use crate::i18n::Text;
use crate::routines::ActionResult;
use crate::traits::{Field, Label};
use crate::AppState;
use chrono::{DateTime, Datelike, NaiveDate, NaiveTime, Timelike, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::time::Duration;

pub const MAX_CONDITIONS: usize = 3;
pub const MAX_ACTIONS: usize = 5;
/// Longest time a measure may have to hold (minutes).
const MAX_FOR_MINUTES: i64 = 24 * 60;
/// Largest shift from sunrise or sunset (minutes).
const MAX_SUN_OFFSET: i64 = 180;
/// A fixed time missed while the server was stopped still runs within this time (seconds).
const CATCH_UP: i64 = 15 * 60;
/// A rule runs at most once in this time (seconds).
const MIN_GAP: i64 = 60;
const CHECKED: &str = "automations.checked_at";
/// Who acts, in the activity feed, when a rule runs by itself.
const ACTOR: &str = "Breezone";

/// A power of the home (W), from its energy measures (see `energy`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Flow {
    /// Returned to the grid.
    Export,
    /// Drawn from the grid.
    Import,
    Solar,
    /// Used by the home.
    Home,
}

/// A number that can be read now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Measure {
    Energy {
        flow: Flow,
    },
    /// A number of the state of a device (the temperature of a zone, the power of a plug).
    Device {
        device_id: i64,
        field: String,
    },
    /// The outdoor temperature (weather).
    Outdoor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    Above,
    Below,
}

impl Op {
    fn holds(self, v: f64, threshold: f64) -> bool {
        match self {
            Op::Above => v > threshold,
            Op::Below => v < threshold,
        }
    }

    /// The measure has come back past the threshold by `margin`.
    fn released(self, v: f64, threshold: f64, margin: f64) -> bool {
        match self {
            Op::Above => v <= threshold - margin,
            Op::Below => v >= threshold + margin,
        }
    }

    fn sign(self) -> &'static str {
        match self {
            Op::Above => ">",
            Op::Below => "<",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SunEvent {
    Sunrise,
    Sunset,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Trigger {
    Measure {
        measure: Measure,
        op: Op,
        value: f64,
        /// The measure must hold that long (0: at once).
        #[serde(default)]
        for_minutes: i64,
    },
    Time {
        /// "HH:MM", local time.
        time: String,
        /// 1 (Monday) to 7 (Sunday); empty: every day.
        #[serde(default)]
        days: Vec<u32>,
    },
    Sun {
        event: SunEvent,
        /// Minutes after the event (negative: before).
        #[serde(default)]
        offset_minutes: i64,
        #[serde(default)]
        days: Vec<u32>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Condition {
    Measure {
        measure: Measure,
        op: Op,
        value: f64,
    },
    /// A yes/no or a choice of the state of a device ("the car is plugged in").
    State {
        device_id: i64,
        field: String,
        value: Value,
    },
    /// Months of the year, 1 to 12; `from` after `to` goes over the new year (11 to 3: winter).
    Months {
        from: u32,
        to: u32,
    },
    /// "HH:MM" to "HH:MM", local time; `from` after `to` goes over midnight.
    Hours {
        from: String,
        to: String,
    },
    Days {
        days: Vec<u32>,
    },
    /// The sun is up (`day`), or down.
    Daylight {
        day: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Step {
    /// An action of a trait of a device, as `POST /api/devices/{id}/actions` takes it.
    Device {
        device_id: i64,
        #[serde(rename = "trait")]
        trait_name: String,
        action: String,
        #[serde(default)]
        params: Map<String, Value>,
    },
    Scene {
        routine_id: i64,
    },
}

/// A rule, as stored.
#[derive(Debug, Clone, PartialEq)]
pub struct Automation {
    pub when: Trigger,
    pub conditions: Vec<Condition>,
    pub actions: Vec<Step>,
}

impl Automation {
    /// `None` for a row this version cannot read (written by a newer one): it is left alone.
    pub fn of(row: &AutomationRow) -> Option<Automation> {
        Some(Automation {
            when: serde_json::from_str(&row.trigger_def).ok()?,
            conditions: serde_json::from_str(&row.conditions).ok()?,
            actions: serde_json::from_str(&row.actions).ok()?,
        })
    }

    fn measures(&self) -> impl Iterator<Item = &Measure> {
        let when = match &self.when {
            Trigger::Measure { measure, .. } => Some(measure),
            _ => None,
        };
        when.into_iter().chain(self.conditions.iter().filter_map(|c| match c {
            Condition::Measure { measure, .. } => Some(measure),
            _ => None,
        }))
    }

    /// The devices the rule reads.
    pub fn read_devices(&self) -> Vec<i64> {
        let measured = self.measures().filter_map(|m| match m {
            Measure::Device { device_id, .. } => Some(*device_id),
            _ => None,
        });
        let states = self.conditions.iter().filter_map(|c| match c {
            Condition::State { device_id, .. } => Some(*device_id),
            _ => None,
        });
        measured.chain(states).collect()
    }

    /// The devices the rule drives.
    pub fn driven_devices(&self) -> Vec<i64> {
        self.actions
            .iter()
            .filter_map(|s| match s {
                Step::Device { device_id, .. } => Some(*device_id),
                _ => None,
            })
            .collect()
    }

    pub fn scenes(&self) -> Vec<i64> {
        self.actions
            .iter()
            .filter_map(|s| match s {
                Step::Scene { routine_id } => Some(*routine_id),
                _ => None,
            })
            .collect()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct AutomationInput {
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    pub when: Trigger,
    #[serde(default)]
    pub conditions: Vec<Condition>,
    pub actions: Vec<Step>,
}

fn yes() -> bool {
    true
}

// ---------------------------------------------------------------------------------------
// What the rules look at
// ---------------------------------------------------------------------------------------

/// The home at a moment: everything a rule may read, loaded once for all of them.
#[derive(Debug, Clone)]
pub struct Context {
    pub now: i64,
    pub tz: Tz,
    pub devices: Vec<DeviceView>,
    pub flows: Flows,
    pub outdoor: Option<f64>,
    /// Latitude and longitude of the home, for the sun.
    pub coords: Option<(f64, f64)>,
    /// Scenes, by id.
    pub scenes: HashMap<i64, String>,
}

impl Context {
    pub async fn load(state: &AppState) -> ApiResult<Context> {
        let devices = home::devices_of(state).await?;
        let flows = crate::api::energy::live_of(state, &devices).await?.live.flows;
        Ok(Context {
            now: db::now(),
            tz: state.cfg.timezone,
            flows,
            devices,
            outdoor: state.weather.temperature().await,
            coords: state.weather.coordinates().await,
            scenes: db::list_routines(&state.db).await?.into_iter().map(|r| (r.id, r.name)).collect(),
        })
    }

    pub fn device(&self, id: i64) -> Option<&DeviceView> {
        self.devices.iter().find(|d| d.id == id)
    }

    /// A field of the state of a device, as the catalog of traits describes it.
    fn field(&self, device_id: i64, name: &str) -> Option<&'static Field> {
        self.device(device_id)?.traits.iter().find_map(|t| {
            let (name_of, version) = t.split_once('@')?;
            crate::traits::get(name_of, version.parse().ok()?)?.state.iter().find(|f| f.name == name)
        })
    }

    fn read(&self, m: &Measure) -> Option<f64> {
        match m {
            Measure::Energy { flow } => match flow {
                Flow::Export => self.flows.grid.map(|g| (-g).max(0.0)),
                Flow::Import => self.flows.grid.map(|g| g.max(0.0)),
                Flow::Solar => self.flows.solar,
                Flow::Home => self.flows.home,
            },
            Measure::Device { device_id, field } => self.device(*device_id).filter(|d| d.available)?.state.get(field)?.as_f64(),
            Measure::Outdoor => self.outdoor,
        }
    }

    fn unit(&self, m: &Measure) -> Option<String> {
        match m {
            Measure::Energy { .. } => Some("W".into()),
            Measure::Device { device_id, field } => self.field(*device_id, field)?.unit.clone(),
            Measure::Outdoor => Some("°C".into()),
        }
    }

    fn local(&self, ts: i64) -> DateTime<Tz> {
        DateTime::<Utc>::from_timestamp(ts, 0).unwrap_or_default().with_timezone(&self.tz)
    }

    fn sun(&self, day: NaiveDate) -> Option<crate::weather::SunTimes> {
        self.coords.map(|(lat, lon)| crate::weather::sun(day, lat, lon))
    }
}

/// How far back past its threshold a measure must come before its rule is armed again.
fn margin(unit: Option<&str>, threshold: f64) -> f64 {
    match unit {
        Some("W") => (threshold.abs() * 0.1).max(50.0),
        Some("°C") => 0.5,
        Some("%") => 2.0,
        _ => (threshold.abs() * 0.02).max(f64::EPSILON),
    }
}

fn time_of(s: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(s.trim(), "%H:%M").ok()
}

fn on_day(days: &[u32], at: DateTime<Tz>) -> bool {
    days.is_empty() || days.contains(&at.weekday().number_from_monday())
}

impl Trigger {
    /// When a time or sun trigger happens on a local day, if it does.
    fn occurrence(&self, day: NaiveDate, ctx: &Context) -> Option<i64> {
        let (ts, days) = match self {
            Trigger::Measure { .. } => return None,
            Trigger::Time { time, days } => (day.and_time(time_of(time)?).and_local_timezone(ctx.tz).earliest()?.timestamp(), days),
            Trigger::Sun { event, offset_minutes, days } => {
                let sun = ctx.sun(day)?;
                let at = match event {
                    SunEvent::Sunrise => sun.sunrise,
                    SunEvent::Sunset => sun.sunset,
                }?;
                (at + offset_minutes * 60, days)
            }
        };
        on_day(days, ctx.local(ts)).then_some(ts)
    }

    /// The occurrence in `(from, to]`, if any.
    fn due(&self, ctx: &Context, from: i64, to: i64) -> Option<i64> {
        let (a, b) = (ctx.local(from).date_naive(), ctx.local(to).date_naive());
        [Some(a), (a != b).then_some(b)].into_iter().flatten().filter_map(|day| self.occurrence(day, ctx)).find(|ts| *ts > from && *ts <= to)
    }

    /// The next occurrence, within a week.
    fn next_at(&self, ctx: &Context) -> Option<i64> {
        let today = ctx.local(ctx.now).date_naive();
        (0..8).filter_map(|d| self.occurrence(today + chrono::Days::new(d), ctx)).find(|ts| *ts > ctx.now)
    }
}

impl Condition {
    /// `None`: cannot be told now (a device that does not answer); the rule then waits.
    fn met(&self, ctx: &Context) -> Option<bool> {
        let now = ctx.local(ctx.now);
        match self {
            Condition::Measure { measure, op, value } => ctx.read(measure).map(|v| op.holds(v, *value)),
            Condition::State { device_id, field, value } => ctx.device(*device_id).filter(|d| d.available)?.state.get(field).map(|v| v == value),
            Condition::Months { from, to } => {
                let m = now.month();
                Some(if from <= to { (*from..=*to).contains(&m) } else { m >= *from || m <= *to })
            }
            Condition::Hours { from, to } => {
                let (from, to, t) = (time_of(from)?, time_of(to)?, now.time().with_second(0)?);
                Some(if from < to { t >= from && t < to } else { from == to || t >= from || t < to })
            }
            Condition::Days { days } => Some(on_day(days, now)),
            Condition::Daylight { day } => {
                let sun = ctx.sun(now.date_naive())?;
                Some((ctx.now >= sun.sunrise? && ctx.now < sun.sunset?) == *day)
            }
        }
    }
}

// ---------------------------------------------------------------------------------------
// Words
// ---------------------------------------------------------------------------------------

fn label(l: &Label) -> Text {
    Text { fr: l.fr.clone(), en: l.en.clone(), es: l.es.clone() }
}

fn num(v: f64) -> String {
    let r = (v * 10.0).round() / 10.0 + 0.0;
    if r.fract() == 0.0 {
        format!("{r:.0}")
    } else {
        format!("{r}")
    }
}

fn amount(v: f64, unit: Option<&str>) -> String {
    match unit {
        Some(u) => format!("{} {u}", num(v)),
        None => num(v),
    }
}

fn yes_no(b: bool) -> Text {
    if b {
        crate::text!("oui", "yes", "sí")
    } else {
        crate::text!("non", "no", "no")
    }
}

const MONTHS: [(&str, &str, &str); 12] = [
    ("janvier", "January", "enero"),
    ("février", "February", "febrero"),
    ("mars", "March", "marzo"),
    ("avril", "April", "abril"),
    ("mai", "May", "mayo"),
    ("juin", "June", "junio"),
    ("juillet", "July", "julio"),
    ("août", "August", "agosto"),
    ("septembre", "September", "septiembre"),
    ("octobre", "October", "octubre"),
    ("novembre", "November", "noviembre"),
    ("décembre", "December", "diciembre"),
];

const DAYS: [(&str, &str, &str); 7] =
    [("lun", "Mon", "lun"), ("mar", "Tue", "mar"), ("mer", "Wed", "mié"), ("jeu", "Thu", "jue"), ("ven", "Fri", "vie"), ("sam", "Sat", "sáb"), ("dim", "Sun", "dom")];

fn named(table: &[(&str, &str, &str)], n: u32) -> Text {
    let (fr, en, es) = table.get((n as usize).wrapping_sub(1)).copied().unwrap_or(("?", "?", "?"));
    Text { fr: fr.into(), en: en.into(), es: es.into() }
}

fn day_list(days: &[u32]) -> Text {
    let mut sorted = days.to_vec();
    sorted.sort_unstable();
    join(&sorted.iter().map(|d| named(&DAYS, *d)).collect::<Vec<_>>(), ", ")
}

impl Context {
    fn device_name(&self, id: i64) -> Text {
        match self.device(id) {
            Some(d) => Text::plain(d.name.clone()),
            None => crate::text!("appareil {0} (supprimé)", "device {0} (deleted)", "dispositivo {0} (eliminado)", id),
        }
    }

    fn field_label(&self, device_id: i64, field: &str) -> Text {
        self.field(device_id, field).map_or_else(|| Text::plain(field), |f| label(&f.label))
    }

    fn measure_label(&self, m: &Measure) -> Text {
        match m {
            Measure::Energy { flow: Flow::Export } => crate::text!("Injection au réseau", "Grid export", "Inyección a la red"),
            Measure::Energy { flow: Flow::Import } => crate::text!("Soutirage du réseau", "Grid import", "Consumo de la red"),
            Measure::Energy { flow: Flow::Solar } => crate::text!("Production solaire", "Solar production", "Producción solar"),
            Measure::Energy { flow: Flow::Home } => crate::text!("Consommation de la maison", "Home consumption", "Consumo de la casa"),
            Measure::Outdoor => crate::text!("Température extérieure", "Outdoor temperature", "Temperatura exterior"),
            Measure::Device { device_id, field } => {
                let (name, field) = (self.device_name(*device_id), self.field_label(*device_id, field));
                crate::text!("{0} · {1}", "{0} · {1}", "{0} · {1}", name.arg(), field.arg())
            }
        }
    }

    /// "Injection au réseau > 500 W".
    fn comparison(&self, m: &Measure, op: Op, value: f64) -> Text {
        let (name, threshold) = (self.measure_label(m), amount(value, self.unit(m).as_deref()));
        crate::text!("{0} {1} {2}", "{0} {1} {2}", "{0} {1} {2}", name.arg(), op.sign(), threshold)
    }

    pub fn describe_trigger(&self, t: &Trigger) -> Text {
        let with_days = |text: Text, days: &[u32]| {
            if days.is_empty() || days.len() == 7 {
                return text;
            }
            let days = day_list(days);
            crate::text!("{0} ({1})", "{0} ({1})", "{0} ({1})", text.arg(), days.arg())
        };
        match t {
            Trigger::Measure { measure, op, value, for_minutes } => {
                let c = self.comparison(measure, *op, *value);
                if *for_minutes > 0 {
                    crate::text!("{0} pendant {1} min", "{0} for {1} min", "{0} durante {1} min", c.arg(), for_minutes)
                } else {
                    c
                }
            }
            Trigger::Time { time, days } => with_days(crate::text!("à {0}", "at {0}", "a las {0}", time), days),
            Trigger::Sun { event, offset_minutes, days } => {
                let n = offset_minutes.abs();
                let text = match (event, offset_minutes.signum()) {
                    (SunEvent::Sunrise, 0) => crate::text!("au lever du soleil", "at sunrise", "al amanecer"),
                    (SunEvent::Sunset, 0) => crate::text!("au coucher du soleil", "at sunset", "al atardecer"),
                    (SunEvent::Sunrise, -1) => crate::text!("{0} min avant le lever du soleil", "{0} min before sunrise", "{0} min antes del amanecer", n),
                    (SunEvent::Sunrise, _) => crate::text!("{0} min après le lever du soleil", "{0} min after sunrise", "{0} min después del amanecer", n),
                    (SunEvent::Sunset, -1) => crate::text!("{0} min avant le coucher du soleil", "{0} min before sunset", "{0} min antes del atardecer", n),
                    (SunEvent::Sunset, _) => crate::text!("{0} min après le coucher du soleil", "{0} min after sunset", "{0} min después del atardecer", n),
                };
                with_days(text, days)
            }
        }
    }

    pub fn describe_condition(&self, c: &Condition) -> Text {
        match c {
            Condition::Measure { measure, op, value } => self.comparison(measure, *op, *value),
            Condition::State { device_id, field, value } => {
                let (name, field) = (self.device_name(*device_id), self.field_label(*device_id, field));
                let value = match value {
                    Value::Bool(b) => yes_no(*b),
                    Value::String(s) => Text::plain(s.clone()),
                    other => Text::plain(other.to_string()),
                };
                crate::text!("{0} · {1} : {2}", "{0} · {1}: {2}", "{0} · {1}: {2}", name.arg(), field.arg(), value.arg())
            }
            Condition::Months { from, to } if from == to => {
                let m = named(&MONTHS, *from);
                crate::text!("en {0}", "in {0}", "en {0}", m.arg())
            }
            Condition::Months { from, to } => {
                let (a, b) = (named(&MONTHS, *from), named(&MONTHS, *to));
                crate::text!("de {0} à {1}", "from {0} to {1}", "de {0} a {1}", a.arg(), b.arg())
            }
            Condition::Hours { from, to } => crate::text!("entre {0} et {1}", "between {0} and {1}", "entre las {0} y las {1}", from, to),
            Condition::Days { days } => day_list(days),
            Condition::Daylight { day: true } => crate::text!("de jour", "during the day", "de día"),
            Condition::Daylight { day: false } => crate::text!("de nuit", "at night", "de noche"),
        }
    }

    pub fn describe_step(&self, s: &Step) -> Text {
        match s {
            Step::Scene { routine_id } => match self.scenes.get(routine_id) {
                Some(name) => crate::text!("scène « {0} »", "scene “{0}”", "escena «{0}»", name),
                None => crate::text!("scène supprimée", "deleted scene", "escena eliminada"),
            },
            Step::Device { device_id, trait_name, action, params } => {
                let name = self.device_name(*device_id);
                let what = self.describe_action(*device_id, trait_name, action, params);
                crate::text!("{0} : {1}", "{0}: {1}", "{0}: {1}", name.arg(), what.arg())
            }
        }
    }

    fn describe_action(&self, device_id: i64, trait_name: &str, action: &str, params: &Map<String, Value>) -> Text {
        if (trait_name, action) == ("climate.zone", "set") {
            if let Ok(cmd) = serde_json::from_value::<ZoneCommand>(Value::Object(params.clone())) {
                return cmd.describe();
            }
        }
        let described = self.device(device_id).and_then(|d| {
            let version = d.traits.iter().find_map(|t| t.split_once('@').filter(|(n, _)| *n == trait_name)?.1.parse().ok())?;
            crate::traits::get(trait_name, version)?.action(action)
        });
        let Some(a) = described else { return Text::plain(format!("{trait_name}.{action}")) };
        let given: Vec<Text> = a
            .params
            .iter()
            .filter_map(|p| {
                let value = match params.get(&p.name)? {
                    Value::Null => return None,
                    Value::Bool(b) => yes_no(*b),
                    Value::String(s) => Text::plain(s.clone()),
                    other => Text::plain(other.as_f64().map_or_else(|| other.to_string(), |v| amount(v, p.unit.as_deref()))),
                };
                let name = label(&p.label);
                Some(crate::text!("{0} {1}", "{0} {1}", "{0} {1}", name.arg(), value.arg()))
            })
            .collect();
        let name = label(&a.label);
        if given.is_empty() {
            return name;
        }
        let given = join(&given, ", ");
        crate::text!("{0} ({1})", "{0} ({1})", "{0} ({1})", name.arg(), given.arg())
    }

    /// What was true when the rule ran: "Injection au réseau : 742 W depuis 5 min".
    fn reason(&self, t: &Trigger) -> Text {
        match t {
            Trigger::Measure { measure, for_minutes, .. } => {
                let (name, unit) = (self.measure_label(measure), self.unit(measure));
                let value = self.read(measure).map_or_else(|| "?".to_string(), |v| amount(v, unit.as_deref()));
                if *for_minutes > 0 {
                    crate::text!("{0} : {1} depuis {2} min", "{0}: {1} for {2} min", "{0}: {1} desde hace {2} min", name.arg(), value, for_minutes)
                } else {
                    crate::text!("{0} : {1}", "{0}: {1}", "{0}: {1}", name.arg(), value)
                }
            }
            other => self.describe_trigger(other),
        }
    }
}

// ---------------------------------------------------------------------------------------
// Views and validation
// ---------------------------------------------------------------------------------------

/// The rule as a sentence: "Quand {when}, si {conditions}, alors {actions}".
#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub when: Text,
    pub conditions: Vec<Text>,
    pub actions: Vec<Text>,
}

/// What the rule sees now.
#[derive(Debug, Clone, Serialize)]
pub struct Status {
    /// The measure of the trigger, now.
    pub value: Option<f64>,
    pub unit: Option<String>,
    /// The measure is past its threshold.
    pub holds: Option<bool>,
    /// Each condition: true, false, or unknown.
    pub conditions: Vec<Option<bool>>,
    /// Next run of a time or sun trigger.
    pub next_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AutomationView {
    pub id: i64,
    pub name: String,
    pub enabled: bool,
    pub when: Trigger,
    pub conditions: Vec<Condition>,
    pub actions: Vec<Step>,
    pub summary: Summary,
    pub status: Status,
    pub last_run_at: Option<i64>,
    pub last_reason: Option<Text>,
    pub created_at: i64,
    pub updated_at: i64,
}

pub fn view(row: &AutomationRow, rule: Automation, ctx: &Context) -> AutomationView {
    let (value, unit, holds) = match &rule.when {
        Trigger::Measure { measure, op, value, .. } => {
            let read = ctx.read(measure);
            (read, ctx.unit(measure), read.map(|v| op.holds(v, *value)))
        }
        _ => (None, None, None),
    };
    AutomationView {
        id: row.id,
        name: row.name.clone(),
        enabled: row.enabled,
        summary: Summary {
            when: ctx.describe_trigger(&rule.when),
            conditions: rule.conditions.iter().map(|c| ctx.describe_condition(c)).collect(),
            actions: rule.actions.iter().map(|s| ctx.describe_step(s)).collect(),
        },
        status: Status { value, unit, holds, conditions: rule.conditions.iter().map(|c| c.met(ctx)).collect(), next_at: rule.when.next_at(ctx) },
        when: rule.when,
        conditions: rule.conditions,
        actions: rule.actions,
        last_run_at: row.last_run_at,
        last_reason: row.last_reason.as_deref().map(Text::load),
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

fn check_days(days: &[u32]) -> ApiResult<()> {
    let mut seen = days.to_vec();
    seen.sort_unstable();
    seen.dedup();
    if seen.len() != days.len() || days.iter().any(|d| !(1..=7).contains(d)) {
        return Err(ApiError::bad_request(crate::text!(
            "jours : de 1 (lundi) à 7 (dimanche), chacun une fois",
            "days: 1 (Monday) to 7 (Sunday), each once",
            "días: de 1 (lunes) a 7 (domingo), una vez cada uno"
        )));
    }
    Ok(())
}

fn check_time(s: &str) -> ApiResult<()> {
    time_of(s).map(|_| ()).ok_or_else(|| ApiError::bad_request(crate::text!("heure attendue : HH:MM", "time expected: HH:MM", "hora esperada: HH:MM")))
}

impl Context {
    fn known(&self, device_id: i64) -> ApiResult<&DeviceView> {
        self.device(device_id).ok_or_else(|| ApiError::bad_request(crate::text!("appareil introuvable", "unknown device", "dispositivo desconocido")))
    }

    fn check_measure(&self, m: &Measure, value: f64) -> ApiResult<()> {
        if !value.is_finite() {
            return Err(ApiError::bad_request(crate::text!("seuil invalide", "invalid threshold", "umbral no válido")));
        }
        if let Measure::Device { device_id, field } = m {
            self.known(*device_id)?;
            if !self.field(*device_id, field).is_some_and(|f| matches!(f.kind.as_str(), "number" | "integer")) {
                return Err(ApiError::bad_request(crate::text!(
                    "« {0} » n'est pas une mesure de cet appareil",
                    "“{0}” is not a measure of this device",
                    "«{0}» no es una medida de este dispositivo",
                    field
                )));
            }
        }
        Ok(())
    }

    fn check_trigger(&self, t: &Trigger) -> ApiResult<()> {
        match t {
            Trigger::Measure { measure, value, for_minutes, .. } => {
                if !(0..=MAX_FOR_MINUTES).contains(for_minutes) {
                    return Err(ApiError::bad_request(crate::text!(
                        "durée entre 0 minute et 24 heures",
                        "duration between 0 minutes and 24 hours",
                        "duración entre 0 minutos y 24 horas"
                    )));
                }
                self.check_measure(measure, *value)
            }
            Trigger::Time { time, days } => {
                check_time(time)?;
                check_days(days)
            }
            Trigger::Sun { offset_minutes, days, .. } => {
                if offset_minutes.abs() > MAX_SUN_OFFSET {
                    return Err(ApiError::bad_request(crate::text!("décalage de 3 heures au plus", "offset of 3 hours at most", "desfase de 3 horas como máximo")));
                }
                check_days(days)
            }
        }
    }

    fn check_condition(&self, c: &Condition) -> ApiResult<()> {
        match c {
            Condition::Measure { measure, value, .. } => self.check_measure(measure, *value),
            Condition::State { device_id, field, value } => {
                self.known(*device_id)?;
                let ok = self.field(*device_id, field).is_some_and(|f| match f.kind.as_str() {
                    "bool" => value.is_boolean(),
                    "enum" => value.as_str().is_some_and(|v| f.values.iter().any(|x| x == v)),
                    _ => false,
                });
                if ok {
                    Ok(())
                } else {
                    Err(ApiError::bad_request(crate::text!(
                        "« {0} » n'est pas un état de cet appareil",
                        "“{0}” is not a state of this device",
                        "«{0}» no es un estado de este dispositivo",
                        field
                    )))
                }
            }
            Condition::Months { from, to } => {
                if (1..=12).contains(from) && (1..=12).contains(to) {
                    Ok(())
                } else {
                    Err(ApiError::bad_request(crate::text!("mois de 1 à 12", "months from 1 to 12", "meses de 1 a 12")))
                }
            }
            Condition::Hours { from, to } => {
                check_time(from)?;
                check_time(to)
            }
            Condition::Days { days } => {
                if days.is_empty() {
                    return Err(ApiError::bad_request(crate::text!("choisissez au moins un jour", "choose at least one day", "elija al menos un día")));
                }
                check_days(days)
            }
            Condition::Daylight { .. } => Ok(()),
        }
    }

    fn check_step(&self, s: &Step) -> ApiResult<()> {
        match s {
            Step::Scene { routine_id } => {
                if self.scenes.contains_key(routine_id) {
                    Ok(())
                } else {
                    Err(ApiError::bad_request(crate::text!("scène introuvable", "unknown scene", "escena desconocida")))
                }
            }
            Step::Device { device_id, trait_name, action, params } => {
                let device = self.known(*device_id)?;
                home::check_action(device, &ActionInput { trait_name: trait_name.clone(), action: action.clone(), params: params.clone() })?;
                if (trait_name.as_str(), action.as_str()) == ("climate.zone", "set") && ZoneCommand::from_value_strict(Value::Object(params.clone()))?.is_empty() {
                    return Err(ApiError::bad_request("every action must change at least one setting"));
                }
                Ok(())
            }
        }
    }
}

/// Checks a rule against the devices of the home; returns its name and the rule to store.
pub fn validate(input: AutomationInput, ctx: &Context) -> ApiResult<(String, Automation)> {
    let name = input.name.trim().to_string();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(ApiError::bad_request("name must be 1-80 characters"));
    }
    if input.conditions.len() > MAX_CONDITIONS {
        return Err(ApiError::bad_request(crate::text!("{0} conditions au plus", "{0} conditions at most", "{0} condiciones como máximo", MAX_CONDITIONS)));
    }
    if input.actions.is_empty() || input.actions.len() > MAX_ACTIONS {
        return Err(ApiError::bad_request(crate::text!("de 1 à {0} actions", "1 to {0} actions", "de 1 a {0} acciones", MAX_ACTIONS)));
    }
    ctx.check_trigger(&input.when)?;
    input.conditions.iter().try_for_each(|c| ctx.check_condition(c))?;
    input.actions.iter().try_for_each(|s| ctx.check_step(s))?;
    Ok((name, Automation { when: input.when, conditions: input.conditions, actions: input.actions }))
}

// ---------------------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------------------

/// What is remembered of a measure trigger between two looks.
#[derive(Debug, Clone, Copy)]
struct Watch {
    /// The rule it was for (an edited rule starts again).
    updated_at: i64,
    /// Since when the measure is past its threshold.
    since: Option<i64>,
    /// The rule ran for this episode.
    fired: bool,
}

/// Decides which rules run. Kept in memory: after a restart, a measure already past its threshold
/// counts as a new episode.
#[derive(Debug)]
pub struct Engine {
    watches: HashMap<i64, Watch>,
    /// When the rules were last looked at (time and sun triggers run once between two looks).
    checked: i64,
}

impl Engine {
    pub fn new(checked: i64) -> Self {
        Self { watches: HashMap::new(), checked }
    }

    /// The rules to run now, each with what made it run.
    pub fn due(&mut self, rules: &[(AutomationRow, Automation)], ctx: &Context) -> Vec<(i64, Text)> {
        let mut out = vec![];
        self.watches.retain(|id, _| rules.iter().any(|(row, _)| row.id == *id && row.enabled));
        for (row, rule) in rules.iter().filter(|(row, _)| row.enabled) {
            let recent = row.last_run_at.is_some_and(|t| ctx.now - t < MIN_GAP);
            let conditions = || rule.conditions.iter().all(|c| c.met(ctx) == Some(true));
            match &rule.when {
                Trigger::Measure { measure, op, value, for_minutes } => {
                    let w = self.watches.entry(row.id).or_insert(Watch { updated_at: row.updated_at, since: None, fired: false });
                    if w.updated_at != row.updated_at {
                        *w = Watch { updated_at: row.updated_at, since: None, fired: false };
                    }
                    // A measure that cannot be read changes nothing.
                    let Some(v) = ctx.read(measure) else { continue };
                    if w.fired {
                        if op.released(v, *value, margin(ctx.unit(measure).as_deref(), *value)) {
                            w.fired = false;
                            w.since = None;
                        }
                        continue;
                    }
                    if !op.holds(v, *value) {
                        w.since = None;
                        continue;
                    }
                    let since = *w.since.get_or_insert(ctx.now);
                    if ctx.now - since >= for_minutes * 60 && !recent && conditions() {
                        w.fired = true;
                        out.push((row.id, ctx.reason(&rule.when)));
                    }
                }
                when => {
                    if when.due(ctx, self.checked, ctx.now).is_some() && !recent && conditions() {
                        out.push((row.id, ctx.reason(when)));
                    }
                }
            }
        }
        self.checked = ctx.now;
        out
    }
}

async fn run_step(state: &AppState, ctx: &Context, step: &Step, actor: &str) -> ApiResult<()> {
    match step {
        Step::Device { device_id, trait_name, action, params } => {
            let device = db::device_by_id(&state.db, *device_id).await?.ok_or_else(|| ApiError::not_found("device not found"))?;
            let view = ctx.device(*device_id).ok_or_else(|| ApiError::not_found("device not found"))?;
            let input = ActionInput { trait_name: trait_name.clone(), action: action.clone(), params: params.clone() };
            home::check_action(view, &input)?;
            home::apply(state, &device, view, &input, actor).await
        }
        Step::Scene { routine_id } => {
            let row = db::routine_by_id(&state.db, *routine_id).await?.ok_or_else(|| ApiError::not_found("routine not found"))?;
            let results = crate::routines::run_routine(&state.hub, &state.db, &row, actor).await?;
            match results.iter().filter(|r| !r.ok).count() {
                0 => Ok(()),
                failed => Err(ApiError::bad_gateway(crate::text!("{0} action(s) en échec", "{0} action(s) failed", "{0} acción(es) con fallo", failed))),
            }
        }
    }
}

/// Runs the actions of a rule, one after the other, and tells the activity feed why.
pub async fn run(state: &AppState, row: &AutomationRow, rule: &Automation, ctx: &Context, reason: Text, actor: &str) -> Vec<ActionResult> {
    let who = format!("{actor} · automatisme « {} »", row.name);
    let mut results = Vec::with_capacity(rule.actions.len());
    for step in &rule.actions {
        let r = run_step(state, ctx, step, &who).await;
        results.push(ActionResult { ok: r.is_ok(), error: r.err().map(|e| e.message) });
    }
    if let Err(e) = db::set_automation_run(&state.db, row.id, db::now(), &reason.store()).await {
        tracing::error!("automation {}: {e}", row.id);
    }
    let failed = results.iter().filter(|r| !r.ok).count();
    let (level, message) = if failed == 0 {
        ("info", crate::text!("Automatisme « {0} » : {1}", "Automation “{0}”: {1}", "Automatismo «{0}»: {1}", row.name, reason.arg()))
    } else {
        (
            "warn",
            crate::text!(
                "Automatisme « {0} » : {1} — {2} échec(s) sur {3}",
                "Automation “{0}”: {1} — {2} failure(s) out of {3}",
                "Automatismo «{0}»: {1} — {2} fallo(s) de {3}",
                row.name,
                reason.arg(),
                failed,
                results.len()
            ),
        )
    };
    state.hub.log("automation", level, None, Some(actor), message, Some(json!({ "automation_id": row.id, "results": results }))).await;
    state.hub.routines_changed();
    results
}

/// Looks at the rules once: runs those that are due.
pub async fn tick(state: &AppState, engine: &mut Engine) {
    let rows = match db::list_automations(&state.db).await {
        Ok(rows) => rows,
        Err(e) => return tracing::error!("automations: {e}"),
    };
    if !rows.iter().any(|r| r.enabled) {
        engine.watches.clear();
        engine.checked = db::now();
        return;
    }
    let ctx = match Context::load(state).await {
        Ok(ctx) => ctx,
        Err(e) => return tracing::error!("automations: {}", e.message.en),
    };
    let rules: Vec<(AutomationRow, Automation)> = rows.into_iter().filter_map(|row| Automation::of(&row).map(|rule| (row, rule))).collect();
    for (id, reason) in engine.due(&rules, &ctx) {
        if let Some((row, rule)) = rules.iter().find(|(row, _)| row.id == id) {
            run(state, row, rule, &ctx, reason, ACTOR).await;
        }
    }
}

pub async fn run_loop(state: AppState) {
    let now = db::now();
    // A fixed time missed during a short stop still runs.
    let kept = db::get_setting(&state.db, CHECKED).await.ok().flatten().and_then(|v| v.parse::<i64>().ok());
    let mut engine = Engine::new(kept.unwrap_or(now).clamp(now - CATCH_UP, now));
    let mut saved = now;
    let mut ticker = tokio::time::interval(Duration::from_secs(5));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticker.tick().await;
        tick(&state, &mut engine).await;
        if engine.checked - saved >= 60 {
            saved = engine.checked;
            if let Err(e) = db::set_setting(&state.db, CHECKED, &saved.to_string()).await {
                tracing::error!("automations: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    const PARIS: Tz = chrono_tz::Europe::Paris;

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> i64 {
        PARIS.with_ymd_and_hms(y, m, d, h, min, 0).unwrap().timestamp()
    }

    fn zone(id: i64, name: &str, temp: f64) -> DeviceView {
        DeviceView {
            id,
            kind: "climate.zone".into(),
            traits: vec!["climate.zone@1".into()],
            source: "airzone".into(),
            source_id: None,
            installation_id: Some(1),
            reference: json!({"system_id": 1, "zone_id": id}),
            name: name.into(),
            room_id: None,
            role: None,
            measures: Default::default(),
            position: 0,
            available: true,
            state: json!({"on": false, "room_temp": temp, "mode": "heating"}),
        }
    }

    fn charger(id: i64, status: &str) -> DeviceView {
        DeviceView {
            kind: "ev.charger".into(),
            traits: vec!["ev.charger@1".into()],
            source: "trydan".into(),
            source_id: Some(3),
            installation_id: None,
            reference: json!("charger"),
            state: json!({"status": status}),
            ..zone(id, "Borne", 0.0)
        }
    }

    fn ctx(now: i64, grid: f64, salon: f64) -> Context {
        Context {
            now,
            tz: PARIS,
            devices: vec![zone(1, "Salon", salon), charger(2, "plugged")],
            flows: Flows { grid: Some(grid), solar: Some(3000.0), battery: None, home: Some(3000.0 + grid) },
            outdoor: Some(6.0),
            // Toulouse.
            coords: Some((43.6045, 1.444)),
            scenes: HashMap::from([(7, "Je pars".to_string())]),
        }
    }

    fn row(id: i64, last_run_at: Option<i64>) -> AutomationRow {
        AutomationRow {
            id,
            name: "Surplus".into(),
            enabled: true,
            trigger_def: String::new(),
            conditions: String::new(),
            actions: String::new(),
            last_run_at,
            last_reason: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    /// The rule of the request: returning more than 500 W for 5 min, the living room below 18 °C
    /// in winter → heat the living room and charge the car.
    fn surplus() -> Automation {
        serde_json::from_value::<AutomationInput>(json!({
            "name": "Surplus",
            "when": {"kind": "measure", "measure": {"kind": "energy", "flow": "export"}, "op": "above", "value": 500, "for_minutes": 5},
            "conditions": [
                {"kind": "measure", "measure": {"kind": "device", "device_id": 1, "field": "room_temp"}, "op": "below", "value": 18},
                {"kind": "months", "from": 11, "to": 3}
            ],
            "actions": [
                {"kind": "device", "device_id": 1, "trait": "climate.zone", "action": "set", "params": {"on": true, "setpoint": 21}},
                {"kind": "device", "device_id": 2, "trait": "ev.charger", "action": "start", "params": {"limit": 16}}
            ]
        }))
        .map(|i| validate(i, &ctx(0, 0.0, 17.0)).unwrap().1)
        .unwrap()
    }

    #[test]
    fn the_surplus_rule_reads_like_a_sentence() {
        let c = ctx(at(2026, 11, 15, 12, 0), -742.0, 17.2);
        let rule = surplus();
        assert_eq!(c.describe_trigger(&rule.when).fr, "Injection au réseau > 500 W pendant 5 min");
        assert_eq!(c.describe_condition(&rule.conditions[0]).fr, "Salon · Température < 18 °C");
        assert_eq!(c.describe_condition(&rule.conditions[1]).fr, "de novembre à mars");
        assert_eq!(c.describe_step(&rule.actions[0]).fr, "Salon : allumée, consigne 21°");
        assert_eq!(c.describe_step(&rule.actions[1]).en, "Borne: Start charging (Maximum current 16 A)");
        assert_eq!(c.reason(&rule.when).fr, "Injection au réseau : 742 W depuis 5 min");
        let v = view(&row(1, None), rule, &c);
        assert_eq!((v.status.value, v.status.holds, v.status.conditions.clone()), (Some(742.0), Some(true), vec![Some(true), Some(true)]));
    }

    #[test]
    fn a_measure_runs_once_per_episode() {
        let rules = vec![(row(1, None), surplus())];
        let t0 = at(2026, 11, 15, 12, 0);
        let mut engine = Engine::new(t0);
        let mut look = |secs: i64, grid: f64, salon: f64| engine.due(&rules, &ctx(t0 + secs, grid, salon)).len();
        assert_eq!(look(0, -800.0, 17.0), 0, "must hold 5 min");
        assert_eq!(look(120, -300.0, 17.0), 0, "a cloud: the count starts again");
        assert_eq!(look(180, -800.0, 17.0), 0);
        assert_eq!(look(470, -800.0, 17.0), 0, "4 min 50");
        assert_eq!(look(480, -800.0, 17.0), 1, "5 min: runs");
        assert_eq!(look(600, -900.0, 17.0), 0, "once");
        assert_eq!(look(700, -470.0, 17.0), 0, "just under the threshold: not armed again");
        assert_eq!(look(1100, -900.0, 17.0), 0);
        assert_eq!(look(1200, -100.0, 17.0), 0, "well under: armed again");
        assert_eq!(look(1300, -900.0, 17.0), 0);
        assert_eq!(look(1600, -900.0, 17.0), 1, "a new episode");
    }

    #[test]
    fn conditions_are_tried_again_while_the_measure_holds() {
        let rules = vec![(row(1, None), surplus())];
        let t0 = at(2026, 11, 15, 12, 0);
        let mut engine = Engine::new(t0);
        assert!(engine.due(&rules, &ctx(t0, -800.0, 19.0)).is_empty());
        assert!(engine.due(&rules, &ctx(t0 + 600, -800.0, 19.0)).is_empty(), "warm enough");
        assert_eq!(engine.due(&rules, &ctx(t0 + 900, -800.0, 17.5)).len(), 1, "the room got cold");
        // In July, never.
        let july = at(2026, 7, 15, 12, 0);
        let mut engine = Engine::new(july);
        assert!(engine.due(&rules, &ctx(july, -800.0, 17.0)).is_empty());
        assert!(engine.due(&rules, &ctx(july + 600, -800.0, 17.0)).is_empty());
        // A zone that does not answer: the rule waits.
        let mut engine = Engine::new(t0);
        let mut blind = ctx(t0 + 600, -800.0, 17.0);
        blind.devices[0].available = false;
        assert!(engine.due(&rules, &ctx(t0, -800.0, 17.0)).is_empty());
        assert!(engine.due(&rules, &blind).is_empty());
    }

    #[test]
    fn a_rule_that_just_ran_waits_a_minute() {
        let t0 = at(2026, 11, 15, 12, 0);
        let rules = vec![(row(1, Some(t0 + 590)), surplus())];
        let mut engine = Engine::new(t0);
        assert!(engine.due(&rules, &ctx(t0, -800.0, 17.0)).is_empty());
        assert!(engine.due(&rules, &ctx(t0 + 600, -800.0, 17.0)).is_empty());
        assert_eq!(engine.due(&rules, &ctx(t0 + 660, -800.0, 17.0)).len(), 1);
    }

    #[test]
    fn fixed_times_and_the_sun() {
        let c = |now| ctx(now, 0.0, 20.0);
        // Sunday 15 November 2026.
        let weekdays = Trigger::Time { time: "07:30".into(), days: vec![1, 2, 3, 4, 5] };
        let sunday = Trigger::Time { time: "07:30".into(), days: vec![7] };
        let (before, after) = (at(2026, 11, 15, 7, 29), at(2026, 11, 15, 7, 31));
        assert_eq!(sunday.due(&c(after), before, after), Some(at(2026, 11, 15, 7, 30)));
        assert_eq!(sunday.due(&c(after), after, after + 60), None, "once");
        assert_eq!(weekdays.due(&c(after), before, after), None);
        assert_eq!(weekdays.next_at(&c(after)), Some(at(2026, 11, 16, 7, 30)));
        // Around midnight.
        let late = Trigger::Time { time: "23:59".into(), days: vec![] };
        assert_eq!(late.due(&c(at(2026, 11, 16, 0, 1)), at(2026, 11, 15, 23, 58), at(2026, 11, 16, 0, 1)), Some(at(2026, 11, 15, 23, 59)));
        // Toulouse, 15 November: the sun sets about 17:29.
        let dusk = Trigger::Sun { event: SunEvent::Sunset, offset_minutes: -30, days: vec![] };
        let ts = dusk.next_at(&c(at(2026, 11, 15, 12, 0))).unwrap();
        assert!((at(2026, 11, 15, 16, 54)..at(2026, 11, 15, 17, 4)).contains(&ts), "{ts}");
        assert_eq!(c(0).describe_trigger(&dusk).fr, "30 min avant le coucher du soleil");
        assert_eq!(c(0).describe_trigger(&weekdays).fr, "à 07:30 (lun, mar, mer, jeu, ven)");
        assert_eq!(Condition::Daylight { day: true }.met(&c(at(2026, 11, 15, 12, 0))), Some(true));
        assert_eq!(Condition::Daylight { day: true }.met(&c(at(2026, 11, 15, 22, 0))), Some(false));
        let night = Condition::Hours { from: "22:00".into(), to: "06:00".into() };
        assert_eq!((night.met(&c(at(2026, 11, 15, 23, 0))), night.met(&c(at(2026, 11, 15, 5, 59))), night.met(&c(at(2026, 11, 15, 6, 0)))), (Some(true), Some(true), Some(false)));
        let plugged = Condition::State { device_id: 2, field: "status".into(), value: json!("plugged") };
        assert_eq!(plugged.met(&c(0)), Some(true));
    }

    #[test]
    fn what_cannot_run_is_refused() {
        let c = ctx(0, 0.0, 20.0);
        let input = |patch: fn(&mut Value)| {
            let mut v = json!({
                "name": "Test",
                "when": {"kind": "time", "time": "07:30"},
                "actions": [{"kind": "scene", "routine_id": 7}]
            });
            patch(&mut v);
            serde_json::from_value::<AutomationInput>(v).map_err(|e| e.to_string()).and_then(|i| validate(i, &c).map_err(|e| e.message.en))
        };
        assert!(input(|_| {}).is_ok());
        assert!(input(|v| v["name"] = json!(" ")).is_err());
        assert!(input(|v| v["when"]["time"] = json!("25:00")).is_err());
        assert!(input(|v| v["when"]["days"] = json!([0])).is_err());
        assert!(input(|v| v["actions"] = json!([])).is_err());
        assert!(input(|v| v["actions"][0]["routine_id"] = json!(8)).is_err(), "unknown scene");
        assert!(
            input(|v| v["when"] = json!({"kind": "measure", "measure": {"kind": "device", "device_id": 1, "field": "mode"}, "op": "above", "value": 1})).is_err(),
            "not a number"
        );
        assert!(input(|v| v["when"] = json!({"kind": "measure", "measure": {"kind": "device", "device_id": 9, "field": "room_temp"}, "op": "above", "value": 1})).is_err());
        assert!(input(|v| v["conditions"] = json!([{"kind": "state", "device_id": 2, "field": "status", "value": "flying"}])).is_err());
        assert!(input(|v| v["conditions"] = json!([{"kind": "months", "from": 0, "to": 3}])).is_err());
        assert!(input(|v| v["actions"] = json!([{"kind": "device", "device_id": 2, "trait": "ev.charger", "action": "start", "params": {"limit": 99}}])).is_err());
        assert!(input(|v| v["actions"] = json!([{"kind": "device", "device_id": 1, "trait": "climate.zone", "action": "set", "params": {}}])).is_err(), "changes nothing");
        assert!(input(|v| v["actions"] = json!([{"kind": "device", "device_id": 1, "trait": "ev.charger", "action": "start"}])).is_err(), "a zone is not a charger");
    }
}
