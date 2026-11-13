//! Breezone server: a small self-hosted hub driving Airzone installations through their
//! local API, with users, routines, history, an activity feed, a REST + WebSocket API for
//! the mobile app, and the web UI served as static files.

pub mod access;
pub mod airzone;
pub mod airzone_schedules;
pub mod api;
pub mod auth;
pub mod cloud;
pub mod config;
pub mod db;
pub mod discovery;
pub mod error;
pub mod federation;
pub mod hub;
pub mod i18n;
pub mod mock;
pub mod routines;
pub mod stats;
pub mod timers;
pub mod update;
pub mod tls;

use crate::auth::AuthService;
use crate::config::Config;
use crate::db::Db;
use crate::hub::Hub;
use axum::response::Html;
use axum::Router;
use std::sync::Arc;
use axum::http::{header, HeaderValue};
use tower_http::cors::{Any, CorsLayer};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub cfg: Arc<Config>,
    pub hub: Arc<Hub>,
    pub auth: Arc<AuthService>,
    /// Key encrypting the remembered Airzone Cloud credentials.
    pub cloud_key: Arc<[u8; 32]>,
    /// Checks for and installs new releases (issue #40).
    pub updater: Arc<update::Updater>,
}

/// Opens the database, applies migrations, bootstraps accounts/installations from the
/// environment and loads the hub.
pub async fn init_state(cfg: Config) -> anyhow::Result<AppState> {
    let cfg = Arc::new(cfg);
    migrate_legacy_database(&cfg.data_dir);
    let db = db::connect(&cfg.data_dir.join("breezone.db")).await?;
    let secret = AuthService::load_secret(cfg.jwt_secret.as_deref(), &cfg.data_dir)?;
    let auth = Arc::new(AuthService::new(&secret, cfg.access_token_ttl_secs, cfg.refresh_token_ttl_days));
    let cloud_key = Arc::new(cloud::derive_key(&secret));
    let hub = Hub::new(db.clone(), cfg.clone(), cloud_key.clone());
    let updater = Arc::new(update::Updater::new(cfg.data_dir.clone(), cfg.self_update));
    updater.set_beta(db::get_setting(&db, update::CHANNEL_SETTING).await?.as_deref() == Some("beta"));
    let state = AppState { db, cfg, hub, auth, cloud_key, updater };
    bootstrap(&state).await?;
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
            _ => tracing::warn!(
                "no account yet: open the web UI (port {}) to create the administrator",
                state.cfg.port
            ),
        }
    }
    if db::list_installations(&state.db).await?.is_empty() {
        for b in &state.cfg.airzone_hosts {
            db::insert_installation(&state.db, &b.name, &b.host, b.port as i64, true).await?;
            tracing::info!("installation « {} » ({}:{}) added from BREEZONE_AIRZONE_HOSTS", b.name, b.host, b.port);
        }
    }
    Ok(())
}

/// The full HTTP application: API + web UI.
pub fn app(state: AppState) -> Router {
    let web_dir = state.cfg.web_dir.clone();
    let index = web_dir.join("index.html");
    let router = api::router();
    let router = if index.exists() {
        // Single page app: unknown paths are routed client-side.
        router.fallback_service(ServeDir::new(&web_dir).fallback(ServeFile::new(index)))
    } else {
        router.fallback(|| async {
            Html("<h1>Breezone</h1><p>The server is running, but the web UI is not installed (BREEZONE_WEB_DIR).</p>")
        })
    };
    with_security_headers(router)
        .layer(axum::middleware::from_fn(i18n::middleware))
        .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
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
    tokio::spawn(cloud::run_auto_sync(state.clone()));
    tokio::spawn(airzone_schedules::run_sync(state.clone()));
    tokio::spawn(timers::run_loop(state.hub.clone(), state.db.clone()));
    tokio::spawn(state.updater.clone().run_checks());
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
    let server = axum::serve(listener, app(state.clone()).into_make_service_with_connect_info::<std::net::SocketAddr>())
        .with_graceful_shutdown(async move {
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
            hub.log("system", "info", None, None, crate::text!("Breezone mis à jour en {0}", "Breezone updated to {0}", "Breezone actualizado a {0}", own), None)
                .await;
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
