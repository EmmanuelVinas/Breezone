//! LAN discovery: scans a subnet for the products Breezone supports. Each address is asked, in
//! parallel, whether it is an Airzone WebServer (local API, port 3000), and every module whose
//! ports (`discovery.tcp` of its manifest) are open there is asked whether it is its hardware;
//! the modules found by mDNS are asked meanwhile. Products are identified down to the model.

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

/// The ports of `ports` open at `host`.
async fn open_ports(host: &str, ports: &[u16]) -> Vec<u16> {
    let checked = futures_util::future::join_all(ports.iter().map(|p| async move { is_open(host, *p).await.then_some(*p) })).await;
    checked.into_iter().flatten().collect()
}

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
    let info = tokio::time::timeout(Duration::from_secs(2), client.webserver_info()).await.ok().and_then(|r| r.ok()).filter(is_airzone_webserver);
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
    Some(Found { host: host.to_string(), port, mac: field("mac"), ws_type: field("ws_type"), firmware: field("ws_firmware"), zones })
}

/// A product found on the network, of a supported brand.
#[derive(Debug, Clone, Serialize)]
pub struct Product {
    /// The maker ("airzone", "v2c"…), most often the id of the module.
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
    /// HomeKit feature flags (1: Apple authentication), for pairing.
    pub flags: u32,
    /// The module that drives it is not installed: where to install it from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install: Option<Install>,
}

/// A module of the library to install for a product found (docs/roadmap/modules.md, lot L7).
#[derive(Debug, Clone, Serialize)]
pub struct Install {
    pub repository: i64,
    pub repository_name: String,
    pub official: bool,
    pub version: String,
}

/// A module of the library, not installed, that a product on the network may need: its HTTP hints
/// (`discovery.http` of its manifest) are tried on the addresses where nothing else answered, and
/// the services it announces by mDNS (`discovery.mdns`) are asked for.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub id: &'static str,
    pub name: String,
    pub hints: Vec<crate::modules::manifest::HttpHint>,
    pub mdns: Vec<crate::modules::manifest::MdnsHint>,
    pub install: Install,
}

/// The id of a module as a `&'static str` (kept once per id for the life of the process).
pub fn interned(id: &str) -> &'static str {
    static IDS: std::sync::LazyLock<std::sync::Mutex<std::collections::HashSet<&'static str>>> = std::sync::LazyLock::new(Default::default);
    let mut ids = IDS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(known) = ids.get(id) {
        return known;
    }
    let leaked: &'static str = Box::leak(id.to_string().into_boxed_str());
    ids.insert(leaked);
    leaked
}

/// Whether `host` answers one of the HTTP hints of a candidate (only on the ports open there).
async fn hinted(host: &str, open: &[u16], hints: &[crate::modules::manifest::HttpHint]) -> bool {
    let Ok(http) = reqwest::Client::builder().timeout(Duration::from_secs(2)).redirect(reqwest::redirect::Policy::none()).build() else { return false };
    for h in hints.iter().filter(|h| open.contains(&h.port)) {
        // Tasmota 16 answers its commands only to a request that says where it comes from.
        let base = format!("http://{host}:{}", h.port);
        let Ok(resp) = http.get(format!("{base}{}", h.path)).header("Referer", format!("{base}/")).send().await else { continue };
        if !resp.status().is_success() {
            continue;
        }
        let Ok(body) = resp.text().await else { continue };
        let contains = h.contains.as_deref().is_none_or(|c| body.contains(c));
        let keys = h.json_has.is_empty() || serde_json::from_str::<serde_json::Value>(&body).is_ok_and(|v| h.json_has.iter().all(|k| v.get(k).is_some()));
        if contains && keys && (h.contains.is_some() || !h.json_has.is_empty()) {
            return true;
        }
    }
    false
}

/// Every supported product on `subnet`, each address asked for every brand at once.
/// `quiet`: hosts only asked through mDNS, never connected to (the HomeKit gateways already
/// added: their few connections are kept for their sessions).
/// `candidates`: modules of the library not installed, whose hardware is looked for too.
pub async fn scan_products(subnet: &str, airzone_port: u16, quiet: &[std::net::Ipv4Addr], candidates: &[Candidate]) -> Result<Vec<Product>, String> {
    let hosts = hosts_in(subnet)?;
    // Meanwhile, the services the installed modules and those of the library announce by mDNS.
    let announced = tokio::spawn({
        let hosts = hosts.clone();
        let mut services: Vec<String> = crate::modules::announced_services();
        services.extend(candidates.iter().flat_map(|c| c.mdns.iter().map(|h| h.service.clone())));
        services.sort();
        services.dedup();
        async move { crate::mdns::browse(&services.iter().map(String::as_str).collect::<Vec<_>>(), Duration::from_secs(2), &hosts).await }
    });
    // Every port a module wants open before its probe, checked once per address.
    let mut ports: Vec<u16> =
        crate::modules::manifests().flat_map(|m| m.discovery.tcp.iter().copied()).chain(candidates.iter().flat_map(|c| c.hints.iter().map(|h| h.port))).collect();
    let candidates = Arc::new(candidates.to_vec());
    ports.sort_unstable();
    ports.dedup();
    let ports = Arc::new(ports);
    let limit = Arc::new(Semaphore::new(SCAN_PARALLEL));
    let mut tasks = Vec::with_capacity(hosts.len());
    for host in hosts.into_iter().filter(|h| !quiet.contains(h)) {
        let limit = limit.clone();
        let ports = ports.clone();
        let candidates = candidates.clone();
        tasks.push(tokio::spawn(async move {
            let _permit = limit.acquire().await.ok()?;
            let host = host.to_string();
            let (airzone, open) = tokio::join!(probe(&host, airzone_port), open_ports(&host, &ports));
            // Each module with a port open here is asked.
            let installed = crate::modules::identify_at(&host, &open).await;
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
                    flags: 0,
                    install: None,
                });
            }
            found.extend(installed);
            // Nothing installed knows it: the modules of the library are asked by their hints.
            if found.is_empty() {
                for c in candidates.iter() {
                    if hinted(&host, &open, &c.hints).await {
                        let port = c.hints.iter().map(|h| h.port).find(|p| open.contains(p)).unwrap_or(80);
                        found.push(Product {
                            brand: c.id,
                            driver: c.id,
                            model: c.name.clone(),
                            host: host.clone(),
                            port,
                            mac: None,
                            device_id: None,
                            firmware: None,
                            note: None,
                            supported: true,
                            flags: 0,
                            install: Some(c.install.clone()),
                        });
                        break;
                    }
                }
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
    let probed: Vec<String> = found.iter().map(|p| ip_of(&p.host).to_string()).collect();
    let announced = announced.await.unwrap_or_default();
    // A Hue bridge (and others) also answers as a HomeKit accessory: the module that probed it is
    // better. An accessory answers once per interface: kept once.
    let mut by_mdns = crate::modules::identify_announced(&announced).await;
    by_mdns.retain(|p| !probed.iter().any(|n| n == ip_of(&p.host)));
    by_mdns.sort_by(|a, b| (a.driver, &a.device_id, &a.host).cmp(&(b.driver, &b.device_id, &b.host)));
    by_mdns.dedup_by(|a, b| a.driver == b.driver && (a.device_id.is_some() && a.device_id == b.device_id || a.host == b.host));
    found.extend(by_mdns);
    found.extend(announced_products(&announced, &candidates, &found));
    found.sort_by_key(|p| (p.host.split(':').next().and_then(|h| h.parse::<Ipv4Addr>().ok()).map(u32::from).unwrap_or(0), p.brand));
    // The MAC its hardware does not tell (a Tydom asks for it): the one the network knows, now that
    // the scan has reached it.
    let macs = parse_arp(&std::fs::read_to_string("/proc/net/arp").unwrap_or_default());
    for p in found.iter_mut().filter(|p| p.mac.is_none()) {
        p.mac = macs.get(ip_of(&p.host)).cloned();
    }
    Ok(found)
}

/// The addresses of the neighbours and their MAC, from Linux's `/proc/net/arp` (empty elsewhere):
/// `IP address  HW type  Flags  HW address  Mask  Device`, incomplete entries left out.
fn parse_arp(text: &str) -> std::collections::HashMap<String, String> {
    text.lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            let (ip, flags, mac) = (*f.first()?, *f.get(2)?, *f.get(3)?);
            (flags != "0x0" && mac != "00:00:00:00:00:00" && mac.len() == 17).then(|| (ip.to_string(), mac.to_uppercase()))
        })
        .collect()
}

/// The products of the modules of the library that announced themselves by mDNS, at addresses
/// where nothing else was found (one per address, the first module whose hint matches).
fn announced_products(announced: &[crate::mdns::Service], candidates: &[Candidate], found: &[Product]) -> Vec<Product> {
    let same = |a: &str, b: &str| a.trim_end_matches(".local").eq_ignore_ascii_case(b.trim_end_matches(".local"));
    let mut out: Vec<Product> = vec![];
    for s in announced {
        let ip = s.ip.to_string();
        if found.iter().chain(out.iter()).any(|p| ip_of(&p.host) == ip) {
            continue;
        }
        let Some(c) = candidates.iter().find(|c| c.mdns.iter().any(|h| same(&h.service, &s.service) && crate::mdns::txt_matches(s, &h.txt))) else { continue };
        out.push(Product {
            brand: c.id,
            driver: c.id,
            model: c.name.clone(),
            host: ip,
            port: s.port,
            mac: None,
            device_id: None,
            firmware: None,
            note: Some(s.name.clone()),
            supported: true,
            flags: 0,
            install: Some(c.install.clone()),
        });
    }
    out
}

fn ip_of(host: &str) -> &str {
    host.split(':').next().unwrap_or(host)
}

/// `POST /api/v1/webserver` of an Airzone WebServer returns its MAC address and type.
pub fn is_airzone_webserver(info: &serde_json::Value) -> bool {
    info.get("mac").and_then(|v| v.as_str()).is_some_and(|m| m.chars().filter(|c| c.is_ascii_hexdigit()).count() == 12) || info.get("ws_type").and_then(|v| v.as_str()).is_some()
}

/// Running inside a container: the local address is the container's, not the LAN's.
pub fn in_container() -> bool {
    std::path::Path::new("/.dockerenv").exists() || std::path::Path::new("/run/.containerenv").exists()
}

const SUBNET_SETTING: &str = "scan.subnet";

/// Subnet scanned by default: the one saved from the web UI, else BREEZONE_SCAN_SUBNET, else the
/// /24 of the hardware Breezone already reaches (its WebServers and modules: inside a container,
/// the server's own address is the container network's), else the server's own /24.
pub async fn global_subnet(db: &crate::db::Db, configured: Option<&str>) -> Option<String> {
    if let Ok(Some(saved)) = crate::db::get_setting(db, SUBNET_SETTING).await {
        return Some(saved);
    }
    if let Some(c) = configured.filter(|c| !c.trim().is_empty()) {
        return Some(c.trim().to_string());
    }
    if let Some(known) = known_subnet(db).await {
        return Some(known);
    }
    if in_container() {
        return None;
    }
    default_subnet()
}

/// The /24 most of the hardware already added is on (private addresses only).
async fn known_subnet(db: &crate::db::Db) -> Option<String> {
    let mut hosts: Vec<String> = crate::db::list_installations(db).await.ok()?.into_iter().map(|i| i.host).filter(|h| !h.trim().is_empty()).collect();
    for source in crate::db::list_sources(db).await.ok()? {
        let config: serde_json::Value = serde_json::from_str(&source.config).unwrap_or_default();
        if let Some(h) = config["host"].as_str() {
            hosts.push(h.to_string());
        }
    }
    subnet_of_most(&hosts)
}

/// The /24 of the most addresses among `hosts` ("ip" or "ip:port"; names and public addresses left out).
fn subnet_of_most(hosts: &[String]) -> Option<String> {
    let mut counts: std::collections::BTreeMap<[u8; 3], usize> = Default::default();
    for h in hosts {
        let Ok(ip) = h.trim().split(':').next().unwrap_or_default().parse::<Ipv4Addr>() else { continue };
        if ip.is_private() {
            let o = ip.octets();
            *counts.entry([o[0], o[1], o[2]]).or_default() += 1;
        }
    }
    counts.into_iter().max_by_key(|(_, n)| *n).map(|(o, _)| format!("{}.{}.{}.0/24", o[0], o[1], o[2]))
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

    #[test]
    fn the_mac_of_a_neighbour_comes_from_the_arp_table() {
        let table = "IP address       HW type     Flags       HW address            Mask     Device
192.168.86.64    0x1         0x2         00:1a:25:12:34:56     *        eth0
192.168.86.70    0x1         0x0         00:00:00:00:00:00     *        eth0
";
        let macs = super::parse_arp(table);
        assert_eq!(macs.get("192.168.86.64").map(String::as_str), Some("00:1A:25:12:34:56"));
        assert!(!macs.contains_key("192.168.86.70"), "an incomplete entry");
    }

    use super::*;

    /// A module of the library whose hardware announces `_hue._tcp`, and what the network said.
    #[test]
    fn a_module_of_the_library_is_proposed_for_what_announces_its_service() {
        use crate::modules::manifest::MdnsHint;
        let install = Install { repository: 1, repository_name: "Breezone".into(), official: true, version: "1.0.0".into() };
        let hue = Candidate {
            id: "hue",
            name: "Philips Hue".into(),
            hints: vec![],
            mdns: vec![MdnsHint { service: "_hue._tcp".into(), txt: [("md".to_string(), "BSB002".to_string())].into() }],
            install,
        };
        let service = |ip: [u8; 4], service: &str, md: &str| crate::mdns::Service {
            service: service.into(),
            name: "Hue Bridge - 1A2B3C".into(),
            ip: ip.into(),
            port: 443,
            txt: [("md".to_string(), md.to_string())].into(),
        };
        let announced = [service([192, 168, 1, 20], "_hue._tcp", "BSB002"), service([192, 168, 1, 21], "_hue._tcp", "BSB001"), service([192, 168, 1, 22], "_hap._tcp", "BSB002")];
        let found = announced_products(&announced, std::slice::from_ref(&hue), &[]);
        assert_eq!(found.len(), 1, "the TXT values and the service must match");
        assert_eq!((found[0].driver, found[0].host.as_str(), found[0].port), ("hue", "192.168.1.20", 443));
        assert_eq!(found[0].install.as_ref().map(|i| i.version.as_str()), Some("1.0.0"));
        // An address where something was found already is left alone.
        assert!(announced_products(&announced, &[hue], &found).is_empty());
    }

    #[test]
    fn the_subnet_of_the_hardware_already_added() {
        let hosts = ["192.168.86.25:5001", "192.168.86.40", "10.0.0.7", "nas.local", "8.8.8.8"].map(String::from);
        assert_eq!(subnet_of_most(&hosts).as_deref(), Some("192.168.86.0/24"));
        assert_eq!(subnet_of_most(&["box.example.com".to_string()]), None);
    }

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
