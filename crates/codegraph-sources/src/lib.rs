//! The CodeGraph indexer core, ported from `core.py`.
//!
//! This crate reads bytes (a list of files, or a local folder) and writes them into the
//! graph database through `codegraph-store`, using the parsing and relationship logic in
//! `codegraph-engine`. Python is the oracle: `tests/parity.rs` holds it to what
//! `tools/record_indexer_parity.py` recorded.
//!
//! It never runs anything from the repositories it reads. It touches the file system only
//! to list and read the folder it is given; it starts no process, opens no network
//! connection and reads no environment variable. `tests/purity.rs` enforces that.

#![forbid(unsafe_code)]

mod analysis;
mod cache;
mod capture;
mod capture_file;
mod clock;
mod context;
mod delivery;
mod edges;
mod error;
mod facts;
mod identity;
mod index;
mod language;
mod options;
mod overview;
mod ownership;
mod projection;
mod query;
mod repo;
mod retention;

pub use cache::{CachedAnalysis, Lookup, cached_git_analyses, cached_git_analyses_in};
pub use clock::{Clock, MonotonicClock};
pub use delivery::{area_waypoints, file_waypoint, overview_waypoints};
pub use error::{Result, SourceError};
pub use identity::{ANALYSIS_VERSION, analysis_key, analysis_version, git_blob_oid, stable_id};
pub use index::{index_files, index_files_in};
pub use language::{SKIP, source_language, unsupported_source};
pub use options::{Environment, IndexOptions, IndexResult, SourceFile};
pub use overview::overview;
pub use ownership::read_codeowners;
pub use projection::{architecture_graph, guidepost_graph};
pub use query::{Snapshot, latest, search, traverse};
pub use repo::{RepoOptions, index_repo, index_repo_in};
pub use retention::{prune_snapshots, snapshot_for_revision};
