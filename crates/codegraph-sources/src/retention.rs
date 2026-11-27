//! Finding a snapshot by revision, and keeping only the newest few of each view.

use crate::error::{Result, SourceError};
use codegraph_store::{collect_unused, connect, delete_snapshot};
use rusqlite::OptionalExtension;
use std::path::Path;

/// The newest snapshot of `repo` captured at `revision`, if any.
pub fn snapshot_for_revision(database: &Path, repo: &str, revision: &str) -> Result<Option<i64>> {
    let connection = connect(database)?;
    Ok(connection
        .query_row(
            "SELECT id FROM snapshots WHERE repo=? AND revision=? ORDER BY id DESC LIMIT 1",
            (repo, revision),
            |row| row.get(0),
        )
        .optional()?)
}

/// Keep the newest `keep` snapshots of every view of `repo` and drop the rest, along with
/// any cached analysis or description nothing refers to any more. Two is the least that
/// keeps a baseline beside the current snapshot.
pub fn prune_snapshots(database: &Path, repo: &str, keep: i64) -> Result<()> {
    if keep < 2 {
        return Err(SourceError::value("Keep at least two snapshots"));
    }
    let mut connection = connect(database)?;
    let tx = connection.transaction()?;
    let views: Vec<String> = tx
        .prepare("SELECT DISTINCT view_key FROM snapshots WHERE repo=?")?
        .query_map([repo], |row| row.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    for view in views {
        let obsolete: Vec<i64> = tx
            .prepare(
                "SELECT id FROM snapshots WHERE repo=? AND view_key=? ORDER BY id DESC LIMIT -1 OFFSET ?",
            )?
            .query_map((repo, &view, keep), |row| row.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        for id in obsolete {
            delete_snapshot(&tx, id)?;
        }
    }
    collect_unused(&tx)?;
    tx.commit()?;
    Ok(())
}
