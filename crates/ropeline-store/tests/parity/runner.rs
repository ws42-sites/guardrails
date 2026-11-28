//! Replays a recorded case through the Rust crate, step by step.

// Two test crates share this module; each uses a different part of it.
#![allow(dead_code)]

use super::value::{canonical, dump_state, schema_matches, sha256_hex, to_json};
use ropeline_store as store;
use ropeline_store::StoreError;
use rusqlite::types::Value as Sql;
use rusqlite::{Connection, params_from_iter};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

type Outcome = Result<Value, StoreError>;

/// The recorder's `{"value": ...}` or `{"error": ..., "message": ...}`.
pub fn outcome_json(result: Outcome) -> Value {
    match result {
        Ok(value) => json!({ "value": value }),
        Err(error) => json!({ "error": error.kind, "message": error.message }),
    }
}

pub struct Run {
    pub folder: PathBuf,
    connections: HashMap<String, Connection>,
}

fn name_of(step: &Value) -> String {
    step.get("as")
        .and_then(Value::as_str)
        .unwrap_or("main")
        .to_owned()
}

fn to_sql(value: &Value) -> Sql {
    match value {
        Value::Null => Sql::Null,
        Value::Bool(flag) => Sql::Integer(i64::from(*flag)),
        Value::Number(number) => number
            .as_i64()
            .map_or_else(|| Sql::Real(number.as_f64().expect("number")), Sql::Integer),
        Value::String(text) => Sql::Text(text.clone()),
        other => panic!("unsupported parameter {other}"),
    }
}

/// What Python's `sqlite3.connect` gives: foreign keys off. The bundled SQLite that
/// rusqlite builds turns them on by default, so the harness switches them off to match.
fn plain_connection(path: &Path) -> rusqlite::Result<Connection> {
    let connection = Connection::open(path)?;
    connection.execute_batch("PRAGMA foreign_keys=OFF")?;
    Ok(connection)
}

impl Run {
    pub fn new(folder: PathBuf) -> Self {
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).expect("case folder");
        Self {
            folder,
            connections: HashMap::new(),
        }
    }

    pub fn path(&self, name: &str) -> PathBuf {
        if name.contains('/') || name.contains('.') {
            self.folder.join(name)
        } else {
            self.folder.join(format!("{name}.sqlite"))
        }
    }

    pub fn main_path(&self) -> PathBuf {
        self.path("main")
    }

    /// Python's recorded dump of an indexed database, replayed as text.
    pub fn load_dump(&self, statements: &[Value], user_version: i64) {
        let script: Vec<&str> = statements.iter().filter_map(Value::as_str).collect();
        let raw = plain_connection(&self.main_path()).expect("open base");
        raw.execute_batch(&script.join("\n")).expect("replay dump");
        raw.execute_batch(&format!("PRAGMA user_version={user_version}"))
            .expect("user version");
    }

    pub fn open_store(&mut self, name: &str) -> Result<(), StoreError> {
        let connection = store::connect(&self.main_path())?;
        self.connections.insert(name.to_owned(), connection);
        Ok(())
    }

    fn live(&mut self, step: &Value) -> &mut Connection {
        let name = name_of(step);
        self.connections.get_mut(&name).expect("open connection")
    }

    fn resolve(connection: &Connection, argument: &Value) -> Sql {
        if let Some(text) = argument.get("sql").and_then(Value::as_str) {
            return connection
                .query_row(text, [], |row| row.get::<_, Sql>(0))
                .expect("lookup");
        }
        if let Some(text) = argument.get("sha256").and_then(Value::as_str) {
            return Sql::Text(sha256_hex(text));
        }
        to_sql(argument)
    }

    fn connect(&mut self, step: &Value) -> Outcome {
        let db = step.get("db").and_then(Value::as_str).unwrap_or("main");
        let connection = store::connect(&self.path(db))?;
        let mut pragmas = Map::new();
        for name in [
            "user_version",
            "journal_mode",
            "foreign_keys",
            "busy_timeout",
        ] {
            let value = connection.query_row(&format!("PRAGMA {name}"), [], |row| {
                Ok(to_json(row.get_ref(0)?))
            })?;
            pragmas.insert(name.to_owned(), value);
        }
        self.connections.insert(name_of(step), connection);
        Ok(Value::Object(pragmas))
    }

    fn sql(&mut self, step: &Value) -> Outcome {
        let connection = self.live(step);
        let tx = connection.transaction()?;
        for statement in step["statements"].as_array().expect("statements") {
            let (text, params): (&str, Vec<Sql>) = match statement {
                Value::String(text) => (text, Vec::new()),
                other => (
                    other["text"].as_str().expect("text"),
                    other
                        .get("params")
                        .and_then(Value::as_array)
                        .map(|items| items.iter().map(|item| Self::resolve(&tx, item)).collect())
                        .unwrap_or_default(),
                ),
            };
            tx.execute(text, params_from_iter(params))?;
        }
        tx.commit()?;
        Ok(Value::Null)
    }

    fn call(&mut self, step: &Value) -> Outcome {
        let function = step["fn"].as_str().expect("fn");
        let connection = self.live(step);
        match function {
            "migrate" => store::migrate(connection)?,
            "rollback" => store::rollback(connection)?,
            "rollback_waypoints" => store::rollback_waypoints(connection)?,
            "delete_snapshot" | "collect_unused" => {
                // Python's callers own the commit, so a failure undoes the whole call.
                let tx = connection.transaction()?;
                if function == "delete_snapshot" {
                    let id = step["args"][0].as_i64().expect("snapshot id");
                    store::delete_snapshot(&tx, id)?;
                } else {
                    store::collect_unused(&tx)?;
                }
                tx.commit()?;
            }
            other => panic!("unknown call {other}"),
        }
        Ok(Value::Null)
    }

    fn query(connection: &Connection, step: &Value) -> Outcome {
        let arguments = step["args"].as_array().expect("args");
        let text = arguments[0].as_str().expect("query text");
        let params: Vec<Sql> = arguments[1..]
            .iter()
            .map(|item| Self::resolve(connection, item))
            .collect();
        let mut statement = connection.prepare(text)?;
        let width = statement.column_count();
        let mut rows = statement.query(params_from_iter(params))?;
        let mut result = Vec::new();
        while let Some(row) = rows.next()? {
            let mut items = Vec::new();
            for index in 0..width {
                items.push(to_json(row.get_ref(index)?));
            }
            result.push(Value::Array(items));
        }
        Ok(Value::Array(result))
    }

    fn read(&mut self, step: &Value) -> Outcome {
        let function = step["fn"].as_str().expect("fn");
        let connection = self.live(step);
        let arguments = step["args"].as_array().expect("args");
        let value = match function {
            "query" => Self::query(connection, step)?,
            "analysis" => {
                let Sql::Text(key) = Self::resolve(connection, &arguments[0]) else {
                    panic!("analysis key must be text");
                };
                match store::analysis(connection, &key)? {
                    Some(record) => record.to_json().expect("representable analysis"),
                    None => Value::Null,
                }
            }
            "summary" => {
                let repository = arguments[0].as_str().expect("repository");
                let summary = store::summary(connection, repository)?;
                json!({
                    "unique_analyses": summary.unique_analyses,
                    "unique_source_bytes": summary.unique_source_bytes,
                    "stored_symbols": summary.stored_symbols,
                    "manifest_entries": summary.manifest_entries,
                })
            }
            "waypoint_records" => {
                let snapshot = arguments[0].as_i64().expect("snapshot");
                let path = arguments.get(1).and_then(Value::as_str);
                Value::Array(store::waypoint_records(connection, snapshot, path)?)
            }
            other => panic!("unknown read {other}"),
        };
        if step.get("digest").and_then(Value::as_bool) == Some(true) {
            let text = canonical(&value);
            return Ok(json!({ "length": text.chars().count(), "sha256": sha256_hex(&text) }));
        }
        Ok(value)
    }

    /// Rewrite a cached payload and keep its checksum honest, as the recorder does.
    fn edit_payload(&mut self, step: &Value) -> Outcome {
        let connection = self.live(step);
        let Sql::Text(key) = Self::resolve(connection, &step["id"]) else {
            panic!("payload id must be text");
        };
        let stored: String = connection.query_row(
            "SELECT payload FROM waypoint_cache WHERE id=?",
            [&key],
            |row| row.get(0),
        )?;
        let mut payload: Value = serde_json::from_str(&stored).expect("stored payload");
        let object = payload.as_object_mut().expect("payload object");
        for (name, value) in step["set"].as_object().expect("set") {
            object.insert(name.clone(), value.clone());
        }
        for name in step["remove"].as_array().expect("remove") {
            object.shift_remove(name.as_str().expect("name"));
        }
        let text = canonical(&payload);
        let tx = connection.transaction()?;
        tx.execute(
            "UPDATE waypoint_cache SET payload=?,payload_hash=? WHERE id=?",
            rusqlite::params![text, sha256_hex(&text), key],
        )?;
        tx.commit()?;
        Ok(Value::Null)
    }

    fn absent(&mut self, step: &Value) -> Outcome {
        self.connections.clear();
        let needle = step["needle"].as_str().expect("needle").as_bytes();
        let mut found = false;
        for entry in std::fs::read_dir(&self.folder).expect("folder") {
            let path = entry.expect("entry").path();
            if path.is_file() {
                let data = std::fs::read(&path).expect("read");
                found |= data.windows(needle.len()).any(|window| window == needle);
            }
        }
        Ok(json!({ "found": found }))
    }

    pub fn step(&mut self, step: &Value) -> Value {
        let operation = step["op"].as_str().expect("op");
        let result = match operation {
            "raw" => self.raw(step),
            "file" => std::fs::write(
                self.path(step.get("db").and_then(Value::as_str).unwrap_or("main")),
                step["text"].as_str().expect("text").as_bytes(),
            )
            .map(|()| Value::Null)
            .map_err(|error| StoreError::new("OSError", error.to_string())),
            "dir" => std::fs::create_dir(self.main_path())
                .map(|()| Value::Null)
                .map_err(|error| StoreError::new("OSError", error.to_string())),
            "connect" => self.connect(step),
            "close" => {
                self.connections.remove(&name_of(step));
                Ok(Value::Null)
            }
            "sql" => self.sql(step),
            "call" => self.call(step),
            "read" => self.read(step),
            "editPayload" => self.edit_payload(step),
            "absent" => self.absent(step),
            "schemaNames" => {
                let words: Vec<String> = step["words"]
                    .as_array()
                    .expect("words")
                    .iter()
                    .filter_map(|word| word.as_str().map(str::to_owned))
                    .collect();
                Ok(schema_matches(&self.main_path(), &words))
            }
            other => panic!("unknown operation {other}"),
        };
        outcome_json(result)
    }

    fn raw(&mut self, step: &Value) -> Outcome {
        let db = step.get("db").and_then(Value::as_str).unwrap_or("main");
        let connection = plain_connection(&self.path(db))?;
        connection.execute_batch(step["script"].as_str().expect("script"))?;
        Ok(Value::Null)
    }

    pub fn state(&self) -> super::value::State {
        dump_state(&self.main_path())
    }

    pub fn close_all(&mut self) {
        self.connections.clear();
    }
}

pub fn case_folder(kind: &str, section: &str, case: &str) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("store-parity")
        .join(kind)
        .join(section)
        .join(case)
}
