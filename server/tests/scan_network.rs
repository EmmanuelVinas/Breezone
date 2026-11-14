//! Scans a real network (not run by default):
//! `BREEZONE_SCAN_SUBNET=192.168.1.0/24 cargo test --test scan_network -- --ignored --nocapture`

#[tokio::test]
#[ignore]
async fn scan_a_real_network() {
    let subnet = std::env::var("BREEZONE_SCAN_SUBNET").expect("BREEZONE_SCAN_SUBNET");
    let started = std::time::Instant::now();
    let products = breezone_server::discovery::scan_products(&subnet, 3000).await.unwrap();
    println!("{} product(s) in {:?}", products.len(), started.elapsed());
    for p in &products {
        println!("  {} {:<16} {} {} {}", p.brand, p.host, p.model, p.note.as_deref().unwrap_or(""), if p.supported { "" } else { "(non pris en charge)" });
    }
}

#[tokio::test]
#[ignore]
async fn probe_one_host() {
    let host = std::env::var("BREEZONE_PROBE_HOST").expect("BREEZONE_PROBE_HOST");
    let open = tokio::net::TcpStream::connect(format!("{host}:80")).await;
    println!("connect :80 -> {:?}", open.map(|_| ()));
    println!("shelly -> {:?}", breezone_server::drivers::shelly::probe(&host).await);
    println!("airzone -> {:?}", breezone_server::discovery::probe(&host, 3000).await);
}

/// Reads a real Shelly through a source (read only):
/// `BREEZONE_SHELLY_HOST=192.168.1.40 cargo test --test scan_network real_shelly -- --ignored --nocapture`
#[tokio::test]
#[ignore]
async fn real_shelly() {
    let host = std::env::var("BREEZONE_SHELLY_HOST").expect("BREEZONE_SHELLY_HOST");
    let dir = std::env::temp_dir().join(format!("breezone-real-shelly-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let state = breezone_server::init_state(breezone_server::config::Config::for_tests(dir)).await.unwrap();
    let found = breezone_server::drivers::shelly::probe(&host).await.expect("a Shelly");
    let config = serde_json::json!({"host": host, "device_id": found.id}).to_string();
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
    let found = breezone_server::homekit::discovery::browse(std::time::Duration::from_secs(2), &[]).await;
    for a in &found {
        println!("{a:?}");
    }
    assert!(!found.is_empty(), "no HomeKit accessory announced");
}

/// The bare mDNS question with std sockets, to see what this machine lets through.
#[test]
#[ignore = "real network"]
fn bare_mdns_question() {
    let q: Vec<u8> = [&[0u8, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0][..], b"\x04_hap\x04_tcp\x05local\x00", &[0, 12, 0, 1]].concat();
    let ip = breezone_server::discovery::local_ipv4().unwrap();
    let s = std::net::UdpSocket::bind((ip, 0)).unwrap();
    println!("bound {:?}", s.local_addr());
    println!("sent {:?}", s.send_to(&q, "224.0.0.251:5353"));
    println!("sent unicast {:?}", s.send_to(&q, "192.168.86.25:5353"));
    s.set_read_timeout(Some(std::time::Duration::from_millis(500))).unwrap();
    let mut buf = [0u8; 9000];
    for _ in 0..4 {
        println!("recv {:?}", s.recv_from(&mut buf).map(|(n, a)| (n, a)));
    }
}
