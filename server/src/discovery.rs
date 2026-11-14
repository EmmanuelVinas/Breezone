//! LAN discovery: scans a subnet for the products Breezone supports. Each address is asked, in
//! parallel, whether it is an Airzone WebServer (local API, port 3000), a Shelly (`/shelly`, port
//! 80) or a V2C Trydan (`/RealTimeData`, port 80), and products are identified down to the model.

use crate::airzone::AirzoneClient;
use serde::Serialize;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::Semaphore;

pub const AIRZONE_PORT: u16 = 3000;

#[derive(Debug, Clone, Serialize)]
pub struct Found {
    pub host: String,
    pub port: u16,
    pub mac: Option<String>,
    pub ws_type: Option<String>,
    pub firmware: Option<String>,
    pub zones: Option<usize>,
}

/// Local IPv4 address used to reach the outside world (no packet is actually sent).
pub fn local_ipv4() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    match socket.local_addr().ok()? {
        SocketAddr::V4(a) => Some(*a.ip()),
        _ => None,
    }
}

/// Parses "192.168.1.0/24" (prefix between 20 and 30) into the list of host addresses.
pub fn hosts_in(subnet: &str) -> Result<Vec<Ipv4Addr>, String> {
    let (ip, prefix) = subnet.trim().split_once('/').unwrap_or((subnet.trim(), "24"));
    let ip: Ipv4Addr = ip.parse().map_err(|_| format!("invalid subnet address: {ip}"))?;
    let prefix: u32 = prefix.parse().map_err(|_| "invalid prefix".to_string())?;
    if !(20..=30).contains(&prefix) {
        return Err("prefix must be between /20 and /30".into());
    }
    let mask = u32::MAX << (32 - prefix);
    let network = u32::from(ip) & mask;
    let broadcast = network | !mask;
    Ok(((network + 1)..broadcast).map(Ipv4Addr::from).collect())
}

pub fn default_subnet() -> Option<String> {
    local_ipv4().map(|ip| {
        let o = ip.octets();
        format!("{}.{}.{}.0/24", o[0], o[1], o[2])
    })
}

/// Scans `subnet` (default: the server's /24) on `port` and returns the Airzone WebServers found.
pub async fn scan(subnet: Option<&str>, port: u16) -> Result<Vec<Found>, String> {
    let subnet = match subnet.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => s.to_string(),
        None => default_subnet().ok_or("cannot determine the local network, please provide a subnet")?,
    };
    let hosts = hosts_in(&subnet)?;
    let limit = Arc::new(Semaphore::new(SCAN_PARALLEL));
    let mut tasks = Vec::with_capacity(hosts.len());
    for host in hosts {
        let limit = limit.clone();
        tasks.push(tokio::spawn(async move {
            let _permit = limit.acquire().await.ok()?;
            probe(&host.to_string(), port).await
        }));
    }
    let mut found = vec![];
    for t in tasks {
        if let Ok(Some(f)) = t.await {
            found.push(f);
        }
    }
    found.sort_by_key(|f| f.host.parse::<Ipv4Addr>().map(u32::from).unwrap_or(0));
    Ok(found)
}

/// Addresses asked at the same time.
const SCAN_PARALLEL: usize = 128;
/// A port that does not accept a connection within this delay is closed. Generous on purpose:
/// asking many addresses at once floods the address resolution (ARP) of the network, and a real
/// device then takes several hundred milliseconds to answer (measured: a Shelly missed at 800 ms,
/// found at 1.5 s; the former 400 ms missed devices).
const CONNECT_WITHIN: Duration = Duration::from_millis(1500);

async fn is_open(host: &str, port: u16) -> bool {
    matches!(tokio::time::timeout(CONNECT_WITHIN, TcpStream::connect(format!("{host}:{port}"))).await, Ok(Ok(_)))
}

/// Checks whether `host:port` is an Airzone WebServer. Any HTTP service may listen on port
/// 3000: only answers carrying the Airzone signature (`mac` / `ws_type`, or HVAC zones) count.
/// The WebServer's own description answers fast; the zones are only counted, briefly.
pub async fn probe(host: &str, port: u16) -> Option<Found> {
    if !is_open(host, port).await {
        return None;
    }
    let client = AirzoneClient::new(host, port);
    let info = tokio::time::timeout(Duration::from_secs(2), client.webserver_info())
        .await
        .ok()
        .and_then(|r| r.ok())
        .filter(is_airzone_webserver);
    // Older WebServers have no description: only their zones tell.
    let zones = tokio::time::timeout(Duration::from_millis(if info.is_some() { 1500 } else { 2500 }), client.read_all())
        .await
        .ok()
        .and_then(|r| r.ok())
        .map(|r| r.zones().len())
        .filter(|n| *n > 0);
    if info.is_none() && zones.is_none() {
        return None;
    }
    let field = |k: &str| info.as_ref().and_then(|i| i.get(k)).and_then(|v| v.as_str()).map(String::from);
    Some(Found {
        host: host.to_string(),
        port,
        mac: field("mac"),
        ws_type: field("ws_type"),
        firmware: field("ws_firmware"),
        zones,
    })
}

/// A product found on the network, of a supported brand.
#[derive(Debug, Clone, Serialize)]
pub struct Product {
    /// "airzone" | "shelly" | "v2c"
    pub brand: &'static str,
    /// What adds it: "airzone" (an installation) or the driver of a source.
    pub driver: &'static str,
    /// Name of the model ("Shelly Pro 3EM", "Airzone WebServer").
    pub model: String,
    pub host: String,
    pub port: u16,
    pub mac: Option<String>,
    /// Stable id of the device (Shelly), to recognise it if its address changes.
    pub device_id: Option<String>,
    pub firmware: Option<String>,
    /// Profile, number of zones, why it is not supported…
    pub note: Option<String>,
    /// Breezone can drive it.
    pub supported: bool,
}

/// Every supported product on `subnet`, each address asked for every brand at once.
pub async fn scan_products(subnet: &str, airzone_port: u16) -> Result<Vec<Product>, String> {
    let hosts = hosts_in(subnet)?;
    let limit = Arc::new(Semaphore::new(SCAN_PARALLEL));
    let mut tasks = Vec::with_capacity(hosts.len());
    for host in hosts {
        let limit = limit.clone();
        tasks.push(tokio::spawn(async move {
            let _permit = limit.acquire().await.ok()?;
            let host = host.to_string();
            let (airzone, (shelly, trydan)) = tokio::join!(probe(&host, airzone_port), async {
                if is_open(&host, 80).await {
                    tokio::join!(crate::drivers::shelly::probe(&host), crate::drivers::trydan::probe(&host))
                } else {
                    (None, None)
                }
            });
            let mut found = vec![];
            if let Some(a) = airzone {
                let note = match (&a.ws_type, a.zones) {
                    (Some(t), Some(z)) => Some(format!("{t} · {z} zones")),
                    (Some(t), None) => Some(t.clone()),
                    (None, Some(z)) => Some(format!("{z} zones")),
                    _ => None,
                };
                found.push(Product {
                    brand: "airzone",
                    driver: "airzone",
                    model: "Airzone WebServer".into(),
                    host: a.host,
                    port: a.port,
                    mac: a.mac,
                    device_id: None,
                    firmware: a.firmware,
                    note,
                    supported: true,
                });
            }
            if let Some(s) = shelly {
                found.push(Product {
                    brand: "shelly",
                    driver: "shelly",
                    model: s.model,
                    host: host.clone(),
                    port: 80,
                    mac: s.mac,
                    device_id: Some(s.id),
                    firmware: s.firmware,
                    note: s.note,
                    supported: s.supported,
                });
            }
            if let Some(t) = trydan {
                found.push(Product {
                    brand: "v2c",
                    driver: "trydan",
                    model: "V2C Trydan".into(),
                    host: host.clone(),
                    port: 80,
                    mac: None,
                    device_id: (!t.id.is_empty()).then_some(t.id),
                    firmware: t.firmware,
                    note: t.note,
                    supported: true,
                });
            }
            Some(found)
        }));
    }
    let mut found = vec![];
    for t in tasks {
        if let Ok(Some(f)) = t.await {
            found.extend(f);
        }
    }
    found.sort_by_key(|p| (p.host.parse::<Ipv4Addr>().map(u32::from).unwrap_or(0), p.brand));
    Ok(found)
}

/// `POST /api/v1/webserver` of an Airzone WebServer returns its MAC address and type.
pub fn is_airzone_webserver(info: &serde_json::Value) -> bool {
    info.get("mac").and_then(|v| v.as_str()).is_some_and(|m| m.chars().filter(|c| c.is_ascii_hexdigit()).count() == 12)
        || info.get("ws_type").and_then(|v| v.as_str()).is_some()
}

/// Running inside a container: the local address is the container's, not the LAN's.
pub fn in_container() -> bool {
    std::path::Path::new("/.dockerenv").exists() || std::path::Path::new("/run/.containerenv").exists()
}

const SUBNET_SETTING: &str = "scan.subnet";

/// Subnet scanned by default: the one saved from the web UI, else BREEZONE_SCAN_SUBNET, else the
/// server's own /24 (not inside a container, where it would be the container network).
pub async fn global_subnet(db: &crate::db::Db, configured: Option<&str>) -> Option<String> {
    if let Ok(Some(saved)) = crate::db::get_setting(db, SUBNET_SETTING).await {
        return Some(saved);
    }
    if let Some(c) = configured.filter(|c| !c.trim().is_empty()) {
        return Some(c.trim().to_string());
    }
    if in_container() {
        return None;
    }
    default_subnet()
}

pub async fn save_global_subnet(db: &crate::db::Db, subnet: Option<&str>) -> Result<(), String> {
    match subnet.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => {
            hosts_in(s)?;
            crate::db::set_setting(db, SUBNET_SETTING, s).await.map_err(|e| e.to_string())
        }
        None => crate::db::delete_setting(db, SUBNET_SETTING).await.map_err(|e| e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_hosts_of_subnet() {
        let hosts = hosts_in("192.168.1.0/24").unwrap();
        assert_eq!(hosts.len(), 254);
        assert_eq!(hosts[0], Ipv4Addr::new(192, 168, 1, 1));
        assert_eq!(hosts[253], Ipv4Addr::new(192, 168, 1, 254));
        assert_eq!(hosts_in("10.0.0.7/30").unwrap().len(), 2);
        assert!(hosts_in("10.0.0.0/8").is_err());
    }

    #[test]
    fn recognises_airzone_webservers_only() {
        // Real answer of an Airzone WebServer (firmware 4.20).
        let airzone = serde_json::json!({"mac":"AA:BB:CC:00:11:22","interface":"ethernet","ws_firmware":"4.20","cloud_connected":0,"ws_type":"ws_az"});
        assert!(is_airzone_webserver(&airzone));
        // Other HTTP services answering on port 3000.
        assert!(!is_airzone_webserver(&serde_json::json!({"type":"https://example/problems/http-400","status":400})));
        assert!(!is_airzone_webserver(&serde_json::json!({"mac":"not-a-mac"})));
    }
}
