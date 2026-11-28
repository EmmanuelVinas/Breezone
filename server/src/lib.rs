//! Breezone server: a small self-hosted hub driving Airzone installations through their
//! local API, with users, routines, history, an activity feed, a REST + WebSocket API for
//! the mobile app, and the web UI served as static files.

// The Matter bridge chains a handler per cluster of each light slot: the types are deep.
#![recursion_limit = "512"]

pub mod access;
pub mod accounts;
pub mod airzone;
pub mod airzone_schedules;
pub mod api;
pub mod auth;
pub mod automations;
pub mod backfill;
pub mod backup;
pub mod capabilities;
pub mod charging;
pub mod cloud;
pub mod config;
pub mod cre;
pub mod crypto;
pub mod dashboard;
pub mod db;
pub mod devices;
pub mod discovery;
pub mod drivers;
pub mod energy;
pub mod error;
#[doc(hidden)]
pub mod esphome_wire;
pub mod federation;
pub mod hub;
pub mod i18n;
pub mod incidents;
pub mod matter;
pub mod mdns;
pub mod mock;
pub mod mock_devices;
pub mod modules;
pub mod price;
pub mod rollup;
pub mod routines;
pub mod services;
pub mod stats;
pub mod surplus;
pub mod tempo;
pub mod timers;
pub mod tls;
pub mod traits;
pub mod update;
pub mod weather;

use crate::auth::AuthService;
use crate::config::Config;
use crate::db::Db;
use crate::hub::Hub;
use axum::http::{header, HeaderValue, StatusCode, Uri};
use axum::response::{Html, IntoResponse, Response};
use axum::Router;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub cfg: Arc<Config>,
    pub hub: Arc<Hub>,
    pub auth: Arc<AuthService>,
    /// Key encrypting the secrets kept in the database (see `crypto`).
    pub data_key: Arc<[u8; 32]>,
    /// Checks for and installs new releases (issue #40).
    pub updater: Arc<update::Updater>,
    /// Sources of devices read by drivers (ADR 0013).
    pub drivers: Arc<drivers::Runtime>,
    /// The sun and the weather of the home (roadmap F9).
    pub weather: Arc<weather::Weather>,
    /// The automations under way (they may wait).
    pub automations: Arc<automations::Runs>,
    /// The solar surplus steering (roadmap E6).
    pub surplus: Arc<surplus::Surplus>,
    /// The library of modules (docs/roadmap/modules.md).
    pub library: Arc<modules::library::Library>,
}

/// Opens the database, applies migrations, bootstraps accounts/installations from the
/// environment and loads the hub.
pub async fn init_state(cfg: Config) -> anyhow::Result<AppState> {
    let cfg = Arc::new(cfg);
    modules::set_timezone(cfg.timezone.name());
    migrate_legacy_database(&cfg.data_dir);
    let db = db::connect(&cfg.data_dir.join("breezone.db")).await?;
    // The drivers once compiled into the server are modules now: their rows go.
    db::forget_native_modules(&db).await?;
    modules::install::load_installed(&db, &cfg.data_dir).await?;
    // The modules that come with Breezone, beside its web UI (`/app/modules` in the image): they
    // drive the hardware, whatever library is read.
    let builtin = cfg.web_dir.parent().unwrap_or(std::path::Path::new(".")).join("modules");
    modules::install::install_builtin(&db, &cfg.data_dir, &builtin, update::RELEASE_PUBLIC_KEY).await;
    if let Some(dev) = &cfg.modules_dev {
        modules::install::install_dev(&db, &cfg.data_dir, dev).await;
        // In development, the fake devices answer on this machine (`dev.sh --modules`):
        // the modules may reach it. Never in a release build.
        #[cfg(feature = "wasm")]
        if cfg!(debug_assertions) && std::env::var("BREEZONE_MODULES_LOOPBACK").is_ok_and(|v| v == "1") {
            tracing::warn!("development: the modules may reach this machine (BREEZONE_MODULES_LOOPBACK)");
            modules::wasm::allow_loopback(true);
        }
    }
    let secret = AuthService::load_secret(cfg.jwt_secret.as_deref(), &cfg.data_dir)?;
    let auth = Arc::new(AuthService::new(&secret, cfg.access_token_ttl_secs, cfg.refresh_token_ttl_days));
    let data_key = Arc::new(crypto::load_key(&cfg.data_dir, &db, &secret).await?);
    let hub = Hub::new(db.clone(), cfg.clone(), data_key.clone());
    let updater = Arc::new(update::Updater::new(cfg.data_dir.clone(), cfg.self_update));
    updater.set_beta(db::get_setting(&db, update::CHANNEL_SETTING).await?.as_deref() == Some("beta"));
    #[cfg(feature = "wasm")]
    modules::wasm::set_context(db.clone(), *data_key);
    let drivers = Arc::new(drivers::Runtime::new(db.clone(), *data_key));
    drivers.reload().await?;
    drivers.clone().hear();
    let weather = Arc::new(weather::Weather::new(db.clone(), cfg.timezone, cfg.weather));
    let automations = Arc::new(automations::Runs::default());
    let surplus = Arc::new(surplus::Surplus::default());
    let library = Arc::new(modules::library::Library::new(&cfg));
    let state = AppState { db, cfg, hub, auth, data_key, updater, drivers, weather, automations, surplus, library };
    bootstrap(&state).await?;
    // An account whose module brings its devices (Netatmo, EZVIZ) has its source.
    for m in modules::manifests().filter(|m| accounts::comes_with_account(&m.id)) {
        if let Err(e) = accounts::ensure_module_source(&state, &m.id).await {
            tracing::warn!("{} module not added: {}", m.id, e.message.en);
        }
    }
    state.hub.reload_installations().await?;
    Ok(state)
}

/// The project was called FreeZone before: its database file is renamed once.
fn migrate_legacy_database(dir: &std::path::Path) {
    let old = dir.join("freezone.db");
    if !old.exists() || dir.join("breezone.db").exists() {
        return;
    }
    for suffix in ["", "-wal", "-shm"] {
        let from = dir.join(format!("freezone.db{suffix}"));
        if from.exists() {
            if let Err(e) = std::fs::rename(&from, dir.join(format!("breezone.db{suffix}"))) {
                tracing::error!("cannot rename {}: {e}", from.display());
            }
        }
    }
    tracing::info!("database renamed from freezone.db to breezone.db");
}

async fn bootstrap(state: &AppState) -> anyhow::Result<()> {
    if db::count_users(&state.db).await? == 0 {
        match (&state.cfg.admin_user, &state.cfg.admin_password) {
            (Some(user), Some(password)) => {
                let hash = auth::hash_password(password.clone()).await?;
                db::insert_user(&state.db, user.trim(), &hash, "admin").await?;
                tracing::info!("administrator « {user} » created from BREEZONE_ADMIN_USER / BREEZONE_ADMIN_PASSWORD");
            }
            _ => tracing::warn!("no account yet: open the web UI (port {}) to create the administrator", state.cfg.port),
        }
    }
    if db::list_installations(&state.db).await?.is_empty() {
        for b in &state.cfg.airzone_hosts {
            db::insert_installation(&state.db, &b.name, &b.host, b.port as i64, true).await?;
            tracing::info!("installation « {} » ({}:{}) added from BREEZONE_AIRZONE_HOSTS", b.name, b.host, b.port);
        }
    }
    if db::ensure_home(&state.db).await?.is_some() {
        tracing::info!("home « {} » created: its devices come from the modules, Airzone among them", db::HOME_NAME);
    }
    Ok(())
}

/// `#[serde(default = "crate::yes")]`: a flag on unless said otherwise.
pub(crate) fn yes() -> bool {
    true
}

/// The full HTTP application: API + web UI.
pub fn app(state: AppState) -> Router {
    let web_dir = state.cfg.web_dir.clone();
    let base_path = state.cfg.base_path.clone();
    let index = web_dir.join("index.html");
    let router = api::router();
    let router = if index.exists() {
        // Single page app: unknown paths are routed client-side.
        let base = base_path.clone();
        let page = axum::routing::get(move |uri: Uri| spa_index(index.clone(), base.clone(), uri));
        router.fallback_service(ServeDir::new(&web_dir).append_index_html_on_directories(false).fallback(page))
    } else {
        router.fallback(|| async { Html("<h1>Breezone</h1><p>The server is running, but the web UI is not installed (BREEZONE_WEB_DIR).</p>") })
    };
    let router = with_security_headers(router)
        .layer(axum::middleware::from_fn(incidents::record))
        .layer(axum::middleware::from_fn(i18n::middleware))
        .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any))
        .layer(TraceLayer::new_for_http())
        .with_state(state);
    if base_path.is_empty() {
        return router;
    }
    // The proxy keeps the prefix in the requests: removed before the routes are matched.
    let stripped = tower::ServiceBuilder::new()
        .map_request(move |mut req: axum::extract::Request| {
            if let Some(uri) = strip_base_path(req.uri(), &base_path) {
                *req.uri_mut() = uri;
            }
            req
        })
        .service(router);
    Router::new().fallback_service(stripped)
}

fn strip_base_path(uri: &Uri, base_path: &str) -> Option<Uri> {
    let rest = uri.path().strip_prefix(base_path)?;
    let path = match rest {
        "" => "/",
        r if r.starts_with('/') => r,
        _ => return None,
    };
    let path_and_query = match uri.query() {
        Some(q) => format!("{path}?{q}"),
        None => path.to_owned(),
    };
    let mut parts = uri.clone().into_parts();
    parts.path_and_query = Some(path_and_query.parse().ok()?);
    Uri::from_parts(parts).ok()
}

/// The page of the web UI, with a `<base>` that leads back to the root of the app, so that it
/// works under a sub-path of a reverse proxy (`https://nas.local/breezone/`): its scripts, styles
/// and calls to the API are relative to it.
async fn spa_index(index: std::path::PathBuf, base_path: String, uri: Uri) -> Response {
    match tokio::fs::read_to_string(&index).await {
        Ok(html) => {
            let mut res = Html(with_base_href(&html, &base_href(&base_path, uri.path()))).into_response();
            // Always the page of the installed version, right after an update.
            res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
            res
        }
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Absolute when the prefix is known, else relative to the path the server sees: a proxy that
/// removes the prefix leaves `/zones/3` for `/breezone/zones/3`, and `../` leads back to
/// `/breezone/` as well as to `/` without a proxy.
fn base_href(base_path: &str, path: &str) -> String {
    if !base_path.is_empty() {
        return format!("{base_path}/");
    }
    match path.matches('/').count().saturating_sub(1) {
        0 => "./".to_owned(),
        depth => "../".repeat(depth),
    }
}

fn with_base_href(html: &str, href: &str) -> String {
    let tag = format!("<base href=\"{href}\" />");
    match html.find("<head>") {
        Some(i) => format!("{}{tag}{}", &html[..i + "<head>".len()], &html[i + "<head>".len()..]),
        None => format!("{tag}{html}"),
    }
}

/// Browser protections for the web UI: only its own scripts, never shown inside another site's
/// frame (clickjacking), no MIME sniffing, no referrer sent. HSTS is left to the HTTPS proxy: on a
/// local address with a self-signed certificate it would make the warning impossible to bypass.
fn with_security_headers(router: Router<AppState>) -> Router<AppState> {
    const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; \
font-src 'self' data:; connect-src 'self' ws: wss:; object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'none'";
    router
        .layer(SetResponseHeaderLayer::if_not_present(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(CSP)))
        .layer(SetResponseHeaderLayer::if_not_present(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY")))
        .layer(SetResponseHeaderLayer::if_not_present(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")))
        .layer(SetResponseHeaderLayer::if_not_present(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer")))
}

/// Starts the poller, the live links to other nodes, the routine scheduler and the maintenance task.
pub fn spawn_background(state: &AppState) {
    tokio::spawn(state.hub.clone().run_poller());
    tokio::spawn(state.hub.clone().run_node_links());
    tokio::spawn(state.hub.clone().run_maintenance());
    tokio::spawn(state.drivers.clone().run(
        std::time::Duration::from_millis(state.cfg.device_read_ms),
        std::time::Duration::from_secs(state.cfg.history_interval_secs.max(1) as u64),
        state.cfg.scan_subnet.clone(),
        state.cfg.scan_port,
    ));
    tokio::spawn(cloud::run_auto_sync(state.clone()));
    tokio::spawn(airzone_schedules::run_sync(state.clone()));
    tokio::spawn(timers::run_loop(state.clone()));
    tokio::spawn(backfill::run_loop(state.clone()));
    tokio::spawn(automations::run_loop(state.clone()));
    {
        let state = state.clone();
        tokio::spawn(async move {
            // Before the surplus looks at them.
            if let Err(e) = charging::adopt_surplus_chargers(&state).await {
                tracing::error!("chargers of the surplus not moved to a charging profile: {e:#}");
            }
            if let Err(e) = surplus::adopt_list(&state).await {
                tracing::error!("list of the surplus not made into automations: {e:#}");
            }
            surplus::run_loop(state).await
        });
    }
    tokio::spawn(state.updater.clone().run_checks());
    tokio::spawn(state.weather.clone().run());
    tokio::spawn(tempo::run_loop(state.clone()));
    tokio::spawn(modules::conditions::run_loop(state.clone()));
    tokio::spawn(accounts::linky::run_loop(state.clone()));
    {
        let state = state.clone();
        tokio::spawn(async move { modules::install::replace_incompatible(&state.db, &state.cfg.data_dir, &state.library).await });
    }
    tokio::spawn(matter::spawn(state.clone()));
    tokio::spawn(cre::run_loop(state.clone()));
}

pub async fn run(cfg: Config) -> anyhow::Result<()> {
    let own = update::current_version();
    // A newer version installed from the web UI runs instead of this one (issue #40).
    if let update::Launch::Switch { version, binary, web } = update::choose_at_start(&cfg.data_dir, own) {
        tracing::info!("starting Breezone {version}, installed from the web UI");
        let e = update::exec(&binary, &web);
        tracing::error!("cannot start Breezone {version}: {e}; Breezone {own} runs");
        update::abandon(&cfg.data_dir, &version);
    }
    let addr = format!("{}:{}", cfg.bind, cfg.port);
    let data_dir = cfg.data_dir.clone();
    let state = init_state(cfg).await?;
    spawn_background(&state);
    report_update(&state, &data_dir, own);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Breezone {own} listening on http://{addr}");
    let (stop, mut stopped) = tokio::sync::watch::channel(false);
    let server = axum::serve(listener, app(state.clone()).into_make_service_with_connect_info::<std::net::SocketAddr>()).with_graceful_shutdown(async move {
        let _ = stopped.changed().await;
    });
    let server = tokio::spawn(async move { server.await });
    let restart = tokio::select! {
        _ = shutdown_signal() => false,
        _ = state.updater.restart_requested() => true,
    };
    let _ = stop.send(true);
    // Open WebSockets would keep the server up: they get a few seconds, then are dropped.
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), server).await;
    // The connections kept with the hardware are ended cleanly: a HomeKit gateway has few, and
    // one left to time out on its side is one less for the server that starts next.
    state.drivers.close_all().await;
    if !restart {
        return Ok(());
    }
    let Some((binary, web)) = state.updater.switch_target() else { return Ok(()) };
    state.db.close().await;
    tracing::info!("restarting on {}", binary.display());
    let e = update::exec(&binary, &web);
    if let Some(version) = binary.parent().and_then(|d| d.file_name()).map(|n| n.to_string_lossy().into_owned()) {
        update::abandon(&data_dir, &version);
    }
    Err(anyhow::anyhow!("cannot start the new version: {e}"))
}

/// Reports an update undone at start, and keeps a new version once it has run long enough.
fn report_update(state: &AppState, data_dir: &std::path::Path, own: update::Version) {
    if let Some(r) = update::take_rolled_back(data_dir) {
        let hub = state.hub.clone();
        let text = crate::text!(
            "Breezone {0} n'a pas démarré : retour à Breezone {1}, avec la base d'avant la mise à jour",
            "Breezone {0} did not start: back to Breezone {1}, with the database from before the update",
            "Breezone {0} no arrancó: vuelta a Breezone {1}, con la base de datos de antes de la actualización",
            r.version,
            r.to
        );
        tokio::spawn(async move { hub.log("system", "error", None, None, text, None).await });
        state.updater.set_rolled_back(Some(r));
    }
    let hub = state.hub.clone();
    let data_dir = data_dir.to_path_buf();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(update::CONFIRM_AFTER_SECS)).await;
        if update::confirm(&data_dir, own) {
            hub.log("system", "info", None, None, crate::text!("Breezone mis à jour en {0}", "Breezone updated to {0}", "Breezone actualizado a {0}", own), None).await;
        }
    });
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutting down");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_href_leads_back_to_the_root_of_the_app() {
        assert_eq!(base_href("", "/"), "./");
        assert_eq!(base_href("", "/settings"), "./");
        assert_eq!(base_href("", "/zones/3"), "../");
        assert_eq!(base_href("", "/a/b/c"), "../../");
        assert_eq!(base_href("/breezone", "/zones/3"), "/breezone/");
    }

    #[test]
    fn base_path_is_removed_from_the_requests() {
        let strip = |u: &str| strip_base_path(&u.parse().unwrap(), "/breezone").map(|u| u.to_string());
        assert_eq!(strip("/breezone").as_deref(), Some("/"));
        assert_eq!(strip("/breezone/api/devices?x=1").as_deref(), Some("/api/devices?x=1"));
        assert_eq!(strip("/breezonex/api"), None);
        assert_eq!(strip("/api/devices"), None);
    }

    #[test]
    fn base_is_the_first_element_of_the_head() {
        let html = with_base_href("<html><head><meta charset=\"UTF-8\" /></head></html>", "../");
        assert_eq!(html, "<html><head><base href=\"../\" /><meta charset=\"UTF-8\" /></head></html>");
    }
}
