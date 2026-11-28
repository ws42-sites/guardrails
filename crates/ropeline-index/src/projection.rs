//! Bounded projections of one snapshot for navigation and architecture checks. Neither
//! returns source text: nodes, edges and the exact facts the architecture engine reads.

use crate::error::{Result, SourceError};
use crate::query::latest;
use ropeline_engine::architecture::contract::valid_source_files;
use ropeline_store::connect;
use ropeline_store::json_text::loads;
use serde_json::{Map, Value, json};
use std::path::Path;

const NODE_LIMIT: usize = 20_000;
const EDGE_LIMIT: usize = 100_000;

fn bad_json(value: &Value, what: &str) -> SourceError {
    SourceError::new(
        "AttributeError",
        format!("{what} is not a mapping: {value}"),
    )
}

/// A bounded navigation graph: nodes with their stored metadata, and the import and
/// possible-call edges between them. `partial` says a bound was reached.
pub fn guidepost_graph(database: &Path, repo: &str, snapshot: i64) -> Result<Value> {
    let connection = connect(database)?;
    let found = latest(&connection, repo, Some(snapshot))?;
    let mut nodes_query = connection.prepare(
        "SELECT id,kind,name,path,line,metadata FROM nodes WHERE snapshot_id=? ORDER BY CASE WHEN kind='file' THEN 0 ELSE 1 END,path,line,id LIMIT 20001",
    )?;
    let nodes = nodes_query
        .query_map([found.id], |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "kind": row.get::<_, String>(1)?,
                "name": row.get::<_, String>(2)?,
                "path": row.get::<_, String>(3)?,
                "line": row.get::<_, Option<i64>>(4)?,
                "metadata": row.get::<_, Option<String>>(5)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut edges_query = connection.prepare(
        "SELECT source,target,kind,evidence FROM edges WHERE snapshot_id=? AND kind IN ('IMPORTS','POSSIBLE_CALL') ORDER BY source,target,kind LIMIT 20001",
    )?;
    let edges = edge_rows(&mut edges_query, found.id)?;
    let partial = nodes.len() > NODE_LIMIT || edges.len() > NODE_LIMIT;
    Ok(json!({
        "repository": repo,
        "revision": found.revision,
        "snapshot": found.id,
        "nodes": nodes.into_iter().take(NODE_LIMIT).collect::<Vec<_>>(),
        "edges": edges.into_iter().take(NODE_LIMIT).collect::<Vec<_>>(),
        "partial": partial,
    }))
}

fn edge_rows(statement: &mut rusqlite::Statement<'_>, snapshot: i64) -> Result<Vec<Value>> {
    Ok(statement
        .query_map([snapshot], |row| {
            Ok(json!({
                "source": row.get::<_, String>(0)?,
                "target": row.get::<_, String>(1)?,
                "kind": row.get::<_, String>(2)?,
                "evidence": row.get::<_, Option<String>>(3)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

/// The shared file analyses and exact import evidence of a snapshot, without source text.
/// `partial` is true when anything was left out or any file has no stored analysis.
pub fn architecture_graph(database: &Path, repo: &str, snapshot: i64) -> Result<Value> {
    let connection = connect(database)?;
    let found = latest(&connection, repo, Some(snapshot))?;
    let text: String = connection.query_row(
        "SELECT metadata FROM snapshots WHERE id=?",
        [found.id],
        |row| row.get(0),
    )?;
    let snapshot_metadata = loads(&text)?;
    let capture = snapshot_metadata
        .get("capture")
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));
    let mut files_query = connection.prepare(
        "SELECT n.id,n.path,n.metadata,a.facts FROM nodes n LEFT JOIN snapshot_files f ON f.snapshot_id=n.snapshot_id AND f.path=n.path LEFT JOIN analyses a ON a.id=f.analysis_id WHERE n.snapshot_id=? AND n.kind='file' ORDER BY n.path LIMIT 20001",
    )?;
    let files = files_query
        .query_map([found.id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut edges_query = connection.prepare(
        "SELECT source,target,kind,evidence FROM edges WHERE snapshot_id=? AND kind='IMPORTS' ORDER BY source,target LIMIT 100001",
    )?;
    let edges = edge_rows(&mut edges_query, found.id)?;
    let mut inputs = Vec::new();
    for (id, path, metadata, facts) in files.iter().take(NODE_LIMIT) {
        let decoded = loads(
            facts
                .as_deref()
                .filter(|text| !text.is_empty())
                .unwrap_or("{}"),
        )?;
        let architecture = match &decoded {
            Value::Object(map) => map
                .get("architecture")
                .cloned()
                .unwrap_or_else(|| json!({})),
            other => return Err(bad_json(other, "stored facts")),
        };
        inputs.push(json!({
            "id": id,
            "path": path,
            "metadata": loads(metadata)?,
            "facts": architecture,
        }));
    }
    let (projected, excluded) = valid_source_files(&inputs)?;
    let excluded_files = capture
        .get("excluded_files")
        .is_some_and(crate::facts::truthy);
    let partial = excluded > 0
        || excluded_files
        || files.len() > NODE_LIMIT
        || edges.len() > EDGE_LIMIT
        || files
            .iter()
            .any(|(_, _, _, facts)| facts.as_deref().is_none_or(str::is_empty));
    Ok(json!({
        "repository": repo,
        "revision": found.revision,
        "snapshot": found.id,
        "files": projected,
        "edges": edges.into_iter().take(EDGE_LIMIT).collect::<Vec<_>>(),
        "partial": partial,
    }))
}
