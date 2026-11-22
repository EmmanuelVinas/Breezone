//! The HomeKit accessories of the local network, from mDNS (`_hap._tcp`).

use std::net::Ipv4Addr;
use std::time::Duration;

const SERVICE: &str = "_hap._tcp";

/// An accessory as it announces itself.
#[derive(Debug, Clone, PartialEq)]
pub struct Announced {
    /// Its name ("Module Control").
    pub name: String,
    /// Its pairing id ("A1:B8:D6:5C:39:A6"), stable.
    pub id: String,
    /// "ip:port".
    pub host: String,
    /// "Legrand Gateway".
    pub model: Option<String>,
    /// Paired with a controller already (`sf` 0).
    pub paired: bool,
    /// Category (2: bridge, 14: window covering…).
    pub category: Option<u32>,
    /// Feature flags: 1, Apple authentication (certified accessories).
    pub flags: u32,
}

/// Asks the network for its HomeKit accessories, and waits `within` for the answers (see
/// [`crate::mdns::browse`]).
pub async fn browse(within: Duration, hosts: &[Ipv4Addr]) -> Vec<Announced> {
    let mut out: Vec<Announced> = crate::mdns::browse(&[SERVICE], within, hosts)
        .await
        .into_iter()
        .filter_map(|s| {
            Some(Announced {
                id: s.txt.get("id")?.clone(),
                host: format!("{}:{}", s.ip, s.port),
                model: s.txt.get("md").map(|m| m.trim_end_matches('\0').trim().to_string()),
                paired: s.txt.get("sf").is_some_and(|sf| sf.trim() == "0"),
                category: s.txt.get("ci").and_then(|c| c.trim().parse().ok()),
                flags: s.txt.get("ff").and_then(|f| f.trim().parse().ok()).unwrap_or(0),
                name: s.name,
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out.dedup_by(|a, b| a.id == b.id);
    out
}
