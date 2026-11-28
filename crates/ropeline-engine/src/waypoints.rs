//! Content-addressed, source-backed descriptions and static index evidence.
//!
//! Port of `codegraph.waypoints`. All source is data: these functions neither
//! execute connected code nor claim that a test suite or an architecture policy
//! has passed. Records are `serde_json` values because Python persisted them as
//! dictionaries and the digests are taken over their JSON text.

use crate::file_categories::file_category;
use crate::parsing::Symbol;
use crate::py_regex::{for_case_insensitive, regex, search};
use crate::pyast::{self, Node, ParseFailure};
use crate::text;
use crate::waypoint_signals::{RUST_COMMENT, Signal, comment_signals, rust_test_names};
use fancy_regex::Regex;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::LazyLock;

pub const VERSION: &str = "waypoint-v2";
pub const VALIDATOR_VERSION: &str = "waypoint-checks-v1";
pub const PURPOSES: [&str; 15] = [
    "artifacts",
    "assets",
    "code",
    "configuration",
    "core",
    "documentation",
    "fixtures",
    "guardrails",
    "integration",
    "other",
    "performance",
    "setup",
    "storage",
    "tests",
    "ui",
];
pub const MAX_SOURCE_BYTES: usize = 16_000_000;
pub const MAX_CHILDREN: usize = 20_000;

fn label(purpose: &str) -> &'static str {
    match purpose {
        "tests" => "Tests",
        "fixtures" => "Test fixtures",
        "setup" => "Test setup",
        "performance" => "Performance tests",
        "guardrails" => "Guardrail tests",
        "ui" => "User interface",
        "storage" => "Data storage",
        "integration" => "Integration",
        "core" => "Core behaviour",
        "configuration" => "Configuration",
        "documentation" => "Documentation",
        "assets" => "Assets",
        "artifacts" => "Generated output",
        "code" => "Source definitions",
        _ => "Purpose not established",
    }
}

/// What Python raised: an exception type name and its message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaypointError {
    pub kind: &'static str,
    pub message: String,
}

impl WaypointError {
    fn value(message: &str) -> Self {
        Self {
            kind: "ValueError",
            message: message.to_owned(),
        }
    }

    fn missing(key: &str) -> Self {
        Self {
            kind: "KeyError",
            message: format!("'{key}'"),
        }
    }
}

impl fmt::Display for WaypointError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.kind, self.message)
    }
}

impl std::error::Error for WaypointError {}

type Outcome<T> = Result<T, WaypointError>;

static SECRET: LazyLock<Regex> = LazyLock::new(|| {
    regex(concat!(
        r"(?i)(?:\bBearer\s+\S+|\b(?:sk-(?:or-v1-)?|gh[pousr]_)[A-Za-z0-9_-]{12,}",
        r"|\bAIza[A-Za-z0-9_-]{20,}|\b(?:password|api[_ -]?key|secret|access[_ -]?token",
        r"|private[_ -]?key)\s*[:=]\s*[^\s,;]+|-----BEGIN [^-]*PRIVATE KEY-----)"
    ))
});
static INSTRUCTION: LazyLock<Regex> = LazyLock::new(|| {
    regex(concat!(
        r"(?i)\b(?:ignore (?:all|previous|prior)|system prompt|reveal (?:the |your )?",
        r"(?:token|secret|password)|send (?:the |your )?(?:token|secret|password))\b"
    ))
});
static TEST_CALL: LazyLock<Regex> = LazyLock::new(|| {
    regex(r#"\b(?:it|test|describe)(?:\.(?:only|skip|todo))?\s*\(\s*(['"`])([^\r\n]*?)\1"#)
});
static COMMENT: LazyLock<Regex> =
    LazyLock::new(|| regex(r"/\*\*?\s*([^*][\s\S]*?)\*/|(?m:^\s*//\s*(.+)$)"));
static TITLE_SPLIT: LazyLock<Regex> =
    LazyLock::new(|| regex(r"\s+(?:when|while|without|after|before|and)\b"));
static HASH: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\A[0-9a-f]{64}\z").expect("valid"));
static CAMEL: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"([a-z0-9])([A-Z])").expect("valid"));
static PERFORMANCE: LazyLock<Regex> = LazyLock::new(|| {
    regex(
        r"\b(?:benchmark|performance|throughput|latency|bounded layout|large (?:graph|dataset))\b",
    )
});
static GUARDRAILS: LazyLock<Regex> = LazyLock::new(|| {
    regex(
        r"\b(?:forbidden|authori[sz]ation|malformed|unsafe|hostile|injection|guardrail|boundary|boundaries|credential)\b",
    )
});
static PURPOSE_RULES: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    vec![
        (
            "fixtures",
            regex(
                r"\b(?:fixture|fixtures|sample(?:\s+[a-z]+){0,3}\s+data|example (?:graph|data|snapshot)|mock data)\b",
            ),
        ),
        (
            "setup",
            regex(r"\b(?:test setup|before each|beforeeach|testing environment)\b"),
        ),
        (
            "storage",
            regex(
                r"\b(?:persistence|persist|sqlite|postgresql|database|storage|repository store)\b",
            ),
        ),
        (
            "ui",
            regex(
                r"\b(?:render|renders|component|user interface|viewport|canvas|visuali[sz])\w*\b",
            ),
        ),
        (
            "integration",
            regex(r"\b(?:adapter|connector|transport|webhook|mcp|http client|remote api)\b"),
        ),
        (
            "configuration",
            regex(r"\b(?:configuration|configure|settings)\b"),
        ),
    ]
});

fn secret_or_instruction(value: &str) -> bool {
    let value = for_case_insensitive(value);
    search(&SECRET, &value) || search(&INSTRUCTION, &value)
}

fn has_other(value: &str) -> bool {
    value.chars().any(text::is_other)
}

/// `_digest`: SHA-256 over Python's sorted, compact, ASCII-only JSON.
pub fn digest(value: &Value) -> String {
    format!("{:x}", Sha256::digest(text::python_json(value).as_bytes()))
}

fn sha256_hex(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

fn is_hash(value: &Value) -> bool {
    value.as_str().is_some_and(|text| HASH.is_match(text))
}

/// `_path`: a repository-relative path, or `.` for the root when `root` is set.
fn path_ok(value: &Value, root: bool) -> bool {
    let Some(path) = value.as_str() else {
        return false;
    };
    let length = text::char_len(path);
    if !(0 < length && length <= 4096) {
        return false;
    }
    let shape = if root && path == "." {
        true
    } else {
        !path.starts_with('/')
            && !path.contains('\\')
            && !path.contains(':')
            && path.split('/').all(|part| !matches!(part, "" | "." | ".."))
    };
    shape && !has_other(path)
}

/// `_text`: display-safe text of bounded length.
fn checked_text(value: &Value, limit: usize) -> Outcome<String> {
    match value.as_str() {
        Some(found)
            if (1..=limit).contains(&text::char_len(found))
                && !has_other(found)
                && !secret_or_instruction(found) =>
        {
            Ok(found.to_owned())
        }
        _ => Err(WaypointError::value("Invalid waypoint text")),
    }
}

/// `_safe`: collapse whitespace, drop control characters and anything that looks
/// like a credential or an instruction, and clip at a word boundary.
fn safe(value: &str, limit: usize) -> String {
    let cleaned: String = value
        .chars()
        .map(|c| if text::is_other(c) { ' ' } else { c })
        .collect();
    let collapsed = text::split_words(&cleaned).join(" ");
    if secret_or_instruction(&collapsed) {
        return String::new();
    }
    if text::char_len(&collapsed) > limit {
        let head = text::char_slice(&collapsed, 0, Some(limit - 1));
        let clipped = head.rsplit_once(' ').map_or(head, |(before, _)| before);
        let kept = if clipped.is_empty() { head } else { clipped };
        return format!("{}\u{2026}", text::rstrip(kept));
    }
    collapsed
}

/// `re.IGNORECASE` match of `test` or `spec` at the start, followed by `_` or `-`.
fn strip_test_prefix(value: &str) -> &str {
    let fold = |c: char| match c {
        '\u{17f}' => 's',
        other => other.to_ascii_lowercase(),
    };
    let chars: Vec<(usize, char)> = value.char_indices().collect();
    for word in ["test", "spec"] {
        let length = word.len();
        if chars.len() > length
            && word
                .chars()
                .zip(chars.iter())
                .all(|(expected, (_, found))| fold(*found) == expected)
        {
            let mut index = length;
            while index < chars.len() && matches!(chars[index].1, '_' | '-') {
                index += 1;
            }
            if index > length {
                return match chars.get(index) {
                    Some((offset, _)) => &value[*offset..],
                    None => "",
                };
            }
        }
    }
    value
}

/// `_human`: turn an identifier into readable words.
fn human(value: &str) -> String {
    let spaced = CAMEL.replace_all(value, "$1 $2");
    let stripped = strip_test_prefix(&spaced);
    safe(&stripped.replace(['_', '-'], " "), 120)
}

fn symbol_values(symbols: &[Symbol]) -> Vec<Value> {
    symbols
        .iter()
        .map(|symbol| json!([symbol.kind, symbol.name, symbol.line, symbol.parent]))
        .collect()
}

/// The symbol list in the shape `build_file_facts` takes, for callers that hold
/// parsed symbols rather than stored JSON.
pub fn symbols_as_values(symbols: &[Symbol]) -> Vec<Value> {
    symbol_values(symbols)
}

fn python_signals(source: &str) -> (Vec<Signal>, Vec<Signal>) {
    let mut descriptions: Vec<Signal> = Vec::new();
    let mut tests: Vec<Signal> = Vec::new();
    let outcome = pyast::with_module(source, |module| {
        let mut found = (Vec::new(), Vec::new());
        for node in module.walk() {
            let is_function = matches!(node, Node::Stmt(ruff_python_ast::Stmt::FunctionDef(_)));
            let is_candidate = is_function
                || matches!(
                    node,
                    Node::Module(_) | Node::Stmt(ruff_python_ast::Stmt::ClassDef(_))
                );
            if !is_candidate {
                continue;
            }
            if let Some(doc) = module.docstring(node)
                && !doc.is_empty()
                && let Some(line) = module.first_statement_line(node)
                && found.0.len() < 16
            {
                let first = text::splitlines(&doc).first().copied().unwrap_or("");
                found
                    .0
                    .push((first.to_owned(), line as i64, "source-docstring"));
            }
            if let Node::Stmt(ruff_python_ast::Stmt::FunctionDef(function)) = node
                && function.name.as_str().starts_with("test_")
                && found.1.len() < 128
            {
                let line = module.visit(node).line.unwrap_or(0);
                found.1.push((
                    human(function.name.as_str()),
                    line as i64,
                    "test-definition",
                ));
            }
        }
        found
    });
    match outcome {
        Ok((found_descriptions, found_tests)) => {
            descriptions = found_descriptions;
            tests = found_tests;
        }
        // SyntaxError, ValueError and RecursionError all leave the tree absent.
        Err(ParseFailure::Syntax(_) | ParseFailure::Recursion) => {}
    }
    (descriptions, tests)
}

fn script_signals(source: &str) -> (Vec<Signal>, Vec<Signal>) {
    let mut tests: Vec<Signal> = Vec::new();
    let mut position = 0;
    while let Ok(Some(captures)) = TEST_CALL.captures_from_pos(source, position) {
        let whole = captures.get(0).expect("whole match");
        let line = source.as_bytes()[..whole.start()]
            .iter()
            .filter(|byte| **byte == b'\n')
            .count() as i64
            + 1;
        tests.push((
            captures
                .get(2)
                .map_or("", |group| group.as_str())
                .to_owned(),
            line,
            "test-description",
        ));
        if tests.len() >= 128 {
            break;
        }
        // An empty match cannot occur, so the search always advances.
        position = whole.end();
    }
    let descriptions = comment_signals(&COMMENT, source);
    (descriptions, tests)
}

/// Doc comments describe a Rust file; `#[test]` functions name its tests.
fn rust_signals(source: &str) -> (Vec<Signal>, Vec<Signal>) {
    let tests = rust_test_names(source)
        .into_iter()
        .map(|(name, line)| (human(&name), line, "test-definition"))
        .collect();
    (comment_signals(&RUST_COMMENT, source), tests)
}

fn definition_signals(symbols: &[Value]) -> Vec<Signal> {
    let mut definitions = Vec::new();
    for symbol in symbols.iter().take(20_000) {
        let Some(items) = symbol.as_array().filter(|items| items.len() >= 3) else {
            continue;
        };
        let (Some(kind), Some(name)) = (items[0].as_str(), items[1].as_str()) else {
            continue;
        };
        if kind.starts_with("import") {
            continue;
        }
        let line = integer(&items[2]);
        if let Some(line) = line.filter(|line| *line > 0) {
            let readable = human(name);
            if !readable.is_empty() {
                definitions.push((readable, line, "indexed-definition"));
                if definitions.len() >= 12 {
                    break;
                }
            }
        }
    }
    definitions
}

/// `type(value) is int`: a JSON integer, never a boolean.
fn integer(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_i64(),
        _ => None,
    }
}

fn purpose_of(signals: &[&str], tests: bool) -> &'static str {
    let joined = text::casefold(&signals.join(" "));
    if tests {
        if search(&PERFORMANCE, &joined) {
            return "performance";
        }
        if search(&GUARDRAILS, &joined) {
            return "guardrails";
        }
        return "tests";
    }
    for (purpose, pattern) in PURPOSE_RULES.iter() {
        if search(pattern, &joined) {
            return purpose;
        }
    }
    "code"
}

fn first_char_upper(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => text::upper_char(first) + chars.as_str(),
        None => String::new(),
    }
}

fn first_char_lower(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => text::lower_char(first) + chars.as_str(),
        None => String::new(),
    }
}

/// Describe one captured byte sequence; no branch or pathname enters its cache.
///
/// `symbols` are `[kind, name, line, parent]` arrays as stored; `facts` is the
/// analysis dictionary. Anything of the wrong shape is refused as Python refuses it.
pub fn build_file_facts(
    language: &str,
    source: &[u8],
    symbols: &[Value],
    facts: &Value,
) -> Outcome<Value> {
    if source.len() > MAX_SOURCE_BYTES || !facts.is_object() {
        return Err(WaypointError::value("Invalid waypoint source analysis"));
    }
    if !matches!(
        language,
        "python" | "javascript" | "typescript" | "tsx" | "rust" | "text"
    ) {
        return Err(WaypointError::value("Unsupported waypoint source language"));
    }
    let content = std::str::from_utf8(source)
        .map_err(|_| WaypointError::value("Waypoint source must be UTF-8"))?;
    let line_count = text::splitlines(content).len() as i64;
    let (comments, tests) = match language {
        "python" => python_signals(content),
        "rust" => rust_signals(content),
        _ => script_signals(content),
    };
    let definitions = definition_signals(symbols);
    let clean = |items: Vec<Signal>| -> Vec<Signal> {
        items
            .into_iter()
            .filter_map(|(value, line, basis)| {
                let cleaned = safe(&value, 240);
                (!cleaned.is_empty() && (1..=line_count).contains(&line))
                    .then_some((cleaned, line, basis))
            })
            .collect()
    };
    let (comments, tests, definitions) = (clean(comments), clean(tests), clean(definitions));
    let signals: Vec<&str> = comments
        .iter()
        .chain(tests.iter())
        .chain(definitions.iter())
        .map(|item| item.0.as_str())
        .collect();
    let mut purpose = purpose_of(&signals, !tests.is_empty());
    let (title, summary, selected): (String, String, Vec<&Signal>);
    if let Some(first) = tests.first() {
        selected = tests.iter().take(3).collect();
        let head = TITLE_SPLIT
            .find(&first.0)
            .ok()
            .flatten()
            .map_or(first.0.as_str(), |found| &first.0[..found.start()]);
        title = first_char_upper(head);
        summary = format!(
            "Tests: {}.",
            selected
                .iter()
                .map(|item| first_char_lower(&item.0))
                .collect::<Vec<_>>()
                .join("; ")
        );
    } else if let Some(first) = comments.first() {
        summary = first.0.clone();
        title = label(purpose).to_owned();
        selected = vec![first];
    } else if !definitions.is_empty() {
        title = label(purpose).to_owned();
        selected = definitions.iter().take(3).collect();
        summary = format!(
            "Defines {}.",
            selected
                .iter()
                .map(|item| item.0.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    } else {
        if source.is_empty() {
            purpose = "other";
        }
        title = if source.is_empty() {
            "Empty source file".to_owned()
        } else {
            label(purpose).to_owned()
        };
        summary = if source.is_empty() {
            "This captured file is empty.".to_owned()
        } else {
            "No readable purpose description was found in the captured source.".to_owned()
        };
        selected = Vec::new();
    }
    let flag = facts
        .get("architecture")
        .and_then(Value::as_object)
        .and_then(|architecture| architecture.get("parse_error"))
        .and_then(Value::as_bool);
    let mut value = json!({
        "version": VERSION,
        "title": safe(&title, 48),
        "summary": safe(&summary, 240),
        "purposeHint": purpose,
        "evidenceLines": selected
            .iter()
            .map(|(_, line, basis)| json!({"line": line, "basis": basis}))
            .collect::<Vec<_>>(),
        "lineCount": line_count,
        "parseErrors": i64::from(flag.unwrap_or(false)),
        "parserEvidence": flag.is_some(),
    });
    let key = digest(&json!([VERSION, sha256_hex(source), value]));
    value["summaryKey"] = Value::String(key);
    Ok(value)
}

const FACT_KEYS: [&str; 9] = [
    "version",
    "title",
    "summary",
    "purposeHint",
    "evidenceLines",
    "lineCount",
    "parseErrors",
    "parserEvidence",
    "summaryKey",
];

fn keys_are(object: &Map<String, Value>, expected: &[&str]) -> bool {
    let wanted: BTreeSet<&str> = expected.iter().copied().collect();
    let found: BTreeSet<&str> = object.keys().map(String::as_str).collect();
    wanted == found
}

fn one_of(value: &Value, allowed: &[&str]) -> bool {
    value.as_str().is_some_and(|text| allowed.contains(&text))
}

/// `_facts`: validate cached facts.
fn check_facts(value: &Value) -> Outcome<&Map<String, Value>> {
    let invalid = || WaypointError::value("Invalid cached waypoint facts");
    let object = value.as_object().ok_or_else(invalid)?;
    if !keys_are(object, &FACT_KEYS)
        || object["version"] != VERSION
        || !one_of(&object["purposeHint"], &PURPOSES)
    {
        return Err(invalid());
    }
    checked_text(&object["title"], 48)?;
    checked_text(&object["summary"], 240)?;
    if !is_hash(&object["summaryKey"]) {
        return Err(WaypointError::value("Invalid waypoint summary identity"));
    }
    let counts = || WaypointError::value("Invalid waypoint source counts");
    let line_count = integer(&object["lineCount"])
        .filter(|count| (0..=MAX_SOURCE_BYTES as i64).contains(count))
        .ok_or_else(counts)?;
    integer(&object["parseErrors"])
        .filter(|count| matches!(count, 0 | 1))
        .ok_or_else(counts)?;
    if !object["parserEvidence"].is_boolean() {
        return Err(counts());
    }
    let evidence_error = || WaypointError::value("Invalid waypoint source evidence");
    let evidence = object["evidenceLines"]
        .as_array()
        .filter(|items| items.len() <= 8)
        .ok_or_else(evidence_error)?;
    for item in evidence {
        let entry = item
            .as_object()
            .filter(|entry| keys_are(entry, &["line", "basis"]))
            .ok_or_else(evidence_error)?;
        integer(&entry["line"])
            .filter(|line| (1..=line_count).contains(line))
            .ok_or_else(evidence_error)?;
        checked_text(&entry["basis"], 80)?;
    }
    Ok(object)
}

/// `_dependencies`: `(path, identity, kind)` triples, de-duplicated and sorted.
fn dependencies(values: &Value) -> Outcome<Vec<(String, String, String)>> {
    let list = values
        .as_array()
        .filter(|items| items.len() <= MAX_CHILDREN)
        .ok_or_else(|| WaypointError::value("Invalid waypoint dependencies"))?;
    let mut result = BTreeSet::new();
    for value in list {
        let triple = value
            .as_array()
            .filter(|items| items.len() == 3)
            .ok_or_else(|| WaypointError::value("Invalid waypoint dependency"))?;
        if !path_ok(&triple[0], false) || !is_hash(&triple[1]) {
            return Err(WaypointError::value("Invalid waypoint dependency identity"));
        }
        result.insert((
            triple[0].as_str().unwrap_or_default().to_owned(),
            triple[1].as_str().unwrap_or_default().to_owned(),
            checked_text(&triple[2], 80)?,
        ));
    }
    Ok(result.into_iter().collect())
}

fn triples_value(triples: &[(String, String, String)]) -> Value {
    Value::Array(
        triples
            .iter()
            .map(|(path, identity, kind)| json!([path, identity, kind]))
            .collect(),
    )
}

pub fn validation_fingerprint(
    analysis_id: &str,
    values: &Value,
    policy_digest: &str,
) -> Outcome<String> {
    let hash = |text: &str| HASH.is_match(text);
    if !hash(analysis_id) || !hash(policy_digest) {
        return Err(WaypointError::value("Invalid waypoint validation identity"));
    }
    Ok(digest(&json!([
        VALIDATOR_VERSION,
        analysis_id,
        triples_value(&dependencies(values)?),
        policy_digest
    ])))
}

pub fn validate_file(
    facts: &Value,
    values: &Value,
    policy_digest: &str,
    analysis_id: &str,
) -> Outcome<Value> {
    let facts = check_facts(facts)?;
    let triples = dependencies(values)?;
    let fingerprint = validation_fingerprint(analysis_id, &triples_value(&triples), policy_digest)?;
    let evidence = facts["parserEvidence"].as_bool().unwrap_or(false);
    let parsed = evidence && integer(&facts["parseErrors"]) == Some(0);
    let parser_detail = if parsed {
        "No parser errors were recorded."
    } else if evidence {
        "Parser errors limit this description."
    } else {
        "Parser evidence was not recorded for this source."
    };
    Ok(json!({
        "kind": "static-index",
        "status": if parsed { "current" } else { "inconclusive" },
        "fingerprint": fingerprint,
        "ruleVersion": VALIDATOR_VERSION,
        "checks": [
            {
                "name": "Captured source",
                "status": "supported",
                "detail": "This description is tied to a captured source analysis.",
            },
            {
                "name": "Parser evidence",
                "status": if parsed { "supported" } else { "inconclusive" },
                "detail": parser_detail,
            },
            {
                "name": "Dependency context",
                "status": "supported",
                "detail": format!(
                    "Pinned to {} loaded dependency identities and the applicable index rules.",
                    triples.len()
                ),
            },
            {
                "name": "Execution coverage",
                "status": "inconclusive",
                "detail": "Static index evidence only; tests and runtime behaviour were not executed or validated.",
            },
        ],
    }))
}

/// `_path_purpose`: the path decides test and non-code purposes; the hint decides the rest.
fn path_purpose(path: &str, hint: &str) -> String {
    let category = file_category(Some(path), "file").map_or("other", |found| found.category);
    let folded = text::casefold(path);
    let parts: Vec<&str> = folded.split('/').collect();
    let name = parts.last().copied().unwrap_or("");
    if category == "test" {
        if parts
            .iter()
            .any(|part| matches!(*part, "fixtures" | "__fixtures__" | "__mocks__"))
            || name.contains("fixture")
        {
            return "fixtures".into();
        }
        if matches!(name, "setup.ts" | "setup.js" | "setup.py" | "conftest.py")
            || name.starts_with("setup.")
        {
            return "setup".into();
        }
        if name.contains("benchmark") || name.contains("performance") {
            return "performance".into();
        }
        return if matches!(
            hint,
            "tests" | "fixtures" | "setup" | "performance" | "guardrails"
        ) {
            hint.to_owned()
        } else {
            "tests".into()
        };
    }
    match category {
        "configuration" => "configuration",
        "documentation" => "documentation",
        "asset" => "assets",
        "artifact" => "artifacts",
        "other" => "other",
        _ => return hint.to_owned(),
    }
    .to_owned()
}

fn static_validation_unchecked(content_hash: &str) -> Value {
    json!({
        "kind": "static-index",
        "status": "inconclusive",
        "fingerprint": digest(&json!([VALIDATOR_VERSION, content_hash, "not-checked"])),
        "ruleVersion": VALIDATOR_VERSION,
        "checks": [{
            "name": "Index evidence",
            "status": "inconclusive",
            "detail": "Source checks have not been prepared; tests were not run.",
        }],
    })
}

/// Bind cached facts to a path, revision and content hash, producing a validated record.
pub fn bind_file(
    facts: &Value,
    path: &str,
    revision: &str,
    content_hash: &str,
    validation: Option<&Value>,
) -> Outcome<Value> {
    let facts = check_facts(facts)?;
    if !path_ok(&Value::String(path.to_owned()), false) || !HASH.is_match(content_hash) {
        return Err(WaypointError::value("Invalid waypoint source identity"));
    }
    let inputs: Map<String, Value> = facts
        .iter()
        .filter(|(key, _)| key.as_str() != "summaryKey")
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let expected = digest(&json!([VERSION, content_hash, Value::Object(inputs)]));
    if facts["summaryKey"] != expected {
        return Err(WaypointError::value(
            "Waypoint facts belong to another source content identity",
        ));
    }
    checked_text(&Value::String(revision.to_owned()), 256)?;
    let validation = validation
        .cloned()
        .unwrap_or_else(|| static_validation_unchecked(content_hash));
    let lines = facts["evidenceLines"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let evidence: Vec<Value> = if lines.is_empty() {
        vec![json!({"path": path, "revision": revision, "method": "content-hash"})]
    } else {
        lines
            .iter()
            .map(|item| {
                json!({"path": path, "revision": revision, "line": item["line"], "method": item["basis"]})
            })
            .collect()
    };
    parse_waypoint(
        &json!({
            "schemaVersion": "1",
            "path": path,
            "kind": "file",
            "title": facts["title"],
            "summary": facts["summary"],
            "purpose": path_purpose(path, facts["purposeHint"].as_str().unwrap_or("")),
            "origin": "inferred",
            "contentHash": content_hash,
            "summaryKey": facts["summaryKey"],
            "evidence": evidence,
            "validation": validation,
        }),
        path,
        revision,
    )
}

/// `_children`: the area's children, validated and sorted by path.
fn children_of(children: &Value) -> Outcome<Vec<&Value>> {
    let list = children
        .as_array()
        .filter(|items| items.len() <= MAX_CHILDREN)
        .ok_or_else(|| WaypointError::value("Invalid area waypoint children"))?;
    if list.iter().any(|child| {
        child
            .as_object()
            .and_then(|object| object.get("path"))
            .is_none_or(|path| !path_ok(path, false))
    }) {
        return Err(WaypointError::value("Invalid area waypoint child"));
    }
    let paths: BTreeSet<&str> = list
        .iter()
        .filter_map(|child| child["path"].as_str())
        .collect();
    if paths.len() != list.len() {
        return Err(WaypointError::value("Duplicated area waypoint child"));
    }
    let mut sorted: Vec<&Value> = list.iter().collect();
    sorted.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    Ok(sorted)
}

fn child_field<'a>(child: &'a Value, key: &str) -> Outcome<&'a Value> {
    child.get(key).ok_or_else(|| WaypointError::missing(key))
}

pub fn area_fingerprint(path: &str, children: &Value) -> Outcome<String> {
    if !path_ok(&Value::String(path.to_owned()), true) {
        return Err(WaypointError::value("Invalid area waypoint path"));
    }
    let mut members = Vec::new();
    for child in children_of(children)? {
        members.push(json!([
            child_field(child, "path")?,
            child_field(child, "summaryKey")?,
            child_field(child, "purpose")?,
            child_field(child, "title")?,
        ]));
    }
    Ok(digest(&json!([VERSION, path, members])))
}

pub fn area_validation(children: &Value) -> Outcome<Value> {
    let children = children_of(children)?;
    let mut fingerprints = Vec::new();
    for child in &children {
        let validation = check_validation(child.get("validation").unwrap_or(&Value::Null))?;
        fingerprints.push(json!([
            child["path"],
            validation["fingerprint"],
            validation["status"]
        ]));
    }
    let mut current = !children.is_empty();
    for child in &children {
        let status = child_field(child, "validation")?
            .get("status")
            .ok_or_else(|| WaypointError::missing("status"))?;
        current &= status == "current";
    }
    Ok(json!({
        "kind": "static-index",
        "status": if current { "current" } else { "inconclusive" },
        "fingerprint": digest(&json!([VALIDATOR_VERSION, fingerprints])),
        "ruleVersion": VALIDATOR_VERSION,
        "checks": [{
            "name": "Child index evidence",
            "status": if current { "supported" } else { "inconclusive" },
            "detail": if current {
                "Included child source checks are current; tests were not run."
            } else {
                "Some child source checks are unavailable or incomplete; tests were not run."
            },
        }],
    }))
}

/// Summarise the children of a directory, or of the whole repository for `.`.
pub fn build_area(path: &str, children: &Value, revision: &str) -> Outcome<Value> {
    let sorted = children_of(children)?;
    if !path_ok(&Value::String(path.to_owned()), true) {
        return Err(WaypointError::value("Invalid area waypoint path"));
    }
    checked_text(&Value::String(revision.to_owned()), 256)?;
    for child in &sorted {
        let child_path = child["path"].as_str().unwrap_or_default();
        if path != "." && !child_path.starts_with(&format!("{path}/")) {
            return Err(WaypointError::value(
                "Area waypoint contains a foreign child",
            ));
        }
        parse_waypoint(child, child_path, revision)?;
    }
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for child in &sorted {
        *counts
            .entry(child_field(child, "purpose")?.as_str().unwrap_or(""))
            .or_default() += 1;
    }
    let mut ranked: Vec<&str> = counts.keys().copied().collect();
    ranked.sort_by(|a, b| counts[b].cmp(&counts[a]).then_with(|| a.cmp(b)));
    let purpose = ranked.first().copied().unwrap_or("other");
    let title = if path == "." {
        "Repository map".to_owned()
    } else {
        text::capitalize(&human(path.rsplit('/').next().unwrap_or(path)))
    };
    let labels: Vec<String> = ranked
        .iter()
        .take(3)
        .map(|purpose| label(purpose).to_ascii_lowercase())
        .collect();
    let summary = if labels.is_empty() {
        "No indexed child descriptions are available for this area.".to_owned()
    } else {
        format!("Brings together {}.", labels.join(", "))
    };
    let mut evidence: Vec<Value> = Vec::new();
    for child in &sorted {
        for item in child_field(child, "evidence")?
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            if !evidence.contains(item) && evidence.len() < 8 {
                evidence.push(item.clone());
            }
        }
    }
    let mut hashed = Vec::new();
    for child in &sorted {
        hashed.push(json!([
            child_field(child, "path")?,
            child_field(child, "contentHash")?
        ]));
    }
    let area_title = {
        let cleaned = safe(&title, 48);
        if cleaned.is_empty() {
            "Source area".to_owned()
        } else {
            cleaned
        }
    };
    parse_waypoint(
        &json!({
            "schemaVersion": "1",
            "path": path,
            "kind": "area",
            "title": area_title,
            "summary": safe(&summary, 240),
            "purpose": purpose,
            "origin": "inferred",
            "contentHash": digest(&Value::Array(hashed)),
            "summaryKey": area_fingerprint(path, children)?,
            "evidence": evidence,
            "validation": area_validation(children)?,
        }),
        path,
        revision,
    )
}

/// `_validation`: validate and normalise a validation record.
fn check_validation(value: &Value) -> Outcome<Value> {
    let invalid = || WaypointError::value("Invalid waypoint source checks");
    let object = value
        .as_object()
        .filter(|object| {
            keys_are(
                object,
                &["kind", "status", "fingerprint", "ruleVersion", "checks"],
            )
        })
        .filter(|object| object["kind"] == "static-index")
        .filter(|object| one_of(&object["status"], &["current", "inconclusive"]))
        .ok_or_else(invalid)?;
    if !is_hash(&object["fingerprint"]) {
        return Err(WaypointError::value("Invalid waypoint check identity"));
    }
    checked_text(&object["ruleVersion"], 80)?;
    let checks = object["checks"]
        .as_array()
        .filter(|items| (1..=16).contains(&items.len()))
        .ok_or_else(invalid)?;
    let mut clean = Vec::new();
    for check in checks {
        let entry = check
            .as_object()
            .filter(|entry| keys_are(entry, &["name", "status", "detail"]))
            .filter(|entry| one_of(&entry["status"], &["supported", "inconclusive"]))
            .ok_or_else(|| WaypointError::value("Invalid waypoint source check"))?;
        clean.push(json!({
            "name": checked_text(&entry["name"], 80)?,
            "status": entry["status"],
            "detail": checked_text(&entry["detail"], 240)?,
        }));
    }
    let mut result = object.clone();
    result.insert("checks".into(), Value::Array(clean));
    Ok(Value::Object(result))
}

/// Validate a stored description before exposing it on any adapter.
pub fn parse_waypoint(value: &Value, path: &str, revision: &str) -> Outcome<Value> {
    let fields = [
        "schemaVersion",
        "path",
        "kind",
        "title",
        "summary",
        "purpose",
        "origin",
        "contentHash",
        "summaryKey",
        "evidence",
        "validation",
    ];
    let record = || WaypointError::value("Invalid waypoint record");
    let object = value
        .as_object()
        .filter(|object| keys_are(object, &fields))
        .filter(|object| object["schemaVersion"] == "1" && object["origin"] == "inferred")
        .filter(|object| one_of(&object["kind"], &["file", "area"]))
        .filter(|object| one_of(&object["purpose"], &PURPOSES))
        .ok_or_else(record)?;
    let kind = object["kind"].as_str().unwrap_or("");
    let is_area = kind == "area";
    if object["path"] != path || !path_ok(&Value::String(path.to_owned()), is_area) {
        return Err(WaypointError::value(
            "Waypoint belongs to another source path",
        ));
    }
    checked_text(&Value::String(revision.to_owned()), 256)?;
    for key in ["contentHash", "summaryKey"] {
        if !is_hash(&object[key]) {
            return Err(WaypointError::value("Invalid waypoint identity"));
        }
    }
    let evidence_error = || WaypointError::value("Invalid waypoint evidence");
    let evidence = object["evidence"]
        .as_array()
        .filter(|items| items.len() <= 8 && !(kind == "file" && items.is_empty()))
        .ok_or_else(evidence_error)?;
    let mut clean = Vec::new();
    for item in evidence {
        let entry = item
            .as_object()
            .filter(|entry| {
                ["path", "revision", "method"]
                    .iter()
                    .all(|key| entry.contains_key(*key))
                    && entry
                        .keys()
                        .all(|key| ["path", "revision", "method", "line"].contains(&key.as_str()))
            })
            .ok_or_else(evidence_error)?;
        let item_path = entry["path"].as_str();
        let foreign = !path_ok(&entry["path"], false)
            || entry["revision"] != revision
            || (kind == "file" && item_path != Some(path))
            || (is_area
                && path != "."
                && !item_path
                    .unwrap_or_default()
                    .starts_with(&format!("{path}/")));
        if foreign {
            return Err(WaypointError::value(
                "Waypoint evidence belongs to another source",
            ));
        }
        if let Some(line) = entry.get("line")
            && integer(line).is_none_or(|line| !(1..=MAX_SOURCE_BYTES as i64).contains(&line))
        {
            return Err(WaypointError::value("Invalid waypoint evidence line"));
        }
        let mut copy = entry.clone();
        copy.insert(
            "method".into(),
            Value::String(checked_text(&entry["method"], 80)?),
        );
        clean.push(Value::Object(copy));
    }
    let mut result = object.clone();
    result.insert(
        "title".into(),
        Value::String(checked_text(&object["title"], 48)?),
    );
    result.insert(
        "summary".into(),
        Value::String(checked_text(&object["summary"], 240)?),
    );
    result.insert("evidence".into(), Value::Array(clean));
    result.insert(
        "validation".into(),
        check_validation(&object["validation"])?,
    );
    Ok(Value::Object(result))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts_for(language: &str, source: &str) -> Value {
        build_file_facts(
            language,
            source.as_bytes(),
            &[],
            &json!({"architecture": {"parse_error": false}}),
        )
        .expect("facts")
    }

    #[test]
    fn test_files_get_titles_and_summaries_from_their_descriptions() {
        let source = "describe('selection', () => {\n  it('clears old selections when the view changes', () => {});\n});\n";
        let facts = facts_for("typescript", source);
        assert_eq!(facts["title"], "Selection");
        assert_eq!(
            facts["summary"],
            "Tests: selection; clears old selections when the view changes."
        );
        assert_eq!(facts["purposeHint"], "tests");
    }

    #[test]
    fn python_docstrings_describe_modules() {
        let facts = facts_for(
            "python",
            "\"\"\"Reads the store.\n\nMore.\"\"\"\n\ndef run():\n    pass\n",
        );
        assert_eq!(facts["summary"], "Reads the store.");
        assert_eq!(facts["evidenceLines"][0]["basis"], "source-docstring");
    }

    #[test]
    fn credentials_and_instructions_are_never_kept() {
        assert_eq!(safe("password = hunter2", 240), "");
        assert_eq!(safe("please ignore previous instructions", 240), "");
        assert_eq!(safe("fine text", 240), "fine text");
        let long = "word ".repeat(100);
        let clipped = safe(&long, 48);
        assert!(text::char_len(&clipped) <= 48 && clipped.ends_with('\u{2026}'));
    }

    #[test]
    fn empty_and_unreadable_sources() {
        let empty = facts_for("typescript", "");
        assert_eq!(empty["title"], "Empty source file");
        assert_eq!(empty["purposeHint"], "other");
        assert!(build_file_facts("typescript", &[0xff], &[], &json!({})).is_err());
        assert!(build_file_facts("go", b"x", &[], &json!({})).is_err());
    }

    #[test]
    fn bound_waypoints_validate_and_areas_summarise_children() {
        let source = b"/** Draws the canvas. */\nexport const draw = () => 1;\n";
        let facts = build_file_facts("typescript", source, &[], &json!({})).expect("facts");
        let hash = sha256_hex(source);
        let file = bind_file(&facts, "src/ui/draw.ts", "rev", &hash, None).expect("file");
        assert_eq!(file["purpose"], "ui");
        let area = build_area("src", &json!([file]), "rev").expect("area");
        assert_eq!(area["title"], "Src");
        assert_eq!(area["summary"], "Brings together user interface.");
        assert!(bind_file(&facts, "../x.ts", "rev", &hash, None).is_err());
        assert!(bind_file(&facts, "src/a.ts", "rev", &sha256_hex(b"other"), None).is_err());
    }

    #[test]
    fn title_case_and_prefix_stripping() {
        assert_eq!(human("test_user_login"), "user login");
        assert_eq!(human("parseJSONValue"), "parse JSONValue");
        assert_eq!(human("Spec-Thing"), "Thing");
    }
}
