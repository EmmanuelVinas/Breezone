//! Host of the modules compiled to WebAssembly (docs/roadmap/modules.md § 4.5): one engine for the
//! server, a module compiled once (and kept compiled on disk), one instance per source.
//!
//! A module sees nothing of the system: no files, no environment, no sockets of WASI. It reaches
//! the network only through `host.http`, which refuses any address its permissions do not allow.
//! Each call has a deadline and a memory limit; a module that traps (panic, loop, memory) only puts
//! its source in error, and is instantiated again at the next call.

use crate::drivers::{Described, DeviceInfo, Driver, DriverError, DriverFuture, State};
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
    imports: { "breezone:module/host.http": async, default: trappable },
    exports: { default: async },
});

use breezone::module::types::{Error as WitError, Header, Request, Response};

/// Ticks of the engine's epoch: one each `TICK`.
const TICK: Duration = Duration::from_millis(100);
/// Longest time a call into a module may compute (time spent waiting for the network excluded).
const CALL_TICKS: u64 = 100;
/// Longest time a call may take, waits for the network included.
const CALL_TIMEOUT: Duration = Duration::from_secs(60);
/// Largest body of an HTTP answer handed to a module.
const MAX_BODY: usize = 4 << 20;

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

struct HostState {
    module: Arc<str>,
    permissions: Arc<Permissions>,
    wasi: WasiCtx,
    table: ResourceTable,
    limits: StoreLimits,
}

impl WasiView for HostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView { ctx: &mut self.wasi, table: &mut self.table }
    }
}

impl breezone::module::types::Host for HostState {}

impl breezone::module::host::Host for HostState {
    fn log(&mut self, level: breezone::module::host::Level, message: String) -> wasmtime::Result<()> {
        use breezone::module::host::Level;
        let module = &*self.module;
        match level {
            Level::Debug => tracing::debug!(module, "{message}"),
            Level::Info => tracing::info!(module, "{message}"),
            Level::Warn => tracing::warn!(module, "{message}"),
            Level::Error => tracing::error!(module, "{message}"),
        }
        Ok(())
    }

    fn now(&mut self) -> wasmtime::Result<i64> {
        Ok(chrono::Utc::now().timestamp())
    }

    async fn http(&mut self, req: Request) -> Result<Response, String> {
        http(&self.permissions, req).await
    }
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
        Ok(Self { id: id.into(), read_every: Duration::ZERO, component, linker: Arc::new(linker), permissions: Arc::new(permissions) })
    }

    async fn instantiate(&self) -> Result<(Store<HostState>, Module), DriverError> {
        let wasi = WasiCtxBuilder::new().allow_tcp(false).allow_udp(false).allow_ip_name_lookup(false).socket_addr_check(|_, _| Box::pin(async { false })).build();
        let memory = self.permissions.memory_mb.clamp(1, 64) << 20;
        let limits = StoreLimitsBuilder::new().memory_size(memory).instances(16).trap_on_grow_failure(true).build();
        let state = HostState { module: self.id.clone(), permissions: self.permissions.clone(), wasi, table: ResourceTable::new(), limits };
        let mut store = Store::new(engine(), state);
        store.limiter(|s| &mut s.limits);
        store.epoch_deadline_trap();
        store.set_epoch_deadline(CALL_TICKS);
        let module = Module::instantiate_async(&mut store, &self.component, &self.linker).await.map_err(|e| DriverError::Invalid(format!("{}: {e}", self.id)))?;
        Ok((store, module))
    }

    /// Whether `host` is hardware of this module (discovery), in an instance of its own.
    pub async fn probe(&self, host: &str) -> Option<String> {
        let (mut store, module) = self.instantiate().await.ok()?;
        tokio::time::timeout(CALL_TIMEOUT, module.breezone_module_driver().call_probe(&mut store, host)).await.ok()?.ok()?
    }

    /// A source of this module.
    pub async fn open(self: &Arc<Self>, config: &serde_json::Value, secret: Option<&str>) -> Result<WasmSource, DriverError> {
        let source = WasmSource { module: self.clone(), config: config.to_string(), secret: secret.map(str::to_string), live: Mutex::new(None), every: self.read_every };
        // Checks the configuration at once, as the native drivers do.
        let mut live = source.live.lock().await;
        *live = Some(source.start().await?);
        drop(live);
        Ok(source)
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

impl WasmSource {
    async fn start(&self) -> Result<Live, DriverError> {
        let (mut store, module) = self.module.instantiate().await?;
        let guest = module.breezone_module_driver().source();
        let open = guest.call_open(&mut store, &self.config, self.secret.as_deref());
        let source = tokio::time::timeout(CALL_TIMEOUT, open)
            .await
            .map_err(|_| DriverError::Unreachable(format!("{}: too long", self.module.id)))?
            .map_err(|e| DriverError::Invalid(format!("{}: {e}", self.module.id)))?
            .map_err(from_wit)?;
        Ok(Live { store, module, source })
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
                Err(DriverError::Invalid(format!("{}: {trap}", self.module.id)))
            }
            Err(_) => {
                *guard = None;
                Err(DriverError::Unreachable(format!("{}: too long", self.module.id)))
            }
        }
    }
}

impl Driver for WasmSource {
    fn read_every(&self) -> Duration {
        self.every
    }

    fn describe(&self) -> DriverFuture<'_, Described> {
        Box::pin(async move {
            let d = self.call(|l| Box::pin(async move { l.module.breezone_module_driver().source().call_describe(&mut l.store, l.source).await })).await?;
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
            let list = self.call(|l| Box::pin(async move { l.module.breezone_module_driver().source().call_read(&mut l.store, l.source).await })).await?;
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
        Box::pin(async move {
            self.call(move |l| {
                Box::pin(async move { l.module.breezone_module_driver().source().call_act(&mut l.store, l.source, &reference, &trait_name, &action, &params).await })
            })
            .await
        })
    }

    fn diagnostic(&self) -> DriverFuture<'_, serde_json::Value> {
        Box::pin(async move {
            let json = self.call(|l| Box::pin(async move { l.module.breezone_module_driver().source().call_diagnostic(&mut l.store, l.source).await })).await?;
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

/// A source of an installed module (`dir`: its files): the component is compiled the first time
/// one of its sources is opened (and kept compiled in `<modules>/cache`), with the permissions of
/// its manifest.
pub async fn source(m: &crate::modules::manifest::Manifest, dir: &Path, config: &serde_json::Value, secret: Option<&str>) -> Result<Arc<dyn Driver>, String> {
    let key = (m.id.clone(), m.version.clone());
    let module = {
        let mut loaded = LOADED.lock().await;
        // Another version than the one loaded: the older is dropped.
        loaded.retain(|(id, version), _| id != &m.id || version == &m.version);
        match loaded.get(&key) {
            Some(module) => module.clone(),
            None => {
                let wasm = std::fs::read(dir.join("module.wasm")).map_err(|e| format!("{}: {e}", m.id))?;
                let net = &m.permissions.network;
                let permissions = Permissions {
                    lan: net.lan,
                    hosts: net.hosts.clone(),
                    loopback: LOOPBACK.load(std::sync::atomic::Ordering::SeqCst),
                    memory_mb: m.permissions.memory_mb as usize,
                };
                let cache = dir.parent().and_then(Path::parent).map(|modules| modules.join("cache"));
                let id = m.id.clone();
                let mut module = tokio::task::spawn_blocking(move || WasmModule::load(&id, &wasm, permissions, cache.as_deref()))
                    .await
                    .map_err(|e| e.to_string())?
                    .map_err(|e| format!("{}: {e}", m.id))?;
                module.read_every = Duration::from_secs(u64::from(m.read_every_s.unwrap_or(0)));
                let module = Arc::new(module);
                loaded.insert(key, module.clone());
                module
            }
        }
    };
    Ok(Arc::new(module.open(config, secret).await.map_err(|e| e.to_string())?))
}
