//! What a Breezone server knows (ADR 0012), exchanged with the other nodes (`POST
//! /api/federation/hello`) and the clients (`GET /api/capabilities`), so that servers and apps of
//! different versions only use what both sides understand, feature by feature, instead of forcing
//! everyone to update.
//!
//! * **traits**: data contracts of devices (`climate.zone`…), with the major versions this server
//!   reads and serves. A trait gets a new major version only for an incompatible change.
//! * **features**: what is not a device (federation routes, schedules…), with their version.
//! * **protocol**: version of the envelope (signatures, hello, snapshots) and the oldest one accepted.
//! * **caps_rev**: fingerprint of the document, to notice a change cheaply.
//!
//! A node that predates this exchange (up to 0.10.0) answers `404` to the hello: it gets the
//! [`Capabilities::legacy`] profile, frozen to what 0.10.0 does.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// Version of the federation envelope spoken by this server.
pub const PROTOCOL: u32 = 1;
/// Oldest envelope still accepted from a peer. Raised only for security or a fundamental change.
pub const PROTOCOL_MIN: u32 = 1;

/// Features that matter between nodes (the others only concern the clients of this server).
const FEDERATION_FEATURES: &[&str] = &["federation.watch", "federation.control", "federation.schedules", "federation.hello"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Protocol {
    pub current: u32,
    pub min: u32,
}

/// A module installed on the server (informational: decisions are taken on traits).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleInfo {
    pub id: String,
    pub version: String,
    /// `trait@version` provided by the module.
    #[serde(default)]
    pub provides: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Capabilities {
    /// Version of Breezone ("0.11.0-beta.2"); empty for a node that does not tell (legacy).
    #[serde(default)]
    pub breezone: String,
    /// "stable" | "beta" (a pre-release runs).
    #[serde(default)]
    pub channel: String,
    pub protocol: Protocol,
    #[serde(default)]
    pub traits: BTreeMap<String, Vec<u32>>,
    #[serde(default)]
    pub features: BTreeMap<String, u32>,
    #[serde(default)]
    pub modules: Vec<ModuleInfo>,
    #[serde(default)]
    pub caps_rev: String,
}

impl Capabilities {
    /// What this server knows.
    pub fn local() -> Self {
        let version = crate::update::current_version();
        let mut caps = Self {
            breezone: version.to_string(),
            channel: if version.is_prerelease() { "beta" } else { "stable" }.into(),
            protocol: Protocol { current: PROTOCOL, min: PROTOCOL_MIN },
            traits: BTreeMap::from([
                ("climate.zone".into(), vec![1]),
                ("energy.meter".into(), vec![1]),
                ("energy.power".into(), vec![1]),
                ("ev.charger".into(), vec![1]),
                ("switch.relay".into(), vec![1]),
            ]),
            features: [
                ("federation.watch", 1),
                ("federation.control", 1),
                ("federation.schedules", 1),
                ("federation.hello", 1),
                ("schedules.airzone", 1),
                ("timers", 1),
                ("scenes", 1),
                ("history", 1),
                ("stats.daily", 1),
                ("air_quality", 1),
                ("demand", 1),
                ("self_update", 1),
                ("update.beta", 1),
                // Rooms, devices and the catalog of traits (ADR 0013): /api/rooms, /api/devices, /api/traits.
                ("home.devices", 1),
                // Sources read by drivers (Shelly…) and the generic action on a device's trait.
                ("home.sources", 1),
                ("devices.actions", 1),
                // Device history and the energy of the home: /api/devices/{id}/history, /api/energy/*.
                ("devices.history", 1),
                ("energy", 1),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
            // Airzone is built in: it is announced like the modules to come.
            modules: vec![
                ModuleInfo { id: "airzone".into(), version: version.to_string(), provides: vec!["climate.zone@1".into()] },
                ModuleInfo { id: "shelly".into(), version: version.to_string(), provides: vec!["energy.power@1".into(), "energy.meter@1".into(), "switch.relay@1".into()] },
                ModuleInfo { id: "trydan".into(), version: version.to_string(), provides: vec!["ev.charger@1".into(), "energy.power@1".into()] },
            ],
            caps_rev: String::new(),
        };
        caps.caps_rev = caps.compute_rev();
        caps
    }

    /// A node that predates the hello (up to 0.10.0): what those versions do, frozen.
    pub fn legacy() -> Self {
        Self {
            breezone: String::new(),
            channel: "stable".into(),
            protocol: Protocol { current: 1, min: 1 },
            traits: BTreeMap::from([("climate.zone".into(), vec![1])]),
            features: [("federation.watch", 1), ("federation.control", 1), ("federation.schedules", 1), ("schedules.airzone", 1)]
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
            modules: vec![],
            caps_rev: "legacy".into(),
        }
    }

    pub fn is_legacy(&self) -> bool {
        self.caps_rev == "legacy"
    }

    pub fn compute_rev(&self) -> String {
        let mut copy = self.clone();
        copy.caps_rev = String::new();
        let json = serde_json::to_vec(&copy).expect("serializable capabilities");
        hex::encode(&Sha256::digest(&json)[..8])
    }

    pub fn has_feature(&self, name: &str, version: u32) -> bool {
        self.features.get(name).is_some_and(|v| *v >= version)
    }

    pub fn has_trait(&self, name: &str, version: u32) -> bool {
        self.traits.get(name).is_some_and(|v| v.contains(&version))
    }

    /// Bounds what a peer may send (it is stored and shown): sizes and lengths are capped.
    pub fn sanitized(mut self) -> Self {
        let clip = |s: &str, n: usize| s.chars().filter(|c| !c.is_control()).take(n).collect::<String>();
        self.breezone = clip(&self.breezone, 40);
        self.channel = clip(&self.channel, 16);
        self.caps_rev = clip(&self.caps_rev, 64);
        self.traits = self
            .traits
            .into_iter()
            .take(200)
            .map(|(k, mut v)| {
                v.truncate(8);
                (clip(&k, 64), v)
            })
            .collect();
        self.features = self.features.into_iter().take(200).map(|(k, v)| (clip(&k, 64), v)).collect();
        self.modules.truncate(50);
        for m in &mut self.modules {
            m.id = clip(&m.id, 64);
            m.version = clip(&m.version, 40);
            m.provides.truncate(50);
            m.provides = m.provides.iter().map(|p| clip(p, 80)).collect();
        }
        self
    }

    /// What `self` (this server) can use with `peer`, for the settings pages.
    pub fn compatibility(&self, peer: &Capabilities) -> Compatibility {
        let protocol_ok = peer.protocol.current >= self.protocol.min && self.protocol.current >= peer.protocol.min;
        // A trait version is supported or not; a feature version includes the earlier ones.
        let lacks = |a: &Capabilities, b: &Capabilities| -> Vec<String> {
            let traits = a.traits.iter().flat_map(|(t, vs)| vs.iter().filter(|v| !b.has_trait(t, **v)).map(move |v| format!("{t}@{v}")));
            let features = a.features.iter().filter(|(f, v)| FEDERATION_FEATURES.contains(&f.as_str()) && !b.has_feature(f, **v)).map(|(f, v)| format!("{f}@{v}"));
            traits.chain(features).collect()
        };
        let missing = lacks(self, peer);
        let newer = lacks(peer, self);
        let state = if !protocol_ok {
            "outdated"
        } else if missing.is_empty() && newer.is_empty() {
            "compatible"
        } else {
            "limited"
        };
        Compatibility { state, missing, newer }
    }
}

/// How well two servers understand each other.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Compatibility {
    /// "compatible" | "limited" (some features on one side only) | "outdated" (protocol too old:
    /// one of them must be updated)
    pub state: &'static str,
    /// What this server can do that the peer cannot (`trait@v`, `feature@v`).
    pub missing: Vec<String>,
    /// What the peer can do that this server cannot read yet.
    pub newer: Vec<String>,
}

/// What the settings pages show about a peer.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PeerView {
    /// Version of Breezone on the peer, if it tells (not before 0.10.1).
    pub version: Option<String>,
    pub channel: String,
    /// The peer predates the exchange of capabilities.
    pub legacy: bool,
    pub checked_at: Option<i64>,
    pub compatibility: Compatibility,
    pub modules: Vec<ModuleInfo>,
}

impl PeerView {
    /// From the stored capabilities of a peer (`None`: no exchange yet).
    pub fn of(stored: Option<&str>, checked_at: Option<i64>) -> Option<PeerView> {
        let peer: Capabilities = serde_json::from_str(stored?).ok()?;
        let compatibility = Capabilities::local().compatibility(&peer);
        Some(PeerView {
            version: Some(peer.breezone.clone()).filter(|v| !v.is_empty()),
            channel: peer.channel.clone(),
            legacy: peer.is_legacy(),
            checked_at,
            compatibility,
            modules: peer.modules,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_local_document_has_a_stable_fingerprint() {
        let a = Capabilities::local();
        assert_eq!(a.caps_rev, a.compute_rev());
        assert_eq!(a.caps_rev, Capabilities::local().caps_rev);
        assert!(a.has_trait("climate.zone", 1) && !a.has_trait("climate.zone", 2));
        assert!(a.has_feature("federation.hello", 1) && !a.has_feature("federation.hello", 2));
    }

    #[test]
    fn a_legacy_peer_is_limited_but_understood() {
        let local = Capabilities::local();
        let c = local.compatibility(&Capabilities::legacy());
        assert_eq!(c.state, "limited");
        for item in ["federation.hello@1", "energy.power@1", "energy.meter@1", "switch.relay@1"] {
            assert!(c.missing.contains(&item.to_string()), "{item} is missing on a legacy peer: {c:?}");
        }
        assert!(c.newer.is_empty());
        assert_eq!(local.compatibility(&local).state, "compatible");
    }

    #[test]
    fn a_newer_peer_shows_what_cannot_be_read_here() {
        let local = Capabilities::local();
        let mut peer = local.clone();
        peer.traits.insert("cover.position".into(), vec![1]);
        peer.features.insert("federation.watch".into(), 2);
        let c = local.compatibility(&peer);
        assert_eq!(c.state, "limited");
        assert!(c.newer.contains(&"cover.position@1".to_string()) && c.newer.contains(&"federation.watch@2".to_string()));
        assert!(c.missing.is_empty(), "version 2 of a feature includes version 1: {c:?}");
    }

    #[test]
    fn an_envelope_below_the_floor_must_be_updated() {
        let local = Capabilities::local();
        let mut peer = local.clone();
        peer.protocol = Protocol { current: 9, min: 9 };
        assert_eq!(local.compatibility(&peer).state, "outdated");
    }

    #[test]
    fn what_a_peer_sends_is_bounded() {
        let mut peer = Capabilities::local();
        peer.breezone = "1.0.0\u{0007}".repeat(20);
        for i in 0..500 {
            peer.features.insert(format!("f{i}"), 1);
        }
        let s = peer.sanitized();
        assert!(s.breezone.chars().count() <= 40 && !s.breezone.contains('\u{0007}'));
        assert!(s.features.len() <= 200);
    }
}
