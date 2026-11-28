//! Versioned, data-only architecture contracts: port of `architecture_contract.py`
//! (the policy half; evidence, finding and match models are in `contract_models`).

use super::PyError;
use super::validate::{
    ANY_TEXT, IDENTIFIER, Loc, Validator, check_iso_date, optional, required, text,
};
use crate::py_json::{self, JsonFailure};
use crate::py_value::PyValue;
use crate::text as pytext;
use regex::Regex;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

pub const VERSION: &str = "1";
pub const ENGINE_VERSION: &str = "architecture-1.1.0";
pub const MAX_FILES: usize = 20_000;
pub const MAX_EDGES: usize = 100_000;
pub const MAX_REPORT_BYTES: usize = 4_000_000;

pub const ORIGINS: [&str; 3] = ["declared", "observed", "inferred"];
pub const SEVERITIES: [&str; 3] = ["block", "review", "inform"];
pub const RULE_KINDS: [&str; 10] = [
    "forbidden-dependency",
    "canonical-authority",
    "cycles",
    "max-module-lines",
    "gate-change",
    "gate-weakening",
    "test-integrity",
    "coupling",
    "orphan-module",
    "duplicate-code",
];

const CONTRACT_BYTES_LIMIT: usize = 262_144;
const SAFE_PATH_MESSAGE: &str = "Invalid architecture source path";
const DUPLICATE_KEY_MESSAGE: &str = "Duplicate architecture contract key";
const STRUCTURAL_LIMITS_MESSAGE: &str = "Architecture contract exceeds structural limits";
const NAMESPACED_MESSAGE: &str = "Ontology extensions require a namespaced identifier";

/// `safe_path`: a repository-relative path with no traversal, drive, control or (for
/// literal paths) glob characters.
pub fn is_safe_path(value: &str, glob: bool) -> bool {
    !(value.is_empty()
        || value.chars().count() > 4096
        || value.starts_with(['/', '\\'])
        || value.contains('\\')
        || value.contains(':')
        || value.chars().any(|c| (c as u32) < 32 || c as u32 == 127)
        || value.split('/').any(|part| matches!(part, "" | "." | ".."))
        || (!glob && value.contains(['*', '?', '[', ']'])))
}

/// `safe_path` as Python raises it.
pub fn safe_path(value: &str, glob: bool) -> Result<&str, PyError> {
    if is_safe_path(value, glob) {
        Ok(value)
    } else {
        Err(PyError::value(SAFE_PATH_MESSAGE))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub id: String,
    pub kind: String,
    pub source: Vec<String>,
    pub target: Vec<String>,
    pub allowed_sources: Vec<String>,
    pub scope: String,
    pub severity: String,
    pub limit: i64,
    pub include_type_only: bool,
    pub reason: String,
    pub remediation: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Domain {
    pub owner: String,
    pub paths: Vec<String>,
    pub public_interfaces: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExceptionPolicy {
    pub id: String,
    pub rule_id: String,
    pub paths: Vec<String>,
    pub reason: String,
    pub expires: String,
}

/// A validated `ArchitectureContract`. `dump` is `model_dump()`.
#[derive(Debug, Clone, PartialEq)]
pub struct Contract {
    pub dump: Value,
    pub styles: Vec<String>,
    pub domains: Vec<(String, Domain)>,
    pub rules: Vec<Rule>,
    pub exceptions: Vec<ExceptionPolicy>,
}

fn strings(value: &Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn string(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or_default().to_owned()
}

impl Rule {
    fn from_dump(value: &Value) -> Self {
        Self {
            id: string(value, "id"),
            kind: string(value, "kind"),
            source: strings(value, "source"),
            target: strings(value, "target"),
            allowed_sources: strings(value, "allowed_sources"),
            scope: string(value, "scope"),
            severity: string(value, "severity"),
            limit: value["limit"].as_i64().unwrap_or(0),
            include_type_only: value["include_type_only"].as_bool().unwrap_or(false),
            reason: string(value, "reason"),
            remediation: string(value, "remediation"),
        }
    }

    /// `model_dump()`.
    pub fn dump(&self) -> Value {
        json!({
            "id": self.id,
            "kind": self.kind,
            "source": self.source,
            "target": self.target,
            "allowed_sources": self.allowed_sources,
            "scope": self.scope,
            "severity": self.severity,
            "limit": self.limit,
            "include_type_only": self.include_type_only,
            "reason": self.reason,
            "remediation": self.remediation,
        })
    }
}

impl Domain {
    pub fn dump(&self) -> Value {
        json!({
            "owner": self.owner,
            "paths": self.paths,
            "public_interfaces": self.public_interfaces,
        })
    }
}

impl Contract {
    fn from_dump(dump: Value) -> Self {
        let rules = dump["rules"]
            .as_array()
            .map(|items| items.iter().map(Rule::from_dump).collect())
            .unwrap_or_default();
        let exceptions = dump["exceptions"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .map(|item| ExceptionPolicy {
                        id: string(item, "id"),
                        rule_id: string(item, "rule_id"),
                        paths: strings(item, "paths"),
                        reason: string(item, "reason"),
                        expires: string(item, "expires"),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let domains = dump["domains"]
            .as_object()
            .map(|map| {
                map.iter()
                    .map(|(name, item)| {
                        (
                            name.clone(),
                            Domain {
                                owner: string(item, "owner"),
                                paths: strings(item, "paths"),
                                public_interfaces: strings(item, "public_interfaces"),
                            },
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            styles: strings(&dump, "styles"),
            domains,
            rules,
            exceptions,
            dump,
        }
    }

    pub fn domain(&self, name: &str) -> Option<&Domain> {
        self.domains
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, domain)| domain)
    }
}

fn glob_list(
    v: &mut Validator,
    input: &PyValue,
    min: Option<usize>,
    max: usize,
) -> Option<Vec<Value>> {
    let items = v.str_list(input, min, Some(max), ANY_TEXT)?;
    if items
        .iter()
        .any(|item| !is_safe_path(item.as_str().unwrap_or_default(), true))
    {
        v.value_error(SAFE_PATH_MESSAGE);
        return None;
    }
    Some(items)
}

fn glob_text(v: &mut Validator, input: &PyValue) -> Option<Value> {
    let value = v.str_field(input, ANY_TEXT)?;
    if !is_safe_path(&value, true) {
        v.value_error(SAFE_PATH_MESSAGE);
        return None;
    }
    Some(Value::String(value))
}

fn validate_rule(v: &mut Validator, input: &PyValue) -> Option<Value> {
    let specs = [
        required("id"),
        required("kind"),
        optional("source", json!(["**"])),
        optional("target", json!([])),
        optional("allowed_sources", json!([])),
        optional("scope", json!("**")),
        optional("severity", json!("review")),
        optional("limit", json!(1000)),
        optional("include_type_only", json!(false)),
        required("reason"),
        required("remediation"),
    ];
    let dumped = v.model("Rule", input, &specs, |v, name, value| match name {
        "id" => v.str_field(value, IDENTIFIER).map(Value::String),
        "kind" => v.literal(value, &RULE_KINDS).map(Value::String),
        "source" => glob_list(v, value, Some(1), 100).map(Value::Array),
        "target" | "allowed_sources" => glob_list(v, value, None, 100).map(Value::Array),
        "scope" => glob_text(v, value),
        "severity" => v.literal(value, &SEVERITIES).map(Value::String),
        "limit" => v
            .int_field(value, Some(1), Some(1_000_000))
            .map(|int| Value::Number(int.into())),
        "include_type_only" => v.bool_field(value).map(Value::Bool),
        _ => v.str_field(value, text(1, 2000)).map(Value::String),
    })?;
    let kind = dumped["kind"].as_str().unwrap_or_default();
    if matches!(kind, "forbidden-dependency" | "canonical-authority")
        && dumped["target"].as_array().is_none_or(Vec::is_empty)
    {
        v.value_error("Dependency policies require explicit targets");
        return None;
    }
    Some(Value::Object(dumped))
}

fn validate_exception(v: &mut Validator, input: &PyValue) -> Option<Value> {
    let specs = [
        required("id"),
        required("rule_id"),
        required("paths"),
        required("reason"),
        required("expires"),
    ];
    let dumped = v.model(
        "ExceptionPolicy",
        input,
        &specs,
        |v, name, value| match name {
            "id" | "rule_id" => v.str_field(value, IDENTIFIER).map(Value::String),
            "paths" => glob_list(v, value, Some(1), 100).map(Value::Array),
            "reason" => v.str_field(value, text(10, 2000)).map(Value::String),
            _ => {
                let day = v.str_field(
                    value,
                    super::validate::StrRule {
                        min: None,
                        max: None,
                        pattern: Some(r"^\d{4}-\d{2}-\d{2}$"),
                    },
                )?;
                match check_iso_date(&day) {
                    Ok(()) => Some(Value::String(day)),
                    Err(message) => {
                        v.value_error(&message);
                        None
                    }
                }
            }
        },
    )?;
    Some(Value::Object(dumped))
}

fn validate_domain(v: &mut Validator, input: &PyValue) -> Option<Value> {
    let specs = [
        required("owner"),
        required("paths"),
        optional("public_interfaces", json!([])),
    ];
    let dumped = v.model("Domain", input, &specs, |v, name, value| match name {
        "owner" => v.str_field(value, IDENTIFIER).map(Value::String),
        "paths" => glob_list(v, value, Some(1), 100).map(Value::Array),
        _ => glob_list(v, value, None, 100).map(Value::Array),
    })?;
    Some(Value::Object(dumped))
}

/// `ontology(extensions)` checks, as the `extensions` field validator runs them.
fn check_extensions(extensions: &Map<String, Value>) -> Result<(), PyError> {
    static NAMESPACED: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
        Regex::new(r"^x-[a-z0-9-]+:[A-Za-z][A-Za-z0-9_]{0,63}$").expect("valid")
    });
    for (kind, values) in extensions {
        let values = values.as_array().map(Vec::as_slice).unwrap_or_default();
        // Python's `fullmatch` takes `\Z`; Rust's `$` without multiline is the same.
        if !matches!(kind.as_str(), "entities" | "relationships")
            || values.len() > 100
            || values
                .iter()
                .any(|item| !NAMESPACED.is_match(item.as_str().unwrap_or_default()))
        {
            return Err(PyError::value(NAMESPACED_MESSAGE));
        }
    }
    Ok(())
}

fn validate_dict<'a>(
    v: &mut Validator,
    input: &'a PyValue,
    mut each: impl FnMut(&mut Validator, &'a str, &'a PyValue) -> Option<Value>,
) -> Option<Map<String, Value>> {
    let PyValue::Dict(entries) = input else {
        v.issue("dict_type", "Input should be a valid dictionary");
        return None;
    };
    let mark = v.issues.len();
    let mut out = Map::new();
    for (key, value) in entries {
        if let Some(done) = v.field(key, |v| each(v, key, value)) {
            out.insert(key.clone(), done);
        }
    }
    v.is_clean_since(mark).then_some(out)
}

pub fn validate_contract(v: &mut Validator, input: &PyValue) -> Option<Value> {
    let specs = [
        optional("version", json!(1)),
        optional("kind", json!("ArchitectureContract")),
        optional("styles", json!([])),
        optional("domains", json!({})),
        optional("rules", json!([])),
        optional("exceptions", json!([])),
        optional("extensions", json!({})),
    ];
    let dumped = v.model(
        "ArchitectureContract",
        input,
        &specs,
        |v, name, value| match name {
            "version" => {
                // A `before` validator: the exact integer 1, not 1.0 and not True.
                if matches!(value, PyValue::Int(1)) {
                    Some(json!(1))
                } else {
                    v.value_error("Unsupported architecture contract version");
                    None
                }
            }
            "kind" => v
                .literal(value, &["ArchitectureContract"])
                .map(Value::String),
            "styles" => v
                .str_list(value, None, Some(32), IDENTIFIER)
                .map(Value::Array),
            "domains" => validate_dict(v, value, |v, key, item| {
                // The key is validated as an identifier before the value.
                let mark = v.issues.len();
                v.at(Loc::Key("[key]".into()), |v| {
                    v.str_field(&PyValue::Str(key.to_owned()), IDENTIFIER)
                });
                if !v.is_clean_since(mark) {
                    return None;
                }
                validate_domain(v, item)
            })
            .map(Value::Object),
            "rules" => v
                .list_field(value, None, Some(256), validate_rule)
                .map(Value::Array),
            "exceptions" => v
                .list_field(value, None, Some(256), validate_exception)
                .map(Value::Array),
            _ => {
                let map = validate_dict(v, value, |v, key, item| {
                    v.at(Loc::Key("[key]".into()), |v| {
                        v.literal(
                            &PyValue::Str(key.to_owned()),
                            &["entities", "relationships"],
                        )
                    })?;
                    v.str_list(item, None, None, IDENTIFIER).map(Value::Array)
                })?;
                match check_extensions(&map) {
                    Ok(()) => Some(Value::Object(map)),
                    Err(error) => {
                        v.value_error(&error.message);
                        None
                    }
                }
            }
        },
    )?;
    // The model validator: only reached when every field passed.
    let rules = dumped["rules"].as_array().cloned().unwrap_or_default();
    let exceptions = dumped["exceptions"].as_array().cloned().unwrap_or_default();
    let mut keys: Vec<(String, String)> = rules
        .iter()
        .map(|rule| (string(rule, "id"), string(rule, "scope")))
        .collect();
    let total = keys.len();
    keys.sort();
    keys.dedup();
    if keys.len() != total {
        v.value_error("Duplicate architecture rule in the same scope");
        return None;
    }
    let ids: std::collections::HashSet<String> =
        rules.iter().map(|rule| string(rule, "id")).collect();
    if exceptions
        .iter()
        .any(|item| !ids.contains(&string(item, "rule_id")))
    {
        v.value_error("Architecture exception refers to an unknown rule");
        return None;
    }
    let mut exception_ids: Vec<String> = exceptions.iter().map(|item| string(item, "id")).collect();
    exception_ids.sort();
    exception_ids.dedup();
    if exception_ids.len() != exceptions.len() {
        v.value_error("Duplicate architecture exception");
        return None;
    }
    if dumped["domains"]
        .as_object()
        .is_some_and(|map| map.len() > 100)
    {
        v.value_error("Architecture contract exceeds extension limits");
        return None;
    }
    Some(Value::Object(dumped))
}

/// What `parse_contract` accepts: text, bytes or an already decoded value.
pub enum ContractInput<'a> {
    Text(&'a str),
    Bytes(&'a [u8]),
    Value(&'a Value),
    /// A decoded value that may hold what JSON numbers here cannot (huge integers).
    Decoded(&'a PyValue),
}

fn json_error(failure: JsonFailure) -> PyError {
    match failure {
        JsonFailure::Decode(message) => PyError::new("JSONDecodeError", message),
        JsonFailure::Value(message) => PyError::value(message),
        // `parse_contract` turns RecursionError into this ValueError.
        JsonFailure::Recursion => PyError::value(STRUCTURAL_LIMITS_MESSAGE),
    }
}

/// `parse_contract`.
pub fn parse_contract(input: ContractInput<'_>) -> Result<Contract, PyError> {
    let decoded;
    let value = match input {
        ContractInput::Value(value) => PyValue::from_value(value),
        ContractInput::Decoded(value) => value.clone(),
        ContractInput::Text(source) => {
            if source.chars().count() > CONTRACT_BYTES_LIMIT {
                return Err(PyError::value("Architecture contract exceeds size limit"));
            }
            py_json::loads(source, Some(DUPLICATE_KEY_MESSAGE)).map_err(json_error)?
        }
        ContractInput::Bytes(data) => {
            if data.len() > CONTRACT_BYTES_LIMIT {
                return Err(PyError::value("Architecture contract exceeds size limit"));
            }
            decoded = decode_json_bytes(data)?;
            py_json::loads_decoded(&decoded, Some(DUPLICATE_KEY_MESSAGE)).map_err(json_error)?
        }
    };
    validate_model(&value)
}

/// `ArchitectureContract.model_validate(data)` on a decoded value.
pub fn validate_model(value: &PyValue) -> Result<Contract, PyError> {
    let mut validator = Validator::new();
    match validate_contract(&mut validator, value) {
        Some(dump) if validator.issues.is_empty() => Ok(Contract::from_dump(dump)),
        _ => Err(PyError::validation(
            "ArchitectureContract",
            validator.issues,
        )),
    }
}

/// `json.loads(bytes)`: the encoding is found from the first bytes, as `json.detect_encoding`
/// does, then the text is decoded. Malformed UTF-16 and UTF-32 report a generic message.
fn decode_json_bytes(data: &[u8]) -> Result<String, PyError> {
    #[derive(Clone, Copy)]
    enum Width {
        Two,
        Four,
    }
    let wide = |bytes: &[u8], width: Width, big: bool| -> Result<String, PyError> {
        let step = match width {
            Width::Two => 2,
            Width::Four => 4,
        };
        let name = match (width, big) {
            (Width::Two, true) => "utf-16-be",
            (Width::Two, false) => "utf-16-le",
            (Width::Four, true) => "utf-32-be",
            (Width::Four, false) => "utf-32-le",
        };
        let broken = || {
            PyError::new(
                "UnicodeDecodeError",
                format!("'{name}' codec can't decode the contract"),
            )
        };
        if !bytes.len().is_multiple_of(step) {
            return Err(broken());
        }
        let units: Vec<u32> = bytes
            .chunks(step)
            .map(|chunk| {
                let mut value = 0u32;
                let ordered: Vec<u8> = if big {
                    chunk.to_vec()
                } else {
                    chunk.iter().rev().copied().collect()
                };
                for byte in ordered {
                    value = (value << 8) | u32::from(byte);
                }
                value
            })
            .collect();
        match width {
            Width::Two => char::decode_utf16(units.iter().map(|unit| *unit as u16))
                .collect::<Result<String, _>>()
                .map_err(|_| broken()),
            Width::Four => units
                .iter()
                .map(|unit| char::from_u32(*unit))
                .collect::<Option<String>>()
                .ok_or_else(broken),
        }
    };
    let utf8 = super::decode_utf8;
    if data.starts_with(&[0xFF, 0xFE, 0x00, 0x00]) {
        return wide(&data[4..], Width::Four, false);
    }
    if data.starts_with(&[0x00, 0x00, 0xFE, 0xFF]) {
        return wide(&data[4..], Width::Four, true);
    }
    if data.starts_with(&[0xFF, 0xFE]) {
        return wide(&data[2..], Width::Two, false);
    }
    if data.starts_with(&[0xFE, 0xFF]) {
        return wide(&data[2..], Width::Two, true);
    }
    if data.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return utf8(&data[3..]);
    }
    if data.len() >= 4 {
        if data[0] == 0 {
            return wide(
                data,
                if data[1] != 0 {
                    Width::Two
                } else {
                    Width::Four
                },
                true,
            );
        }
        if data[1] == 0 {
            let two = data[2] != 0 || data[3] != 0;
            return wide(data, if two { Width::Two } else { Width::Four }, false);
        }
    } else if data.len() == 2 {
        if data[0] == 0 {
            return wide(data, Width::Two, true);
        }
        if data[1] == 0 {
            return wide(data, Width::Two, false);
        }
    }
    utf8(data)
}

/// `digest(value)`: SHA-256 of the canonical JSON.
pub fn digest(value: &Value) -> String {
    format!(
        "{:x}",
        Sha256::digest(pytext::python_json(value).as_bytes())
    )
}

pub fn contract_digest(contract: &Contract) -> String {
    digest(&contract.dump)
}

/// `ontology(extensions)`.
pub fn ontology(extensions: Option<&Value>) -> Result<Value, PyError> {
    let empty = Value::Object(Map::new());
    let extensions = extensions.unwrap_or(&empty);
    let map = extensions.as_object().cloned().unwrap_or_default();
    // `extensions or {}`: an empty mapping is stored as `{}`.
    let result = json!({
        "version": VERSION,
        "entities": [
            "Repository", "Package", "Directory", "File", "Symbol", "Domain", "Service",
            "Adapter", "Contract", "Interface", "APIEndpoint", "Event", "Worker", "Database",
            "Deployment", "Test", "ArchitecturePolicy", "ArchitectureDecision", "Finding",
        ],
        "relationships": [
            "CONTAINS", "IMPORTS", "CALLS", "IMPLEMENTS", "EXPOSES", "DEPENDS_ON", "OWNS",
            "PERSISTS_TO", "DEPLOYS_TO", "TESTED_BY", "GOVERNED_BY", "VIOLATES",
        ],
        "extensions": map,
        "notice": "Available types are a vocabulary, not claims that all relationships are extracted. POSSIBLE_CALL remains inferred and is not CALLS.",
    });
    check_extensions(&map)?;
    Ok(result)
}

/// `valid_source_files`: keep files whose path is safe, and count the rest.
pub fn valid_source_files(files: &[Value]) -> Result<(Vec<Value>, usize), PyError> {
    let mut valid = Vec::new();
    for file in files {
        let path = file
            .as_object()
            .ok_or_else(|| PyError::new("TypeError", super::util::not_subscriptable(file)))?
            .get("path")
            .ok_or_else(|| PyError::new("KeyError", "'path'"))?;
        let keep = match path {
            Value::String(text) => is_safe_path(text, false),
            Value::Null | Value::Bool(false) => false,
            Value::Number(number) if number.as_f64() == Some(0.0) => false,
            Value::Array(items) if items.is_empty() => false,
            Value::Object(map) if map.is_empty() => false,
            Value::Number(number) => {
                let kind = if number.is_f64() { "float" } else { "int" };
                return Err(PyError::new(
                    "TypeError",
                    format!("object of type '{kind}' has no len()"),
                ));
            }
            Value::Bool(_) => {
                return Err(PyError::new(
                    "TypeError",
                    "object of type 'bool' has no len()",
                ));
            }
            Value::Array(_) => {
                return Err(PyError::new(
                    "AttributeError",
                    "'list' object has no attribute 'startswith'",
                ));
            }
            Value::Object(_) => {
                return Err(PyError::new(
                    "AttributeError",
                    "'dict' object has no attribute 'startswith'",
                ));
            }
        };
        if keep {
            valid.push(file.clone());
        }
    }
    let skipped = files.len() - valid.len();
    Ok((valid, skipped))
}
