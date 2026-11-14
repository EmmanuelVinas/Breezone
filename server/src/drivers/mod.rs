//! Drivers (ADR 0013): what every source of devices implements, so that the rest of Breezone reads
//! and drives devices through their traits without knowing the hardware. A *source* is one
//! configured piece of hardware (a Shelly at 192.168.1.40…), handled by a driver.
//!
//! The interface is defined with real drivers before being frozen: Shelly first, then Airzone,
//! whose zones are still read by the hub for now (they are already devices, and their actions go
//! through the same generic route). The future WASM modules (ADR 0012) will be drivers too.

pub mod shelly;
pub mod trydan;

use crate::db::{self, Db};
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

pub type DriverFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, DriverError>> + Send + 'a>>;

#[derive(Debug, Clone, thiserror::Error)]
pub enum DriverError {
    /// The hardware does not answer.
    #[error("{0}")]
    Unreachable(String),
    /// It answered something this driver does not understand.
    #[error("{0}")]
    Invalid(String),
    /// It cannot do what was asked.
    #[error("{0}")]
    Unsupported(String),
}

/// A device a source provides.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DeviceInfo {
    /// Its reference within the source ("switch:0").
    pub reference: String,
    /// Main trait.
    pub kind: String,
    /// `trait@version` it has.
    pub traits: Vec<String>,
    /// Name given by the hardware (the user can rename it).
    pub name: String,
}

/// What a source says about itself.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Described {
    pub model: String,
    pub firmware: Option<String>,
    pub devices: Vec<DeviceInfo>,
}

/// Values of the traits of one device.
pub type State = Map<String, Value>;

/// What every driver implements.
pub trait Driver: Send + Sync {
    /// Identifies the hardware and lists its devices.
    fn describe(&self) -> DriverFuture<'_, Described>;
    /// Current state of each device, by reference.
    fn read(&self) -> DriverFuture<'_, HashMap<String, State>>;
    /// Applies an action of a trait to a device. The parameters were checked against the catalog
    /// of traits before.
    fn act<'a>(&'a self, reference: &'a str, trait_name: &'a str, action: &'a str, params: &'a State) -> DriverFuture<'a, ()>;
}

/// A field of a driver's configuration form.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigField {
    pub name: &'static str,
    /// "string" | "integer"
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub optional: bool,
    pub label: crate::i18n::Text,
}

/// A driver this server has.
#[derive(Debug, Clone, Serialize)]
pub struct DriverInfo {
    pub id: &'static str,
    pub label: &'static str,
    /// `trait@version` its devices may have.
    pub provides: Vec<&'static str>,
    /// Models recognised by name (others of the same family may work generically).
    pub models: Vec<String>,
    pub config: Vec<ConfigField>,
}

pub fn available() -> Vec<DriverInfo> {
    vec![shelly::info(), trydan::info()]
}

/// The driver of a source, from its configuration.
pub fn create(driver: &str, config: &Value) -> Result<Arc<dyn Driver>, String> {
    match driver {
        "shelly" => Ok(Arc::new(shelly::Shelly::new(config)?)),
        "trydan" => Ok(Arc::new(trydan::Trydan::new(config)?)),
        other => Err(format!("unknown driver: {other}")),
    }
}

// ---------------------------------------------------------------------------------------
// Runtime: reads every source, keeps the state, applies actions
// ---------------------------------------------------------------------------------------

/// Connection state of a source.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct SourceStatus {
    /// "online" | "offline" | "error" | "unknown" | "disabled"
    pub state: &'static str,
    pub last_seen: Option<i64>,
    pub last_error: Option<String>,
    pub model: Option<String>,
    pub firmware: Option<String>,
}

struct Running {
    source: db::Source,
    driver: Option<Arc<dyn Driver>>,
    devices: Vec<DeviceInfo>,
    states: HashMap<String, State>,
    status: SourceStatus,
    /// Numbers read since the last history point, by device then field.
    pending: HashMap<String, HashMap<String, Pending>>,
}

/// Readings of one number since the last history point.
#[derive(Debug, Clone, Copy)]
struct Pending {
    sum: f64,
    count: u32,
    last: f64,
}

/// A number of the state worth a history (a switch counts as 0 or 1: its mean is the time on).
fn number(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_bool().map(|b| if b { 1.0 } else { 0.0 }))
}

/// A device of a source, with its last known state.
#[derive(Debug, Clone)]
pub struct SourceDevice {
    pub source_id: i64,
    pub driver: String,
    pub info: DeviceInfo,
    pub state: Option<State>,
    pub online: bool,
}

pub struct Runtime {
    db: Db,
    sources: RwLock<HashMap<i64, Running>>,
    /// Charges to stop once they have added some energy, by source and device.
    goals: RwLock<HashMap<(i64, String), Goal>>,
}

/// A charge started with an energy to add (`ev.charger` `start`): Breezone stops it once added.
/// Kept in memory: a restart forgets it, and the charge then goes on.
#[derive(Debug, Clone, Copy)]
struct Goal {
    /// Energy of the session when the goal was set.
    base: f64,
    energy: f64,
    /// A car was plugged in since: unplugging it ends the goal.
    plugged: bool,
}

impl Goal {
    fn target(&self) -> f64 {
        ((self.base + self.energy) * 1000.0).round() / 1000.0
    }
}

impl Runtime {
    pub fn new(db: Db) -> Self {
        Self { db, sources: RwLock::new(HashMap::new()), goals: RwLock::new(HashMap::new()) }
    }

    /// Takes the configured sources into account (at start, and after each change).
    pub async fn reload(&self) -> sqlx::Result<()> {
        let rows = db::list_sources(&self.db).await?;
        let mut sources = self.sources.write().await;
        let mut next = HashMap::new();
        for source in rows {
            let previous = sources.remove(&source.id).filter(|r| r.source.driver == source.driver && r.source.config == source.config);
            let running = match previous {
                Some(mut r) => {
                    r.source = source;
                    r
                }
                None => {
                    let config: Value = serde_json::from_str(&source.config).unwrap_or(Value::Null);
                    let (driver, error) = match create(&source.driver, &config) {
                        Ok(d) => (Some(d), None),
                        Err(e) => (None, Some(e)),
                    };
                    let status = SourceStatus { state: if error.is_some() { "error" } else { "unknown" }, last_error: error, ..Default::default() };
                    Running { source, driver, devices: vec![], states: HashMap::new(), status, pending: HashMap::new() }
                }
            };
            next.insert(running.source.id, running);
        }
        *sources = next;
        Ok(())
    }

    /// Reads one source: what it has (once, or after an error), then its state.
    pub async fn poll(&self, id: i64) {
        let (driver, known) = match self.sources.read().await.get(&id) {
            Some(r) if r.source.enabled => (r.driver.clone(), !r.devices.is_empty() && r.status.state == "online"),
            Some(_) => {
                if let Some(r) = self.sources.write().await.get_mut(&id) {
                    r.status.state = "disabled";
                }
                return;
            }
            None => return,
        };
        let Some(driver) = driver else { return };
        let described = if known { None } else { Some(driver.describe().await) };
        let result = match &described {
            Some(Err(e)) => Err(e.clone()),
            _ => driver.read().await,
        };
        let mut new_devices = vec![];
        {
            let mut sources = self.sources.write().await;
            let Some(r) = sources.get_mut(&id) else { return };
            if let Some(Ok(d)) = described {
                r.status.model = Some(d.model);
                r.status.firmware = d.firmware;
                if r.devices != d.devices {
                    new_devices = d.devices.clone();
                    r.devices = d.devices;
                }
            }
            match result {
                Ok(states) => {
                    for (reference, state) in &states {
                        let pending = r.pending.entry(reference.clone()).or_default();
                        for (field, v) in state {
                            let Some(v) = number(v) else { continue };
                            let p = pending.entry(field.clone()).or_insert(Pending { sum: 0.0, count: 0, last: v });
                            p.sum += v;
                            p.count += 1;
                            p.last = v;
                        }
                    }
                    r.states = states;
                    r.status.state = "online";
                    r.status.last_seen = Some(db::now());
                    r.status.last_error = None;
                }
                Err(e) => {
                    r.status.state = if matches!(e, DriverError::Unreachable(_)) { "offline" } else { "error" };
                    r.status.last_error = Some(e.to_string());
                }
            }
        }
        // Each device gets its stable id (ADR 0013).
        for d in new_devices {
            let source = self.sources.read().await.get(&id).map(|r| r.source.driver.clone());
            if let Some(driver) = source {
                if let Err(e) = db::ensure_source_device(&self.db, id, &driver, &d.reference, &d.kind).await {
                    tracing::error!("cannot record device {} of source {id}: {e}", d.reference);
                }
            }
        }
        self.check_goals(id).await;
    }

    pub async fn poll_all(&self) {
        let ids: Vec<i64> = self.sources.read().await.keys().copied().collect();
        for id in ids {
            self.poll(id).await;
        }
    }

    /// Reads the sources every `read_every`, and writes their history every `record_every`.
    pub async fn run(self: Arc<Self>, read_every: Duration, record_every: Duration) {
        let mut recorded = Instant::now();
        loop {
            self.poll_all().await;
            if recorded.elapsed() >= record_every {
                recorded = Instant::now();
                self.record().await;
            }
            tokio::time::sleep(read_every).await;
        }
    }

    /// Writes a history point of every device read since the last one: the mean of each number,
    /// the last value of a counter.
    pub async fn record(&self) {
        let taken: Vec<(String, HashMap<String, Pending>)> = {
            let mut sources = self.sources.write().await;
            sources
                .values_mut()
                .flat_map(|r| {
                    let id = r.source.id;
                    r.pending.drain().map(move |(reference, p)| (format!("{id}/{reference}"), p)).collect::<Vec<_>>()
                })
                .collect()
        };
        if taken.is_empty() {
            return;
        }
        let ids = match db::source_device_ids(&self.db).await {
            Ok(ids) => ids,
            Err(e) => return tracing::error!("cannot record the device history: {e}"),
        };
        let ts = db::now();
        let samples: Vec<db::DeviceSample> = taken
            .into_iter()
            .filter_map(|(key, fields)| ids.get(&key).map(|id| (*id, fields)))
            .flat_map(|(device_id, fields)| {
                fields.into_iter().map(move |(field, p)| {
                    let value = if crate::traits::is_counter(&field) { p.last } else { p.sum / p.count as f64 };
                    db::DeviceSample { device_id, ts, field, value }
                })
            })
            .collect();
        if let Err(e) = db::insert_device_samples(&self.db, &samples).await {
            tracing::error!("cannot record the device history: {e}");
        }
    }

    pub async fn status(&self, id: i64) -> Option<SourceStatus> {
        self.sources.read().await.get(&id).map(|r| r.status.clone())
    }

    pub async fn devices(&self) -> Vec<SourceDevice> {
        let sources = self.sources.read().await;
        let goals = self.goals.read().await;
        let mut out = vec![];
        for r in sources.values() {
            for info in &r.devices {
                let mut state = r.states.get(&info.reference).cloned();
                if let (Some(s), Some(g)) = (state.as_mut(), goals.get(&(r.source.id, info.reference.clone()))) {
                    s.insert("energy_goal".into(), Value::from(g.target()));
                }
                out.push(SourceDevice { source_id: r.source.id, driver: r.source.driver.clone(), state, online: r.status.state == "online", info: info.clone() });
            }
        }
        out
    }

    /// Applies an action, then reads the source again so that the new state shows at once.
    pub async fn act(&self, source_id: i64, reference: &str, trait_name: &str, action: &str, params: &State) -> Result<(), DriverError> {
        let driver = self.sources.read().await.get(&source_id).and_then(|r| r.driver.clone()).ok_or_else(|| DriverError::Unreachable("source not configured".into()))?;
        driver.act(reference, trait_name, action, params).await?;
        if trait_name == "ev.charger" && matches!(action, "start" | "stop") {
            let key = (source_id, reference.to_string());
            let energy = params.get("energy").and_then(Value::as_f64).filter(|_| action == "start");
            let base = self.number(source_id, reference, "session_energy").await.unwrap_or(0.0);
            let mut goals = self.goals.write().await;
            match energy {
                Some(energy) => goals.insert(key, Goal { base, energy, plugged: false }),
                None => goals.remove(&key),
            };
        }
        self.poll(source_id).await;
        Ok(())
    }

    async fn number(&self, source_id: i64, reference: &str, field: &str) -> Option<f64> {
        self.sources.read().await.get(&source_id)?.states.get(reference)?.get(field)?.as_f64()
    }

    /// Stops the charges of a source that have added their energy.
    async fn check_goals(&self, id: i64) {
        let (driver, reached) = {
            let sources = self.sources.read().await;
            let Some(r) = sources.get(&id) else { return };
            let mut goals = self.goals.write().await;
            let mut reached = vec![];
            goals.retain(|(source, reference), g| {
                let Some(s) = r.states.get(reference).filter(|_| *source == id) else { return true };
                let energy = s.get("session_energy").and_then(Value::as_f64).unwrap_or(0.0);
                if energy + 1e-9 < g.base {
                    // A new session started.
                    g.base = 0.0;
                }
                match s.get("status").and_then(Value::as_str) {
                    Some("plugged" | "charging") => g.plugged = true,
                    Some("available") if g.plugged => return false,
                    _ => {}
                }
                if energy - g.base + 1e-9 >= g.energy {
                    reached.push(reference.clone());
                    return false;
                }
                true
            });
            (r.driver.clone(), reached)
        };
        let Some(driver) = driver else { return };
        for reference in reached {
            match driver.act(&reference, "ev.charger", "stop", &State::new()).await {
                Ok(()) => tracing::info!("charge of {reference} (source {id}) stopped: its energy is added"),
                Err(e) => tracing::error!("cannot stop the charge of {reference} (source {id}): {e}"),
            }
        }
    }
}
