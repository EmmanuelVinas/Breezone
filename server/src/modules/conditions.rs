//! The conditions a module brings to the automations (`conditions` of its manifest): the values it
//! tells by the service contract `conditions@1` (`query read`, an object of its fields), read every
//! few minutes and kept here for the rules to read.

use super::manifest::{Manifest, Role, CONDITIONS};
use crate::traits::Field;
use crate::AppState;
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};
use std::time::Duration;

/// How often the values of a module are read, unless its manifest says (`read_every_s`).
const EVERY: i64 = 300;

/// What a module told, and when it was read.
type Read = (i64, Map<String, Value>);

/// The values of each module.
static VALUES: LazyLock<RwLock<HashMap<String, Read>>> = LazyLock::new(Default::default);

/// The modules that bring conditions (installed).
pub fn providers() -> impl Iterator<Item = &'static Manifest> {
    super::manifests().filter(|m| !m.conditions.is_empty() && m.has(Role::Service) && super::installed_dir(&m.id).is_some())
}

/// A field of a module's conditions.
pub fn field(module: &str, name: &str) -> Option<&'static Field> {
    super::manifest(module)?.conditions.iter().find(|f| f.name == name)
}

/// The last values read, by module.
pub fn values() -> HashMap<String, Map<String, Value>> {
    VALUES.read().unwrap_or_else(|e| e.into_inner()).iter().map(|(id, (_, v))| (id.clone(), v.clone())).collect()
}

/// Only the fields the manifest declares, of the right type.
fn checked(m: &Manifest, answer: &Value) -> Map<String, Value> {
    m.conditions
        .iter()
        .filter_map(|f| {
            let v = answer.get(&f.name)?;
            let ok = match f.kind.as_str() {
                "bool" => v.is_boolean(),
                "enum" => v.as_str().is_some_and(|s| f.values.iter().any(|x| x == s)),
                _ => v.is_number(),
            };
            ok.then(|| (f.name.clone(), v.clone()))
        })
        .collect()
}

/// The modules whose conditions an enabled rule reads: only those are read on their own (a
/// module is loaded only when it is used).
async fn wanted(db: &crate::db::Db) -> Vec<String> {
    let rows = crate::db::list_automations(db).await.unwrap_or_default();
    let mut out: Vec<String> = rows.iter().filter(|r| r.enabled).filter_map(crate::automations::Automation::of).flat_map(|a| a.read_modules()).collect();
    out.sort();
    out.dedup();
    out
}

/// Reads the values of the modules whose turn it is (`force`: all of them, now); `only`: of
/// these modules alone.
pub async fn refresh(db: &crate::db::Db, now: i64, force: bool, only: Option<&[String]>) {
    for m in providers().filter(|m| only.is_none_or(|ids| ids.contains(&m.id))) {
        let every = m.read_every_s.map_or(EVERY, i64::from).max(60);
        let due = VALUES.read().unwrap_or_else(|e| e.into_inner()).get(&m.id).is_none_or(|(at, _)| now - at >= every);
        if !force && !due {
            continue;
        }
        match super::ask(db, m, CONDITIONS, "read", &Value::Object(Map::new())).await {
            Some(Ok(answer)) => {
                VALUES.write().unwrap_or_else(|e| e.into_inner()).insert(m.id.clone(), (now, checked(m, &answer)));
            }
            // Not readable now: what it said before no longer holds.
            Some(Err(_)) | None => {
                VALUES.write().unwrap_or_else(|e| e.into_inner()).remove(&m.id);
            }
        }
    }
    let ids: Vec<String> = providers().map(|m| m.id.clone()).collect();
    VALUES.write().unwrap_or_else(|e| e.into_inner()).retain(|id, _| ids.contains(id));
}

pub async fn run_loop(state: AppState) {
    loop {
        let wanted = wanted(&state.db).await;
        if !wanted.is_empty() {
            refresh(&state.db, crate::db::now(), false, Some(&wanted)).await;
        }
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}

/// What the pages read: the conditions of each module and their values now.
#[derive(Debug, Clone, Serialize)]
pub struct ModuleConditions {
    pub module: String,
    pub name: crate::traits::Label,
    pub fields: Vec<Field>,
    pub values: Map<String, Value>,
}

/// For the editor of the rules: the values of every module are read (when their turn has come).
pub async fn view(db: &crate::db::Db) -> Vec<ModuleConditions> {
    refresh(db, crate::db::now(), false, None).await;
    listed()
}

fn listed() -> Vec<ModuleConditions> {
    let now = values();
    providers()
        .map(|m| ModuleConditions { module: m.id.clone(), name: m.name.clone(), fields: m.conditions.clone(), values: now.get(&m.id).cloned().unwrap_or_default() })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_the_fields_declared_are_kept() {
        let m = super::super::manifest::example();
        let answer = json!({"overload": true, "phase": "d", "peak": 4200, "other": 1});
        let kept = checked(&m, &answer);
        assert_eq!(Value::Object(kept), json!({"overload": true, "peak": 4200}), "an unknown value and an unknown field are left out");
    }
}
