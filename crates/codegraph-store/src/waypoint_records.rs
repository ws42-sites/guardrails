//! Read side of the waypoint cache: descriptions and checks are stored once by content
//! and rebound to a snapshot's revision when they are read.

use crate::error::{Result, StoreError};
use codegraph_engine::py_json::{self, JsonFailure};
use codegraph_engine::waypoints::{self, parse_waypoint};
use rusqlite::types::Value as Stored;
use rusqlite::{Connection, OptionalExtension};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

const MAX_PAYLOAD_BYTES: usize = 65_536;
const MAX_RECORDS: usize = 30_000;

fn python_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(number) if number.is_f64() => "float",
        Value::Number(_) => "int",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

fn hex_digest(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// A cached record, or `None` when nothing is stored under `key`. A record whose kind,
/// version, size or checksum is off is refused rather than served.
pub(crate) fn cached(
    connection: &Connection,
    key: &str,
    kind: &str,
) -> Result<Option<Map<String, Value>>> {
    let row = connection
        .query_row(
            "SELECT kind,version,payload,payload_hash FROM waypoint_cache WHERE id=?",
            [key],
            |row| {
                Ok((
                    row.get::<_, Stored>(0)?,
                    row.get::<_, Stored>(1)?,
                    row.get::<_, Stored>(2)?,
                    row.get::<_, Stored>(3)?,
                ))
            },
        )
        .optional()?;
    let Some((stored_kind, version, payload, checksum)) = row else {
        return Ok(None);
    };
    let expected_version = if kind == "validation" {
        waypoints::VALIDATOR_VERSION
    } else {
        waypoints::VERSION
    };
    let invalid = || StoreError::value("Cached waypoint identity is invalid");
    let Stored::Text(payload) = payload else {
        return Err(invalid());
    };
    if stored_kind != Stored::Text(kind.to_owned())
        || version != Stored::Text(expected_version.to_owned())
        || payload.len() > MAX_PAYLOAD_BYTES
        || checksum != Stored::Text(hex_digest(&payload))
    {
        return Err(invalid());
    }
    let decoded = py_json::loads_decoded(&payload, None).map_err(|failure| match failure {
        JsonFailure::Decode(message) => StoreError::new("JSONDecodeError", message),
        JsonFailure::Value(message) => StoreError::value(&message),
        JsonFailure::Recursion => StoreError::new("RecursionError", "maximum recursion depth"),
    })?;
    // A number JSON cannot carry exactly (huge integer, NaN) has no `serde_json` form.
    let Some(value) = decoded.to_value() else {
        return Err(StoreError::value(
            "Cached waypoint holds a number outside the supported range",
        ));
    };
    match value {
        Value::Object(map) => Ok(Some(map)),
        _ => Err(StoreError::new(
            "TypeError",
            "Cached waypoint has an invalid shape",
        )),
    }
}

/// Evidence is stamped with the snapshot's revision; items that are not mappings fail as
/// Python's `{**item}` unpacking does.
fn bound_evidence(summary: &Map<String, Value>, revision: &str) -> Result<Value> {
    let items: Vec<Value> = match summary.get("evidence") {
        None => Vec::new(),
        Some(Value::Array(items)) => items.clone(),
        Some(Value::Object(map)) => map.keys().cloned().map(Value::String).collect(),
        Some(Value::String(text)) => text.chars().map(|c| Value::String(c.to_string())).collect(),
        Some(other) => {
            return Err(StoreError::new(
                "TypeError",
                format!("'{}' object is not iterable", python_type(other)),
            ));
        }
    };
    let mut bound = Vec::new();
    for item in items {
        let Value::Object(mut map) = item else {
            return Err(StoreError::new(
                "TypeError",
                format!("'{}' object is not a mapping", python_type(&item)),
            ));
        };
        map.insert("revision".into(), Value::String(revision.to_owned()));
        bound.push(Value::Object(map));
    }
    Ok(Value::Array(bound))
}

pub(crate) fn public(
    summary: Map<String, Value>,
    validation: Map<String, Value>,
    path: &str,
    revision: &str,
) -> Result<Value> {
    let evidence = bound_evidence(&summary, revision)?;
    let mut value = summary;
    value.insert("evidence".into(), evidence);
    value.insert("validation".into(), Value::Object(validation));
    parse_waypoint(&Value::Object(value), path, revision)
        .map_err(|error| StoreError::new(error.kind, error.message))
}

fn invalid_path(path: &str) -> bool {
    path.starts_with('/')
        || path.contains('\\')
        || path.contains(':')
        || path.split('/').any(|part| part.is_empty() || part == "..")
        || path.chars().any(|character| u32::from(character) < 32)
}

/// Every waypoint bound to a snapshot, optionally limited to one path and what is under it.
pub fn records(
    connection: &Connection,
    snapshot_id: i64,
    path: Option<&str>,
) -> Result<Vec<Value>> {
    if snapshot_id < 1 {
        return Err(StoreError::value("Invalid waypoint snapshot"));
    }
    if path.is_some_and(invalid_path) {
        return Err(StoreError::value("Invalid waypoint path"));
    }
    let mut statement = connection.prepare(
        "SELECT w.path,w.kind,w.summary_key,w.validation_key,s.revision \
         FROM snapshot_waypoints w JOIN snapshots s ON s.id=w.snapshot_id \
         WHERE w.snapshot_id=? ORDER BY w.path LIMIT 30001",
    )?;
    let rows = statement
        .query_map([snapshot_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if rows.len() > MAX_RECORDS {
        return Err(StoreError::value("Waypoint snapshot exceeds its bound"));
    }
    let mut result = Vec::new();
    for (source_path, kind, summary_key, validation_key, revision) in rows {
        if let Some(path) = path.filter(|path| *path != ".")
            && source_path != path
            && !source_path.starts_with(&format!("{path}/"))
        {
            continue;
        }
        let summary = cached(connection, &summary_key, &kind)?;
        let validation = cached(connection, &validation_key, "validation")?;
        let (Some(summary), Some(validation)) = (summary, validation) else {
            return Err(StoreError::value("Snapshot waypoint binding is incomplete"));
        };
        result.push(public(summary, validation, &source_path, &revision)?);
    }
    Ok(result)
}
