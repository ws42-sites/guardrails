//! Opening a graph database and bringing it to the current layout.

use crate::error::{Result, StoreError};
use crate::graph_schema::{
    EDGE_VIEW, STAGING_EDGE_TRIGGER, TABLES, VERSION, WAYPOINT_TABLES, node_view,
};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use std::path::Path;
use std::time::Duration;

fn user_version(connection: &Connection) -> Result<i64> {
    Ok(connection.query_row("PRAGMA user_version", [], |row| row.get(0))?)
}

fn install_views(connection: &Connection) -> Result<()> {
    connection.execute(&node_view(), [])?;
    connection.execute(EDGE_VIEW, [])?;
    // The staging edge-write contract used by integrity qualification stays in place.
    connection.execute(STAGING_EDGE_TRIGGER, [])?;
    Ok(())
}

/// Upgrade in one immediate transaction: a failure leaves the original schema and data.
pub fn migrate(connection: &mut Connection) -> Result<()> {
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let version = user_version(&tx)?;
    if version == VERSION {
        tx.commit()?;
        return Ok(());
    }
    if version != 0 && version != 2 {
        return Err(StoreError::runtime("Unsupported graph storage version"));
    }
    if version == 2 {
        tx.execute("DROP TRIGGER staging_edge_insert", [])?;
        tx.execute("DROP VIEW nodes", [])?;
        tx.execute("DROP VIEW edges", [])?;
        for statement in WAYPOINT_TABLES {
            tx.execute(statement, [])?;
        }
        install_views(&tx)?;
        tx.execute_batch(&format!("PRAGMA user_version={VERSION}"))?;
        tx.commit()?;
        return Ok(());
    }
    for name in ["nodes", "edges"] {
        let kind: Option<String> = tx
            .query_row("SELECT type FROM sqlite_master WHERE name=?", [name], |r| {
                r.get(0)
            })
            .optional()?;
        match kind.as_deref() {
            Some("table") => {
                tx.execute(&format!("ALTER TABLE {name} RENAME TO legacy_{name}"), [])?;
            }
            Some(_) => return Err(StoreError::runtime("Unexpected legacy graph schema")),
            None => {}
        }
    }
    for statement in TABLES.iter().chain(WAYPOINT_TABLES.iter()) {
        tx.execute(statement, [])?;
    }
    let mut columns = Vec::new();
    {
        let mut statement = tx.prepare("PRAGMA table_info(snapshots)")?;
        let names = statement.query_map([], |row| row.get::<_, String>(1))?;
        for name in names {
            columns.push(name?);
        }
    }
    for (name, definition) in [
        ("metadata", "TEXT NOT NULL DEFAULT '{}'"),
        ("node_prefix", "TEXT"),
        ("view_key", "TEXT NOT NULL DEFAULT 'default'"),
    ] {
        if !columns.iter().any(|column| column == name) {
            tx.execute(
                &format!("ALTER TABLE snapshots ADD COLUMN {name} {definition}"),
                [],
            )?;
        }
    }
    tx.execute(
        "CREATE INDEX IF NOT EXISTS snapshot_view ON snapshots(repo,view_key,id DESC)",
        [],
    )?;
    install_views(&tx)?;
    tx.execute_batch(&format!("PRAGMA user_version={VERSION}"))?;
    tx.commit()?;
    Ok(())
}

/// rusqlite appends the path to an open failure; Python's message is SQLite's alone.
fn open_error(error: rusqlite::Error) -> StoreError {
    match error {
        rusqlite::Error::SqliteFailure(failure, _) => {
            StoreError::from(rusqlite::Error::SqliteFailure(
                failure,
                Some(rusqlite::ffi::code_to_str(failure.extended_code).to_owned()),
            ))
        }
        other => other.into(),
    }
}

/// Open with Python's settings: 30 s busy wait, WAL, foreign keys on, then migrate.
pub fn connect(database: &Path) -> Result<Connection> {
    // No URI handling: Python's sqlite3.connect treats the argument as a plain path.
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let mut connection = Connection::open_with_flags(database, flags).map_err(open_error)?;
    connection.busy_timeout(Duration::from_secs(30))?;
    connection.query_row("PRAGMA journal_mode=WAL", [], |row| row.get::<_, String>(0))?;
    connection.execute_batch("PRAGMA foreign_keys=ON")?;
    if user_version(&connection)? != VERSION {
        migrate(&mut connection)?;
    }
    Ok(connection)
}
