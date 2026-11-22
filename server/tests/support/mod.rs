//! What the tests share: a server set up as the image is, with the modules shipped with Breezone
//! (they drive the hardware: there are no drivers compiled in any more), and the fakes they need.
#![allow(dead_code)]

pub mod hap;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub fn modules_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../modules")
}

/// Builds the modules once per process (`modules/`, target `wasm32-wasip2`).
pub fn build_modules() {
    static BUILT: OnceLock<()> = OnceLock::new();
    BUILT.get_or_init(|| {
        let status = std::process::Command::new(env!("CARGO"))
            .args(["build", "--release", "--target", "wasm32-wasip2"])
            .current_dir(modules_dir())
            .status()
            .expect("cargo build of the modules");
        assert!(status.success(), "the modules do not build");
    });
}

/// The component of a module, built.
pub fn wasm(name: &str) -> Vec<u8> {
    build_modules();
    std::fs::read(modules_dir().join(format!("target/wasm32-wasip2/release/breezone_module_{name}.wasm"))).expect("module built")
}

/// The modules shipped with Breezone (`SHIPPED_MODULES` of the Dockerfile).
pub fn shipped() -> Vec<String> {
    let dockerfile = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../Dockerfile")).expect("Dockerfile");
    let line = dockerfile.lines().find_map(|l| l.strip_prefix("ARG SHIPPED_MODULES=\"")).expect("SHIPPED_MODULES");
    line.trim_end_matches('"').split_whitespace().map(str::to_string).collect()
}

/// `app/web` of an install made as the image is: its `app/modules` has the shipped modules,
/// packed once per process. The modules may reach this machine (the fakes listen on it), and the
/// servers of the tests share the modules they compile.
pub fn app_web() -> PathBuf {
    static APP: OnceLock<PathBuf> = OnceLock::new();
    APP.get_or_init(|| {
        let app = std::env::temp_dir().join(format!("breezone-test-app-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&app);
        std::fs::create_dir_all(app.join("modules")).unwrap();
        std::fs::create_dir_all(app.join("web")).unwrap();
        for id in shipped() {
            let manifest = std::fs::read_to_string(modules_dir().join(&id).join("manifest.json")).unwrap();
            let package = breezone_server::modules::package::build(&manifest, Some(&wasm(&id)), &[]).unwrap();
            std::fs::write(app.join("modules").join(format!("{id}.bzm")), package).unwrap();
        }
        let cache = std::env::temp_dir().join("breezone-test-modules-cache");
        std::fs::create_dir_all(&cache).unwrap();
        std::env::set_var("BREEZONE_MODULES_CACHE", &cache);
        app.join("web")
    })
    .clone()
}

/// The configuration of a test server with the shipped modules.
pub fn config(data_dir: &Path) -> breezone_server::config::Config {
    breezone_server::modules::wasm::allow_loopback(true);
    let mut cfg = breezone_server::config::Config::for_tests(data_dir.to_path_buf());
    cfg.web_dir = app_web();
    cfg
}
