//! The manifest of a module (`manifest.json`, docs/roadmap/modules.md § 4.3): who it is, what it
//! needs, what it gives, how it is configured, found and paired, and what it may reach. Read by
//! the server without loading the module's code; its JSON Schema is `contracts/module.schema.json`.

use crate::traits::Label;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Gives sources of devices.
    Driver,
    /// A connection to an online service, used by other modules.
    Account,
    /// Answers a service contract.
    Service,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub version: String,
    pub name: Label,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<Label>,
    pub roles: Vec<Role>,
    pub requires: Requires,
    #[serde(default)]
    pub provides: Provides,
    /// Models recognised by name (others of the same family may work).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub config: Vec<ConfigField>,
    /// Version of the configuration of its sources: the module migrates an older one.
    #[serde(default = "one")]
    pub config_version: u32,
    /// Values of the configuration Breezone gives the module itself, by key: `home.latitude`,
    /// `home.longitude` (when the form leaves them empty), `server.timezone` (always).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fills: BTreeMap<String, String>,
    #[serde(default)]
    pub pairing: Pairing,
    #[serde(default)]
    pub discovery: Discovery,
    #[serde(default)]
    pub permissions: Permissions,
    /// Shortest time between two readings of the hardware (some cope badly with frequent ones).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_every_s: Option<u32>,
    /// Reaches the hardware through its maker's cloud (its devices may be the twins of local ones).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cloud: bool,
    /// A service the core uses by itself (the protocol of the Airzone WebServer): never switched
    /// off, not listed in Settings › Accounts and services.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub internal: bool,
    /// The calls of the interface it answers beyond the required ones.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub optional_calls: Vec<String>,
    /// Its account is an OAuth 2 one: Breezone renews its access itself (`host.access-token`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth: Option<OAuth>,
    /// What the automations may read from it, as the fields of a state (the service contract
    /// `conditions@1`: `query("conditions@1", "read")` gives their values).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<crate::traits::Field>,
}

/// The service contract of the conditions a module brings to the automations.
pub const CONDITIONS: &str = "conditions@1";

/// How Breezone renews the access of an OAuth 2 account: the refresh token (a field of the form,
/// `refresh_token`, with `client_id` and `client_secret`) traded at `token_url`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OAuth {
    pub token_url: String,
}

fn one() -> u32 {
    1
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Requires {
    /// Versions of the module interface (`^1.0`).
    pub api: String,
    /// Versions of Breezone, besides the interface (`>=0.14.0, <2.0.0`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub breezone: Option<String>,
    /// Modules of the `account` role it needs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accounts: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Provides {
    /// `trait@major.revision`: the traits its devices may have, with the lowest revision it uses.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub traits: Vec<String>,
    /// `contract@major.revision`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub services: Vec<String>,
}

/// A field of the configuration form of a source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigField {
    pub name: String,
    /// "string" | "integer" | "number" | "bool" | "password" (kept encrypted, in the source's
    /// secret: never shown again)
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub optional: bool,
    pub label: Label,
}

/// How a source is added (docs/roadmap/modules.md § 4.6).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pairing {
    /// The fields of `config`.
    #[default]
    Form,
    /// Then a button to press on the hardware.
    Button,
    /// Then a code written on the hardware.
    Code,
    /// An authorisation on the maker's site.
    Oauth,
}

/// What the server looks for on the network to propose this module, without loading it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Discovery {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mdns: Vec<MdnsHint>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub http: Vec<HttpHint>,
    /// Open TCP ports worth a `probe` of the module.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tcp: Vec<u16>,
    /// First three bytes of the MAC addresses of the maker (`E8:DB:84`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mac_oui: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MdnsHint {
    /// `_shelly._tcp`
    pub service: String,
    /// TXT records that must have these values.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub txt: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HttpHint {
    pub port: u16,
    pub path: String,
    /// Keys the JSON answer must have.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub json_has: Vec<String>,
    /// Text the answer must contain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contains: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Permissions {
    #[serde(default)]
    pub network: Network,
    /// Memory of one instance, at most 64.
    #[serde(default = "default_memory")]
    pub memory_mb: u32,
}

impl Default for Permissions {
    fn default() -> Self {
        Self { network: Network::default(), memory_mb: default_memory() }
    }
}

fn default_memory() -> u32 {
    16
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Network {
    /// The addresses of the local network.
    #[serde(default = "crate::yes")]
    pub lan: bool,
    /// Hosts of the Internet, by name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hosts: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub udp_broadcast: bool,
    /// mDNS services it may browse.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mdns: Vec<String>,
}

impl Default for Network {
    fn default() -> Self {
        Self { lan: true, hosts: vec![], udp_broadcast: false, mdns: vec![] }
    }
}

/// What Breezone can fill in a configuration (`fills`).
pub const FILLS: &[&str] = &["home.latitude", "home.longitude", "server.timezone"];

/// The calls of the driver interface a module may answer beyond `describe`, `read` and `act`.
pub const OPTIONAL_CALLS: &[&str] = &["rooms", "scenes", "picture", "forecast", "listen", "unlock", "diagnostic", "probe", "schedules", "forget", "identify", "history", "rename"];

/// A trait or a contract a module gives: its name, major version and lowest revision used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Needed {
    pub name: String,
    pub major: u32,
    pub revision: u32,
}

impl Needed {
    /// `cover.position@1.2`, or `cover.position@1` (revision 0).
    pub fn parse(s: &str) -> Option<Self> {
        let (name, version) = s.split_once('@')?;
        let (major, revision) = match version.split_once('.') {
            Some((m, r)) => (m.parse().ok()?, r.parse().ok()?),
            None => (version.parse().ok()?, 0),
        };
        let valid = !name.is_empty() && name.split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'));
        (valid && major >= 1).then(|| Needed { name: name.to_string(), major, revision })
    }

    /// `x.<module>.<name>`: a trait a module adds while the catalog has none for it.
    pub fn is_extension(&self) -> bool {
        self.name.starts_with("x.")
    }
}

/// `1.0` → `1.0.0`: the interface is versioned by major and minor only.
pub fn api_version(s: &str) -> Option<semver::Version> {
    semver::Version::parse(s).ok().or_else(|| semver::Version::parse(&format!("{s}.0")).ok())
}

fn is_id(s: &str) -> bool {
    (2..=40).contains(&s.len()) && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') && s.starts_with(|c: char| c.is_ascii_lowercase())
}

fn is_host(s: &str) -> bool {
    s.len() <= 253 && s.contains('.') && s.split('.').all(|l| !l.is_empty() && l.len() <= 63 && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') && !l.starts_with('-'))
}

fn label_ok(l: &Label) -> bool {
    ![&l.fr, &l.en, &l.es].iter().any(|s| s.trim().is_empty())
}

impl Manifest {
    pub fn parse(json: &str) -> Result<Self, Vec<String>> {
        let m: Manifest = serde_json::from_str(json).map_err(|e| vec![e.to_string()])?;
        m.validate()?;
        Ok(m)
    }

    pub fn has(&self, role: Role) -> bool {
        self.roles.contains(&role)
    }

    pub fn semver(&self) -> semver::Version {
        semver::Version::parse(&self.version).expect("validated")
    }

    /// What is wrong with it, all at once.
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = vec![];
        let mut fail = |e: String| errors.push(e);
        if !is_id(&self.id) {
            fail(format!("id {:?}: 2 to 40 of a-z, 0-9 and _, starting with a letter", self.id));
        }
        if semver::Version::parse(&self.version).is_err() {
            fail(format!("version {:?} is not x.y.z", self.version));
        }
        if !label_ok(&self.name) {
            fail("name: a label is missing".into());
        }
        if self.roles.is_empty() {
            fail("roles: none".into());
        }
        let mut roles = self.roles.clone();
        roles.sort();
        roles.dedup();
        if roles.len() != self.roles.len() {
            fail("roles: one is given twice".into());
        }
        if semver::VersionReq::parse(&self.requires.api).is_err() {
            fail(format!("requires.api {:?} is not a version requirement", self.requires.api));
        }
        if let Some(b) = &self.requires.breezone {
            if semver::VersionReq::parse(b).is_err() {
                fail(format!("requires.breezone {b:?} is not a version requirement"));
            }
        }
        for a in &self.requires.accounts {
            if !is_id(a) {
                fail(format!("requires.accounts: {a:?} is not a module id"));
            }
        }
        for t in &self.provides.traits {
            match Needed::parse(t) {
                None => fail(format!("provides.traits: {t:?} is not name@major[.revision]")),
                Some(n) if n.is_extension() && !n.name.starts_with(&format!("x.{}.", self.id)) => {
                    fail(format!("provides.traits: {t}: an extension trait is named x.{}.<name>", self.id))
                }
                Some(_) => {}
            }
        }
        for s in &self.provides.services {
            if Needed::parse(s).is_none_or(|n| n.is_extension()) {
                fail(format!("provides.services: {s:?} is not name@major[.revision]"));
            }
        }
        if self.has(Role::Driver) && self.provides.traits.is_empty() {
            fail("a driver provides traits".into());
        }
        if self.has(Role::Service) && self.provides.services.is_empty() {
            fail("a service provides service contracts".into());
        }
        let gives_conditions = self.provides.services.iter().any(|s| s.split_once('@').is_some_and(|(n, v)| n == "conditions" && v.starts_with('1')));
        if gives_conditions == self.conditions.is_empty() {
            fail("conditions: listed, with the service contract conditions@1.0, or neither".into());
        }
        let mut seen = std::collections::HashSet::new();
        for f in &self.conditions {
            if !seen.insert(&f.name) || f.name.is_empty() || !f.name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
                fail(format!("conditions.{}: a name of its own, in lowercase", f.name));
            }
            if !["bool", "enum", "integer", "number"].contains(&f.kind.as_str()) {
                fail(format!("conditions.{}: bool, enum, integer or number, not {}", f.name, f.kind));
            }
            let mut labelled: Vec<&String> = f.value_labels.keys().collect();
            let mut values: Vec<&String> = f.values.iter().collect();
            labelled.sort();
            values.sort();
            if (f.kind == "enum") == f.values.is_empty() || labelled != values || !f.value_labels.values().all(label_ok) {
                fail(format!("conditions.{}: an enum lists its values, each with its labels", f.name));
            }
            if !label_ok(&f.label) {
                fail(format!("conditions.{}: a label is missing", f.name));
            }
        }
        let mut names = std::collections::HashSet::new();
        for f in &self.config {
            if !names.insert(&f.name) {
                fail(format!("config.{}: twice", f.name));
            }
            if !["string", "integer", "number", "bool", "password"].contains(&f.kind.as_str()) {
                fail(format!("config.{}: unknown type {}", f.name, f.kind));
            }
            if !label_ok(&f.label) {
                fail(format!("config.{}: a label is missing", f.name));
            }
        }
        for (key, from) in &self.fills {
            if !FILLS.contains(&from.as_str()) {
                fail(format!("fills.{key}: {from} is not one of {}", FILLS.join(", ")));
            }
        }
        if self.config_version == 0 {
            fail("config_version starts at 1".into());
        }
        if !(1..=64).contains(&self.permissions.memory_mb) {
            fail("permissions.memory_mb: 1 to 64".into());
        }
        for h in &self.permissions.network.hosts {
            // `*.example.com`: its subdomains (a service whose regions and pictures move), never `*.com`.
            let named = h.strip_prefix("*.").map_or(h.as_str(), |d| d);
            if !is_host(named) || (h.starts_with("*.") && named.matches('.').count() < 1) {
                fail(format!("permissions.network.hosts: {h:?} is not a host name"));
            }
        }
        for s in self.permissions.network.mdns.iter().chain(self.discovery.mdns.iter().map(|m| &m.service)) {
            if !(s.starts_with('_') && (s.ends_with("._tcp") || s.ends_with("._udp"))) {
                fail(format!("mdns: {s:?} is not _service._tcp or _service._udp"));
            }
        }
        for oui in &self.discovery.mac_oui {
            let ok = oui.len() == 8 && oui.split(':').all(|b| b.len() == 2 && b.chars().all(|c| c.is_ascii_hexdigit()));
            if !ok {
                fail(format!("discovery.mac_oui: {oui:?} is not AA:BB:CC"));
            }
        }
        for h in &self.discovery.http {
            if !h.path.starts_with('/') {
                fail(format!("discovery.http: {:?} does not start with /", h.path));
            }
        }
        for c in &self.optional_calls {
            if !OPTIONAL_CALLS.contains(&c.as_str()) {
                fail(format!("optional_calls: unknown call {c}"));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[cfg(test)]
pub(crate) fn example() -> Manifest {
    Manifest::parse(include_str!("../../contracts/module.example.json")).expect("the example manifest is valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_example_is_valid_and_complete() {
        let m = example();
        assert_eq!(m.id, "shelly");
        assert!(m.has(Role::Driver));
        // Every property of the JSON Schema is a field of the manifest, and the other way round.
        let schema: serde_json::Value = serde_json::from_str(include_str!("../../contracts/module.schema.json")).unwrap();
        let mut in_schema: Vec<&String> = schema["properties"].as_object().unwrap().keys().collect();
        in_schema.sort();
        let full: serde_json::Value = serde_json::from_str(include_str!("../../contracts/module.example.json")).unwrap();
        let mut in_example: Vec<&String> = full.as_object().unwrap().keys().collect();
        in_example.sort();
        assert_eq!(in_schema, in_example, "contracts/module.schema.json and module.example.json list the same properties");
        assert_eq!(Manifest::parse(&serde_json::to_string(&m).unwrap()), Ok(m), "nothing is lost when the manifest is written again");
    }

    #[test]
    fn mistakes_are_all_told() {
        let mut m = example();
        m.id = "Shelly".into();
        m.version = "1.0".into();
        m.requires.api = "one".into();
        m.provides.traits = vec!["cover.position".into(), "x.other.thing@1".into()];
        m.permissions.memory_mb = 500;
        m.permissions.network.hosts = vec!["https://api.example.com".into()];
        m.optional_calls = vec!["exec".into()];
        let errors = m.validate().unwrap_err();
        assert_eq!(errors.len(), 8, "{errors:#?}");
    }

    #[test]
    fn needed_traits_are_read() {
        assert_eq!(Needed::parse("cover.position@1.2"), Some(Needed { name: "cover.position".into(), major: 1, revision: 2 }));
        assert_eq!(Needed::parse("light@1"), Some(Needed { name: "light".into(), major: 1, revision: 0 }));
        for bad in ["light", "light@0", "Light@1", "light@1.x", "@1", "a..b@1"] {
            assert_eq!(Needed::parse(bad), None, "{bad}");
        }
        assert_eq!(api_version("1.0"), Some(semver::Version::new(1, 0, 0)));
    }
}
