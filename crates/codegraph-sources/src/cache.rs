//! Looking analyses up by Git blob id, so a caller can skip reading a file it already has.
//!
//! The blob id is a lookup key and never a trust boundary: only rows stamped with this
//! runtime's own signature are returned, and the caller still hands over bytes or an
//! analysis id that `index_files` checks.

use crate::context::is_commit;
use crate::error::{Result, SourceError};
use crate::identity::analysis_version;
use crate::language::is_language;
use codegraph_store::connect;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const MAX_OBJECTS: usize = 20_000;
const CHUNK: usize = 500;

/// What the cache knows about one blob: the analysis to reuse and how big the file was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedAnalysis {
    pub id: String,
    pub content_hash: String,
    pub size_bytes: i64,
    pub language: String,
}

impl CachedAnalysis {
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "content_hash": self.content_hash,
            "size_bytes": self.size_bytes,
            "language": self.language,
        })
    }
}

pub type Lookup = BTreeMap<(String, String), CachedAnalysis>;

pub fn cached_git_analyses(database: &Path, objects: &[(String, String)]) -> Result<Lookup> {
    cached_git_analyses_in(database, objects, analysis_version())
}

pub fn cached_git_analyses_in(
    database: &Path,
    objects: &[(String, String)],
    signature: &str,
) -> Result<Lookup> {
    if objects.len() > MAX_OBJECTS
        || objects
            .iter()
            .any(|(oid, language)| !is_commit(oid) || !is_language(language))
    {
        return Err(SourceError::value("Invalid Git object lookup"));
    }
    let wanted: BTreeSet<&(String, String)> = objects.iter().collect();
    let identities: Vec<&str> = objects
        .iter()
        .map(|(oid, _)| oid.as_str())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let connection = connect(database)?;
    let mut result = Lookup::new();
    for chunk in identities.chunks(CHUNK) {
        let marks = vec!["?"; chunk.len()].join(",");
        let sql = format!(
            "SELECT git_oid,id,content_hash,size_bytes,language FROM analyses WHERE version=? AND git_oid IN ({marks})"
        );
        let mut statement = connection.prepare(&sql)?;
        let mut parameters: Vec<&str> = vec![signature];
        parameters.extend(chunk.iter().copied());
        let rows = statement.query_map(rusqlite::params_from_iter(parameters), |row| {
            Ok((
                row.get::<_, String>(0)?,
                CachedAnalysis {
                    id: row.get(1)?,
                    content_hash: row.get(2)?,
                    size_bytes: row.get(3)?,
                    language: row.get(4)?,
                },
            ))
        })?;
        for row in rows {
            let (oid, found) = row?;
            let key = (oid, found.language.clone());
            if wanted.contains(&key) {
                result.insert(key, found);
            }
        }
    }
    Ok(result)
}
