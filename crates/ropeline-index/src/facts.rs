//! Stored facts back into the typed form the relationship resolver takes.
//!
//! A fresh analysis and a cached one go through the same conversion, so a row written by
//! Python (or by an earlier run) is read exactly as one just made.

use crate::error::{Result, SourceError};
use ropeline_engine::relationships::{CallFact, ImportFact, References};
use ropeline_engine::relationships_rust::RustUse;
use serde_json::Value;

fn missing(key: &str) -> SourceError {
    SourceError::new("KeyError", format!("'{key}'"))
}

fn field<'a>(item: &'a Value, key: &str) -> Result<&'a Value> {
    item.get(key).ok_or_else(|| missing(key))
}

fn text_of(value: &Value) -> Result<String> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| SourceError::new("TypeError", "expected a string"))
}

fn count_of(value: &Value) -> Result<usize> {
    value
        .as_u64()
        .and_then(|number| usize::try_from(number).ok())
        .ok_or_else(|| SourceError::new("TypeError", "expected a non-negative integer"))
}

fn list_of(value: &Value) -> Result<&Vec<Value>> {
    value
        .as_array()
        .ok_or_else(|| SourceError::new("TypeError", "expected a list"))
}

fn names_of(value: &Value) -> Result<Vec<String>> {
    list_of(value)?.iter().map(text_of).collect()
}

fn import_of(item: &Value) -> Result<Option<ImportFact>> {
    let line = count_of(field(item, "line")?)?;
    let kind = field(item, "kind")?.as_str().unwrap_or_default();
    Ok(Some(match kind {
        "relative" => ImportFact::Relative {
            specifier: text_of(field(item, "specifier")?)?,
            line,
            type_only: item.get("type_only").is_some_and(truthy),
        },
        "rust_use" => ImportFact::RustUse(RustUse {
            path: names_of(field(item, "path")?)?,
            line,
            depth: count_of(field(item, "depth")?)?,
            external: item.get("extern").is_some_and(truthy),
        }),
        "python_from" => ImportFact::PythonFrom {
            level: u32::try_from(count_of(field(item, "level")?)?)
                .map_err(|_| SourceError::new("TypeError", "expected a small integer"))?,
            module: match field(item, "module")? {
                Value::Null => None,
                other => Some(text_of(other)?),
            },
            names: names_of(field(item, "names")?)?,
            line,
        },
        "python_import" => ImportFact::PythonImport {
            names: names_of(field(item, "names")?)?,
            line,
        },
        // A kind this resolver does not know matches no branch and is ignored.
        _ => return Ok(None),
    }))
}

/// The only basis a call candidate has ever carried; any other is refused rather than
/// served as if it were confirmed.
fn basis_of(value: &Value) -> Result<&'static str> {
    match value.as_str() {
        Some("local-name-candidate") => Ok("local-name-candidate"),
        _ => Err(SourceError::value("Unsupported call basis")),
    }
}

fn call_of(item: &Value) -> Result<CallFact> {
    let parts = list_of(item)?;
    if parts.len() != 3 {
        return Err(SourceError::new("TypeError", "expected a call candidate"));
    }
    Ok(CallFact(
        text_of(&parts[0])?,
        count_of(&parts[1])?,
        basis_of(&parts[2])?,
    ))
}

/// `facts["imports"]` and `facts["calls"]`, in that order, as Python reads them.
pub fn references_of(facts: &Value) -> Result<References> {
    let mut imports = Vec::new();
    for item in list_of(field(facts, "imports")?)? {
        if let Some(fact) = import_of(item)? {
            imports.push(fact);
        }
    }
    let calls = list_of(field(facts, "calls")?)?
        .iter()
        .map(call_of)
        .collect::<Result<Vec<_>>>()?;
    Ok(References { imports, calls })
}

/// Python truthiness of a stored value.
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64() != Some(0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// Whether any import fact written on `line` is type-only (`import type ...`).
pub fn type_only_on_line(facts: &Value, line: usize) -> bool {
    facts["imports"].as_array().is_some_and(|items| {
        items.iter().any(|item| {
            item.get("line").and_then(Value::as_u64) == Some(line as u64)
                && item.get("type_only").is_some_and(truthy)
        })
    })
}
