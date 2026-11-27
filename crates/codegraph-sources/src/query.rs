//! Read queries over one repository snapshot: find a snapshot, search its nodes, walk its
//! edges. Every answer carries the repository, revision and snapshot it came from, and a
//! possible call stays what it is: an inference, never a confirmed call.

use crate::delivery::file_waypoint;
use crate::error::{Result, SourceError};
use codegraph_engine::file_categories::file_category;
use codegraph_store::connect;
use rusqlite::{Connection, OptionalExtension, params_from_iter};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;

const MAX_SEARCH: i64 = 100;
const MAX_TRAVERSE: i64 = 200;
const MAX_DEPTH: i64 = 5;

/// The snapshot a read is answered from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub id: i64,
    pub revision: String,
    pub indexed_at: Option<String>,
}

impl Snapshot {
    pub fn to_json(&self) -> Value {
        json!({"id": self.id, "revision": self.revision, "indexed_at": self.indexed_at})
    }
}

/// The newest snapshot of `repo`, or the one named. A repository nobody indexed is an error.
pub fn latest(connection: &Connection, repo: &str, snapshot: Option<i64>) -> Result<Snapshot> {
    let row = connection
        .query_row(
            "SELECT id,revision,indexed_at FROM snapshots WHERE repo=? AND (? IS NULL OR id=?) ORDER BY id DESC LIMIT 1",
            (repo, snapshot, snapshot),
            |row| {
                Ok(Snapshot {
                    id: row.get(0)?,
                    revision: row.get(1)?,
                    indexed_at: row.get(2)?,
                })
            },
        )
        .optional()?;
    row.ok_or_else(|| SourceError::value(&format!("No indexed snapshot for {repo}")))
}

/// Descriptions of the files at `paths`, found by path in one snapshot.
pub(crate) fn waypoints_for_paths(
    connection: &Connection,
    snapshot_id: i64,
    revision: &str,
    paths: &HashSet<String>,
) -> Result<BTreeMap<String, Value>> {
    let sorted: BTreeSet<&String> = paths
        .iter()
        .filter(|path| !path.is_empty() && path.as_str() != ".")
        .collect();
    let mut result = BTreeMap::new();
    if sorted.is_empty() {
        return Ok(result);
    }
    let marks = vec!["?"; sorted.len()].join(",");
    let sql = format!(
        "SELECT path,metadata FROM nodes WHERE snapshot_id=? AND kind='file' AND path IN ({marks})"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(
        params_from_iter(
            std::iter::once(rusqlite::types::Value::Integer(snapshot_id)).chain(
                sorted
                    .iter()
                    .map(|path| rusqlite::types::Value::Text((*path).clone())),
            ),
        ),
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    )?;
    for row in rows {
        let (path, metadata) = row?;
        if let Some(description) = file_waypoint(&Value::String(metadata), &path, revision)? {
            result.insert(path, description);
        }
    }
    Ok(result)
}

fn category_of(path: &str, kind: &str) -> Value {
    json!(file_category(Some(path), kind))
}

/// Nodes whose name or path contains `query`, an exact name first, at most `limit` (1 to 100).
pub fn search(
    database: &Path,
    repo: &str,
    query: &str,
    limit: i64,
    snapshot: Option<i64>,
) -> Result<Vec<Value>> {
    let connection = connect(database)?;
    let found = latest(&connection, repo, snapshot)?;
    // The query is text to find, never a pattern: LIKE's own wildcards are escaped.
    let escaped = query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let pattern = format!("%{escaped}%");
    let mut statement = connection.prepare(
        "SELECT id,kind,name,path,line FROM nodes WHERE snapshot_id=? AND (name LIKE ? ESCAPE '\\' OR path LIKE ? ESCAPE '\\') ORDER BY CASE WHEN lower(name)=lower(?) THEN 0 ELSE 1 END,kind LIMIT ?",
    )?;
    let rows = statement
        .query_map(
            (
                found.id,
                &pattern,
                &pattern,
                query,
                limit.clamp(1, MAX_SEARCH),
            ),
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                ))
            },
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let paths: HashSet<String> = rows.iter().map(|row| row.3.clone()).collect();
    let descriptions = waypoints_for_paths(&connection, found.id, &found.revision, &paths)?;
    Ok(rows
        .into_iter()
        .map(|(id, kind, name, path, line)| {
            let mut map = Map::new();
            map.insert("id".into(), json!(id));
            map.insert("kind".into(), json!(kind));
            map.insert("name".into(), json!(name));
            map.insert("path".into(), json!(path));
            map.insert("line".into(), json!(line));
            map.insert("file_category".into(), category_of(&path, &kind));
            map.insert("repository".into(), json!(repo));
            map.insert("revision".into(), json!(found.revision));
            map.insert("snapshot".into(), json!(found.id));
            if let Some(description) = descriptions.get(&path) {
                map.insert("waypoint".into(), description.clone());
            }
            Value::Object(map)
        })
        .collect())
}

struct Neighbour {
    kind: String,
    source: String,
    target: String,
    evidence: Option<String>,
    node_kind: String,
    name: String,
    path: String,
    line: Option<i64>,
}

fn neighbours(
    connection: &Connection,
    snapshot: i64,
    node: &str,
    outbound: bool,
    limit: i64,
) -> Result<Vec<Neighbour>> {
    let (column, other) = if outbound {
        ("source", "target")
    } else {
        ("target", "source")
    };
    let sql = format!(
        "SELECT e.kind,e.source,e.target,e.evidence,n.kind node_kind,n.name,n.path,n.line FROM edges e JOIN nodes n ON n.id=e.{other} WHERE e.snapshot_id=? AND e.{column}=? LIMIT ?"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement
        .query_map((snapshot, node, limit), |row| {
            Ok(Neighbour {
                kind: row.get(0)?,
                source: row.get(1)?,
                target: row.get(2)?,
                evidence: row.get(3)?,
                node_kind: row.get(4)?,
                name: row.get(5)?,
                path: row.get(6)?,
                line: row.get(7)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Nodes reached from `node_id` along edges, breadth first, in one direction.
pub fn traverse(
    database: &Path,
    repo: &str,
    node_id: &str,
    direction: &str,
    depth: i64,
    limit: i64,
    snapshot: Option<i64>,
) -> Result<Vec<Value>> {
    if direction != "inbound" && direction != "outbound" {
        return Err(SourceError::value("direction must be inbound or outbound"));
    }
    let outbound = direction == "outbound";
    let connection = connect(database)?;
    let found = latest(&connection, repo, snapshot)?;
    let exists = connection
        .query_row(
            "SELECT 1 FROM nodes WHERE id=? AND snapshot_id=?",
            (node_id, found.id),
            |_| Ok(()),
        )
        .optional()?;
    if exists.is_none() {
        return Err(SourceError::value("Node not found in current snapshot"));
    }
    let cap = limit.min(MAX_TRAVERSE);
    let mut frontier: BTreeSet<String> = BTreeSet::from([node_id.to_owned()]);
    let mut seen: HashSet<String> = HashSet::from([node_id.to_owned()]);
    let mut results: Vec<(Neighbour, String)> = Vec::new();
    for _ in 0..depth.clamp(0, MAX_DEPTH) {
        if frontier.is_empty() || results.len() as i64 >= cap {
            break;
        }
        let mut next = BTreeSet::new();
        for node in &frontier {
            for row in neighbours(&connection, found.id, node, outbound, cap)? {
                if results.len() as i64 >= cap {
                    break;
                }
                let reached = if outbound {
                    row.target.clone()
                } else {
                    row.source.clone()
                };
                if seen.insert(reached.clone()) {
                    next.insert(reached.clone());
                    results.push((row, reached));
                }
            }
        }
        frontier = next;
    }
    let paths: HashSet<String> = results.iter().map(|(row, _)| row.path.clone()).collect();
    let descriptions = waypoints_for_paths(&connection, found.id, &found.revision, &paths)?;
    Ok(results
        .into_iter()
        .map(|(row, _)| {
            let mut map = Map::new();
            map.insert("kind".into(), json!(row.kind));
            map.insert("source".into(), json!(row.source));
            map.insert("target".into(), json!(row.target));
            map.insert("evidence".into(), json!(row.evidence));
            map.insert("node_kind".into(), json!(row.node_kind));
            map.insert("name".into(), json!(row.name));
            map.insert("path".into(), json!(row.path));
            map.insert("line".into(), json!(row.line));
            map.insert(
                "file_category".into(),
                category_of(&row.path, &row.node_kind),
            );
            map.insert("repository".into(), json!(repo));
            map.insert("revision".into(), json!(found.revision));
            map.insert("snapshot".into(), json!(found.id));
            if let Some(description) = descriptions.get(&row.path) {
                map.insert("waypoint".into(), description.clone());
            }
            Value::Object(map)
        })
        .collect())
}
