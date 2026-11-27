//! Describing one file: its checks (reused when its inputs are unchanged) and its summary
//! (reused whenever its content is).

use crate::error::{Result, StoreError};
use crate::waypoint_cache_write::{as_map, bind, incomplete, store, waypoint_error};
use crate::waypoint_paths::policy_for;
use crate::waypoint_records::{cached, public};
use crate::waypoint_refresh_types::{Identity, Link, Refresh, RefreshFile, Stats};
use codegraph_engine::waypoints::{self, digest};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Identities of links whose far end has no displayable path, by the end that does.
pub(crate) fn unavailable_links(
    request: &Refresh<'_>,
    known: &HashMap<&str, &RefreshFile<'_>>,
    eligible: &HashSet<&str>,
) -> HashMap<String, BTreeSet<Identity>> {
    let mut result: HashMap<String, BTreeSet<Identity>> = HashMap::new();
    for (source, links) in request.dependencies {
        for (target, analysis_id, kind) in links {
            if eligible.contains(source.as_str()) && eligible.contains(target.as_str()) {
                continue;
            }
            let Some(origin) = known.get(source.as_str()) else {
                continue;
            };
            let identity = (
                digest(&json!(source)),
                origin.analysis_id.to_owned(),
                digest(&json!(target)),
                analysis_id.clone(),
                kind.clone(),
            );
            for endpoint in [source, target] {
                if eligible.contains(endpoint.as_str()) {
                    result
                        .entry(endpoint.clone())
                        .or_default()
                        .insert(identity.clone());
                }
            }
        }
    }
    result
}

fn identity_value(identities: &BTreeSet<Identity>) -> Value {
    Value::Array(
        identities
            .iter()
            .map(|(a, b, c, d, e)| json!([a, b, c, d, e]))
            .collect(),
    )
}

pub(crate) fn refresh_file(
    connection: &Connection,
    request: &Refresh<'_>,
    file: &RefreshFile<'_>,
    configuration: &BTreeMap<String, String>,
    omitted: Option<&BTreeSet<Identity>>,
    eligible: &HashSet<&str>,
    stats: &mut Stats,
) -> Result<Value> {
    let facts = match file.facts.get("waypoint") {
        Some(found) if found.is_object() => found,
        _ => {
            return Err(StoreError::new(
                "TypeError",
                "Waypoint facts are missing from the indexed analysis",
            ));
        }
    };
    let policy = policy_for(file.path, configuration, request.policy_digest);
    let inputs: BTreeSet<&Link> = request
        .dependencies
        .get(file.path)
        .map(|links| {
            links
                .iter()
                .filter(|link| eligible.contains(link.0.as_str()))
                .collect()
        })
        .unwrap_or_default();
    let inputs_value = Value::Array(
        inputs
            .iter()
            .map(|(target, id, kind)| json!([target, id, kind]))
            .collect(),
    );
    let mut fingerprint =
        waypoints::validation_fingerprint(file.analysis_id, &inputs_value, &policy)
            .map_err(waypoint_error)?;
    let omitted = omitted.filter(|items| !items.is_empty());
    if let Some(items) = omitted {
        fingerprint = digest(&json!([
            fingerprint,
            "unavailable-waypoint-paths-v1",
            identity_value(items)
        ]));
    }
    let validation_key = digest(&json!([
        "validation",
        request.repository,
        file.path,
        fingerprint
    ]));
    let validation = match cached(connection, &validation_key, "validation")? {
        Some(found) => {
            stats.add("validationsReused");
            found
        }
        None => {
            let mut computed =
                waypoints::validate_file(facts, &inputs_value, &policy, file.analysis_id)
                    .map_err(waypoint_error)?;
            if let Some(items) = omitted {
                computed = incomplete(
                    &computed,
                    &identity_value(items),
                    "Dependency waypoint coverage",
                    "Some indexed dependency paths have no display-safe description; dependency evidence is incomplete.",
                );
            }
            store(connection, &validation_key, "validation", &computed)?;
            stats.add("validationsComputed");
            as_map(computed)
        }
    };
    if validation.get("fingerprint") != Some(&json!(fingerprint))
        || validation.get("ruleVersion") != Some(&json!(waypoints::VALIDATOR_VERSION))
    {
        return Err(StoreError::value(
            "Cached waypoint checks do not match the current inputs",
        ));
    }
    let summary_key_fact = facts
        .get("summaryKey")
        .ok_or_else(|| StoreError::new("KeyError", "'summaryKey'"))?;
    let summary_key = digest(&json!([
        "file",
        waypoints::VERSION,
        file.path,
        file.content_hash,
        summary_key_fact
    ]));
    let summary = match cached(connection, &summary_key, "file")? {
        Some(found) => found,
        None => {
            let mut bound =
                waypoints::bind_file(facts, file.path, "cache", file.content_hash, None)
                    .map_err(waypoint_error)?;
            if let Some(map) = bound.as_object_mut() {
                map.remove("validation");
            }
            store(connection, &summary_key, "file", &bound)?;
            as_map(bound)
        }
    };
    if summary.get("path") != Some(&json!(file.path))
        || summary.get("kind") != Some(&json!("file"))
        || summary.get("contentHash") != Some(&json!(file.content_hash))
        || summary.get("summaryKey") != Some(summary_key_fact)
    {
        return Err(StoreError::value(
            "Cached waypoint description does not match its analysis",
        ));
    }
    bind(
        connection,
        request.snapshot_id,
        file.path,
        "file",
        &summary_key,
        &validation_key,
    )?;
    public(summary, validation, file.path, request.revision)
}
