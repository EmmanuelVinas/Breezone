//! Drivers (ADR 0013): what every source of devices implements, so that the rest of Breezone reads
//! and drives devices through their traits without knowing the hardware. A *source* is one
//! configured piece of hardware (a Shelly at 192.168.1.40…), handled by a driver.
//!
//! The interface is defined with real drivers before being frozen: Shelly first, then Airzone,
//! whose zones are still read by the hub for now (they are already devices, and their actions go
//! through the same generic route). The future WASM modules (ADR 0012) will be drivers too.

pub mod shelly;

use crate::db::{self, Db};
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
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
    vec![shelly::info()]
}

/// The driver of a source, from its configuration.
pub fn create(driver: &str, config: &Value) -> Result<Arc<dyn Driver>, String> {
    match driver {
        "shelly" => Ok(Arc::new(shelly::Shelly::new(config)?)),
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
}

/// How often a source is read.
const READ_EVERY: Duration = Duration::from_secs(5);

impl Runtime {
    pub fn new(db: Db) -> Self {
        Self { db, sources: RwLock::new(HashMap::new()) }
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
                    Running { source, driver, devices: vec![], states: HashMap::new(), status }
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
    }

    pub async fn poll_all(&self) {
        let ids: Vec<i64> = self.sources.read().await.keys().copied().collect();
        for id in ids {
            self.poll(id).await;
        }
    }

    pub async fn run(self: Arc<Self>) {
        loop {
            self.poll_all().await;
            tokio::time::sleep(READ_EVERY).await;
        }
    }

    pub async fn status(&self, id: i64) -> Option<SourceStatus> {
        self.sources.read().await.get(&id).map(|r| r.status.clone())
    }

    pub async fn devices(&self) -> Vec<SourceDevice> {
        let sources = self.sources.read().await;
        let mut out = vec![];
        for r in sources.values() {
            for info in &r.devices {
                out.push(SourceDevice {
                    source_id: r.source.id,
                    driver: r.source.driver.clone(),
                    state: r.states.get(&info.reference).cloned(),
                    online: r.status.state == "online",
                    info: info.clone(),
                });
            }
        }
        out
    }

    /// Applies an action, then reads the source again so that the new state shows at once.
    pub async fn act(&self, source_id: i64, reference: &str, trait_name: &str, action: &str, params: &State) -> Result<(), DriverError> {
        let driver = self.sources.read().await.get(&source_id).and_then(|r| r.driver.clone()).ok_or_else(|| DriverError::Unreachable("source not configured".into()))?;
        driver.act(reference, trait_name, action, params).await?;
        self.poll(source_id).await;
        Ok(())
    }
}
