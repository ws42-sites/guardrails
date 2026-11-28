//! A cross-process lock that serialises cache misses without blocking vector writes.
//!
//! It is an exclusive transaction on a sidecar database next to the cache, held until
//! the guard drops, exactly as Python's context manager holds it.

use crate::error::{Result, StoreError};
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const LONGEST_WAIT: Duration = Duration::from_secs(120);

fn timed_out() -> StoreError {
    StoreError::new(
        "TimeoutError",
        "Semantic computation lock deadline exceeded",
    )
}

/// Holds the lock; dropping it rolls the transaction back and closes the sidecar.
pub struct ComputationGuard {
    connection: Connection,
}

impl Drop for ComputationGuard {
    fn drop(&mut self) {
        let _ = self.connection.execute_batch("ROLLBACK");
    }
}

pub fn lock_path(cache: &Path) -> PathBuf {
    let mut name = cache.as_os_str().to_owned();
    name.push(".compute-lock.sqlite3");
    PathBuf::from(name)
}

pub fn acquire(cache: &Path, deadline: Instant) -> Result<ComputationGuard> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(timed_out());
    }
    let failed = |_: rusqlite::Error| {
        if Instant::now() >= deadline {
            timed_out()
        } else {
            StoreError::new(
                "SemanticCacheError",
                "Semantic computation lock is unavailable",
            )
        }
    };
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let connection = Connection::open_with_flags(lock_path(cache), flags).map_err(failed)?;
    connection
        .busy_timeout(remaining.min(LONGEST_WAIT))
        .map_err(failed)?;
    connection
        .execute_batch("BEGIN EXCLUSIVE")
        .map_err(failed)?;
    let guard = ComputationGuard { connection };
    if Instant::now() >= deadline {
        return Err(timed_out());
    }
    Ok(guard)
}
