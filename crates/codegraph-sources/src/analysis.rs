//! One file's analysis: symbols, references, architecture and waypoint facts, with the
//! bounds Python puts on how large a stored analysis may be.

use crate::error::{Result, SourceError};
use crate::identity::{git_blob_oid, sha256_hex};
use codegraph_engine::architecture::decode_utf8;
use codegraph_engine::architecture::extract::extract_facts;
use codegraph_engine::parsing::symbols;
use codegraph_engine::relationships::references;
use codegraph_engine::waypoints::{build_file_facts, symbols_as_values};
use codegraph_store::json_text::dumps_ordered;
use codegraph_store::{Analysis, MAX_ANALYSIS_BYTES};
use serde_json::Value;

const MAX_STRING_BYTES: usize = 8192;

/// A stored analysis, in memory.
#[derive(Debug)]
pub struct Record {
    pub id: String,
    pub content_hash: String,
    pub git_oid: String,
    pub language: String,
    pub version: String,
    pub size_bytes: i64,
    pub symbols: Vec<Value>,
    pub facts: Value,
    pub encoded_bytes: i64,
}

/// A fresh analysis and the exact text that goes into the `analyses` row.
pub struct Fresh {
    pub record: Record,
    pub encoded_symbols: String,
    pub encoded_facts: String,
}

fn too_large() -> SourceError {
    SourceError::value("Analysis exceeds encoded output limit")
}

/// A conservative bound on the encoded size, so a hostile identifier cannot expand before
/// it is measured: six bytes per byte of text, 32 for anything else.
fn analysis_size(value: &Value) -> Result<i64> {
    match value {
        Value::String(text) => {
            if text.len() > MAX_STRING_BYTES {
                return Err(SourceError::value(
                    "Analysis identifier/reference exceeds string limit",
                ));
            }
            Ok(2 + 6 * i64::try_from(text.len()).unwrap_or(i64::MAX))
        }
        Value::Array(items) => {
            let mut size = 2;
            for item in items {
                size += analysis_size(item)? + 2;
                if size > MAX_ANALYSIS_BYTES {
                    return Err(too_large());
                }
            }
            Ok(size)
        }
        // A mapping counts its keys and values as one flat list of members.
        Value::Object(map) => {
            let mut size = 2;
            for (key, item) in map {
                size += analysis_size(&Value::String(key.clone()))? + 2;
                if size > MAX_ANALYSIS_BYTES {
                    return Err(too_large());
                }
                size += analysis_size(item)? + 2;
                if size > MAX_ANALYSIS_BYTES {
                    return Err(too_large());
                }
            }
            Ok(size)
        }
        _ => Ok(32),
    }
}

fn insert(facts: &mut Value, key: &str, value: Value) {
    if let Some(map) = facts.as_object_mut() {
        map.insert(key.to_owned(), value);
    }
}

/// Parse and describe one file. Nothing here runs the code it reads.
pub fn analyse(language: &str, data: &[u8], signature: &str, key: &str) -> Result<Fresh> {
    let content_hash = sha256_hex(data);
    let entries = symbols(language, data)?;
    let text = decode_utf8(data)?;
    let found = references(Some(language), &text)?;
    let mut facts = serde_json::to_value(&found)
        .map_err(|error| SourceError::new("TypeError", error.to_string()))?;
    let architecture = extract_facts(language, data, &facts)?;
    insert(&mut facts, "architecture", architecture);
    let rows = symbols_as_values(&entries);
    let waypoint = build_file_facts(language, data, &rows, &facts)?;
    insert(&mut facts, "waypoint", waypoint);
    analysis_size(&Value::Array(vec![
        Value::Array(rows.clone()),
        facts.clone(),
    ]))?;
    let encoded_symbols = dumps_ordered(&Value::Array(rows.clone()));
    let encoded_facts = dumps_ordered(&facts);
    let encoded_bytes =
        i64::try_from(encoded_symbols.len() + encoded_facts.len()).unwrap_or(i64::MAX);
    Ok(Fresh {
        record: Record {
            id: key.to_owned(),
            content_hash,
            git_oid: git_blob_oid(data),
            language: language.to_owned(),
            version: signature.to_owned(),
            size_bytes: i64::try_from(data.len()).unwrap_or(i64::MAX),
            symbols: rows,
            facts,
            encoded_bytes,
        },
        encoded_symbols,
        encoded_facts,
    })
}

/// A stored row as a record. A number JSON cannot carry exactly has no `serde_json` form,
/// so such a row is refused where Python would have served it.
pub fn from_stored(stored: Analysis) -> Result<Record> {
    let range = || SourceError::value("Stored analysis holds a number outside the supported range");
    let symbols = stored.symbols.to_value().ok_or_else(range)?;
    let facts = stored.facts.to_value().ok_or_else(range)?;
    let Value::Array(symbols) = symbols else {
        return Err(SourceError::new(
            "TypeError",
            "Stored symbols are not a list",
        ));
    };
    if !facts.is_object() {
        return Err(SourceError::new(
            "TypeError",
            "Stored facts are not a mapping",
        ));
    }
    Ok(Record {
        id: stored.id,
        content_hash: stored.content_hash,
        git_oid: stored.git_oid,
        language: stored.language,
        version: stored.version,
        size_bytes: stored.size_bytes,
        symbols,
        facts,
        encoded_bytes: stored.encoded_bytes,
    })
}
