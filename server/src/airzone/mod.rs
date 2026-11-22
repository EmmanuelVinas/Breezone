//! The Airzone **local** API of a WebServer on the LAN (`http://<host>:3000/api/v1/`, no
//! authentication), spoken by the module `airzone_local` (`airzone.webserver@1`): the core keeps
//! the model of its answers (systems, zones, master zone, air quality) and everything built on
//! them (installations, history, Airzone Cloud, schedules, federation).
//!
//! Breezone's server is the only component talking to the WebServers; the mobile app and the web
//! UI only ever talk to Breezone.

pub mod model;

pub use model::{air_quality_level, master_zone, HvacResponse, OperationMode, ZoneData};

use crate::drivers::DriverError;
use serde_json::{json, Value};

#[derive(Debug, thiserror::Error)]
pub enum AirzoneError {
    #[error("Airzone WebServer unreachable at {0}: {1}")]
    Unreachable(String, String),
    #[error("Airzone API error: {0}")]
    Api(String),
    #[error("invalid Airzone response: {0}")]
    InvalidResponse(String),
}

/// Client for one Airzone WebServer, through the module that speaks its API.
#[derive(Clone)]
pub struct AirzoneClient {
    host: String,
    port: u16,
}

impl AirzoneClient {
    pub fn new(host: &str, port: u16) -> Self {
        Self { host: host.to_string(), port }
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Reads every zone of every system.
    pub async fn read_all(&self) -> Result<HvacResponse, AirzoneError> {
        let v = self.ask("read", json!({})).await?;
        serde_json::from_value(v).map_err(|e| AirzoneError::InvalidResponse(e.to_string()))
    }

    /// Writes parameters on a zone; `params` must contain the fields to change
    /// (e.g. {"on":1}, {"setpoint":21.5}, {"mode":3}). Returns the updated zones.
    pub async fn write(&self, system_id: i64, zone_id: i64, params: &serde_json::Map<String, Value>) -> Result<HvacResponse, AirzoneError> {
        let v = self.ask("write", json!({"system_id": system_id, "zone_id": zone_id, "params": params})).await?;
        serde_json::from_value(v).map_err(|e| AirzoneError::InvalidResponse(e.to_string()))
    }

    /// WebServer information (MAC, firmware, Wi-Fi). Used for discovery and tests.
    pub async fn webserver_info(&self) -> Result<Value, AirzoneError> {
        self.ask("info", json!({})).await
    }

    /// A query of `airzone.webserver@1` at this WebServer: its answer as the WebServer gave it.
    async fn ask(&self, query: &str, mut params: Value) -> Result<Value, AirzoneError> {
        params["host"] = json!(self.host);
        params["port"] = json!(self.port);
        let addr = format!("{}:{}", self.host, self.port);
        match crate::modules::ask_internal("airzone.webserver@1", query, &params).await {
            Ok(mut v) => Ok(v["answer"].take()),
            Err(DriverError::Unreachable(m)) | Err(DriverError::Unsupported(m)) => Err(AirzoneError::Unreachable(addr, m)),
            Err(DriverError::Invalid(m)) => Err(match m.strip_prefix("api: ") {
                Some(errors) => AirzoneError::Api(errors.to_string()),
                None => AirzoneError::InvalidResponse(m),
            }),
        }
    }
}
