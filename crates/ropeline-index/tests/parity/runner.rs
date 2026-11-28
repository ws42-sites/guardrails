//! Replays a recorded case through the crate, step by step.

use super::args::{CountingClock, content_bytes, index_options, source_files};
use super::value::{sha256_hex, to_json};
use ropeline_engine::text::python_json as canonical;
use ropeline_index::{
    Clock, Environment, MonotonicClock, RepoOptions, SourceError, cached_git_analyses_in,
    index_files_in, index_repo_in,
};
use rusqlite::types::Value as Sql;
use rusqlite::{Connection, params_from_iter};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub type Outcome = Result<Value, SourceError>;

/// Operating-system messages and paths belong to the platform, so both sides drop them.
const PLACEHOLDER: &str = "<operating system message>";
const OS_ERRORS: [&str; 5] = [
    "FileNotFoundError",
    "NotADirectoryError",
    "PermissionError",
    "IsADirectoryError",
    "OSError",
];

pub fn outcome_json(result: Outcome) -> Value {
    match result {
        Ok(value) => json!({ "value": value }),
        Err(error) if OS_ERRORS.contains(&error.kind) => {
            json!({ "error": error.kind, "message": PLACEHOLDER })
        }
        Err(error) => json!({ "error": error.kind, "message": error.message }),
    }
}

/// The pinned signatures the recorder and this crate agree on.
pub struct Signatures {
    pub oracle: String,
    pub other: String,
}

pub struct Run {
    pub folder: PathBuf,
    pub repo: PathBuf,
    pub db: PathBuf,
    signatures: std::sync::Arc<Signatures>,
}

/// A plain connection like Python's raw `sqlite3.connect`: foreign keys off.
pub fn plain(path: &Path) -> rusqlite::Result<Connection> {
    let connection = Connection::open(path)?;
    connection.execute_batch("PRAGMA foreign_keys=OFF")?;
    Ok(connection)
}

pub fn to_sql_value(value: &Value) -> Sql {
    to_sql(value)
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

fn runtime(message: &str) -> SourceError {
    SourceError::new("RuntimeError", message)
}

impl Run {
    pub fn new(folder: PathBuf, signatures: std::sync::Arc<Signatures>) -> Self {
        let _ = std::fs::remove_dir_all(&folder);
        let repo = folder.join("repo");
        std::fs::create_dir_all(&repo).expect("case folder");
        let db = folder.join("graph.sqlite");
        Self {
            folder,
            repo,
            db,
            signatures,
        }
    }

    fn target(&self, relative: &str) -> PathBuf {
        let mut path = self.repo.clone();
        for part in relative.split('/') {
            path.push(part);
        }
        path
    }

    fn write(&self, step: &Value) {
        let mut files = Vec::new();
        if let Some(name) = step.get("fixture").and_then(Value::as_str) {
            files.extend(super::args::manifest(name));
        }
        if let Some(given) = step.get("files").and_then(Value::as_object) {
            files.extend(given.iter().map(|(p, c)| (p.clone(), c.clone())));
        }
        for (relative, spec) in files {
            let path = self.target(&relative);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("folder");
            std::fs::write(path, content_bytes(&spec)).expect("write");
        }
    }

    fn remove(&self, step: &Value) {
        for relative in step["paths"].as_array().expect("paths") {
            let path = self.target(relative.as_str().expect("path"));
            if path.is_dir() {
                std::fs::remove_dir_all(path).expect("remove folder");
            } else {
                std::fs::remove_file(path).expect("remove file");
            }
        }
    }

    fn mkdir(&self, step: &Value) {
        for relative in step["paths"].as_array().expect("paths") {
            std::fs::create_dir_all(self.target(relative.as_str().expect("path"))).expect("mkdir");
        }
    }

    pub fn resolve(&self, argument: &Value) -> Value {
        if let Some(sql) = argument.get("sql").and_then(Value::as_str) {
            let connection = plain(&self.db).expect("open");
            return connection
                .query_row(sql, [], |row| Ok(to_json(row.get_ref(0)?)))
                .expect("lookup");
        }
        if let Some(sql) = argument.get("sqlRows").and_then(Value::as_str) {
            let connection = plain(&self.db).expect("open");
            let mut statement = connection.prepare(sql).expect("prepare");
            let width = statement.column_count();
            let rows = statement
                .query_map([], |row| {
                    Ok(Value::Array(
                        (0..width)
                            .map(|at| to_json(row.get_ref_unwrap(at)))
                            .collect(),
                    ))
                })
                .expect("rows")
                .map(|row| row.expect("row"))
                .collect();
            return Value::Array(rows);
        }
        if let Some(spec) = argument.get("generate") {
            let language = spec["language"].as_str().expect("language");
            let count = spec["count"].as_u64().expect("count");
            return Value::Array(
                (0..count)
                    .map(|number| json!([format!("{number:040x}"), language]))
                    .collect(),
            );
        }
        argument.clone()
    }

    fn signature<'a>(&'a self, step: &Value) -> &'a str {
        if step.get("signature").and_then(Value::as_str) == Some("other") {
            &self.signatures.other
        } else {
            &self.signatures.oracle
        }
    }

    fn index_files(&self, step: &Value) -> Outcome {
        let options = index_options(step);
        let files = source_files(step, &|spec| self.resolve(spec));
        let clock: Box<dyn Clock> = match step.get("clock") {
            Some(spec) => Box::new(CountingClock::new(spec["step"].as_f64().expect("step"))),
            None => Box::new(MonotonicClock::new()),
        };
        let mut refuse = || -> Result<(), SourceError> { Err(runtime("before commit refused")) };
        let before_commit: Option<&mut dyn FnMut() -> Result<(), SourceError>> =
            if step.get("beforeCommit").and_then(Value::as_str) == Some("fail") {
                Some(&mut refuse)
            } else {
                None
            };
        let repository = step
            .get("repo")
            .and_then(Value::as_str)
            .unwrap_or("owner/repo");
        let result = index_files_in(
            files,
            &self.db,
            repository,
            &options,
            Environment {
                clock: clock.as_ref(),
                signature: self.signature(step),
                before_commit,
            },
        )?;
        Ok(result.to_json())
    }

    fn index_repo(&self, step: &Value) -> Outcome {
        let mut options = RepoOptions {
            index: index_options(step),
            ..RepoOptions::default()
        };
        if let Some(given) = step.get("options").and_then(Value::as_object) {
            if let Some(flag) = given.get("include_dist").and_then(Value::as_bool) {
                options.include_dist = flag;
            }
            if let Some(size) = given.get("max_file_bytes").and_then(Value::as_i64) {
                options.max_file_bytes = size;
            }
        }
        let clock: Box<dyn Clock> = match step.get("clock") {
            Some(spec) => Box::new(CountingClock::new(spec["step"].as_f64().expect("step"))),
            None => Box::new(MonotonicClock::new()),
        };
        let mut refuse = || -> Result<(), SourceError> { Err(runtime("before commit refused")) };
        let before_commit: Option<&mut dyn FnMut() -> Result<(), SourceError>> =
            if step.get("beforeCommit").and_then(Value::as_str) == Some("fail") {
                Some(&mut refuse)
            } else {
                None
            };
        let root = match step.get("root").and_then(Value::as_str) {
            Some(relative) => self.target(relative),
            None => self.repo.clone(),
        };
        let repository = step
            .get("repo")
            .and_then(Value::as_str)
            .unwrap_or("owner/repo");
        let result = index_repo_in(
            &root,
            &self.db,
            Some(repository),
            options,
            Environment {
                clock: clock.as_ref(),
                signature: self.signature(step),
                before_commit,
            },
        )?;
        Ok(result.to_json())
    }

    pub fn cached(&self, step: &Value) -> Outcome {
        let objects: Vec<(String, String)> = self
            .resolve(&step["args"][0])
            .as_array()
            .expect("objects")
            .iter()
            .map(|pair| {
                (
                    pair[0].as_str().expect("oid").to_owned(),
                    pair[1].as_str().expect("language").to_owned(),
                )
            })
            .collect();
        let found = cached_git_analyses_in(&self.db, &objects, self.signature(step))?;
        Ok(Value::Array(
            found
                .iter()
                .map(|((oid, language), value)| json!([oid, language, value.to_json()]))
                .collect(),
        ))
    }

    fn sql(&self, step: &Value) -> Outcome {
        let mut connection = plain(&self.db).expect("open");
        let tx = connection.transaction().map_err(SourceError::from)?;
        for statement in step["statements"].as_array().expect("statements") {
            let (text, params): (&str, Vec<Sql>) = match statement {
                Value::String(text) => (text, Vec::new()),
                other => (
                    other["text"].as_str().expect("text"),
                    other
                        .get("params")
                        .and_then(Value::as_array)
                        .map(|items| items.iter().map(|i| to_sql(&self.resolve(i))).collect())
                        .unwrap_or_default(),
                ),
            };
            tx.execute(text, params_from_iter(params))
                .map_err(SourceError::from)?;
        }
        tx.commit().map_err(SourceError::from)?;
        Ok(Value::Null)
    }

    fn edit_payload(&self, step: &Value) -> Outcome {
        let mut connection = plain(&self.db).expect("open");
        let tx = connection.transaction().map_err(SourceError::from)?;
        let key: String = tx
            .query_row(step["select"].as_str().expect("select"), [], |r| r.get(0))
            .map_err(SourceError::from)?;
        let payload: String = tx
            .query_row(
                "SELECT payload FROM waypoint_cache WHERE id=?",
                [&key],
                |r| r.get(0),
            )
            .map_err(SourceError::from)?;
        let mut value: Value = serde_json::from_str(&payload).expect("payload");
        if let Some(changes) = step.get("set").and_then(Value::as_object) {
            for (name, item) in changes {
                value[name] = self.resolve(item);
            }
        }
        for name in step
            .get("remove")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            value
                .as_object_mut()
                .expect("object")
                .remove(name.as_str().expect("name"));
        }
        let text = canonical(&value);
        tx.execute(
            "UPDATE waypoint_cache SET payload=?,payload_hash=? WHERE id=?",
            (&text, sha256_hex(&text), &key),
        )
        .map_err(SourceError::from)?;
        tx.commit().map_err(SourceError::from)?;
        Ok(Value::Null)
    }

    fn absent(&self, step: &Value) -> Outcome {
        let needle = step["needle"].as_str().expect("needle").as_bytes().to_vec();
        let mut names: Vec<PathBuf> = std::fs::read_dir(&self.folder)
            .expect("folder")
            .map(|entry| entry.expect("entry").path())
            .filter(|path| {
                path.is_file()
                    && path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with("graph.sqlite"))
            })
            .collect();
        names.sort();
        let data: Vec<u8> = names
            .iter()
            .flat_map(|path| std::fs::read(path).expect("read"))
            .collect();
        Ok(json!({ "found": data.windows(needle.len()).any(|w| w == needle) }))
    }

    fn count(&self, step: &Value) -> Outcome {
        if !self.db.exists() {
            return Ok(Value::Null);
        }
        let connection = plain(&self.db).expect("open");
        let number: i64 = connection
            .query_row(step["sql"].as_str().expect("sql"), [], |row| row.get(0))
            .map_err(SourceError::from)?;
        Ok(json!(number))
    }

    pub fn step(&mut self, step: &Value) -> Outcome {
        let mut result = match step["op"].as_str().expect("op") {
            "write" => {
                self.write(step);
                Ok(Value::Null)
            }
            "remove" => {
                self.remove(step);
                Ok(Value::Null)
            }
            "mkdir" => {
                self.mkdir(step);
                Ok(Value::Null)
            }
            "indexFiles" => self.index_files(step),
            "indexRepo" => self.index_repo(step),
            "read" => self.read(step),
            "prune" => self.prune(step),
            "sql" => self.sql(step),
            "editPayload" => self.edit_payload(step),
            "absent" => self.absent(step),
            "count" => self.count(step),
            other => panic!("unknown operation {other}"),
        };
        result = result.map(super::reads::normalise);
        if step.get("digest").and_then(Value::as_bool) == Some(true)
            && let Ok(value) = &result
        {
            let text = canonical(value);
            result = Ok(json!({ "length": text.chars().count(), "sha256": sha256_hex(&text) }));
        }
        result
    }
}
