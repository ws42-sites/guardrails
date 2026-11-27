//! Versioned local architecture report cache, separate from shared branch graphs.
//!
//! Reports are keyed by repository, revision, base, policy digest, engine version and
//! the identity of the analysis inputs, and bounded per repository. Evaluations are
//! advisory records with a monthly call and character budget.

use crate::error::{Result, StoreError};
use crate::json_text::{dumps_ordered, dumps_sorted, loads};
use codegraph_engine::architecture::contract::{ENGINE_VERSION, MAX_REPORT_BYTES, digest};
use codegraph_engine::architecture::presentation::PRESENTATION_VERSION;
use codegraph_engine::architecture::util::not_subscriptable;
use codegraph_engine::py_value::PyValue;
use rusqlite::types::Value as Sql;
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS architecture_reports (
 repository TEXT NOT NULL, revision TEXT NOT NULL, base_revision TEXT NOT NULL,
 policy_digest TEXT NOT NULL, engine_version TEXT NOT NULL, analysis_key TEXT NOT NULL,
 report TEXT NOT NULL CHECK(length(report)<=4000000),
 PRIMARY KEY(repository,revision,base_revision,policy_digest,engine_version,analysis_key)
);";
const USAGE: &str = "CREATE TABLE IF NOT EXISTS architecture_evaluation_usage (scope TEXT NOT NULL, month TEXT NOT NULL, calls INTEGER NOT NULL, characters INTEGER NOT NULL, PRIMARY KEY(scope,month))";
const EVALUATIONS: &str = "CREATE TABLE IF NOT EXISTS architecture_evaluations (repository TEXT NOT NULL, revision TEXT NOT NULL, request_key TEXT NOT NULL, result TEXT NOT NULL CHECK(length(result)<=131072), PRIMARY KEY(repository,revision,request_key))";

const MAX_EVALUATION_BYTES: usize = 131_072;
const MONTHLY_CALLS: i64 = 50;
const MONTHLY_CHARACTERS: i64 = 200_000;
/// SQLite's journal-mode switch can answer BUSY before its busy timeout applies.
const BUSY_RETRIES: u32 = 5;

pub struct ReportStore {
    path: PathBuf,
}

fn python_type(value: &Value) -> &'static str {
    PyValue::from_value(value).type_name()
}

/// `report["key"]`: a missing key is a `KeyError`, a non-mapping a `TypeError`.
fn field<'a>(value: &'a Value, key: &str) -> Result<&'a Value> {
    match value {
        Value::Object(map) => map
            .get(key)
            .ok_or_else(|| StoreError::new("KeyError", format!("'{key}'"))),
        other => Err(StoreError::new("TypeError", not_subscriptable(other))),
    }
}

/// `value.get(key)` where `value` must be a mapping.
fn get_attribute<'a>(value: &'a Value, key: &str) -> Result<Option<&'a Value>> {
    match value {
        Value::Object(map) => Ok(map.get(key)),
        other => Err(StoreError::new(
            "AttributeError",
            format!("'{}' object has no attribute 'get'", python_type(other)),
        )),
    }
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|float| float != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// What sqlite3 binds for a JSON scalar; containers are not bindable.
fn bound(value: &Value) -> Result<Sql> {
    Ok(match value {
        Value::Null => Sql::Null,
        Value::Bool(flag) => Sql::Integer(i64::from(*flag)),
        Value::Number(number) => number
            .as_i64()
            .map_or_else(|| Sql::Real(number.as_f64().unwrap_or(0.0)), Sql::Integer),
        Value::String(text) => Sql::Text(text.clone()),
        other => {
            return Err(StoreError::new(
                "ProgrammingError",
                format!(
                    "Error binding parameter: type '{}' is not supported",
                    python_type(other)
                ),
            ));
        }
    })
}

impl ReportStore {
    /// The cache must be a plain file: a link could point a write somewhere else.
    pub fn new(path: &Path) -> Result<Self> {
        let is_link = std::fs::symlink_metadata(path)
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(false);
        if is_link {
            return Err(StoreError::value("Architecture cache must not be a link"));
        }
        Ok(Self {
            path: path.to_path_buf(),
        })
    }

    fn connect(&self) -> Result<Connection> {
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let connection =
            Connection::open_with_flags(&self.path, flags).map_err(|error| open_error(&error))?;
        connection.busy_timeout(Duration::from_secs(30))?;
        for attempt in 0..BUSY_RETRIES {
            match connection.query_row("PRAGMA journal_mode=WAL", [], |row| row.get::<_, String>(0))
            {
                Ok(_) => break,
                Err(rusqlite::Error::SqliteFailure(failure, _))
                    if failure.extended_code == rusqlite::ffi::SQLITE_BUSY
                        && attempt + 1 < BUSY_RETRIES =>
                {
                    std::thread::sleep(Duration::from_millis(10 * 2u64.pow(attempt)));
                }
                Err(error) => return Err(error.into()),
            }
        }
        let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version != 0 && version != 1 {
            return Err(StoreError::runtime(
                "Unsupported architecture report schema",
            ));
        }
        // Python's DDL runs outside a transaction, so each statement stands alone.
        connection.execute(SCHEMA, [])?;
        connection.execute(USAGE, [])?;
        connection.execute(EVALUATIONS, [])?;
        connection.execute_batch("PRAGMA user_version=1")?;
        Ok(connection)
    }

    /// Keep an advisory evaluation; the newest 50 per repository survive.
    pub fn put_evaluation(&self, result: &Value, request_key: &str) -> Result<()> {
        let authority = get_attribute(result, "enforcement_authority")?;
        let status = get_attribute(result, "status")?.and_then(Value::as_str);
        if authority != Some(&Value::Bool(false))
            || !matches!(status, Some("complete" | "uncertain" | "inconclusive"))
        {
            return Err(StoreError::value(
                "Only advisory evaluation results may be stored",
            ));
        }
        let encoded = dumps_ordered(result);
        if encoded.len() > MAX_EVALUATION_BYTES {
            return Err(StoreError::value(
                "Evaluation result exceeds storage budget",
            ));
        }
        let mut connection = self.connect()?;
        let repository = bound(field(result, "repository")?)?;
        let revision = bound(field(result, "revision")?)?;
        let tx = connection.transaction()?;
        tx.execute(
            "INSERT OR REPLACE INTO architecture_evaluations VALUES(?,?,?,?)",
            rusqlite::params![repository, revision, request_key, encoded],
        )?;
        tx.execute(
            "DELETE FROM architecture_evaluations WHERE repository=? AND rowid NOT IN (SELECT rowid FROM architecture_evaluations WHERE repository=? ORDER BY rowid DESC LIMIT 50)",
            rusqlite::params![repository, repository],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Up to eight evaluations for a revision, newest first.
    pub fn evaluations(&self, repository: &str, revision: &str) -> Result<Vec<Value>> {
        let connection = self.connect()?;
        let mut statement = connection.prepare(
            "SELECT result FROM architecture_evaluations WHERE repository=? AND revision=? ORDER BY rowid DESC LIMIT 8",
        )?;
        let rows = statement
            .query_map([repository, revision], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows.iter().map(|text| loads(text)).collect()
    }

    pub fn evaluation(
        &self,
        repository: &str,
        revision: &str,
        request_key: &str,
    ) -> Result<Option<Value>> {
        let connection = self.connect()?;
        let row: Option<String> = connection
            .query_row(
                "SELECT result FROM architecture_evaluations WHERE repository=? AND revision=? AND request_key=?",
                [repository, revision, request_key],
                |row| row.get(0),
            )
            .optional()?;
        row.map(|text| loads(&text)).transpose()
    }

    /// Count a model call against the scope's month, atomically; refuse past the pilot budget.
    pub fn reserve_evaluation(&self, scope: &str, month: &str, characters: i64) -> Result<()> {
        if !(1..=24_000).contains(&characters) {
            return Err(StoreError::value("Evaluation input exceeds its budget"));
        }
        let mut connection = self.connect()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (calls, used): (i64, i64) = tx
            .query_row(
                "SELECT calls,characters FROM architecture_evaluation_usage WHERE scope=? AND month=?",
                [scope, month],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .unwrap_or((0, 0));
        if calls >= MONTHLY_CALLS || used + characters > MONTHLY_CHARACTERS {
            return Err(StoreError::value(
                "Pilot architecture evaluation budget is exhausted",
            ));
        }
        tx.execute(
            "INSERT INTO architecture_evaluation_usage VALUES(?,?,?,?) ON CONFLICT(scope,month) DO UPDATE SET calls=excluded.calls,characters=excluded.characters",
            rusqlite::params![scope, month, calls + 1, used + characters],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Store a report for the current engine version; the newest 100 per repository survive.
    pub fn put(&self, report: &Value, analysis_key: &str) -> Result<()> {
        let encoded = dumps_sorted(report);
        let invalid = || StoreError::value("Invalid or oversized architecture report");
        if encoded.len() > MAX_REPORT_BYTES {
            return Err(invalid());
        }
        if get_attribute(report, "engine_version")? != Some(&Value::String(ENGINE_VERSION.into())) {
            return Err(invalid());
        }
        let mut connection = self.connect()?;
        let repository = bound(field(report, "repository")?)?;
        let revision = bound(field(report, "revision")?)?;
        // `report.get("base_revision") or ""`: any falsy value means "no base".
        let base_revision = match get_attribute(report, "base_revision")? {
            Some(value) if truthy(value) => bound(value)?,
            _ => Sql::Text(String::new()),
        };
        let policy_digest = bound(field(report, "policy_digest")?)?;
        let tx = connection.transaction()?;
        tx.execute(
            "INSERT OR REPLACE INTO architecture_reports VALUES(?,?,?,?,?,?,?)",
            rusqlite::params![
                repository,
                revision,
                base_revision,
                policy_digest,
                ENGINE_VERSION,
                analysis_key,
                encoded
            ],
        )?;
        // Bounded retention per repository; never evict another repository's report.
        tx.execute(
            "DELETE FROM architecture_reports WHERE repository=? AND rowid NOT IN (SELECT rowid FROM architecture_reports WHERE repository=? ORDER BY rowid DESC LIMIT 100)",
            rusqlite::params![repository, repository],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// The cached report, or `None`. A stored report that names another identity than the
    /// key it was found under is an error, not a hit.
    pub fn get(
        &self,
        repository: &str,
        revision: &str,
        base_revision: Option<&str>,
        policy_digest: &str,
        analysis_key: &str,
    ) -> Result<Option<Value>> {
        let connection = self.connect()?;
        let row: Option<String> = connection
            .query_row(
                "SELECT report FROM architecture_reports WHERE repository=? AND revision=? AND base_revision=? AND policy_digest=? AND engine_version=? AND analysis_key=?",
                rusqlite::params![
                    repository,
                    revision,
                    base_revision.unwrap_or(""),
                    policy_digest,
                    ENGINE_VERSION,
                    analysis_key
                ],
                |row| row.get(0),
            )
            .optional()?;
        let Some(text) = row else {
            return Ok(None);
        };
        let data = loads(&text)?;
        let matches = |key: &str, expected: &str| -> Result<bool> {
            Ok(field(&data, key)?.as_str() == Some(expected))
        };
        if !matches("repository", repository)?
            || !matches("revision", revision)?
            || !matches("policy_digest", policy_digest)?
        {
            return Err(StoreError::value("Architecture cache identity mismatch"));
        }
        Ok(Some(data))
    }
}

/// rusqlite adds the path to an open failure; Python's message is SQLite's alone.
fn open_error(error: &rusqlite::Error) -> StoreError {
    match error {
        rusqlite::Error::SqliteFailure(failure, _) => {
            StoreError::from(rusqlite::Error::SqliteFailure(
                *failure,
                Some(rusqlite::ffi::code_to_str(failure.extended_code).to_owned()),
            ))
        }
        other => StoreError::new("OperationalError", other.to_string()),
    }
}

/// The key under which an analysis of these inputs is cached.
pub fn analysis_identity(candidate: &Value, baseline: &Value, options: &Value) -> String {
    digest(&json!([
        candidate,
        baseline,
        options,
        ENGINE_VERSION,
        PRESENTATION_VERSION
    ]))
}
