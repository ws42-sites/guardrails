//! Turning each file's stored facts into graph edges.
//!
//! Imports are confirmed only when they resolve to a path in the same snapshot. A call
//! candidate becomes a `POSSIBLE_CALL` edge only when exactly one function in its own file
//! has that name, and it is never promoted to a confirmed call.

use crate::analysis::Record;
use crate::clock::Clock;
use crate::error::{Result, SourceError};
use crate::facts::{references_of, type_only_on_line};
use codegraph_engine::relationships::{crate_roots, module_aliases, resolve};
use codegraph_store::Link;
use codegraph_store::json_text::dumps_ordered;
use rusqlite::Connection;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::rc::Rc;

pub struct KnownFile {
    pub path: String,
    pub file_id: String,
    pub record: Rc<Record>,
}

/// The files of one snapshot in capture order, found by path.
#[derive(Default)]
pub struct Known {
    entries: Vec<KnownFile>,
    by_path: HashMap<String, usize>,
}

impl Known {
    pub fn push(&mut self, path: String, file_id: String, record: Rc<Record>) {
        self.by_path.insert(path.clone(), self.entries.len());
        self.entries.push(KnownFile {
            path,
            file_id,
            record,
        });
    }

    pub fn contains(&self, path: &str) -> bool {
        self.by_path.contains_key(path)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &KnownFile> {
        self.entries.iter()
    }

    fn get(&self, path: &str) -> Option<&KnownFile> {
        self.by_path.get(path).map(|index| &self.entries[*index])
    }
}

pub struct Edges {
    pub imports: usize,
    pub calls: usize,
    pub dependencies: HashMap<String, Vec<Link>>,
}

/// The files whose facts the resolvers read for package aliases and Cargo crates, with
/// their facts. Everything else is skipped before it is copied.
fn manifests(known: &Known, wanted: &str) -> Vec<(String, Value)> {
    known
        .iter()
        .filter(|file| file.path.ends_with(wanted))
        .map(|file| (file.path.clone(), file.record.facts.clone()))
        .collect()
}

fn timed_out(clock: &dyn Clock, deadline: f64) -> Result<()> {
    if clock.now() >= deadline {
        return Err(SourceError::timeout());
    }
    Ok(())
}

pub fn resolve_edges(
    connection: &Connection,
    snapshot: i64,
    prefix: &str,
    known: &Known,
    clock: &dyn Clock,
    deadline: f64,
) -> Result<Edges> {
    let aliases = module_aliases(&manifests(known, "package.json"))?;
    let crates = crate_roots(&manifests(known, "Cargo.toml"), &|path| {
        known.contains(path)
    })?;
    let mut result = Edges {
        imports: 0,
        calls: 0,
        dependencies: HashMap::new(),
    };
    for file in known.iter() {
        timed_out(clock, deadline)?;
        let facts = references_of(&file.record.facts)?;
        let (imports, calls) = resolve(
            &file.path,
            &facts,
            &known.by_path,
            Some(&aliases),
            Some(&crates),
        );
        let links = imports
            .iter()
            .filter_map(|import| {
                known.get(&import.0).map(|target| {
                    (
                        import.0.clone(),
                        target.record.id.clone(),
                        "IMPORTS".to_owned(),
                    )
                })
            })
            .collect();
        result.dependencies.insert(file.path.clone(), links);
        let source = format!("f:{prefix}:{}", file.file_id);
        for import in &imports {
            let Some(target) = known.get(&import.0) else {
                continue;
            };
            let evidence = json!({
                "path": file.path,
                "line": import.1,
                "confidence": import.2,
                "type_only": type_only_on_line(&file.record.facts, import.1),
            });
            insert_edge(
                connection,
                snapshot,
                &source,
                &format!("f:{prefix}:{}", target.file_id),
                "IMPORTS",
                &evidence,
            )?;
            result.imports += 1;
        }
        let mut by_name: HashMap<&str, Vec<usize>> = HashMap::new();
        for (ordinal, entry) in file.record.symbols.iter().enumerate() {
            if entry.get(0).and_then(Value::as_str) == Some("function_definition")
                && let Some(name) = entry.get(1).and_then(Value::as_str)
            {
                by_name.entry(name).or_default().push(ordinal);
            }
        }
        for call in &calls {
            let [only] = by_name.get(call.0.as_str()).map_or(&[][..], Vec::as_slice) else {
                continue;
            };
            let evidence = json!({"path": file.path, "line": call.1, "confidence": call.2});
            insert_edge(
                connection,
                snapshot,
                &source,
                &format!("s:{prefix}:{}:{only}", file.file_id),
                "POSSIBLE_CALL",
                &evidence,
            )?;
            result.calls += 1;
        }
    }
    Ok(result)
}

fn insert_edge(
    connection: &Connection,
    snapshot: i64,
    source: &str,
    target: &str,
    kind: &str,
    evidence: &Value,
) -> Result<()> {
    connection.execute(
        "INSERT OR IGNORE INTO resolved_edges VALUES (?,?,?,?,?)",
        (snapshot, source, target, kind, dumps_ordered(evidence)),
    )?;
    Ok(())
}
