//! Scans a real network (not run by default), with the modules shipped with Breezone:
//! `BREEZONE_SCAN_SUBNET=192.168.1.0/24 cargo test --test scan_network -- --ignored --nocapture`

mod support;

/// A server with the shipped modules installed (the scan asks them).
async fn server(name: &str) -> breezone_server::AppState {
    let dir = std::env::temp_dir().join(format!("breezone-real-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    breezone_server::init_state(support::config(&dir)).await.unwrap()
}

#[tokio::test]
#[ignore]
async fn scan_a_real_network() {
    let subnet = std::env::var("BREEZONE_SCAN_SUBNET").expect("BREEZONE_SCAN_SUBNET");
    let _state = server("scan").await;
    let started = std::time::Instant::now();
    let products = breezone_server::discovery::scan_products(&subnet, 3000, &[], &[]).await.unwrap();
    println!("{} product(s) in {:?}", products.len(), started.elapsed());
    for p in &products {
        println!("  {} {:<16} {} {} {}", p.brand, p.host, p.model, p.note.as_deref().unwrap_or(""), if p.supported { "" } else { "(non pris en charge)" });
    }
}

#[tokio::test]
#[ignore]
async fn probe_one_host() {
    let host = std::env::var("BREEZONE_PROBE_HOST").expect("BREEZONE_PROBE_HOST");
    let _state = server("probe").await;
    let open = tokio::net::TcpStream::connect(format!("{host}:80")).await;
    println!("connect :80 -> {:?}", open.map(|_| ()));
    println!("modules -> {:?}", breezone_server::modules::identify_at(&host, &[80]).await);
    println!("airzone -> {:?}", breezone_server::discovery::probe(&host, 3000).await);
}

/// Reads a real Shelly through a source (read only):
/// `BREEZONE_SHELLY_HOST=192.168.1.40 cargo test --test scan_network real_shelly -- --ignored --nocapture`
#[tokio::test]
#[ignore]
async fn real_shelly() {
    let host = std::env::var("BREEZONE_SHELLY_HOST").expect("BREEZONE_SHELLY_HOST");
    let state = server("shelly").await;
    let found = breezone_server::modules::identify_at(&host, &[80]).await.into_iter().find(|p| p.driver == "shelly").expect("a Shelly");
    let config = serde_json::json!({"host": host, "device_id": found.device_id}).to_string();
    let id = breezone_server::db::insert_source(&state.db, "shelly", "Tableau", &config).await.unwrap();
    state.drivers.reload().await.unwrap();
    state.drivers.poll(id).await;
    println!("status: {:?}", state.drivers.status(id).await);
    for d in state.drivers.devices().await {
        println!("  {} [{}] {:?}", d.info.name, d.info.traits.join(", "), d.state);
    }
}

/// The HomeKit accessories announced on this network (`cargo test --test scan_network -- --ignored homekit`).
#[tokio::test]
#[ignore = "real network"]
async fn homekit_accessories_are_announced() {
    let _state = server("homekit").await;
    let announced = breezone_server::mdns::browse(&["_hap._tcp"], std::time::Duration::from_secs(2), &[]).await;
    let found = breezone_server::modules::identify_announced(&announced).await;
    for p in &found {
        println!("{p:?}");
    }
    assert!(!found.is_empty(), "no HomeKit accessory announced");
}
