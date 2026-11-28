//! Write side of the waypoint cache: derive only the records a snapshot is missing and
//! bind the rest by content. A record that already exists is immutable and is reused by
//! every view that shares its inputs.
//!
//! Python's `waypoint_store.refresh` is the oracle. The indexer hands over each file's stored
//! analysis and the imports it resolved; this module decides what to compute, what to
//! reuse and what to refuse.

use crate::error::{Result, StoreError};
use crate::waypoint_paths::{annotation_path, configuration_path, is_digest};
use crate::waypoint_refresh_areas::refresh_areas;
use crate::waypoint_refresh_files::{refresh_file, unavailable_links};
use crate::waypoint_refresh_types::{Refresh, RefreshFile, Stats};
use rusqlite::Connection;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

fn check_configuration(request: &Refresh<'_>) -> Result<()> {
    if request.configuration.len() > 20_000
        || request
            .configuration
            .iter()
            .any(|(path, value)| !configuration_path(path) || !is_digest(value))
    {
        return Err(StoreError::value(
            "Invalid waypoint configuration fingerprints",
        ));
    }
    if !request.policy_digest.is_empty() && !is_digest(request.policy_digest) {
        return Err(StoreError::value("Invalid waypoint policy fingerprint"));
    }
    Ok(())
}

/// Derive only missing records, bind them to the snapshot and return the counters.
pub fn refresh(connection: &Connection, request: &Refresh<'_>) -> Result<Value> {
    check_configuration(request)?;
    let mut configuration = request.configuration.clone();
    let known: HashMap<&str, &RefreshFile<'_>> =
        request.files.iter().map(|file| (file.path, file)).collect();
    for file in request.files {
        if configuration_path(file.path) {
            configuration.insert(file.path.to_owned(), file.content_hash.to_owned());
        }
    }
    let mut stats = Stats::new(
        request.summaries_computed,
        i64::try_from(request.files.len()).unwrap_or(i64::MAX),
    );
    // Git permits names that the public annotation contract cannot safely display.
    // Optional descriptions must never prevent those files from being indexed.
    let eligible: HashSet<&str> = request
        .files
        .iter()
        .map(|file| file.path)
        .filter(|path| annotation_path(path, false))
        .collect();
    let unavailable: Vec<&str> = request
        .files
        .iter()
        .map(|file| file.path)
        .filter(|path| !eligible.contains(path))
        .collect();
    stats.unavailable(unavailable.len());
    let unavailable_links = unavailable_links(request, &known, &eligible);
    let mut public_records: HashMap<String, Value> = HashMap::new();
    let mut ordered: Vec<&RefreshFile<'_>> = request.files.iter().collect();
    ordered.sort_by(|a, b| a.path.cmp(b.path));
    for file in ordered {
        if !eligible.contains(file.path) {
            continue;
        }
        let record = refresh_file(
            connection,
            request,
            file,
            &configuration,
            unavailable_links.get(file.path),
            &eligible,
            &mut stats,
        )?;
        public_records.insert(file.path.to_owned(), record);
    }
    refresh_areas(
        connection,
        request,
        &known,
        &unavailable,
        &mut public_records,
        &mut stats,
    )?;
    Ok(stats.value())
}
