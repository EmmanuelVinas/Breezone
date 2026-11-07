//! The hub: in-memory state of every Airzone installation, kept fresh by a poller.
//!
//! * Polls each WebServer, detects connection changes and zone changes.
//! * Applies commands (on/off, setpoint, mode, fan speed, sleep), redirecting mode changes
//!   to the master zone of the system.
//! * Records temperature/humidity history and an activity feed.
//! * Broadcasts every change to WebSocket clients (mobile app, web UI).

use crate::airzone::{master_zone, AirzoneClient, AirzoneError, OperationMode, ZoneData};
use crate::cloud::normalize_mac;
use ed25519_dalek::SigningKey;
use crate::config::Config;
use crate::db::{self, Db, Event, Installation, ZoneMeta};
use crate::error::{ApiError, ApiResult};
use crate::federation::{FedConnection, FedInstallation, FedSnapshot, FedZoneMeta, NodeClient, NodeError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, RwLock};

/// Consecutive poll failures before an installation is reported offline.
const OFFLINE_AFTER_FAILURES: u32 = 2;

// ---------------------------------------------------------------------------------------
// Views (what the API and the WebSocket expose)
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ConnectionView {
    /// "online" | "offline" | "unknown"
    pub state: &'static str,
    pub last_seen: Option<i64>,
    pub last_error: Option<String>,
    pub latency_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ZoneView {
    pub installation_id: i64,
    pub system_id: i64,
    pub zone_id: i64,
    /// Name to display (Breezone custom name, else the Airzone name).
    pub name: String,
    pub airzone_name: Option<String>,
    pub group: Option<String>,
    pub position: i64,
    pub on: bool,
    pub room_temp: Option<f64>,
    pub humidity: Option<i64>,
    pub setpoint: Option<f64>,
    pub min_temp: Option<f64>,
    pub max_temp: Option<f64>,
    pub step: f64,
    /// "celsius" | "fahrenheit"
    pub units: &'static str,
    pub mode: Option<OperationMode>,
    /// Modes available on this zone's system.
    pub modes: Vec<OperationMode>,
    pub is_master: bool,
    pub speed: Option<i64>,
    pub speeds: Vec<i64>,
    pub sleep: Option<i64>,
    pub double_setpoint: bool,
    pub cool_setpoint: Option<f64>,
    pub heat_setpoint: Option<f64>,
    /// The zone is currently asking for heat/cold.
    pub demand: bool,
    pub battery_low: bool,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstallationView {
    pub id: i64,
    pub name: String,
    pub location: Option<String>,
    pub host: String,
    pub port: i64,
    pub enabled: bool,
    /// "local" (WebServer on this server's network) | "node" (through another Breezone node)
    pub access: &'static str,
    /// Node the installation is reached through.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<NodeRef>,
    /// The installation can be driven (false: shared read-only by its node).
    pub controllable: bool,
    pub connection: ConnectionView,
    pub zones: Vec<ZoneView>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct NodeRef {
    pub id: i64,
    pub name: String,
    /// Connection state of the node (see `NodeStatus`).
    pub state: &'static str,
}

/// Connection state of a node this server consumes.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct NodeStatus {
    /// "online" | "offline" (unreachable) | "revoked" | "expired" | "error" | "unknown"
    pub state: &'static str,
    pub last_seen: Option<i64>,
    pub last_error: Option<String>,
    pub latency_ms: Option<u64>,
    /// Permission granted by the node: "read" | "control".
    pub permission: String,
}

/// Messages pushed to WebSocket clients.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HubMessage {
    Installation { installation: InstallationView },
    InstallationRemoved { id: i64 },
    Event { event: Event },
    RoutinesChanged,
}

/// A command on one zone. Every field is optional; only the provided ones are applied.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ZoneCommand {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setpoint: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cool_setpoint: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heat_setpoint: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<OperationMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sleep: Option<i64>,
}

impl ZoneCommand {
    pub fn is_empty(&self) -> bool {
        *self == ZoneCommand::default()
    }

    /// Human readable summary, used in the activity feed.
    pub fn describe(&self) -> String {
        let mut parts = vec![];
        if let Some(on) = self.on {
            parts.push(if on { "allumée".to_string() } else { "éteinte".to_string() });
        }
        if let Some(sp) = self.setpoint {
            parts.push(format!("consigne {sp}°"));
        }
        if let Some(sp) = self.heat_setpoint {
            parts.push(format!("consigne chaud {sp}°"));
        }
        if let Some(sp) = self.cool_setpoint {
            parts.push(format!("consigne froid {sp}°"));
        }
        if let Some(m) = self.mode {
            parts.push(format!("mode {}", mode_label(m)));
        }
        if let Some(s) = self.speed {
            parts.push(format!("ventilation {s}"));
        }
        if let Some(s) = self.sleep {
            parts.push(if s == 0 { "veille désactivée".into() } else { format!("veille {s} min") });
        }
        parts.join(", ")
    }
}

pub fn mode_label(m: OperationMode) -> &'static str {
    match m {
        OperationMode::Stop => "arrêt",
        OperationMode::Cooling => "froid",
        OperationMode::Heating => "chaud",
        OperationMode::Fan => "ventilation",
        OperationMode::Dry => "déshumidification",
        OperationMode::AuxHeating => "chauffage d'appoint",
        OperationMode::Auto => "auto",
    }
}

// ---------------------------------------------------------------------------------------
// Hub
// ---------------------------------------------------------------------------------------

/// Last history point recorded for a zone: (ts, setpoint, on, mode).
type Recorded = (i64, Option<f64>, bool, Option<i64>);

/// How an installation is reached.
#[derive(Clone)]
enum Backend {
    /// Airzone local API of a WebServer on this server's network.
    Local(AirzoneClient),
    /// Federation API of another Breezone node (`remote_id` = id over there).
    Node { node_id: i64, remote_id: i64 },
}

struct InstState {
    inst: Installation,
    backend: Backend,
    conn: ConnectionView,
    failures: u32,
    zones: Vec<ZoneData>,
    meta: HashMap<(i64, i64), ZoneMeta>,
    /// Names and groups set on the node exposing the installation.
    remote_meta: HashMap<(i64, i64), FedZoneMeta>,
    node: Option<NodeRef>,
    controllable: bool,
    /// Last recorded history point per zone.
    last_recorded: HashMap<(i64, i64), Recorded>,
    /// Incremented when a command starts: a poll started before it carries stale values (the
    /// previous setpoint…) and must not overwrite what the command set.
    commands: u64,
}

impl InstState {
    fn new(inst: Installation, meta: Vec<ZoneMeta>, node: Option<NodeRef>, controllable: bool) -> Self {
        let backend = match (inst.node_id, inst.remote_id) {
            (Some(node_id), Some(remote_id)) => Backend::Node { node_id, remote_id },
            _ => Backend::Local(AirzoneClient::new(&inst.host, inst.port as u16)),
        };
        Self {
            inst,
            backend,
            conn: ConnectionView { state: "unknown", last_seen: None, last_error: None, latency_ms: None },
            failures: 0,
            zones: vec![],
            meta: meta.into_iter().map(|m| ((m.system_id, m.zone_id), m)).collect(),
            remote_meta: HashMap::new(),
            node,
            controllable,
            last_recorded: HashMap::new(),
            commands: 0,
        }
    }

    fn node_id(&self) -> Option<i64> {
        match self.backend {
            Backend::Node { node_id, .. } => Some(node_id),
            Backend::Local(_) => None,
        }
    }

    /// Breezone metadata of a zone: set here, else on the node exposing it.
    fn zone_meta(&self, key: (i64, i64)) -> Option<ZoneMeta> {
        let remote = self.remote_meta.get(&key);
        match (self.meta.get(&key), remote) {
            (Some(l), Some(r)) => Some(ZoneMeta {
                display_name: l.display_name.clone().or_else(|| r.display_name.clone()),
                group_name: l.group_name.clone().or_else(|| r.group.clone()),
                position: if l.group_name.is_some() { l.position } else { r.position },
                ..l.clone()
            }),
            (Some(l), None) => Some(l.clone()),
            (None, Some(r)) => Some(ZoneMeta {
                installation_id: self.inst.id,
                system_id: key.0,
                zone_id: key.1,
                display_name: r.display_name.clone(),
                group_name: r.group.clone(),
                position: r.position,
            }),
            (None, None) => None,
        }
    }

    /// Stores a successful read and returns the zones whose history point is due.
    fn apply_zones(&mut self, zones: Vec<ZoneData>, history_interval: i64) -> Vec<ZoneData> {
        self.zones = zones;
        let now = db::now();
        let mut due_zones = vec![];
        for z in &self.zones {
            let key = (z.system_id, z.zone_id);
            let current = (z.setpoint, z.is_on(), z.mode);
            let due = match self.last_recorded.get(&key) {
                None => true,
                Some((ts, sp, on, mode)) => now - ts >= history_interval || (*sp, *on, *mode) != current,
            };
            if due {
                self.last_recorded.insert(key, (now, current.0, current.1, current.2));
                due_zones.push(z.clone());
            }
        }
        due_zones
    }

    /// What this installation looks like to a node it is shared with.
    fn federated(&self) -> FedInstallation {
        FedInstallation {
            id: self.inst.id,
            name: self.inst.name.clone(),
            location: self.inst.location.clone(),
            mac: self.inst.mac.clone(),
            connection: FedConnection { state: self.conn.state.to_string(), last_seen: self.conn.last_seen, last_error: self.conn.last_error.clone() },
            zones: self.zones.clone(),
            meta: self
                .meta
                .values()
                .map(|m| FedZoneMeta { system_id: m.system_id, zone_id: m.zone_id, display_name: m.display_name.clone(), group: m.group_name.clone(), position: m.position })
                .collect(),
        }
    }

    fn view(&self) -> InstallationView {
        // Modes available on a system are carried by its master zone.
        let masters: HashMap<i64, i64> = self
            .zones
            .iter()
            .filter_map(|z| master_zone(&self.zones, z.system_id).map(|m| (z.system_id, m)))
            .collect();
        let system_modes: HashMap<i64, Vec<OperationMode>> = self
            .zones
            .iter()
            .filter(|z| masters.get(&z.system_id) == Some(&z.zone_id))
            .map(|z| {
                let modes = z.modes.clone().unwrap_or_default().into_iter().filter_map(OperationMode::from_code).collect();
                (z.system_id, modes)
            })
            .collect();
        let mut zones: Vec<ZoneView> = self
            .zones
            .iter()
            .map(|z| {
                let is_master = masters.get(&z.system_id) == Some(&z.zone_id);
                zone_view(self.inst.id, z, is_master, self.zone_meta((z.system_id, z.zone_id)).as_ref(), system_modes.get(&z.system_id))
            })
            .collect();
        zones.sort_by(|a, b| {
            (a.group.is_none(), a.group.clone(), a.position, a.system_id, a.zone_id)
                .cmp(&(b.group.is_none(), b.group.clone(), b.position, b.system_id, b.zone_id))
        });
        InstallationView {
            id: self.inst.id,
            name: self.inst.name.clone(),
            location: self.inst.location.clone(),
            host: self.inst.host.clone(),
            port: self.inst.port,
            enabled: self.inst.enabled,
            access: if self.node.is_some() { "node" } else { "local" },
            node: self.node.clone(),
            controllable: self.controllable,
            connection: self.conn.clone(),
            zones,
        }
    }
}

fn zone_view(installation_id: i64, z: &ZoneData, is_master: bool, meta: Option<&ZoneMeta>, system_modes: Option<&Vec<OperationMode>>) -> ZoneView {
    let airzone_name = z.name.clone().filter(|n| !n.trim().is_empty());
    let name = meta
        .and_then(|m| m.display_name.clone())
        .filter(|n| !n.trim().is_empty())
        .or_else(|| airzone_name.clone())
        .unwrap_or_else(|| format!("Zone {}", z.zone_id));
    let speeds = match (&z.speed_values, z.speeds) {
        (Some(values), _) if !values.is_empty() => values.clone(),
        (_, Some(n)) if n > 0 => (0..=n).collect(),
        _ => vec![],
    };
    let errors = z
        .errors
        .iter()
        .map(|e| match e {
            Value::Object(map) => map.values().map(|v| v.as_str().unwrap_or_default().to_string()).collect::<Vec<_>>().join(", "),
            Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .filter(|e| !e.is_empty())
        .collect();
    ZoneView {
        installation_id,
        system_id: z.system_id,
        zone_id: z.zone_id,
        name,
        airzone_name,
        group: meta.and_then(|m| m.group_name.clone()).filter(|g| !g.trim().is_empty()),
        position: meta.map(|m| m.position).unwrap_or(0),
        on: z.is_on(),
        room_temp: z.room_temp,
        // Zones without a humidity sensor report 0.
        humidity: z.humidity.filter(|h| *h > 0),
        setpoint: z.setpoint,
        min_temp: z.min_temp,
        max_temp: z.max_temp,
        step: z.temp_step.filter(|s| *s > 0.0).unwrap_or(0.5),
        units: if z.units == Some(1) { "fahrenheit" } else { "celsius" },
        mode: z.mode.and_then(OperationMode::from_code),
        modes: system_modes.cloned().unwrap_or_default(),
        is_master,
        speed: z.speed,
        speeds,
        sleep: z.sleep,
        double_setpoint: z.double_setpoint.unwrap_or(0) != 0,
        cool_setpoint: z.cool_setpoint,
        heat_setpoint: z.heat_setpoint,
        demand: z.air_demand.unwrap_or(0) != 0 || z.floor_demand.unwrap_or(0) != 0,
        battery_low: z.battery == Some(0),
        errors,
    }
}

/// A node this server consumes.
struct NodeState {
    node: db::Node,
    /// `None` when the stored key cannot be decrypted (server secret changed).
    client: Option<NodeClient>,
    status: NodeStatus,
    failures: u32,
    /// Last time `last_seen` was written to the database.
    persisted_at: i64,
}

impl NodeState {
    fn new(node: db::Node, me: Option<&(String, SigningKey)>) -> Self {
        // Requests to the node are signed with this node's key (see `federation`).
        let client = me.map(|(node_id, key)| NodeClient::new(&node.url, node_id, key.clone()));
        let status = NodeStatus {
            state: "unknown",
            last_seen: node.last_seen,
            last_error: node.last_error.clone(),
            latency_ms: None,
            permission: node.permission.clone(),
        };
        Self { node, client, status, failures: 0, persisted_at: 0 }
    }

    fn reference(&self) -> NodeRef {
        NodeRef { id: self.node.id, name: self.node.name.clone(), state: self.status.state }
    }
}

pub struct Hub {
    db: Db,
    cfg: Arc<Config>,
    /// Key decrypting the stored node keys.
    secret: Arc<[u8; 32]>,
    state: RwLock<HashMap<i64, InstState>>,
    nodes: RwLock<HashMap<i64, NodeState>>,
    tx: broadcast::Sender<HubMessage>,
}

impl Hub {
    pub fn new(db: Db, cfg: Arc<Config>, secret: Arc<[u8; 32]>) -> Arc<Self> {
        let (tx, _) = broadcast::channel(256);
        Arc::new(Self { db, cfg, secret, state: RwLock::new(HashMap::new()), nodes: RwLock::new(HashMap::new()), tx })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<HubMessage> {
        self.tx.subscribe()
    }

    fn send(&self, msg: HubMessage) {
        let _ = self.tx.send(msg);
    }

    /// Synchronizes the in-memory state with the installations table.
    pub async fn reload_installations(&self) -> anyhow::Result<()> {
        let installations = db::list_installations(&self.db).await?;
        let mut metas = HashMap::new();
        for inst in &installations {
            metas.insert(inst.id, db::list_zone_meta(&self.db, inst.id).await?);
        }
        // Nodes first: installations reached through a node show its name and state.
        let nodes = db::list_nodes(&self.db).await?;
        let me = if nodes.is_empty() {
            None
        } else {
            let id = crate::federation::identity(&self.db).await?.node_id;
            Some((id, crate::federation::signing_key(&self.db, &self.secret).await?))
        };
        let node_refs: HashMap<i64, (NodeRef, bool)> = {
            let mut states = self.nodes.write().await;
            let ids: Vec<i64> = nodes.iter().map(|n| n.id).collect();
            states.retain(|id, _| ids.contains(id));
            for n in nodes {
                match states.get_mut(&n.id) {
                    Some(existing) if existing.node.url == n.url && existing.client.is_some() => {
                        existing.status.permission = n.permission.clone();
                        existing.node = n;
                    }
                    _ => {
                        states.insert(n.id, NodeState::new(n, me.as_ref()));
                    }
                }
            }
            states.values().map(|n| (n.node.id, (n.reference(), n.status.permission == "control"))).collect()
        };
        let mut changed = vec![];
        let mut removed = vec![];
        {
            let mut state = self.state.write().await;
            let ids: Vec<i64> = installations.iter().map(|i| i.id).collect();
            state.retain(|id, _| {
                let keep = ids.contains(id);
                if !keep {
                    removed.push(*id);
                }
                keep
            });
            for inst in installations {
                let meta = metas.remove(&inst.id).unwrap_or_default();
                let (node, controllable) = match inst.node_id {
                    Some(node_id) => match node_refs.get(&node_id) {
                        Some((r, control)) => (Some(r.clone()), *control),
                        None => continue,
                    },
                    None => (None, true),
                };
                match state.get_mut(&inst.id) {
                    Some(existing)
                        if existing.inst.host == inst.host
                            && existing.inst.port == inst.port
                            && existing.inst.node_id == inst.node_id
                            && existing.inst.remote_id == inst.remote_id =>
                    {
                        existing.inst = inst;
                        existing.meta = meta.into_iter().map(|m| ((m.system_id, m.zone_id), m)).collect();
                        existing.node = node;
                        existing.controllable = controllable;
                        changed.push(existing.view());
                    }
                    _ => {
                        let s = InstState::new(inst, meta, node, controllable);
                        changed.push(s.view());
                        state.insert(s.inst.id, s);
                    }
                }
            }
        }
        for id in removed {
            self.send(HubMessage::InstallationRemoved { id });
        }
        for view in changed {
            self.send(HubMessage::Installation { installation: view });
        }
        Ok(())
    }

    pub async fn snapshot(&self) -> Vec<InstallationView> {
        let state = self.state.read().await;
        let mut views: Vec<InstallationView> = state.values().map(InstState::view).collect();
        views.sort_by_key(|v| v.id);
        views
    }

    pub async fn installation(&self, id: i64) -> Option<InstallationView> {
        self.state.read().await.get(&id).map(InstState::view)
    }

    pub async fn zone(&self, id: i64, system_id: i64, zone_id: i64) -> Option<ZoneView> {
        self.installation(id)
            .await
            .and_then(|i| i.zones.into_iter().find(|z| z.system_id == system_id && z.zone_id == zone_id))
    }

    /// MAC address of the WebServer (read once, then stored).
    pub async fn webserver_mac(&self, id: i64) -> Option<String> {
        self.read_webserver_mac(id, false).await
    }

    /// MAC address read from the WebServer now (its address may have changed); the stored one
    /// if it does not answer.
    pub async fn current_webserver_mac(&self, id: i64) -> Option<String> {
        self.read_webserver_mac(id, true).await
    }

    async fn read_webserver_mac(&self, id: i64, fresh: bool) -> Option<String> {
        let (backend, known) = {
            let state = self.state.read().await;
            let s = state.get(&id)?;
            (s.backend.clone(), s.inst.mac.clone())
        };
        let known = known.filter(|m| !m.is_empty());
        // Installations reached through a node: the MAC address comes from the node.
        let Backend::Local(client) = backend else { return known };
        if !fresh {
            if let Some(mac) = known {
                return Some(mac);
            }
        }
        let Some(info) = client.webserver_info().await.ok() else { return known };
        let Some(mac) = info.get("mac").and_then(Value::as_str).map(String::from) else { return known };
        if let Err(e) = db::set_installation_mac(&self.db, id, &mac).await {
            tracing::error!("cannot store MAC address: {e}");
        }
        if let Some(s) = self.state.write().await.get_mut(&id) {
            s.inst.mac = Some(mac.clone());
        }
        Some(mac)
    }

    /// Writes an entry in the activity feed and pushes it to clients.
    pub async fn log(&self, kind: &str, level: &str, installation_id: Option<i64>, actor: Option<&str>, message: &str, data: Option<Value>) {
        match db::insert_event(&self.db, kind, level, installation_id, actor, message, data.as_ref()).await {
            Ok(event) => self.send(HubMessage::Event { event }),
            Err(e) => tracing::error!("cannot record event: {e}"),
        }
        match level {
            "error" => tracing::error!("[{kind}] {message}"),
            "warn" => tracing::warn!("[{kind}] {message}"),
            _ => tracing::info!("[{kind}] {message}"),
        }
    }

    pub fn routines_changed(&self) {
        self.send(HubMessage::RoutinesChanged);
    }

    // -----------------------------------------------------------------------------------
    // Polling
    // -----------------------------------------------------------------------------------

    pub async fn poll_all(self: &Arc<Self>) {
        let ids: Vec<i64> = {
            let state = self.state.read().await;
            state.values().filter(|s| s.inst.enabled && s.node_id().is_none()).map(|s| s.inst.id).collect()
        };
        // One request per node, for all the installations it exposes.
        let node_ids: Vec<i64> = self.nodes.read().await.keys().copied().collect();
        let polls = ids.into_iter().map(|id| {
            let hub = self.clone();
            async move { hub.poll_one(id).await }
        });
        let node_polls = node_ids.into_iter().map(|id| {
            let hub = self.clone();
            async move { hub.poll_node(id).await }
        });
        futures_util::future::join(futures_util::future::join_all(polls), futures_util::future::join_all(node_polls)).await;
    }

    pub async fn poll_one(&self, id: i64) {
        let (backend, commands) = match self.state.read().await.get(&id) {
            Some(s) => (s.backend.clone(), s.commands),
            None => return,
        };
        let client = match backend {
            Backend::Local(client) => client,
            Backend::Node { node_id, .. } => return self.poll_node(node_id).await,
        };
        let started = Instant::now();
        let result = client.read_all().await;
        let latency = started.elapsed().as_millis() as u64;

        let mut logs: Vec<(&'static str, String)> = vec![];
        let mut to_record: Vec<ZoneData> = vec![];
        let view = {
            let mut state = self.state.write().await;
            let Some(s) = state.get_mut(&id) else { return };
            let before_conn = s.conn.clone();
            let before_zones = s.zones.clone();
            match result {
                Ok(resp) => {
                    if s.conn.state != "online" {
                        logs.push(("info", format!("Connexion établie avec « {} » ({}:{})", s.inst.name, s.inst.host, s.inst.port)));
                    }
                    s.failures = 0;
                    s.conn = ConnectionView { state: "online", last_seen: Some(db::now()), last_error: None, latency_ms: Some(latency) };
                    // A command was sent while this read was in flight: its values are stale.
                    if s.commands == commands {
                        to_record = s.apply_zones(resp.zones(), self.cfg.history_interval_secs);
                    }
                }
                Err(e) => {
                    s.failures += 1;
                    s.conn.last_error = Some(e.to_string());
                    if s.failures >= OFFLINE_AFTER_FAILURES && s.conn.state != "offline" {
                        s.conn.state = "offline";
                        logs.push(("warn", format!("Connexion perdue avec « {} » : {e}", s.inst.name)));
                    }
                }
            }
            let conn_changed = before_conn.state != s.conn.state || before_conn.last_error != s.conn.last_error;
            (conn_changed || before_zones != s.zones).then(|| s.view())
        };

        self.record(id, to_record).await;
        for (level, msg) in logs {
            self.log("connection", level, Some(id), None, &msg, None).await;
        }
        if let Some(view) = view {
            self.send(HubMessage::Installation { installation: view });
        }
    }

    async fn record(&self, id: i64, zones: Vec<ZoneData>) {
        for z in zones {
            if let Err(e) = db::insert_reading(&self.db, id, z.system_id, z.zone_id, z.room_temp, z.humidity.filter(|h| *h > 0), z.setpoint, z.is_on(), z.mode).await {
                tracing::error!("cannot record history: {e}");
            }
        }
    }

    // -----------------------------------------------------------------------------------
    // Federation: installations reached through other nodes (ADR 0008)
    // -----------------------------------------------------------------------------------

    /// Connection state of the nodes this server consumes.
    pub async fn node_statuses(&self) -> HashMap<i64, NodeStatus> {
        self.nodes.read().await.iter().map(|(id, n)| (*id, n.status.clone())).collect()
    }

    /// What this server exposes to a share: its local installations among `ids`.
    pub async fn federated(&self, ids: &[i64]) -> Vec<FedInstallation> {
        let state = self.state.read().await;
        let mut out: Vec<FedInstallation> = ids
            .iter()
            .filter_map(|id| state.get(id))
            .filter(|s| s.node_id().is_none() && s.inst.enabled)
            .map(InstState::federated)
            .collect();
        out.sort_by_key(|i| i.id);
        out
    }

    /// Reads the installations a node exposes, creates/removes the matching installations
    /// here, and updates their state.
    pub async fn poll_node(&self, node_id: i64) {
        let (client, node_name) = match self.nodes.read().await.get(&node_id) {
            Some(n) => (n.client.clone(), n.node.name.clone()),
            None => return,
        };
        // Commands sent while the snapshot is in flight make its values stale (see poll_one).
        let commands: HashMap<i64, u64> = self
            .state
            .read()
            .await
            .values()
            .filter(|s| s.node_id() == Some(node_id))
            .map(|s| (s.inst.id, s.commands))
            .collect();
        let started = Instant::now();
        let result = match client {
            Some(c) => c.snapshot().await,
            None => Err(NodeError::Rejected { status: 401, message: "clé illisible : ajoutez de nouveau ce nœud".into() }),
        };
        let latency = started.elapsed().as_millis() as u64;
        match result {
            Ok(snap) => self.node_ok(node_id, &node_name, snap, latency, &commands).await,
            Err(e) => self.node_failed(node_id, &node_name, e).await,
        }
    }

    async fn node_ok(&self, node_id: i64, node_name: &str, snap: FedSnapshot, latency: u64, commands: &HashMap<i64, u64>) {
        // 1. Node state.
        let now = db::now();
        let (was, persist, permission_changed) = {
            let mut nodes = self.nodes.write().await;
            let Some(n) = nodes.get_mut(&node_id) else { return };
            let was = n.status.state;
            let permission_changed = n.status.permission != snap.permission;
            n.failures = 0;
            n.status = NodeStatus { state: "online", last_seen: Some(now), last_error: None, latency_ms: Some(latency), permission: snap.permission.clone() };
            n.node.permission = snap.permission.clone();
            let persist = was != "online" || permission_changed || now - n.persisted_at >= 60;
            if persist {
                n.persisted_at = now;
            }
            (was, persist, permission_changed)
        };
        if persist {
            if let Err(e) = db::set_node_seen(&self.db, node_id, Some(&snap.name), Some(&snap.permission), None).await {
                tracing::error!("cannot store node state: {e}");
            }
        }
        if was != "online" {
            self.log("connection", "info", None, None, &format!("Nœud « {node_name} » connecté"), None).await;
        }

        // 2. Installations exposed by the node.
        let list_changed = match self.sync_node_installations(node_id, node_name, &snap).await {
            Ok(changed) => changed,
            Err(e) => {
                tracing::error!("cannot update the installations of node {node_name}: {e:#}");
                false
            }
        };
        if list_changed || permission_changed || was != "online" {
            if let Err(e) = self.reload_installations().await {
                tracing::error!("cannot reload installations: {e:#}");
            }
        }

        // 3. Their state.
        let mut views = vec![];
        let mut records = vec![];
        {
            let mut state = self.state.write().await;
            for fed in snap.installations {
                let Some(s) = state.values_mut().find(|s| matches!(s.backend, Backend::Node { node_id: n, remote_id } if n == node_id && remote_id == fed.id)) else {
                    continue;
                };
                if !s.inst.enabled {
                    continue;
                }
                let stale = commands.get(&s.inst.id).is_some_and(|c| *c != s.commands);
                let before = (s.conn.clone(), s.zones.clone(), s.remote_meta.clone());
                s.remote_meta = fed.meta.into_iter().map(|m| ((m.system_id, m.zone_id), m)).collect();
                s.failures = 0;
                s.conn = ConnectionView {
                    state: match fed.connection.state.as_str() {
                        "online" => "online",
                        "offline" => "offline",
                        _ => "unknown",
                    },
                    last_seen: fed.connection.last_seen,
                    last_error: fed.connection.last_error,
                    latency_ms: Some(latency),
                };
                if stale {
                    // A command was sent meanwhile: keep the values it set.
                } else if s.conn.state == "online" {
                    records.push((s.inst.id, s.apply_zones(fed.zones, self.cfg.history_interval_secs)));
                } else {
                    s.zones = fed.zones;
                }
                let after = (&s.conn, &s.zones, &s.remote_meta);
                if before.0.state != after.0.state || before.0.last_error != after.0.last_error || &before.1 != after.1 || &before.2 != after.2 {
                    views.push(s.view());
                }
            }
        }
        for (id, zones) in records {
            self.record(id, zones).await;
        }
        for view in views {
            self.send(HubMessage::Installation { installation: view });
        }
    }

    async fn node_failed(&self, node_id: i64, node_name: &str, e: NodeError) {
        let message = e.to_string();
        let (changed, state_now) = {
            let mut nodes = self.nodes.write().await;
            let Some(n) = nodes.get_mut(&node_id) else { return };
            n.failures += 1;
            let was = n.status.state;
            // A refused key is final; a network error must repeat before the node is offline.
            if e.state() != "offline" || n.failures >= OFFLINE_AFTER_FAILURES {
                n.status.state = e.state();
            }
            n.status.last_error = Some(message.clone());
            n.status.latency_ms = None;
            (was != n.status.state, n.status.state)
        };
        if changed {
            let _ = db::set_node_seen(&self.db, node_id, None, None, Some(&message)).await;
            let text = match state_now {
                "revoked" => format!("Nœud « {node_name} » : la clé de partage a été révoquée"),
                "expired" => format!("Nœud « {node_name} » : le partage a expiré"),
                "pending" => format!("Nœud « {node_name} » : en attente d'acceptation sur ce serveur (Réglages › Partages)"),
                _ => format!("Nœud « {node_name} » injoignable : {message}"),
            };
            self.log("connection", "warn", None, None, &text, None).await;
        }
        let mut views = vec![];
        {
            let mut state = self.state.write().await;
            for s in state.values_mut().filter(|s| s.node_id() == Some(node_id)) {
                s.failures += 1;
                let before = s.conn.clone();
                s.conn.last_error = Some(format!("via « {node_name} » : {message}"));
                s.conn.latency_ms = None;
                if state_now != "online" {
                    s.conn.state = "offline";
                }
                if let Some(r) = s.node.as_mut() {
                    r.state = state_now;
                }
                if changed || before.state != s.conn.state {
                    views.push(s.view());
                }
            }
        }
        for view in views {
            self.send(HubMessage::Installation { installation: view });
        }
    }

    /// Creates the installations a node exposes (unless their WebServer is already driven
    /// here) and deletes the ones it no longer exposes. Returns whether anything changed.
    async fn sync_node_installations(&self, node_id: i64, node_name: &str, snap: &FedSnapshot) -> anyhow::Result<bool> {
        let all = db::list_installations(&self.db).await?;
        let local_macs: HashSet<String> = all.iter().filter(|i| i.is_local()).filter_map(|i| i.mac.as_deref().map(normalize_mac)).collect();
        let Some(node) = db::node_by_id(&self.db, node_id).await? else { return Ok(false) };
        let mine: Vec<&Installation> = all.iter().filter(|i| i.node_id == Some(node_id)).collect();
        let mut changed = false;
        for fed in &snap.installations {
            let driven_here = fed.mac.as_deref().map(normalize_mac).is_some_and(|m| local_macs.contains(&m));
            let existing = mine.iter().find(|i| i.remote_id == Some(fed.id));
            match (existing, driven_here) {
                (None, false) => {
                    let inst = db::insert_remote_installation(&self.db, &fed.name, node_id, fed.id, &node.url, fed.mac.as_deref(), fed.location.as_deref()).await?;
                    self.log("system", "info", Some(inst.id), None, &format!("Installation « {} » ajoutée via le nœud « {node_name} »", fed.name), None).await;
                    changed = true;
                }
                (Some(i), true) => {
                    // Its WebServer is on this network now: drive it directly.
                    db::delete_installation(&self.db, i.id).await?;
                    changed = true;
                }
                (Some(i), false) if i.mac.is_none() && fed.mac.is_some() => {
                    db::set_installation_mac(&self.db, i.id, fed.mac.as_deref().unwrap_or_default()).await?;
                }
                _ => {}
            }
        }
        for i in mine.iter().filter(|i| !snap.installations.iter().any(|f| Some(f.id) == i.remote_id)) {
            db::delete_installation(&self.db, i.id).await?;
            self.log("system", "warn", None, None, &format!("« {} » n'est plus partagée par le nœud « {node_name} »", i.name), None).await;
            changed = true;
        }
        Ok(changed)
    }

    /// Background poller.
    pub async fn run_poller(self: Arc<Self>) {
        let mut ticker = tokio::time::interval(Duration::from_secs(self.cfg.poll_interval_secs));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            self.poll_all().await;
        }
    }

    /// Hourly cleanup of old history, events and expired sessions.
    pub async fn run_maintenance(self: Arc<Self>) {
        let mut ticker = tokio::time::interval(Duration::from_secs(3600));
        loop {
            ticker.tick().await;
            if let Err(e) = db::purge(&self.db, self.cfg.history_retention_days, self.cfg.events_retention_days).await {
                tracing::error!("maintenance failed: {e}");
            }
        }
    }

    // -----------------------------------------------------------------------------------
    // Commands
    // -----------------------------------------------------------------------------------

    /// Applies a command on a zone and returns the updated zone.
    pub async fn control(&self, id: i64, system_id: i64, zone_id: i64, cmd: &ZoneCommand, actor: &str) -> ApiResult<ZoneView> {
        if cmd.is_empty() {
            return Err(ApiError::bad_request("empty command"));
        }
        let remote = {
            let mut state = self.state.write().await;
            let s = state.get_mut(&id).ok_or_else(|| ApiError::not_found("installation not found"))?;
            if !s.inst.enabled {
                return Err(ApiError::bad_request("installation is disabled"));
            }
            // Any poll already in flight now carries stale values (see `poll_one`).
            s.commands += 1;
            match s.backend {
                Backend::Node { node_id, remote_id } => Some((node_id, remote_id, s.inst.name.clone(), s.controllable)),
                Backend::Local(_) => None,
            }
        };
        if let Some((node_id, remote_id, inst_name, controllable)) = remote {
            if !controllable {
                return Err(ApiError::forbidden("cette installation est partagée en lecture seule par son nœud"));
            }
            return self.control_remote(id, node_id, remote_id, &inst_name, system_id, zone_id, cmd, actor).await;
        }
        let (client, zone, master_id, inst_name) = {
            let state = self.state.read().await;
            let s = state.get(&id).ok_or_else(|| ApiError::not_found("installation not found"))?;
            let Backend::Local(client) = &s.backend else { return Err(ApiError::internal("unexpected backend")) };
            let zone = s
                .zones
                .iter()
                .find(|z| z.system_id == system_id && z.zone_id == zone_id)
                .cloned()
                .ok_or_else(|| ApiError::not_found("zone not found (installation not reachable yet?)"))?;
            let master_id = master_zone(&s.zones, system_id);
            (client.clone(), zone, master_id, s.inst.name.clone())
        };

        let (zone_params, mode_param) = build_params(cmd, &zone)?;
        let zone_name = zone.name.clone().unwrap_or_else(|| format!("Zone {zone_id}"));

        let mut updated: Vec<ZoneData> = vec![];
        let mut mode_applied = false;
        let outcome: Result<(), AirzoneError> = async {
            if let Some(mode) = mode_param {
                // The operation mode is system-wide and can only be written on the master zone.
                let target = master_id.unwrap_or(zone_id);
                let mut p = Map::new();
                p.insert("mode".into(), json!(mode.code()));
                updated.extend(client.write(system_id, target, &p).await?.zones());
                mode_applied = true;
            }
            if !zone_params.is_empty() {
                updated.extend(client.write(system_id, zone_id, &zone_params).await?.zones());
            }
            Ok(())
        }
        .await;

        if let Err(e) = outcome {
            // The mode may already be applied (two writes): keep the cache true to the system.
            let message = if mode_applied {
                let view = {
                    let mut state = self.state.write().await;
                    state.get_mut(&id).map(|s| {
                        for z in s.zones.iter_mut().filter(|z| z.system_id == system_id) {
                            z.mode = mode_param.map(OperationMode::code);
                        }
                        s.view()
                    })
                };
                if let Some(view) = view {
                    self.send(HubMessage::Installation { installation: view });
                }
                format!("Commande partielle sur {zone_name} ({inst_name}) : mode appliqué, le reste a échoué : {e}")
            } else {
                format!("Échec de la commande sur {zone_name} ({inst_name}) : {e}")
            };
            self.log("command", "error", Some(id), Some(actor), &message, Some(json!({ "system_id": system_id, "zone_id": zone_id, "command": cmd }))).await;
            return Err(if mode_applied { ApiError::bad_gateway(format!("mode appliqué, mais le reste de la commande a échoué : {e}")) } else { e.into() });
        }

        // Merge what the WebServer returned into the cache.
        let view = {
            let mut state = self.state.write().await;
            if let Some(s) = state.get_mut(&id) {
                for nz in updated {
                    if let Some(z) = s.zones.iter_mut().find(|z| z.system_id == nz.system_id && z.zone_id == nz.zone_id) {
                        *z = merge_zone(z, &nz);
                    }
                }
                if let Some(mode) = mode_param {
                    for z in s.zones.iter_mut().filter(|z| z.system_id == system_id) {
                        z.mode = Some(mode.code());
                    }
                }
                Some(s.view())
            } else {
                None
            }
        };
        self.log(
            "command",
            "info",
            Some(id),
            Some(actor),
            &format!("{zone_name} ({inst_name}) : {}", cmd.describe()),
            Some(json!({ "system_id": system_id, "zone_id": zone_id, "command": cmd })),
        )
        .await;
        if let Some(view) = view {
            self.send(HubMessage::Installation { installation: view.clone() });
            if let Some(z) = view.zones.into_iter().find(|z| z.system_id == system_id && z.zone_id == zone_id) {
                return Ok(z);
            }
        }
        self.zone(id, system_id, zone_id).await.ok_or_else(|| ApiError::not_found("zone not found"))
    }

    /// Sends a command to the node exposing the installation; it validates and applies it.
    #[allow(clippy::too_many_arguments)]
    async fn control_remote(&self, id: i64, node_id: i64, remote_id: i64, inst_name: &str, system_id: i64, zone_id: i64, cmd: &ZoneCommand, actor: &str) -> ApiResult<ZoneView> {
        let (client, node_name) = {
            let nodes = self.nodes.read().await;
            let n = nodes.get(&node_id).ok_or_else(|| ApiError::not_found("node not found"))?;
            (n.client.clone(), n.node.name.clone())
        };
        let client = client.ok_or_else(|| ApiError::bad_gateway(format!("clé du nœud « {node_name} » illisible : ajoutez-le de nouveau")))?;
        let zone_label = self.zone(id, system_id, zone_id).await.map(|z| z.name).unwrap_or_else(|| format!("Zone {zone_id}"));
        let data = Some(json!({ "system_id": system_id, "zone_id": zone_id, "command": cmd, "node": node_name }));
        match client.control(remote_id, system_id, zone_id, cmd, actor).await {
            Ok(fed) => {
                let view = {
                    let mut state = self.state.write().await;
                    state.get_mut(&id).map(|s| {
                        s.zones = fed.zones;
                        s.view()
                    })
                };
                self.log("command", "info", Some(id), Some(actor), &format!("{zone_label} ({inst_name}, via {node_name}) : {}", cmd.describe()), data).await;
                let view = view.ok_or_else(|| ApiError::not_found("installation not found"))?;
                self.send(HubMessage::Installation { installation: view.clone() });
                view.zones.into_iter().find(|z| z.system_id == system_id && z.zone_id == zone_id).ok_or_else(|| ApiError::not_found("zone not found"))
            }
            Err(e) => {
                self.log("command", "error", Some(id), Some(actor), &format!("Échec de la commande sur {zone_label} ({inst_name}, via {node_name}) : {e}"), data).await;
                Err(e.into())
            }
        }
    }

    /// Updates Breezone-side metadata of a zone (custom name, group, position).
    pub async fn set_zone_meta(&self, mut meta: ZoneMeta) -> ApiResult<Option<ZoneView>> {
        // Installation reached through another node: a value equal to the one that node gives is
        // not an override (later changes over there keep showing here).
        if let Some(remote) = self.state.read().await.get(&meta.installation_id).and_then(|s| s.remote_meta.get(&(meta.system_id, meta.zone_id)).cloned()) {
            if meta.display_name.is_some() && meta.display_name == remote.display_name {
                meta.display_name = None;
            }
            if meta.group_name.is_some() && meta.group_name == remote.group {
                meta.group_name = None;
                if meta.position == remote.position {
                    meta.position = 0;
                }
            }
        }
        db::upsert_zone_meta(&self.db, &meta).await?;
        let view = {
            let mut state = self.state.write().await;
            match state.get_mut(&meta.installation_id) {
                Some(s) => {
                    s.meta.insert((meta.system_id, meta.zone_id), meta.clone());
                    Some(s.view())
                }
                None => None,
            }
        };
        if let Some(view) = view {
            self.send(HubMessage::Installation { installation: view });
        }
        Ok(self.zone(meta.installation_id, meta.system_id, meta.zone_id).await)
    }
}

/// Keeps the fields the WebServer did not return in a partial update.
fn merge_zone(old: &ZoneData, new: &ZoneData) -> ZoneData {
    let mut old_v = serde_json::to_value(old).unwrap_or_default();
    if let (Value::Object(o), Value::Object(n)) = (&mut old_v, serde_json::to_value(new).unwrap_or_default()) {
        for (k, v) in n {
            if !v.is_null() {
                o.insert(k, v);
            }
        }
    }
    serde_json::from_value(old_v).unwrap_or_else(|_| new.clone())
}

/// Validates a command against the zone limits and converts it to Airzone parameters.
/// Returns the zone parameters and, separately, the mode (written on the master zone).
fn build_params(cmd: &ZoneCommand, zone: &ZoneData) -> ApiResult<(Map<String, Value>, Option<OperationMode>)> {
    let mut p = Map::new();
    let min = zone.min_temp.unwrap_or(5.0);
    let max = zone.max_temp.unwrap_or(35.0);
    let check_temp = |label: &str, v: f64| -> ApiResult<f64> {
        if !v.is_finite() || v < min || v > max {
            return Err(ApiError::bad_request(format!("{label} must be between {min} and {max}")));
        }
        Ok((v * 10.0).round() / 10.0)
    };
    if let Some(on) = cmd.on {
        p.insert("on".into(), json!(if on { 1 } else { 0 }));
    }
    if let Some(sp) = cmd.setpoint {
        p.insert("setpoint".into(), json!(check_temp("setpoint", sp)?));
    }
    if let Some(sp) = cmd.cool_setpoint {
        p.insert("coolsetpoint".into(), json!(check_temp("cool_setpoint", sp)?));
    }
    if let Some(sp) = cmd.heat_setpoint {
        p.insert("heatsetpoint".into(), json!(check_temp("heat_setpoint", sp)?));
    }
    if let Some(speed) = cmd.speed {
        if !(0..=10).contains(&speed) {
            return Err(ApiError::bad_request("invalid fan speed"));
        }
        p.insert("speed".into(), json!(speed));
    }
    if let Some(sleep) = cmd.sleep {
        if ![0, 30, 60, 90].contains(&sleep) {
            return Err(ApiError::bad_request("sleep must be 0, 30, 60 or 90 minutes"));
        }
        p.insert("sleep".into(), json!(sleep));
    }
    Ok((p, cmd.mode))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone() -> ZoneData {
        ZoneData { system_id: 1, zone_id: 2, min_temp: Some(15.0), max_temp: Some(30.0), ..Default::default() }
    }

    #[test]
    fn builds_airzone_params() {
        let cmd = ZoneCommand { on: Some(true), setpoint: Some(21.04), mode: Some(OperationMode::Heating), ..Default::default() };
        let (p, mode) = build_params(&cmd, &zone()).unwrap();
        assert_eq!(p["on"], json!(1));
        assert_eq!(p["setpoint"], json!(21.0));
        assert_eq!(mode, Some(OperationMode::Heating));
    }

    #[test]
    fn rejects_out_of_range_setpoint() {
        let cmd = ZoneCommand { setpoint: Some(40.0), ..Default::default() };
        assert!(build_params(&cmd, &zone()).is_err());
        let cmd = ZoneCommand { sleep: Some(45), ..Default::default() };
        assert!(build_params(&cmd, &zone()).is_err());
    }

    #[test]
    fn merge_keeps_missing_fields() {
        let old = ZoneData { system_id: 1, zone_id: 1, name: Some("Salon".into()), setpoint: Some(20.0), on: Some(1), ..Default::default() };
        let new = ZoneData { system_id: 1, zone_id: 1, setpoint: Some(22.0), ..Default::default() };
        let m = merge_zone(&old, &new);
        assert_eq!(m.name.as_deref(), Some("Salon"));
        assert_eq!(m.setpoint, Some(22.0));
        assert_eq!(m.on, Some(1));
    }

    #[test]
    fn describes_commands_in_french() {
        let cmd = ZoneCommand { on: Some(false), setpoint: Some(19.5), ..Default::default() };
        assert_eq!(cmd.describe(), "éteinte, consigne 19.5°");
    }
}
