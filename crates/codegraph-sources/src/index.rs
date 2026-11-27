//! `index_files`: capture a list of files as one snapshot, all or nothing.
//!
//! The snapshot, its file manifest, the analyses it needs, the resolved edges and the
//! waypoint bindings are written in one transaction. A limit, a bad path, a parser or
//! cache failure, a time-out or a refusing `before_commit` hook leaves the database as it
//! was. Source bytes are only ever parsed.

use crate::analysis::Record;
use crate::capture_file::{Totals, capture_file};
use crate::clock::{Clock, MonotonicClock};
use crate::context::{is_commit, node_prefix, snapshot_context};
use crate::edges::{Known, resolve_edges};
use crate::error::{Result, SourceError};
use crate::identity::{analysis_version, sha256_hex};
use crate::options::{Environment, IndexOptions, IndexResult, SourceFile, check_limits};
use codegraph_store::json_text::{dumps_ordered, dumps_sorted};
use codegraph_store::{Refresh, RefreshFile, connect, refresh_waypoints};
use rusqlite::Connection;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

fn timeout_check(clock: &dyn Clock, deadline: f64) -> Result<()> {
    if clock.now() >= deadline {
        return Err(SourceError::timeout());
    }
    Ok(())
}

/// Capture `files` with the real clock and this runtime's signature.
pub fn index_files(
    files: impl IntoIterator<Item = Result<SourceFile>>,
    database: &Path,
    repository: &str,
    options: &IndexOptions,
) -> Result<IndexResult> {
    let clock = MonotonicClock::new();
    let environment = Environment {
        clock: &clock,
        signature: analysis_version(),
        before_commit: None,
    };
    index_files_in(files, database, repository, options, environment)
}

pub fn index_files_in(
    files: impl IntoIterator<Item = Result<SourceFile>>,
    database: &Path,
    repository: &str,
    options: &IndexOptions,
    mut environment: Environment<'_>,
) -> Result<IndexResult> {
    check_limits(options)?;
    if let Some(revision) = &options.source_revision
        && !is_commit(revision)
    {
        return Err(SourceError::value(
            "Source revision must be a full commit SHA",
        ));
    }
    let deadline = environment.clock.now() + options.timeout_seconds;
    let context = snapshot_context(
        options.source_revision.as_deref(),
        options.workspace_context.as_ref(),
    )?;
    let mut connection = connect(database)?;
    let tx = connection.transaction()?;
    let mut metadata = context.metadata;
    let mut revision = context.revision;
    tx.execute(
        "INSERT INTO snapshots(repo,revision,metadata,view_key) VALUES (?,?,?,?)",
        (
            repository,
            &revision,
            dumps_ordered(&Value::Object(metadata.clone())),
            &context.view_key,
        ),
    )?;
    let snapshot = tx.last_insert_rowid();
    let prefix = node_prefix(snapshot, repository, &revision, &metadata);
    tx.execute(
        "UPDATE snapshots SET node_prefix=? WHERE id=?",
        (&prefix, snapshot),
    )?;
    let signature = environment.signature;
    let mut known = Known::default();
    let mut used: HashMap<String, Rc<Record>> = HashMap::new();
    let mut totals = Totals {
        parsed: 0,
        symbols: 0,
        bytes: 0,
        analysis_bytes: 0,
    };
    for item in files {
        let item = item?;
        timeout_check(environment.clock, deadline)?;
        capture_file(
            &tx,
            snapshot,
            item,
            options,
            signature,
            &mut known,
            &mut used,
            &mut totals,
        )?;
    }
    let edges = resolve_edges(&tx, snapshot, &prefix, &known, environment.clock, deadline)?;
    let reused = known.len() - totals.parsed;
    metadata.insert(
        "analysis_stats".into(),
        json!({"parsed_files": totals.parsed, "reused_files": reused}),
    );
    // Every analysis used here matched this signature, so it is the truthful producer of
    // the snapshot; older snapshots simply lack the record.
    metadata.insert("analyzer".into(), json!({"version": signature}));
    if let Some(ownership) = &options.ownership {
        // Parsed rules travel with the snapshot so every read cites the same revision's
        // CODEOWNERS rather than whatever the checkout holds later.
        metadata.insert("codeowners".into(), ownership.record());
    }
    if options.source_revision.is_none() {
        revision.push_str("-tree-");
        revision.push_str(&tree_digest(&known, options));
    }
    let stats = waypoints_for(
        &tx,
        snapshot,
        &known,
        &edges.dependencies,
        &revision,
        repository,
        options,
        totals.parsed,
    )?;
    metadata.insert("waypoint_stats".into(), stats.clone());
    tx.execute(
        "UPDATE snapshots SET metadata=?,revision=? WHERE id=?",
        (dumps_ordered(&Value::Object(metadata)), &revision, snapshot),
    )?;
    timeout_check(environment.clock, deadline)?;
    if let Some(hook) = environment.before_commit.as_mut() {
        hook()?;
    }
    tx.commit()?;
    Ok(IndexResult {
        repository: repository.to_owned(),
        revision,
        snapshot,
        files: known.len(),
        symbols: totals.symbols,
        imports: edges.imports,
        possible_calls: edges.calls,
        skipped_files: 0,
        parsed_files: totals.parsed,
        reused_files: reused,
        waypoints: stats,
    })
}

/// The working-tree revision is named after what it holds: every path with its analysis,
/// the configuration fingerprints and the policy.
fn tree_digest(known: &Known, options: &IndexOptions) -> String {
    let mut pairs: Vec<(&str, &str)> = known
        .iter()
        .map(|file| (file.path.as_str(), file.record.id.as_str()))
        .collect();
    pairs.sort();
    let configuration: Map<String, Value> = options
        .waypoint_configuration
        .iter()
        .map(|(path, value)| (path.clone(), json!(value)))
        .collect();
    let value = json!([
        pairs
            .iter()
            .map(|(p, id)| json!([p, id]))
            .collect::<Vec<_>>(),
        configuration,
        options.waypoint_policy_digest
    ]);
    sha256_hex(dumps_sorted(&value).as_bytes())
}

#[allow(clippy::too_many_arguments)]
fn waypoints_for(
    connection: &Connection,
    snapshot: i64,
    known: &Known,
    dependencies: &HashMap<String, Vec<(String, String, String)>>,
    revision: &str,
    repository: &str,
    options: &IndexOptions,
    parsed: usize,
) -> Result<Value> {
    let files: Vec<RefreshFile<'_>> = known
        .iter()
        .map(|file| RefreshFile {
            path: &file.path,
            analysis_id: &file.record.id,
            content_hash: &file.record.content_hash,
            facts: &file.record.facts,
        })
        .collect();
    Ok(refresh_waypoints(
        connection,
        &Refresh {
            snapshot_id: snapshot,
            files: &files,
            dependencies,
            revision,
            repository,
            configuration: &options.waypoint_configuration,
            policy_digest: &options.waypoint_policy_digest,
            summaries_computed: i64::try_from(parsed).unwrap_or(i64::MAX),
        },
    )?)
}
