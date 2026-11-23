//! Automations (roadmap E15): rules that read like a sentence — *when* something happens, *if* a few
//! things are true, *then* act, *else* act otherwise — run by this server, with no cloud and no
//! script to write.
//!
//! * **When** (1 to [`MAX_TRIGGERS`] triggers, any one starts the rule): a measure crossing a
//!   threshold for some time (the power returned to the grid, the temperature of a room, the
//!   weather…), a device whose state becomes a value (the charger starts charging), a fixed time,
//!   sunrise, sunset, dawn or dusk, the sun reaching a façade.
//! * **If** (up to [`MAX_CONDITIONS`] conditions): all true (*and*) or at least one (*or*), with
//!   one level of groups to mix them ("A and (B or C)").
//! * **Then** / **else** (up to [`MAX_ACTIONS`] steps each): an action of a trait of a device (ADR
//!   0013: a zone and a charger are driven the same way), a scene, wait, wait until something is
//!   true (at most some time), put back what the rule changed.
//!
//! A trigger that holds (a measure past its threshold, a state, the sun on a façade) starts its
//! rule once per episode: when it has held for the time asked and the conditions are true (they are
//! tried again as long as it holds), or at once with the *else* actions when the rule has some. It
//! is armed again once it no longer holds (a measure: back past the threshold by a margin, so that
//! a value hovering around it does not start the rule over and over). A state or a façade starts
//! the rule on a change only: one that already holds when the server starts does not.
//!
//! The actions run in the background. A rule started again while it still runs is ignored, or
//! started over (`mode`). A stop of the server ends the runs under way (like Home Assistant): they
//! are marked as interrupted in the history.

use crate::db::{self, AutomationRow};
use crate::devices::{self, ActionInput, DeviceView};
use crate::energy::Flows;
use crate::error::{ApiError, ApiResult};
use crate::hub::{join, ZoneCommand};
use crate::i18n::Text;
use crate::traits::{Field, Label};
use crate::weather::Report;
use crate::AppState;
use chrono::{DateTime, Datelike, NaiveDate, NaiveTime, Timelike, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

mod conflicts;
mod describe;
mod engine;
mod run;
mod view;

pub use conflicts::*;
use describe::*;
pub use engine::*;
pub use run::*;
pub use view::*;

pub const MAX_TRIGGERS: usize = 3;
/// Conditions of a rule, those of its groups included.
pub const MAX_CONDITIONS: usize = 8;
/// Steps of each branch (then, else).
pub const MAX_ACTIONS: usize = 12;
/// Longest time a measure or a state may have to hold (minutes).
const MAX_FOR_MINUTES: i64 = 24 * 60;
/// Longest wait (seconds).
const MAX_WAIT: i64 = 24 * 3600;
/// Largest shift from a sun event (minutes).
const MAX_SUN_OFFSET: i64 = 180;
/// A fixed time missed while the server was stopped still runs within this time (seconds).
const CATCH_UP: i64 = 15 * 60;
/// A rule starts at most once in this time (seconds).
const MIN_GAP: i64 = 60;
/// How often a wait looks at the home again.
const POLL: Duration = Duration::from_secs(5);
/// A weather reading older than this is not used (seconds).
const WEATHER_MAX_AGE: i64 = 3 * 3600;
/// The sun is on a façade when it is this high at least (degrees)…
const FACADE_MIN_ELEVATION: f64 = 3.0;
/// …and at most this far from the direction the façade faces (degrees).
const FACADE_HALF_WIDTH: f64 = 80.0;
const CHECKED: &str = "automations.checked_at";
/// While set (a timestamp, or 0 for "until switched back on"), no rule starts by itself.
pub const PAUSED: &str = "automations.paused_until";
/// An ntfy topic (https://ntfy.sh/… or a server of one's own) the notifications are sent to.
pub const NTFY: &str = "automations.ntfy_url";
/// Longest notification (characters).
const MAX_MESSAGE: usize = 200;
/// The presses of a remote: single, double, long.
const PRESSES: [&str; 3] = ["single", "double", "long"];
/// Who acts, in the activity feed, when a rule runs by itself.
const ACTOR: &str = "Breezone";

// ---------------------------------------------------------------------------------------
// Rules
// ---------------------------------------------------------------------------------------

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

/// A number of the weather (Open-Meteo, read every 30 minutes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WeatherField {
    /// Rain now (mm).
    Precipitation,
    /// Wind now (km/h).
    Wind,
    /// Chance of rain within the hour (%).
    RainNextHour,
    /// Lowest temperature tomorrow (°C): frost.
    TomorrowMin,
    /// Highest temperature tomorrow (°C): heat wave.
    TomorrowMax,
    /// Relative humidity outside (%).
    Humidity,
    /// Weather warning in force (Météo-France vigilance, through MeteoAlarm): 0 green, 1 yellow,
    /// 2 orange, 3 red.
    Vigilance,
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
    Weather {
        field: WeatherField,
    },
    /// The price of electricity now, per kWh (a tariff of Settings › Electricity).
    Price,
    /// The colour of a Tempo day: 1 blue, 2 white, 3 red (none until it is announced).
    Tempo {
        day: TempoDay,
    },
    /// A number a module tells (a field of the `conditions` of its manifest).
    Module {
        module: String,
        field: String,
    },
}

fn today() -> TempoDay {
    TempoDay::Today
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TempoDay {
    Today,
    /// Announced in the morning.
    Tomorrow,
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
    /// Civil dawn: the sun 6° below the horizon, before it rises.
    Dawn,
    /// Civil dusk: the sun 6° below the horizon, after it sets.
    Dusk,
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
    /// A yes/no or a choice of the state of a device becomes `value` ("the charger starts
    /// charging"), and stays so `for_minutes`.
    State {
        device_id: i64,
        field: String,
        value: Value,
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
    /// The sun reaches a façade facing `facing` (degrees from the north: 180 south).
    Facade { facing: u32 },
    /// A button of a remote is pressed (`button@1`); any button or press when not given.
    Button {
        device_id: i64,
        #[serde(default)]
        button: Option<u32>,
        #[serde(default)]
        press: Option<String>,
    },
    /// The first one arrives home (`home`), or the last one leaves.
    Presence { home: bool },
    /// A yes/no or a choice a module tells (a field of the `conditions` of its manifest) becomes
    /// `value`, and stays so `for_minutes`.
    Module {
        module: String,
        field: String,
        value: Value,
        #[serde(default)]
        for_minutes: i64,
    },
    /// The tariff of the home comes into a stretch: a kind of day ("Jour bleu"), a slot ("HC"),
    /// or both (ids and names of the tariff; at least one).
    Tariff {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        slot: Option<String>,
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
    /// The sun is on a façade facing `facing` (degrees from the north).
    Facade {
        facing: u32,
    },
    /// Someone is at home (`home`), or nobody.
    Presence {
        home: bool,
    },
    /// A yes/no or a choice a module tells is `value` ("tomorrow is red", from the module of RTE).
    Module {
        module: String,
        field: String,
        value: Value,
    },
    /// The tariff of the home is in a stretch: today, a kind of day and/or a slot; tomorrow, a
    /// kind of day (known once its colour is announced, for a calendar that follows them).
    Tariff {
        #[serde(default = "today")]
        day: TempoDay,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        slot: Option<String>,
    },
    /// Conditions joined the other way ("A and (B or C)"): one level, no group in a group.
    Group {
        #[serde(rename = "match")]
        joined: Match,
        conditions: Vec<Condition>,
    },
}

/// How conditions are joined: all true (*and*), or at least one (*or*).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Match {
    #[default]
    All,
    Any,
}

impl Match {
    pub fn parse(s: &str) -> Option<Match> {
        match s {
            "all" => Some(Match::All),
            "any" => Some(Match::Any),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Match::All => "all",
            Match::Any => "any",
        }
    }

    /// Joins results that may be unknown: an unknown one decides only when the others do not.
    fn join(self, results: impl IntoIterator<Item = Option<bool>>) -> Option<bool> {
        let (decisive, mut unknown) = (self == Match::Any, false);
        for r in results {
            match r {
                Some(b) if b == decisive => return Some(decisive),
                None => unknown = true,
                _ => {}
            }
        }
        (!unknown).then_some(!decisive)
    }

    fn word(self) -> Text {
        match self {
            Match::All => crate::text!("et", "and", "y"),
            Match::Any => crate::text!("ou", "or", "o"),
        }
    }
}

/// What a wait does when its time runs out.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnTimeout {
    /// Go on with the next steps.
    #[default]
    Continue,
    /// Stop the run there.
    Stop,
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
    /// The same action on every device of a module of a type, in a room or anywhere: found when the
    /// rule runs, so that a device added later follows.
    Group {
        /// See `devices::TYPES`.
        device_type: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        room_id: Option<i64>,
        #[serde(rename = "trait")]
        trait_name: String,
        action: String,
        #[serde(default)]
        params: Map<String, Value>,
    },
    Scene {
        routine_id: i64,
    },
    Wait {
        seconds: i64,
    },
    /// Waits until `condition` is true (for `for_seconds` in a row), `timeout_seconds` at most.
    WaitUntil {
        condition: Box<Condition>,
        #[serde(default)]
        for_seconds: i64,
        timeout_seconds: i64,
        #[serde(default)]
        on_timeout: OnTimeout,
    },
    /// Puts back the devices this run drove before, as they were before it.
    Restore,
    /// Tells it: the activity feed, the open apps and, when set, an ntfy topic.
    Notify {
        message: String,
    },
    /// A charger follows a charging profile (`None`: none).
    ChargerProfile {
        device_id: i64,
        #[serde(default)]
        profile_id: Option<i64>,
    },
    /// A device of the list of the solar surplus follows it (`enabled`), or no longer does (it is
    /// then left as it is).
    Surplus {
        device_id: i64,
        enabled: bool,
    },
    /// A charger charges now, without a break: up to `kw` (its maximum without), until "HH:MM" or
    /// `kwh` more, else until it is unplugged.
    ChargerNow {
        device_id: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kw: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        until: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kwh: Option<f64>,
    },
}

/// What a rule does when it is started again while it still runs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunMode {
    /// The new start is ignored.
    #[default]
    Single,
    /// The run under way stops, a new one starts.
    Restart,
}

impl RunMode {
    pub fn parse(s: &str) -> Option<RunMode> {
        match s {
            "single" => Some(RunMode::Single),
            "restart" => Some(RunMode::Restart),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            RunMode::Single => "single",
            RunMode::Restart => "restart",
        }
    }
}

/// A rule, as stored.
#[derive(Debug, Clone, PartialEq)]
pub struct Automation {
    pub triggers: Vec<Trigger>,
    /// How the conditions are joined.
    pub joined: Match,
    pub conditions: Vec<Condition>,
    pub actions: Vec<Step>,
    /// Run when a trigger starts the rule and the conditions are false.
    pub otherwise: Vec<Step>,
    pub mode: RunMode,
    /// Run when the lasting trigger that started the rule stops holding ("tant que… ; ensuite…").
    pub over: Vec<Step>,
}

/// One trigger (0.11.0-beta.18) or several.
#[derive(Deserialize)]
#[serde(untagged)]
enum Triggers {
    Many(Vec<Trigger>),
    One(Trigger),
}

impl Automation {
    /// `None` for a row this version cannot read (written by a newer one): it is left alone.
    pub fn of(row: &AutomationRow) -> Option<Automation> {
        let triggers = match serde_json::from_str(&row.trigger_def).ok()? {
            Triggers::Many(t) => t,
            Triggers::One(t) => vec![t],
        };
        Some(Automation {
            triggers,
            joined: Match::parse(&row.condition_match)?,
            conditions: serde_json::from_str(&row.conditions).ok()?,
            actions: serde_json::from_str(&row.actions).ok()?,
            otherwise: serde_json::from_str(&row.else_actions).ok()?,
            mode: RunMode::parse(&row.run_mode)?,
            over: serde_json::from_str(&row.over_actions).ok()?,
        })
    }

    /// Every condition, those of the groups and of the waits included.
    fn leaves(&self) -> Vec<&Condition> {
        let waits = self.steps().filter_map(|s| match s {
            Step::WaitUntil { condition, .. } => Some(condition.as_ref()),
            _ => None,
        });
        self.conditions
            .iter()
            .chain(waits)
            .flat_map(|c| match c {
                Condition::Group { conditions, .. } => conditions.iter().collect::<Vec<_>>(),
                other => vec![other],
            })
            .collect()
    }

    /// Every step of the rule: then, else, and after it is over.
    pub fn steps(&self) -> impl Iterator<Item = &Step> {
        self.actions.iter().chain(&self.otherwise).chain(&self.over)
    }

    /// The conditions are true now (`None`: cannot be told).
    fn met(&self, ctx: &Context) -> Option<bool> {
        self.joined.join(self.conditions.iter().map(|c| c.met(ctx)))
    }

    /// The devices the rule reads.
    pub fn read_devices(&self) -> Vec<i64> {
        let triggers = self.triggers.iter().filter_map(|t| match t {
            Trigger::Measure { measure: Measure::Device { device_id, .. }, .. } | Trigger::State { device_id, .. } | Trigger::Button { device_id, .. } => Some(*device_id),
            _ => None,
        });
        let conditions = self.leaves().into_iter().filter_map(|c| match c {
            Condition::Measure { measure: Measure::Device { device_id, .. }, .. } | Condition::State { device_id, .. } => Some(*device_id),
            _ => None,
        });
        triggers.chain(conditions).collect()
    }

    /// The modules whose conditions the rule reads (its triggers, conditions and waits).
    pub fn read_modules(&self) -> Vec<String> {
        let of_measure = |m: &Measure| match m {
            Measure::Module { module, .. } => Some(module.clone()),
            _ => None,
        };
        let of_condition = |c: &Condition| match c {
            Condition::Module { module, .. } => Some(module.clone()),
            Condition::Measure { measure, .. } => of_measure(measure),
            _ => None,
        };
        let triggers = self.triggers.iter().filter_map(|t| match t {
            Trigger::Module { module, .. } => Some(module.clone()),
            Trigger::Measure { measure, .. } => of_measure(measure),
            _ => None,
        });
        let waits = self.steps().filter_map(|s| match s {
            Step::WaitUntil { condition, .. } => Some(condition.as_ref()),
            _ => None,
        });
        let conditions = self.leaves().into_iter().chain(waits.flat_map(|c| match c {
            Condition::Group { conditions, .. } => conditions.iter().collect::<Vec<_>>(),
            other => vec![other],
        }));
        let mut out: Vec<String> = triggers.chain(conditions.filter_map(of_condition)).collect();
        out.sort();
        out.dedup();
        out
    }

    /// The devices it drives, those of its groups included (as they are now).
    pub fn expanded_devices(&self, ctx: &Context) -> Vec<i64> {
        let mut ids: Vec<i64> = ctx
            .expand(&self.steps().cloned().collect::<Vec<_>>())
            .into_iter()
            .filter_map(|s| match s {
                Step::Device { device_id, .. } => Some(device_id),
                _ => None,
            })
            .collect();
        ids.dedup();
        ids
    }

    pub fn driven_devices(&self) -> Vec<i64> {
        self.steps()
            .filter_map(|s| match s {
                Step::Device { device_id, .. } => Some(*device_id),
                _ => None,
            })
            .collect()
    }

    /// The installation a rule is about: that of the first zone it drives, else reads (its sun, its
    /// weather); `None`: the home.
    pub fn place(&self, ctx: &Context) -> Option<i64> {
        self.driven_devices().into_iter().chain(self.read_devices()).find_map(|id| ctx.device(id)?.installation_id)
    }

    /// The groups the rule drives: (type, room).
    pub fn driven_groups(&self) -> Vec<(&str, Option<i64>)> {
        self.steps()
            .filter_map(|s| match s {
                Step::Group { device_type, room_id, .. } => Some((device_type.as_str(), *room_id)),
                _ => None,
            })
            .collect()
    }

    pub fn scenes(&self) -> Vec<i64> {
        self.steps()
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
    #[serde(default = "crate::yes")]
    pub enabled: bool,
    /// One trigger (as before `triggers`)…
    #[serde(default)]
    pub when: Option<Trigger>,
    /// …or several: any one starts the rule.
    #[serde(default)]
    pub triggers: Vec<Trigger>,
    /// "all" (default) or "any".
    #[serde(default, rename = "match")]
    pub joined: Match,
    #[serde(default)]
    pub conditions: Vec<Condition>,
    pub actions: Vec<Step>,
    #[serde(default, rename = "else")]
    pub otherwise: Vec<Step>,
    #[serde(default)]
    pub mode: RunMode,
    /// When the lasting trigger stops holding.
    #[serde(default)]
    pub over: Vec<Step>,
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
    pub weather: Option<Report>,
    /// Someone is at home (`None`: nobody ever told).
    pub presence: Option<bool>,
    /// Latitude and longitude of the place, for the sun.
    pub coords: Option<(f64, f64)>,
    /// Scenes, by id.
    pub scenes: HashMap<i64, String>,
    /// Rooms, by id.
    pub rooms: HashMap<i64, String>,
    /// Where each located installation is, and its weather; `outdoor`, `weather` and `coords` are
    /// those of the place a rule is looked at for (see [`Context::at`]).
    pub places: HashMap<i64, ((f64, f64), Option<Report>)>,
    /// The home: the first installation of this server with a city.
    pub home: Option<i64>,
    /// The price of electricity now (`None`: no tariff), and its currency.
    pub price: Option<f64>,
    pub currency: String,
    /// The colours of today and tomorrow (1 blue, 2 white, 3 red).
    pub tempo: [Option<f64>; 2],
    /// Charging profiles, by id.
    pub charge_profiles: HashMap<i64, String>,
    /// The tariff of the home now: its kind of day and its slot (ids, slot names).
    pub tariff_now: Option<(String, String)>,
    /// The kind of day of the tariff tomorrow.
    pub tariff_tomorrow: Option<String>,
    /// The kinds of day of the tariff in force (id, name), and the names of its slots.
    pub tariff_profiles: Vec<(String, String)>,
    pub tariff_slots: Vec<String>,
    /// The devices of the list of the solar surplus.
    pub surplus_loads: Vec<i64>,
    /// What the modules tell for the automations (`conditions` of their manifests), by module.
    pub module_values: HashMap<String, Map<String, Value>>,
}

impl Context {
    pub async fn load(state: &AppState) -> ApiResult<Context> {
        let devices = devices::list(state).await?;
        let flows = crate::energy::live_of(&state.db, &devices).await?.live.flows;
        Ok(Context {
            now: db::now(),
            tz: state.cfg.timezone,
            flows,
            devices,
            outdoor: None,
            weather: None,
            presence: {
                let people = db::list_presence(&state.db).await?;
                (!people.is_empty()).then(|| people.iter().any(|p| p.home))
            },
            coords: None,
            scenes: db::list_routines(&state.db).await?.into_iter().map(|r| (r.id, r.name)).collect(),
            rooms: db::list_rooms(&state.db).await?.into_iter().map(|r| (r.id, r.name)).collect(),
            places: {
                let mut reports = state.weather.reports().await;
                state.weather.places().await.into_iter().map(|p| (p.installation_id, ((p.lat, p.lon), reports.remove(&p.installation_id)))).collect()
            },
            home: state.weather.place(None).await.map(|p| p.installation_id),
            price: None,
            currency: match db::home_installation(&state.db).await? {
                Some(home) => db::contract(&state.db, home).await?.map_or_else(|| "EUR".into(), |c| c.currency),
                None => "EUR".into(),
            },
            tempo: [None, None],
            charge_profiles: db::list_charge_profiles(&state.db).await?.into_iter().map(|p| (p.id, p.name)).collect(),
            tariff_now: None,
            tariff_tomorrow: None,
            tariff_profiles: vec![],
            tariff_slots: vec![],
            surplus_loads: db::list_surplus_loads(&state.db).await?.into_iter().map(|l| l.device_id).collect(),
            module_values: crate::modules::conditions::values(),
        }
        .with_prices(state)
        .await?
        .at(None))
    }

    /// The price now and the colours of today and tomorrow.
    async fn with_prices(mut self, state: &AppState) -> ApiResult<Context> {
        let today = crate::rollup::local_day(self.now, self.tz);
        let known = crate::tempo::colors(&state.db, today - chrono::Days::new(1), today + chrono::Days::new(1)).await?;
        let color = |d: NaiveDate| known.get(&d).copied();
        // The prices of the home (the first installation of this server).
        let tariffs = crate::api::price::tariffs(state, None).await?;
        let at = crate::price::price_at(&tariffs, &color, self.tz, self.now);
        self.price = at.as_ref().map(|p| p.price);
        if let Some(at) = at {
            let day = NaiveDate::parse_from_str(&at.day, "%Y-%m-%d").unwrap_or(today);
            self.tariff_tomorrow = crate::price::profile_on(&tariffs, &color, day + chrono::Days::new(1)).map(|(id, _)| id);
            self.tariff_now = Some((at.profile, at.slot));
        }
        if let Some(t) = crate::price::current(&tariffs) {
            self.tariff_profiles = t.profiles.iter().map(|p| (p.id.clone(), p.name.clone())).collect();
            self.tariff_slots = t.profiles.iter().flat_map(|p| p.slots.iter().map(|s| s.name.clone())).filter(|n| !n.is_empty()).collect();
            self.tariff_slots.sort();
            self.tariff_slots.dedup();
        }
        let code = |d: NaiveDate| {
            color(d).map(|c| match c {
                crate::price::DayColor::Blue => 1.0,
                crate::price::DayColor::White => 2.0,
                crate::price::DayColor::Red => 3.0,
            })
        };
        self.tempo = [code(today), code(today + chrono::Days::new(1))];
        Ok(self)
    }

    /// What a module tells now (`None`: not read, or not told).
    pub(super) fn module_value(&self, module: &str, field: &str) -> Option<&Value> {
        self.module_values.get(module)?.get(field)
    }

    /// The tariff is in this stretch (`None`: no tariff, or tomorrow not known yet).
    fn in_tariff(&self, day: TempoDay, profile: Option<&str>, slot: Option<&str>) -> Option<bool> {
        match day {
            TempoDay::Today => {
                let (p, s) = self.tariff_now.as_ref()?;
                Some(profile.is_none_or(|x| x == p) && slot.is_none_or(|x| x == s))
            }
            TempoDay::Tomorrow => {
                let tomorrow = self.tariff_tomorrow.as_deref()?;
                Some(profile.is_none_or(|x| x == tomorrow))
            }
        }
    }

    /// The home as seen from an installation (its sun, its weather), or from the home.
    pub fn at(&self, installation_id: Option<i64>) -> Context {
        let mut here = self.clone();
        let id = installation_id.filter(|i| self.places.contains_key(i)).or(self.home);
        if let Some((coords, weather)) = id.and_then(|i| self.places.get(&i)) {
            here.coords = Some(*coords);
            here.outdoor = weather.as_ref().and_then(|w| w.temperature);
            here.weather = weather.clone();
        }
        here
    }

    pub fn device(&self, id: i64) -> Option<&DeviceView> {
        self.devices.iter().find(|d| d.id == id)
    }

    /// The devices of the modules of a group that have this trait.
    pub fn members(&self, device_type: &str, room_id: Option<i64>, trait_name: &str) -> Vec<&DeviceView> {
        self.devices
            .iter()
            .filter(|d| {
                d.installation_id.is_none()
                    && devices::type_of(&d.kind, &d.traits) == device_type
                    && room_id.is_none_or(|r| d.room_id == Some(r))
                    && d.traits.iter().any(|t| t.split_once('@').is_some_and(|(n, _)| n == trait_name))
            })
            .collect()
    }

    /// The steps with each group replaced by the action on each of its devices.
    pub fn expand(&self, steps: &[Step]) -> Vec<Step> {
        steps
            .iter()
            .flat_map(|s| match s {
                Step::Group { device_type, room_id, trait_name, action, params } => self
                    .members(device_type, *room_id, trait_name)
                    .into_iter()
                    .map(|d| Step::Device { device_id: d.id, trait_name: trait_name.clone(), action: action.clone(), params: params.clone() })
                    .collect(),
                other => vec![other.clone()],
            })
            .collect()
    }

    /// A field of the state of a device, as the catalog of traits describes it.
    fn field(&self, device_id: i64, name: &str) -> Option<&'static Field> {
        self.device(device_id)?.traits.iter().find_map(|t| {
            let (name_of, version) = t.split_once('@')?;
            crate::traits::get(name_of, version.parse().ok()?)?.state.iter().find(|f| f.name == name)
        })
    }

    /// A value of the state of a device that answers.
    fn state_of(&self, device_id: i64, field: &str) -> Option<&Value> {
        self.device(device_id).filter(|d| d.available)?.state.get(field).filter(|v| !v.is_null())
    }

    fn read(&self, m: &Measure) -> Option<f64> {
        match m {
            Measure::Energy { flow } => match flow {
                Flow::Export => self.flows.grid.map(|g| (-g).max(0.0)),
                Flow::Import => self.flows.grid.map(|g| g.max(0.0)),
                Flow::Solar => self.flows.solar,
                Flow::Home => self.flows.home,
            },
            Measure::Device { device_id, field } => self.state_of(*device_id, field)?.as_f64(),
            Measure::Outdoor => self.outdoor,
            Measure::Weather { field } => {
                let w = self.weather.as_ref().filter(|w| self.now - w.read_at < WEATHER_MAX_AGE)?;
                match field {
                    WeatherField::Precipitation => w.precipitation,
                    WeatherField::Wind => w.wind,
                    WeatherField::RainNextHour => {
                        w.hours.iter().filter(|h| h.ts > self.now - 3600 && h.ts <= self.now + 3600).filter_map(|h| h.precipitation_probability).reduce(f64::max)
                    }
                    WeatherField::TomorrowMin => w.tomorrow_min,
                    WeatherField::TomorrowMax => w.tomorrow_max,
                    WeatherField::Vigilance => w.vigilance.as_ref().map(|v| v.level as f64),
                    WeatherField::Humidity => w.humidity,
                }
            }
            Measure::Price => self.price,
            Measure::Tempo { day } => self.tempo[*day as usize],
            Measure::Module { module, field } => self.module_value(module, field)?.as_f64(),
        }
    }

    fn unit(&self, m: &Measure) -> Option<String> {
        match m {
            Measure::Energy { .. } => Some("W".into()),
            Measure::Device { device_id, field } => self.field(*device_id, field)?.unit.clone(),
            Measure::Outdoor => Some("°C".into()),
            Measure::Weather { field } => Some(
                match field {
                    WeatherField::Precipitation => "mm",
                    WeatherField::Wind => "km/h",
                    WeatherField::RainNextHour | WeatherField::Humidity => "%",
                    WeatherField::TomorrowMin | WeatherField::TomorrowMax => "°C",
                    WeatherField::Vigilance => return None,
                }
                .into(),
            ),
            Measure::Price => Some(format!("{}/kWh", if self.currency == "EUR" { "€" } else { self.currency.as_str() })),
            Measure::Tempo { .. } => None,
            Measure::Module { module, field } => crate::modules::conditions::field(module, field)?.unit.clone(),
        }
    }

    fn local(&self, ts: i64) -> DateTime<Tz> {
        DateTime::<Utc>::from_timestamp(ts, 0).unwrap_or_default().with_timezone(&self.tz)
    }

    fn sun(&self, day: NaiveDate) -> Option<crate::weather::SunTimes> {
        self.coords.map(|(lat, lon)| crate::weather::sun(day, lat, lon))
    }

    /// The sun is on a façade facing `facing` now.
    fn on_facade(&self, facing: u32) -> Option<bool> {
        let (lat, lon) = self.coords?;
        let (elevation, azimuth) = crate::weather::position(self.now, lat, lon);
        let off = (azimuth - facing as f64).rem_euclid(360.0);
        Some(elevation >= FACADE_MIN_ELEVATION && off.min(360.0 - off) <= FACADE_HALF_WIDTH)
    }
}

/// How far back past its threshold a measure must come before its rule is armed again.
fn margin(unit: Option<&str>, threshold: f64) -> f64 {
    match unit {
        Some("W") => (threshold.abs() * 0.1).max(50.0),
        Some("°C") => 0.5,
        Some("%") => 2.0,
        Some("km/h") => 5.0,
        // Rain: "it rains" (above 0) is over when it stops.
        Some("mm") => 0.0,
        _ => (threshold.abs() * 0.02).max(f64::EPSILON),
    }
}

fn time_of(s: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(s.trim(), "%H:%M").ok()
}

fn on_day(days: &[u32], at: DateTime<Tz>) -> bool {
    days.is_empty() || days.contains(&at.weekday().number_from_monday())
}

/// What a trigger sees now.
enum Look {
    /// A time or sun trigger: it happened since the last look.
    Event(bool),
    /// A trigger that holds for a while; `released` when it is over, `edge` when it must be seen
    /// changing (a state, the sun on a façade), `for_minutes` it must hold.
    Holds { holds: bool, released: bool, edge: bool, for_minutes: i64 },
}

impl Trigger {
    /// When a time or sun trigger happens on a local day, if it does.
    fn occurrence(&self, day: NaiveDate, ctx: &Context) -> Option<i64> {
        let (ts, days) = match self {
            Trigger::Time { time, days } => (day.and_time(time_of(time)?).and_local_timezone(ctx.tz).earliest()?.timestamp(), days),
            Trigger::Sun { event, offset_minutes, days } => {
                let (lat, lon) = ctx.coords?;
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

    /// It holds for a while (a measure, a state, the sun on a façade, presence): it can tell when
    /// it is over.
    pub fn lasts(&self) -> bool {
        matches!(self, Trigger::Measure { .. } | Trigger::State { .. } | Trigger::Facade { .. } | Trigger::Presence { .. } | Trigger::Tariff { .. } | Trigger::Module { .. })
    }

    /// `None`: cannot be told now (a device that does not answer).
    fn look(&self, ctx: &Context, since: i64) -> Option<Look> {
        Some(match self {
            Trigger::Measure { measure, op, value, for_minutes } => {
                let v = ctx.read(measure)?;
                let margin = margin(ctx.unit(measure).as_deref(), *value);
                Look::Holds { holds: op.holds(v, *value), released: op.released(v, *value, margin), edge: false, for_minutes: *for_minutes }
            }
            Trigger::State { device_id, field, value, for_minutes } => {
                let holds = ctx.state_of(*device_id, field)? == value;
                Look::Holds { holds, released: !holds, edge: true, for_minutes: *for_minutes }
            }
            Trigger::Facade { facing } => {
                let holds = ctx.on_facade(*facing)?;
                Look::Holds { holds, released: !holds, edge: true, for_minutes: 0 }
            }
            Trigger::Presence { home } => {
                let holds = ctx.presence? == *home;
                Look::Holds { holds, released: !holds, edge: true, for_minutes: 0 }
            }
            Trigger::Tariff { profile, slot } => {
                let holds = ctx.in_tariff(TempoDay::Today, profile.as_deref(), slot.as_deref())?;
                Look::Holds { holds, released: !holds, edge: true, for_minutes: 0 }
            }
            Trigger::Module { module, field, value, for_minutes } => {
                let holds = ctx.module_value(module, field)? == value;
                Look::Holds { holds, released: !holds, edge: true, for_minutes: *for_minutes }
            }
            Trigger::Button { device_id, button, press } => {
                let state = &ctx.device(*device_id)?.state;
                let at = state.get("last_at").and_then(Value::as_i64).unwrap_or(0);
                let which = button.is_none_or(|b| state.get("last_button").and_then(Value::as_u64) == Some(b as u64));
                let how = press.as_deref().is_none_or(|p| state.get("last_press").and_then(Value::as_str) == Some(p));
                Look::Event(at > since && at <= ctx.now && which && how)
            }
            Trigger::Time { .. } | Trigger::Sun { .. } => Look::Event(self.due(ctx, since, ctx.now).is_some()),
        })
    }
}

impl Condition {
    /// `None`: cannot be told now (a device that does not answer); the rule then waits.
    fn met(&self, ctx: &Context) -> Option<bool> {
        let now = ctx.local(ctx.now);
        match self {
            Condition::Measure { measure, op, value } => ctx.read(measure).map(|v| op.holds(v, *value)),
            Condition::State { device_id, field, value } => ctx.state_of(*device_id, field).map(|v| v == value),
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
            Condition::Facade { facing } => ctx.on_facade(*facing),
            Condition::Presence { home } => ctx.presence.map(|p| p == *home),
            Condition::Tariff { day, profile, slot } => ctx.in_tariff(*day, profile.as_deref(), slot.as_deref()),
            Condition::Module { module, field, value } => ctx.module_value(module, field).map(|v| v == value),
            Condition::Group { joined, conditions } => joined.join(conditions.iter().map(|c| c.met(ctx))),
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
            can_control: true,
            reference: json!({"system_id": 1, "zone_id": id}),
            name: name.into(),
            room_id: None,
            role: None,
            measures: Default::default(),
            position: 0,
            available: true,
            state: json!({"on": false, "room_temp": temp, "mode": "heating", "setpoint": 19.0}),
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
            state: json!({"status": status, "boost": false}),
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
            weather: None,
            presence: None,
            // Toulouse.
            coords: Some((43.6045, 1.444)),
            scenes: HashMap::from([(7, "Je pars".to_string())]),
            rooms: HashMap::new(),
            places: HashMap::new(),
            home: None,
            price: Some(0.27),
            currency: "EUR".into(),
            tempo: [Some(1.0), Some(3.0)],
            charge_profiles: HashMap::from([(1, "Nuit bleue".to_string())]),
            tariff_now: Some(("blue".into(), "HC".into())),
            tariff_tomorrow: Some("red".into()),
            tariff_profiles: vec![("blue".into(), "Jour bleu".into()), ("red".into(), "Jour rouge".into())],
            tariff_slots: vec!["HC".into(), "HP".into()],
            surplus_loads: vec![1],
            module_values: HashMap::from([("rte".to_string(), json!({"tomorrow": "red", "red_left": 12}).as_object().unwrap().clone())]),
        }
    }

    fn row(id: i64, last_run_at: Option<i64>) -> AutomationRow {
        AutomationRow {
            id,
            name: "Surplus".into(),
            enabled: true,
            trigger_def: String::new(),
            condition_match: "all".into(),
            conditions: String::new(),
            actions: String::new(),
            else_actions: "[]".into(),
            over_actions: "[]".into(),
            run_mode: "single".into(),
            last_run_at,
            last_reason: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn input(v: Value) -> AutomationInput {
        serde_json::from_value(v).unwrap()
    }

    /// The rule of the request: returning more than 500 W for 5 min, the living room below 18 °C
    /// in winter → heat the living room and charge the car.
    fn surplus() -> Automation {
        let i = input(json!({
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
        }));
        validate(i, &ctx(0, 0.0, 17.0)).unwrap().1
    }

    /// The rules started (then or else), not those the history tells did not start.
    fn started(engine: &mut Engine, rules: &[(AutomationRow, Automation)], c: &Context) -> Vec<Due> {
        engine.due(rules, c).into_iter().filter(|d| d.branch != Branch::Skipped).collect()
    }

    #[test]
    fn the_surplus_rule_reads_like_a_sentence() {
        let c = ctx(at(2026, 11, 15, 12, 0), -742.0, 17.2);
        let rule = surplus();
        assert_eq!(c.describe_trigger(&rule.triggers[0]).fr, "Injection au réseau > 500 W pendant 5 min");
        assert_eq!(c.describe_condition(&rule.conditions[0]).fr, "Salon · Température < 18 °C");
        assert_eq!(c.describe_condition(&rule.conditions[1]).fr, "de novembre à mars");
        assert_eq!(c.describe_step(&rule.actions[0]).fr, "Salon : allumée, consigne 21°");
        assert_eq!(c.describe_step(&rule.actions[1]).en, "Borne: Start charging (Maximum current 16 A)");
        assert_eq!(c.reason(&rule.triggers[0]).fr, "Injection au réseau : 742 W depuis 5 min");
        let v = view(&row(1, None), rule, &c, false);
        assert_eq!((v.status.value, v.status.holds, v.status.conditions.clone()), (Some(742.0), Some(true), vec![Some(true), Some(true)]));
    }

    #[test]
    fn a_measure_runs_once_per_episode() {
        let rules = vec![(row(1, None), surplus())];
        let t0 = at(2026, 11, 15, 12, 0);
        let mut engine = Engine::new(t0);
        let mut look = |secs: i64, grid: f64, salon: f64| started(&mut engine, &rules, &ctx(t0 + secs, grid, salon)).len();
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
        let skipped = engine.due(&rules, &ctx(t0 + 600, -800.0, 19.0));
        assert_eq!(skipped.len(), 1, "the history tells why");
        assert_eq!(skipped[0].branch, Branch::Skipped);
        assert_eq!(skipped[0].reason.fr, "Injection au réseau : 800 W depuis 5 min — conditions non remplies : Salon · Température < 18 °C (19 °C)");
        assert!(engine.due(&rules, &ctx(t0 + 700, -800.0, 19.0)).is_empty(), "told once per episode");
        let due = engine.due(&rules, &ctx(t0 + 900, -800.0, 17.5));
        assert_eq!((due.len(), due[0].branch), (1, Branch::Then), "the room got cold");
        // In July, never.
        let july = at(2026, 7, 15, 12, 0);
        let mut engine = Engine::new(july);
        assert!(started(&mut engine, &rules, &ctx(july, -800.0, 17.0)).is_empty());
        assert!(started(&mut engine, &rules, &ctx(july + 600, -800.0, 17.0)).is_empty());
        // A zone that does not answer: the rule waits, without a word.
        let mut engine = Engine::new(t0);
        let mut blind = ctx(t0 + 600, -800.0, 17.0);
        blind.devices[0].available = false;
        assert!(engine.due(&rules, &ctx(t0, -800.0, 17.0)).is_empty());
        assert!(engine.due(&rules, &blind).is_empty());
    }

    #[test]
    fn else_runs_when_the_conditions_are_false() {
        let mut rule = surplus();
        rule.otherwise = vec![Step::Device { device_id: 2, trait_name: "ev.charger".into(), action: "start".into(), params: Map::new() }];
        let rules = vec![(row(1, None), rule)];
        let t0 = at(2026, 11, 15, 12, 0);
        let mut engine = Engine::new(t0);
        assert!(engine.due(&rules, &ctx(t0, -800.0, 19.0)).is_empty());
        let due = engine.due(&rules, &ctx(t0 + 600, -800.0, 19.0));
        assert_eq!((due.len(), due[0].branch), (1, Branch::Else), "warm enough: charge the car instead");
        assert!(engine.due(&rules, &ctx(t0 + 900, -800.0, 17.0)).is_empty(), "the episode is decided");
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
    fn several_triggers_and_a_change_of_state() {
        let t0 = at(2026, 11, 15, 12, 0);
        let c = |secs: i64, status: &str| {
            let mut c = ctx(t0 + secs, 0.0, 20.0);
            c.devices[1] = charger(2, status);
            c
        };
        let rule = validate(
            input(json!({
                "name": "Charge",
                "triggers": [
                    {"kind": "state", "device_id": 2, "field": "status", "value": "charging"},
                    {"kind": "time", "time": "12:30"}
                ],
                "actions": [{"kind": "scene", "routine_id": 7}]
            })),
            &c(0, "plugged"),
        )
        .unwrap()
        .1;
        assert_eq!(c(0, "plugged").describe_triggers(&rule.triggers).fr, "Borne · État : En charge devient vrai ou à 12:30");
        let rules = vec![(row(1, None), rule)];
        let mut engine = Engine::new(t0);
        assert_eq!(started(&mut engine, &rules, &c(0, "plugged")).len(), 0);
        assert_eq!(started(&mut engine, &rules, &c(60, "charging")).len(), 1, "the charge starts");
        assert_eq!(started(&mut engine, &rules, &c(120, "charging")).len(), 0, "once");
        assert_eq!(started(&mut engine, &rules, &c(1800, "charging")).len(), 1, "12:30, the other trigger");

        // Already charging when the server starts: not a change.
        let mut engine = Engine::new(t0);
        assert_eq!(started(&mut engine, &rules, &c(0, "charging")).len(), 0);
        assert_eq!(started(&mut engine, &rules, &c(60, "plugged")).len(), 0);
        assert_eq!(started(&mut engine, &rules, &c(120, "charging")).len(), 1);
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
        // Toulouse, 15 November: the sun sets about 17:29, dusk about 17:59.
        let before_sunset = Trigger::Sun { event: SunEvent::Sunset, offset_minutes: -30, days: vec![] };
        let ts = before_sunset.next_at(&c(at(2026, 11, 15, 12, 0))).unwrap();
        assert!((at(2026, 11, 15, 16, 54)..at(2026, 11, 15, 17, 4)).contains(&ts), "{ts}");
        let dusk = Trigger::Sun { event: SunEvent::Dusk, offset_minutes: 0, days: vec![] };
        let ts = dusk.next_at(&c(at(2026, 11, 15, 12, 0))).unwrap();
        assert!((at(2026, 11, 15, 17, 54)..at(2026, 11, 15, 18, 4)).contains(&ts), "{ts}");
        assert_eq!(c(0).describe_trigger(&before_sunset).fr, "30 min avant le coucher du soleil");
        assert_eq!(c(0).describe_trigger(&dusk).fr, "au crépuscule");
        assert_eq!(c(0).describe_trigger(&before_sunset).es, "30 min antes del atardecer");
        assert_eq!(c(0).describe_trigger(&weekdays).fr, "à 07:30 (lun, mar, mer, jeu, ven)");
        assert_eq!(Condition::Daylight { day: true }.met(&c(at(2026, 11, 15, 12, 0))), Some(true));
        assert_eq!(Condition::Daylight { day: true }.met(&c(at(2026, 11, 15, 22, 0))), Some(false));
        let night = Condition::Hours { from: "22:00".into(), to: "06:00".into() };
        assert_eq!((night.met(&c(at(2026, 11, 15, 23, 0))), night.met(&c(at(2026, 11, 15, 5, 59))), night.met(&c(at(2026, 11, 15, 6, 0)))), (Some(true), Some(true), Some(false)));
        let plugged = Condition::State { device_id: 2, field: "status".into(), value: json!("plugged") };
        assert_eq!(plugged.met(&c(0)), Some(true));
        // The south façade at noon, the north never in November; the east one in the morning only.
        let (south, north, east) = (Condition::Facade { facing: 180 }, Condition::Facade { facing: 0 }, Condition::Facade { facing: 90 });
        let noon = c(at(2026, 11, 15, 12, 40));
        assert_eq!((south.met(&noon), north.met(&noon), east.met(&noon)), (Some(true), Some(false), Some(false)));
        assert_eq!(east.met(&c(at(2026, 11, 15, 9, 30))), Some(true));
        assert_eq!(c(0).describe_condition(&south).fr, "le soleil est sur la façade sud");
    }

    #[test]
    fn the_weather_is_a_measure() {
        let mut c = ctx(at(2026, 11, 15, 12, 0), 0.0, 20.0);
        let rain = Measure::Weather { field: WeatherField::RainNextHour };
        assert_eq!(c.read(&rain), None, "no weather yet");
        let hour = |ts: i64, p: f64| crate::weather::Hour { ts, temperature: None, code: None, precipitation_probability: Some(p) };
        c.weather = Some(Report {
            place: "Toulouse".into(),
            temperature: Some(6.0),
            apparent: None,
            humidity: None,
            code: None,
            precipitation: Some(0.0),
            wind: Some(42.0),
            today_min: None,
            today_max: None,
            tomorrow_min: Some(-2.0),
            tomorrow_max: Some(9.0),
            tomorrow_code: None,
            hours: vec![hour(c.now - 1800, 10.0), hour(c.now + 1800, 80.0), hour(c.now + 7200, 100.0)],
            read_at: c.now - 600,
            vigilance: None,
        });
        assert_eq!(c.read(&rain), Some(80.0), "the hour under way and the next one");
        assert_eq!(c.read(&Measure::Weather { field: WeatherField::TomorrowMin }), Some(-2.0));
        let frost = Condition::Measure { measure: Measure::Weather { field: WeatherField::TomorrowMin }, op: Op::Below, value: 0.0 };
        assert_eq!(frost.met(&c), Some(true));
        assert_eq!(c.describe_condition(&frost).fr, "Minimale de demain < 0 °C");
        c.now += WEATHER_MAX_AGE;
        assert_eq!(c.read(&rain), None, "too old");
    }

    #[test]
    fn what_is_put_back() {
        let before = |v: Value| v.as_object().unwrap().clone();
        let keys = |k: &[&str]| k.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let zone = before(json!({"on": false, "setpoint": 19.0, "mode": "heating"}));
        assert_eq!(undo("climate.zone", "set", &keys(&["on", "setpoint"]), &zone), Some(("set".into(), before(json!({"on": false, "setpoint": 19.0})))));
        let plugged = before(json!({"status": "plugged"}));
        assert_eq!(undo("ev.charger", "start", &[], &plugged), Some(("stop".into(), Map::new())));
        assert_eq!(undo("ev.charger", "stop", &[], &before(json!({"status": "charging"}))), Some(("start".into(), Map::new())));
        assert_eq!(undo("ventilation.room", "boost", &keys(&["minutes"]), &before(json!({"boost": false}))), Some(("stop_boost".into(), Map::new())));
        assert_eq!(undo("ventilation.room", "boost", &keys(&["minutes"]), &before(json!({"boost": true}))), None, "it was already boosting");
        assert_eq!(undo("cover.position", "stop", &[], &before(json!({"position": 40}))), None);
        assert_eq!(undo("cover.position", "set", &keys(&["position"]), &before(json!({"position": 40}))), Some(("set".into(), before(json!({"position": 40})))));
    }

    #[test]
    fn a_setting_changed_by_hand_is_not_put_back() {
        let obj = |v: Value| v.as_object().unwrap().clone();
        let t = Touched {
            device_id: 1,
            trait_name: "climate.zone".into(),
            action: "set".into(),
            keys: vec!["on".into(), "setpoint".into()],
            before: obj(json!({"on": false, "setpoint": 19.0})),
            set: obj(json!({"on": true, "setpoint": 21})),
        };
        assert!(changed_since(&t, &obj(json!({"on": true, "setpoint": 21.0}))).is_empty(), "as the run left it");
        assert!(changed_since(&t, &obj(json!({"on": true, "setpoint": 21.2}))).is_empty(), "rounded by the device");
        assert!(changed_since(&t, &obj(json!({"on": false, "setpoint": 19.0}))).is_empty(), "not caught up, or put back by hand");
        assert_eq!(changed_since(&t, &obj(json!({"on": true, "setpoint": 23.0}))), vec!["setpoint".to_string()]);
        assert_eq!(changed_since(&t, &obj(json!({"on": true}))), Vec::<String>::new(), "a setting the device does not report");
    }

    #[test]
    fn conditions_joined_by_or_and_in_groups() {
        let c = ctx(at(2026, 7, 15, 12, 0), -800.0, 17.0);
        let rule = |v: Value| {
            let mut i = json!({
                "name": "Test",
                "when": {"kind": "time", "time": "07:30"},
                "actions": [{"kind": "scene", "routine_id": 7}]
            });
            i.as_object_mut().unwrap().extend(v.as_object().unwrap().clone());
            validate(input(i), &c).map(|(_, r)| r)
        };
        let cold = json!({"kind": "measure", "measure": {"kind": "device", "device_id": 1, "field": "room_temp"}, "op": "below", "value": 18});
        let winter = json!({"kind": "months", "from": 11, "to": 3});
        let freezing = json!({"kind": "measure", "measure": {"kind": "outdoor"}, "op": "below", "value": 5});

        // In July at 17 °C: cold, not winter.
        let all = rule(json!({"conditions": [cold, winter]})).unwrap();
        assert_eq!(all.met(&c), Some(false));
        let any = rule(json!({"match": "any", "conditions": [cold, winter]})).unwrap();
        assert_eq!(any.met(&c), Some(true));

        // Cold and (winter or freezing outside): outside is 6 °C, so no.
        let grouped = rule(json!({"conditions": [cold, {"kind": "group", "match": "any", "conditions": [winter, freezing]}]})).unwrap();
        assert_eq!(grouped.met(&c), Some(false));
        assert_eq!(c.describe_conditions(grouped.joined, &grouped.conditions).fr, "Salon · Température < 18 °C et (de novembre à mars ou Température extérieure < 5 °C)");
        let mut freezing_day = c.clone();
        freezing_day.outdoor = Some(2.0);
        assert_eq!(grouped.met(&freezing_day), Some(true));
        assert!(grouped.read_devices().contains(&1), "a group's devices are read too");

        // An unknown value decides only when the others do not.
        let mut blind = c.clone();
        blind.devices[0].available = false;
        assert_eq!(any.met(&blind), None, "winter is false, the room unknown");
        assert_eq!(all.met(&blind), Some(false), "winter is false: false whatever the room");

        assert!(rule(json!({"conditions": [{"kind": "group", "match": "any", "conditions": [winter]}]})).is_err(), "a group of one");
        let nested = json!({"kind": "group", "match": "all", "conditions": [cold, winter]});
        assert!(rule(json!({"conditions": [{"kind": "group", "match": "any", "conditions": [nested, freezing]}]})).is_err(), "a group in a group");
    }

    #[test]
    fn what_cannot_run_is_refused() {
        let c = ctx(0, 0.0, 20.0);
        let check = |patch: fn(&mut Value)| {
            let mut v = json!({
                "name": "Test",
                "when": {"kind": "time", "time": "07:30"},
                "actions": [{"kind": "scene", "routine_id": 7}]
            });
            patch(&mut v);
            serde_json::from_value::<AutomationInput>(v).map_err(|e| e.to_string()).and_then(|i| validate(i, &c).map_err(|e| e.message.en))
        };
        assert!(check(|_| {}).is_ok());
        assert!(check(|v| v["name"] = json!(" ")).is_err());
        assert!(check(|v| v["when"]["time"] = json!("25:00")).is_err());
        assert!(check(|v| v["when"]["days"] = json!([0])).is_err());
        assert!(check(|v| v["actions"] = json!([])).is_err());
        assert!(check(|v| v["actions"][0]["routine_id"] = json!(8)).is_err(), "unknown scene");
        assert!(
            check(|v| v["when"] = json!({"kind": "measure", "measure": {"kind": "device", "device_id": 1, "field": "mode"}, "op": "above", "value": 1})).is_err(),
            "not a number"
        );
        assert!(check(|v| v["when"] = json!({"kind": "measure", "measure": {"kind": "device", "device_id": 9, "field": "room_temp"}, "op": "above", "value": 1})).is_err());
        assert!(check(|v| v["conditions"] = json!([{"kind": "state", "device_id": 2, "field": "status", "value": "flying"}])).is_err());
        assert!(check(|v| v["conditions"] = json!([{"kind": "months", "from": 0, "to": 3}])).is_err());
        assert!(check(|v| v["actions"] = json!([{"kind": "device", "device_id": 2, "trait": "ev.charger", "action": "start", "params": {"limit": 99}}])).is_err());
        assert!(check(|v| v["actions"] = json!([{"kind": "device", "device_id": 1, "trait": "climate.zone", "action": "set", "params": {}}])).is_err(), "changes nothing");
        assert!(check(|v| v["actions"] = json!([{"kind": "device", "device_id": 1, "trait": "ev.charger", "action": "start"}])).is_err(), "a zone is not a charger");
        // A device of the surplus list.
        assert!(check(|v| v["actions"] = json!([{"kind": "surplus", "device_id": 1, "enabled": false}])).is_ok());
        assert!(check(|v| v["actions"] = json!([{"kind": "surplus", "device_id": 2, "enabled": true}])).is_err(), "not in the list");
        // A stretch of the tariff it has; tomorrow, a kind of day only.
        assert!(check(|v| v["conditions"] = json!([{"kind": "tariff", "profile": "blue", "slot": "HC"}])).is_ok());
        assert!(check(|v| v["triggers"] = json!([{"kind": "tariff", "slot": "HC"}])).is_ok());
        assert!(check(|v| v["conditions"] = json!([{"kind": "tariff", "day": "tomorrow", "slot": "HC"}])).is_err());
        assert!(check(|v| v["conditions"] = json!([{"kind": "tariff", "profile": "white"}])).is_err(), "not in the tariff");
        assert!(check(|v| v["conditions"] = json!([{"kind": "tariff"}])).is_err(), "nothing chosen");
        // The actions of a charger: a profile that exists, sensible powers and times, a charger.
        assert!(check(|v| v["actions"] = json!([{"kind": "charger_profile", "device_id": 2, "profile_id": 1}])).is_ok());
        assert!(check(|v| v["actions"] = json!([{"kind": "charger_profile", "device_id": 2, "profile_id": 7}])).is_err(), "unknown profile");
        assert!(check(|v| v["actions"] = json!([{"kind": "charger_profile", "device_id": 1, "profile_id": null}])).is_err(), "a zone");
        assert!(check(|v| v["actions"] = json!([{"kind": "charger_now", "device_id": 2, "kw": 7, "until": "06:00"}])).is_ok());
        assert!(check(|v| v["actions"] = json!([{"kind": "charger_now", "device_id": 2, "kw": 70}])).is_err());
        assert!(check(|v| v["actions"] = json!([{"kind": "charger_now", "device_id": 2, "until": "6h"}])).is_err());
        // Several triggers, waits and a restore.
        assert!(check(|v| v["triggers"] = json!([{"kind": "time", "time": "08:00"}, {"kind": "time", "time": "09:00"}, {"kind": "time", "time": "10:00"}])).is_err(), "4 triggers");
        assert!(check(|v| v["actions"] = json!([{"kind": "restore"}])).is_err(), "nothing to put back");
        assert!(check(|v| v["actions"] = json!([{"kind": "wait", "seconds": 0}])).is_err());
        assert!(check(|v| v["actions"] = json!([
            {"kind": "device", "device_id": 1, "trait": "climate.zone", "action": "set", "params": {"on": false}},
            {"kind": "wait_until", "condition": {"kind": "state", "device_id": 2, "field": "status", "value": "available"}, "timeout_seconds": 3600},
            {"kind": "restore"}
        ]))
        .is_ok());
        assert!(check(|v| v["else"] = json!([{"kind": "wait", "seconds": 90000}])).is_err(), "a wait of more than a day");
    }

    #[test]
    fn remotes_presence_and_warnings() {
        let t0 = at(2026, 11, 15, 12, 0);
        let remote = |at: i64, button: u64, press: &str| DeviceView {
            kind: "remote".into(),
            traits: vec!["button@1".into()],
            source: "homekit".into(),
            source_id: Some(4),
            installation_id: None,
            reference: json!("remote"),
            state: json!({"buttons": 4, "last_button": button, "last_press": press, "last_at": at}),
            ..zone(3, "Télécommande", 0.0)
        };
        let c = |secs: i64, r: DeviceView, home: Option<bool>| {
            let mut c = ctx(t0 + secs, 0.0, 20.0);
            c.devices.push(r);
            c.presence = home;
            c
        };
        let rule = validate(
            input(json!({
                "name": "Télécommande",
                "triggers": [{"kind": "button", "device_id": 3, "button": 2, "press": "long"}, {"kind": "presence", "home": false}],
                "conditions": [{"kind": "presence", "home": false}],
                "actions": [{"kind": "notify", "message": "Maison vide"}]
            })),
            &c(0, remote(0, 1, "single"), None),
        )
        .unwrap()
        .1;
        let words = c(0, remote(0, 1, "single"), None).describe_triggers(&rule.triggers);
        assert_eq!(words.fr, "Télécommande : bouton 2, appui long ou quand le dernier part de la maison");
        assert_eq!(c(0, remote(0, 1, "single"), None).describe_step(&rule.actions[0]).fr, "prévenir : « Maison vide »");
        let rules = vec![(row(1, None), rule)];
        let mut engine = Engine::new(t0);
        // Someone home, then button 2 long pressed: the condition (nobody home) is false.
        assert!(started(&mut engine, &rules, &c(5, remote(0, 1, "single"), Some(true))).is_empty());
        assert!(started(&mut engine, &rules, &c(10, remote(t0 + 8, 2, "long"), Some(true))).is_empty());
        // The last one leaves: starts once.
        assert_eq!(started(&mut engine, &rules, &c(80, remote(t0 + 8, 2, "long"), Some(false))).len(), 1);
        assert!(started(&mut engine, &rules, &c(90, remote(t0 + 8, 2, "long"), Some(false))).is_empty(), "the same press is not seen twice");
        // Another press, nobody home: starts again (a minute after the last start).
        assert!(started(&mut engine, &rules, &c(140, remote(t0 + 138, 1, "long"), Some(false))).is_empty(), "button 1");
        assert_eq!(started(&mut engine, &rules, &c(200, remote(t0 + 198, 2, "long"), Some(false))).len(), 1);

        // A remote that has 4 buttons has no 5th; a zone has none.
        let check = |t: Value| validate(input(json!({"name": "x", "triggers": [t], "actions": [{"kind": "notify", "message": "x"}]})), &c(0, remote(0, 1, "single"), None)).is_ok();
        assert!(!check(json!({"kind": "button", "device_id": 3, "button": 5})));
        assert!(!check(json!({"kind": "button", "device_id": 1})));
        assert!(!check(json!({"kind": "button", "device_id": 3, "press": "triple"})));
        assert!(validate(
            input(json!({"name": "x", "when": {"kind": "time", "time": "08:00"}, "actions": [{"kind": "notify", "message": " "}]})),
            &c(0, remote(0, 1, "single"), None)
        )
        .is_err());

        // The weather warning, in words.
        let mut w = c(0, remote(0, 1, "single"), None);
        let orange = Trigger::Measure { measure: Measure::Weather { field: WeatherField::Vigilance }, op: Op::Above, value: 1.0, for_minutes: 0 };
        assert_eq!(w.describe_trigger(&orange).fr, "Vigilance météo > jaune");
        w.weather = Some(Report {
            place: "Toulouse".into(),
            temperature: None,
            apparent: None,
            humidity: None,
            code: None,
            precipitation: None,
            wind: None,
            today_min: None,
            today_max: None,
            tomorrow_min: None,
            tomorrow_max: None,
            tomorrow_code: None,
            hours: vec![],
            read_at: w.now,
            vigilance: Some(crate::weather::Vigilance { level: 2, events: vec!["wind".into()], area: "Haute-Garonne".into() }),
        });
        assert_eq!(w.reason(&orange).fr, "Vigilance météo : orange (wind)");

        // The price of electricity and the Tempo colours (today blue, tomorrow red in `ctx`).
        let red = Trigger::Measure { measure: Measure::Tempo { day: TempoDay::Tomorrow }, op: Op::Above, value: 2.0, for_minutes: 0 };
        assert_eq!(w.describe_trigger(&red).fr, "Couleur Tempo de demain : rouge");
        assert_eq!(w.read(&Measure::Tempo { day: TempoDay::Tomorrow }), Some(3.0));
        assert_eq!(w.reason(&red).fr, "Couleur Tempo de demain : rouge");
        let dear = Trigger::Measure { measure: Measure::Price, op: Op::Above, value: 0.2, for_minutes: 0 };
        assert_eq!(w.describe_trigger(&dear).fr, "Prix de l'électricité > 0.2 €/kWh");
        assert_eq!(w.reason(&dear).fr, "Prix de l'électricité : 0.27 €/kWh");
        assert!(validate(input(json!({"name": "x", "triggers": [{"kind": "measure", "measure": {"kind": "tempo", "day": "tomorrow"}, "op": "above", "value": 2}], "actions": [{"kind": "notify", "message": "x"}]})), &w).is_ok());
    }

    #[test]
    fn rules_that_contradict_each_other() {
        let c = ctx(0, 0.0, 20.0);
        let rule = |v: Value| validate(input(v), &c).unwrap().1;
        let named = |id: i64, name: &str| AutomationRow { name: name.into(), ..row(id, None) };
        let set = |sp: f64| json!({"kind": "device", "device_id": 1, "trait": "climate.zone", "action": "set", "params": {"setpoint": sp}});
        let at = |t: &str, days: Value| json!({"kind": "time", "time": t, "days": days});
        let rules = vec![
            (named(1, "a"), rule(json!({"name": "a", "when": at("07:00", json!([1, 2])), "actions": [set(21.0)]}))),
            (named(2, "b"), rule(json!({"name": "b", "when": at("07:00", json!([2, 3])), "actions": [set(18.0)]}))),
            (named(3, "c"), rule(json!({"name": "c", "when": at("07:00", json!([6])), "actions": [set(17.0)]}))),
            (named(4, "d"), rule(json!({"name": "d", "when": at("07:00", json!([])), "actions": [set(21.0)]}))),
            (named(5, "e"), rule(json!({"name": "e", "when": at("07:00", json!([])), "actions": [{"kind": "device", "device_id": 2, "trait": "ev.charger", "action": "start"}]}))),
            (named(6, "f"), rule(json!({"name": "f", "when": at("07:00", json!([])), "actions": [{"kind": "device", "device_id": 2, "trait": "ev.charger", "action": "stop"}]}))),
        ];
        let found = conflicts(&rules, &c);
        let names = |id: i64| found.get(&id).map(|v| v.iter().map(|x| x.name.clone()).collect::<Vec<_>>()).unwrap_or_default();
        assert_eq!(names(1), vec!["b".to_string()], "same time on Tuesday, 21° and 18°; d sets 21° too");
        assert_eq!(names(3), vec!["d".to_string()], "c is on Saturday: only d, every day");
        assert_eq!(names(5), vec!["f".to_string()], "start and stop");
        assert_eq!(found[&1][0].what.fr, "Salon (Consigne)");

        // One when it is cold and the car charges, the other when it is hot and it is only plugged
        // (as « Salon · Mode : Chaud » and « … : Froid »): never at the same time.
        let surplus = |name: &str, op: &str, t: f64, mode: &str, sp: f64| {
            rule(json!({"name": name, "when": at("12:00", json!([])), "conditions": [
                {"kind": "measure", "measure": {"kind": "outdoor"}, "op": op, "value": t},
                {"kind": "state", "device_id": 2, "field": "status", "value": mode}
            ], "actions": [set(sp)]}))
        };
        let rules = vec![(named(1, "heat"), surplus("heat", "below", 14.0, "charging", 20.0)), (named(2, "cool"), surplus("cool", "above", 25.0, "plugged", 23.0))];
        assert!(conflicts(&rules, &c).is_empty(), "their conditions exclude each other");
        let rules = vec![(named(1, "heat"), surplus("heat", "below", 14.0, "charging", 20.0)), (named(2, "warm"), surplus("warm", "below", 18.0, "charging", 22.0))];
        assert_eq!(conflicts(&rules, &c).len(), 2, "both may run when it is 12° outside");
    }

    #[test]
    fn each_rule_sees_the_weather_of_its_installation() {
        let report = |t: f64, h: f64| Report {
            place: String::new(),
            temperature: Some(t),
            apparent: None,
            humidity: Some(h),
            code: None,
            precipitation: None,
            wind: None,
            today_min: None,
            today_max: None,
            tomorrow_min: None,
            tomorrow_max: None,
            tomorrow_code: None,
            hours: vec![],
            read_at: 0,
            vigilance: None,
        };
        let mut c = ctx(0, 0.0, 20.0);
        // The home (installation 1) in Toulouse, installation 2 in Lille.
        c.places = HashMap::from([(1, ((43.6, 1.44), Some(report(14.0, 60.0)))), (2, ((50.63, 3.06), Some(report(6.0, 91.0))))]);
        c.home = Some(1);
        let mut lille = zone(5, "Bureau", 19.0);
        lille.installation_id = Some(2);
        c.devices.push(lille);
        let humid = Measure::Weather { field: WeatherField::Humidity };
        let rule = |device: i64| {
            validate(
                input(json!({"name": "x", "when": {"kind": "time", "time": "08:00"}, "actions": [{"kind": "device", "device_id": device, "trait": "climate.zone", "action": "set", "params": {"on": true}}]})),
                &c,
            )
            .unwrap()
            .1
        };
        let (salon, bureau) = (rule(1), rule(5));
        assert_eq!(bureau.place(&c), Some(2));
        let (here, there) = (c.at(salon.place(&c)), c.at(bureau.place(&c)));
        assert_eq!((here.outdoor, here.read(&humid)), (Some(14.0), Some(60.0)));
        assert_eq!((there.outdoor, there.read(&humid)), (Some(6.0), Some(91.0)));
        assert_eq!(there.coords, Some((50.63, 3.06)));
        assert_eq!(c.at(None).outdoor, Some(14.0), "a rule of the modules alone: the home");
        assert_eq!(c.describe_condition(&Condition::Measure { measure: humid, op: Op::Above, value: 80.0 }).fr, "Humidité extérieure > 80 %");
    }

    #[test]
    fn waits_read_like_a_sentence() {
        let c = ctx(0, 0.0, 20.0);
        let until = Step::WaitUntil {
            condition: Box::new(Condition::Measure { measure: Measure::Energy { flow: Flow::Import }, op: Op::Above, value: 200.0 }),
            for_seconds: 600,
            timeout_seconds: 8 * 3600,
            on_timeout: OnTimeout::Continue,
        };
        assert_eq!(c.describe_step(&until).fr, "attendre que Soutirage du réseau > 200 W pendant 10 min (au plus 8 h)");
        assert_eq!(c.describe_step(&Step::Wait { seconds: 5400 }).fr, "attendre 1 h 30");
        assert_eq!(c.describe_step(&Step::Restore).fr, "remettre comme avant");
    }

    #[test]
    fn the_stretches_of_the_tariff() {
        let c = ctx(0, 0.0, 20.0);
        let cond = |day, profile: Option<&str>, slot: Option<&str>| Condition::Tariff { day, profile: profile.map(Into::into), slot: slot.map(Into::into) };
        // Now: a blue day, off-peak hours; tomorrow: red.
        assert_eq!(cond(TempoDay::Today, Some("blue"), Some("HC")).met(&c), Some(true));
        assert_eq!(cond(TempoDay::Today, None, Some("HP")).met(&c), Some(false));
        assert_eq!(cond(TempoDay::Tomorrow, Some("red"), None).met(&c), Some(true));
        let unknown = Context { tariff_tomorrow: None, ..c.clone() };
        assert_eq!(cond(TempoDay::Tomorrow, Some("red"), None).met(&unknown), None, "tomorrow not announced yet");
        assert_eq!(c.describe_condition(&cond(TempoDay::Today, Some("blue"), Some("HC"))).fr, "le tarif est en Jour bleu · HC");
        assert_eq!(c.describe_condition(&cond(TempoDay::Tomorrow, Some("red"), None)).fr, "le tarif de demain est en Jour rouge");
        let starts = Trigger::Tariff { profile: None, slot: Some("HC".into()) };
        assert_eq!(c.describe_trigger(&starts).fr, "quand le tarif passe en HC");
        assert!(starts.lasts());
    }

    #[test]
    fn what_a_module_tells() {
        let c = ctx(0, 0.0, 20.0);
        let red = Condition::Module { module: "rte".into(), field: "tomorrow".into(), value: json!("red") };
        assert_eq!(red.met(&c), Some(true));
        assert_eq!(Condition::Module { module: "rte".into(), field: "tomorrow".into(), value: json!("blue") }.met(&c), Some(false));
        assert_eq!(Condition::Module { module: "rte".into(), field: "today".into(), value: json!("blue") }.met(&c), None, "not told: the rule waits");
        assert_eq!(c.read(&Measure::Module { module: "rte".into(), field: "red_left".into() }), Some(12.0));
        // Without the module installed, its words are its names.
        assert_eq!(c.describe_condition(&red).fr, "rte · tomorrow : red");
        let becomes = Trigger::Module { module: "rte".into(), field: "tomorrow".into(), value: json!("red"), for_minutes: 0 };
        assert!(becomes.lasts());
    }

    #[test]
    fn the_actions_of_a_charger_read_like_a_sentence() {
        let c = ctx(0, 0.0, 20.0);
        assert_eq!(c.describe_step(&Step::ChargerProfile { device_id: 2, profile_id: Some(1) }).fr, "Borne : profil « Nuit bleue »");
        assert_eq!(c.describe_step(&Step::ChargerProfile { device_id: 2, profile_id: None }).fr, "Borne : aucun profil");
        let now = Step::ChargerNow { device_id: 2, kw: Some(7.4), until: Some("06:00".into()), kwh: None };
        assert_eq!(c.describe_step(&now).fr, "Borne : charger maintenant jusqu'à 7,4 kW, jusqu'à 06:00");
        assert_eq!(c.describe_step(&now).en, "Borne: charge now up to 7.4 kW, until 06:00");
        assert_eq!(c.describe_step(&Step::Surplus { device_id: 1, enabled: false }).fr, "Salon : ne plus suivre le surplus solaire");
        let full = Step::ChargerNow { device_id: 2, kw: None, until: None, kwh: Some(10.0) };
        assert_eq!(c.describe_step(&full).fr, "Borne : charger maintenant au maximum, +10 kWh");
    }
}
