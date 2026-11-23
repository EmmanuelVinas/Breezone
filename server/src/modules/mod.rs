//! Modules (docs/roadmap/modules.md): what drives hardware and online services, loaded only when
//! a source or an account uses it.

/// The version of the module interface this server is built against.
pub const INTERFACE: &str = "1.1";

/// Versions of the module interface (`contracts/wit/breezone-module.wit`) this server hosts: none
/// while the WebAssembly host is not built in (only Airzone then, which is part of the core).
#[cfg(feature = "wasm")]
pub const HOST_API: &[&str] = &[INTERFACE];
#[cfg(not(feature = "wasm"))]
pub const HOST_API: &[&str] = &[];

pub mod compat;
pub mod conditions;
pub mod install;
pub mod library;
pub mod manifest;
pub mod package;

use manifest::{Manifest, Role};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, RwLock};

/// Where an appairage stands after a step.
#[derive(Debug)]
pub enum Paired {
    /// Waiting for the next step (the code shown on the accessory…).
    Next,
    /// Something to do on the hardware first (press its button), then the same step again.
    Waiting(String),
    /// Done: the source to add, with its secret and the name to give it if none is given.
    Done { config: serde_json::Value, secret: String, name: String },
}

/// The modules installed from a package (the library, the archive of Breezone, a file).
static INSTALLED: RwLock<Vec<&'static Manifest>> = RwLock::new(Vec::new());

/// Every module this server knows: those installed (shipped with Breezone, from the library, from
/// a file), in the order of the lists.
pub fn manifests() -> std::vec::IntoIter<&'static Manifest> {
    let mut all: Vec<&'static Manifest> = INSTALLED.read().unwrap_or_else(|e| e.into_inner()).clone();
    all.sort_by_key(|m| m.name.fr.to_lowercase());
    all.into_iter()
}

/// Where each installed module's files are, by id.
static DIRS: LazyLock<RwLock<HashMap<String, PathBuf>>> = LazyLock::new(Default::default);

/// The folder of an installed module (its manifest, component, words and icon).
pub fn installed_dir(id: &str) -> Option<PathBuf> {
    DIRS.read().unwrap_or_else(|e| e.into_inner()).get(id).cloned()
}

/// Takes an installed module into account (in place of its previous version), its files in `dir`.
pub fn register(m: Manifest, dir: PathBuf) {
    DIRS.write().unwrap_or_else(|e| e.into_inner()).insert(m.id.clone(), dir);
    // A few hundred bytes per installation, kept for the life of the process.
    let m: &'static Manifest = Box::leak(Box::new(m));
    let mut installed = INSTALLED.write().unwrap_or_else(|e| e.into_inner());
    installed.retain(|x| x.id != m.id);
    installed.push(m);
}

pub fn unregister(id: &str) {
    DIRS.write().unwrap_or_else(|e| e.into_inner()).remove(id);
    INSTALLED.write().unwrap_or_else(|e| e.into_inner()).retain(|x| x.id != id);
}

/// A line of a module's log.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LogLine {
    pub at: i64,
    /// "debug" | "info" | "warn" | "error"
    pub level: &'static str,
    pub message: String,
}

/// Lines kept per module, the latest.
const LOG_LINES: usize = 200;

static LOGS: LazyLock<std::sync::Mutex<HashMap<String, std::collections::VecDeque<LogLine>>>> = LazyLock::new(Default::default);

/// Writes to a module's log (what it says, and what befalls it: a stop, a call refused).
pub fn log(id: &str, level: &'static str, message: impl Into<String>) {
    let mut logs = LOGS.lock().unwrap_or_else(|e| e.into_inner());
    let lines = logs.entry(id.to_string()).or_default();
    if lines.len() == LOG_LINES {
        lines.pop_front();
    }
    let message: String = message.into().chars().take(1000).collect();
    lines.push_back(LogLine { at: crate::db::now(), level, message });
}

/// The log of a module, oldest first.
pub fn logs(id: &str) -> Vec<LogLine> {
    LOGS.lock().unwrap_or_else(|e| e.into_inner()).get(id).map(|l| l.iter().cloned().collect()).unwrap_or_default()
}

/// The time zone of this server, given to the modules that ask for it (`server.timezone` in their
/// `fills`), set at start.
static TIMEZONE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

pub fn set_timezone(tz: &str) {
    let _ = TIMEZONE.set(tz.to_string());
}

/// The configuration of a source with what Breezone fills itself at each start (`server.timezone`
/// of its manifest's `fills`): a source added before the module asked for it gets it too.
#[cfg_attr(not(feature = "wasm"), allow(dead_code))]
fn filled(m: &Manifest, config: &serde_json::Value) -> serde_json::Value {
    let mut config = config.clone();
    if let (Some(o), Some(tz)) = (config.as_object_mut(), TIMEZONE.get()) {
        for (key, from) in &m.fills {
            if from == "server.timezone" {
                o.insert(key.clone(), tz.clone().into());
            }
        }
    }
    config
}

/// The driver of a source, run from its installed module's component (loaded and compiled only
/// now, when a source needs it).
/// `source_id`: the source it is for (`None` while its configuration is only checked).
pub async fn open_source(id: &str, source_id: Option<i64>, config: &serde_json::Value, secret: Option<&str>) -> Result<Arc<dyn crate::drivers::Driver>, String> {
    let m = manifest(id).ok_or_else(|| format!("unknown driver: {id}"))?;
    if !m.has(Role::Driver) {
        return Err(format!("the {id} module has no devices"));
    }
    #[cfg(feature = "wasm")]
    {
        let dir = installed_dir(id).ok_or_else(|| format!("the {id} module is not installed"))?;
        let opened = wasm::source(m, &dir, source_id, &filled(m, config), secret).await;
        if let Err(e) = &opened {
            log(id, "error", e.clone());
        }
        opened
    }
    #[cfg(not(feature = "wasm"))]
    {
        let _ = (source_id, config, secret);
        Err(format!("the {id} module needs the module host, which this build of Breezone lacks"))
    }
}

/// The installed (or shipped) modules that answer `contract` ("tariff.day_color@1"), in the
/// order of the lists: those of the `service` role whose `provides.services` has it.
pub fn service_providers(contract: &str) -> Vec<&'static Manifest> {
    let major = |c: &str| c.split('.').next().map(str::to_string);
    let (name, version) = contract.split_once('@').map(|(n, v)| (n.to_string(), major(v))).unwrap_or_default();
    manifests()
        .filter(|m| m.has(Role::Service) && installed_dir(&m.id).is_some())
        .filter(|m| m.provides.services.iter().any(|s| s.split_once('@').is_some_and(|(n, v)| n == name && major(v) == version)))
        .collect()
}

/// Asks the module that provides a service the core uses by itself (`internal` in its manifest:
/// the protocol of the Airzone WebServer) for `query`; its error as it is (unreachable or not).
pub async fn ask_internal(contract: &str, query: &str, params: &serde_json::Value) -> Result<serde_json::Value, crate::drivers::DriverError> {
    use crate::drivers::DriverError;
    let m = service_providers(contract).into_iter().next().ok_or_else(|| DriverError::Unsupported(format!("no module answers {contract}: Breezone's own modules are missing")))?;
    #[cfg(feature = "wasm")]
    {
        let dir = installed_dir(&m.id).ok_or_else(|| DriverError::Unsupported(format!("the {} module is not installed", m.id)))?;
        let module = wasm::loaded(m, &dir).await.map_err(DriverError::Invalid)?;
        module.query(contract, query, params).await
    }
    #[cfg(not(feature = "wasm"))]
    {
        let _ = (query, params);
        Err(DriverError::Unsupported(format!("the {} module needs the module host, which this build of Breezone lacks", m.id)))
    }
}

/// Asks the modules that provide `contract` for `query`, in turn, and gives the first answer
/// (checked against the contract). A module that is an account is asked only when its account is
/// connected and switched on; how it went is noted on the account. `None`: no module answers
/// this contract here.
pub async fn ask_service(db: &crate::db::Db, contract: &str, query: &str, params: &serde_json::Value) -> Option<Result<serde_json::Value, String>> {
    let mut last: Option<Result<serde_json::Value, String>> = None;
    for m in service_providers(contract) {
        match ask(db, m, contract, query, params).await {
            Some(Ok(v)) => return Some(Ok(v)),
            Some(Err(e)) => last = Some(Err(e)),
            None => {}
        }
    }
    last
}

/// Asks one module a query of a service contract (`None`: its account is not connected, or the
/// modules cannot run here).
pub async fn ask(db: &crate::db::Db, m: &Manifest, contract: &str, query: &str, params: &serde_json::Value) -> Option<Result<serde_json::Value, String>> {
    if m.has(Role::Account) && !crate::db::account(db, &m.id).await.ok().flatten().is_some_and(|a| a.enabled && a.secret.is_some()) {
        return None;
    }
    #[cfg(feature = "wasm")]
    {
        let dir = installed_dir(&m.id)?;
        let answer = match wasm::loaded(m, &dir).await {
            Ok(module) => module.query(contract, query, params).await.map_err(|e| e.to_string()),
            Err(e) => Err(e),
        };
        if m.has(Role::Account) {
            let error = answer.as_ref().err().map(|e| crate::i18n::Text::plain(e.clone()).store());
            let _ = crate::db::set_account_state(db, &m.id, if error.is_none() { "ok" } else { "error" }, error.as_deref()).await;
        }
        if let Err(e) = &answer {
            log(&m.id, "warn", format!("{contract} {query}: {e}"));
        }
        Some(answer)
    }
    #[cfg(not(feature = "wasm"))]
    {
        let _ = (contract, query, params);
        None
    }
}

/// A step of the appairage of a module.
pub async fn pair(id: &str, step: String, mut input: serde_json::Value) -> Result<Paired, crate::error::ApiError> {
    use crate::error::ApiError;
    // The module speaks to the user in their language.
    if let Some(o) = input.as_object_mut() {
        o.insert("lang".into(), crate::i18n::current().code().into());
    }
    let m = manifest(id).ok_or_else(|| ApiError::not_found("unknown module"))?;
    if matches!(m.pairing, manifest::Pairing::Form | manifest::Pairing::Oauth) {
        return Err(ApiError::bad_request("this module is added with its form"));
    }
    #[cfg(feature = "wasm")]
    {
        let dir = installed_dir(id).ok_or_else(|| ApiError::not_found("unknown module"))?;
        let module = wasm::loaded(m, &dir).await.map_err(ApiError::bad_request)?;
        // What was typed or the hardware refuses (a wrong code) is the user's to fix, a step the
        // module does not know is a bad request.
        module.pair(&step, &input).await.map_err(|e| {
            let status = match &e {
                crate::drivers::DriverError::Invalid(_) => axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                crate::drivers::DriverError::Unsupported(_) => axum::http::StatusCode::BAD_REQUEST,
                crate::drivers::DriverError::Unreachable(_) => axum::http::StatusCode::BAD_GATEWAY,
            };
            ApiError::new(status, e.to_string())
        })
    }
    #[cfg(not(feature = "wasm"))]
    {
        let _ = (step, input);
        Err(ApiError::bad_request(format!("the {id} module needs the module host, which this build of Breezone lacks")))
    }
}

/// A product found by a module: what it said of the hardware (`identify`), or its name only
/// (`probe`, a module built before `identify`).
#[cfg_attr(not(feature = "wasm"), allow(dead_code))]
fn found_product(m: &'static Manifest, host: &str, port: u16, said: &serde_json::Value) -> crate::discovery::Product {
    let text = |k: &str| said[k].as_str().map(str::trim).filter(|v| !v.is_empty()).map(|v| v.chars().take(200).collect::<String>());
    crate::discovery::Product {
        brand: m.id.as_str(),
        driver: m.id.as_str(),
        model: text("model").unwrap_or_else(|| m.name.fr.clone()),
        host: text("host").filter(|h| h.split(':').next() == host.split(':').next()).unwrap_or_else(|| host.to_string()),
        port: said["port"].as_u64().and_then(|p| u16::try_from(p).ok()).unwrap_or(port),
        mac: text("mac"),
        device_id: text("device_id"),
        firmware: text("firmware"),
        note: text("note"),
        supported: said["supported"].as_bool().unwrap_or(true),
        flags: said["flags"].as_u64().and_then(|f| u32::try_from(f).ok()).unwrap_or(0),
        install: None,
    }
}

fn identifies(m: &Manifest) -> bool {
    m.optional_calls.iter().any(|c| c == "identify")
}

/// What the installed modules say of `host`, whose ports `open` are open: each one that listens on
/// one of them and identifies (or probes) its hardware is asked.
pub async fn identify_at(host: &str, open: &[u16]) -> Vec<crate::discovery::Product> {
    let asked: Vec<&'static Manifest> =
        manifests().filter(|m| (identifies(m) || m.optional_calls.iter().any(|c| c == "probe")) && m.discovery.tcp.iter().any(|p| open.contains(p))).collect();
    #[cfg_attr(not(feature = "wasm"), allow(unused_mut))]
    let mut found = vec![];
    for m in asked {
        #[cfg(feature = "wasm")]
        {
            let Some(dir) = installed_dir(&m.id) else { continue };
            let port = m.discovery.tcp.iter().copied().find(|p| open.contains(p)).unwrap_or(80);
            if identifies(m) {
                if let Some(said) = wasm::identify(m, &dir, host, None).await {
                    found.push(found_product(m, host, port, &said));
                }
            } else if let Some(name) = wasm::probe(m, &dir, host).await {
                found.push(found_product(m, host, port, &serde_json::json!({"note": format!("« {name} »")})));
            }
        }
        #[cfg(not(feature = "wasm"))]
        let _ = (m, host);
    }
    found
}

/// The mDNS services the installed modules announce their hardware with.
pub fn announced_services() -> Vec<String> {
    let mut services: Vec<String> = manifests().filter(|m| m.has(Role::Driver)).flat_map(|m| m.discovery.mdns.iter().map(|h| h.service.clone())).collect();
    services.sort();
    services.dedup();
    services
}

/// What the installed modules say of the hardware that announced itself by mDNS (nothing is asked
/// of it: the gateways already added keep their few connections for their sessions).
pub async fn identify_announced(announced: &[crate::mdns::Service]) -> Vec<crate::discovery::Product> {
    let same = |a: &str, b: &str| a.trim_end_matches(".local").eq_ignore_ascii_case(b.trim_end_matches(".local"));
    #[cfg_attr(not(feature = "wasm"), allow(unused_mut))]
    let mut found = vec![];
    for s in announced {
        let Some(m) = manifests().find(|m| m.has(Role::Driver) && m.discovery.mdns.iter().any(|h| same(&h.service, &s.service) && crate::mdns::txt_matches(s, &h.txt))) else {
            continue;
        };
        let host = s.ip.to_string();
        #[cfg(feature = "wasm")]
        {
            let said = match installed_dir(&m.id) {
                Some(dir) if identifies(m) => {
                    let txt: serde_json::Map<String, serde_json::Value> = s.txt.iter().map(|(k, v)| (k.clone(), v.clone().into())).collect();
                    let announce = serde_json::json!({"service": s.service, "name": s.name, "port": s.port, "txt": txt});
                    wasm::identify(m, &dir, &host, Some(&announce)).await
                }
                _ => Some(serde_json::json!({"note": s.name})),
            };
            if let Some(said) = said {
                found.push(found_product(m, &host, s.port, &said));
            }
        }
        #[cfg(not(feature = "wasm"))]
        let _ = (m, host);
    }
    found
}

/// Where the installed modules are kept: `<data>/modules/<id>/<version>/`.
pub fn modules_dir(data_dir: &std::path::Path) -> std::path::PathBuf {
    data_dir.join("modules")
}

pub fn manifest(id: &str) -> Option<&'static Manifest> {
    manifests().find(|m| m.id == id)
}

/// The modules that have `role`.
pub fn with_role(role: Role) -> impl Iterator<Item = &'static Manifest> {
    manifests().filter(move |m| m.has(role))
}

#[cfg(feature = "wasm")]
pub mod wasm;

#[cfg(test)]
mod tests {
    /// Files of the core that still name a module, and why: no other file may
    /// (docs/roadmap/modules.md § 8.4).
    const PENDING: &[(&str, &str)] = &[
        // What the core does with these services itself: the history of the meter (Linky), the
        // panels set with the position of the home (Forecast.Solar), the switch of the weather.
        ("lib.rs", "linky"),
        ("api/linked.rs", "linky, forecast_solar"),
        ("accounts/mod.rs", "weather"),
    ];

    /// The core names no module but Airzone: what a module is, does and needs is in its manifest.
    #[test]
    fn the_core_names_no_module() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        // The modules of Breezone (`modules/`), installed or not.
        let modules = root.join("../../modules");
        let ids: Vec<String> =
            std::fs::read_dir(&modules).unwrap().flatten().filter(|e| e.path().join("manifest.json").is_file()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert!(ids.len() > 10, "{ids:?}");
        let mut found = vec![];
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                let rel = path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
                let module_own =
                    rel == "mock_devices.rs" || ["drivers/", "accounts/"].iter().any(|d| rel.starts_with(d) && !rel.ends_with("/mod.rs")) || PENDING.iter().any(|(f, _)| *f == rel);
                if module_own || !rel.ends_with(".rs") {
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap();
                // Tests may name the modules they set up.
                let code = text.split("#[cfg(test)]").next().unwrap_or_default();
                for id in &ids {
                    for needle in [format!("\"{id}\""), format!("drivers::{id}::"), format!("accounts::{id}::")] {
                        if code.contains(&needle) {
                            found.push(format!("{rel}: {needle}"));
                        }
                    }
                }
            }
        }
        assert!(found.is_empty(), "the core names modules (move it to the module, or its manifest):\n{}", found.join("\n"));
    }
}
