//! Modules (docs/roadmap/modules.md): what drives hardware and online services, loaded only when
//! a source or an account uses it.

/// The version of the module interface this server is built against.
pub const INTERFACE: &str = "1.0";

/// Versions of the module interface (`contracts/wit/breezone-module.wit`) this server hosts: none
/// while the WebAssembly host is not built in (the modules compiled into the server need none).
#[cfg(feature = "wasm")]
pub const HOST_API: &[&str] = &[INTERFACE];
#[cfg(not(feature = "wasm"))]
pub const HOST_API: &[&str] = &[];

pub mod compat;
pub mod install;
pub mod library;
pub mod manifest;
pub mod native;
pub mod package;

use manifest::{Manifest, Role};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, RwLock};

/// The modules installed from a package (the library, the archive of Breezone, a file).
static INSTALLED: RwLock<Vec<&'static Manifest>> = RwLock::new(Vec::new());

/// Every module this server knows, in the order of the lists: those compiled in (or the installed
/// module that replaces one of them, same id), then the other installed ones.
pub fn manifests() -> std::vec::IntoIter<&'static Manifest> {
    let installed = INSTALLED.read().unwrap_or_else(|e| e.into_inner());
    let mut all: Vec<&'static Manifest> = native::all().iter().map(|n| installed.iter().find(|m| m.id == n.manifest.id).copied().unwrap_or(&n.manifest)).collect();
    all.extend(installed.iter().filter(|m| native::get(&m.id).is_none()).copied());
    all.into_iter()
}

/// An installed module replaces the one compiled in with the same id (the WebAssembly one runs).
pub fn replaces_native(id: &str) -> bool {
    native::get(id).is_some() && installed_dir(id).is_some()
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

/// The driver of a source: compiled into the server, or run from an installed module's component
/// (loaded and compiled only now, when a source needs it).
/// `source_id`: the source it is for (`None` while its configuration is only checked).
pub async fn open_source(id: &str, source_id: Option<i64>, config: &serde_json::Value, secret: Option<&str>) -> Result<Arc<dyn crate::drivers::Driver>, String> {
    if native::get(id).is_some() && !replaces_native(id) {
        return crate::drivers::create(id, config, secret);
    }
    let m = manifest(id).ok_or_else(|| format!("unknown driver: {id}"))?;
    if !m.has(Role::Driver) {
        return Err(format!("the {id} module has no devices"));
    }
    #[cfg(feature = "wasm")]
    {
        let dir = installed_dir(id).ok_or_else(|| format!("the {id} module is not installed"))?;
        match wasm::source(m, &dir, source_id, config, secret).await {
            // The module compiled in takes over when the one that replaces it cannot start.
            Err(e) if native::get(id).is_some() => {
                log(id, "error", format!("{e}: the module compiled into Breezone takes over"));
                crate::drivers::create(id, config, secret)
            }
            other => other,
        }
    }
    #[cfg(not(feature = "wasm"))]
    {
        let _ = (source_id, config, secret);
        Err(format!("the {id} module needs the module host, which this build of Breezone lacks"))
    }
}

/// The installed (or shipped) modules that answer `contract` ("tariff.day_color@1"), in the
/// order they were installed: those of the `service` role whose `provides.services` has it, and
/// which replace the code of the core when it has the same id.
pub fn service_providers(contract: &str) -> Vec<&'static Manifest> {
    let major = |c: &str| c.split('.').next().map(str::to_string);
    let (name, version) = contract.split_once('@').map(|(n, v)| (n.to_string(), major(v))).unwrap_or_default();
    manifests()
        .filter(|m| m.has(Role::Service) && installed_dir(&m.id).is_some() && (native::get(&m.id).is_none() || replaces_native(&m.id)))
        .filter(|m| m.provides.services.iter().any(|s| s.split_once('@').is_some_and(|(n, v)| n == name && major(v) == version)))
        .collect()
}

/// Asks the modules that provide `contract` for `query`, in turn, and gives the first answer
/// (checked against the contract). A module that is an account is asked only when its account is
/// connected and switched on; how it went is noted on the account. `None`: no module answers
/// this contract here, the core does it itself.
pub async fn ask_service(db: &crate::db::Db, contract: &str, query: &str, params: &serde_json::Value) -> Option<Result<serde_json::Value, String>> {
    #[cfg_attr(not(feature = "wasm"), allow(unused_mut))]
    let mut last: Option<Result<serde_json::Value, String>> = None;
    for m in service_providers(contract) {
        if m.has(Role::Account) && !crate::db::account(db, &m.id).await.ok().flatten().is_some_and(|a| a.enabled && a.secret.is_some()) {
            continue;
        }
        #[cfg(feature = "wasm")]
        {
            let Some(dir) = installed_dir(&m.id) else { continue };
            let answer = match wasm::loaded(m, &dir).await {
                Ok(module) => module.query(contract, query, params).await.map_err(|e| e.to_string()),
                Err(e) => Err(e),
            };
            if m.has(Role::Account) {
                let error = answer.as_ref().err().map(|e| crate::i18n::Text::plain(e.clone()).store());
                let _ = crate::db::set_account_state(db, &m.id, if error.is_none() { "ok" } else { "error" }, error.as_deref()).await;
            }
            match answer {
                Ok(v) => return Some(Ok(v)),
                Err(e) => {
                    log(&m.id, "warn", format!("{contract} {query}: {e}"));
                    last = Some(Err(e));
                }
            }
        }
        #[cfg(not(feature = "wasm"))]
        let _ = (query, params);
    }
    last
}

/// A step of the appairage of a module: the installed (or shipped) module when there is one, else
/// the driver compiled in.
pub async fn pair(id: &str, step: String, mut input: serde_json::Value) -> Result<native::Paired, crate::error::ApiError> {
    use crate::error::ApiError;
    // The module speaks to the user in their language.
    if let Some(o) = input.as_object_mut() {
        o.insert("lang".into(), crate::i18n::current().code().into());
    }
    #[cfg(feature = "wasm")]
    if let (Some(m), Some(dir)) = (manifest(id).filter(|m| m.pairing != manifest::Pairing::Form), installed_dir(id)) {
        if native::get(id).is_none() || replaces_native(id) {
            let module = wasm::loaded(m, &dir).await.map_err(ApiError::bad_request)?;
            return module.pair(&step, &input).await.map_err(|e| ApiError::new(axum::http::StatusCode::BAD_GATEWAY, e.to_string()));
        }
    }
    let native = native::get(id).ok_or_else(|| ApiError::not_found("unknown module"))?;
    let pair = native.pair.ok_or_else(|| ApiError::bad_request("this module is added with its form"))?;
    pair(step, input).await
}

/// What the installed modules (not those compiled in) say of `host`, whose ports `open` are open:
/// each one that probes and listens on one of them is asked.
pub async fn probe_installed(host: &str, open: &[u16]) -> Vec<crate::discovery::Product> {
    let asked: Vec<&'static Manifest> =
        manifests().filter(|m| native::get(&m.id).is_none() && m.optional_calls.iter().any(|c| c == "probe") && m.discovery.tcp.iter().any(|p| open.contains(p))).collect();
    #[cfg_attr(not(feature = "wasm"), allow(unused_mut))]
    let mut found = vec![];
    for m in asked {
        #[cfg(feature = "wasm")]
        {
            let Some(dir) = installed_dir(&m.id) else { continue };
            if let Some(name) = wasm::probe(m, &dir, host).await {
                let port = m.discovery.tcp.iter().copied().find(|p| open.contains(p)).unwrap_or(80);
                found.push(crate::discovery::Product {
                    brand: m.id.as_str(),
                    driver: m.id.as_str(),
                    model: m.name.fr.clone(),
                    host: host.to_string(),
                    port,
                    mac: None,
                    device_id: None,
                    firmware: None,
                    note: Some(format!("« {name} »")),
                    supported: true,
                    flags: 0,
                    install: None,
                });
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
    /// Files of the core that still name a module, and the lot that takes the name away: no other
    /// file may (docs/roadmap/modules.md § 8.4).
    const PENDING: &[(&str, &str)] = &[
        // Accounts and services: their forms, tests, loops and the sources they bring (L9).
        ("api/linked.rs", "L9"),
        ("lib.rs", "L9"),
        ("tempo.rs", "L9"),
        ("config.rs", "L9"),
        ("accounts/mod.rs", "L9"),
    ];

    /// The core names no module but Airzone: what a module is, does and needs is in its manifest.
    #[test]
    fn the_core_names_no_module() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let ids: Vec<&str> = super::manifests().map(|m| m.id.as_str()).collect();
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
                let module_own = rel == "modules/native.rs"
                    || rel == "mock_devices.rs"
                    || ["drivers/", "accounts/"].iter().any(|d| rel.starts_with(d) && !rel.ends_with("/mod.rs"))
                    || PENDING.iter().any(|(f, _)| *f == rel);
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
