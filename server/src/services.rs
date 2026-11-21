//! Catalog of service contracts (docs/roadmap/modules.md § 4.2): what a module of the `service`
//! role answers that is not a device (the colour of the Tempo days, the weather, the readings of
//! the grid meter). One JSON file per contract in `contracts/services/`, embedded like the traits
//! and served at `GET /api/services`. Breezone keeps the caches and the tables; a module only
//! answers the queries.

use crate::traits::{Field, Label};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::LazyLock;

const FILES: &[(&str, &str)] = &[
    ("energy.history.json", include_str!("../contracts/services/energy.history.json")),
    ("tariff.day_color.json", include_str!("../contracts/services/tariff.day_color.json")),
    ("weather.forecast.json", include_str!("../contracts/services/weather.forecast.json")),
    ("weather.warnings.json", include_str!("../contracts/services/weather.warnings.json")),
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Query {
    pub name: String,
    pub label: Label,
    #[serde(default)]
    pub params: Vec<Field>,
    /// The answer is a list of such records (else one record).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub list: bool,
    pub result: Vec<Field>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Service {
    #[serde(rename = "service")]
    pub name: String,
    pub version: u32,
    #[serde(default)]
    pub revision: u32,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub since: BTreeMap<String, u32>,
    pub label: Label,
    pub queries: Vec<Query>,
}

impl Service {
    pub fn id(&self) -> String {
        format!("{}@{}", self.name, self.version)
    }

    pub fn query(&self, name: &str) -> Option<&Query> {
        self.queries.iter().find(|q| q.name == name)
    }
}

static CATALOG: LazyLock<Vec<Service>> =
    LazyLock::new(|| FILES.iter().map(|(file, json)| serde_json::from_str(json).unwrap_or_else(|e| panic!("contracts/services/{file}: {e}"))).collect());

pub fn catalog() -> &'static [Service] {
    &CATALOG
}

pub fn get(name: &str, version: u32) -> Option<&'static Service> {
    catalog().iter().find(|s| s.name == name && s.version == version)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_service_file_is_embedded_and_valid() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("contracts/services");
        let on_disk: HashSet<String> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        let embedded: HashSet<String> = FILES.iter().map(|(f, _)| f.to_string()).collect();
        assert_eq!(on_disk, embedded, "add the new service files to FILES");
        let mut ids = HashSet::new();
        for s in catalog() {
            assert!(s.version >= 1, "{}", s.name);
            assert!(ids.insert(s.id()), "{} twice", s.id());
            assert!(!s.queries.is_empty(), "{}: no query", s.name);
            assert!(!crate::traits::catalog().iter().any(|t| t.name == s.name), "{}: also the name of a trait", s.name);
            let mut queries = HashSet::new();
            for q in &s.queries {
                assert!(queries.insert(&q.name), "{}.{} twice", s.name, q.name);
                assert!(!q.result.is_empty(), "{}.{}: no result", s.name, q.name);
                for f in q.params.iter().chain(&q.result) {
                    crate::traits::check_field(&format!("{}.{}", s.name, q.name), f);
                }
            }
            for (what, rev) in &s.since {
                assert!(queries.contains(what), "{}: since names {what}, which it does not have", s.name);
                assert!(*rev >= 1 && *rev <= s.revision, "{}.{what}: since {rev} is not a revision", s.name);
            }
        }
    }
}
