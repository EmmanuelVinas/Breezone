//! Catalog of service contracts (docs/roadmap/modules.md § 4.2): what a module of the `service`
//! role answers that is not a device (the colour of the Tempo days, the weather, the readings of
//! the grid meter). One JSON file per contract in `contracts/services/`, embedded like the traits
//! and served at `GET /api/services`. Breezone keeps the caches and the tables; a module only
//! answers the queries.

use crate::traits::{Field, Label};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::LazyLock;

const FILES: &[(&str, &str)] = &[
    ("airzone.webserver.json", include_str!("../contracts/services/airzone.webserver.json")),
    ("conditions.json", include_str!("../contracts/services/conditions.json")),
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
    /// The answer is a record of the fields the module's manifest declares (`conditions`), not of
    /// the contract's.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub open: bool,
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

/// A contract by its id ("tariff.day_color@1").
pub fn by_id(id: &str) -> Option<&'static Service> {
    let (name, version) = id.split_once('@')?;
    get(name, version.split('.').next()?.parse().ok()?)
}

/// Whether a value is one of a field.
fn accepts(f: &Field, v: &Value) -> bool {
    let within = |n: f64| f.min.is_none_or(|m| n >= m) && f.max.is_none_or(|m| n <= m);
    match f.kind.as_str() {
        "bool" => v.is_boolean(),
        "integer" => v.as_i64().is_some_and(|n| within(n as f64)),
        "number" => v.as_f64().is_some_and(within),
        "string" => v.is_string(),
        "enum" => v.as_str().is_some_and(|s| f.values.iter().any(|x| x == s)),
        // A document passed as it is (the answer of a device, for the core to read).
        "json" => !v.is_null(),
        _ => false,
    }
}

fn check_record(q: &Query, v: &Value) -> Result<(), String> {
    let o = v.as_object().ok_or("a record is not an object")?;
    for f in &q.result {
        match o.get(&f.name) {
            Some(Value::Null) | None if f.optional => {}
            Some(x) if accepts(f, x) => {}
            Some(x) => return Err(format!("{}: {x} is not a value of it", f.name)),
            None => return Err(format!("{}: missing", f.name)),
        }
    }
    Ok(())
}

/// Checks the answer of a module to a query of a contract: a record (or a list of them, as the
/// contract says) whose values are of their fields, the required ones there.
pub fn check_answer(contract: &str, query: &str, answer: &Value) -> Result<(), String> {
    let service = by_id(contract).ok_or_else(|| format!("{contract}: unknown contract"))?;
    let q = service.query(query).ok_or_else(|| format!("{contract}: no query {query}"))?;
    if q.open {
        return answer.as_object().map(|_| ()).ok_or_else(|| "a record is expected".into());
    }
    if q.list {
        answer.as_array().ok_or("a list is expected")?.iter().try_for_each(|r| check_record(q, r))
    } else {
        check_record(q, answer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashSet;

    #[test]
    fn an_answer_is_checked_against_its_contract() {
        let ok = json!([{"day": "2026-11-22", "color": "red"}]);
        assert_eq!(check_answer("tariff.day_color@1", "colors", &ok), Ok(()));
        assert!(check_answer("tariff.day_color@1", "colors", &json!([{"day": "2026-11-22", "color": "green"}])).is_err());
        assert!(check_answer("tariff.day_color@1", "colors", &json!([{"color": "red"}])).is_err());
        assert!(check_answer("tariff.day_color@1", "colors", &json!({"day": "2026-11-22", "color": "red"})).is_err(), "a list");
        assert!(check_answer("tariff.day_color@2", "colors", &ok).is_err());
        assert!(check_answer("tariff.day_color@1", "other", &ok).is_err());
    }

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
                assert!(q.open || !q.result.is_empty(), "{}.{}: no result", s.name, q.name);
                for f in q.params.iter().chain(&q.result) {
                    // `json`: a document passed as it is, for services only.
                    if f.kind == "json" {
                        assert!(f.values.is_empty() && ![&f.label.fr, &f.label.en, &f.label.es].iter().any(|l| l.trim().is_empty()), "{}.{}.{}", s.name, q.name, f.name);
                    } else {
                        crate::traits::check_field(&format!("{}.{}", s.name, q.name), f);
                    }
                }
            }
            for (what, rev) in &s.since {
                assert!(queries.contains(what), "{}: since names {what}, which it does not have", s.name);
                assert!(*rev >= 1 && *rev <= s.revision, "{}.{what}: since {rev} is not a revision", s.name);
            }
        }
    }
}
