//! Ropeline's SQLite stores, ported from CodeGraph's Python one module at a time.
//!
//! Python is the oracle: these modules open the same files with the same schema,
//! pragmas and statements, and `tests/parity.rs` holds them to what
//! `tools/record_store_parity.py` recorded. The only I/O is opening the database
//! path a caller hands over; nothing here reads the environment, the network or
//! other processes.

#![forbid(unsafe_code)]

mod architecture_store;
mod error;
mod graph_migrate;
mod graph_read;
mod graph_rollback;
pub mod graph_schema;
mod graph_write;
pub mod json_text;
mod semantic_lock;
mod semantic_month;
mod semantic_store;
mod semantic_vector;
mod waypoint_cache_write;
mod waypoint_paths;
mod waypoint_records;
mod waypoint_refresh;
mod waypoint_refresh_areas;
mod waypoint_refresh_files;
mod waypoint_refresh_types;

pub use architecture_store::{ReportStore, analysis_identity};
pub use error::{Result, StoreError};
pub use graph_migrate::{connect, migrate};
pub use graph_read::{Analysis, Summary, analysis, summary};
pub use graph_rollback::{rollback, rollback_waypoints};
pub use graph_schema::{MAX_ANALYSIS_BYTES, VERSION};
pub use graph_write::{collect_unused, delete_snapshot};
pub use semantic_lock::ComputationGuard;
pub use semantic_month::{current_month, month_of};
pub use semantic_store::{DEFAULT_MAX_ENTRIES, SemanticStore};
pub use semantic_vector::{DIMENSIONS, fsum};
pub use waypoint_paths::configuration_path;
pub use waypoint_records::records as waypoint_records;
pub use waypoint_refresh::refresh as refresh_waypoints;
pub use waypoint_refresh_types::{Link, Refresh, RefreshFile};
