//! Downgrades kept for operators: they retire the shared layout without losing a node or edge.

use crate::error::{Result, StoreError};
use crate::graph_schema::{
    EDGE_VIEW, OLD_SNAPSHOTS, STAGING_EDGE_TRIGGER_DOWNGRADE, V2_NODE_VIEW, VERSION,
};
use rusqlite::{Connection, TransactionBehavior};

fn current_version(connection: &Connection) -> Result<i64> {
    Ok(connection.query_row("PRAGMA user_version", [], |row| row.get(0))?)
}

/// Materialise every retained graph before restoring the original schema.
pub fn rollback(connection: &mut Connection) -> Result<()> {
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if current_version(&tx)? != VERSION {
        return Err(StoreError::runtime(
            "Only the current graph storage version can be rolled back",
        ));
    }
    tx.execute(
        "INSERT INTO legacy_nodes SELECT * FROM nodes WHERE snapshot_id IN (SELECT id FROM snapshots WHERE node_prefix IS NOT NULL)",
        [],
    )?;
    tx.execute(
        "INSERT INTO legacy_edges SELECT * FROM edges WHERE snapshot_id IN (SELECT id FROM snapshots WHERE node_prefix IS NOT NULL)",
        [],
    )?;
    tx.execute("DROP TRIGGER staging_edge_insert", [])?;
    tx.execute("DROP VIEW nodes", [])?;
    tx.execute("DROP VIEW edges", [])?;
    for name in [
        "snapshot_waypoints",
        "waypoint_cache",
        "resolved_edges",
        "snapshot_files",
        "analyses",
    ] {
        tx.execute(&format!("DROP TABLE {name}"), [])?;
    }
    tx.execute(OLD_SNAPSHOTS, [])?;
    tx.execute(
        "INSERT INTO old_snapshots SELECT id,repo,revision,indexed_at FROM snapshots",
        [],
    )?;
    tx.execute("DROP TABLE snapshots", [])?;
    tx.execute("ALTER TABLE old_snapshots RENAME TO snapshots", [])?;
    tx.execute("ALTER TABLE legacy_nodes RENAME TO nodes", [])?;
    tx.execute("ALTER TABLE legacy_edges RENAME TO edges", [])?;
    tx.execute_batch("PRAGMA user_version=0")?;
    tx.commit()?;
    Ok(())
}

/// Remove derived waypoint caches while preserving the version-2 graph.
pub fn rollback_waypoints(connection: &mut Connection) -> Result<()> {
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if current_version(&tx)? != VERSION {
        return Err(StoreError::runtime(
            "Only waypoint storage version 3 can be downgraded",
        ));
    }
    tx.execute("DROP TRIGGER staging_edge_insert", [])?;
    tx.execute("DROP VIEW nodes", [])?;
    tx.execute("DROP VIEW edges", [])?;
    tx.execute("DROP TABLE snapshot_waypoints", [])?;
    tx.execute("DROP TABLE waypoint_cache", [])?;
    tx.execute(V2_NODE_VIEW, [])?;
    tx.execute(EDGE_VIEW, [])?;
    tx.execute(STAGING_EDGE_TRIGGER_DOWNGRADE, [])?;
    tx.execute(
        "UPDATE snapshots SET metadata=json_remove(metadata,'$.waypoint_stats')",
        [],
    )?;
    tx.execute_batch("PRAGMA user_version=2")?;
    tx.commit()?;
    Ok(())
}
