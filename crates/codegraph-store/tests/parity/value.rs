//! Turning database contents into the JSON shapes `tools/record_store_parity.py` writes.

#![allow(dead_code)]

use rusqlite::types::ValueRef;
use rusqlite::{Connection, params};
use serde_json::{Map, Value, json};
use std::path::Path;

pub fn to_json(value: ValueRef<'_>) -> Value {
    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(number) => json!(number),
        ValueRef::Real(number) => json!(number),
        ValueRef::Text(bytes) => Value::String(String::from_utf8_lossy(bytes).into_owned()),
        ValueRef::Blob(bytes) => {
            let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
            json!({ "blob": hex })
        }
    }
}

/// `json.dumps(sort_keys=True, separators=(",", ":"))` for the ASCII values the cases use.
pub fn canonical(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .into_iter()
                .map(|key| {
                    let name = serde_json::to_string(key).expect("key");
                    format!("{name}:{}", canonical(&map[key]))
                })
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(canonical).collect();
            format!("[{}]", parts.join(","))
        }
        other => serde_json::to_string(other).expect("scalar"),
    }
}

pub fn sha256_hex(text: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// A recorded `indexed_at` default is the instant of the run, not part of the contract.
fn column_value(column: &str, value: ValueRef<'_>) -> Value {
    if column == "indexed_at"
        && let ValueRef::Text(bytes) = value
        && is_timestamp(bytes)
    {
        return Value::String("<timestamp>".into());
    }
    to_json(value)
}

fn is_timestamp(bytes: &[u8]) -> bool {
    const SHAPE: &[u8; 19] = b"dddd-dd-dd dd:dd:dd";
    bytes.len() == 19
        && bytes.iter().zip(SHAPE).all(|(byte, shape)| match shape {
            b'd' => byte.is_ascii_digit(),
            other => byte == other,
        })
}

pub struct State {
    pub schema: Value,
    /// Everything else, in the recorder's shape minus `schemaId`.
    pub rest: Value,
}

/// Schema, pragmas and every table in rowid order, read through a plain connection.
pub fn dump_state(path: &Path) -> State {
    let connection = Connection::open(path).expect("open for dump");
    let mut statement = connection
        .prepare(
            "SELECT type,name,tbl_name,sql FROM sqlite_master \
             WHERE name NOT LIKE 'sqlite_%' ORDER BY type,name",
        )
        .expect("schema query");
    let schema: Vec<Value> = statement
        .query_map([], |row| {
            Ok(Value::Array(
                (0..4)
                    .map(|index| to_json(row.get_ref_unwrap(index)))
                    .collect(),
            ))
        })
        .expect("schema rows")
        .map(|row| row.expect("schema row"))
        .collect();
    let names: Vec<String> = connection
        .prepare(
            "SELECT name FROM sqlite_master WHERE type='table' \
             AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .expect("table names")
        .query_map([], |row| row.get(0))
        .expect("names")
        .map(|name| name.expect("name"))
        .collect();
    let mut tables = Map::new();
    for name in names {
        let columns: Vec<String> = connection
            .prepare(&format!("PRAGMA table_info(\"{name}\")"))
            .expect("table_info")
            .query_map([], |row| row.get(1))
            .expect("columns")
            .map(|column| column.expect("column"))
            .collect();
        let mut rows = Vec::new();
        let mut select = connection
            .prepare(&format!("SELECT * FROM \"{name}\" ORDER BY rowid"))
            .expect("select");
        let mut cursor = select.query([]).expect("rows");
        while let Some(row) = cursor.next().expect("next") {
            rows.push(Value::Array(
                columns
                    .iter()
                    .enumerate()
                    .map(|(index, column)| column_value(column, row.get_ref_unwrap(index)))
                    .collect(),
            ));
        }
        tables.insert(name, json!({ "columns": columns, "rows": rows }));
    }
    let integrity: String = connection
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .expect("integrity");
    let journal_mode: String = connection
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .expect("journal mode");
    let user_version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("user version");
    State {
        schema: Value::Array(schema),
        rest: json!({
            "integrity": integrity,
            "journal_mode": journal_mode,
            "tables": Value::Object(tables),
            "user_version": user_version,
        }),
    }
}

/// Table and column names that look like they could hold a credential.
pub fn schema_matches(path: &Path, words: &[String]) -> Value {
    let connection = Connection::open(path).expect("open for names");
    let mut names: Vec<String> = Vec::new();
    for sql in [
        "SELECT name FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
        "SELECT p.name FROM sqlite_master m, pragma_table_info(m.name) p WHERE m.type='table'",
    ] {
        let mut statement = connection.prepare(sql).expect("names query");
        names.extend(
            statement
                .query_map(params![], |row| row.get::<_, String>(0))
                .expect("names")
                .map(|name| name.expect("name")),
        );
    }
    let mut matches: Vec<String> = names
        .into_iter()
        .filter(|name| words.iter().any(|word| name.to_lowercase().contains(word)))
        .collect();
    matches.sort();
    matches.dedup();
    json!({ "matches": matches })
}

/// Compare a replayed database with the recorded state, reporting only what differs.
pub fn compare_state(
    label: &str,
    state: &State,
    expected: &Value,
    schemas: &Value,
    problems: &mut Vec<String>,
) {
    let id = expected["schemaId"].as_str().expect("schemaId");
    if state.schema != schemas[id] {
        problems.push(format!(
            "{label}: schema differs
  rust   {}
  python {}",
            state.schema, schemas[id]
        ));
    }
    let mut want = expected.clone();
    want.as_object_mut().expect("state").remove("schemaId");
    if state.rest != want {
        let mut detail = String::new();
        for key in ["integrity", "journal_mode", "user_version"] {
            if state.rest[key] != want[key] {
                detail += &format!(
                    "
  {key}: rust {} python {}",
                    state.rest[key], want[key]
                );
            }
        }
        for (name, table) in want["tables"].as_object().expect("tables") {
            if state.rest["tables"][name] != *table {
                detail += &format!(
                    "
  table {name}:
    rust   {}
    python {table}",
                    state.rest["tables"][name]
                );
            }
        }
        problems.push(format!("{label}: state differs{detail}"));
    }
}
