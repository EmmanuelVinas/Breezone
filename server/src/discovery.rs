//! LAN discovery of Airzone WebServers: scans a subnet for hosts answering the local API
//! on port 3000.

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
    let limit = Arc::new(Semaphore::new(64));
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

/// Checks whether `host:port` is an Airzone WebServer. Any HTTP service may listen on port
/// 3000: only answers carrying the Airzone signature (`mac` / `ws_type`, or HVAC zones) count.
pub async fn probe(host: &str, port: u16) -> Option<Found> {
    let addr = format!("{host}:{port}");
    let open = tokio::time::timeout(Duration::from_millis(400), TcpStream::connect(&addr)).await;
    if !matches!(open, Ok(Ok(_))) {
        return None;
    }
    let client = AirzoneClient::new(host, port);
    let info = tokio::time::timeout(Duration::from_secs(3), client.webserver_info())
        .await
        .ok()
        .and_then(|r| r.ok())
        .filter(is_airzone_webserver);
    let zones = tokio::time::timeout(Duration::from_secs(4), client.read_all())
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
