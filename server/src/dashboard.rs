//! The pages of an installation as dashboards: blocks the user arranges (energy figures, rooms,
//! devices, charts, the parts of a page…). An administrator of the installation sets the layout
//! everyone sees on a page; each user may make their own, for them only. Without either, the
//! clients compose one.
//!
//! The server keeps the layout and checks its shape; what a block shows is the clients' (a block of
//! a type a client does not know is skipped by it).

use crate::error::{ApiError, ApiResult};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const VERSION: u32 = 1;
const MAX_BLOCKS: usize = 60;

/// The pages that are dashboards: the web's, "mobile", the home of the mobile app, and the pages
/// of the app (`mobile_*`), each arranged there apart from the web's.
pub const PAGES: &[&str] = &[
    "overview",
    "energy",
    "climate",
    "lights",
    "ventilation",
    "charging",
    "shutters",
    "solar",
    "mobile",
    "mobile_energy",
    "mobile_solar",
    "mobile_charging",
    "mobile_lights",
    "mobile_shutters",
    "mobile_ventilation",
    "mobile_climate",
];

/// A type of block: lower case words joined by "_" (the clients define them).
fn kind_ok(kind: &str) -> bool {
    !kind.is_empty() && kind.len() <= 40 && kind.starts_with(|c: char| c.is_ascii_lowercase()) && kind.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

pub fn check_page(page: &str) -> ApiResult<()> {
    if PAGES.contains(&page) {
        Ok(())
    } else {
        Err(ApiError::bad_request(format!("unknown page: {page}")))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Layout {
    pub version: u32,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Block {
    /// Stable within the layout (the clients' key while moving blocks).
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    /// "quarter" | "half" | "full": the share of a row on a wide screen.
    pub size: String,
    /// What the block shows (a room, a device and its field…), as its type defines it.
    #[serde(default)]
    pub config: Map<String, Value>,
}

/// Whose layout an overview shows.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// The user's own.
    Mine,
    /// The one the installation's administrators set.
    Installation,
    /// None: the clients compose it.
    Generated,
}

pub fn validate(layout: &Layout) -> ApiResult<String> {
    if layout.version != VERSION {
        return Err(ApiError::bad_request(format!("layout version {VERSION} expected")));
    }
    if layout.blocks.len() > MAX_BLOCKS {
        return Err(ApiError::bad_request(format!("at most {MAX_BLOCKS} blocks")));
    }
    let mut ids = std::collections::HashSet::new();
    for b in &layout.blocks {
        if b.id.trim().is_empty() || b.id.len() > 40 || !ids.insert(b.id.as_str()) {
            return Err(ApiError::bad_request("each block needs an id of its own"));
        }
        if !kind_ok(&b.kind) {
            return Err(ApiError::bad_request(format!("unknown block: {}", b.kind)));
        }
        if !["quarter", "half", "full"].contains(&b.size.as_str()) {
            return Err(ApiError::bad_request("size: quarter, half or full"));
        }
        if serde_json::to_string(&b.config).map_or(0, |s| s.len()) > 4000 {
            return Err(ApiError::bad_request("block settings too long"));
        }
    }
    serde_json::to_string(layout).map_err(|e| ApiError::internal(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn layout(blocks: Value) -> Layout {
        serde_json::from_value(json!({"version": 1, "blocks": blocks})).unwrap()
    }

    #[test]
    fn checks_the_shape_of_a_layout() {
        assert!(validate(&layout(json!([{"id": "a", "type": "energy", "size": "quarter", "config": {"flow": "solar"}}]))).is_ok());
        assert!(validate(&layout(json!([{"id": "a", "type": "energy", "size": "quarter"}, {"id": "a", "type": "weather", "size": "half"}]))).is_err(), "same id");
        assert!(validate(&layout(json!([{"id": "a", "type": "energy_kpis", "size": "full"}]))).is_ok(), "a type of a newer client");
        assert!(validate(&layout(json!([{"id": "a", "type": "Clock!", "size": "half"}]))).is_err(), "not a type");
        assert!(validate(&layout(json!([{"id": "a", "type": "weather", "size": "huge"}]))).is_err(), "unknown size");
        assert!(validate(&Layout { version: 2, blocks: vec![] }).is_err());
    }
}
