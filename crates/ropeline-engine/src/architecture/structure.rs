//! Content-only structural evidence: port of `architecture_structure.py`.
//! No dynamic dispatch is resolved and no repository code is run.

use super::PyError;
use super::structure_python::python_facts;
use super::structure_rust::rust_facts;
use super::structure_script::script_facts;
use serde_json::{Map, Value, json};

pub const VERSION: &str = "structure-1";
const LIMIT: usize = 512;

const LIMITATIONS: [&str; 3] = [
    "Static lexical binding only; reflection, monkey-patching, receiver dispatch and dynamic imports are unresolved.",
    "Typed parameters and construction are composition candidates, not proof of behavioural substitution.",
    "Rust calls, macros and effects are not extracted.",
];

fn empty_result(supported: bool, parse_error: bool) -> Value {
    let mut result = json!({
        "version": VERSION,
        "supported": supported,
        "partial": true,
        "declarations": [],
        "relations": [],
        "effects": [],
    });
    if parse_error {
        result["parse_error"] = Value::Bool(true);
    }
    result
}

fn list(value: &Value, key: &str) -> Vec<Value> {
    value[key].as_array().cloned().unwrap_or_default()
}

/// `extract_structure(language, data)`.
pub fn extract_structure(language: &str, data: &[u8]) -> Result<Value, PyError> {
    if !matches!(
        language,
        "python" | "typescript" | "tsx" | "javascript" | "rust"
    ) {
        return Ok(empty_result(false, false));
    }
    let facts = match language {
        "python" => python_facts(&super::decode_utf8(data)?)?,
        "rust" => Some(rust_facts(data)?),
        _ => Some(script_facts(language, data)?),
    };
    // A syntax error or a tree too deep for CPython leaves nothing to report.
    let Some(mut result) = facts else {
        return Ok(empty_result(true, true));
    };

    // Repeated calls share one architectural relationship. Keep only a bounded sample
    // of unresolved dispatch, rather than storing every test assertion.
    let relations = list(&result, "relations");
    let relation_count = relations.len();
    let is_unresolved = |item: &Value| item["kind"] == "UNRESOLVED_CALL";
    let unresolved_count = relations.iter().filter(|item| is_unresolved(item)).count();
    let mut seen: Vec<(String, String, String)> = Vec::new();
    let mut unique: Vec<Value> = Vec::new();
    for item in relations {
        let key = (
            item["kind"].as_str().unwrap_or_default().to_owned(),
            item["source"].as_str().unwrap_or_default().to_owned(),
            item["target"].as_str().unwrap_or_default().to_owned(),
        );
        if !seen.contains(&key) {
            seen.push(key);
            unique.push(item);
        }
    }
    let confirmed: Vec<Value> = unique
        .iter()
        .filter(|r| !is_unresolved(r))
        .cloned()
        .collect();
    let unresolved: Vec<Value> = unique
        .iter()
        .filter(|r| is_unresolved(r))
        .cloned()
        .collect();
    let mut kept = confirmed.clone();
    kept.extend(unresolved.iter().take(16).cloned());
    let parse_error = result["parse_error"].as_bool().unwrap_or(false);
    let declarations = list(&result, "declarations");
    let effects = list(&result, "effects");

    let object = result.as_object_mut().expect("facts are an object");
    object.insert("relations".into(), Value::Array(kept.clone()));
    object.insert("relation_occurrences".into(), json!(relation_count));
    object.insert(
        "unresolved_call_occurrences".into(),
        json!(unresolved_count),
    );
    object.insert(
        "confirmed_facts_partial".into(),
        json!(
            parse_error
                || confirmed.len() > LIMIT
                || declarations.len() > LIMIT
                || effects.len() > LIMIT
        ),
    );
    object.insert(
        "unresolved_samples_partial".into(),
        json!(unresolved.len() > 16),
    );
    let partial = unresolved.len() > 16
        || parse_error
        || [declarations.len(), kept.len(), effects.len()]
            .iter()
            .any(|length| *length > LIMIT);

    let mut out: Map<String, Value> = object.clone();
    out.insert("version".into(), json!(VERSION));
    out.insert("supported".into(), json!(true));
    out.insert("partial".into(), json!(partial));
    for (key, items) in [
        ("declarations", declarations),
        ("relations", kept),
        ("effects", effects),
    ] {
        out.insert(
            key.into(),
            Value::Array(items.into_iter().take(LIMIT).collect()),
        );
    }
    out.insert("limitations".into(), json!(LIMITATIONS));
    Ok(Value::Object(out))
}
