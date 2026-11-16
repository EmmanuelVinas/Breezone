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

use crate::api::home::{self, ActionInput, DeviceView};
use crate::db::{self, AutomationRow};
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

pub const MAX_TRIGGERS: usize = 3;
/// Conditions of a rule, those of its groups included.
pub const MAX_CONDITIONS: usize = 8;
/// Steps of each branch (then, else).
pub const MAX_ACTIONS: usize = 8;
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

    fn steps(&self) -> impl Iterator<Item = &Step> {
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

    /// The devices the rule drives.
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
    #[serde(default = "yes")]
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
    pub weather: Option<Report>,
    /// Someone is at home (`None`: nobody ever told).
    pub presence: Option<bool>,
    /// Latitude and longitude of the place, for the sun.
    pub coords: Option<(f64, f64)>,
    /// Scenes, by id.
    pub scenes: HashMap<i64, String>,
    /// Where each located installation is, and its weather; `outdoor`, `weather` and `coords` are
    /// those of the place a rule is looked at for (see [`Context::at`]).
    pub places: HashMap<i64, ((f64, f64), Option<Report>)>,
    /// The home: the first installation of this server with a city.
    pub home: Option<i64>,
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
            outdoor: None,
            weather: None,
            presence: {
                let people = db::list_presence(&state.db).await?;
                (!people.is_empty()).then(|| people.iter().any(|p| p.home))
            },
            coords: None,
            scenes: db::list_routines(&state.db).await?.into_iter().map(|r| (r.id, r.name)).collect(),
            places: {
                let mut reports = state.weather.reports().await;
                state.weather.places().await.into_iter().map(|p| (p.installation_id, ((p.lat, p.lon), reports.remove(&p.installation_id)))).collect()
            },
            home: state.weather.place(None).await.map(|p| p.installation_id),
        }
        .at(None))
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
        matches!(self, Trigger::Measure { .. } | Trigger::State { .. } | Trigger::Facade { .. } | Trigger::Presence { .. })
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
            Condition::Group { joined, conditions } => joined.join(conditions.iter().map(|c| c.met(ctx))),
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

/// "45 s", "10 min", "2 h", "1 h 30".
fn duration(seconds: i64) -> String {
    if seconds < 60 {
        format!("{seconds} s")
    } else {
        crate::timers::human_delay(seconds / 60)
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

/// The eight directions a façade faces, from the north.
const FACINGS: [(&str, &str, &str); 8] = [
    ("nord", "north", "norte"),
    ("nord-est", "north-east", "noreste"),
    ("est", "east", "este"),
    ("sud-est", "south-east", "sureste"),
    ("sud", "south", "sur"),
    ("sud-ouest", "south-west", "suroeste"),
    ("ouest", "west", "oeste"),
    ("nord-ouest", "north-west", "noroeste"),
];

/// The colours of the weather warnings, 0 to 3.
const VIGILANCE: [(&str, &str, &str); 4] = [("verte", "green", "verde"), ("jaune", "yellow", "amarilla"), ("orange", "orange", "naranja"), ("rouge", "red", "roja")];

fn colour(level: f64) -> Text {
    named(&VIGILANCE, (level.round().clamp(0.0, 3.0) as u32) + 1)
}

fn press_name(press: &str) -> Text {
    match press {
        "double" => crate::text!("double appui", "double press", "doble pulsación"),
        "long" => crate::text!("appui long", "long press", "pulsación larga"),
        _ => crate::text!("appui", "press", "pulsación"),
    }
}

fn named(table: &[(&str, &str, &str)], n: u32) -> Text {
    let (fr, en, es) = table.get((n as usize).wrapping_sub(1)).copied().unwrap_or(("?", "?", "?"));
    Text { fr: fr.into(), en: en.into(), es: es.into() }
}

fn day_list(days: &[u32]) -> Text {
    let mut sorted = days.to_vec();
    sorted.sort_unstable();
    join(&sorted.iter().map(|d| named(&DAYS, *d)).collect::<Vec<_>>(), ", ")
}

fn facing(degrees: u32) -> Text {
    named(&FACINGS, ((degrees as f64 / 45.0).round() as u32) % 8 + 1)
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
            Measure::Weather { field } => match field {
                WeatherField::Precipitation => crate::text!("Pluie", "Rain", "Lluvia"),
                WeatherField::Wind => crate::text!("Vent", "Wind", "Viento"),
                WeatherField::RainNextHour => crate::text!("Risque de pluie dans l'heure", "Chance of rain within the hour", "Probabilidad de lluvia en la próxima hora"),
                WeatherField::TomorrowMin => crate::text!("Minimale de demain", "Tomorrow's low", "Mínima de mañana"),
                WeatherField::TomorrowMax => crate::text!("Maximale de demain", "Tomorrow's high", "Máxima de mañana"),
                WeatherField::Vigilance => crate::text!("Vigilance météo", "Weather warning", "Aviso meteorológico"),
                WeatherField::Humidity => crate::text!("Humidité extérieure", "Outdoor humidity", "Humedad exterior"),
            },
            Measure::Device { device_id, field } => {
                let (name, field) = (self.device_name(*device_id), self.field_label(*device_id, field));
                crate::text!("{0} · {1}", "{0} · {1}", "{0} · {1}", name.arg(), field.arg())
            }
        }
    }

    /// A value of a measure, in words: "742 W", "orange".
    fn value_text(&self, m: &Measure, v: f64) -> Text {
        match m {
            Measure::Weather { field: WeatherField::Vigilance } => colour(v),
            _ => Text::plain(amount(v, self.unit(m).as_deref())),
        }
    }

    /// "Injection au réseau > 500 W".
    fn comparison(&self, m: &Measure, op: Op, value: f64) -> Text {
        let (name, threshold) = (self.measure_label(m), self.value_text(m, value));
        let threshold = threshold.arg();
        crate::text!("{0} {1} {2}", "{0} {1} {2}", "{0} {1} {2}", name.arg(), op.sign(), threshold)
    }

    /// "Borne · État : En charge".
    fn state_text(&self, device_id: i64, field: &str, value: &Value) -> Text {
        let named = self.field(device_id, field).and_then(|f| f.value_labels.get(value.as_str()?)).map(label);
        let (name, field) = (self.device_name(device_id), self.field_label(device_id, field));
        let value = match (named, value) {
            (Some(named), _) => named,
            (None, Value::Bool(b)) => yes_no(*b),
            (None, Value::String(s)) => Text::plain(s.clone()),
            (None, other) => Text::plain(other.to_string()),
        };
        crate::text!("{0} · {1} : {2}", "{0} · {1}: {2}", "{0} · {1}: {2}", name.arg(), field.arg(), value.arg())
    }

    pub fn describe_trigger(&self, t: &Trigger) -> Text {
        let with_days = |text: Text, days: &[u32]| {
            if days.is_empty() || days.len() == 7 {
                return text;
            }
            let days = day_list(days);
            crate::text!("{0} ({1})", "{0} ({1})", "{0} ({1})", text.arg(), days.arg())
        };
        let lasting = |text: Text, minutes: i64| {
            if minutes > 0 {
                crate::text!("{0} pendant {1} min", "{0} for {1} min", "{0} durante {1} min", text.arg(), minutes)
            } else {
                text
            }
        };
        match t {
            Trigger::Measure { measure, op, value, for_minutes } => lasting(self.comparison(measure, *op, *value), *for_minutes),
            Trigger::State { device_id, field, value, for_minutes } => {
                let state = self.state_text(*device_id, field, value);
                lasting(crate::text!("{0} devient vrai", "{0} becomes true", "{0} pasa a ser cierto", state.arg()), *for_minutes)
            }
            Trigger::Time { time, days } => with_days(crate::text!("à {0}", "at {0}", "a las {0}", time), days),
            Trigger::Sun { event, offset_minutes, days } => {
                // "au lever du soleil", "30 min avant le lever du soleil"; "al amanecer", "antes del amanecer".
                let (at, of) = match event {
                    SunEvent::Sunrise => (crate::text!("au lever du soleil", "at sunrise", "al amanecer"), crate::text!("le lever du soleil", "sunrise", "del amanecer")),
                    SunEvent::Sunset => (crate::text!("au coucher du soleil", "at sunset", "al atardecer"), crate::text!("le coucher du soleil", "sunset", "del atardecer")),
                    SunEvent::Dawn => (crate::text!("à l'aube", "at dawn", "al alba"), crate::text!("l'aube", "dawn", "del alba")),
                    SunEvent::Dusk => (crate::text!("au crépuscule", "at dusk", "al anochecer"), crate::text!("le crépuscule", "dusk", "del anochecer")),
                };
                let n = offset_minutes.abs();
                let text = match offset_minutes.signum() {
                    0 => at,
                    -1 => crate::text!("{0} min avant {1}", "{0} min before {1}", "{0} min antes {1}", n, of.arg()),
                    _ => crate::text!("{0} min après {1}", "{0} min after {1}", "{0} min después {1}", n, of.arg()),
                };
                with_days(text, days)
            }
            Trigger::Facade { facing: f } => {
                let f = facing(*f);
                crate::text!("quand le soleil arrive sur la façade {0}", "when the sun reaches the {0} façade", "cuando el sol llega a la fachada {0}", f.arg())
            }
            Trigger::Presence { home: true } => crate::text!("quand le premier arrive à la maison", "when the first one gets home", "cuando llega el primero a casa"),
            Trigger::Presence { home: false } => crate::text!("quand le dernier part de la maison", "when the last one leaves home", "cuando se va el último de casa"),
            Trigger::Button { device_id, button, press } => {
                let name = self.device_name(*device_id);
                let how = press_name(press.as_deref().unwrap_or("single"));
                match (button, press) {
                    (Some(b), Some(_)) => crate::text!("{0} : bouton {1}, {2}", "{0}: button {1}, {2}", "{0}: botón {1}, {2}", name.arg(), b, how.arg()),
                    (Some(b), None) => crate::text!("{0} : bouton {1}", "{0}: button {1}", "{0}: botón {1}", name.arg(), b),
                    (None, Some(_)) => crate::text!("{0} : {1}", "{0}: {1}", "{0}: {1}", name.arg(), how.arg()),
                    (None, None) => crate::text!("{0} : un bouton", "{0}: a button", "{0}: un botón", name.arg()),
                }
            }
        }
    }

    /// The triggers, joined by "ou".
    pub fn describe_triggers(&self, triggers: &[Trigger]) -> Text {
        let parts: Vec<Text> = triggers.iter().map(|t| self.describe_trigger(t)).collect();
        let word = Match::Any.word();
        Text::build(|l| parts.iter().map(|p| p.get(l)).collect::<Vec<_>>().join(&format!(" {} ", word.get(l))))
    }

    pub fn describe_condition(&self, c: &Condition) -> Text {
        match c {
            Condition::Measure { measure, op, value } => self.comparison(measure, *op, *value),
            Condition::State { device_id, field, value } => self.state_text(*device_id, field, value),
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
            Condition::Facade { facing: f } => {
                let f = facing(*f);
                crate::text!("le soleil est sur la façade {0}", "the sun is on the {0} façade", "el sol da en la fachada {0}", f.arg())
            }
            Condition::Presence { home: true } => crate::text!("quelqu'un est à la maison", "someone is home", "hay alguien en casa"),
            Condition::Presence { home: false } => crate::text!("personne n'est à la maison", "nobody is home", "no hay nadie en casa"),
            Condition::Group { joined, conditions } => {
                let inner = self.describe_conditions(*joined, conditions);
                crate::text!("({0})", "({0})", "({0})", inner.arg())
            }
        }
    }

    /// "A et B", "A ou B ou C".
    pub fn describe_conditions(&self, joined: Match, conditions: &[Condition]) -> Text {
        let word = joined.word();
        let parts: Vec<Text> = conditions.iter().map(|c| self.describe_condition(c)).collect();
        Text::build(|l| parts.iter().map(|p| p.get(l)).collect::<Vec<_>>().join(&format!(" {} ", word.get(l))))
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
            Step::Wait { seconds } => crate::text!("attendre {0}", "wait {0}", "esperar {0}", duration(*seconds)),
            Step::WaitUntil { condition, for_seconds, timeout_seconds, on_timeout } => {
                let what = self.describe_condition(condition);
                let what = if *for_seconds > 0 { crate::text!("{0} pendant {1}", "{0} for {1}", "{0} durante {1}", what.arg(), duration(*for_seconds)) } else { what };
                let max = duration(*timeout_seconds);
                match on_timeout {
                    OnTimeout::Continue => crate::text!("attendre que {0} (au plus {1})", "wait until {0} ({1} at most)", "esperar a que {0} (como máximo {1})", what.arg(), max),
                    OnTimeout::Stop => crate::text!(
                        "attendre que {0} (au plus {1}, sinon s'arrêter)",
                        "wait until {0} ({1} at most, else stop)",
                        "esperar a que {0} (como máximo {1}; si no, parar)",
                        what.arg(),
                        max
                    ),
                }
            }
            Step::Restore => crate::text!("remettre comme avant", "put back as before", "dejar como antes"),
            Step::Notify { message } => crate::text!("prévenir : « {0} »", "notify: “{0}”", "avisar: «{0}»", message),
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

    /// What was true when a trigger started the rule: "Injection au réseau : 742 W depuis 5 min".
    fn reason(&self, t: &Trigger) -> Text {
        match t {
            Trigger::Measure { measure, for_minutes, .. } => {
                let name = self.measure_label(measure);
                let mut value = self.read(measure).map_or_else(|| Text::plain("?"), |v| self.value_text(measure, v));
                if let (Measure::Weather { field: WeatherField::Vigilance }, Some(v)) = (measure, self.weather.as_ref().and_then(|w| w.vigilance.as_ref())) {
                    if !v.events.is_empty() {
                        let events = v.events.join(", ");
                        value = crate::text!("{0} ({1})", "{0} ({1})", "{0} ({1})", value.arg(), events);
                    }
                }
                let value = value.arg();
                if *for_minutes > 0 {
                    crate::text!("{0} : {1} depuis {2} min", "{0}: {1} for {2} min", "{0}: {1} desde hace {2} min", name.arg(), value, for_minutes)
                } else {
                    crate::text!("{0} : {1}", "{0}: {1}", "{0}: {1}", name.arg(), value)
                }
            }
            Trigger::State { device_id, field, value, .. } => self.state_text(*device_id, field, value),
            other => self.describe_trigger(other),
        }
    }

    /// The conditions that are not true now, with what they read: "Salon · Température < 18 °C
    /// (19,1 °C)".
    fn why_not(&self, rule: &Automation) -> Text {
        let parts: Vec<Text> = rule
            .conditions
            .iter()
            .filter_map(|c| {
                let met = c.met(self);
                if met == Some(true) {
                    return None;
                }
                let text = self.describe_condition(c);
                Some(match (met, c) {
                    (None, _) => crate::text!("{0} : inconnu", "{0}: unknown", "{0}: desconocido", text.arg()),
                    (_, Condition::Measure { measure, .. }) => match self.read(measure) {
                        Some(v) => {
                            let now = self.value_text(measure, v);
                            crate::text!("{0} ({1})", "{0} ({1})", "{0} ({1})", text.arg(), now.arg())
                        }
                        None => text,
                    },
                    _ => text,
                })
            })
            .collect();
        let list = join(&parts, ", ");
        crate::text!("conditions non remplies : {0}", "conditions not met: {0}", "condiciones no cumplidas: {0}", list.arg())
    }
}

// ---------------------------------------------------------------------------------------
// Views and validation
// ---------------------------------------------------------------------------------------

/// The rule as a sentence: "Quand {when}, si {conditions}, alors {actions}, sinon {else}".
#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    /// The triggers, joined by "ou".
    pub when: Text,
    pub triggers: Vec<Text>,
    /// One per condition (a group in brackets), to be joined by `joined`.
    pub conditions: Vec<Text>,
    /// "et" or "ou", in the language of the request.
    pub joined: Text,
    pub actions: Vec<Text>,
    #[serde(rename = "else")]
    pub otherwise: Vec<Text>,
    pub over: Vec<Text>,
}

/// What the rule sees now.
#[derive(Debug, Clone, Serialize)]
pub struct Status {
    /// The measure of the first measure trigger, now.
    pub value: Option<f64>,
    pub unit: Option<String>,
    /// That measure is past its threshold.
    pub holds: Option<bool>,
    /// Each condition: true, false, or unknown.
    pub conditions: Vec<Option<bool>>,
    /// Next run of a time or sun trigger.
    pub next_at: Option<i64>,
    /// A run is under way (it waits).
    pub running: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AutomationView {
    pub id: i64,
    pub name: String,
    pub enabled: bool,
    /// The first trigger (for the clients of 0.11.0-beta.18).
    pub when: Trigger,
    pub triggers: Vec<Trigger>,
    #[serde(rename = "match")]
    pub joined: Match,
    pub conditions: Vec<Condition>,
    pub actions: Vec<Step>,
    #[serde(rename = "else")]
    pub otherwise: Vec<Step>,
    pub mode: RunMode,
    pub over: Vec<Step>,
    pub summary: Summary,
    pub status: Status,
    /// Enabled rules that may set the same device otherwise at the same time.
    pub conflicts: Vec<Conflict>,
    pub last_run_at: Option<i64>,
    pub last_reason: Option<Text>,
    pub created_at: i64,
    pub updated_at: i64,
}

pub fn view(row: &AutomationRow, rule: Automation, ctx: &Context, running: bool) -> AutomationView {
    let measured = rule.triggers.iter().find_map(|t| match t {
        Trigger::Measure { measure, op, value, .. } => Some((measure, op, value)),
        _ => None,
    });
    let (value, unit, holds) = match measured {
        Some((measure, op, threshold)) => {
            let read = ctx.read(measure);
            (read, ctx.unit(measure), read.map(|v| op.holds(v, *threshold)))
        }
        None => (None, None, None),
    };
    AutomationView {
        id: row.id,
        name: row.name.clone(),
        enabled: row.enabled,
        summary: Summary {
            when: ctx.describe_triggers(&rule.triggers),
            triggers: rule.triggers.iter().map(|t| ctx.describe_trigger(t)).collect(),
            conditions: rule.conditions.iter().map(|c| ctx.describe_condition(c)).collect(),
            joined: rule.joined.word(),
            actions: rule.actions.iter().map(|s| ctx.describe_step(s)).collect(),
            otherwise: rule.otherwise.iter().map(|s| ctx.describe_step(s)).collect(),
            over: rule.over.iter().map(|s| ctx.describe_step(s)).collect(),
        },
        status: Status {
            value,
            unit,
            holds,
            conditions: rule.conditions.iter().map(|c| c.met(ctx)).collect(),
            next_at: rule.triggers.iter().filter_map(|t| t.next_at(ctx)).min(),
            running,
        },
        when: rule.triggers[0].clone(),
        triggers: rule.triggers,
        joined: rule.joined,
        conditions: rule.conditions,
        actions: rule.actions,
        otherwise: rule.otherwise,
        mode: rule.mode,
        over: rule.over,
        conflicts: vec![],
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

fn check_lasting(minutes: i64) -> ApiResult<()> {
    if (0..=MAX_FOR_MINUTES).contains(&minutes) {
        Ok(())
    } else {
        Err(ApiError::bad_request(crate::text!("durée entre 0 minute et 24 heures", "duration between 0 minutes and 24 hours", "duración entre 0 minutos y 24 horas")))
    }
}

fn check_facing(facing: u32) -> ApiResult<()> {
    if facing < 360 {
        Ok(())
    } else {
        Err(ApiError::bad_request(crate::text!("orientation de 0 à 359°", "facing from 0 to 359°", "orientación de 0 a 359°")))
    }
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

    fn check_state(&self, device_id: i64, field: &str, value: &Value) -> ApiResult<()> {
        self.known(device_id)?;
        let ok = self.field(device_id, field).is_some_and(|f| match f.kind.as_str() {
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

    fn check_trigger(&self, t: &Trigger) -> ApiResult<()> {
        match t {
            Trigger::Measure { measure, value, for_minutes, .. } => {
                check_lasting(*for_minutes)?;
                self.check_measure(measure, *value)
            }
            Trigger::State { device_id, field, value, for_minutes } => {
                check_lasting(*for_minutes)?;
                self.check_state(*device_id, field, value)
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
            Trigger::Facade { facing } => check_facing(*facing),
            Trigger::Presence { .. } => Ok(()),
            Trigger::Button { device_id, button, press } => {
                let device = self.known(*device_id)?;
                if !device.traits.iter().any(|t| t.starts_with("button@")) {
                    return Err(ApiError::bad_request(crate::text!("cet appareil n'a pas de boutons", "this device has no buttons", "este dispositivo no tiene botones")));
                }
                let buttons = device.state.get("buttons").and_then(Value::as_u64).unwrap_or(u64::MAX);
                if button.is_some_and(|b| b == 0 || b as u64 > buttons) || press.as_deref().is_some_and(|p| !PRESSES.contains(&p)) {
                    return Err(ApiError::bad_request(crate::text!("bouton ou appui inconnu", "unknown button or press", "botón o pulsación desconocidos")));
                }
                Ok(())
            }
        }
    }

    fn check_condition(&self, c: &Condition) -> ApiResult<()> {
        match c {
            Condition::Measure { measure, value, .. } => self.check_measure(measure, *value),
            Condition::State { device_id, field, value } => self.check_state(*device_id, field, value),
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
            Condition::Daylight { .. } | Condition::Presence { .. } => Ok(()),
            Condition::Facade { facing } => check_facing(*facing),
            Condition::Group { conditions, .. } => {
                if conditions.len() < 2 {
                    return Err(ApiError::bad_request(crate::text!(
                        "un groupe réunit au moins deux conditions",
                        "a group joins at least two conditions",
                        "un grupo reúne al menos dos condiciones"
                    )));
                }
                if conditions.iter().any(|c| matches!(c, Condition::Group { .. })) {
                    return Err(ApiError::bad_request(crate::text!("pas de groupe dans un groupe", "no group within a group", "ningún grupo dentro de un grupo")));
                }
                conditions.iter().try_for_each(|c| self.check_condition(c))
            }
        }
    }

    fn check_wait(seconds: i64, min: i64) -> ApiResult<()> {
        if (min..=MAX_WAIT).contains(&seconds) {
            Ok(())
        } else {
            Err(ApiError::bad_request(crate::text!("attente de 24 heures au plus", "a wait of 24 hours at most", "espera de 24 horas como máximo")))
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
            Step::Wait { seconds } => Self::check_wait(*seconds, 1),
            Step::WaitUntil { condition, for_seconds, timeout_seconds, .. } => {
                Self::check_wait(*timeout_seconds, 1)?;
                Self::check_wait(*for_seconds, 0)?;
                self.check_condition(condition)
            }
            Step::Restore => Ok(()),
            Step::Notify { message } => {
                let n = message.trim().chars().count();
                if n == 0 || n > MAX_MESSAGE {
                    return Err(ApiError::bad_request(crate::text!(
                        "message de 1 à {0} caractères",
                        "a message of 1 to {0} characters",
                        "mensaje de 1 a {0} caracteres",
                        MAX_MESSAGE
                    )));
                }
                Ok(())
            }
        }
    }

    /// `before`: the steps run before these in the same run (what "put back" can undo).
    fn check_branch(&self, steps: &[Step], required: bool, before: &[Step]) -> ApiResult<()> {
        if steps.len() > MAX_ACTIONS || (required && steps.is_empty()) {
            return Err(ApiError::bad_request(crate::text!("de 1 à {0} actions", "1 to {0} actions", "de 1 a {0} acciones", MAX_ACTIONS)));
        }
        for (i, s) in steps.iter().enumerate() {
            self.check_step(s)?;
            if *s == Step::Restore && !before.iter().chain(&steps[..i]).any(|p| matches!(p, Step::Device { .. })) {
                return Err(ApiError::bad_request(crate::text!(
                    "« remettre comme avant » vient après une action sur un appareil",
                    "“put back as before” comes after an action on a device",
                    "«dejar como antes» va después de una acción sobre un dispositivo"
                )));
            }
        }
        Ok(())
    }
}

/// Checks a rule against the devices of the home; returns its name and the rule to store.
pub fn validate(input: AutomationInput, ctx: &Context) -> ApiResult<(String, Automation)> {
    let name = input.name.trim().to_string();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(ApiError::bad_request("name must be 1-80 characters"));
    }
    let triggers: Vec<Trigger> = input.when.into_iter().chain(input.triggers).collect();
    if triggers.is_empty() || triggers.len() > MAX_TRIGGERS {
        return Err(ApiError::bad_request(crate::text!("de 1 à {0} déclencheurs", "1 to {0} triggers", "de 1 a {0} disparadores", MAX_TRIGGERS)));
    }
    let leaves: usize = input.conditions.iter().map(|c| if let Condition::Group { conditions, .. } = c { conditions.len() } else { 1 }).sum();
    if leaves > MAX_CONDITIONS {
        return Err(ApiError::bad_request(crate::text!("{0} conditions au plus", "{0} conditions at most", "{0} condiciones como máximo", MAX_CONDITIONS)));
    }
    triggers.iter().try_for_each(|t| ctx.check_trigger(t))?;
    input.conditions.iter().try_for_each(|c| ctx.check_condition(c))?;
    ctx.check_branch(&input.actions, true, &[])?;
    ctx.check_branch(&input.otherwise, false, &[])?;
    ctx.check_branch(&input.over, false, &input.actions)?;
    if !input.over.is_empty() && !triggers.iter().any(Trigger::lasts) {
        return Err(ApiError::bad_request(crate::text!(
            "« quand ce n'est plus vrai » demande un déclencheur qui dure : une mesure, un état, le soleil sur une façade ou la présence",
            "“when it is no longer true” needs a trigger that lasts: a measure, a state, the sun on a façade or presence",
            "«cuando deja de ser cierto» necesita un disparador que dure: una medida, un estado, el sol en una fachada o la presencia"
        )));
    }
    let rule = Automation { triggers, joined: input.joined, conditions: input.conditions, actions: input.actions, otherwise: input.otherwise, mode: input.mode, over: input.over };
    Ok((name, rule))
}

// ---------------------------------------------------------------------------------------
// Rules that contradict each other
// ---------------------------------------------------------------------------------------

/// Two triggers may start their rules at the same time.
fn coincide(a: &Trigger, b: &Trigger) -> bool {
    let days = |x: &[u32], y: &[u32]| x.is_empty() || y.is_empty() || x.iter().any(|d| y.contains(d));
    match (a, b) {
        (Trigger::Time { time: t1, days: d1 }, Trigger::Time { time: t2, days: d2 }) => t1 == t2 && days(d1, d2),
        (Trigger::Sun { event: e1, offset_minutes: o1, days: d1 }, Trigger::Sun { event: e2, offset_minutes: o2, days: d2 }) => e1 == e2 && o1 == o2 && days(d1, d2),
        (Trigger::Measure { measure: m1, op: o1, .. }, Trigger::Measure { measure: m2, op: o2, .. }) => m1 == m2 && o1 == o2,
        (Trigger::Button { device_id: d1, button: b1, .. }, Trigger::Button { device_id: d2, button: b2, .. }) => d1 == d2 && (b1.is_none() || b2.is_none() || b1 == b2),
        _ => a == b,
    }
}

/// A setting both rules give a device, with different values: (device, setting).
fn clash(a: &[Step], b: &[Step]) -> Option<(i64, String)> {
    for x in a {
        let Step::Device { device_id, trait_name, action, params } = x else { continue };
        for y in b {
            let Step::Device { device_id: d2, trait_name: t2, action: a2, params: p2 } = y else { continue };
            if device_id != d2 || trait_name != t2 {
                continue;
            }
            if action != a2 && matches!((action.as_str(), a2.as_str()), ("start", "stop") | ("stop", "start") | ("boost", "stop_boost") | ("stop_boost", "boost")) {
                return Some((*device_id, action.clone()));
            }
            if action == a2 {
                if let Some(k) = params.keys().find(|k| p2.get(*k).is_some_and(|v| Some(v) != params.get(*k))) {
                    return Some((*device_id, k.clone()));
                }
            }
        }
    }
    None
}

/// A rule that may contradict another one.
#[derive(Debug, Clone, Serialize)]
pub struct Conflict {
    pub id: i64,
    pub name: String,
    /// "Salon (consigne)".
    pub what: Text,
}

/// For each enabled rule, the enabled ones that may set the same device otherwise at the same time.
pub fn conflicts(rules: &[(AutomationRow, Automation)], ctx: &Context) -> HashMap<i64, Vec<Conflict>> {
    let mut out: HashMap<i64, Vec<Conflict>> = HashMap::new();
    let enabled: Vec<&(AutomationRow, Automation)> = rules.iter().filter(|(r, _)| r.enabled).collect();
    for (i, (ra, a)) in enabled.iter().enumerate() {
        for (rb, b) in enabled.iter().skip(i + 1) {
            if !a.triggers.iter().any(|x| b.triggers.iter().any(|y| coincide(x, y))) {
                continue;
            }
            let (sa, sb): (Vec<Step>, Vec<Step>) = (a.steps().cloned().collect(), b.steps().cloned().collect());
            let Some((device, setting)) = clash(&sa, &sb) else { continue };
            let name = ctx.device_name(device);
            let setting = ctx.device(device).and_then(|d| {
                d.traits.iter().find_map(|t| {
                    let (n, v) = t.split_once('@')?;
                    crate::traits::get(n, v.parse().ok()?)?.actions.iter().flat_map(|a| &a.params).find(|p| p.name == setting).map(|p| label(&p.label))
                })
            });
            let what = match setting {
                Some(s) => crate::text!("{0} ({1})", "{0} ({1})", "{0} ({1})", name.arg(), s.arg()),
                None => name,
            };
            out.entry(ra.id).or_default().push(Conflict { id: rb.id, name: rb.name.clone(), what: what.clone() });
            out.entry(rb.id).or_default().push(Conflict { id: ra.id, name: ra.name.clone(), what });
        }
    }
    out
}

// ---------------------------------------------------------------------------------------
// Engine: which rules start
// ---------------------------------------------------------------------------------------

/// What is remembered of a trigger that holds, between two looks.
#[derive(Debug, Clone, Copy)]
struct Watch {
    /// The rule it was for (an edited rule starts again).
    updated_at: i64,
    /// Since when it holds.
    since: Option<i64>,
    /// The rule started for this episode.
    fired: bool,
    /// Seen at least once (a state or a façade must be seen changing).
    seen: bool,
    /// The history already tells why the rule did not start in this episode.
    skipped: bool,
}

/// Which actions run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Branch {
    Then,
    Else,
    /// A trigger started the rule, but its conditions were false and it has no "else".
    Skipped,
}

impl Branch {
    fn as_str(self) -> &'static str {
        match self {
            Branch::Then => "then",
            Branch::Else => "else",
            Branch::Skipped => "skipped",
        }
    }
}

/// A rule a trigger started.
#[derive(Debug, Clone)]
pub struct Due {
    pub id: i64,
    pub branch: Branch,
    pub reason: Text,
    /// The trigger that holds and started it (its end runs the "over" steps).
    pub lasting: Option<usize>,
}

/// Decides which rules start. Kept in memory: after a restart, a measure already past its
/// threshold counts as a new episode.
#[derive(Debug)]
pub struct Engine {
    watches: HashMap<(i64, usize), Watch>,
    /// When the rules were last looked at (time and sun triggers start once between two looks).
    checked: i64,
}

impl Engine {
    pub fn new(checked: i64) -> Self {
        Self { watches: HashMap::new(), checked }
    }

    /// The rules to start now, each with its branch and what started it.
    pub fn due(&mut self, rules: &[(AutomationRow, Automation)], ctx: &Context) -> Vec<Due> {
        let mut out = vec![];
        self.watches.retain(|(id, _), _| rules.iter().any(|(row, _)| row.id == *id && row.enabled));
        for (row, rule) in rules.iter().filter(|(row, _)| row.enabled) {
            // Its sun and its weather are those of the installation it is about.
            let here = ctx.at(rule.place(ctx));
            let ctx = &here;
            // The triggers that start the rule now: (index, holds for a while).
            let mut started: Vec<(usize, bool)> = vec![];
            for (i, t) in rule.triggers.iter().enumerate() {
                let fresh = Watch { updated_at: row.updated_at, since: None, fired: false, seen: false, skipped: false };
                let w = self.watches.entry((row.id, i)).or_insert(fresh);
                if w.updated_at != row.updated_at {
                    *w = fresh;
                }
                // What cannot be read changes nothing.
                match t.look(ctx, self.checked) {
                    None => {}
                    Some(Look::Event(happened)) => {
                        if happened {
                            started.push((i, false));
                        }
                    }
                    Some(Look::Holds { holds, released, edge, for_minutes }) => {
                        if !w.seen {
                            w.seen = true;
                            // Already so when first seen: not a change.
                            w.fired = edge && holds;
                        }
                        if w.fired {
                            if released {
                                *w = Watch { seen: true, ..fresh };
                            }
                            continue;
                        }
                        if !holds {
                            w.since = None;
                            w.skipped = false;
                            continue;
                        }
                        let since = *w.since.get_or_insert(ctx.now);
                        if ctx.now - since >= for_minutes * 60 {
                            started.push((i, true));
                        }
                    }
                }
            }
            let Some(&(first, _)) = started.first() else { continue };
            if row.last_run_at.is_some_and(|t| ctx.now - t < MIN_GAP) {
                continue;
            }
            let reason = ctx.reason(&rule.triggers[first]);
            let branch = match rule.met(ctx) {
                Some(true) => Branch::Then,
                Some(false) if !rule.otherwise.is_empty() => Branch::Else,
                // A trigger that holds tries the conditions again while it holds: the history
                // tells once why the rule did not start.
                met => {
                    let holding: Vec<usize> = started.iter().filter(|(_, h)| *h).map(|(i, _)| *i).collect();
                    let told = !holding.is_empty() && holding.iter().all(|i| self.watches.get(&(row.id, *i)).is_some_and(|w| w.skipped));
                    for i in &holding {
                        if let Some(w) = self.watches.get_mut(&(row.id, *i)) {
                            w.skipped = true;
                        }
                    }
                    let waits = met.is_none() && !holding.is_empty();
                    if !told && !waits {
                        let why = ctx.why_not(rule);
                        out.push(Due {
                            id: row.id, branch: Branch::Skipped, reason: crate::text!("{0} — {1}", "{0} — {1}", "{0} — {1}", reason.arg(), why.arg()), lasting: None
                        });
                    }
                    continue;
                }
            };
            for (i, holding) in &started {
                if *holding {
                    if let Some(w) = self.watches.get_mut(&(row.id, *i)) {
                        w.fired = true;
                    }
                }
            }
            let lasting = started.iter().find(|(_, holding)| *holding).map(|(i, _)| *i);
            out.push(Due { id: row.id, branch, reason, lasting });
        }
        self.checked = ctx.now;
        out
    }
}

// ---------------------------------------------------------------------------------------
// Runs
// ---------------------------------------------------------------------------------------

fn text_json(t: &Text) -> Value {
    json!({ "fr": t.fr, "en": t.en, "es": t.es })
}

fn text_of(v: &Value) -> Text {
    Text::load(&v.to_string())
}

/// A step done, as the history keeps it.
#[derive(Debug, Clone, Serialize)]
pub struct StepDone {
    pub what: Text,
    pub ok: bool,
    pub error: Option<Text>,
}

impl StepDone {
    fn stored(&self) -> Value {
        json!({ "what": text_json(&self.what), "ok": self.ok, "error": self.error.as_ref().map(text_json) })
    }

    fn load(v: &Value) -> StepDone {
        StepDone { what: text_of(&v["what"]), ok: v["ok"].as_bool().unwrap_or(false), error: v.get("error").filter(|e| !e.is_null()).map(text_of) }
    }
}

/// A run in the history.
#[derive(Debug, Clone, Serialize)]
pub struct RunView {
    pub started_at: i64,
    pub finished_at: Option<i64>,
    /// "then" | "else" | "skipped" | "busy"
    pub branch: String,
    pub reason: Text,
    /// "done" | "stopped" | "cancelled" | "interrupted"; `None` while it runs.
    pub ended: Option<String>,
    pub steps: Vec<StepDone>,
}

impl RunView {
    pub fn of(r: db::AutomationRun) -> RunView {
        let steps: Vec<Value> = serde_json::from_str(&r.steps).unwrap_or_default();
        RunView {
            started_at: r.started_at,
            finished_at: r.finished_at,
            branch: r.branch,
            reason: Text::load(&r.reason),
            ended: r.ended,
            steps: steps.iter().map(StepDone::load).collect(),
        }
    }
}

/// The runs under way, by rule.
#[derive(Default)]
pub struct Runs {
    running: Mutex<HashMap<i64, (u64, tokio::task::AbortHandle, i64)>>,
    next: AtomicU64,
}

impl Runs {
    pub fn is_running(&self, id: i64) -> bool {
        self.running.lock().unwrap_or_else(|e| e.into_inner()).contains_key(&id)
    }

    /// Ends the run of a rule (changed, switched off, deleted); its history says it was cancelled.
    pub async fn cancel(&self, db: &db::Db, id: i64) {
        let taken = self.running.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
        if let Some((_, handle, run_id)) = taken {
            handle.abort();
            if let Err(e) = db::finish_automation_run_ended(db, run_id, "cancelled").await {
                tracing::error!("automation {id}: {e}");
            }
        }
    }
}

/// Starts the actions of a branch in the background. `None` when the rule already runs and is set
/// to ignore a new start (the history tells it).
pub async fn start(state: &AppState, row: &AutomationRow, rule: &Automation, due: Due, ctx: Context, actor: &str) -> Option<tokio::task::JoinHandle<Vec<StepDone>>> {
    let Due { branch, reason, lasting, .. } = due;
    let runs = state.automations.clone();
    if runs.is_running(row.id) {
        match rule.mode {
            RunMode::Single => {
                let busy = crate::text!("{0} — déjà en cours", "{0} — already running", "{0} — ya en curso", reason.arg());
                if let Err(e) = db::insert_automation_run(&state.db, row.id, "busy", &busy.store(), true).await {
                    tracing::error!("automation {}: {e}", row.id);
                }
                return None;
            }
            RunMode::Restart => {
                let old = runs.running.lock().unwrap_or_else(|e| e.into_inner()).remove(&row.id);
                if let Some((_, handle, run_id)) = old {
                    handle.abort();
                    if let Err(e) = db::finish_automation_run_ended(&state.db, run_id, "cancelled").await {
                        tracing::error!("automation {}: {e}", row.id);
                    }
                }
            }
        }
    }
    let run_id = match db::insert_automation_run(&state.db, row.id, branch.as_str(), &reason.store(), false).await {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("automation {}: {e}", row.id);
            return None;
        }
    };
    if let Err(e) = db::set_automation_run(&state.db, row.id, db::now(), &reason.store()).await {
        tracing::error!("automation {}: {e}", row.id);
    }
    let place = rule.place(&ctx);
    let steps = if branch == Branch::Else { rule.otherwise.clone() } else { rule.actions.clone() };
    // Only a lasting trigger that started the "then" steps tells when it is over.
    let until = lasting.filter(|_| branch == Branch::Then && !rule.over.is_empty()).and_then(|i| rule.triggers.get(i).cloned());
    let over = rule.over.clone();
    let generation = runs.next.fetch_add(1, Ordering::Relaxed);
    let (state2, row2, actor2) = (state.clone(), row.clone(), actor.to_string());
    // Registered before it starts, so that a run that ends at once removes itself.
    let mut running = runs.running.lock().unwrap_or_else(|e| e.into_inner());
    let task = tokio::spawn(async move {
        let mut p = Progress { run_id, place, out: vec![], touched: vec![] };
        let here = ctx.at(place);
        let ended = match until {
            None => execute(&state2, &row2, &steps, here, &actor2, &mut p).await,
            Some(trigger) => {
                let gone = ended_holding(&state2, &trigger, place);
                tokio::pin!(gone);
                let first = tokio::select! {
                    e = execute(&state2, &row2, &steps, here.clone(), &actor2, &mut p) => Some(e),
                    () = &mut gone => None,
                };
                match first {
                    Some("stopped") => "stopped",
                    finished => {
                        if finished.is_some() {
                            gone.await;
                        }
                        let what = here.describe_trigger(&trigger);
                        let what = if finished.is_some() {
                            crate::text!("ce n'est plus vrai : {0}", "no longer true: {0}", "ya no es cierto: {0}", what.arg())
                        } else {
                            crate::text!(
                                "ce n'est plus vrai : {0} — la suite est arrêtée",
                                "no longer true: {0} — the rest is stopped",
                                "ya no es cierto: {0} — el resto se detiene",
                                what.arg()
                            )
                        };
                        p.out.push(StepDone { what, ok: true, error: None });
                        let now = Context::load(&state2).await.map(|c| c.at(place)).unwrap_or(here);
                        execute(&state2, &row2, &over, now, &actor2, &mut p).await
                    }
                }
            }
        };
        let done = p.out;
        let stored = Value::Array(done.iter().map(StepDone::stored).collect()).to_string();
        if let Err(e) = db::finish_automation_run(&state2.db, run_id, ended, &stored).await {
            tracing::error!("automation {}: {e}", row2.id);
        }
        {
            let mut running = state2.automations.running.lock().unwrap_or_else(|e| e.into_inner());
            if running.get(&row2.id).is_some_and(|(g, _, _)| *g == generation) {
                running.remove(&row2.id);
            }
        }
        log_run(&state2, &row2, branch, &reason, &done, ended, &actor2).await;
        state2.hub.routines_changed();
        done
    });
    running.insert(row.id, (generation, task.abort_handle(), run_id));
    drop(running);
    state.hub.routines_changed();
    Some(task)
}

async fn log_run(state: &AppState, row: &AutomationRow, branch: Branch, reason: &Text, done: &[StepDone], ended: &str, actor: &str) {
    let failed = done.iter().filter(|s| !s.ok).count();
    let what = match branch {
        Branch::Else => crate::text!("{0} (sinon)", "{0} (else)", "{0} (si no)", reason.arg()),
        _ => reason.clone(),
    };
    let (level, message) = if failed == 0 && ended == "done" {
        ("info", crate::text!("Automatisme « {0} » : {1}", "Automation “{0}”: {1}", "Automatismo «{0}»: {1}", row.name, what.arg()))
    } else if failed == 0 {
        (
            "info",
            crate::text!(
                "Automatisme « {0} » : {1} — arrêté après une attente",
                "Automation “{0}”: {1} — stopped after a wait",
                "Automatismo «{0}»: {1} — detenido tras una espera",
                row.name,
                what.arg()
            ),
        )
    } else {
        (
            "warn",
            crate::text!(
                "Automatisme « {0} » : {1} — {2} échec(s) sur {3}",
                "Automation “{0}”: {1} — {2} failure(s) out of {3}",
                "Automatismo «{0}»: {1} — {2} fallo(s) de {3}",
                row.name,
                what.arg(),
                failed,
                done.len()
            ),
        )
    };
    let data = json!({ "automation_id": row.id, "results": done });
    state.hub.log("automation", level, None, Some(actor), message, Some(data)).await;
}

/// A device a run drove, and its state before.
struct Touched {
    device_id: i64,
    trait_name: String,
    /// The last action on it, and every setting the run changed.
    action: String,
    keys: Vec<String>,
    before: Map<String, Value>,
    /// The last value the run gave to each setting.
    set: Map<String, Value>,
}

/// Settings someone changed since the run set them: neither what the run gave nor what was there
/// before (a device that has not caught up yet, or that was put back by hand, still counts as ours).
fn changed_since(t: &Touched, now: &Map<String, Value>) -> Vec<String> {
    let same = |a: &Value, b: &Value| match (a.as_f64(), b.as_f64()) {
        // A setpoint comes back rounded to the device's step (0.5 °C on Airzone).
        (Some(x), Some(y)) => (x - y).abs() < 0.3,
        _ => a == b,
    };
    t.set
        .iter()
        .filter(|(k, v)| match now.get(*k).filter(|n| !n.is_null()) {
            Some(n) => !same(n, v) && t.before.get(*k).is_none_or(|b| !same(n, b)),
            None => false,
        })
        .map(|(k, _)| k.clone())
        .collect()
}

/// The action that puts back `before` after `action` set `keys` (`None`: nothing to put back).
fn undo(trait_name: &str, action: &str, keys: &[String], before: &Map<String, Value>) -> Option<(String, Map<String, Value>)> {
    let get = |k: &str| before.get(k).filter(|v| !v.is_null());
    match (trait_name, action) {
        ("ev.charger", "start" | "stop") => Some((if get("status").and_then(Value::as_str) == Some("charging") { "start" } else { "stop" }.into(), Map::new())),
        ("ev.charger", "set_auto_limit") => Some(("set_auto_limit".into(), Map::from_iter([("on".into(), get("auto_limit")?.clone())]))),
        (_, "boost") => (get("boost").and_then(Value::as_bool) == Some(false)).then(|| ("stop_boost".into(), Map::new())),
        (_, "stop" | "stop_boost") => None,
        _ => {
            let params: Map<String, Value> = keys.iter().filter_map(|k| Some((k.clone(), get(k)?.clone()))).collect();
            (!params.is_empty()).then(|| (action.to_string(), params))
        }
    }
}

async fn apply(state: &AppState, ctx: &Context, device_id: i64, input: &ActionInput, actor: &str) -> ApiResult<()> {
    let device = db::device_by_id(&state.db, device_id).await?.ok_or_else(|| ApiError::not_found("device not found"))?;
    let view = ctx.device(device_id).ok_or_else(|| ApiError::not_found("device not found"))?;
    home::check_action(view, input)?;
    home::apply(state, &device, view, input, actor).await
}

fn done(what: Text, r: ApiResult<()>) -> StepDone {
    StepDone { what, ok: r.is_ok(), error: r.err().map(|e| e.message) }
}

/// Returns once `trigger` no longer holds: past its threshold back by the margin (a measure), or
/// changed (a state, the sun on a façade, presence), for as long as it had to hold to start.
async fn ended_holding(state: &AppState, trigger: &Trigger, place: Option<i64>) {
    let mut since: Option<i64> = None;
    loop {
        tokio::time::sleep(POLL).await;
        let Ok(ctx) = Context::load(state).await else { continue };
        let ctx = ctx.at(place);
        // What cannot be read is not an end.
        let Some(Look::Holds { released: true, for_minutes, .. }) = trigger.look(&ctx, ctx.now) else {
            since = None;
            continue;
        };
        if ctx.now - *since.get_or_insert(ctx.now) >= for_minutes * 60 {
            return;
        }
    }
}

/// What a run did so far: kept outside the steps, so that a run cut short (its lasting trigger is
/// over) still tells what it did and puts back what it drove.
struct Progress {
    run_id: i64,
    /// The installation the rule is about (its sun, its weather).
    place: Option<i64>,
    out: Vec<StepDone>,
    touched: Vec<Touched>,
}

/// Runs the steps one after the other. Returns how it ended.
async fn execute(state: &AppState, row: &AutomationRow, steps: &[Step], mut ctx: Context, actor: &str, p: &mut Progress) -> &'static str {
    let (run_id, place) = (p.run_id, p.place);
    let who = format!("{actor} · automatisme « {} »", row.name);
    let reload = |ctx: Context| async move { Context::load(state).await.map(|c| c.at(place)).unwrap_or(ctx) };
    for step in steps {
        let what = ctx.describe_step(step);
        match step {
            Step::Device { device_id, trait_name, action, params } => {
                let keys: Vec<String> = params.keys().cloned().collect();
                match p.touched.iter_mut().find(|t| t.device_id == *device_id && t.trait_name == *trait_name) {
                    Some(t) => {
                        t.action = action.clone();
                        t.set.extend(params.clone());
                        for k in keys {
                            if !t.keys.contains(&k) {
                                t.keys.push(k);
                            }
                        }
                    }
                    None => p.touched.push(Touched {
                        device_id: *device_id,
                        trait_name: trait_name.clone(),
                        action: action.clone(),
                        keys,
                        before: ctx.device(*device_id).and_then(|d| d.state.as_object().cloned()).unwrap_or_default(),
                        set: params.clone(),
                    }),
                }
                let input = ActionInput { trait_name: trait_name.clone(), action: action.clone(), params: params.clone() };
                p.out.push(done(what, apply(state, &ctx, *device_id, &input, &who).await));
            }
            Step::Scene { routine_id } => {
                let r = async {
                    let row = db::routine_by_id(&state.db, *routine_id).await?.ok_or_else(|| ApiError::not_found("routine not found"))?;
                    let results = crate::routines::run_routine(&state.hub, &state.db, &row, &who).await?;
                    match results.iter().filter(|r| !r.ok).count() {
                        0 => Ok(()),
                        failed => Err(ApiError::bad_gateway(crate::text!("{0} action(s) en échec", "{0} action(s) failed", "{0} acción(es) con fallo", failed))),
                    }
                }
                .await;
                p.out.push(done(what, r));
            }
            Step::Wait { seconds } => {
                tokio::time::sleep(Duration::from_secs(*seconds as u64)).await;
                ctx = reload(ctx).await;
                p.out.push(done(what, Ok(())));
            }
            Step::WaitUntil { condition, for_seconds, timeout_seconds, on_timeout } => {
                let started = tokio::time::Instant::now();
                let deadline = started + Duration::from_secs(*timeout_seconds as u64);
                let mut since: Option<tokio::time::Instant> = None;
                let reached = loop {
                    ctx = reload(ctx).await;
                    let now = tokio::time::Instant::now();
                    if condition.met(&ctx) == Some(true) {
                        let held = *since.get_or_insert(now);
                        if now.duration_since(held).as_secs() as i64 >= *for_seconds {
                            break true;
                        }
                    } else {
                        since = None;
                    }
                    if now >= deadline {
                        break false;
                    }
                    tokio::time::sleep(POLL.min(deadline.saturating_duration_since(now)).max(Duration::from_millis(100))).await;
                };
                if reached {
                    p.out.push(done(what, Ok(())));
                } else {
                    let late = crate::text!("{0} — temps écoulé", "{0} — time is up", "{0} — tiempo agotado", what.arg());
                    p.out.push(StepDone { what: late, ok: true, error: None });
                    if *on_timeout == OnTimeout::Stop {
                        save_steps(state, run_id, &p.out).await;
                        return "stopped";
                    }
                }
            }
            Step::Notify { message } => {
                p.out.push(done(what, notify(state, &row.name, message.trim(), actor).await));
            }
            Step::Restore => {
                p.out.push(done(what, Ok(())));
                ctx = reload(ctx).await;
                for t in p.touched.drain(..).rev() {
                    let Some((action, params)) = undo(&t.trait_name, &t.action, &t.keys, &t.before) else { continue };
                    let name = ctx.device_name(t.device_id);
                    let now = ctx.device(t.device_id).and_then(|d| d.state.as_object().cloned()).unwrap_or_default();
                    if !changed_since(&t, &now).is_empty() {
                        let what = crate::text!(
                            "{0} laissé tel quel : changé entre-temps",
                            "{0} left as it is: changed in the meantime",
                            "{0} se deja como está: cambiado mientras tanto",
                            name.arg()
                        );
                        p.out.push(StepDone { what, ok: true, error: None });
                        continue;
                    }
                    let how = ctx.describe_action(t.device_id, &t.trait_name, &action, &params);
                    let what = crate::text!("remettre {0} : {1}", "put back {0}: {1}", "dejar {0}: {1}", name.arg(), how.arg());
                    let input = ActionInput { trait_name: t.trait_name.clone(), action, params };
                    p.out.push(done(what, apply(state, &ctx, t.device_id, &input, &who).await));
                }
            }
        }
        save_steps(state, run_id, &p.out).await;
    }
    "done"
}

/// Tells a message: in the activity feed (the open apps show it at once) and, when set, to an
/// ntfy topic (a phone notification, with no account and no cloud of ours).
pub async fn notify(state: &AppState, rule: &str, message: &str, actor: &str) -> ApiResult<()> {
    let text = crate::text!("{0} : {1}", "{0}: {1}", "{0}: {1}", rule, message);
    state.hub.log("notification", "info", None, Some(actor), text, None).await;
    let Some(url) = db::get_setting(&state.db, NTFY).await?.filter(|u| !u.trim().is_empty()) else { return Ok(()) };
    let sent = reqwest::Client::new()
        .post(url.trim())
        // In the address: a header carries no accent.
        .query(&[("title", format!("Breezone · {rule}")), ("tags", "house".into())])
        .timeout(Duration::from_secs(10))
        .body(message.to_string())
        .send()
        .await
        .and_then(|r| r.error_for_status());
    sent.map(|_| ()).map_err(|e| {
        let e = e.without_url().to_string();
        ApiError::bad_gateway(crate::text!("ntfy : {0}", "ntfy: {0}", "ntfy: {0}", e))
    })
}

/// No rule starts by itself until this time (`Some(0)`: until switched back on).
pub async fn paused_until(db: &db::Db) -> Option<i64> {
    let until: i64 = db::get_setting(db, PAUSED).await.ok()??.parse().ok()?;
    (until == 0 || until > db::now()).then_some(until)
}

/// Keeps the steps done so far, so that the history shows a run that waits.
async fn save_steps(state: &AppState, run_id: i64, done: &[StepDone]) {
    let stored = Value::Array(done.iter().map(StepDone::stored).collect()).to_string();
    if let Err(e) = db::set_automation_run_steps(&state.db, run_id, &stored).await {
        tracing::error!("automation run {run_id}: {e}");
    }
}

/// Looks at the rules once: starts those that are due.
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
    // On holiday: nothing starts by itself; what happened meanwhile is not caught up afterwards.
    if paused_until(&state.db).await.is_some() {
        engine.checked = db::now();
        return;
    }
    let ctx = match Context::load(state).await {
        Ok(ctx) => ctx,
        Err(e) => return tracing::error!("automations: {}", e.message.en),
    };
    let rules: Vec<(AutomationRow, Automation)> = rows.into_iter().filter_map(|row| Automation::of(&row).map(|rule| (row, rule))).collect();
    for due in engine.due(&rules, &ctx) {
        let Some((row, rule)) = rules.iter().find(|(row, _)| row.id == due.id) else { continue };
        if due.branch == Branch::Skipped {
            if let Err(e) = db::insert_automation_run(&state.db, row.id, "skipped", &due.reason.store(), true).await {
                tracing::error!("automation {}: {e}", row.id);
            }
            state.hub.routines_changed();
            continue;
        }
        start(state, row, rule, due, ctx.clone(), ACTOR).await;
    }
}

pub async fn run_loop(state: AppState) {
    if let Err(e) = db::interrupt_automation_runs(&state.db).await {
        tracing::error!("automations: {e}");
    }
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
            places: HashMap::new(),
            home: None,
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
}
