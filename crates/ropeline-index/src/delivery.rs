//! Bounded projection of the shared, cached waypoints for agents and the explorer.
//!
//! A waypoint is stored once by content and bound to a snapshot's revision when it is read.
//! These functions take what a node or the repository row stores and hand back records that
//! have passed the public contract, never raw cache text.

use crate::error::{Result, SourceError};
use ropeline_engine::waypoints::parse_waypoint;
use ropeline_store::json_text::{dumps_ordered, loads};
use serde_json::{Map, Value};
use std::collections::HashSet;

const MAX_AREAS: usize = 10_000;

/// The default size of the first screen of area descriptions, in encoded characters.
pub const OVERVIEW_BUDGET: usize = 8_000;

/// Stored metadata is JSON text on a node and a mapping once decoded; anything else is not
/// metadata.
fn metadata_map(value: &Value) -> Result<Map<String, Value>> {
    let decoded;
    let value = match value {
        Value::String(text) => {
            decoded = loads(text)?;
            &decoded
        }
        other => other,
    };
    match value {
        Value::Object(map) => Ok(map.clone()),
        _ => Err(SourceError::new("TypeError", "Invalid waypoint metadata")),
    }
}

/// The file description stored in a node's metadata, checked against its revision.
pub fn file_waypoint(metadata: &Value, path: &str, revision: &str) -> Result<Option<Value>> {
    let metadata = metadata_map(metadata)?;
    let Some(value) = metadata.get("waypoint").filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let record = parse_waypoint(value, path, revision)?;
    if record["kind"] != "file" {
        return Err(SourceError::value("A file waypoint is required"));
    }
    if let Some(hash) = metadata.get("sha256")
        && *hash != record["contentHash"]
    {
        return Err(SourceError::value(
            "Waypoint source content differs from its indexed file",
        ));
    }
    Ok(Some(record))
}

/// Every area description stored on the repository row.
pub fn area_waypoints(metadata: &Value, revision: &str) -> Result<Vec<Value>> {
    let metadata = metadata_map(metadata)?;
    let empty = Value::Array(Vec::new());
    let values = match metadata.get("areaWaypoints").unwrap_or(&empty) {
        Value::Array(values) if values.len() <= MAX_AREAS => values,
        _ => {
            return Err(SourceError::value("Area waypoints exceed the read budget"));
        }
    };
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for value in values {
        if !value.is_object() {
            return Err(SourceError::new("TypeError", "Invalid area waypoint"));
        }
        let path = value
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let record = parse_waypoint(value, path, revision)?;
        let key = record["path"].as_str().unwrap_or_default().to_owned();
        if record["kind"] != "area" || !seen.insert(key) {
            return Err(SourceError::value("Invalid or duplicated area waypoint"));
        }
        result.push(record);
    }
    Ok(result)
}

/// The top-level area descriptions that fit `budget`; exact file descriptions travel with
/// search, so the first map of a repository stays compact.
pub fn overview_waypoints(
    metadata: &Value,
    revision: &str,
    budget: usize,
) -> Result<(Vec<Value>, bool)> {
    let mut values = area_waypoints(metadata, revision)?;
    values.sort_by(|a, b| {
        a["path"]
            .as_str()
            .unwrap_or_default()
            .cmp(b["path"].as_str().unwrap_or_default())
    });
    values.retain(|item| {
        let path = item["path"].as_str().unwrap_or_default();
        path == "." || !path.contains('/')
    });
    let mut result: Vec<Value> = Vec::new();
    for value in &values {
        let mut trial = result.clone();
        trial.push(value.clone());
        if dumps_ordered(&Value::Array(trial)).len() > budget {
            break;
        }
        result.push(value.clone());
    }
    let partial = result.len() < values.len();
    Ok((result, partial))
}
