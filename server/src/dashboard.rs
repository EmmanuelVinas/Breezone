//! The overview of an installation: blocks the user arranges (energy figures, rooms, devices,
//! charts…). An administrator of the installation sets the one everyone sees; each user may make
//! their own, for them only. Without either, the clients compose one from the rooms and devices.
//!
//! The server keeps the layout and checks its shape; what a block shows is the clients' (a block of
//! a type a client does not know is skipped by it).

use crate::error::{ApiError, ApiResult};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const VERSION: u32 = 1;
const MAX_BLOCKS: usize = 60;

/// The blocks of this version.
pub const KINDS: &[&str] = &["energy", "surplus", "group", "device", "chart", "weather", "scenes"];

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
        if !KINDS.contains(&b.kind.as_str()) {
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
        assert!(validate(&layout(json!([{"id": "a", "type": "clock", "size": "half"}]))).is_err(), "unknown type");
        assert!(validate(&layout(json!([{"id": "a", "type": "weather", "size": "huge"}]))).is_err(), "unknown size");
        assert!(validate(&Layout { version: 2, blocks: vec![] }).is_err());
    }
}
