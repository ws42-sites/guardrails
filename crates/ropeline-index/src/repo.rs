//! `index_repo`: index a local folder, read-only.
//!
//! The folder is walked, its source files are read as bytes and indexed as one snapshot,
//! and the number of files left out (links, oversize or non-UTF-8 files, unsupported source)
//! is recorded with the snapshot so a reader can tell the graph is partial.

use crate::capture::{Capture, resolve_root};
use crate::clock::MonotonicClock;
use crate::error::{Result, SourceError};
use crate::identity::analysis_version;
use crate::index::index_files_in;
use crate::options::{Environment, IndexOptions, IndexResult, check_limits};
use crate::ownership::read_codeowners;
use ropeline_store::connect;
use ropeline_store::json_text::{dumps_ordered, loads};
use serde_json::json;
use std::cell::Cell;
use std::path::Path;

pub struct RepoOptions {
    pub index: IndexOptions,
    /// Walk `dist/` too (its protected sub-folders stay excluded).
    pub include_dist: bool,
    /// Files larger than this are left out and counted.
    pub max_file_bytes: i64,
}

impl Default for RepoOptions {
    fn default() -> Self {
        Self {
            index: IndexOptions::default(),
            include_dist: false,
            max_file_bytes: 1_000_000,
        }
    }
}

/// Index `root` with the real clock and this runtime's signature.
pub fn index_repo(
    root: &Path,
    database: &Path,
    repository: Option<&str>,
    options: RepoOptions,
) -> Result<IndexResult> {
    let clock = MonotonicClock::new();
    let environment = Environment {
        clock: &clock,
        signature: analysis_version(),
        before_commit: None,
    };
    index_repo_in(root, database, repository, options, environment)
}

pub fn index_repo_in(
    root: &Path,
    database: &Path,
    repository: Option<&str>,
    options: RepoOptions,
    environment: Environment<'_>,
) -> Result<IndexResult> {
    if options.max_file_bytes <= 0 {
        return Err(SourceError::value("Invalid source limits"));
    }
    check_limits(&options.index)?;
    let root = resolve_root(root)?;
    let clock = environment.clock;
    let deadline = clock.now() + options.index.timeout_seconds;
    let skipped = Cell::new(0);
    let capture = Capture::new(
        root.clone(),
        options.include_dist,
        u64::try_from(options.max_file_bytes).unwrap_or(u64::MAX),
        clock,
        deadline,
        &skipped,
    );
    let name = match repository.filter(|name| !name.is_empty()) {
        Some(name) => name.to_owned(),
        None => root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
    };
    let mut index = options.index;
    index.ownership = read_codeowners(&root, None);
    let mut result = index_files_in(capture, database, &name, &index, environment)?;
    result.skipped_files = skipped.get();
    record_capture(database, result.snapshot, result.skipped_files)?;
    Ok(result)
}

/// The count of files left out, stored with the snapshot's metadata.
fn record_capture(database: &Path, snapshot: i64, excluded: usize) -> Result<()> {
    let mut connection = connect(database)?;
    let tx = connection.transaction()?;
    let text: String = tx.query_row(
        "SELECT metadata FROM snapshots WHERE id=?",
        [snapshot],
        |row| row.get(0),
    )?;
    let mut metadata = loads(&text)?;
    if let Some(map) = metadata.as_object_mut() {
        map.insert("capture".into(), json!({ "excluded_files": excluded }));
    }
    tx.execute(
        "UPDATE snapshots SET metadata=? WHERE id=?",
        (dumps_ordered(&metadata), snapshot),
    )?;
    tx.commit()?;
    Ok(())
}
