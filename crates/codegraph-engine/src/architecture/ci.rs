//! Bounded, data-only CI extraction: port of `architecture_ci.py`.
//! No repository command is ever executed; shell text is only split into words.

use super::yaml::{ConfigurationError, Node, configuration_documents};
use crate::py_regex::{regex, search};
use crate::py_shlex;
use crate::text;
use fancy_regex::Regex;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::sync::LazyLock;

static CONTINUATION: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\\\r?\n").expect("valid"));
static ENV_ASSIGNMENT: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\A[A-Za-z_][A-Za-z0-9_]*=.*\z").expect("valid"));
static SCRIPT_NAME: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\A[A-Za-z][A-Za-z0-9:_-]{0,127}\z").expect("valid"));
static SCRIPT_PATH: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"\A(?:tools|scripts|tests)/[A-Za-z0-9_./-]+\.(?:py|mjs|js)\z")
        .expect("valid")
});
static CI_KEYS: LazyLock<Regex> =
    LazyLock::new(|| regex(r"(?m)^\s*(?:jobs|steps|stages|pipeline)\s*:"));

const LINE_LIMIT: usize = 2000;
const FACT_LIMIT: usize = 256;

/// `PurePosixPath(word.replace("\\", "/")).name`: the last component, without
/// empty or `.` components.
fn path_name(word: &str) -> String {
    let word = word.replace('\\', "/");
    word.split('/')
        .rfind(|part| !part.is_empty() && *part != ".")
        .unwrap_or("")
        .to_owned()
}

/// `quality_commands`: the recognised quality commands a shell text invokes, sorted.
pub fn quality_commands(source: &str) -> Vec<String> {
    let mut result: BTreeSet<String> = BTreeSet::new();
    let source = CONTINUATION.replace_all(source, " ");
    for line in text::splitlines(&source).into_iter().take(LINE_LIMIT) {
        let Ok(tokens) = py_shlex::split(line) else {
            continue;
        };
        let mut parts: Vec<Vec<String>> = vec![Vec::new()];
        for token in tokens {
            if !token.is_empty() && token.chars().all(|c| ";&|".contains(c)) {
                parts.push(Vec::new());
            } else if let Some(last) = parts.last_mut() {
                last.push(token);
            }
        }
        for mut words in parts {
            while words.first().is_some_and(|word| {
                ENV_ASSIGNMENT.is_match(word)
                    || matches!(word.as_str(), "env" | "time" | "npx" | "--yes")
            }) {
                words.remove(0);
            }
            if words.is_empty() {
                continue;
            }
            let lowered = text::lower(&path_name(&words[0]));
            let mut command = lowered.strip_suffix(".exe").unwrap_or(&lowered).to_owned();
            if matches!(command.as_str(), "python" | "python3")
                && words.len() > 2
                && words[1] == "-m"
            {
                command = words[2].clone();
                words.drain(..2);
            }
            match command.as_str() {
                "pytest" | "mypy" | "semgrep" | "compileall" | "build" | "tsc" | "vitest"
                | "eslint" | "playwright" => {
                    result.insert(command);
                }
                "ruff" if words.len() > 1 => {
                    if words[1] == "check" {
                        result.insert("ruff:check".into());
                    } else if words[1] == "format" && words.iter().any(|word| word == "--check") {
                        result.insert("ruff:format-check".into());
                    }
                }
                "prettier" if words.iter().any(|word| word == "--check") => {
                    result.insert("prettier:check".into());
                }
                "npm" | "npm.cmd" | "pnpm" | "yarn" if words.len() > 1 => {
                    if words.len() > 3 && matches!(words[1].as_str(), "--prefix" | "--dir" | "-C") {
                        let mut rest = vec![words[0].clone()];
                        rest.extend(words[3..].iter().cloned());
                        words = rest;
                    }
                    let script = if words[1] == "run" && words.len() > 2 {
                        &words[2]
                    } else {
                        &words[1]
                    };
                    if SCRIPT_NAME.is_match(script) {
                        result.insert(format!("npm:{script}"));
                    }
                }
                "python" | "python3" | "node"
                    if words.len() > 1 && SCRIPT_PATH.is_match(&words[1]) =>
                {
                    result.insert(format!("{command}:{}", words[1]));
                }
                _ => {}
            }
        }
    }
    result.into_iter().collect()
}

/// The mapping of a node, keyed by scalar keys; a repeated key is an error.
fn mapping(node: Option<&Node>) -> Result<Vec<(&str, &Node)>, ConfigurationError> {
    let Some(Node::Mapping { pairs, .. }) = node else {
        return Ok(Vec::new());
    };
    let mut found: Vec<(&str, &Node)> = Vec::new();
    let mut scalar_pairs = 0;
    for (key, value) in pairs {
        if let Node::Scalar { value: name, .. } = key {
            scalar_pairs += 1;
            match found.iter_mut().find(|(existing, _)| existing == name) {
                Some(slot) => slot.1 = value,
                None => found.push((name, value)),
            }
        }
    }
    if found.len() != scalar_pairs {
        return Err(ConfigurationError("Duplicate configuration key".into()));
    }
    Ok(found)
}

fn get<'a>(values: &[(&str, &'a Node)], name: &str) -> Option<&'a Node> {
    values
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, node)| *node)
}

struct Collected {
    commands: Vec<Value>,
    risks: Vec<Value>,
}

fn inspect(node: &Node, collected: &mut Collected) -> Result<(), ConfigurationError> {
    let values = mapping(Some(node))?;
    for name in ["run", "script"] {
        let found = get(&values, name);
        let bodies: Vec<&Node> = match found {
            Some(Node::Sequence { items, .. }) => items.iter().collect(),
            Some(other) => vec![other],
            None => Vec::new(),
        };
        for body in bodies {
            if let Node::Scalar { value, line } = body {
                for identifier in quality_commands(value) {
                    collected
                        .commands
                        .push(json!({"id": identifier, "line": line}));
                }
            }
        }
    }
    for (name, disabled) in [
        ("continue-on-error", "true"),
        ("allow_failure", "true"),
        ("if", "false"),
        ("if", "${{false}}"),
    ] {
        if let Some(Node::Scalar { value, line }) = get(&values, name) {
            let squeezed: String = value.chars().filter(|c| !text::is_space(*c)).collect();
            if text::lower(&squeezed) == disabled {
                collected
                    .risks
                    .push(json!({"kind": format!("{name}:{disabled}"), "line": line}));
            }
        }
    }
    if let Some(Node::Sequence { items, .. }) = get(&values, "steps") {
        for step in items {
            inspect(step, collected)?;
        }
    }
    Ok(())
}

fn partial_facts() -> Value {
    json!({"commands": [], "bypasses": [], "partial": true})
}

/// `ci_facts`: recognised quality commands and literal bypasses of one CI configuration.
pub fn ci_facts(source: &str) -> Value {
    if !search(&CI_KEYS, source) {
        return json!({});
    }
    let Ok(roots) = configuration_documents(source) else {
        return partial_facts();
    };
    let [root] = roots.as_slice() else {
        return partial_facts();
    };
    let mut collected = Collected {
        commands: Vec::new(),
        risks: Vec::new(),
    };
    let outcome = (|| -> Result<(), ConfigurationError> {
        let top = mapping(Some(root))?;
        inspect(root, &mut collected)?;
        for (_, node) in mapping(get(&top, "jobs"))? {
            inspect(node, &mut collected)?;
        }
        if get(&top, "stages").is_some() {
            for (_, node) in &top {
                if matches!(node, Node::Mapping { .. }) {
                    inspect(node, &mut collected)?;
                }
            }
        }
        Ok(())
    })();
    if outcome.is_err() {
        return partial_facts();
    }
    let partial = collected.commands.len() > FACT_LIMIT || collected.risks.len() > FACT_LIMIT;
    json!({
        "commands": collected.commands.iter().take(FACT_LIMIT).collect::<Vec<_>>(),
        "bypasses": collected.risks.iter().take(FACT_LIMIT).collect::<Vec<_>>(),
        "partial": partial,
    })
}
