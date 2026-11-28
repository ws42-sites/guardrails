//! The read queries and pruning, replayed from the recorded steps.

use super::runner::{Outcome, Run};
use ropeline_index::{
    SourceError, architecture_graph, guidepost_graph, latest, overview, prune_snapshots, search,
    snapshot_for_revision, traverse,
};
use serde_json::{Value, json};

fn text(value: &Value) -> &str {
    value.as_str().expect("text argument")
}

fn number(value: &Value) -> i64 {
    value.as_i64().expect("integer argument")
}

/// `indexed_at` defaults to the instant of the run, which is not part of the contract.
pub fn normalise(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, item)| {
                    let item = match (&*key, &item) {
                        ("indexed_at", Value::String(found)) if is_timestamp(found) => {
                            json!("<timestamp>")
                        }
                        _ => normalise(item),
                    };
                    (key, item)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(normalise).collect()),
        other => other,
    }
}

fn is_timestamp(found: &str) -> bool {
    const SHAPE: &[u8; 19] = b"dddd-dd-dd dd:dd:dd";
    found.len() == 19
        && found.bytes().zip(SHAPE).all(|(byte, shape)| match shape {
            b'd' => byte.is_ascii_digit(),
            other => byte == *other,
        })
}

impl Run {
    pub fn read(&self, step: &Value) -> Outcome {
        let function = text(&step["fn"]);
        if function == "cached_git_analyses" {
            return self.cached(step);
        }
        let args: Vec<Value> = step["args"]
            .as_array()
            .expect("args")
            .iter()
            .map(|argument| self.resolve(argument))
            .collect();
        let snapshot = step
            .get("kwargs")
            .and_then(|kwargs| kwargs.get("snapshot"))
            .map(number);
        let at = |index: usize| args.get(index);
        let repo = text(&args[0]);
        match function {
            "latest" => {
                let connection = ropeline_store::connect(&self.db)?;
                let wanted = at(1).map(number);
                Ok(latest(&connection, repo, wanted)?.to_json())
            }
            "search" => Ok(Value::Array(search(
                &self.db,
                repo,
                text(&args[1]),
                at(2).map_or(20, number),
                snapshot,
            )?)),
            "traverse" => Ok(Value::Array(traverse(
                &self.db,
                repo,
                text(&args[1]),
                at(2).map_or("outbound", text),
                at(3).map_or(2, number),
                at(4).map_or(50, number),
                snapshot,
            )?)),
            "overview" => Ok(overview(&self.db, repo, snapshot)?),
            "guidepost_graph" => Ok(guidepost_graph(
                &self.db,
                repo,
                snapshot.expect("snapshot"),
            )?),
            "architecture_graph" => Ok(architecture_graph(
                &self.db,
                repo,
                snapshot.expect("snapshot"),
            )?),
            "snapshot_for_revision" => Ok(json!(snapshot_for_revision(
                &self.db,
                repo,
                text(&args[1])
            )?)),
            other => panic!("unknown read {other}"),
        }
    }

    pub fn prune(&self, step: &Value) -> Result<Value, SourceError> {
        let repo = step
            .get("repo")
            .and_then(Value::as_str)
            .unwrap_or("owner/repo");
        let keep = number(&step["kwargs"]["keep"]);
        prune_snapshots(&self.db, repo, keep)?;
        Ok(Value::Null)
    }
}
