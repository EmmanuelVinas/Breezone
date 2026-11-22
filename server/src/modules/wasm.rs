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
    world: "module",
    imports: {
        "breezone:module/host.http": async,
        "breezone:module/host.subscribe": async,
        "breezone:module/host.store-get": async,
        "breezone:module/host.store-set": async,
        "breezone:module/host.account": async,
        "breezone:module/host.exchange": async,
        "breezone:module/host.wait": async,
        default: trappable,
    },
    exports: { default: async },
});

use breezone::module::types::{Error as WitError, Header, ReadUntil, Request, Response};

/// Ticks of the engine's epoch: one each `TICK`.
const TICK: Duration = Duration::from_millis(100);
/// Longest time a call into a module may compute (time spent waiting for the network excluded).
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

    fn allows_name(&self, host: &str) -> bool {
        self.hosts.iter().any(|h| h.eq_ignore_ascii_case(host))
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

/// What a source shares with the host functions of its instances: who it is, where it tells what
/// it hears, and the messages of its subscriptions, handed to `on-message` by its pump.
struct SourceCtx {
    module: Arc<str>,
    /// The source (`None` while its configuration is only checked: its store is then in memory).
    source_id: Option<i64>,
    sink: std::sync::Mutex<Option<Sink>>,
    messages: tokio::sync::mpsc::UnboundedSender<(u32, Vec<u8>)>,
    memory_store: std::sync::Mutex<HashMap<String, Vec<u8>>>,
    /// The fields of the module's own account while it is checked, before Breezone keeps them.
    checked_account: Option<String>,
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
        tokio::time::sleep(Duration::from_millis(u64::from(ms.min(10_000)))).await;
    }

    async fn http(&mut self, req: Request) -> Result<Response, String> {
        let url = req.url.clone();
        let r = http(&self.permissions, req).await;
        if let Err(e) = &r {
            crate::modules::log(&self.ctx.module, "warn", format!("{url}: {e}"));
        }
        r
    }

    async fn exchange(&mut self, url: String, data: Vec<u8>, until: ReadUntil, timeout_ms: u32) -> Result<Vec<u8>, String> {
        let r = exchange(&self.permissions, &url, &data, until, timeout_ms).await;
        if let Err(e) = &r {
            crate::modules::log(&self.ctx.module, "warn", format!("{url}: {e}"));
        }
        r
    }

    async fn subscribe(&mut self, url: String) -> Result<u32, String> {
        if self.subscriptions.len() >= MAX_SUBSCRIPTIONS {
            return Err(format!("at most {MAX_SUBSCRIPTIONS} subscriptions"));
        }
        self.next_subscription += 1;
        let id = self.next_subscription;
        let sub = subscribe(&self.permissions, &url, id, self.ctx.messages.clone()).await.inspect_err(|e| crate::modules::log(&self.ctx.module, "warn", format!("{url}: {e}")))?;
        self.subscriptions.insert(id, sub);
        Ok(id)
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
        match (self.ctx.source_id, context()) {
            (Some(id), Some((db, _))) => crate::db::store_get(&db, id, &key).await.ok().flatten(),
            _ => self.ctx.memory_store.lock().unwrap_or_else(|e| e.into_inner()).get(&key).cloned(),
        }
    }

    async fn store_set(&mut self, key: String, value: Option<Vec<u8>>) {
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

    async fn account(&mut self, kind: String) -> Result<String, String> {
        // Only the accounts its manifest requires.
        if !self.permissions.accounts.contains(&kind) {
            return Err(format!("{kind}: not an account this module requires"));
        }
        if let Some(fields) = self.ctx.checked_account.as_ref().filter(|_| *kind == *self.ctx.module) {
            return Ok(fields.clone());
        }
        let (db, key) = context().ok_or("no account here")?;
        let row = crate::db::account(&db, &kind).await.map_err(|e| e.to_string())?.ok_or_else(|| format!("{kind}: not connected"))?;
        if !row.enabled {
            return Err(format!("{kind}: switched off"));
        }
        row.secret.as_deref().and_then(|s| crate::crypto::decrypt(&key, s)).ok_or_else(|| format!("{kind}: not connected"))
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

/// Opens a subscription (`ws://`, `wss://` or `tcp://`) to an allowed address, and the task that hands its
/// messages to `messages` and writes what is sent on it.
async fn subscribe(permissions: &Permissions, url: &str, id: u32, messages: tokio::sync::mpsc::UnboundedSender<(u32, Vec<u8>)>) -> Result<Subscription, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let parsed = reqwest::Url::parse(url).map_err(|e| e.to_string())?;
    let addrs = permissions.resolve(&parsed).await?;
    let connect = tokio::time::timeout(Duration::from_secs(10), tokio::net::TcpStream::connect(&addrs[..]));
    let (out, mut outgoing) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    let task = match parsed.scheme() {
        scheme @ ("ws" | "wss") => {
            let tcp = connect.await.map_err(|_| "no answer".to_string())?.map_err(|e| e.to_string())?;
            if scheme == "wss" {
                // A device on the local network has its own certificate; a service, a real one.
                let device = addrs.iter().all(|a| local(a.ip()));
                let config = crate::tls::client_config(device).ok_or("no TLS")?;
                let name = rustls::pki_types::ServerName::try_from(parsed.host_str().unwrap_or_default().to_string()).map_err(|e| e.to_string())?;
                let tls = tokio_rustls::TlsConnector::from(Arc::new(config)).connect(name, tcp).await.map_err(|e| e.to_string())?;
                let (ws, _) = tokio_tungstenite::client_async(url, tls).await.map_err(|e| e.to_string())?;
                websocket(ws, id, messages, outgoing)
            } else {
                let (ws, _) = tokio_tungstenite::client_async(url, tcp).await.map_err(|e| e.to_string())?;
                websocket(ws, id, messages, outgoing)
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
/// what comes on `outgoing` (text when it is UTF-8).
fn websocket<S>(
    ws: tokio_tungstenite::WebSocketStream<S>,
    id: u32,
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
                            Ok(text) => Message::Text(text.into()),
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

async fn http(permissions: &Permissions, req: Request) -> Result<Response, String> {
    let url = reqwest::Url::parse(&req.url).map_err(|e| e.to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!("{}: only http and https", url.scheme()));
    }
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
}

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
                        c
                    }
                }
            }
            None => Component::new(engine, wasm).map_err(|e| anyhow::anyhow!("{e}"))?,
        };
        let mut linker = Linker::new(engine);
        wasmtime_wasi::p2::add_to_linker_async(&mut linker).map_err(|e| anyhow::anyhow!("{e}"))?;
        Module::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |s| s).map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(Self { id: id.into(), read_every: Duration::ZERO, optional: vec![], component, linker: Arc::new(linker), permissions: Arc::new(permissions) })
    }

    /// The calls it answers beyond the required ones.
    pub fn with_optional_calls(mut self, calls: &[String]) -> Self {
        self.optional = calls.to_vec();
        self
    }

    async fn instantiate(&self, ctx: Arc<SourceCtx>) -> Result<(Store<HostState>, Module), DriverError> {
        let wasi = WasiCtxBuilder::new().allow_tcp(false).allow_udp(false).allow_ip_name_lookup(false).socket_addr_check(|_, _| Box::pin(async { false })).build();
        let memory = self.permissions.memory_mb.clamp(1, 64) << 20;
        let limits = StoreLimitsBuilder::new().memory_size(memory).instances(16).trap_on_grow_failure(true).build();
        let state = HostState { permissions: self.permissions.clone(), ctx, subscriptions: HashMap::new(), next_subscription: 0, wasi, table: ResourceTable::new(), limits };
        let mut store = Store::new(engine(), state);
        store.limiter(|s| &mut s.limits);
        store.epoch_deadline_trap();
        store.set_epoch_deadline(CALL_TICKS);
        let module = Module::instantiate_async(&mut store, &self.component, &self.linker).await.map_err(|e| DriverError::Invalid(format!("{}: {e}", self.id)))?;
        Ok((store, module))
    }

    fn context(&self, source_id: Option<i64>) -> (Arc<SourceCtx>, tokio::sync::mpsc::UnboundedReceiver<(u32, Vec<u8>)>) {
        let (messages, received) = tokio::sync::mpsc::unbounded_channel();
        let ctx = SourceCtx { module: self.id.clone(), source_id, sink: std::sync::Mutex::new(None), messages, memory_store: Default::default(), checked_account: None };
        (Arc::new(ctx), received)
    }

    /// Whether `host` is hardware of this module (discovery), in an instance of its own.
    pub async fn probe(&self, host: &str) -> Option<String> {
        let (ctx, _) = self.context(None);
        let (mut store, module) = self.instantiate(ctx).await.ok()?;
        tokio::time::timeout(CALL_TIMEOUT, module.breezone_module_driver().call_probe(&mut store, host)).await.ok()?.ok()?
    }

    /// Checks the module's own account (`fields`: those of its form, as JSON) before it is kept.
    pub async fn check_account(&self, fields: &str) -> Result<(), DriverError> {
        let (ctx, _) = self.context(None);
        let ctx = Arc::new(SourceCtx { checked_account: Some(fields.to_string()), ..Arc::into_inner(ctx).ok_or_else(|| DriverError::Invalid("context".into()))? });
        let (mut store, module) = self.instantiate(ctx).await?;
        tokio::time::timeout(CALL_TIMEOUT, module.breezone_module_service().call_check(&mut store))
            .await
            .map_err(|_| DriverError::Unreachable(format!("{}: no answer", self.id)))?
            .map_err(|e| DriverError::Invalid(format!("{}: {e}", self.id)))?
            .map_err(from_wit)
    }

    /// A query of a service contract (`contract`: "tariff.day_color@1"), in an instance of its own;
    /// the answer is checked against the contract.
    pub async fn query(&self, contract: &str, query: &str, params: &serde_json::Value) -> Result<serde_json::Value, DriverError> {
        let (ctx, _) = self.context(None);
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

    /// A step of the appairage, in an instance of its own.
    pub async fn pair(&self, step: &str, input: &serde_json::Value) -> Result<crate::modules::native::Paired, DriverError> {
        use crate::modules::native::Paired;
        use breezone::module::types::Paired as P;
        let (ctx, _) = self.context(None);
        let (mut store, module) = self.instantiate(ctx).await?;
        let answer = tokio::time::timeout(CALL_TIMEOUT, module.breezone_module_driver().call_pair(&mut store, step, &input.to_string()))
            .await
            .map_err(|_| DriverError::Unreachable(format!("{}: no answer", self.id)))?
            .map_err(|e| DriverError::Invalid(format!("{}: {e}", self.id)))?;
        match answer.map_err(from_wit)? {
            P::Next => Ok(Paired::Next),
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
        tokio::spawn(pump(Arc::downgrade(&source), received));
        Ok(source)
    }
}

/// Hands the messages of a source's subscriptions to its module, until the source is dropped.
async fn pump(source: std::sync::Weak<WasmSource>, mut received: tokio::sync::mpsc::UnboundedReceiver<(u32, Vec<u8>)>) {
    while let Some((subscription, data)) = received.recv().await {
        let Some(source) = source.upgrade() else { break };
        let result =
            source.call(move |l| Box::pin(async move { l.module.breezone_module_driver().source().call_on_message(&mut l.store, l.source, subscription, &data).await })).await;
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
        live.store.set_epoch_deadline(CALL_TICKS);
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

/// The modules compiled in this process, by id and version.
type Loaded = HashMap<(String, String), Arc<WasmModule>>;
static LOADED: std::sync::LazyLock<Mutex<Loaded>> = std::sync::LazyLock::new(Default::default);

/// An installed module, compiled (the first time one of its sources needs it, kept compiled in
/// `<modules>/cache`), with the permissions of its manifest.
pub async fn loaded(m: &crate::modules::manifest::Manifest, dir: &Path) -> Result<Arc<WasmModule>, String> {
    let key = (m.id.clone(), m.version.clone());
    let mut loaded = LOADED.lock().await;
    // Another version than the one loaded: the older is dropped.
    loaded.retain(|(id, version), _| id != &m.id || version == &m.version);
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
    };
    let cache = dir.parent().and_then(Path::parent).map(|modules| modules.join("cache"));
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
