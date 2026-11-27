//! The first screen of a repository: counts, storage, capture notes and top-level areas.

use crate::delivery::{OVERVIEW_BUDGET, overview_waypoints};
use crate::error::{Result, SourceError};
use crate::query::latest;
use codegraph_store::json_text::loads;
use codegraph_store::{connect, summary};
use rusqlite::OptionalExtension;
use serde_json::{Map, Value, json};
use std::path::Path;

pub fn overview(database: &Path, repo: &str, snapshot: Option<i64>) -> Result<Value> {
    let connection = connect(database)?;
    let found = latest(&connection, repo, snapshot)?;
    let mut statement =
        connection.prepare("SELECT kind,count(*) FROM nodes WHERE snapshot_id=? GROUP BY kind")?;
    let mut counts = Map::new();
    let rows = statement.query_map([found.id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    for row in rows {
        let (kind, count) = row?;
        counts.insert(kind, json!(count));
    }
    let text: Option<String> = connection
        .query_row(
            "SELECT metadata FROM nodes WHERE snapshot_id=? AND kind='repository'",
            [found.id],
            |row| row.get(0),
        )
        .optional()?;
    let text = text
        .ok_or_else(|| SourceError::new("TypeError", "'NoneType' object is not subscriptable"))?;
    let mut metadata = loads(&text)?;
    let (waypoints, partial) = overview_waypoints(&metadata, &found.revision, OVERVIEW_BUDGET)?;
    if let Some(map) = metadata.as_object_mut() {
        map.remove("areaWaypoints");
    }
    let stored = summary(&connection, repo)?;
    let mut result = Map::new();
    result.insert("snapshot".into(), json!(found.id));
    result.insert("revision".into(), json!(found.revision));
    result.insert("indexed_at".into(), json!(found.indexed_at));
    result.insert("counts".into(), Value::Object(counts));
    result.insert(
        "storage_summary".into(),
        json!({
            "unique_analyses": stored.unique_analyses,
            "unique_source_bytes": stored.unique_source_bytes,
            "stored_symbols": stored.stored_symbols,
            "manifest_entries": stored.manifest_entries,
        }),
    );
    if let Value::Object(map) = metadata {
        for (key, value) in map {
            result.insert(key, value);
        }
    }
    // An empty list is not worth a key: the overview only names areas when it has some.
    if !waypoints.is_empty() {
        result.insert("waypoints".into(), Value::Array(waypoints));
        result.insert("waypoints_partial".into(), json!(partial));
    }
    Ok(Value::Object(result))
}
