//! Host of the modules compiled to WebAssembly (docs/roadmap/modules.md § 4.5): one engine for the
//! server, a module compiled once (and kept compiled on disk), one instance per source.
//!
//! A module sees nothing of the system: no files, no environment, no sockets of WASI. It reaches
//! the network only through `host.http` and `host.subscribe`, which refuse any address its
//! permissions do not allow. Each call has a deadline and a memory limit; a module that traps
//! (panic, loop, memory) only puts its source in error, and is instantiated again at the next call.
//! What the hardware says without being asked comes through subscriptions: the host keeps the
//! connection and hands each message to the module's `on-message`, between its other calls.

use crate::drivers::{Described, DeviceInfo, Driver, DriverError, DriverFuture, Forecast, HardwareRoom, Scene, Sink, State};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::Mutex;
use wasmtime::component::{Component, Linker, ResourceAny, ResourceTable};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

wasmtime::component::bindgen!({
    path: "contracts/wit",
    // Without `extras`: a module built before one of them was added still loads.
    world: "core",
    imports: {
        "breezone:module/host.http": async,
        "breezone:module/host.subscribe": async,
        "breezone:module/host.subscribe-with": async,
        "breezone:module/host.store-get": async,
        "breezone:module/host.store-set": async,
        "breezone:module/host.account": async,
        "breezone:module/host.exchange": async,
        "breezone:module/host.wait": async,
        "breezone:module/host.access-token": async,
        "breezone:module/host.account-update": async,
        "breezone:module/host.receive": async,
        default: trappable,
    },
    exports: { default: async },
});

type Module = Core;

/// The interface of the calls added after 1.0, looked up function by function.
const EXTRAS: &str = "breezone:module/extras@1.0.0";

use breezone::module::types::{Error as WitError, Header, ReadUntil, Request, Response};

/// Ticks of the engine's epoch: one each `TICK`.
const TICK: Duration = Duration::from_millis(100);
/// Longest time a call into a module may compute (time spent waiting in the host excluded).
const CALL_COMPUTE: Duration = Duration::from_secs(10);
/// The same, in ticks: when the engine looks at what was computed.
const CALL_TICKS: u64 = 100;
/// Longest time a call may take, waits for the network included.
const CALL_TIMEOUT: Duration = Duration::from_secs(60);
/// Largest body of an HTTP answer handed to a module.
const MAX_BODY: usize = 4 << 20;
/// Subscriptions one source may keep open.
const MAX_SUBSCRIPTIONS: usize = 4;
/// Largest message of a subscription handed to a module.
const MAX_MESSAGE: usize = 1 << 20;
/// Largest value of the store of a source.
const MAX_STORED: usize = 64 << 10;

/// What a module may reach.
#[derive(Debug, Clone, Default)]
pub struct Permissions {
    /// The addresses of the local network (private IPv4, unique-local and link-local IPv6).
    pub lan: bool,
    /// Hosts of the Internet, by name (`api.netatmo.com`).
    pub hosts: Vec<String>,
    /// The machine itself: tests and development only (it would reach Breezone's own API).
    pub loopback: bool,
    pub memory_mb: usize,
    /// The accounts whose secret it may read (`requires.accounts` of its manifest).
    pub accounts: Vec<String>,
    /// Where the access of its own OAuth account is renewed (`oauth.token_url` of its manifest).
    pub oauth_token_url: Option<String>,
    /// The fields of its account's form: once the account is kept, the module may not change them.
    pub account_fields: Vec<String>,
}

impl Permissions {
    fn allows_ip(&self, ip: IpAddr) -> bool {
        if ip.is_loopback() {
            return self.loopback;
        }
        self.lan
            && match ip {
                IpAddr::V4(v4) => v4.is_private() || v4.is_link_local(),
                IpAddr::V6(v6) => (v6.segments()[0] & 0xfe00) == 0xfc00 || (v6.segments()[0] & 0xffc0) == 0xfe80,
            }
    }

    /// A host of the Internet it declared: by name, or `*.example.com` for its subdomains.
    fn allows_name(&self, host: &str) -> bool {
        let host = host.to_ascii_lowercase();
        self.hosts.iter().any(|h| {
            let h = h.to_ascii_lowercase();
            match h.strip_prefix("*.") {
                Some(domain) => host.ends_with(&format!(".{domain}")),
                None => h == host,
            }
        })
    }

    /// Where `url` may be reached: the addresses to connect to, all allowed (a name is resolved here,
    /// once, so that it cannot point elsewhere between the check and the connection).
    async fn resolve(&self, url: &reqwest::Url) -> Result<Vec<SocketAddr>, String> {
        let host = url.host_str().ok_or("no host")?.trim_start_matches('[').trim_end_matches(']').to_string();
        let port = url.port_or_known_default().ok_or("no port")?;
        if let Ok(ip) = host.parse::<IpAddr>() {
            return if self.allows_ip(ip) { Ok(vec![SocketAddr::new(ip, port)]) } else { Err(format!("{host}: not allowed for this module")) };
        }
        let named = self.allows_name(&host);
        let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), port)).await.map_err(|e| format!("{host}: {e}"))?.collect();
        if addrs.is_empty() {
            return Err(format!("{host}: not found"));
        }
        // A declared host of the Internet, or a local name whose every address is allowed.
        if named || addrs.iter().all(|a| self.allows_ip(a.ip())) {
            Ok(addrs)
        } else {
            Err(format!("{host}: not allowed for this module"))
        }
    }
}

/// The messages of a source's subscriptions, in the order they came: taken by `receive` during a
/// call of the module, else handed to `on-message` by the pump between calls.
#[derive(Default)]
struct Inbox {
    queue: std::sync::Mutex<std::collections::VecDeque<(u32, Vec<u8>)>>,
    arrived: tokio::sync::Notify,
    /// Its subscriptions are all gone (the source is dropped).
    closed: std::sync::atomic::AtomicBool,
}

/// Messages a source keeps waiting at most: the oldest go first.
const MAX_WAITING: usize = 1024;

impl Inbox {
    fn push(&self, message: (u32, Vec<u8>)) {
        {
            let mut q = self.queue.lock().unwrap_or_else(|e| e.into_inner());
            if q.len() >= MAX_WAITING {
                q.pop_front();
            }
            q.push_back(message);
        }
        self.arrived.notify_waiters();
    }

    fn pop(&self) -> Option<(u32, Vec<u8>)> {
        self.queue.lock().unwrap_or_else(|e| e.into_inner()).pop_front()
    }

    /// The first message waiting of a subscription.
    fn take(&self, subscription: u32) -> Option<Vec<u8>> {
        let mut q = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        let at = q.iter().position(|(s, _)| *s == subscription)?;
        q.remove(at).map(|(_, data)| data)
    }

    fn is_empty(&self) -> bool {
        self.queue.lock().unwrap_or_else(|e| e.into_inner()).is_empty()
    }
}

/// What a source shares with the host functions of its instances: who it is, where it tells what
/// it hears, and the messages of its subscriptions, handed to `on-message` by its pump.
struct SourceCtx {
    module: Arc<str>,
    /// The source (`None` while its configuration is only checked: its store is then in memory).
    source_id: Option<i64>,
    sink: std::sync::Mutex<Option<Sink>>,
    messages: tokio::sync::mpsc::UnboundedSender<(u32, Vec<u8>)>,
    inbox: Arc<Inbox>,
    memory_store: std::sync::Mutex<HashMap<String, Vec<u8>>>,
    /// The fields of the module's own account while it is checked, before Breezone keeps them
    /// (with a renewed OAuth access, to keep too).
    checked_account: std::sync::Mutex<Option<String>>,
}

/// An open subscription: what is sent on it, and its task (stopped with it).
struct Subscription {
    out: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    task: tokio::task::AbortHandle,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct HostState {
    permissions: Arc<Permissions>,
    ctx: Arc<SourceCtx>,
    subscriptions: HashMap<u32, Subscription>,
    next_subscription: u32,
    wasi: WasiCtx,
    table: ResourceTable,
    limits: StoreLimits,
    /// When the current call into the module began, and how long it has waited in the host since
    /// (the network, a wait): only the rest counts against `CALL_COMPUTE`.
    call_started: std::time::Instant,
    waited: Duration,
}

/// The next call into the module begins: its time to compute starts again.
fn start_call(store: &mut Store<HostState>) {
    let state = store.data_mut();
    state.call_started = std::time::Instant::now();
    state.waited = Duration::ZERO;
    store.set_epoch_deadline(CALL_TICKS);
}

impl WasiView for HostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView { ctx: &mut self.wasi, table: &mut self.table }
    }
}

/// The database and the key of the secrets, for the store of the sources and their accounts (the
/// last server started in the process: one, except in the tests).
static CONTEXT: std::sync::RwLock<Option<(crate::db::Db, [u8; 32])>> = std::sync::RwLock::new(None);

/// Gives the host what the store of the sources and the accounts need (at start).
pub fn set_context(db: crate::db::Db, key: [u8; 32]) {
    *CONTEXT.write().unwrap_or_else(|e| e.into_inner()) = Some((db, key));
}

fn context() -> Option<(crate::db::Db, [u8; 32])> {
    CONTEXT.read().unwrap_or_else(|e| e.into_inner()).clone()
}

impl HostState {
    async fn open_subscription(&mut self, url: String, headers: Vec<Header>, binary: bool) -> Result<u32, String> {
        let started = std::time::Instant::now();
        let r: Result<u32, String> = async {
            if self.subscriptions.len() >= MAX_SUBSCRIPTIONS {
                return Err(format!("at most {MAX_SUBSCRIPTIONS} subscriptions"));
            }
            self.next_subscription += 1;
            let id = self.next_subscription;
            let opening = Opening { headers: &headers, binary };
            let sub = subscribe(&self.permissions, &url, opening, id, self.ctx.messages.clone())
                .await
                .inspect_err(|e| crate::modules::log(&self.ctx.module, "warn", format!("{url}: {e}")))?;
            self.subscriptions.insert(id, sub);
            Ok(id)
        }
        .await;
        self.waited += started.elapsed();
        r
    }
}

impl breezone::module::types::Host for HostState {}

impl breezone::module::host::Host for HostState {
    fn log(&mut self, level: breezone::module::host::Level, message: String) -> wasmtime::Result<()> {
        use breezone::module::host::Level;
        let module = &*self.ctx.module;
        let level = match level {
            Level::Debug => "debug",
            Level::Info => "info",
            Level::Warn => "warn",
            Level::Error => "error",
        };
        tracing::debug!(module, level, "{message}");
        crate::modules::log(module, level, message);
        Ok(())
    }

    fn now(&mut self) -> wasmtime::Result<i64> {
        Ok(chrono::Utc::now().timestamp())
    }

    async fn wait(&mut self, ms: u32) {
        let started = std::time::Instant::now();
        let r: () = async {
            tokio::time::sleep(Duration::from_millis(u64::from(ms.min(10_000)))).await;
        }
        .await;
        self.waited += started.elapsed();
        r
    }

    async fn http(&mut self, req: Request) -> Result<Response, String> {
        let started = std::time::Instant::now();
        let r: Result<Response, String> = async {
            let url = req.url.clone();
            let r = http(&self.permissions, req).await;
            if let Err(e) = &r {
                crate::modules::log(&self.ctx.module, "warn", format!("{url}: {e}"));
            }
            r
        }
        .await;
        self.waited += started.elapsed();
        r
    }

    async fn exchange(&mut self, url: String, data: Vec<u8>, until: ReadUntil, timeout_ms: u32) -> Result<Vec<u8>, String> {
        let started = std::time::Instant::now();
        let r: Result<Vec<u8>, String> = async {
            let r = exchange(&self.permissions, &url, &data, until, timeout_ms).await;
            if let Err(e) = &r {
                crate::modules::log(&self.ctx.module, "warn", format!("{url}: {e}"));
            }
            r
        }
        .await;
        self.waited += started.elapsed();
        r
    }

    async fn subscribe(&mut self, url: String) -> Result<u32, String> {
        self.open_subscription(url, vec![], false).await
    }

    async fn subscribe_with(&mut self, url: String, headers: Vec<Header>, binary: bool) -> Result<u32, String> {
        self.open_subscription(url, headers, binary).await
    }

    async fn receive(&mut self, subscription: u32, timeout_ms: u32) -> Result<Vec<u8>, String> {
        let started = std::time::Instant::now();
        let r: Result<Vec<u8>, String> = async {
            if !self.subscriptions.contains_key(&subscription) {
                return Err("unknown subscription".into());
            }
            let deadline = tokio::time::Instant::now() + Duration::from_millis(u64::from(timeout_ms.clamp(100, 30_000)));
            let inbox = self.ctx.inbox.clone();
            loop {
                let arrived = inbox.arrived.notified();
                if let Some(data) = inbox.take(subscription) {
                    return Ok(data);
                }
                if inbox.closed.load(std::sync::atomic::Ordering::SeqCst) {
                    return Err("the subscription is closed".into());
                }
                if tokio::time::timeout_at(deadline, arrived).await.is_err() {
                    return Err("timeout".into());
                }
            }
        }
        .await;
        self.waited += started.elapsed();
        r
    }

    fn send(&mut self, subscription: u32, data: Vec<u8>) -> wasmtime::Result<Result<(), String>> {
        Ok(match self.subscriptions.get(&subscription) {
            Some(s) => s.out.send(data).map_err(|_| "the subscription is closed".to_string()),
            None => Err("unknown subscription".into()),
        })
    }

    fn unsubscribe(&mut self, subscription: u32) -> wasmtime::Result<()> {
        self.subscriptions.remove(&subscription);
        Ok(())
    }

    async fn store_get(&mut self, key: String) -> Option<Vec<u8>> {
        let started = std::time::Instant::now();
        let r: Option<Vec<u8>> = async {
            match (self.ctx.source_id, context()) {
                (Some(id), Some((db, _))) => crate::db::store_get(&db, id, &key).await.ok().flatten(),
                _ => self.ctx.memory_store.lock().unwrap_or_else(|e| e.into_inner()).get(&key).cloned(),
            }
        }
        .await;
        self.waited += started.elapsed();
        r
    }

    async fn store_set(&mut self, key: String, value: Option<Vec<u8>>) {
        let started = std::time::Instant::now();
        let r: () = async {
            if key.len() > 200 || value.as_ref().is_some_and(|v| v.len() > MAX_STORED) {
                crate::modules::log(&self.ctx.module, "warn", format!("store: {key} too large, not kept"));
                return;
            }
            match (self.ctx.source_id, context()) {
                (Some(id), Some((db, _))) => {
                    if let Err(e) = crate::db::store_set(&db, id, &key, value.as_deref()).await {
                        crate::modules::log(&self.ctx.module, "error", format!("store: {e}"));
                    }
                }
                _ => {
                    let mut store = self.ctx.memory_store.lock().unwrap_or_else(|e| e.into_inner());
                    match value {
                        Some(v) => store.insert(key, v),
                        None => store.remove(&key),
                    };
                }
            }
        }
        .await;
        self.waited += started.elapsed();
        r
    }

    async fn account(&mut self, kind: String) -> Result<String, String> {
        let started = std::time::Instant::now();
        let r: Result<String, String> = async {
            // Only the accounts its manifest requires.
            if !self.permissions.accounts.contains(&kind) {
                return Err(format!("{kind}: not an account this module requires"));
            }
            if let Some(fields) = self.ctx.checked_account.lock().unwrap_or_else(|e| e.into_inner()).clone().filter(|_| *kind == *self.ctx.module) {
                return Ok(fields);
            }
            let (db, key) = context().ok_or("no account here")?;
            let row = crate::db::account(&db, &kind).await.map_err(|e| e.to_string())?.ok_or_else(|| format!("{kind}: not connected"))?;
            if !row.enabled {
                return Err(format!("{kind}: switched off"));
            }
            let secret = row.secret.as_deref().and_then(|s| crate::crypto::decrypt(&key, s)).ok_or_else(|| format!("{kind}: not connected"))?;
            // What is not secret (a meter number) with the secret, the secret's fields first.
            let mut fields: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&row.config).unwrap_or_default();
            match serde_json::from_str::<serde_json::Value>(&secret) {
                Ok(serde_json::Value::Object(s)) => fields.extend(s),
                _ => return Ok(secret),
            }
            Ok(serde_json::Value::Object(fields).to_string())
        }
        .await;
        self.waited += started.elapsed();
        r
    }

    async fn access_token(&mut self) -> Result<String, String> {
        let started = std::time::Instant::now();
        let r: Result<String, String> = async {
            let token_url = self.permissions.oauth_token_url.clone().ok_or("this module has no OAuth account")?;
            let kind = self.ctx.module.to_string();
            // One renewal at a time: the service replaces the refresh token at each.
            let lock = oauth_lock(&kind);
            let _one = lock.lock().await;
            let checked = self.ctx.checked_account.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let stored = match &checked {
                Some(_) => None,
                None => {
                    let (db, key) = context().ok_or("no account here")?;
                    let row = crate::db::account(&db, &kind).await.map_err(|e| e.to_string())?.filter(|r| r.enabled).ok_or_else(|| format!("{kind}: not connected"))?;
                    let secret = row.secret.as_deref().and_then(|s| crate::crypto::decrypt(&key, s)).ok_or_else(|| format!("{kind}: not connected"))?;
                    Some((db, key, row.config, secret))
                }
            };
            let text = checked.clone().or_else(|| stored.as_ref().map(|s| s.3.clone())).unwrap_or_default();
            let mut fields: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            let now = crate::db::now();
            if let (Some(token), Some(until)) = (fields.get("access_token").and_then(|v| v.as_str()), fields.get("expires_at").and_then(|v| v.as_i64())) {
                if until - 60 > now {
                    return Ok(token.to_string());
                }
            }
            let renewed = renew_oauth(&self.permissions, &token_url, &mut fields, now).await;
            if let Some((db, key, config, _)) = &stored {
                match &renewed {
                    Ok(()) => {
                        let config: serde_json::Value = serde_json::from_str(config).unwrap_or_default();
                        let expires = fields.get("expires_at").and_then(|v| v.as_i64());
                        crate::accounts::save_in(db, key, &kind, &config, &serde_json::Value::Object(fields.clone()), expires).await.map_err(|e| e.message.en)?;
                        let _ = crate::db::set_account_state(db, &kind, "ok", None).await;
                    }
                    Err(e) => {
                        let _ = crate::db::set_account_state(db, &kind, "error", Some(&crate::i18n::Text::plain(e.clone()).store())).await;
                    }
                }
            } else if renewed.is_ok() {
                *self.ctx.checked_account.lock().unwrap_or_else(|e| e.into_inner()) = Some(serde_json::Value::Object(fields.clone()).to_string());
            }
            renewed?;
            fields.get("access_token").and_then(|v| v.as_str()).map(str::to_string).ok_or_else(|| "no access token".to_string())
        }
        .await;
        self.waited += started.elapsed();
        r
    }

    async fn account_update(&mut self, fields: String) -> Result<(), String> {
        let started = std::time::Instant::now();
        let r: Result<(), String> = async {
            let kind = self.ctx.module.to_string();
            if !self.permissions.accounts.contains(&kind) {
                return Err("this module is not an account".into());
            }
            let update: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&fields).map_err(|e| e.to_string())?;
            let merge = |fields: &mut serde_json::Map<String, serde_json::Value>| {
                for (k, v) in &update {
                    if v.is_null() {
                        fields.remove(k);
                    } else {
                        fields.insert(k.clone(), v.clone());
                    }
                }
            };
            // While it is checked: what it keeps of what was typed (a password made a hash…).
            {
                let mut checked = self.ctx.checked_account.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(text) = checked.as_ref() {
                    let mut fields: serde_json::Map<String, serde_json::Value> = serde_json::from_str(text).unwrap_or_default();
                    merge(&mut fields);
                    *checked = Some(serde_json::Value::Object(fields).to_string());
                    return Ok(());
                }
            }
            // Kept: only what it adds itself (a session), never the fields of its form.
            if let Some(k) = update.keys().find(|k| self.permissions.account_fields.contains(k)) {
                return Err(format!("{k}: a field of the account's form, changed only by the user"));
            }
            let lock = oauth_lock(&kind);
            let _one = lock.lock().await;
            let (db, key) = context().ok_or("no account here")?;
            let row = crate::db::account(&db, &kind).await.map_err(|e| e.to_string())?.ok_or_else(|| format!("{kind}: not connected"))?;
            let secret = row.secret.as_deref().and_then(|s| crate::crypto::decrypt(&key, s)).ok_or_else(|| format!("{kind}: not connected"))?;
            let mut fields: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&secret).map_err(|e| e.to_string())?;
            merge(&mut fields);
            let config: serde_json::Value = serde_json::from_str(&row.config).unwrap_or_default();
            crate::accounts::save_in(&db, &key, &kind, &config, &serde_json::Value::Object(fields), row.expires_at).await.map_err(|e| e.message.en)
        }
        .await;
        self.waited += started.elapsed();
        r
    }

    fn pressed(&mut self, reference: String, button: u32, press: String) -> wasmtime::Result<()> {
        let press = match press.as_str() {
            "double" => "double",
            "long" => "long",
            _ => "single",
        };
        if let Some(sink) = self.ctx.sink.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            sink.press(&reference, button, press);
        }
        Ok(())
    }

    fn changed(&mut self) -> wasmtime::Result<()> {
        if let Some(sink) = self.ctx.sink.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            sink.changed();
        }
        Ok(())
    }
}

/// How a WebSocket opens: the headers its module adds, and whether its frames are sent as binary.
#[derive(Clone, Copy)]
struct Opening<'a> {
    headers: &'a [Header],
    binary: bool,
}

/// The headers of the handshake of a WebSocket: its own, never a module's.
const HANDSHAKE_HEADERS: &[&str] = &["host", "connection", "upgrade", "sec-websocket-key", "sec-websocket-version", "sec-websocket-extensions"];

/// The request that opens a WebSocket, with the headers of its module.
fn ws_request(url: &str, headers: &[Header]) -> Result<tokio_tungstenite::tungstenite::handshake::client::Request, String> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::http::{HeaderName, HeaderValue};
    let mut request = url.into_client_request().map_err(|e| e.to_string())?;
    for h in headers {
        let name = HeaderName::from_bytes(h.name.as_bytes()).map_err(|_| format!("header {:?}: invalid name", h.name))?;
        if HANDSHAKE_HEADERS.contains(&name.as_str()) {
            return Err(format!("header {name}: part of the handshake"));
        }
        let value = HeaderValue::from_str(&h.value).map_err(|_| format!("header {name}: invalid value"))?;
        request.headers_mut().append(name, value);
    }
    Ok(request)
}

/// Opens a subscription (`ws://`, `wss://` or `tcp://`) to an allowed address, and the task that hands its
/// messages to `messages` and writes what is sent on it.
async fn subscribe(
    permissions: &Permissions,
    url: &str,
    opening: Opening<'_>,
    id: u32,
    messages: tokio::sync::mpsc::UnboundedSender<(u32, Vec<u8>)>,
) -> Result<Subscription, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let parsed = reqwest::Url::parse(url).map_err(|e| e.to_string())?;
    if parsed.scheme() == "tcp" && (!opening.headers.is_empty() || opening.binary) {
        return Err("headers and binary frames: WebSocket only".into());
    }
    let addrs = permissions.resolve(&parsed).await?;
    let connect = tokio::time::timeout(Duration::from_secs(10), tokio::net::TcpStream::connect(&addrs[..]));
    let (out, mut outgoing) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    let task = match parsed.scheme() {
        scheme @ ("ws" | "wss") => {
            let request = ws_request(url, opening.headers)?;
            let tcp = connect.await.map_err(|_| "no answer".to_string())?.map_err(|e| e.to_string())?;
            if scheme == "wss" {
                // A device on the local network has its own certificate; a service, a real one.
                let device = addrs.iter().all(|a| local(a.ip()));
                let config = crate::tls::client_config(device).ok_or("no TLS")?;
                let name = rustls::pki_types::ServerName::try_from(parsed.host_str().unwrap_or_default().to_string()).map_err(|e| e.to_string())?;
                let tls = tokio_rustls::TlsConnector::from(Arc::new(config)).connect(name, tcp).await.map_err(|e| e.to_string())?;
                let (ws, _) = tokio_tungstenite::client_async(request, tls).await.map_err(|e| e.to_string())?;
                websocket(ws, id, opening.binary, messages, outgoing)
            } else {
                let (ws, _) = tokio_tungstenite::client_async(request, tcp).await.map_err(|e| e.to_string())?;
                websocket(ws, id, opening.binary, messages, outgoing)
            }
        }
        "tcp" => {
            let tcp = connect.await.map_err(|_| "no answer".to_string())?.map_err(|e| e.to_string())?;
            let (mut read, mut write) = tcp.into_split();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 64 << 10];
                loop {
                    tokio::select! {
                        n = read.read(&mut buf) => match n {
                            Ok(0) | Err(_) => break,
                            Ok(n) => {
                                if messages.send((id, buf[..n].to_vec())).is_err() {
                                    break;
                                }
                            }
                        },
                        sent = outgoing.recv() => match sent {
                            Some(data) => {
                                if write.write_all(&data).await.is_err() {
                                    break;
                                }
                            }
                            None => break,
                        },
                    }
                }
            })
        }
        other => return Err(format!("{other}: only ws://, wss:// and tcp://")),
    };
    Ok(Subscription { out, task: task.abort_handle() })
}

/// The task of a WebSocket subscription: hands its text and binary frames to `messages`, sends
/// what comes on `outgoing` (text when it is UTF-8, unless `binary`).
fn websocket<S>(
    ws: tokio_tungstenite::WebSocketStream<S>,
    id: u32,
    binary: bool,
    messages: tokio::sync::mpsc::UnboundedSender<(u32, Vec<u8>)>,
    mut outgoing: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
) -> tokio::task::JoinHandle<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    let (mut write, mut read) = ws.split();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                incoming = read.next() => match incoming {
                    Some(Ok(m @ (Message::Text(_) | Message::Binary(_)))) => {
                        let data = m.into_data();
                        if data.len() <= MAX_MESSAGE && messages.send((id, data.to_vec())).is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    Some(Ok(_)) => {}
                },
                sent = outgoing.recv() => match sent {
                    Some(data) => {
                        let m = match String::from_utf8(data) {
                            Ok(text) if !binary => Message::Text(text.into()),
                            Ok(text) => Message::Binary(text.into_bytes().into()),
                            Err(e) => Message::Binary(e.into_bytes().into()),
                        };
                        if write.send(m).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                },
            }
        }
    })
}

/// One lock per OAuth account, held while its access is renewed.
fn oauth_lock(kind: &str) -> Arc<tokio::sync::Mutex<()>> {
    static LOCKS: std::sync::LazyLock<std::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = std::sync::LazyLock::new(Default::default);
    LOCKS.lock().unwrap_or_else(|e| e.into_inner()).entry(kind.to_string()).or_default().clone()
}

/// Trades the refresh token of an account for a new access (and a new refresh token when the
/// service gives one), at an address the module may reach.
async fn renew_oauth(permissions: &Permissions, token_url: &str, fields: &mut serde_json::Map<String, serde_json::Value>, now: i64) -> Result<(), String> {
    let field = |k: &str| fields.get(k).and_then(|v| v.as_str()).map(str::trim).unwrap_or_default().to_string();
    let (refresh, id, secret) = (field("refresh_token"), field("client_id"), field("client_secret"));
    if refresh.is_empty() {
        return Err("no refresh token".into());
    }
    let parsed = reqwest::Url::parse(token_url).map_err(|e| e.to_string())?;
    let parsed = match redirected(&parsed) {
        Some(to) if permissions.allows_name(parsed.host_str().unwrap_or_default()) => to,
        _ => parsed,
    };
    permissions.resolve(&parsed).await?;
    let form = [("grant_type", "refresh_token"), ("refresh_token", refresh.as_str()), ("client_id", id.as_str()), ("client_secret", secret.as_str())];
    let http = reqwest::Client::builder().timeout(Duration::from_secs(20)).build().map_err(|e| e.to_string())?;
    let resp = http.post(parsed).form(&form).send().await.map_err(|e| e.to_string())?;
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.unwrap_or_default();
    if !status.is_success() {
        let detail = body["error_description"].as_str().or(body["error"].as_str()).unwrap_or(status.as_str()).to_string();
        return Err(format!("the service refuses the refresh token ({detail})"));
    }
    let access = body["access_token"].as_str().ok_or("no access token")?;
    fields.insert("access_token".into(), access.into());
    if let Some(r) = body["refresh_token"].as_str() {
        fields.insert("refresh_token".into(), r.into());
    }
    fields.insert("expires_at".into(), (now + body["expires_in"].as_i64().unwrap_or(3600)).into());
    Ok(())
}

/// An address of the local network (or the machine itself).
fn local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_link_local() || v4.is_loopback(),
        IpAddr::V6(v6) => v6.is_loopback() || (v6.segments()[0] & 0xfe00) == 0xfc00 || (v6.segments()[0] & 0xffc0) == 0xfe80,
    }
}

/// Largest answer of an `exchange`.
const MAX_EXCHANGE: usize = 1 << 20;

/// A question and its answer on TCP (see `host.exchange`).
async fn exchange(permissions: &Permissions, url: &str, data: &[u8], until: ReadUntil, timeout_ms: u32) -> Result<Vec<u8>, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let parsed = reqwest::Url::parse(url).map_err(|e| e.to_string())?;
    if parsed.scheme() != "tcp" || parsed.port().is_none() {
        return Err(format!("{url}: tcp://host:port only"));
    }
    let addrs = permissions.resolve(&parsed).await?;
    let io = async {
        let mut tcp = tokio::net::TcpStream::connect(&addrs[..]).await.map_err(|e| e.to_string())?;
        tcp.write_all(data).await.map_err(|e| e.to_string())?;
        let mut out: Vec<u8> = Vec::new();
        let mut buf = vec![0u8; 16 << 10];
        loop {
            let wanted = match &until {
                ReadUntil::Bytes(n) => Some(*n as usize),
                ReadUntil::LengthPrefixed(p) => {
                    let (at, size) = (p.offset as usize, usize::from(p.size));
                    out.get(at..at + size).map(|b| {
                        let n = b.iter().fold(0u64, |acc, x| acc << 8 | u64::from(*x));
                        let n = if p.big_endian { n } else { b.iter().rev().fold(0u64, |acc, x| acc << 8 | u64::from(*x)) };
                        (n as i64 + i64::from(p.adds)).max(0) as usize
                    })
                }
                _ => None,
            };
            if wanted.is_some_and(|w| out.len() >= w) {
                out.truncate(wanted.unwrap_or(out.len()));
                return Ok(out);
            }
            let read = match &until {
                ReadUntil::Idle(ms) if !out.is_empty() => match tokio::time::timeout(Duration::from_millis(u64::from(*ms)), tcp.read(&mut buf)).await {
                    Ok(r) => r,
                    Err(_) => return Ok(out),
                },
                _ => tcp.read(&mut buf).await,
            };
            match read.map_err(|e| e.to_string())? {
                0 if matches!(until, ReadUntil::Close | ReadUntil::Idle(_)) => return Ok(out),
                0 => return Err("the connection closed before the whole answer".into()),
                n => out.extend_from_slice(&buf[..n]),
            }
            if out.len() > MAX_EXCHANGE {
                return Err("answer too large".into());
            }
        }
    };
    tokio::time::timeout(Duration::from_millis(u64::from(timeout_ms.clamp(100, 30_000))), io).await.map_err(|_| "no answer in time".to_string())?
}

/// Where the tests send the requests of the modules to a service of the Internet (`host` →
/// "127.0.0.1:port", a fake): development builds only.
static REDIRECTS: std::sync::LazyLock<std::sync::RwLock<HashMap<String, String>>> = std::sync::LazyLock::new(Default::default);

/// The requests of the modules to `host` go to `to` ("127.0.0.1:port", in clear) instead: for
/// the tests, whose fakes stand for services of the Internet (development builds only).
pub fn redirect_for_tests(host: &str, to: &str) {
    if cfg!(debug_assertions) {
        REDIRECTS.write().unwrap_or_else(|e| e.into_inner()).insert(host.to_ascii_lowercase(), to.to_string());
    }
}

fn redirected(url: &reqwest::Url) -> Option<reqwest::Url> {
    if !cfg!(debug_assertions) {
        return None;
    }
    let to = REDIRECTS.read().unwrap_or_else(|e| e.into_inner()).get(&url.host_str()?.to_ascii_lowercase()).cloned()?;
    reqwest::Url::parse(&format!("http://{to}{}{}", url.path(), url.query().map(|q| format!("?{q}")).unwrap_or_default())).ok()
}

async fn http(permissions: &Permissions, req: Request) -> Result<Response, String> {
    let url = reqwest::Url::parse(&req.url).map_err(|e| e.to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!("{}: only http and https", url.scheme()));
    }
    // A test's fake, for a host the module may reach.
    let url = match redirected(&url) {
        Some(to) if permissions.allows_name(url.host_str().unwrap_or_default()) => to,
        _ => url,
    };
    let addrs = permissions.resolve(&url).await?;
    let host = url.host_str().unwrap_or_default().to_string();
    let timeout = Duration::from_millis(u64::from(req.timeout_ms.clamp(100, 30_000)));
    // No redirection: it would lead past the check of the address.
    let mut builder = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(timeout).no_proxy();
    if host.parse::<IpAddr>().is_err() {
        builder = builder.resolve_to_addrs(&host, &addrs);
    }
    // A camera's own certificate: accepted only on the local network, never on the Internet.
    if req.insecure {
        if !addrs.iter().all(|a| local(a.ip())) {
            return Err(format!("{host}: an unchecked certificate only on the local network"));
        }
        builder = builder.danger_accept_invalid_certs(true);
    }
    let client = builder.build().map_err(|e| e.to_string())?;
    let method = reqwest::Method::from_bytes(req.method.as_bytes()).map_err(|e| e.to_string())?;
    let mut r = client.request(method, url);
    for h in &req.headers {
        r = r.header(&h.name, &h.value);
    }
    if let Some(body) = req.body {
        r = r.body(body);
    }
    let mut resp = r.send().await.map_err(|e| chain(&e))?;
    let status = resp.status().as_u16();
    let headers = resp.headers().iter().map(|(k, v)| Header { name: k.to_string(), value: String::from_utf8_lossy(v.as_bytes()).into_owned() }).collect();
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
        if body.len() + chunk.len() > MAX_BODY {
            return Err("answer too large".into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Response { status, headers, body })
}

/// An error with its causes (reqwest only says « error sending request » itself).
fn chain(e: &dyn std::error::Error) -> String {
    let mut out = e.to_string();
    let mut cause = e.source();
    while let Some(c) = cause {
        out.push_str(&format!(": {c}"));
        cause = c.source();
    }
    out
}

/// The engine shared by every module, with the thread that moves its epoch on.
fn engine() -> &'static Engine {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    ENGINE.get_or_init(|| {
        let mut config = Config::new();
        config.wasm_component_model(true).epoch_interruption(true);
        // Each source keeps an instance, which runs on a stack of its own (its host calls too: an
        // HTTPS request, a TLS handshake): 2 MiB by default, all of it resident once used. A module
        // needs far less; Breezone's memory stays small (100 MB for everything).
        config.max_wasm_stack(256 << 10).async_stack_size(512 << 10);
        let engine = Engine::new(&config).expect("WebAssembly engine");
        let ticking = engine.weak();
        std::thread::Builder::new()
            .name("wasm-epoch".into())
            .spawn(move || {
                while let Some(engine) = ticking.upgrade() {
                    engine.increment_epoch();
                    drop(engine);
                    std::thread::sleep(TICK);
                }
            })
            .expect("epoch thread");
        engine
    })
}

/// A module, compiled: shared by all its sources.
pub struct WasmModule {
    id: Arc<str>,
    read_every: Duration,
    /// The calls it answers beyond the required ones (`optional_calls` of its manifest).
    optional: Vec<String>,
    component: Component,
    linker: Arc<Linker<HostState>>,
    permissions: Arc<Permissions>,
    /// The instance of an appairage that waits for its next step (a code to type): kept while a
    /// step answers `next`, for the same address, and dropped after a while.
    pairing: tokio::sync::Mutex<Option<Pairing>>,
}

/// An appairage between two steps.
struct Pairing {
    /// The address it is for (`host` of the steps).
    host: String,
    store: Store<HostState>,
    module: Module,
    /// Tells it apart from a later one (its timer drops only it).
    generation: u64,
}

/// How long an appairage waits for its next step.
const PAIRING_FOR: Duration = Duration::from_secs(120);

impl WasmModule {
    /// Compiles a component, or takes it from `cache` (a directory) when it was compiled before by
    /// this same engine.
    pub fn load(id: &str, wasm: &[u8], permissions: Permissions, cache: Option<&Path>) -> anyhow::Result<Self> {
        let engine = engine();
        let component = match cache {
            Some(dir) => {
                let mut hasher = std::hash::DefaultHasher::new();
                std::hash::Hash::hash(&engine.precompile_compatibility_hash(), &mut hasher);
                let key = format!("{id}-{}-{:016x}.cwasm", hex::encode(&<sha2::Sha256 as sha2::Digest>::digest(wasm)[..8]), std::hash::Hasher::finish(&hasher));
                let path = dir.join(key);
                // SAFETY: the file was written by `serialize` of this engine, in Breezone's own data
                // directory; a file that does not fit is refused by `deserialize_file`.
                match unsafe { Component::deserialize_file(engine, &path) } {
                    Ok(c) => c,
                    Err(_) => {
                        let c = Component::new(engine, wasm).map_err(|e| anyhow::anyhow!("{e}"))?;
                        std::fs::create_dir_all(dir)?;
                        std::fs::write(&path, c.serialize().map_err(|e| anyhow::anyhow!("{e}"))?)?;
                        // Read back from the file: its code is then mapped from the disk (memory the
                        // system may take back), not held in the process.
                        // SAFETY: as above, the file was just written by this engine.
                        unsafe { Component::deserialize_file(engine, &path) }.unwrap_or(c)
                    }
                }
            }
            None => Component::new(engine, wasm).map_err(|e| anyhow::anyhow!("{e}"))?,
        };
        let mut linker = Linker::new(engine);
        wasmtime_wasi::p2::add_to_linker_async(&mut linker).map_err(|e| anyhow::anyhow!("{e}"))?;
        Module::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |s| s).map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(Self {
            pairing: Default::default(),
            id: id.into(),
            read_every: Duration::ZERO,
            optional: vec![],
            component,
            linker: Arc::new(linker),
            permissions: Arc::new(permissions),
        })
    }

    /// The calls it answers beyond the required ones.
    pub fn with_optional_calls(mut self, calls: &[String]) -> Self {
        self.optional = calls.to_vec();
        self
    }

    async fn instantiate(&self, ctx: Arc<SourceCtx>) -> Result<(Store<HostState>, Module), DriverError> {
        let (store, module, _) = self.instantiate_with(ctx).await?;
        Ok((store, module))
    }

    async fn instantiate_with(&self, ctx: Arc<SourceCtx>) -> Result<(Store<HostState>, Module, wasmtime::component::Instance), DriverError> {
        let wasi = WasiCtxBuilder::new().allow_tcp(false).allow_udp(false).allow_ip_name_lookup(false).socket_addr_check(|_, _| Box::pin(async { false })).build();
        let memory = self.permissions.memory_mb.clamp(1, 64) << 20;
        let limits = StoreLimitsBuilder::new().memory_size(memory).instances(16).trap_on_grow_failure(true).build();
        let state = HostState {
            permissions: self.permissions.clone(),
            ctx,
            subscriptions: HashMap::new(),
            next_subscription: 0,
            wasi,
            table: ResourceTable::new(),
            limits,
            call_started: std::time::Instant::now(),
            waited: Duration::ZERO,
        };
        let mut store = Store::new(engine(), state);
        store.limiter(|s| &mut s.limits);
        // At the deadline, what the call computed (its waits in the host left out) decides.
        store.epoch_deadline_callback(|ctx| {
            let s = ctx.data();
            let computed = s.call_started.elapsed().saturating_sub(s.waited);
            if computed >= CALL_COMPUTE {
                return Err(wasmtime::Error::msg(format!("computed for more than {} s without a pause", CALL_COMPUTE.as_secs())));
            }
            let left = (CALL_COMPUTE - computed).as_millis() / TICK.as_millis();
            Ok(wasmtime::UpdateDeadline::Continue((left as u64).max(1)))
        });
        start_call(&mut store);
        let instance = self.linker.instantiate_async(&mut store, &self.component).await.map_err(|e| DriverError::Invalid(format!("{}: {e}", self.id)))?;
        let module = Module::new(&mut store, &instance).map_err(|e| DriverError::Invalid(format!("{}: {e}", self.id)))?;
        Ok((store, module, instance))
    }

    /// `extras.forget` on an instance of its own, when the module exports it (one built before
    /// 1.1 does not: nothing to do then).
    pub async fn forget(&self, config: &serde_json::Value, secret: Option<&str>) -> Result<(), DriverError> {
        let (mut store, _, instance) = self.instantiate_with(self.one_shot()).await?;
        let Some(extras) = instance.get_export_index(&mut store, None, EXTRAS) else { return Ok(()) };
        let Some(index) = instance.get_export_index(&mut store, Some(&extras), "forget") else { return Ok(()) };
        let func = instance
            .get_typed_func::<(String, Option<String>), (Result<(), WitError>,)>(&mut store, &index)
            .map_err(|e| DriverError::Invalid(format!("{}: forget: {e}", self.id)))?;
        let (answer,) = tokio::time::timeout(CALL_TIMEOUT, func.call_async(&mut store, (config.to_string(), secret.map(str::to_string))))
            .await
            .map_err(|_| DriverError::Unreachable(format!("{}: no answer", self.id)))?
            .map_err(|e| DriverError::Invalid(format!("{}: {e}", self.id)))?;
        answer.map_err(from_wit)
    }

    /// `extras.history` on an instance of its own: what the hardware kept of a device.
    pub async fn history(&self, config: &serde_json::Value, secret: Option<&str>, reference: &str, since: i64, until: i64) -> Result<serde_json::Value, DriverError> {
        let (mut store, _, instance) = self.instantiate_with(self.one_shot()).await?;
        let missing = || DriverError::Unsupported(format!("{}: no history", self.id));
        let extras = instance.get_export_index(&mut store, None, EXTRAS).ok_or_else(missing)?;
        let index = instance.get_export_index(&mut store, Some(&extras), "history").ok_or_else(missing)?;
        let func = instance
            .get_typed_func::<(String, Option<String>, String, i64, i64), (Result<String, WitError>,)>(&mut store, &index)
            .map_err(|e| DriverError::Invalid(format!("{}: history: {e}", self.id)))?;
        let args = (config.to_string(), secret.map(str::to_string), reference.to_string(), since, until);
        let (answer,) = tokio::time::timeout(CALL_TIMEOUT, func.call_async(&mut store, args))
            .await
            .map_err(|_| DriverError::Unreachable(format!("{}: no answer", self.id)))?
            .map_err(|e| DriverError::Invalid(format!("{}: {e}", self.id)))?;
        serde_json::from_str(&answer.map_err(from_wit)?).map_err(|e| DriverError::Invalid(format!("{}: history: {e}", self.id)))
    }

    /// The context of an instance of its own (a probe, a step of an appairage, a query): what its
    /// subscriptions receive goes to its inbox, for `receive`.
    fn one_shot(&self) -> Arc<SourceCtx> {
        let (ctx, received) = self.context(None);
        tokio::spawn(forward(received, ctx.inbox.clone()));
        ctx
    }

    fn context(&self, source_id: Option<i64>) -> (Arc<SourceCtx>, tokio::sync::mpsc::UnboundedReceiver<(u32, Vec<u8>)>) {
        let (messages, received) = tokio::sync::mpsc::unbounded_channel();
        let ctx = SourceCtx {
            module: self.id.clone(),
            source_id,
            sink: std::sync::Mutex::new(None),
            messages,
            inbox: Default::default(),
            memory_store: Default::default(),
            checked_account: Default::default(),
        };
        (Arc::new(ctx), received)
    }

    /// Whether `host` is hardware of this module (discovery), in an instance of its own.
    /// `extras.identify` on an instance of its own (`None` when the module does not export it, or
    /// does not know this hardware): what the network scan shows of it.
    pub async fn identify(&self, host: &str, announced: Option<&serde_json::Value>) -> Option<serde_json::Value> {
        let (mut store, _, instance) = self.instantiate_with(self.one_shot()).await.ok()?;
        let extras = instance.get_export_index(&mut store, None, EXTRAS)?;
        let index = instance.get_export_index(&mut store, Some(&extras), "identify")?;
        let func = instance.get_typed_func::<(String, Option<String>), (Option<String>,)>(&mut store, &index).ok()?;
        let (answer,) = tokio::time::timeout(CALL_TIMEOUT, func.call_async(&mut store, (host.to_string(), announced.map(|a| a.to_string())))).await.ok()?.ok()?;
        serde_json::from_str(&answer?).ok().filter(serde_json::Value::is_object)
    }

    pub async fn probe(&self, host: &str) -> Option<String> {
        let ctx = self.one_shot();
        let (mut store, module) = self.instantiate(ctx).await.ok()?;
        tokio::time::timeout(CALL_TIMEOUT, module.breezone_module_driver().call_probe(&mut store, host)).await.ok()?.ok()?
    }

    /// Checks the module's own account (`fields`: those of its form, as JSON) before it is kept;
    /// the fields to keep (an OAuth access renewed during the check comes with them).
    pub async fn check_account(&self, fields: &str) -> Result<String, DriverError> {
        let ctx = self.one_shot();
        *ctx.checked_account.lock().unwrap_or_else(|e| e.into_inner()) = Some(fields.to_string());
        let (mut store, module) = self.instantiate(ctx.clone()).await?;
        tokio::time::timeout(CALL_TIMEOUT, module.breezone_module_service().call_check(&mut store))
            .await
            .map_err(|_| DriverError::Unreachable(format!("{}: no answer", self.id)))?
            .map_err(|e| DriverError::Invalid(format!("{}: {e}", self.id)))?
            .map_err(from_wit)?;
        let kept = ctx.checked_account.lock().unwrap_or_else(|e| e.into_inner()).clone();
        Ok(kept.unwrap_or_else(|| fields.to_string()))
    }

    /// A query of a service contract (`contract`: "tariff.day_color@1"), in an instance of its own;
    /// the answer is checked against the contract.
    pub async fn query(&self, contract: &str, query: &str, params: &serde_json::Value) -> Result<serde_json::Value, DriverError> {
        let ctx = self.one_shot();
        let (mut store, module) = self.instantiate(ctx).await?;
        let answer = tokio::time::timeout(CALL_TIMEOUT, module.breezone_module_service().call_query(&mut store, contract, query, &params.to_string()))
            .await
            .map_err(|_| DriverError::Unreachable(format!("{}: no answer", self.id)))?
            .map_err(|e| DriverError::Invalid(format!("{}: {e}", self.id)))?
            .map_err(from_wit)?;
        let value: serde_json::Value = serde_json::from_str(&answer).map_err(|e| DriverError::Invalid(format!("{}: {e}", self.id)))?;
        crate::services::check_answer(contract, query, &value).map_err(|e| DriverError::Invalid(format!("{}: {e}", self.id)))?;
        Ok(value)
    }

    /// A step of the appairage, in an instance of its own; one that answers `next` keeps it for
    /// the next step at the same address (a pairing started, waiting for its code).
    pub async fn pair(self: &Arc<Self>, step: &str, input: &serde_json::Value) -> Result<crate::modules::Paired, DriverError> {
        use crate::modules::Paired;
        use breezone::module::types::Paired as P;
        static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let host = input["host"].as_str().unwrap_or_default().to_string();
        let mut slot = self.pairing.lock().await;
        let (mut store, module) = match slot.take().filter(|p| p.host == host) {
            Some(p) => (p.store, p.module),
            None => self.instantiate(self.one_shot()).await?,
        };
        // The instance of the previous step, maybe: a new call.
        start_call(&mut store);
        let answer = tokio::time::timeout(CALL_TIMEOUT, module.breezone_module_driver().call_pair(&mut store, step, &input.to_string()))
            .await
            .map_err(|_| DriverError::Unreachable(format!("{}: no answer", self.id)))?
            .map_err(|e| DriverError::Invalid(format!("{}: {e}", self.id)))?;
        match answer.map_err(from_wit)? {
            P::Next => {
                let generation = GENERATION.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                *slot = Some(Pairing { host, store, module, generation });
                // Never finished: dropped (its connections closed), else the hardware stays busy.
                let me = Arc::downgrade(self);
                tokio::spawn(async move {
                    tokio::time::sleep(PAIRING_FOR).await;
                    if let Some(me) = me.upgrade() {
                        let mut slot = me.pairing.lock().await;
                        if slot.as_ref().is_some_and(|p| p.generation == generation) {
                            *slot = None;
                        }
                    }
                });
                Ok(Paired::Next)
            }
            P::Waiting(message) => Ok(Paired::Waiting(message)),
            P::Done(s) => {
                let config = serde_json::from_str(&s.config).map_err(|e| DriverError::Invalid(format!("{}: {e}", self.id)))?;
                Ok(Paired::Done { config, secret: s.secret, name: s.name.chars().take(60).collect() })
            }
        }
    }

    /// A source of this module (`source_id`: the one it is for, `None` while its configuration is
    /// only checked), with its pump: the messages of its subscriptions go to `on-message`.
    pub async fn open(self: &Arc<Self>, source_id: Option<i64>, config: &serde_json::Value, secret: Option<&str>) -> Result<Arc<WasmSource>, DriverError> {
        let (ctx, received) = self.context(source_id);
        let source =
            Arc::new(WasmSource { module: self.clone(), config: config.to_string(), secret: secret.map(str::to_string), ctx, live: Mutex::new(None), every: self.read_every });
        // Checks the configuration at once, as the native drivers do.
        let started = source.start().await?;
        *source.live.lock().await = Some(started);
        let inbox = source.ctx.inbox.clone();
        tokio::spawn(forward(received, inbox.clone()));
        tokio::spawn(pump(Arc::downgrade(&source), inbox));
        Ok(source)
    }
}

/// Moves the messages of a source's subscriptions into its inbox, as they come (never waiting for
/// the module), until its subscriptions are all gone.
async fn forward(mut received: tokio::sync::mpsc::UnboundedReceiver<(u32, Vec<u8>)>, inbox: Arc<Inbox>) {
    while let Some(message) = received.recv().await {
        inbox.push(message);
    }
    inbox.closed.store(true, std::sync::atomic::Ordering::SeqCst);
    inbox.arrived.notify_waiters();
}

/// Hands the messages of a source's subscriptions to its module between its calls (one taken only
/// once the instance is free, so that `receive` during a call sees them first), until the source is
/// dropped.
async fn pump(source: std::sync::Weak<WasmSource>, inbox: Arc<Inbox>) {
    loop {
        let arrived = inbox.arrived.notified();
        if inbox.is_empty() {
            if inbox.closed.load(std::sync::atomic::Ordering::SeqCst) {
                break;
            }
            // Woken by a message, or now and then to see whether the source is still there.
            let _ = tokio::time::timeout(Duration::from_secs(60), arrived).await;
            if source.strong_count() == 0 {
                break;
            }
            continue;
        }
        let Some(source) = source.upgrade() else { break };
        let waiting = inbox.clone();
        let result = source
            .call(move |l| {
                Box::pin(async move {
                    match waiting.pop() {
                        Some((subscription, data)) => l.module.breezone_module_driver().source().call_on_message(&mut l.store, l.source, subscription, &data).await,
                        None => Ok(Ok(())),
                    }
                })
            })
            .await;
        if let Err(e) = result {
            crate::modules::log(&source.module.id, "warn", format!("on-message: {e}"));
        }
    }
}

struct Live {
    store: Store<HostState>,
    module: Module,
    source: ResourceAny,
}

/// A source run by a module in WebAssembly: a driver like the others for the rest of Breezone.
pub struct WasmSource {
    module: Arc<WasmModule>,
    config: String,
    secret: Option<String>,
    ctx: Arc<SourceCtx>,
    /// The instance, created again after a trap.
    live: Mutex<Option<Live>>,
    every: Duration,
}

fn json_map(text: &str) -> Result<serde_json::Map<String, serde_json::Value>, DriverError> {
    match serde_json::from_str(text) {
        Ok(serde_json::Value::Object(m)) => Ok(m),
        _ => Err(DriverError::Invalid("the parameters of a schedule are not an object".into())),
    }
}

fn schedule_from_wit(s: breezone::module::types::Schedule) -> Result<crate::drivers::Schedule, DriverError> {
    let end = match s.end {
        Some(e) => Some(crate::drivers::ScheduleEnd { time: e.time, params: json_map(&e.params)? }),
        None => None,
    };
    Ok(crate::drivers::Schedule {
        id: s.id,
        name: s.name,
        enabled: s.enabled,
        days: s.days,
        time: s.time,
        references: s.references,
        trait_name: s.trait_name,
        action: s.action,
        params: json_map(&s.params)?,
        end,
        editable: s.editable,
        note: s.note,
    })
}

fn schedule_to_wit(s: &crate::drivers::Schedule) -> breezone::module::types::Schedule {
    breezone::module::types::Schedule {
        id: s.id.clone(),
        name: s.name.clone(),
        enabled: s.enabled,
        days: s.days.clone(),
        time: s.time.clone(),
        references: s.references.clone(),
        trait_name: s.trait_name.clone(),
        action: s.action.clone(),
        params: serde_json::Value::Object(s.params.clone()).to_string(),
        end: s.end.as_ref().map(|e| breezone::module::types::ScheduleEnd { time: e.time.clone(), params: serde_json::Value::Object(e.params.clone()).to_string() }),
        editable: s.editable,
        note: s.note.clone(),
    }
}

fn from_wit(e: WitError) -> DriverError {
    match e {
        WitError::Unreachable(m) => DriverError::Unreachable(m),
        WitError::Invalid(m) => DriverError::Invalid(m),
        WitError::Unsupported(m) => DriverError::Unsupported(m),
    }
}

/// A media type a picture may have (others are given as JPEG).
fn media_type(t: &str) -> &'static str {
    match t {
        "image/png" => "image/png",
        "image/webp" => "image/webp",
        "image/gif" => "image/gif",
        _ => "image/jpeg",
    }
}

impl WasmSource {
    async fn start(&self) -> Result<Live, DriverError> {
        let (mut store, module) = self.module.instantiate(self.ctx.clone()).await?;
        let guest = module.breezone_module_driver().source();
        let open = guest.call_open(&mut store, &self.config, self.secret.as_deref());
        let source = tokio::time::timeout(CALL_TIMEOUT, open)
            .await
            .map_err(|_| DriverError::Unreachable(format!("{}: too long", self.module.id)))?
            .map_err(|e| DriverError::Invalid(format!("{}: {e}", self.module.id)))?
            .map_err(from_wit)?;
        Ok(Live { store, module, source })
    }

    fn has(&self, call: &str) -> Result<(), DriverError> {
        if self.module.optional.iter().any(|c| c == call) {
            Ok(())
        } else {
            Err(DriverError::Unsupported(format!("{}: no {call}", self.module.id)))
        }
    }

    /// Runs one call in the instance (created if needed); a trap or a call too long drops the
    /// instance, so that the next call starts from a clean one.
    async fn call<T>(
        &self,
        f: impl for<'a> FnOnce(&'a mut Live) -> std::pin::Pin<Box<dyn std::future::Future<Output = wasmtime::Result<Result<T, WitError>>> + Send + 'a>>,
    ) -> Result<T, DriverError>
    where
        T: Send,
    {
        let mut guard = self.live.lock().await;
        if guard.is_none() {
            *guard = Some(self.start().await?);
        }
        let live = guard.as_mut().expect("instance");
        start_call(&mut live.store);
        let outcome = tokio::time::timeout(CALL_TIMEOUT, f(live)).await;
        match outcome {
            Ok(Ok(result)) => result.map_err(from_wit),
            Ok(Err(trap)) => {
                *guard = None;
                tracing::warn!(module = &*self.module.id, "module stopped: {trap}");
                crate::modules::log(&self.module.id, "error", format!("stopped: {trap}"));
                Err(DriverError::Invalid(format!("{}: {trap}", self.module.id)))
            }
            Err(_) => {
                *guard = None;
                crate::modules::log(&self.module.id, "error", "a call took too long: stopped");
                Err(DriverError::Unreachable(format!("{}: too long", self.module.id)))
            }
        }
    }
}

macro_rules! guest {
    ($self:ident, $method:ident $(, $arg:expr)*) => {
        $self.call(move |l| Box::pin(async move { l.module.breezone_module_driver().source().$method(&mut l.store, l.source $(, $arg)*).await }))
    };
}

impl Driver for WasmSource {
    fn read_every(&self) -> Duration {
        self.every
    }

    fn cloud(&self) -> bool {
        crate::accounts::is_cloud(&self.module.id)
    }

    fn describe(&self) -> DriverFuture<'_, Described> {
        Box::pin(async move {
            let d = guest!(self, call_describe).await?;
            Ok(Described {
                model: d.model,
                firmware: d.firmware,
                devices: d.devices.into_iter().map(|x| DeviceInfo { reference: x.reference, kind: x.kind, traits: x.traits, name: x.name }).collect(),
                identity: d.identity,
            })
        })
    }

    fn read(&self) -> DriverFuture<'_, HashMap<String, State>> {
        Box::pin(async move {
            let list = guest!(self, call_read).await?;
            list.into_iter()
                .map(|(reference, json)| match serde_json::from_str(&json) {
                    Ok(serde_json::Value::Object(state)) => Ok((reference, state)),
                    _ => Err(DriverError::Invalid(format!("{}: the state of {reference} is not an object", self.module.id))),
                })
                .collect()
        })
    }

    fn act<'a>(&'a self, reference: &'a str, trait_name: &'a str, action: &'a str, params: &'a State) -> DriverFuture<'a, ()> {
        let (reference, trait_name, action, params) = (reference.to_string(), trait_name.to_string(), action.to_string(), serde_json::Value::Object(params.clone()).to_string());
        Box::pin(async move { guest!(self, call_act, &reference, &trait_name, &action, &params).await })
    }

    fn rooms(&self) -> DriverFuture<'_, Vec<HardwareRoom>> {
        Box::pin(async move {
            self.has("rooms")?;
            Ok(guest!(self, call_rooms).await?.into_iter().map(|r| HardwareRoom { name: r.name, references: r.references }).collect())
        })
    }

    fn scenes(&self) -> DriverFuture<'_, Vec<Scene>> {
        Box::pin(async move {
            self.has("scenes")?;
            Ok(guest!(self, call_scenes).await?.into_iter().map(|s| Scene { id: s.id, name: s.name, room: s.room, references: s.references }).collect())
        })
    }

    fn activate_scene<'a>(&'a self, id: &'a str) -> DriverFuture<'a, ()> {
        let id = id.to_string();
        Box::pin(async move {
            self.has("scenes")?;
            guest!(self, call_activate_scene, &id).await
        })
    }

    fn unlock<'a>(&'a self, key: &'a str) -> DriverFuture<'a, bool> {
        let key = key.to_string();
        Box::pin(async move {
            self.has("unlock")?;
            guest!(self, call_unlock, &key).await
        })
    }

    fn forget(&self) -> DriverFuture<'_, ()> {
        Box::pin(async move {
            if self.has("forget").is_err() {
                return Ok(());
            }
            let config = serde_json::from_str(&self.config).unwrap_or(serde_json::Value::Null);
            self.module.forget(&config, self.secret.as_deref()).await
        })
    }

    fn keeps_history(&self) -> bool {
        self.has("history").is_ok()
    }

    fn history<'a>(&'a self, reference: &'a str, since: i64, until: i64) -> DriverFuture<'a, serde_json::Value> {
        Box::pin(async move {
            self.has("history")?;
            let config = serde_json::from_str(&self.config).unwrap_or(serde_json::Value::Null);
            self.module.history(&config, self.secret.as_deref(), reference, since, until).await
        })
    }

    fn schedules(&self) -> DriverFuture<'_, Vec<crate::drivers::Schedule>> {
        Box::pin(async move {
            self.has("schedules")?;
            guest!(self, call_schedules).await?.into_iter().map(schedule_from_wit).collect()
        })
    }

    fn save_schedule<'a>(&'a self, schedule: &'a crate::drivers::Schedule) -> DriverFuture<'a, String> {
        let s = schedule_to_wit(schedule);
        Box::pin(async move {
            self.has("schedules")?;
            guest!(self, call_save_schedule, &s).await
        })
    }

    fn delete_schedule<'a>(&'a self, id: &'a str) -> DriverFuture<'a, ()> {
        let id = id.to_string();
        Box::pin(async move {
            self.has("schedules")?;
            guest!(self, call_delete_schedule, &id).await
        })
    }

    fn listen(&self, sink: Sink) {
        *self.ctx.sink.lock().unwrap_or_else(|e| e.into_inner()) = Some(sink);
    }

    fn close(&self) -> DriverFuture<'_, ()> {
        Box::pin(async move {
            let _ = self.call(|l| Box::pin(async move { l.module.breezone_module_driver().source().call_close(&mut l.store, l.source).await.map(Ok) })).await;
            Ok(())
        })
    }

    fn picture<'a>(&'a self, reference: &'a str) -> DriverFuture<'a, (Vec<u8>, &'static str)> {
        let reference = reference.to_string();
        Box::pin(async move {
            self.has("picture")?;
            let p = guest!(self, call_picture, &reference).await?;
            Ok((p.data, media_type(&p.media_type)))
        })
    }

    fn forecast<'a>(&'a self, reference: &'a str) -> DriverFuture<'a, Forecast> {
        let reference = reference.to_string();
        Box::pin(async move {
            self.has("forecast")?;
            Ok(guest!(self, call_forecast, &reference).await?.into_iter().map(|c| (c.name, c.points)).collect())
        })
    }

    fn diagnostic(&self) -> DriverFuture<'_, serde_json::Value> {
        Box::pin(async move {
            let json = guest!(self, call_diagnostic).await?;
            Ok(serde_json::from_str(&json).unwrap_or(serde_json::Value::Null))
        })
    }
}

/// The machine itself is reachable by the modules (tests and development only).
static LOOPBACK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[doc(hidden)]
pub fn allow_loopback(allowed: bool) {
    LOOPBACK.store(allowed, std::sync::atomic::Ordering::SeqCst);
}

/// The modules compiled in this process, by id, version, folder and when its component was written.
type Loaded = HashMap<(String, String, std::path::PathBuf, Option<std::time::SystemTime>), Arc<WasmModule>>;
static LOADED: std::sync::LazyLock<Mutex<Loaded>> = std::sync::LazyLock::new(Default::default);

/// An installed module, compiled (the first time one of its sources needs it, kept compiled in
/// `<modules>/cache`), with the permissions of its manifest.
pub async fn loaded(m: &crate::modules::manifest::Manifest, dir: &Path) -> Result<Arc<WasmModule>, String> {
    // Its folder and its component too: the same version installed again (a module in
    // development, a package sent again) is another one.
    let written = std::fs::metadata(dir.join("module.wasm")).and_then(|f| f.modified()).ok();
    let key = (m.id.clone(), m.version.clone(), dir.to_path_buf(), written);
    let mut loaded = LOADED.lock().await;
    // Another than the one loaded: the older is dropped.
    loaded.retain(|(id, version, folder, at), _| id != &m.id || (version == &m.version && folder == dir && *at == written));
    if let Some(module) = loaded.get(&key) {
        return Ok(module.clone());
    }
    let wasm = std::fs::read(dir.join("module.wasm")).map_err(|e| format!("{}: {e}", m.id))?;
    let net = &m.permissions.network;
    let permissions = Permissions {
        lan: net.lan,
        hosts: net.hosts.clone(),
        loopback: LOOPBACK.load(std::sync::atomic::Ordering::SeqCst),
        memory_mb: m.permissions.memory_mb as usize,
        // A module that is an account reads its own.
        accounts: m.requires.accounts.iter().cloned().chain(m.has(crate::modules::manifest::Role::Account).then(|| m.id.clone())).collect(),
        oauth_token_url: m.oauth.as_ref().map(|o| o.token_url.clone()),
        account_fields: m.config.iter().map(|f| f.name.clone()).collect(),
    };
    // The tests share one cache (`BREEZONE_MODULES_CACHE`, development builds only): each of their
    // servers would compile the same modules again.
    let shared = std::env::var_os("BREEZONE_MODULES_CACHE").filter(|_| cfg!(debug_assertions)).map(std::path::PathBuf::from);
    let cache = shared.or_else(|| dir.parent().and_then(Path::parent).map(|modules| modules.join("cache")));
    let id = m.id.clone();
    let mut module = tokio::task::spawn_blocking(move || WasmModule::load(&id, &wasm, permissions, cache.as_deref()))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("{}: {e}", m.id))?
        .with_optional_calls(&m.optional_calls);
    module.read_every = Duration::from_secs(u64::from(m.read_every_s.unwrap_or(0)));
    let module = Arc::new(module);
    loaded.insert(key, module.clone());
    Ok(module)
}

/// A source of an installed module (`dir`: its files).
pub async fn source(
    m: &crate::modules::manifest::Manifest,
    dir: &Path,
    source_id: Option<i64>,
    config: &serde_json::Value,
    secret: Option<&str>,
) -> Result<Arc<dyn Driver>, String> {
    let module = loaded(m, dir).await?;
    Ok(module.open(source_id, config, secret).await.map_err(|e| e.to_string())?)
}

/// Whether `host` is hardware of an installed module (discovery).
pub async fn probe(m: &crate::modules::manifest::Manifest, dir: &Path, host: &str) -> Option<String> {
    loaded(m, dir).await.ok()?.probe(host).await
}

pub async fn identify(m: &crate::modules::manifest::Manifest, dir: &Path, host: &str, announced: Option<&serde_json::Value>) -> Option<serde_json::Value> {
    loaded(m, dir).await.ok()?.identify(host, announced).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_websocket_opens_with_the_headers_of_its_module_but_not_those_of_the_handshake() {
        let h = |name: &str, value: &str| Header { name: name.into(), value: value.into() };
        let r = ws_request("wss://192.168.1.80/mediation/client?mac=001A25123456&appli=1", &[h("Authorization", r#"Digest username="001A25123456""#)]).unwrap();
        assert_eq!(r.headers()["authorization"], r#"Digest username="001A25123456""#);
        assert!(r.headers().contains_key("sec-websocket-key"), "its own handshake");
        for name in ["Host", "Upgrade", "Sec-WebSocket-Key"] {
            assert!(ws_request("ws://192.168.1.80/", &[h(name, "x")]).is_err(), "{name}");
        }
        assert!(ws_request("ws://192.168.1.80/", &[h("Bad Name", "x")]).is_err());
        assert!(ws_request("ws://192.168.1.80/", &[h("X-Ok", "line\nbreak")]).is_err());
    }

    #[test]
    fn a_declared_host_is_a_name_or_its_subdomains() {
        let p = Permissions { hosts: vec!["api.netatmo.com".into(), "*.ezvizlife.com".into()], ..Default::default() };
        assert!(p.allows_name("api.netatmo.com") && p.allows_name("API.Netatmo.com"));
        assert!(p.allows_name("apiieu.ezvizlife.com") && p.allows_name("a.b.ezvizlife.com"));
        assert!(!p.allows_name("ezvizlife.com.evil.net") && !p.allows_name("evilezvizlife.com") && !p.allows_name("netatmo.com"));
    }
}
