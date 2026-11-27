//! The evidence, finding and match models of `architecture_contract.py`.
//!
//! Each `validate_*` takes a decoded value, checks it as the strict pydantic model
//! does and returns `model_dump()`.

use super::PyError;
use super::contract::{MAX_FILES, ORIGINS, RULE_KINDS, SEVERITIES, is_safe_path};
use super::validate::{
    ANY_TEXT, IDENTIFIER, SHA256, StrRule, Validator, optional, required, text, text_max,
};
use crate::py_value::PyValue;
use serde_json::{Map, Value, json};

const SAFE_PATH_MESSAGE: &str = "Invalid architecture source path";

/// A string field that `safe_path` then checks (a field validator, so it runs only on valid text).
pub fn safe_path_field(v: &mut Validator, input: &PyValue) -> Option<String> {
    let value = v.str_field(input, ANY_TEXT)?;
    if !is_safe_path(&value, false) {
        v.value_error(SAFE_PATH_MESSAGE);
        return None;
    }
    Some(value)
}

fn nullable_safe_path(v: &mut Validator, input: &PyValue) -> Option<Value> {
    if matches!(input, PyValue::Null) {
        return Some(Value::Null);
    }
    safe_path_field(v, input).map(Value::String)
}

pub fn validate_evidence_in(v: &mut Validator, input: &PyValue) -> Option<Value> {
    let specs = [
        required("repository"),
        required("revision"),
        required("path"),
        optional("line", Value::Null),
        required("method"),
        optional("sha256", Value::Null),
    ];
    let dumped = v.model("Evidence", input, &specs, |v, name, value| match name {
        "repository" => v.str_field(value, text(1, 1024)).map(Value::String),
        "revision" => v.str_field(value, text(1, 256)).map(Value::String),
        "path" => safe_path_field(v, value).map(Value::String),
        "line" => v.nullable_int(value, Some(1), None),
        "method" => v.str_field(value, text(1, 128)).map(Value::String),
        _ => v.nullable_str(value, SHA256),
    })?;
    Some(Value::Object(dumped))
}

fn evidence_list(
    v: &mut Validator,
    input: &PyValue,
    min: Option<usize>,
    max: usize,
) -> Option<Vec<Value>> {
    v.list_field(input, min, Some(max), validate_evidence_in)
}

fn finish(model: &str, v: Validator, dumped: Option<Value>) -> Result<Value, PyError> {
    match dumped {
        Some(value) if v.issues.is_empty() => Ok(value),
        _ => Err(PyError::validation(model, v.issues)),
    }
}

pub fn validate_evidence(input: &PyValue) -> Result<Value, PyError> {
    let mut v = Validator::new();
    let dumped = validate_evidence_in(&mut v, input);
    finish("Evidence", v, dumped)
}

pub fn validate_classification(input: &PyValue) -> Result<Value, PyError> {
    let mut v = Validator::new();
    let specs = [
        required("dimension"),
        required("label"),
        required("origin"),
        required("confidence"),
        optional("confidence_calibration", json!("not-applicable")),
        optional("model_version", Value::Null),
        optional("review_status", json!("unreviewed")),
        required("evidence"),
    ];
    let dumped = v
        .model(
            "Classification",
            input,
            &specs,
            |v, name, value| match name {
                "dimension" => v.str_field(value, IDENTIFIER).map(Value::String),
                "label" => v.str_field(value, text(1, 256)).map(Value::String),
                "origin" => v.literal(value, &ORIGINS).map(Value::String),
                "confidence" => v.float_field(value, 0.0, 1.0),
                "confidence_calibration" => v
                    .literal(
                        value,
                        &[
                            "not-applicable",
                            "uncalibrated",
                            "uncalibrated-path-signals",
                            "evaluated",
                        ],
                    )
                    .map(Value::String),
                "model_version" => v.nullable_str(value, ANY_TEXT),
                "review_status" => v
                    .literal(
                        value,
                        &["unreviewed", "approved", "rejected", "not-required"],
                    )
                    .map(Value::String),
                _ => evidence_list(v, value, Some(1), 32).map(Value::Array),
            },
        )
        .map(Value::Object);
    finish("Classification", v, dumped)
}

pub fn validate_structural_relationship(input: &PyValue) -> Result<Value, PyError> {
    let mut v = Validator::new();
    let specs = [
        required("kind"),
        required("source"),
        required("target"),
        required("line"),
        required("source_path"),
        required("target_path"),
        required("target_node"),
        required("resolution"),
        required("origin"),
        required("evidence"),
    ];
    let dumped = v
        .model(
            "StructuralRelationship",
            input,
            &specs,
            |v, name, value| match name {
                "kind" => v
                    .literal(
                        value,
                        &[
                            "CALLS",
                            "UNRESOLVED_CALL",
                            "IMPLEMENTS",
                            "UNRESOLVED_IMPLEMENTATION",
                            "EXTENDS",
                            "INJECTS",
                            "CONSTRUCTS",
                        ],
                    )
                    .map(Value::String),
                "source" | "target" => v.str_field(value, text_max(512)).map(Value::String),
                "line" => v
                    .int_field(value, Some(1), None)
                    .map(|int| Value::Number(int.into())),
                "source_path" => safe_path_field(v, value).map(Value::String),
                "target_path" => nullable_safe_path(v, value),
                "target_node" => v.nullable_str(value, ANY_TEXT),
                "resolution" => v.str_field(value, IDENTIFIER).map(Value::String),
                "origin" => v.literal(value, &ORIGINS).map(Value::String),
                _ => validate_evidence_in(v, value),
            },
        )
        .map(Value::Object);
    finish("StructuralRelationship", v, dumped)
}

fn validate_signal(v: &mut Validator, input: &PyValue) -> Option<Value> {
    let specs = [required("signal"), required("evidence")];
    v.model(
        "PatternSignal",
        input,
        &specs,
        |v, name, value| match name {
            "signal" => v.str_field(value, IDENTIFIER).map(Value::String),
            _ => evidence_list(v, value, Some(1), 8).map(Value::Array),
        },
    )
    .map(Value::Object)
}

pub fn validate_pattern_match(input: &PyValue) -> Result<Value, PyError> {
    let mut v = Validator::new();
    let specs = [
        required("pattern_id"),
        required("pattern_library_version"),
        required("scope_id"),
        required("outcome"),
        required("supporting"),
        required("contradictory"),
        required("missing_evidence"),
        required("origin"),
        required("classifier_version"),
        required("model_version"),
        required("confidence_calibration"),
        required("review_status"),
        required("enforcement_authority"),
        required("explanation"),
        required("limitations"),
    ];
    let dumped = v
        .model("PatternMatch", input, &specs, |v, name, value| match name {
            "pattern_id" | "pattern_library_version" | "scope_id" | "classifier_version" => {
                v.str_field(value, IDENTIFIER).map(Value::String)
            }
            "outcome" => v
                .literal(value, &["supported", "contradicted", "inconclusive"])
                .map(Value::String),
            "supporting" | "contradictory" => v
                .list_field(value, None, Some(24), validate_signal)
                .map(Value::Array),
            "missing_evidence" => v
                .str_list(value, None, Some(32), IDENTIFIER)
                .map(Value::Array),
            "origin" => v.literal(value, &["inferred"]).map(Value::String),
            "model_version" => v.nullable_str(value, ANY_TEXT),
            "confidence_calibration" => v.literal(value, &["uncalibrated"]).map(Value::String),
            "review_status" => v.literal(value, &["unreviewed"]).map(Value::String),
            "enforcement_authority" => match value {
                // `Literal[False]` compares by value, so 0 and 0.0 are accepted too.
                PyValue::Bool(false) | PyValue::Int(0) => Some(Value::Bool(false)),
                PyValue::Float(float) if *float == 0.0 => Some(Value::Bool(false)),
                _ => {
                    v.issue("literal_error", "Input should be False");
                    None
                }
            },
            "explanation" => v.str_field(value, text_max(2000)).map(Value::String),
            _ => v
                .str_list(value, None, Some(16), ANY_TEXT)
                .map(Value::Array),
        })
        .map(Value::Object);
    finish("PatternMatch", v, dumped)
}

fn validate_baseline(v: &mut Validator, input: &PyValue) -> Option<Value> {
    let specs = [
        required("revision"),
        required("status"),
        required("previous_measure"),
    ];
    v.model(
        "BaselineComparison",
        input,
        &specs,
        |v, name, value| match name {
            "revision" => v.nullable_str(value, ANY_TEXT),
            "status" => v
                .literal(value, &["new", "existing", "worsened", "uncompared"])
                .map(Value::String),
            _ => v.nullable_int(value, None, None),
        },
    )
    .map(Value::Object)
}

fn validate_suppression(v: &mut Validator, input: &PyValue) -> Option<Value> {
    let specs = [
        required("status"),
        optional("exception_id", Value::Null),
        optional("reason", Value::Null),
        optional("expires", Value::Null),
    ];
    v.model("Suppression", input, &specs, |v, name, value| match name {
        "status" => v
            .literal(value, &["none", "active", "expired", "requested"])
            .map(Value::String),
        _ => v.nullable_str(value, ANY_TEXT),
    })
    .map(Value::Object)
}

pub fn validate_finding(input: &PyValue) -> Result<Value, PyError> {
    let mut v = Validator::new();
    let specs = [
        required("id"),
        required("rule_id"),
        required("rule_kind"),
        required("repository"),
        required("revision"),
        required("requested_severity"),
        required("severity"),
        required("origin"),
        required("confidence"),
        required("paths"),
        required("target"),
        required("measure"),
        required("explanation"),
        required("remediation"),
        required("affected_nodes"),
        required("affected_edges"),
        required("evidence"),
        required("baseline"),
        required("policy_digest"),
        required("policy_approved"),
        required("suppression"),
    ];
    let dumped = v
        .model(
            "ArchitectureFinding",
            input,
            &specs,
            |v, name, value| match name {
                "id" | "policy_digest" => v.str_field(value, SHA256).map(Value::String),
                "rule_id" => v.str_field(value, IDENTIFIER).map(Value::String),
                "rule_kind" => v.literal(value, &RULE_KINDS).map(Value::String),
                "repository" => v.str_field(value, text(1, 1024)).map(Value::String),
                "revision" => v.str_field(value, text(1, 256)).map(Value::String),
                "requested_severity" | "severity" => {
                    v.literal(value, &SEVERITIES).map(Value::String)
                }
                "origin" => v.literal(value, &["observed"]).map(Value::String),
                "confidence" => v.float_field(value, 0.0, 1.0),
                "paths" => {
                    let paths = v.str_list(value, Some(1), Some(MAX_FILES), ANY_TEXT)?;
                    if paths
                        .iter()
                        .any(|item| !is_safe_path(item.as_str().unwrap_or_default(), false))
                    {
                        v.value_error(SAFE_PATH_MESSAGE);
                        return None;
                    }
                    Some(Value::Array(paths))
                }
                "target" | "explanation" | "remediation" => {
                    v.str_field(value, ANY_TEXT).map(Value::String)
                }
                "measure" => v
                    .int_field(value, Some(1), None)
                    .map(|int| Value::Number(int.into())),
                "affected_nodes" => v.str_list(value, None, None, ANY_TEXT).map(Value::Array),
                "affected_edges" => v
                    .list_field(value, None, None, |v, edge| {
                        let PyValue::Dict(entries) = edge else {
                            v.issue("dict_type", "Input should be a valid dictionary");
                            return None;
                        };
                        let mark = v.issues.len();
                        let mut map = Map::new();
                        for (key, item) in entries {
                            if let Some(done) = v.field(key, |v| {
                                v.str_field(item, StrRule::default()).map(Value::String)
                            }) {
                                map.insert(key.clone(), done);
                            }
                        }
                        v.is_clean_since(mark).then_some(Value::Object(map))
                    })
                    .map(Value::Array),
                "evidence" => evidence_list(v, value, Some(1), MAX_FILES).map(Value::Array),
                "baseline" => validate_baseline(v, value),
                "policy_approved" => v.bool_field(value).map(Value::Bool),
                _ => validate_suppression(v, value),
            },
        )
        .map(Value::Object);
    finish("ArchitectureFinding", v, dumped)
}
