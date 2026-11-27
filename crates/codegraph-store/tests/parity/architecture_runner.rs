//! Replays a recorded report-cache case through the Rust crate.

use super::runner::{Run, outcome_json};
use super::value::to_json;
use codegraph_store::{ReportStore, StoreError, analysis_identity};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;

const ENGINE: &str = "architecture-1.1.0";
const POLICY: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

pub struct ArchitectureRun {
    base: Run,
    stores: HashMap<String, ReportStore>,
}

fn many_report(repository: &str, index: u64) -> Value {
    json!({
        "repository": repository,
        "revision": format!("rev-{index}"),
        "base_revision": null,
        "policy_digest": POLICY,
        "engine_version": ENGINE,
        "i": index,
    })
}

fn many_evaluation(repository: &str, index: u64) -> Value {
    json!({
        "repository": repository,
        "revision": "rev-1",
        "status": "complete",
        "enforcement_authority": false,
        "n": index,
    })
}

fn padded(value: &Value, step: &Value) -> Value {
    let Some(size) = step.get("pad").and_then(Value::as_u64) else {
        return value.clone();
    };
    let mut copy = value.clone();
    copy.as_object_mut()
        .expect("padded values are objects")
        .insert(
            "pad".into(),
            Value::String("x".repeat(usize::try_from(size).expect("size"))),
        );
    copy
}

fn name_of(step: &Value) -> String {
    step.get("as")
        .and_then(Value::as_str)
        .unwrap_or("main")
        .to_owned()
}

fn text<'a>(step: &'a Value, key: &str) -> &'a str {
    step[key].as_str().expect("text field")
}

impl ArchitectureRun {
    pub fn new(folder: PathBuf) -> Self {
        Self {
            base: Run::new(folder),
            stores: HashMap::new(),
        }
    }

    pub fn state(&self) -> super::value::State {
        self.base.state()
    }

    pub fn close_all(&mut self) {
        self.stores.clear();
        self.base.close_all();
    }

    fn store(&mut self, step: &Value) -> Result<&ReportStore, StoreError> {
        let name = name_of(step);
        if !self.stores.contains_key(&name) {
            let store = ReportStore::new(&self.base.main_path())?;
            self.stores.insert(name.clone(), store);
        }
        Ok(&self.stores[&name])
    }

    fn query(&self, step: &Value) -> Result<Value, StoreError> {
        let connection = Connection::open(self.base.main_path())?;
        let mut statement = connection.prepare(text(step, "sql"))?;
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

    fn reserve_many(&mut self, step: &Value) -> Result<Value, StoreError> {
        let characters = step["characters"].as_i64().expect("characters");
        let store = self.store(step)?;
        let mut results = Vec::new();
        for _ in 0..step["count"].as_u64().expect("count") {
            results.push(
                match store.reserve_evaluation(text(step, "scope"), text(step, "month"), characters)
                {
                    Ok(()) => Value::Null,
                    Err(error) if error.kind == "ValueError" => Value::String(error.message),
                    Err(error) => return Err(error),
                },
            );
        }
        Ok(Value::Array(results))
    }

    fn run(&mut self, operation: &str, step: &Value) -> Result<Value, StoreError> {
        match operation {
            "store" => {
                let db = step.get("db").and_then(Value::as_str).unwrap_or("main");
                let store = ReportStore::new(&self.base.path(db))?;
                self.stores.insert(name_of(step), store);
                Ok(Value::Null)
            }
            "put" => {
                let report = padded(&step["report"], step);
                self.store(step)?.put(&report, text(step, "key"))?;
                Ok(Value::Null)
            }
            "putMany" => {
                let repository = text(step, "repository").to_owned();
                for index in 0..step["count"].as_u64().expect("count") {
                    let key = format!("k-{index}");
                    self.store(step)?
                        .put(&many_report(&repository, index), &key)?;
                }
                Ok(Value::Null)
            }
            "get" => {
                let base = step["base"].as_str();
                let found = self.store(step)?.get(
                    text(step, "repository"),
                    text(step, "revision"),
                    base,
                    text(step, "policy"),
                    text(step, "key"),
                )?;
                Ok(found.unwrap_or(Value::Null))
            }
            "putEvaluation" => {
                let result = padded(&step["result"], step);
                self.store(step)?
                    .put_evaluation(&result, text(step, "key"))?;
                Ok(Value::Null)
            }
            "putEvaluationMany" => {
                let repository = text(step, "repository").to_owned();
                for index in 0..step["count"].as_u64().expect("count") {
                    let key = format!("eval-{index}");
                    self.store(step)?
                        .put_evaluation(&many_evaluation(&repository, index), &key)?;
                }
                Ok(Value::Null)
            }
            "evaluations" => Ok(Value::Array(
                self.store(step)?
                    .evaluations(text(step, "repository"), text(step, "revision"))?,
            )),
            "evaluation" => Ok(self
                .store(step)?
                .evaluation(
                    text(step, "repository"),
                    text(step, "revision"),
                    text(step, "key"),
                )?
                .unwrap_or(Value::Null)),
            "reserve" => {
                let characters = step["characters"].as_i64().expect("characters");
                self.store(step)?.reserve_evaluation(
                    text(step, "scope"),
                    text(step, "month"),
                    characters,
                )?;
                Ok(Value::Null)
            }
            "reserveMany" => self.reserve_many(step),
            "identity" => Ok(Value::String(analysis_identity(
                &step["candidate"],
                &step["baseline"],
                &step["options"],
            ))),
            "query" => self.query(step),
            other => panic!("unknown operation {other}"),
        }
    }

    pub fn step(&mut self, step: &Value) -> Value {
        let operation = step["op"].as_str().expect("op");
        match operation {
            "raw" | "file" | "dir" | "schemaNames" | "absent" => self.base.step(step),
            _ => outcome_json(self.run(operation, step)),
        }
    }
}
