//! Reads over the shared analysis cache.

use crate::error::{Result, StoreError};
use crate::graph_schema::MAX_ANALYSIS_BYTES;
use ropeline_engine::py_json::{self, JsonFailure};
use ropeline_engine::py_value::PyValue;
use rusqlite::types::Value as Stored;
use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};

/// One cached file analysis, with `symbols` and `facts` decoded as Python's `json` would.
#[derive(Debug, Clone, PartialEq)]
pub struct Analysis {
    pub id: String,
    pub content_hash: String,
    pub git_oid: String,
    pub language: String,
    pub version: String,
    pub size_bytes: i64,
    pub symbols: PyValue,
    pub facts: PyValue,
    pub encoded_bytes: i64,
}

impl Analysis {
    /// The record as Python returns it. `None` only for a number JSON cannot carry.
    pub fn to_json(&self) -> Option<Value> {
        Some(json!({
            "id": self.id,
            "content_hash": self.content_hash,
            "git_oid": self.git_oid,
            "language": self.language,
            "version": self.version,
            "size_bytes": self.size_bytes,
            "symbols": self.symbols.to_value()?,
            "facts": self.facts.to_value()?,
            "encoded_bytes": self.encoded_bytes,
        }))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    pub unique_analyses: i64,
    pub unique_source_bytes: i64,
    pub stored_symbols: i64,
    pub manifest_entries: i64,
}

/// `json.loads` on a stored column. The CHECK constraint keeps text columns valid, so
/// a failure here means someone bypassed it.
fn decode(value: &Stored) -> Result<PyValue> {
    let text = match value {
        Stored::Text(text) => text,
        Stored::Integer(_) => {
            return Err(StoreError::new(
                "TypeError",
                "the JSON object must be str, bytes or bytearray, not int",
            ));
        }
        Stored::Real(_) => {
            return Err(StoreError::new(
                "TypeError",
                "the JSON object must be str, bytes or bytearray, not float",
            ));
        }
        _ => return Err(StoreError::value("Stored analysis is not text")),
    };
    py_json::loads_decoded(text, None).map_err(|failure| match failure {
        JsonFailure::Decode(message) => StoreError::new("JSONDecodeError", message),
        JsonFailure::Value(message) => StoreError::value(&message),
        JsonFailure::Recursion => StoreError::new("RecursionError", "maximum recursion depth"),
    })
}

pub fn analysis(connection: &Connection, key: &str) -> Result<Option<Analysis>> {
    let row = connection
        .query_row(
            "SELECT *,length(symbols)+length(facts) AS encoded_bytes FROM analyses WHERE id=?",
            [key],
            |row| {
                Ok((
                    Analysis {
                        id: row.get("id")?,
                        content_hash: row.get("content_hash")?,
                        git_oid: row.get("git_oid")?,
                        language: row.get("language")?,
                        version: row.get("version")?,
                        size_bytes: row.get("size_bytes")?,
                        symbols: PyValue::Null,
                        facts: PyValue::Null,
                        encoded_bytes: row.get("encoded_bytes")?,
                    },
                    row.get::<_, Stored>("symbols")?,
                    row.get::<_, Stored>("facts")?,
                ))
            },
        )
        .optional()?;
    let Some((mut record, symbols, facts)) = row else {
        return Ok(None);
    };
    // The size is judged before any JSON is decoded, so a hostile row costs nothing.
    if record.encoded_bytes > MAX_ANALYSIS_BYTES {
        return Err(StoreError::value("Cached analysis exceeds output limit"));
    }
    record.symbols = decode(&symbols)?;
    record.facts = decode(&facts)?;
    Ok(Some(record))
}

pub fn summary(connection: &Connection, repository: &str) -> Result<Summary> {
    let (unique_analyses, unique_source_bytes, stored_symbols) = connection.query_row(
        "SELECT count(*),coalesce(sum(size_bytes),0),coalesce(sum(json_array_length(symbols)),0)
        FROM analyses WHERE id IN (SELECT f.analysis_id FROM snapshot_files f JOIN snapshots s ON s.id=f.snapshot_id WHERE s.repo=?)",
        [repository],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    let manifest_entries = connection.query_row(
        "SELECT count(*) FROM snapshot_files f JOIN snapshots s ON s.id=f.snapshot_id WHERE s.repo=?",
        [repository],
        |row| row.get(0),
    )?;
    Ok(Summary {
        unique_analyses,
        unique_source_bytes,
        stored_symbols,
        manifest_entries,
    })
}
