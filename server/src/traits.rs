//! Catalog of device traits (ADR 0013): the data contract of each kind of device, one JSON file
//! per trait in `contracts/traits/`. Embedded in the server, served at `GET /api/traits`: clients
//! and other nodes that do not know a trait render it from this description (ADR 0012). The web
//! and mobile types are generated from the same files.

use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// Every trait file, embedded at build time (the tests check that none is forgotten).
const FILES: &[(&str, &str)] = &[
    ("climate.zone.json", include_str!("../contracts/traits/climate.zone.json")),
    ("energy.meter.json", include_str!("../contracts/traits/energy.meter.json")),
    ("energy.power.json", include_str!("../contracts/traits/energy.power.json")),
    ("switch.relay.json", include_str!("../contracts/traits/switch.relay.json")),
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Label {
    pub fr: String,
    pub en: String,
    pub es: String,
}

/// A value of the state, or a parameter of an action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    /// "bool" | "integer" | "number" | "string" | "enum"
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(default)]
    pub optional: bool,
    pub label: Label,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Action {
    pub name: String,
    pub label: Label,
    #[serde(default)]
    pub params: Vec<Field>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Trait {
    #[serde(rename = "trait")]
    pub name: String,
    pub version: u32,
    pub label: Label,
    #[serde(default)]
    pub state: Vec<Field>,
    #[serde(default)]
    pub actions: Vec<Action>,
}

impl Trait {
    /// `name@version`, as announced in the capabilities.
    pub fn id(&self) -> String {
        format!("{}@{}", self.name, self.version)
    }

    pub fn action(&self, name: &str) -> Option<&Action> {
        self.actions.iter().find(|a| a.name == name)
    }
}

static CATALOG: LazyLock<Vec<Trait>> = LazyLock::new(|| {
    FILES
        .iter()
        .map(|(file, json)| serde_json::from_str(json).unwrap_or_else(|e| panic!("contracts/traits/{file}: {e}")))
        .collect()
});

/// All the traits this server knows.
pub fn catalog() -> &'static [Trait] {
    &CATALOG
}

pub fn get(name: &str, version: u32) -> Option<&'static Trait> {
    catalog().iter().find(|t| t.name == name && t.version == version)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const KINDS: &[&str] = &["bool", "integer", "number", "string", "enum"];

    fn check_field(t: &str, f: &Field) {
        assert!(KINDS.contains(&f.kind.as_str()), "{t}.{}: unknown type {}", f.name, f.kind);
        assert_eq!(f.kind == "enum", !f.values.is_empty(), "{t}.{}: an enum lists its values, and only an enum", f.name);
        for l in [&f.label.fr, &f.label.en, &f.label.es] {
            assert!(!l.trim().is_empty(), "{t}.{}: a label is missing", f.name);
        }
        if let (Some(min), Some(max)) = (f.min, f.max) {
            assert!(min <= max, "{t}.{}: min > max", f.name);
        }
    }

    #[test]
    fn every_trait_file_is_embedded_and_valid() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("contracts/traits");
        let on_disk: HashSet<String> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        let embedded: HashSet<String> = FILES.iter().map(|(f, _)| f.to_string()).collect();
        assert_eq!(on_disk, embedded, "add the new trait files to FILES");

        let mut ids = HashSet::new();
        for t in catalog() {
            assert!(t.version >= 1, "{}", t.name);
            assert!(ids.insert(t.id()), "{} defined twice", t.id());
            let mut names = HashSet::new();
            for f in &t.state {
                check_field(&t.name, f);
                assert!(names.insert(&f.name), "{}.{} twice", t.name, f.name);
            }
            let mut actions = HashSet::new();
            for a in &t.actions {
                assert!(actions.insert(&a.name), "{}.{} twice", t.name, a.name);
                for p in &a.params {
                    check_field(&format!("{}.{}", t.name, a.name), p);
                }
            }
        }
    }

    #[test]
    fn announced_traits_are_in_the_catalog() {
        for (name, versions) in crate::capabilities::Capabilities::local().traits {
            for v in versions {
                assert!(get(&name, v).is_some(), "{name}@{v} is announced but not described in contracts/traits");
            }
        }
    }

    #[test]
    fn climate_zone_matches_the_zone_commands() {
        let t = get("climate.zone", 1).unwrap();
        let params: Vec<&str> = t.action("set").unwrap().params.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(params, crate::hub::ZoneCommand::FIELDS, "the `set` action and ZoneCommand must list the same fields");
        let modes = &t.state.iter().find(|f| f.name == "mode").unwrap().values;
        for m in modes {
            assert!(serde_json::from_value::<crate::airzone::OperationMode>(serde_json::json!(m)).is_ok(), "mode {m}");
        }
    }
}
