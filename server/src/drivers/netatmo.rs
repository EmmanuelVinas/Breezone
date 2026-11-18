//! Netatmo's cloud (Legrand Home + Control, its Ecometer…), through the linked account
//! (`accounts::netatmo`). A cloud module: what it reads also exists locally for most of it (the
//! shutters through HomeKit), so the runtime keeps one device per real one and uses this module
//! for what the local one lacks (stopping a shutter midway) or does not have at all (the Ecometer).
//!
//!   GET  {netatmo}/api/homesdata             → {body: {homes: [{id, name, rooms, modules: [{id, type, name, bridge}]}]}}
//!   GET  {netatmo}/api/homestatus?home_id=…  → {body: {home: {modules: [{id, current_position, on, power…}]}}}
//!   POST {netatmo}/api/setstate {home: {id, modules: [{id, bridge, target_position | on | brightness}]}}
//!        target_position -1 stops a Legrand shutter where it is.

use super::{Described, DeviceInfo, Driver, DriverError, DriverFuture, DriverInfo, State};
use crate::crypto::Key;
use crate::db::Db;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::Mutex;

pub fn info() -> DriverInfo {
    DriverInfo {
        id: "netatmo",
        label: "Netatmo · Legrand (cloud)",
        provides: vec!["cover.position@1", "light@1", "switch.relay@1", "energy.power@1"],
        models: vec!["Legrand Home + Control".into(), "Legrand Ecocompteur".into()],
        config: vec![],
    }
}

/// What a module of Netatmo is, for Breezone.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Cover,
    Light {
        dimmable: bool,
    },
    Relay,
    /// Measures only (the lines of an Ecometer, a meter).
    Power,
}

/// The kind of a module from its type; `None`: nothing to show (a gateway, a remote…) unless its
/// status measures a power.
fn kind_of(module_type: &str) -> Option<Kind> {
    match module_type {
        "NLV" | "NLLV" | "NLIV" | "NBR" | "NBO" | "NBS" => Some(Kind::Cover),
        "NLF" | "NLFN" | "NLFE" => Some(Kind::Light { dimmable: true }),
        "NLL" | "NLIS" | "NLPT" => Some(Kind::Light { dimmable: false }),
        "NLP" | "NLPM" | "NLPO" | "NLPD" | "NLC" | "NLM" | "NLAO" => Some(Kind::Relay),
        "NLE" | "NLPC" => Some(Kind::Power),
        _ => None,
    }
}

#[derive(Debug, Clone)]
struct Module {
    home: String,
    id: String,
    name: String,
    bridge: Option<String>,
    kind: Kind,
    /// Its status gives a power: a relay that measures, a line of the Ecometer.
    measures: bool,
}

impl Module {
    fn reference(&self) -> String {
        format!("{}/{}", self.home, self.id)
    }

    fn info(&self) -> DeviceInfo {
        let (kind, mut traits) = match self.kind {
            Kind::Cover => ("shutter", vec!["cover.position@1"]),
            Kind::Light { .. } => ("light", vec!["light@1"]),
            Kind::Relay => ("switch.relay", vec!["switch.relay@1"]),
            Kind::Power => ("energy.power", vec![]),
        };
        if self.measures || self.kind == Kind::Power {
            traits.push("energy.power@1");
        }
        DeviceInfo { reference: self.reference(), kind: kind.into(), traits: traits.into_iter().map(str::to_string).collect(), name: self.name.clone() }
    }
}

pub struct Netatmo {
    http: reqwest::Client,
    db: Db,
    key: Key,
    url: String,
    /// The modules of every home, read again at each `describe`.
    modules: Mutex<Vec<Module>>,
}

fn unreachable(e: impl std::fmt::Display) -> DriverError {
    DriverError::Unreachable(format!("Netatmo: {e}"))
}

impl Netatmo {
    pub fn new(db: Db, key: Key, url: String) -> Self {
        let http = reqwest::Client::builder().timeout(Duration::from_secs(20)).build().unwrap_or_default();
        Self { http, db, key, url: url.trim_end_matches('/').to_string(), modules: Mutex::new(vec![]) }
    }

    async fn token(&self) -> Result<String, DriverError> {
        match crate::accounts::netatmo::access_in(&self.db, &self.key, &self.url).await {
            Ok(Some(t)) => Ok(t),
            Ok(None) => Err(DriverError::Unreachable("aucun compte Netatmo : Réglages › Comptes liés".into())),
            Err(e) => Err(unreachable(e.message.en)),
        }
    }

    async fn get(&self, path: &str) -> Result<Value, DriverError> {
        let resp = self.http.get(format!("{}{path}", self.url)).bearer_auth(self.token().await?).send().await.map_err(unreachable)?;
        let status = resp.status();
        let body: Value = resp.json().await.map_err(unreachable)?;
        if !status.is_success() {
            return Err(unreachable(body["error"]["message"].as_str().unwrap_or(status.as_str())));
        }
        Ok(body)
    }

    async fn status(&self, home: &str) -> Result<Vec<Value>, DriverError> {
        let body = self.get(&format!("/api/homestatus?home_id={home}")).await?;
        Ok(body["body"]["home"]["modules"].as_array().cloned().unwrap_or_default())
    }

    async fn modules(&self) -> Result<Vec<Module>, DriverError> {
        {
            let known = self.modules.lock().await;
            if !known.is_empty() {
                return Ok(known.clone());
            }
        }
        self.load().await
    }

    /// Reads the homes and their modules again (and which ones measure a power).
    async fn load(&self) -> Result<Vec<Module>, DriverError> {
        let data = self.get("/api/homesdata").await?;
        let mut out = vec![];
        for home in data["body"]["homes"].as_array().into_iter().flatten() {
            let Some(home_id) = home["id"].as_str() else { continue };
            let status = self.status(home_id).await.unwrap_or_default();
            let measures = |id: &str| status.iter().any(|m| m["id"].as_str() == Some(id) && m["power"].is_number());
            for m in home["modules"].as_array().into_iter().flatten() {
                let (Some(id), Some(t)) = (m["id"].as_str(), m["type"].as_str()) else { continue };
                // A module of a type not known here still counts when it measures a power.
                let Some(kind) = kind_of(t).or_else(|| measures(id).then_some(Kind::Power)) else { continue };
                out.push(Module {
                    home: home_id.to_string(),
                    id: id.to_string(),
                    name: m["name"].as_str().unwrap_or(id).to_string(),
                    bridge: m["bridge"].as_str().map(str::to_string),
                    kind,
                    measures: measures(id),
                });
            }
        }
        *self.modules.lock().await = out.clone();
        Ok(out)
    }
}

/// The state of a module as its trait says it, from its status.
fn state_of(m: &Module, s: &Value) -> State {
    let mut out = Map::new();
    let num = |k: &str| s[k].as_f64();
    match m.kind {
        Kind::Cover => {
            let position = num("current_position").map(|p| p.clamp(0.0, 100.0).round() as i64);
            let target = num("target_position").filter(|t| *t >= 0.0).map(|t| t.clamp(0.0, 100.0).round() as i64);
            if let Some(p) = position {
                out.insert("position".into(), json!(p));
                let moving = match target {
                    Some(t) if t > p => "opening",
                    Some(t) if t < p => "closing",
                    _ => "stopped",
                };
                out.insert("moving".into(), json!(moving));
            }
            if let Some(t) = target {
                out.insert("target".into(), json!(t));
            }
            out.insert("stoppable".into(), json!(true));
        }
        Kind::Light { dimmable } => {
            out.insert("on".into(), json!(s["on"].as_bool().unwrap_or(false)));
            if dimmable {
                if let Some(b) = num("brightness") {
                    out.insert("brightness".into(), json!(b.round() as i64));
                }
            }
        }
        Kind::Relay => {
            out.insert("on".into(), json!(s["on"].as_bool().unwrap_or(false)));
        }
        Kind::Power => {}
    }
    if let Some(p) = num("power") {
        out.insert("power".into(), json!(p));
    }
    out
}

impl Driver for Netatmo {
    fn describe(&self) -> DriverFuture<'_, Described> {
        Box::pin(async move {
            let modules = self.load().await?;
            Ok(Described { model: "Netatmo · Legrand".into(), firmware: None, devices: modules.iter().map(Module::info).collect(), identity: None })
        })
    }

    fn read(&self) -> DriverFuture<'_, HashMap<String, State>> {
        Box::pin(async move {
            let modules = self.modules().await?;
            let mut homes: Vec<&str> = modules.iter().map(|m| m.home.as_str()).collect();
            homes.dedup();
            let mut out = HashMap::new();
            for home in homes {
                for s in self.status(home).await? {
                    let Some(m) = modules.iter().find(|m| m.home == home && s["id"].as_str() == Some(m.id.as_str())) else { continue };
                    // A module the gateway does not reach is left without a state: unavailable.
                    if s["reachable"].as_bool() == Some(false) {
                        continue;
                    }
                    out.insert(m.reference(), state_of(m, &s));
                }
            }
            Ok(out)
        })
    }

    fn act<'a>(&'a self, reference: &'a str, trait_name: &'a str, action: &'a str, params: &'a State) -> DriverFuture<'a, ()> {
        Box::pin(async move {
            let modules = self.modules().await?;
            let m = modules.iter().find(|m| m.reference() == reference).ok_or_else(|| DriverError::Unsupported(format!("{reference}: unknown module")))?;
            let mut change = Map::new();
            change.insert("id".into(), json!(m.id));
            if let Some(b) = &m.bridge {
                change.insert("bridge".into(), json!(b));
            }
            match (trait_name, action) {
                ("cover.position", "set") => {
                    let p = params.get("position").and_then(Value::as_i64).ok_or_else(|| DriverError::Invalid("`position` is required".into()))?;
                    change.insert("target_position".into(), json!(p.clamp(0, 100)));
                }
                ("cover.position", "stop") => {
                    change.insert("target_position".into(), json!(-1));
                }
                ("light", "set") | ("switch.relay", "set") => {
                    if let Some(on) = params.get("on").and_then(Value::as_bool) {
                        change.insert("on".into(), json!(on));
                    }
                    if let Some(b) = params.get("brightness").and_then(Value::as_i64) {
                        change.insert("brightness".into(), json!(b.clamp(1, 100)));
                        change.insert("on".into(), json!(true));
                    }
                }
                _ => return Err(DriverError::Unsupported(format!("{reference}: {trait_name}.{action}"))),
            }
            let body = json!({ "home": { "id": m.home, "modules": [Value::Object(change)] } });
            let resp = self.http.post(format!("{}/api/setstate", self.url)).bearer_auth(self.token().await?).json(&body).send().await.map_err(unreachable)?;
            if !resp.status().is_success() {
                let status = resp.status();
                let body: Value = resp.json().await.unwrap_or_default();
                return Err(DriverError::Invalid(format!("Netatmo: {}", body["error"]["message"].as_str().unwrap_or(status.as_str()))));
            }
            Ok(())
        })
    }

    /// Netatmo asks for few calls (a few hundred an hour for an account).
    fn read_every(&self) -> Duration {
        Duration::from_secs(60)
    }

    fn cloud(&self) -> bool {
        true
    }

    fn diagnostic(&self) -> DriverFuture<'_, Value> {
        Box::pin(async move {
            let data = self.get("/api/homesdata").await?;
            let mut status = Map::new();
            for home in data["body"]["homes"].as_array().into_iter().flatten() {
                if let Some(id) = home["id"].as_str() {
                    status.insert(id.into(), Value::Array(self.status(id).await.unwrap_or_default()));
                }
            }
            Ok(json!({ "homes": data["body"]["homes"], "status": status }))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(kind: Kind, measures: bool) -> Module {
        Module { home: "h".into(), id: "m".into(), name: "Volet salon".into(), bridge: Some("g".into()), kind, measures }
    }

    #[test]
    fn a_shutter_moving_and_a_line_of_the_ecometer() {
        let s = state_of(&module(Kind::Cover, false), &json!({"current_position": 20, "target_position": 100}));
        assert_eq!((s["position"].clone(), s["moving"].clone(), s["stoppable"].clone()), (json!(20), json!("opening"), json!(true)));
        let s = state_of(&module(Kind::Cover, false), &json!({"current_position": 40, "target_position": -1}));
        assert_eq!(s["moving"], "stopped", "-1: stopped where it is");
        let line = module(Kind::Power, true);
        assert_eq!(line.info().traits, ["energy.power@1"]);
        assert_eq!(state_of(&line, &json!({"power": 812}))["power"], 812.0);
        assert_eq!(kind_of("NLG"), None, "the gateway itself is not a device");
    }
}
