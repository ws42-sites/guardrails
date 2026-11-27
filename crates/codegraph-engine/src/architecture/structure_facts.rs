//! Building blocks the Python and script extractors share: names, effects and relations.
//! They live apart from `structure.rs` so the extractors do not import their caller.

use super::PyError;
use crate::py_regex::{for_case_insensitive, regex, search};
use crate::text;
use fancy_regex::Regex;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::LazyLock;
use tree_sitter::Node;

/// The most characters of a node's source that structure facts keep.
pub const TEXT_LIMIT: usize = 512;

pub fn named_children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

/// `_descendants`: the node and every named node beneath it, depth first, in order.
pub fn descendants(node: Option<Node<'_>>) -> Vec<Node<'_>> {
    let mut found = Vec::new();
    let mut pending: Vec<Node<'_>> = node.into_iter().collect();
    while let Some(item) = pending.pop() {
        found.push(item);
        pending.extend(named_children(item).into_iter().rev());
    }
    found
}

/// `text(node)`: up to 512 characters of the node's source; empty for no node.
/// Python decodes the slice strictly, so invalid UTF-8 in a node is an error.
pub fn node_text(data: &[u8], node: Option<Node<'_>>) -> Result<String, PyError> {
    let Some(node) = node else {
        return Ok(String::new());
    };
    let decoded = super::decode_utf8(&data[node.start_byte()..node.end_byte()])?;
    Ok(text::char_slice(&decoded, 0, Some(TEXT_LIMIT)).to_owned())
}

/// `_identifier`: names that fit the character class, or `(dynamic)`.
pub fn identifier(value: &str) -> String {
    let count = value.chars().count();
    // The punctuation the Python character class lists besides `\w`.
    let allowed = |c: char| text::is_word(c) || ".$:/@<>[] |?,=-".contains(c);
    if (1..=256).contains(&count) && value.chars().all(allowed) {
        value.to_owned()
    } else {
        "(dynamic)".to_owned()
    }
}

/// `collections.Counter`: absent names count zero.
#[derive(Default)]
pub struct Counter(HashMap<String, usize>);

impl Counter {
    pub fn bump(&mut self, name: &str) {
        *self.0.entry(name.to_owned()).or_insert(0) += 1;
    }

    pub fn get(&self, name: &str) -> usize {
        self.0.get(name).copied().unwrap_or(0)
    }
}

/// `_effect(dependency, method)`.
pub fn effect(dependency: &str, method: &str) -> Option<&'static str> {
    let starts = |prefixes: &[&str]| prefixes.iter().any(|prefix| dependency.starts_with(prefix));
    if starts(&["sqlite3", "psycopg", "sqlalchemy", "pg", "@prisma/client"])
        && matches!(method, "connect" | "execute" | "query" | "transaction")
    {
        return Some("persistence-call");
    }
    if starts(&["kafkajs", "amqplib", "@google-cloud/pubsub"]) {
        if matches!(method, "publish" | "send" | "sendToQueue" | "emit") {
            return Some("event-producer");
        }
        if matches!(method, "subscribe" | "consume" | "on") {
            return Some("event-consumer");
        }
    }
    None
}

/// One effect record.
pub fn effect_fact(kind: &str, line: usize, dependency: &str, resolution: &str) -> Value {
    json!({
        "kind": kind,
        "line": line,
        "dependency": dependency,
        "resolution": resolution,
    })
}

static SELECT: LazyLock<Regex> = LazyLock::new(|| regex(r"(?i)\A\s*SELECT\b"));
static WRITE: LazyLock<Regex> = LazyLock::new(|| regex(r"(?i)\A\s*(?:INSERT|UPDATE|DELETE)\b"));
static OUTBOX: LazyLock<Regex> =
    LazyLock::new(|| regex(r"(?i)\A\s*INSERT\s+INTO\s+(?:[\w]+\.)?outbox\b"));
static CONFLICT: LazyLock<Regex> =
    LazyLock::new(|| regex(r"(?i)\bON\s+CONFLICT\b.*\bDO\s+NOTHING\b"));
static UNIQUE: LazyLock<Regex> = LazyLock::new(|| regex(r"(?i)\bUNIQUE\b|\bPRIMARY\s+KEY\b"));
static CREATE_TABLE: LazyLock<Regex> = LazyLock::new(|| regex(r"(?i)\A\s*CREATE\s+TABLE\b"));

/// `_sql_effects`: only structural flags; SQL values, identifiers and credentials stay out.
pub fn sql_effects(sql: &str, line: usize, dependency: &str) -> Vec<Value> {
    let sql = for_case_insensitive(sql);
    let mut kinds = Vec::new();
    if search(&SELECT, &sql) {
        kinds.push("database-read");
    }
    if search(&WRITE, &sql) {
        kinds.push("database-write");
    }
    if search(&OUTBOX, &sql) {
        kinds.push("outbox-insert");
    }
    if search(&CONFLICT, &sql) {
        kinds.push("conflict-guard");
    }
    if search(&UNIQUE, &sql) && search(&CREATE_TABLE, &sql) {
        kinds.push("durable-unique-key");
    }
    kinds
        .into_iter()
        .map(|kind| effect_fact(kind, line, dependency, "literal-sql-in-import-bound-call"))
        .collect()
}

pub fn relation(
    kind: &str,
    source: &str,
    target: &str,
    line: usize,
    resolution: &str,
    origin: &str,
) -> Value {
    json!({
        "kind": kind,
        "source": source,
        "target": target,
        "line": line,
        "resolution": resolution,
        "origin": origin,
    })
}
