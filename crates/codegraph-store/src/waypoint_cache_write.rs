//! Writing records into the waypoint cache and the snapshot bindings.

use crate::error::{Result, StoreError};
use codegraph_engine::text;
use codegraph_engine::waypoints::{self, WaypointError, digest};
use rusqlite::Connection;
use serde_json::{Map, Value, json};

const MAX_PAYLOAD_BYTES: usize = 65_536;

pub(crate) fn waypoint_error(error: WaypointError) -> StoreError {
    StoreError::new(error.kind, error.message)
}

fn digest_text(text: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

fn encoded(value: &Value) -> Result<String> {
    let text = text::python_json(value);
    if text.len() > MAX_PAYLOAD_BYTES {
        return Err(StoreError::value("Waypoint cache record exceeds its bound"));
    }
    Ok(text)
}

pub(crate) fn store(connection: &Connection, key: &str, kind: &str, payload: &Value) -> Result<()> {
    let text = encoded(payload)?;
    let version = if kind == "validation" {
        waypoints::VALIDATOR_VERSION
    } else {
        waypoints::VERSION
    };
    connection.execute(
        "INSERT INTO waypoint_cache(id,kind,version,payload,payload_hash) VALUES (?,?,?,?,?)",
        (key, kind, version, &text, digest_text(&text)),
    )?;
    Ok(())
}

pub(crate) fn bind(
    connection: &Connection,
    snapshot_id: i64,
    path: &str,
    kind: &str,
    summary_key: &str,
    validation_key: &str,
) -> Result<()> {
    connection.execute(
        "INSERT INTO snapshot_waypoints(snapshot_id,path,kind,summary_key,validation_key) VALUES (?,?,?,?,?)",
        (snapshot_id, path, kind, summary_key, validation_key),
    )?;
    Ok(())
}

/// `{**validation, ...}` with the status and fingerprint replaced and one more check.
pub(crate) fn incomplete(
    validation: &Value,
    identities: &Value,
    name: &str,
    detail: &str,
) -> Value {
    let mut result = validation.as_object().cloned().unwrap_or_default();
    let before = validation["fingerprint"].clone();
    result.insert("status".into(), json!("inconclusive"));
    result.insert(
        "fingerprint".into(),
        json!(digest(&json!([
            before,
            "unavailable-waypoint-paths-v1",
            identities
        ]))),
    );
    let mut checks = validation["checks"].as_array().cloned().unwrap_or_default();
    checks.push(json!({"name": name, "status": "inconclusive", "detail": detail}));
    result.insert("checks".into(), Value::Array(checks));
    Value::Object(result)
}

pub(crate) fn as_map(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}
