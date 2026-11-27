//! Replays a recorded semantic-cache case through the Rust crate.

use super::runner::{Run, outcome_json};
use super::value::to_json;
use codegraph_store::{ComputationGuard, DEFAULT_MAX_ENTRIES, SemanticStore, StoreError, month_of};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// SQLite stops waiting for a held lock at the same instant as the deadline, so which
/// of the two errors comes back is settled by under a millisecond, in Python as here.
/// A step marked `contended` records only that the lock was refused.
fn contended(recorded: Value) -> Value {
    let refused = matches!(
        (recorded["error"].as_str(), recorded["message"].as_str()),
        (
            Some("TimeoutError"),
            Some("Semantic computation lock deadline exceeded")
        ) | (
            Some("SemanticCacheError"),
            Some("Semantic computation lock is unavailable")
        )
    );
    if refused {
        json!({ "refused": true })
    } else {
        recorded
    }
}

pub struct SemanticRun {
    base: Run,
    stores: HashMap<String, SemanticStore>,
    locks: HashMap<String, ComputationGuard>,
}

fn special(value: &Value) -> f64 {
    match value.as_str() {
        Some("nan") => f64::NAN,
        Some("inf") => f64::INFINITY,
        Some("-inf") => f64::NEG_INFINITY,
        Some(other) => panic!("unknown special {other}"),
        None => value.as_f64().expect("number"),
    }
}

fn build_vector(spec: &Value) -> Vec<f64> {
    if let Some(values) = spec.get("values").and_then(Value::as_array) {
        return values
            .iter()
            .map(|item| item.as_f64().expect("float"))
            .collect();
    }
    if let Some(length) = spec.get("length").and_then(Value::as_u64) {
        return vec![spec["fill"].as_f64().expect("fill"); usize::try_from(length).expect("len")];
    }
    if let Some(size) = spec.get("uniform").and_then(Value::as_u64) {
        let size = usize::try_from(size).expect("size");
        return vec![1.0 / (size as f64).sqrt(); size];
    }
    let mut vector = vec![0.0; 768];
    if spec.get("mixed").and_then(Value::as_bool) == Some(true) {
        vector[0] = (1.0f64 - 1e-10).sqrt();
        vector[1] = 1e-05;
        return vector;
    }
    let axis = usize::try_from(spec["axis"].as_u64().expect("axis")).expect("axis");
    vector[axis] = spec
        .get("scale")
        .map_or(1.0, |scale| scale.as_f64().expect("scale"));
    if let Some(set) = spec.get("set").and_then(Value::as_object) {
        for (index, value) in set {
            vector[index.parse::<usize>().expect("index")] = special(value);
        }
    }
    vector
}

fn name_of(step: &Value) -> String {
    step.get("as")
        .and_then(Value::as_str)
        .unwrap_or("main")
        .to_owned()
}

impl SemanticRun {
    pub fn new(folder: PathBuf) -> Self {
        Self {
            base: Run::new(folder),
            stores: HashMap::new(),
            locks: HashMap::new(),
        }
    }

    pub fn state(&self) -> super::value::State {
        self.base.state()
    }

    pub fn close_all(&mut self) {
        self.locks.clear();
        self.stores.clear();
        self.base.close_all();
    }

    fn store(&mut self, step: &Value) -> &mut SemanticStore {
        let name = name_of(step);
        self.stores.get_mut(&name).expect("open store")
    }

    fn open(&mut self, step: &Value) -> Result<Value, StoreError> {
        let db = step.get("db").and_then(Value::as_str).unwrap_or("main");
        let bound = step
            .get("maxEntries")
            .map_or(DEFAULT_MAX_ENTRIES, |value| value.as_i64().expect("bound"));
        let store = SemanticStore::open(&self.base.path(db), bound)?;
        self.stores.insert(name_of(step), store);
        Ok(Value::Null)
    }

    fn lock(&mut self, step: &Value) -> Result<Value, StoreError> {
        let seconds = step["seconds"].as_f64().expect("seconds");
        // A negative wait is a deadline already behind us.
        let now = Instant::now();
        let deadline = if seconds >= 0.0 {
            now + Duration::from_secs_f64(seconds)
        } else {
            now.checked_sub(Duration::from_secs_f64(-seconds))
                .unwrap_or(now)
        };
        let guard = self.store(step).computation(deadline)?;
        self.locks
            .insert(step["name"].as_str().expect("name").to_owned(), guard);
        Ok(Value::Null)
    }

    fn query(&self, step: &Value) -> Result<Value, StoreError> {
        let connection = Connection::open(self.base.main_path())?;
        let mut statement = connection.prepare(step["sql"].as_str().expect("sql"))?;
        let width = statement.column_count();
        let mut rows = statement.query([])?;
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

    pub fn step(&mut self, step: &Value) -> Value {
        let operation = step["op"].as_str().expect("op");
        let result = match operation {
            "open" => self.open(step),
            "put" => {
                let vector = build_vector(&step["vector"]);
                let characters = step["characters"].as_i64().expect("characters");
                let key = step["key"].as_str().expect("key").to_owned();
                self.store(step)
                    .put(&key, &vector, characters)
                    .map(|()| Value::Null)
            }
            "get" => {
                let key = step["key"].as_str().expect("key").to_owned();
                self.store(step)
                    .get(&key)
                    .map(|found| found.map_or(Value::Null, |vector| json!(vector)))
            }
            "reserve" => {
                let project = step["project"].as_str().expect("project").to_owned();
                let characters = step["characters"].as_i64().expect("characters");
                let limit = step["limit"].as_i64().expect("limit");
                let month = step.get("month").and_then(Value::as_str).map(str::to_owned);
                self.store(step)
                    .reserve(&project, characters, limit, month.as_deref())
                    .map(|total| json!(total))
            }
            "close" => {
                self.stores.remove(&name_of(step));
                Ok(Value::Null)
            }
            "lock" => self.lock(step),
            "release" => {
                self.locks.remove(step["name"].as_str().expect("name"));
                Ok(Value::Null)
            }
            "query" => self.query(step),
            "monthOf" => Ok(json!(month_of(step["seconds"].as_i64().expect("seconds")))),
            _ => {
                if operation == "absent" {
                    self.locks.clear();
                    self.stores.clear();
                }
                return self.base.step(step);
            }
        };
        let recorded = outcome_json(result);
        if operation == "lock" && step["contended"] == true {
            return contended(recorded);
        }
        recorded
    }
}
