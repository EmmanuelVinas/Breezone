//! Drivers (ADR 0013): what every source of devices implements, so that the rest of Breezone reads
//! and drives devices through their traits without knowing the hardware. A *source* is one
//! configured piece of hardware (a Shelly at 192.168.1.40…), handled by a driver.
//!
//! The interface is defined with real drivers before being frozen: Shelly first, then Airzone,
//! whose zones are still read by the hub for now (they are already devices, and their actions go
//! through the same generic route). The future WASM modules (ADR 0012) will be drivers too.

pub mod homekit;
pub mod hoymiles;
pub mod hue;
pub mod netatmo;
pub mod renson;
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
    /// What tells this hardware apart (Shelly id, charger id, serial number): kept in the source's
    /// configuration (`device_id`), so that it is found again if its address changes.
    pub identity: Option<String>,
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
    /// Shortest time between two readings of the hardware (some cope badly with frequent ones);
    /// an action or a refresh reads it at once anyway.
    fn read_every(&self) -> Duration {
        Duration::ZERO
    }
    /// The rooms set in the hardware's own app (Hue), when it has some.
    fn rooms(&self) -> DriverFuture<'_, Vec<HardwareRoom>> {
        Box::pin(async { Ok(vec![]) })
    }
    /// The scenes set in the hardware's own app (Hue), when it has some.
    fn scenes(&self) -> DriverFuture<'_, Vec<Scene>> {
        Box::pin(async { Ok(vec![]) })
    }
    fn activate_scene<'a>(&'a self, id: &'a str) -> DriverFuture<'a, ()> {
        Box::pin(async move { Err(DriverError::Unsupported(format!("scene {id}"))) })
    }
    /// Gives the hardware a key of its maker that unlocks more of its data (Renson); true once it
    /// accepts it. The hardware keeps it: Breezone does not.
    fn unlock<'a>(&'a self, _key: &'a str) -> DriverFuture<'a, bool> {
        Box::pin(async { Err(DriverError::Unsupported("no key for this module".into())) })
    }
    /// Starts listening to what the hardware tells without being asked (the presses of a remote's
    /// buttons), sent to `sink` until the driver is dropped.
    fn listen(&self, _sink: Sink) {}
    /// Ends the connections kept open with the hardware, cleanly, before the server stops or
    /// restarts on a new version (a gateway with few connections frees them at once).
    fn close(&self) -> DriverFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
    /// Reaches the hardware through its maker's cloud: what it shares with a local module (same
    /// name and kind) stays the local one's, and this one drives only what the local cannot.
    fn cloud(&self) -> bool {
        false
    }
    /// What the hardware says about itself, raw, to understand how a model behaves (support).
    fn diagnostic(&self) -> DriverFuture<'_, Value> {
        Box::pin(async { Ok(Value::Null) })
    }
}

/// A press of a button of a remote (`button@1`).
#[derive(Debug, Clone, PartialEq)]
pub struct Press {
    pub source_id: i64,
    /// The device: the remote.
    pub reference: String,
    /// 1, 2…
    pub button: u32,
    /// "single" | "double" | "long"
    pub press: &'static str,
}

/// Where a driver sends what it hears, for its source.
#[derive(Debug, Clone)]
pub struct Sink {
    source_id: i64,
    tx: tokio::sync::mpsc::UnboundedSender<Press>,
}

impl Sink {
    pub fn new(source_id: i64, tx: tokio::sync::mpsc::UnboundedSender<Press>) -> Self {
        Self { source_id, tx }
    }

    /// False once nobody listens any more (the server stops).
    pub fn press(&self, reference: &str, button: u32, press: &'static str) -> bool {
        self.tx.send(Press { source_id: self.source_id, reference: reference.to_string(), button, press }).is_ok()
    }
}

/// A room of the hardware's own app: its name and the references of the devices in it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HardwareRoom {
    pub name: String,
    pub references: Vec<String>,
}

/// A scene of the hardware's own app: the room it belongs to, if any, and the devices it sets.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Scene {
    pub id: String,
    pub name: String,
    pub room: Option<String>,
    pub references: Vec<String>,
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
    vec![shelly::info(), trydan::info(), hoymiles::info(), homekit::info(), hue::info(), renson::info(), netatmo::info()]
}

/// The driver of a source, from its configuration and its secret (decrypted).
pub fn create(driver: &str, config: &Value, secret: Option<&str>) -> Result<Arc<dyn Driver>, String> {
    match driver {
        "shelly" => Ok(Arc::new(shelly::Shelly::new(config)?)),
        "trydan" => Ok(Arc::new(trydan::Trydan::new(config)?)),
        "hoymiles" => Ok(Arc::new(hoymiles::Hoymiles::new(config)?)),
        "homekit" => Ok(Arc::new(homekit::HomeKit::new(config, secret)?)),
        "hue" => Ok(Arc::new(hue::Hue::new(config, secret)?)),
        "renson" => Ok(Arc::new(renson::Renson::new(config)?)),
        // Created by the runtime, which holds what reaches its account (see `Runtime::reload`).
        "netatmo" => Err("the Netatmo module comes with its linked account".into()),
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
    last_read: Option<Instant>,
    /// Readings failed in a row: an unreachable source is read less and less often.
    failures: u32,
    /// For a cloud source: its devices that are also a local source's (same name and kind), by
    /// that key; they are not devices of their own, only another way to drive the local ones.
    twins: HashMap<(String, String), String>,
    /// For a cloud source: every device it described, its twins included (sorted out at each reading).
    cloud_devices: Vec<DeviceInfo>,
}

/// A device's name (normalised) and family.
type TwinKey = (String, String);
type HashSet<T> = std::collections::HashSet<T>;

/// What tells a device of a cloud source and one of a local source apart from being the same:
/// its name (as its app names it, the same on both paths) and its family.
fn twin_key(d: &DeviceInfo) -> TwinKey {
    let family = match d.kind.as_str() {
        "shutter" | "window" | "door" | "cover" => "cover",
        "light" | "switch.relay" => "onoff",
        other => other,
    };
    (d.name.trim().to_lowercase().split_whitespace().collect::<Vec<_>>().join(" "), family.to_string())
}

/// The longest wait between two readings of a source that does not answer.
const RETRY_AT_MOST: Duration = Duration::from_secs(300);

/// When a source is read again: at its pace, or, once it is offline, twice as late after each
/// failure (up to `RETRY_AT_MOST`), so that a struggling device is not sent a new connection every
/// few seconds. « Tester » reads it at once all the same.
fn read_wait(every: Duration, failures: u32, offline: bool) -> Duration {
    if !offline || failures == 0 {
        return every;
    }
    (every * 2u32.pow(failures.min(8))).min(RETRY_AT_MOST.max(every))
}

/// Readings of one number since the last history point.
#[derive(Debug, Clone, Copy)]
struct Pending {
    sum: f64,
    count: u32,
    last: f64,
}

/// The identity of the hardware a source was configured with (`device_id`).
fn config_identity(source: &db::Source) -> Option<String> {
    let config: Value = serde_json::from_str(&source.config).ok()?;
    config["device_id"].as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
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

/// What « Rechercher » found of a source's hardware.
#[derive(Debug, Clone, Serialize)]
pub struct Located {
    /// The source knows its hardware's identity (else it cannot be looked for).
    pub identified: bool,
    /// What the network announces of it (`None`: not announced).
    pub seen: Option<Seen>,
    /// Found at another address, now used.
    pub moved: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Seen {
    pub host: String,
    /// HomeKit: still paired with a controller (`false`: reset, it waits for a new pairing).
    pub paired: Option<bool>,
}

/// Seconds a source stays online after its last successful reading, at least.
const MISSED_READINGS_GRACE: i64 = 60;

pub struct Runtime {
    db: Db,
    key: [u8; 32],
    sources: RwLock<HashMap<i64, Running>>,
    /// Charges to stop once they have added some energy, by source and device.
    goals: RwLock<HashMap<(i64, String), Goal>>,
    /// When each lost source was last looked for on the network.
    searched: RwLock<HashMap<i64, Instant>>,
    /// The presses of the remotes, sent by the drivers that listen (`Driver::listen`).
    presses: tokio::sync::mpsc::UnboundedSender<Press>,
    heard: std::sync::Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<Press>>>,
    /// Netatmo's address (the cloud module reaches its linked account).
    netatmo_url: String,
    /// When this server started: a source not read since is not lost before `RELOCATE_AFTER`.
    started: i64,
}

/// A source unreachable this long (seconds) is looked for on the network by its identity.
const RELOCATE_AFTER: i64 = 180;
/// At most one search per source in this time.
const RELOCATE_EVERY: Duration = Duration::from_secs(15 * 60);
/// How often the lost sources are checked.
const RELOCATE_CHECK: Duration = Duration::from_secs(60);

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
    /// `key`: decrypts the secrets of the sources (the server key).
    pub fn new(db: Db, key: [u8; 32], netatmo_url: String) -> Self {
        let (presses, heard) = tokio::sync::mpsc::unbounded_channel();
        Self {
            db,
            key,
            sources: RwLock::new(HashMap::new()),
            goals: RwLock::new(HashMap::new()),
            searched: RwLock::new(HashMap::new()),
            presses,
            heard: std::sync::Mutex::new(Some(heard)),
            netatmo_url,
            started: db::now(),
        }
    }

    /// Takes the configured sources into account (at start, and after each change).
    pub async fn reload(&self) -> sqlx::Result<()> {
        let rows = db::list_sources(&self.db).await?;
        let mut sources = self.sources.write().await;
        let mut next = HashMap::new();
        for source in rows {
            let previous = sources.remove(&source.id).filter(|r| r.source.driver == source.driver && r.source.config == source.config && r.source.secret == source.secret);
            let running = match previous {
                Some(mut r) => {
                    r.source = source;
                    r
                }
                None => {
                    let config: Value = serde_json::from_str(&source.config).unwrap_or(Value::Null);
                    let secret = source.secret.as_deref().and_then(|s| crate::crypto::decrypt(&self.key, s));
                    let created = match source.driver.as_str() {
                        "netatmo" => Ok(Arc::new(netatmo::Netatmo::new(self.db.clone(), self.key, self.netatmo_url.clone())) as Arc<dyn Driver>),
                        _ => create(&source.driver, &config, secret.as_deref()),
                    };
                    let (driver, error) = match created {
                        Ok(d) => {
                            d.listen(Sink::new(source.id, self.presses.clone()));
                            (Some(d), None)
                        }
                        Err(e) => (None, Some(e)),
                    };
                    let status = SourceStatus { state: if error.is_some() { "error" } else { "unknown" }, last_error: error, ..Default::default() };
                    Running {
                        source,
                        driver,
                        devices: vec![],
                        states: HashMap::new(),
                        status,
                        pending: HashMap::new(),
                        last_read: None,
                        failures: 0,
                        twins: HashMap::new(),
                        cloud_devices: vec![],
                    }
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
        // A cloud source waits for the local ones to be read once: what they have is not repeated.
        let locals_pending =
            driver.cloud() && self.sources.read().await.values().any(|o| o.source.enabled && o.driver.as_ref().is_some_and(|d| !d.cloud()) && o.status.state == "unknown");
        if locals_pending {
            return;
        }
        if let Some(r) = self.sources.write().await.get_mut(&id) {
            r.last_read = Some(Instant::now());
        }
        let described = if known { None } else { Some(driver.describe().await) };
        let result = match &described {
            Some(Err(e)) => Err(e.clone()),
            _ => driver.read().await,
        };
        // The devices of the local sources (for a cloud source), or those the cloud can also drive.
        let (local, in_cloud): (HashSet<TwinKey>, HashSet<TwinKey>) = {
            let sources = self.sources.read().await;
            let is_cloud = |o: &Running| o.driver.as_ref().is_some_and(|d| d.cloud());
            (
                sources.values().filter(|o| o.source.id != id && !is_cloud(o)).flat_map(|o| o.devices.iter().map(twin_key)).collect(),
                sources.values().filter(|o| is_cloud(o)).flat_map(|o| o.twins.keys().cloned()).collect(),
            )
        };
        let mut new_devices = vec![];
        let mut identity: Option<(db::Source, String)> = None;
        {
            let mut sources = self.sources.write().await;
            let Some(r) = sources.get_mut(&id) else { return };
            if let Some(Ok(d)) = described {
                r.status.model = Some(d.model);
                r.status.firmware = d.firmware;
                identity = d.identity.filter(|_| config_identity(&r.source).is_none()).map(|i| (r.source.clone(), i));
                if driver.cloud() {
                    r.cloud_devices = d.devices;
                } else if r.devices != d.devices {
                    new_devices = d.devices.clone();
                    r.devices = d.devices;
                }
            }
            // A cloud source does not repeat a device a local source has: it is that one's twin.
            if driver.cloud() {
                r.twins = r.cloud_devices.iter().filter(|x| local.contains(&twin_key(x))).map(|x| (twin_key(x), x.reference.clone())).collect();
                let own: Vec<DeviceInfo> = r.cloud_devices.iter().filter(|x| !local.contains(&twin_key(x))).cloned().collect();
                if r.devices != own {
                    new_devices = own.iter().filter(|x| !r.devices.contains(x)).cloned().collect();
                    r.devices = own;
                }
            }
            match result {
                Ok(mut states) => {
                    // A local shutter the cloud can stop midway can be stopped.
                    for d in r.devices.iter().filter(|d| in_cloud.contains(&twin_key(d))) {
                        if let Some(s) = states.get_mut(&d.reference).filter(|s| s.get("stoppable") == Some(&Value::Bool(false))) {
                            s.insert("stoppable".into(), Value::Bool(true));
                        }
                    }
                    for (reference, state) in &states {
                        let pending = r.pending.entry(reference.clone()).or_default();
                        for (field, v) in state {
                            // Settings and fixed values of the hardware have no history.
                            if !crate::traits::keeps_history(field) {
                                continue;
                            }
                            let Some(v) = number(v) else { continue };
                            let p = pending.entry(field.clone()).or_insert(Pending { sum: 0.0, count: 0, last: v });
                            p.sum += v;
                            p.count += 1;
                            p.last = v;
                        }
                    }
                    r.states = states;
                    r.failures = 0;
                    r.status.state = "online";
                    r.status.last_seen = Some(db::now());
                    r.status.last_error = None;
                }
                Err(e) => {
                    r.failures = r.failures.saturating_add(1);
                    // A missed reading is not an outage (a Hoymiles DTU serves one client at a time):
                    // the source stays online, with its last values, until its readings fail for a while.
                    let grace = (driver.read_every().as_secs() as i64 * 3).max(MISSED_READINGS_GRACE);
                    let recent = r.status.last_seen.is_some_and(|t| db::now() - t < grace);
                    if !(recent && r.status.state == "online" && matches!(e, DriverError::Unreachable(_))) {
                        r.status.state = if matches!(e, DriverError::Unreachable(_)) { "offline" } else { "error" };
                    }
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
        // A source added by its address learns the identity of its hardware.
        if let Some((source, identity)) = identity {
            let mut config: Value = serde_json::from_str(&source.config).unwrap_or_else(|_| Value::Object(Map::new()));
            if let Some(o) = config.as_object_mut() {
                o.insert("device_id".into(), Value::from(identity));
            }
            let text = config.to_string();
            match db::update_source(&self.db, id, &source.name, &text, source.enabled).await {
                Ok(_) => {
                    if let Some(r) = self.sources.write().await.get_mut(&id) {
                        r.source.config = text;
                    }
                }
                Err(e) => tracing::error!("cannot keep the identity of source {id}: {e}"),
            }
        }
        self.check_goals(id).await;
    }

    /// Handles the presses of the remotes as they come (once: the first call takes them).
    pub fn hear(self: Arc<Self>) {
        let Some(mut heard) = self.heard.lock().unwrap_or_else(|e| e.into_inner()).take() else { return };
        tokio::spawn(async move {
            while let Some(p) = heard.recv().await {
                self.pressed(p).await;
            }
        });
    }

    /// A button was pressed: its remote shows it and the activity tells it. Breezone does nothing
    /// else: what a button does is set in the remote's own app (Legrand…), not done twice.
    async fn pressed(&self, p: Press) {
        let given = {
            let mut sources = self.sources.write().await;
            let Some(r) = sources.get_mut(&p.source_id).filter(|r| r.source.enabled) else { return };
            let given = r.devices.iter().find(|d| d.reference == p.reference).map(|d| d.name.clone());
            let state = r.states.entry(p.reference.clone()).or_default();
            state.insert("last_button".into(), Value::from(p.button));
            state.insert("last_press".into(), Value::from(p.press));
            state.insert("last_at".into(), Value::from(db::now()));
            given
        };
        let source_ref = format!("{}/{}", p.source_id, p.reference);
        let device: Option<(i64, Option<String>)> =
            match sqlx::query_as("SELECT id, name FROM devices WHERE source_id = ? AND source_ref = ?").bind(p.source_id).bind(&source_ref).fetch_optional(&self.db).await {
                Ok(d) => d,
                Err(e) => return tracing::error!("press of {source_ref}: {e}"),
            };
        let Some((device_id, name)) = device else { return };
        let name = name.or(given).unwrap_or_else(|| p.reference.clone());
        let how = match p.press {
            "double" => crate::text!("double appui", "double press", "doble pulsación"),
            "long" => crate::text!("appui long", "long press", "pulsación larga"),
            _ => crate::text!("appui", "press", "pulsación"),
        };
        let message = crate::i18n::Text {
            fr: format!("« {name} » : bouton {}, {}", p.button, how.fr),
            en: format!("“{name}”: button {}, {}", p.button, how.en),
            es: format!("«{name}»: botón {}, {}", p.button, how.es),
        };
        let data = serde_json::json!({"device_id": device_id, "button": p.button, "press": p.press});
        if let Err(e) = db::insert_event(&self.db, "button", "info", None, None, &message, Some(&data)).await {
            tracing::error!("cannot log the press of {source_ref}: {e}");
        }
    }

    /// Reads every source that is due (see `Driver::read_every`).
    pub async fn poll_all(&self) {
        let due: Vec<i64> = self
            .sources
            .read()
            .await
            .values()
            .filter(|r| match (&r.driver, r.last_read) {
                (Some(d), Some(last)) => last.elapsed() >= read_wait(d.read_every(), r.failures, matches!(r.status.state, "offline" | "error")),
                _ => true,
            })
            .map(|r| r.source.id)
            .collect();
        for id in due {
            self.poll(id).await;
        }
    }

    /// Reads the sources every `read_every`, and writes their history every `record_every`.
    /// Looks for the lost sources on `scan`: the subnet configured (else the one saved or found).
    pub async fn run(self: Arc<Self>, read_every: Duration, record_every: Duration, scan: Option<String>, airzone_port: u16) {
        let mut recorded = Instant::now();
        let mut checked = Instant::now();
        loop {
            self.poll_all().await;
            if recorded.elapsed() >= record_every {
                recorded = Instant::now();
                self.record().await;
            }
            if checked.elapsed() >= RELOCATE_CHECK {
                checked = Instant::now();
                let me = self.clone();
                let scan = scan.clone();
                // A network scan takes a few seconds: the readings go on meanwhile.
                tokio::spawn(async move { me.relocate_lost(scan.as_deref(), airzone_port).await });
            }
            tokio::time::sleep(read_every).await;
        }
    }

    /// The addresses of the HomeKit accessories added: a scan finds them through mDNS only.
    pub async fn quiet_hosts(&self) -> Vec<std::net::Ipv4Addr> {
        self.sources
            .read()
            .await
            .values()
            .filter(|r| r.source.driver == "homekit")
            .filter_map(|r| {
                let config: Value = serde_json::from_str(&r.source.config).ok()?;
                let host = config["host"].as_str()?.trim();
                host.split(':').next()?.parse().ok()
            })
            .collect()
    }

    /// Ends the drivers' open connections cleanly (see `Driver::close`), a few seconds at most.
    pub async fn close_all(&self) {
        let drivers: Vec<Arc<dyn Driver>> = self.sources.read().await.values().filter_map(|r| r.driver.clone()).collect();
        let all = futures_util::future::join_all(drivers.iter().map(|d| d.close()));
        let _ = tokio::time::timeout(Duration::from_secs(3), all).await;
    }

    /// The sources unreachable for a while that know the identity of their hardware, and were not
    /// looked for lately.
    async fn lost(&self) -> Vec<i64> {
        let now = db::now();
        let searched = self.searched.read().await;
        self.sources
            .read()
            .await
            .values()
            .filter(|r| r.source.enabled && r.driver.is_some() && matches!(r.status.state, "offline" | "error"))
            .filter(|r| config_identity(&r.source).is_some())
            // Not read since the server started: lost only once it has had the time to answer (a
            // search right after a restart would probe a gateway still freeing its connections).
            .filter(|r| now - r.status.last_seen.unwrap_or(r.source.created_at.max(self.started)) >= RELOCATE_AFTER)
            .filter(|r| searched.get(&r.source.id).is_none_or(|t| t.elapsed() >= RELOCATE_EVERY))
            .map(|r| r.source.id)
            .collect()
    }

    async fn relocate_lost(&self, configured: Option<&str>, airzone_port: u16) {
        let lost = self.lost().await;
        if lost.is_empty() {
            return;
        }
        {
            let mut searched = self.searched.write().await;
            for id in &lost {
                searched.insert(*id, Instant::now());
            }
        }
        let Some(subnet) = crate::discovery::global_subnet(&self.db, configured).await else { return };
        match crate::discovery::scan_products(&subnet, airzone_port, &self.quiet_hosts().await).await {
            Ok(products) => {
                self.relocate(&products, &lost).await;
            }
            Err(e) => tracing::warn!("cannot look for the lost sources on {subnet}: {e}"),
        }
    }

    /// Looks for one source on the network now (« Rechercher »): what the network announces of its
    /// hardware, and whether the source followed it to another address.
    pub async fn locate(&self, id: i64, configured: Option<&str>, airzone_port: u16) -> Result<Located, String> {
        let subnet = crate::discovery::global_subnet(&self.db, configured).await.ok_or_else(|| "no network to scan: set it in Settings".to_string())?;
        let products = crate::discovery::scan_products(&subnet, airzone_port, &self.quiet_hosts().await).await?;
        self.searched.write().await.insert(id, Instant::now());
        let identity = self.sources.read().await.get(&id).and_then(|r| Some((config_identity(&r.source)?, r.source.driver.clone())));
        let seen = identity.as_ref().and_then(|(identity, driver)| {
            let p = products.iter().find(|p| p.driver == driver.as_str() && p.device_id.as_deref() == Some(identity.as_str()))?;
            // A HomeKit accessory that can be added again is no longer paired with anyone.
            Some(Seen { host: p.host.clone(), paired: (p.driver == "homekit").then_some(!p.supported) })
        });
        let moved = !self.relocate(&products, &[id]).await.is_empty();
        Ok(Located { identified: identity.is_some(), seen, moved })
    }

    /// Gives the sources in `ids` the new address of their hardware, when the network has it
    /// elsewhere; returns the sources moved.
    pub async fn relocate(&self, products: &[crate::discovery::Product], ids: &[i64]) -> Vec<i64> {
        let sources: Vec<db::Source> = self.sources.read().await.values().filter(|r| ids.contains(&r.source.id)).map(|r| r.source.clone()).collect();
        let mut moved = vec![];
        for source in sources {
            let Some(identity) = config_identity(&source) else { continue };
            let mut config: Value = serde_json::from_str(&source.config).unwrap_or(Value::Null);
            let old = config["host"].as_str().unwrap_or_default().to_string();
            let Some(found) = products.iter().find(|p| p.driver == source.driver && p.device_id.as_deref() == Some(identity.as_str()) && p.host != old) else {
                continue;
            };
            config["host"] = Value::from(found.host.clone());
            if let Err(e) = db::update_source(&self.db, source.id, &source.name, &config.to_string(), source.enabled).await {
                tracing::error!("cannot move source {}: {e}", source.id);
                continue;
            }
            let message =
                crate::text!("« {0} » a changé d'adresse : {1} → {2}", "“{0}” changed address: {1} → {2}", "«{0}» cambió de dirección: {1} → {2}", source.name, old, found.host);
            if let Err(e) = db::insert_event(&self.db, "connection", "info", None, None, &message, None).await {
                tracing::error!("cannot record event: {e}");
            }
            tracing::info!("source {} ({}) moved from {old} to {}", source.id, source.name, found.host);
            moved.push(source.id);
        }
        if !moved.is_empty() {
            if let Err(e) = self.reload().await {
                tracing::error!("cannot reload the sources: {e}");
            }
            for id in &moved {
                self.poll(*id).await;
            }
        }
        moved
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
    /// The driver of a configured source.
    pub async fn driver(&self, source_id: i64) -> Option<Arc<dyn Driver>> {
        self.sources.read().await.get(&source_id).and_then(|r| r.driver.clone())
    }

    /// Activates a scene, then reads the source again (its devices changed).
    pub async fn activate_scene(&self, source_id: i64, scene: &str) -> Result<(), DriverError> {
        let driver = self.driver(source_id).await.ok_or_else(|| DriverError::Unreachable("source not configured".into()))?;
        driver.activate_scene(scene).await?;
        self.poll(source_id).await;
        Ok(())
    }

    pub async fn act(&self, source_id: i64, reference: &str, trait_name: &str, action: &str, params: &State) -> Result<(), DriverError> {
        let driver = self.sources.read().await.get(&source_id).and_then(|r| r.driver.clone()).ok_or_else(|| DriverError::Unreachable("source not configured".into()))?;
        match driver.act(reference, trait_name, action, params).await {
            // What the local module cannot do, or cannot reach now, its cloud twin may.
            Err(e @ (DriverError::Unsupported(_) | DriverError::Unreachable(_))) => {
                let Some((cloud_id, cloud, twin)) = self.twin_of(source_id, reference).await else { return Err(e) };
                tracing::info!("{reference} of source {source_id}: {trait_name}.{action} through the cloud ({e})");
                cloud.act(&twin, trait_name, action, params).await?;
                self.poll(cloud_id).await;
            }
            other => other?,
        }
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

    /// The cloud source that can also drive a device of a local source, and its reference there.
    async fn twin_of(&self, source_id: i64, reference: &str) -> Option<(i64, Arc<dyn Driver>, String)> {
        let sources = self.sources.read().await;
        let key = twin_key(sources.get(&source_id)?.devices.iter().find(|d| d.reference == reference)?);
        sources.values().find_map(|o| {
            let twin = o.twins.get(&key)?;
            Some((o.source.id, o.driver.clone().filter(|d| d.cloud())?, twin.clone()))
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_device_of_the_cloud_and_one_of_a_local_module_are_the_same_by_name_and_family() {
        let d = |name: &str, kind: &str| DeviceInfo { reference: "r".into(), kind: kind.into(), traits: vec![], name: name.into() };
        assert_eq!(twin_key(&d("Volet  Salon ", "shutter")), twin_key(&d("volet salon", "cover")));
        assert_eq!(twin_key(&d("Lampe", "light")), twin_key(&d("lampe", "switch.relay")));
        assert_ne!(twin_key(&d("Volet salon", "shutter")), twin_key(&d("Volet salon", "switch.relay")));
    }

    /// A local module whose shutter cannot be stopped, or the cloud one that can, with the same shutter.
    struct Fake {
        cloud: bool,
        devices: Vec<DeviceInfo>,
        done: Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl Driver for Fake {
        fn describe(&self) -> DriverFuture<'_, Described> {
            Box::pin(async move { Ok(Described { model: "fake".into(), firmware: None, devices: self.devices.clone(), identity: None }) })
        }
        fn read(&self) -> DriverFuture<'_, HashMap<String, State>> {
            Box::pin(async move {
                Ok(self.devices.iter().map(|d| (d.reference.clone(), serde_json::from_value(serde_json::json!({"position": 40, "stoppable": self.cloud})).unwrap())).collect())
            })
        }
        fn act<'a>(&'a self, reference: &'a str, _trait_name: &'a str, action: &'a str, _params: &'a State) -> DriverFuture<'a, ()> {
            Box::pin(async move {
                if !self.cloud && action == "stop" {
                    return Err(DriverError::Unsupported("no hold position".into()));
                }
                self.done.lock().unwrap().push(format!("{reference} {action}"));
                Ok(())
            })
        }
        fn cloud(&self) -> bool {
            self.cloud
        }
    }

    #[tokio::test]
    async fn the_cloud_twin_of_a_local_shutter_stops_it_and_is_not_repeated() {
        let dir = std::env::temp_dir().join(format!("breezone-twins-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = db::connect(&dir.join("breezone.db")).await.unwrap();
        let rt = Runtime::new(db.clone(), [0; 32], String::new());
        let local_id = db::insert_source(&db, "homekit", "Passerelle", "{}").await.unwrap();
        let cloud_id = db::insert_source(&db, "netatmo", "Netatmo", "{}").await.unwrap();
        let sources = db::list_sources(&db).await.unwrap();
        let done = Arc::new(std::sync::Mutex::new(vec![]));
        let device =
            |reference: &str, name: &str, kind: &str| DeviceInfo { reference: reference.into(), kind: kind.into(), traits: vec!["cover.position@1".into()], name: name.into() };
        for (id, cloud, devices) in [
            (local_id, false, vec![device("2.8", "Volet salon", "shutter")]),
            (cloud_id, true, vec![device("h/v1", "Volet Salon", "shutter"), device("h/e1", "Chauffage", "energy.power")]),
        ] {
            let driver: Arc<dyn Driver> = Arc::new(Fake { cloud, devices, done: done.clone() });
            let source = sources.iter().find(|s| s.id == id).unwrap().clone();
            let running = Running {
                source,
                driver: Some(driver),
                devices: vec![],
                states: HashMap::new(),
                status: SourceStatus { state: "unknown", ..Default::default() },
                pending: HashMap::new(),
                last_read: None,
                failures: 0,
                twins: HashMap::new(),
                cloud_devices: vec![],
            };
            rt.sources.write().await.insert(id, running);
        }

        // The cloud waits for the local module to be read: nothing of it is repeated.
        rt.poll(cloud_id).await;
        assert!(rt.sources.read().await[&cloud_id].devices.is_empty());
        rt.poll(local_id).await;
        rt.poll(cloud_id).await;
        let names: Vec<String> = rt.sources.read().await[&cloud_id].devices.iter().map(|d| d.name.clone()).collect();
        assert_eq!(names, ["Chauffage"], "the shutter is the local one's twin, the Ecometer's line its own");

        // The local shutter can now be stopped: through the cloud.
        rt.poll(local_id).await;
        assert_eq!(rt.sources.read().await[&local_id].states["2.8"]["stoppable"], true);
        rt.act(local_id, "2.8", "cover.position", "stop", &State::new()).await.unwrap();
        rt.act(local_id, "2.8", "cover.position", "set", &State::new()).await.unwrap();
        assert_eq!(*done.lock().unwrap(), ["h/v1 stop", "2.8 set"], "what the local module can do stays its own");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_offline_source_is_read_less_and_less_often() {
        let every = Duration::from_secs(10);
        assert_eq!(read_wait(every, 3, false), every, "a missed reading of an online source keeps its pace");
        assert_eq!(read_wait(every, 0, true), every);
        assert_eq!(read_wait(every, 1, true), Duration::from_secs(20));
        assert_eq!(read_wait(every, 3, true), Duration::from_secs(80));
        assert_eq!(read_wait(every, 40, true), RETRY_AT_MOST);
        assert_eq!(read_wait(Duration::from_secs(600), 5, true), Duration::from_secs(600), "never sooner than its own pace");
    }
}
