//! Reusable, bounded architecture facts for one file: port of `architecture_extract.py`.
//! Source is parsed as data, never executed.

use super::PyError;
use super::ci::{ci_facts, quality_commands};
use super::contract::{ContractInput, parse_contract};
use super::decode_utf8;
use super::extract_rust::rust_counts;
use super::structure::extract_structure;
use super::topology::deployment_facts;
use super::util::{get, lower_name};
use crate::parsing::parse_tree;
use crate::py_json;
use crate::py_regex::{for_case_insensitive, regex};
use crate::py_toml;
use crate::py_unparse::unparse;
use crate::py_value::PyValue;
use crate::pyast::{self, Node, ParseFailure};
use crate::text;
use fancy_regex::Regex;
use ruff_python_ast::{Expr, Number, Stmt};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::sync::LazyLock;
use tree_sitter::Node as SyntaxNode;

static GRANT: LazyLock<Regex> = LazyLock::new(|| {
    regex(
        r"(?im)^\s*(REVOKE|GRANT)\s+(SELECT|ALL(?:\s+PRIVILEGES)?)\s+ON\s+[^;\n]+?\s+(?:FROM|TO)\s+([A-Za-z_][A-Za-z0-9_]*)\s*;",
    )
});
static DEPENDENCY_NAME: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\A[A-Za-z0-9_.@/-]+").expect("valid"));

const GRANT_LIMIT: usize = 128;
const LIST_LIMIT: usize = 256;

fn hex(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

/// The matched group's text from the original string. The regex ran on a copy with
/// `İ` and `ı` swapped for `i`, one character for one, so positions carry over.
fn original_group(original: &str, mapped: &str, start: usize, end: usize) -> String {
    let skip = mapped[..start].chars().count();
    let take = mapped[start..end].chars().count();
    original.chars().skip(skip).take(take).collect()
}

fn configured_grants(source: &str) -> Vec<Value> {
    let mapped = for_case_insensitive(source);
    let mut found = Vec::new();
    for captures in GRANT.captures_iter(&mapped).flatten() {
        let whole = captures.get(0).expect("group zero");
        let group = |index: usize| {
            captures.get(index).map_or(String::new(), |matched| {
                original_group(source, &mapped, matched.start(), matched.end())
            })
        };
        let action = text::upper(&group(1));
        let privilege = text::upper(&group(2));
        let kind = if action == "REVOKE" {
            "explicit-revoke"
        } else if privilege == "SELECT" {
            "scoped-read-grant"
        } else {
            "broad-grant"
        };
        found.push(json!({
            "kind": kind,
            "principal_digest": hex(group(3).as_bytes()),
            "line": mapped[..whole.start()].matches('\n').count() + 1,
        }));
        if found.len() >= GRANT_LIMIT {
            break;
        }
    }
    found
}

/// The first of `json.loads` and `tomllib.loads` that accepts the text.
fn parse_config(source: &str) -> Option<PyValue> {
    py_json::loads(source, None)
        .ok()
        .or_else(|| py_toml::loads(source).ok())
}

fn manifest_facts(config: &PyValue) -> Value {
    let project = config.get("project").filter(|found| found.is_dict());
    let manifest = project.unwrap_or(config);
    // Cargo keeps the crate name under [package].
    let name = manifest.get("name").and_then(PyValue::as_str).or_else(|| {
        config
            .get("package")
            .filter(|found| found.is_dict())
            .and_then(|package| package.get("name"))
            .and_then(PyValue::as_str)
    });
    let dependencies = manifest.get("dependencies");
    let mut names: Vec<String> = match dependencies {
        Some(PyValue::List(items)) => items
            .iter()
            .filter_map(|item| item.as_str())
            .filter_map(|value| {
                DEPENDENCY_NAME
                    .find(value)
                    .map(|found| found.as_str().to_owned())
            })
            .collect(),
        Some(PyValue::Dict(entries)) => entries.iter().map(|(key, _)| key.clone()).collect(),
        _ => Vec::new(),
    };
    names.sort();
    names.truncate(LIST_LIMIT);

    let mut exports = Map::new();
    if let Some(PyValue::Dict(entries)) = config.get("exports") {
        for (key, value) in entries {
            let shown = match value {
                PyValue::Str(text) if text::char_len(text) <= 4096 => json!(text),
                _ => Value::Null,
            };
            exports.insert(text::char_slice(key, 0, Some(4096)).to_owned(), shown);
        }
    }
    let mut scripts = Map::new();
    if let Some(PyValue::Dict(entries)) = config.get("scripts") {
        for (key, value) in entries {
            scripts.insert(
                text::char_slice(key, 0, Some(128)).to_owned(),
                json!(hex(value.py_str().as_bytes())),
            );
        }
    }
    json!({
        "name": name,
        "dependencies": names,
        "exports": exports,
        "scripts": scripts,
    })
}

fn text_facts(source: &str, result: &mut Map<String, Value>) {
    result.insert("ci".into(), ci_facts(source));
    result.insert("deployment".into(), deployment_facts(source));
    result.insert(
        "configured_grants".into(),
        Value::Array(configured_grants(source)),
    );
    // Only structural keys leave config parsing. Scripts are hashed, not run or returned;
    // configuration values may contain credentials.
    let Some(config) = parse_config(source) else {
        return;
    };
    if !config.is_dict() {
        return;
    }
    if config.get("kind").and_then(PyValue::as_str) == Some("ArchitectureContract") {
        match parse_contract(ContractInput::Text(source)) {
            Ok(contract) => {
                result.insert("contract".into(), contract.dump);
            }
            Err(_) => {
                result.insert("invalid_contract".into(), Value::Bool(true));
            }
        }
    }
    result.insert("manifest".into(), manifest_facts(&config));
    if let Some(PyValue::Dict(entries)) = config.get("scripts") {
        let commands: Vec<Value> = entries
            .iter()
            .filter_map(|(_, value)| value.as_str())
            .flat_map(quality_commands)
            .take(LIST_LIMIT)
            .map(|identifier| json!({"id": identifier, "line": 1}))
            .collect();
        if let Some(Value::Object(ci)) = result.get_mut("ci") {
            ci.insert("commands".into(), Value::Array(commands));
        }
    }
}

/// Python truthiness of a constant expression, for `assert <constant>`.
fn constant_is_truthy(test: &Expr) -> bool {
    match test {
        Expr::StringLiteral(text) => !text.value.to_str().is_empty(),
        Expr::BytesLiteral(bytes) => bytes.value.bytes().next().is_some(),
        Expr::NumberLiteral(number) => match &number.value {
            Number::Int(int) => int.to_string() != "0",
            Number::Float(float) => *float != 0.0,
            Number::Complex { real, imag } => *real != 0.0 || *imag != 0.0,
        },
        Expr::BooleanLiteral(flag) => flag.value,
        Expr::EllipsisLiteral(_) => true,
        _ => false,
    }
}

struct Counts {
    assertions: u64,
    skips: u64,
    vacuous: Vec<usize>,
}

fn python_counts(source: &str) -> Result<Option<Counts>, PyError> {
    let counted = pyast::with_module(source, |module| {
        let mut counts = Counts {
            assertions: 0,
            skips: 0,
            vacuous: Vec::new(),
        };
        for node in module.walk() {
            match node {
                Node::Stmt(Stmt::Assert(assert)) => {
                    counts.assertions += 1;
                    if constant_is_truthy(&assert.test) {
                        counts.vacuous.push(module.visit(node).line.unwrap_or(0));
                    }
                }
                Node::Expr(Expr::Call(call)) => {
                    let name = unparse(module.source(), &call.func);
                    if [".skip", ".skipif", ".xfail"]
                        .iter()
                        .any(|suffix| name.ends_with(suffix))
                    {
                        counts.skips += 1;
                    }
                    if name.starts_with("assert")
                        || name.starts_with("pytest.raises")
                        || name.contains(".assert")
                    {
                        counts.assertions += 1;
                    }
                }
                _ => {}
            }
        }
        counts
    });
    match counted {
        Ok(counts) => Ok(Some(counts)),
        Err(ParseFailure::Syntax(_)) => Ok(None),
        // `ast.parse` raises `RecursionError` here and nothing catches it.
        Err(ParseFailure::Recursion) => Err(PyError::new(
            "RecursionError",
            "maximum recursion depth exceeded during ast construction",
        )),
    }
}

fn script_counts(
    language: &str,
    data: &[u8],
    result: &mut Map<String, Value>,
) -> Result<(), PyError> {
    let tree = parse_tree(language, data)
        .map_err(|error| PyError::new("RuntimeError", error.to_string()))?;
    let root = tree.root_node();
    result.insert("parse_error".into(), Value::Bool(root.has_error()));
    let (mut assertions, mut skips, mut focused) = (0u64, 0u64, 0u64);
    let mut vacuous: Vec<Value> = Vec::new();
    let mut pending: Vec<SyntaxNode<'_>> = vec![root];
    while let Some(node) = pending.pop() {
        if node.kind() == "call_expression" {
            let function = node.child_by_field_name("function");
            let arguments = node.child_by_field_name("arguments");
            let name = match function {
                Some(found) => decode_utf8(&data[found.start_byte()..found.end_byte()])?,
                None => String::new(),
            };
            if name.starts_with("assert") || name.starts_with("expect") {
                assertions += 1;
            }
            if matches!(
                name.as_str(),
                "test.skip" | "it.skip" | "describe.skip" | "test.todo" | "it.todo"
            ) {
                skips += 1;
            }
            if matches!(name.as_str(), "test.only" | "it.only" | "describe.only") {
                focused += 1;
            }
            if matches!(name.as_str(), "assert" | "assert.ok")
                && let Some(arguments) = arguments
            {
                let mut cursor = arguments.walk();
                let first = arguments.named_children(&mut cursor).next();
                if first.is_some_and(|argument| argument.kind() == "true") {
                    vacuous.push(json!(node.start_position().row + 1));
                }
            }
        }
        let mut cursor = node.walk();
        let children: Vec<SyntaxNode<'_>> = node.named_children(&mut cursor).collect();
        pending.extend(children);
    }
    result.insert("test_assertions".into(), json!(assertions));
    result.insert("test_skips".into(), json!(skips));
    result.insert("test_focused".into(), json!(focused));
    result.insert("vacuous_assertions".into(), Value::Array(vacuous));
    Ok(())
}

/// `extract_facts(language, data, references)`.
pub fn extract_facts(language: &str, data: &[u8], references: &Value) -> Result<Value, PyError> {
    let source = decode_utf8(data)?;
    let lines = text::splitlines(&source);
    let mut result = Map::new();
    result.insert("version".into(), json!(2));
    result.insert("lines".into(), json!(lines.len()));
    result.insert(
        "nonblank_lines".into(),
        json!(
            lines
                .iter()
                .filter(|line| !text::strip(line).is_empty())
                .count()
        ),
    );
    result.insert("sha256".into(), json!(hex(data)));
    result.insert("imports".into(), get(references, "imports")?.clone());
    for key in ["test_assertions", "test_skips", "test_focused"] {
        result.insert(key.into(), json!(0));
    }
    result.insert("vacuous_assertions".into(), json!([]));
    if language == "text" {
        text_facts(&source, &mut result);
        return Ok(Value::Object(result));
    }
    result.insert("structure".into(), extract_structure(language, data)?);
    if language == "python" {
        match python_counts(&source)? {
            None => {
                result.insert("parse_error".into(), Value::Bool(true));
            }
            Some(counts) => {
                result.insert("test_assertions".into(), json!(counts.assertions));
                result.insert("test_skips".into(), json!(counts.skips));
                result.insert("vacuous_assertions".into(), json!(counts.vacuous));
            }
        }
    } else if matches!(language, "typescript" | "tsx" | "javascript") {
        script_counts(language, data, &mut result)?;
    } else if language == "rust" {
        rust_counts(data, &mut result)?;
    }
    Ok(Value::Object(result))
}

/// `artifact_role(path)`: what kind of governance or configuration file a path names, if any.
pub fn artifact_role(path: &str) -> Option<&'static str> {
    let name = lower_name(path);
    let name = name.as_str();
    if matches!(name, "agents.md" | "claude.md" | "gemini.md") {
        return Some("agent-instruction");
    }
    if ["docs/adr/", "docs/decisions/", "docs/architecture/"]
        .iter()
        .any(|prefix| path.starts_with(prefix))
        && name.ends_with(".md")
    {
        return Some(if path.contains("/adr/") || path.contains("/decisions/") {
            "architecture-decision"
        } else {
            "architecture-document"
        });
    }
    if path.starts_with(".github/workflows/")
        || matches!(name, ".gitlab-ci.yml" | "cloudbuild.yaml" | "jenkinsfile")
    {
        return Some("ci-configuration");
    }
    if name.starts_with("dockerfile")
        || matches!(
            name,
            "render.yaml"
                | "compose.yaml"
                | "docker-compose.yml"
                | "wrangler.json"
                | "wrangler.toml"
        )
        || name.ends_with(".tf")
    {
        return Some("deployment-configuration");
    }
    if matches!(name, "package.json" | "pyproject.toml" | "cargo.toml") {
        return Some("package-manifest");
    }
    None
}
