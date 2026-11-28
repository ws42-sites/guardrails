//! Snapshot removal and cache collection. The caller owns the transaction, as in Python.

use crate::error::Result;
use rusqlite::Connection;

pub fn delete_snapshot(connection: &Connection, snapshot_id: i64) -> Result<()> {
    connection.execute(
        "DELETE FROM legacy_edges WHERE snapshot_id=?",
        [snapshot_id],
    )?;
    connection.execute(
        "DELETE FROM legacy_nodes WHERE snapshot_id=?",
        [snapshot_id],
    )?;
    connection.execute("DELETE FROM snapshots WHERE id=?", [snapshot_id])?;
    Ok(())
}

/// Drop cached waypoints and analyses that no manifest refers to any more.
pub fn collect_unused(connection: &Connection) -> Result<()> {
    connection.execute(
        "DELETE FROM waypoint_cache WHERE id NOT IN (SELECT summary_key FROM snapshot_waypoints UNION SELECT validation_key FROM snapshot_waypoints)",
        [],
    )?;
    connection.execute(
        "DELETE FROM analyses WHERE id NOT IN (SELECT analysis_id FROM snapshot_files)",
        [],
    )?;
    Ok(())
}
