//! HTTP API. See `docs/API.md` for the reference.

mod account;
mod cloud;
mod federation;
mod hvac;
mod routines;
mod schedules;
mod timers;
mod ws;

use crate::AppState;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post, put};
use axum::{Json, Router};
use serde_json::json;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/api/version", get(version))
        // First start & authentication
        .route("/api/setup", get(account::setup_status).post(account::setup))
        .route("/api/auth/login", post(account::login))
        .route("/api/auth/refresh", post(account::refresh))
        .route("/api/auth/logout", post(account::logout))
        // Current user & devices
        .route("/api/me", get(account::me))
        .route("/api/me/password", put(account::change_password))
        .route("/api/sessions", get(account::list_sessions))
        .route("/api/sessions/{id}", axum::routing::delete(account::delete_session))
        // Users (admin)
        .route("/api/users", get(account::list_users).post(account::create_user))
        .route("/api/users/{id}", put(account::update_user).delete(account::delete_user))
        // Installations & zones
        .route("/api/status", get(hvac::status))
        .route("/api/installations", get(hvac::list_installations).post(hvac::create_installation))
        .route("/api/installations/test", post(hvac::test_installation))
        .route(
            "/api/installations/{id}",
            get(hvac::get_installation).put(hvac::update_installation).delete(hvac::delete_installation),
        )
        .route("/api/installations/{id}/refresh", post(hvac::refresh_installation))
        .route("/api/installations/{id}/cloud", put(hvac::link_cloud))
        .route("/api/installations/{id}/zones/{system_id}/{zone_id}", patch(hvac::control_zone))
        .route("/api/installations/{id}/zones/{system_id}/{zone_id}/meta", put(hvac::update_zone_meta))
        .route("/api/installations/{id}/zones/{system_id}/{zone_id}/history", get(hvac::zone_history))
        .route("/api/control/batch", post(hvac::batch))
        .route("/api/discovery/scan", post(hvac::scan))
        .route("/api/discovery/settings", get(hvac::discovery_settings).put(hvac::save_discovery_settings))
        .route("/api/events", get(hvac::events))
        // Airzone Cloud synchronisation (optional)
        .route("/api/cloud", get(cloud::status).put(cloud::connect).delete(cloud::disconnect))
        .route("/api/cloud/sync", post(cloud::sync))
        .route("/api/cloud/installations/{cloud_id}/access", put(cloud::set_access))
        // Federation between Breezone nodes (ADR 0008)
        .route("/api/node", get(federation::get_identity).put(federation::save_identity))
        .route("/api/shares", get(federation::list_shares).post(federation::create_share))
        .route("/api/shares/{id}", put(federation::update_share).delete(federation::delete_share))
        .route("/api/shares/{id}/approve", post(federation::approve_share))
        .route("/api/nodes", get(federation::list_nodes).post(federation::add_node))
        .route("/api/nodes/{id}", put(federation::update_node).delete(federation::delete_node))
        .route("/api/nodes/{id}/refresh", post(federation::refresh_node))
        .route("/api/nodes/{id}/installations", put(federation::set_node_installations))
        .route(
            "/api/nodes/{id}/certificate",
            get(federation::node_certificate).put(federation::trust_node_certificate).delete(federation::untrust_node_certificate),
        )
        .route("/api/federation/pair", post(federation::pair))
        .route("/api/federation/installations", get(federation::installations))
        .route("/api/federation/installations/{id}/zones/{system_id}/{zone_id}", patch(federation::control))
        // Routines
        .route("/api/routines", get(routines::list).post(routines::create))
        .route("/api/routines/{id}", get(routines::get).put(routines::update).delete(routines::delete))
        .route("/api/routines/{id}/run", post(routines::run))
        // Timers and Airzone routines
        .route("/api/timers", get(timers::list).post(timers::create))
        .route("/api/timers/{id}", axum::routing::delete(timers::cancel))
        .route("/api/airzone/routines", get(timers::airzone_routines))
        // Schedules of the Airzone app (Airzone Cloud)
        .route("/api/airzone/schedules", get(schedules::list).post(schedules::create))
        .route("/api/airzone/schedules/sync", post(schedules::sync))
        .route("/api/airzone/schedules/{cloud_id}/{id}", put(schedules::update).delete(schedules::delete))
        .route("/api/airzone/schedules/{cloud_id}/{id}/enabled", put(schedules::set_enabled))
        // Live updates
        .route("/api/ws", get(ws::handler))
        // Unknown API routes must not fall back to the web UI.
        .route("/api/{*rest}", axum::routing::any(api_not_found))
        .layer(middleware::from_fn(client_api_guard))
}

/// Version of the HTTP API, raised on breaking changes only: new routes and new fields keep it
/// (clients ignore unknown fields). Policy in `docs/API.md`, "Versions de l'API".
pub const API_VERSION: u32 = 1;
/// Oldest client API version still served. Raise it only when the older API is really dropped.
pub const MIN_CLIENT_API: u32 = 1;
/// Header carrying the API version of a client (the mobile app). Clients that do not send it are served.
pub const CLIENT_API_HEADER: &str = "breezone-api";

/// Refuses API calls from clients older than [`MIN_CLIENT_API`] with 426, so the app can ask for an update.
async fn client_api_guard(request: Request, next: Next) -> Response {
    let path = request.uri().path();
    let too_old = path.starts_with("/api/")
        && path != "/api/version"
        && request
            .headers()
            .get(CLIENT_API_HEADER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u32>().ok())
            .is_some_and(|v| v < MIN_CLIENT_API);
    if too_old {
        return (
            StatusCode::UPGRADE_REQUIRED,
            Json(json!({
                "error": "this app is too old for this server: update it",
                "api_version": API_VERSION,
                "min_client_api": MIN_CLIENT_API,
            })),
        )
            .into_response();
    }
    next.run(request).await
}

fn versions() -> serde_json::Value {
    json!({ "version": env!("CARGO_PKG_VERSION"), "api_version": API_VERSION, "min_client_api": MIN_CLIENT_API })
}

async fn health() -> impl IntoResponse {
    let mut body = versions();
    body["status"] = json!("ok");
    Json(body)
}

async fn version() -> impl IntoResponse {
    let mut body = versions();
    body["name"] = json!("Breezone");
    Json(body)
}

async fn api_not_found() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, Json(json!({ "error": "not found" })))
}
