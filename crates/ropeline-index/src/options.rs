//! What a caller hands the indexer and what it gets back.

use crate::clock::Clock;
use crate::error::{Result, SourceError};
use ropeline_engine::codeowners::Ownership;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// One file to capture: its repository path and either its bytes or the id of an analysis
/// already stored for the same bytes. An empty analysis id counts as none, as in Python.
#[derive(Debug, Clone, Default)]
pub struct SourceFile {
    pub path: String,
    pub data: Option<Vec<u8>>,
    pub analysis_id: Option<String>,
    pub git_oid: Option<String>,
}

impl SourceFile {
    pub fn bytes(path: &str, data: &[u8]) -> Self {
        Self {
            path: path.to_owned(),
            data: Some(data.to_vec()),
            ..Self::default()
        }
    }
}

pub struct IndexOptions {
    pub source_revision: Option<String>,
    /// The workspace the files came from, as the caller supplied it; checked on use.
    pub workspace_context: Option<Value>,
    pub max_files: i64,
    pub max_bytes: i64,
    pub max_symbols: i64,
    pub timeout_seconds: f64,
    pub waypoint_configuration: BTreeMap<String, String>,
    pub waypoint_policy_digest: String,
    pub ownership: Option<Ownership>,
}

impl Default for IndexOptions {
    fn default() -> Self {
        Self {
            source_revision: None,
            workspace_context: None,
            max_files: 20_000,
            max_bytes: 64_000_000,
            max_symbols: 250_000,
            timeout_seconds: 120.0,
            waypoint_configuration: BTreeMap::new(),
            waypoint_policy_digest: String::new(),
            ownership: None,
        }
    }
}

/// The ambient pieces of one run, so tests can hand over a counting clock, another
/// signature or a hook that refuses.
pub struct Environment<'a> {
    pub clock: &'a dyn Clock,
    pub signature: &'a str,
    pub before_commit: Option<&'a mut dyn FnMut() -> Result<()>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IndexResult {
    pub repository: String,
    pub revision: String,
    pub snapshot: i64,
    pub files: usize,
    pub symbols: i64,
    pub imports: usize,
    pub possible_calls: usize,
    pub skipped_files: usize,
    pub parsed_files: usize,
    pub reused_files: usize,
    pub waypoints: Value,
}

impl IndexResult {
    pub fn to_json(&self) -> Value {
        json!({
            "repository": self.repository,
            "revision": self.revision,
            "snapshot": self.snapshot,
            "files": self.files,
            "symbols": self.symbols,
            "imports": self.imports,
            "possible_calls": self.possible_calls,
            "skipped_files": self.skipped_files,
            "parsed_files": self.parsed_files,
            "reused_files": self.reused_files,
            "waypoints": self.waypoints,
        })
    }
}

pub fn check_limits(options: &IndexOptions) -> Result<()> {
    if options.max_files <= 0
        || options.max_bytes <= 0
        || options.max_symbols <= 0
        || options.timeout_seconds <= 0.0
    {
        return Err(SourceError::value("Indexing limits must be positive"));
    }
    Ok(())
}
