//! Private vector cache and conservative, transactional character reservations.
//!
//! The cache keeps vectors, least-recently-used counters and a project/month ledger.
//! It never sees descriptor text, only how many characters were spent on it.

use crate::error::{Result, StoreError};
use crate::semantic_lock::{ComputationGuard, acquire};
use crate::semantic_month::current_month;
use crate::semantic_vector::{encode, validate};
use ropeline_engine::py_json;
use ropeline_engine::py_value::PyValue;
use rusqlite::types::Value as Stored;
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const DEFAULT_MAX_ENTRIES: i64 = 20_000;

const INSTALL: [&str; 3] = [
    "CREATE TABLE semantic_vectors (cache_key TEXT PRIMARY KEY, vector TEXT NOT NULL, characters INTEGER NOT NULL CHECK(characters>=0), last_used INTEGER NOT NULL)",
    "CREATE TABLE semantic_usage (project TEXT NOT NULL, month TEXT NOT NULL, reserved INTEGER NOT NULL CHECK(reserved>=0), PRIMARY KEY(project,month))",
    "CREATE INDEX semantic_lru ON semantic_vectors(last_used)",
];

fn cache_error(message: &str) -> StoreError {
    StoreError::new("SemanticCacheError", message)
}

fn unavailable<E>(_: E) -> StoreError {
    cache_error("Semantic cache is unavailable")
}

pub struct SemanticStore {
    connection: Connection,
    path: PathBuf,
    max_entries: i64,
}

fn install(connection: &mut Connection) -> rusqlite::Result<bool> {
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let version: i64 = tx.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let mut tables = Vec::new();
    {
        let mut statement = tx.prepare("SELECT name FROM sqlite_master WHERE type='table'")?;
        for name in statement.query_map([], |row| row.get::<_, String>(0))? {
            tables.push(name?);
        }
    }
    tables.sort();
    if version == 0 && tables.is_empty() {
        for statement in INSTALL {
            tx.execute(statement, [])?;
        }
        tx.execute_batch("PRAGMA user_version=1")?;
    } else if version != 1 || tables != ["semantic_usage", "semantic_vectors"] {
        return Ok(false);
    }
    // Check the read contract before publishing an opened cache.
    tx.prepare("SELECT cache_key,vector,characters,last_used FROM semantic_vectors LIMIT 0")?
        .query([])?
        .next()?;
    tx.prepare("SELECT project,month,reserved FROM semantic_usage LIMIT 0")?
        .query([])?
        .next()?;
    tx.commit()?;
    Ok(true)
}

impl SemanticStore {
    /// Open or create the cache. Every failure reads as "unavailable", so a corrupt or
    /// foreign file never leaks detail and is never modified.
    pub fn open(path: &Path, max_entries: i64) -> Result<Self> {
        if !(1..=100_000).contains(&max_entries) {
            return Err(StoreError::value("Invalid semantic cache bound"));
        }
        let path = std::path::absolute(path).map_err(unavailable)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(unavailable)?;
        }
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let mut connection = Connection::open_with_flags(&path, flags).map_err(unavailable)?;
        connection
            .busy_timeout(Duration::from_secs(30))
            .map_err(unavailable)?;
        match install(&mut connection) {
            Ok(true) => {}
            Ok(false) => return Err(unavailable(())),
            Err(error) => return Err(unavailable(error)),
        }
        Ok(Self {
            connection,
            path,
            max_entries,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The vector under `key`, marking it as just used. Corrupt data is an error, not a miss.
    pub fn get(&mut self, key: &str) -> Result<Option<Vec<f64>>> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(unavailable)?;
        let stored: Option<Stored> = tx
            .query_row(
                "SELECT vector FROM semantic_vectors WHERE cache_key=?",
                [key],
                |row| row.get(0),
            )
            .optional()
            .map_err(unavailable)?;
        let Some(stored) = stored else {
            return Ok(None);
        };
        let text = match stored {
            Stored::Text(text) => text,
            Stored::Blob(bytes) => String::from_utf8(bytes).map_err(unavailable)?,
            _ => return Err(unavailable(())),
        };
        let decoded = py_json::loads_decoded(&text, None).map_err(unavailable)?;
        let vector = validate(&decoded)?;
        tx.execute(
            "UPDATE semantic_vectors SET last_used=(SELECT COALESCE(MAX(last_used),0)+1 FROM semantic_vectors) WHERE cache_key=?",
            [key],
        )
        .map_err(unavailable)?;
        tx.commit().map_err(unavailable)?;
        Ok(Some(vector))
    }

    pub fn put(&mut self, key: &str, vector: &[f64], characters: i64) -> Result<()> {
        let vector = validate(&PyValue::List(
            vector.iter().map(|value| PyValue::Float(*value)).collect(),
        ))?;
        if !(0..=2000).contains(&characters) {
            return Err(StoreError::value("Invalid descriptor size"));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(unavailable)?;
        tx.execute(
            "INSERT OR REPLACE INTO semantic_vectors VALUES(?,?,?,(SELECT COALESCE(MAX(last_used),0)+1 FROM semantic_vectors))",
            rusqlite::params![key, encode(&vector), characters],
        )
        .map_err(unavailable)?;
        tx.execute(
            "DELETE FROM semantic_vectors WHERE cache_key IN (SELECT cache_key FROM semantic_vectors ORDER BY last_used DESC,cache_key LIMIT -1 OFFSET ?)",
            [self.max_entries],
        )
        .map_err(unavailable)?;
        tx.commit().map_err(unavailable)?;
        Ok(())
    }

    /// Reserve characters against a monthly allowance, atomically across connections.
    /// Returns the month's total after the reservation.
    pub fn reserve(
        &mut self,
        project: &str,
        characters: i64,
        monthly_limit: i64,
        month: Option<&str>,
    ) -> Result<i64> {
        if !(0..=1_000_000).contains(&characters) || !(0..=100_000_000).contains(&monthly_limit) {
            return Err(StoreError::value("Invalid semantic character allowance"));
        }
        let month = month.map_or_else(current_month, str::to_owned);
        let ledger = |_: rusqlite::Error| cache_error("Semantic usage ledger is unavailable");
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(ledger)?;
        let reserved: i64 = tx
            .query_row(
                "SELECT reserved FROM semantic_usage WHERE project=? AND month=?",
                [project, &month],
                |row| row.get(0),
            )
            .optional()
            .map_err(ledger)?
            .unwrap_or(0);
        if characters != 0 && reserved + characters > monthly_limit {
            return Err(StoreError::new(
                "SemanticBudgetExceeded",
                "Monthly embedding allowance reached; use cached views or increase the configured character allowance",
            ));
        }
        tx.execute(
            "INSERT INTO semantic_usage VALUES(?,?,?) ON CONFLICT(project,month) DO UPDATE SET reserved=excluded.reserved",
            rusqlite::params![project, month, reserved + characters],
        )
        .map_err(ledger)?;
        tx.commit().map_err(ledger)?;
        Ok(reserved + characters)
    }

    /// Serialise cache misses across processes until the guard is dropped.
    pub fn computation(&self, deadline: Instant) -> Result<ComputationGuard> {
        acquire(&self.path, deadline)
    }

    /// The month the ledger is keyed by right now, in UTC.
    pub fn month() -> String {
        current_month()
    }
}
