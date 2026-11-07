//! Fake Airzone WebServer for development and demos.
//!
//!   MOCK_PORT=3000 MOCK_PRESET=maison cargo run --bin mock-airzone
//!   MOCK_PORT=3001 MOCK_PRESET=bureau cargo run --bin mock-airzone
//!
//! With MOCK_CLOUD_PORT=3900 it also serves a fake Airzone Cloud account
//! (login demo@breezone.local / demo-password) for the synchronisation feature:
//! start Breezone with BREEZONE_AIRZONE_CLOUD_URL=http://127.0.0.1:3900.

use breezone_server::mock;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();
    let port: u16 = std::env::var("MOCK_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(3000);
    let preset = std::env::var("MOCK_PRESET").unwrap_or_else(|_| "maison".into());
    let state = Arc::new(Mutex::new(mock::preset(&preset)));
    mock::spawn_simulation(state.clone(), Duration::from_secs(5));
    if let Some(cloud_port) = std::env::var("MOCK_CLOUD_PORT").ok().and_then(|p| p.parse::<u16>().ok()) {
        let cloud = mock::cloud_router(mock::cloud_account(mock::MOCK_MAC));
        let cloud_listener = tokio::net::TcpListener::bind(("0.0.0.0", cloud_port)).await?;
        tracing::info!("mock Airzone Cloud on http://0.0.0.0:{cloud_port} ({} / {})", mock::MOCK_CLOUD_EMAIL, mock::MOCK_CLOUD_PASSWORD);
        tokio::spawn(async move { axum::serve(cloud_listener, cloud).await });
    }
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    tracing::info!("mock Airzone WebServer « {preset} » on http://0.0.0.0:{port}/api/v1/hvac");
    axum::serve(listener, mock::router_with_mac(state, mock::preset_mac(&preset))).await?;
    Ok(())
}
