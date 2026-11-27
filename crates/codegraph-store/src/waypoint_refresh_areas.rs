//! Describing directories: each is summarised from its children, deepest first.

use crate::error::{Result, StoreError};
use crate::waypoint_cache_write::{as_map, bind, incomplete, store, waypoint_error};
use crate::waypoint_paths::{annotation_path, parent_of};
use crate::waypoint_records::{cached, public};
use crate::waypoint_refresh_types::{Refresh, RefreshFile, Stats};
use codegraph_engine::posix_path::parts;
use codegraph_engine::waypoints::{self, digest};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::collections::HashMap;

const MAX_AREAS: usize = 10_000;

/// exist so the repository root and its folders stay explicitly incomplete.
fn area_children(
    public_records: &HashMap<String, Value>,
    unavailable: &[&str],
) -> HashMap<String, Vec<String>> {
    let mut children: HashMap<String, Vec<String>> = HashMap::new();
    let mut shown: Vec<&String> = public_records.keys().collect();
    shown.sort();
    for path in shown {
        let mut parent = parent_of(path);
        children
            .entry(parent.clone())
            .or_default()
            .push(path.clone());
        while parent != "." {
            let ancestor = parent_of(&parent);
            let values = children.entry(ancestor.clone()).or_default();
            if !values.contains(&parent) {
                values.push(parent.clone());
            }
            parent = ancestor;
        }
    }
    for path in unavailable {
        let mut parent = parent_of(path);
        let mut child: Option<String> = None;
        loop {
            if annotation_path(&parent, true) {
                let values = children.entry(parent.clone()).or_default();
                if let Some(found) = &child
                    && !values.contains(found)
                {
                    values.push(found.clone());
                }
                child = Some(parent.clone());
            }
            if parent == "." {
                break;
            }
            parent = parent_of(&parent);
        }
    }
    children
}

pub(crate) fn refresh_areas(
    connection: &Connection,
    request: &Refresh<'_>,
    known: &HashMap<&str, &RefreshFile<'_>>,
    unavailable: &[&str],
    public_records: &mut HashMap<String, Value>,
    stats: &mut Stats,
) -> Result<()> {
    let children = area_children(public_records, unavailable);
    if children.len() > MAX_AREAS {
        return Err(StoreError::value("Repository exceeds waypoint area limit"));
    }
    let mut order: Vec<&String> = children.keys().collect();
    order.sort_by(|a, b| parts(b).len().cmp(&parts(a).len()).then_with(|| a.cmp(b)));
    for path in order {
        let mut names = children[path].clone();
        names.sort();
        let mut members = Vec::new();
        for name in &names {
            let member = public_records
                .get(name)
                .ok_or_else(|| StoreError::new("KeyError", format!("'{name}'")))?;
            members.push(member.clone());
        }
        let members = Value::Array(members);
        let summary_key = waypoints::area_fingerprint(path, &members).map_err(waypoint_error)?;
        let summary = match cached(connection, &summary_key, "area")? {
            Some(found) => {
                stats.add("areasReused");
                found
            }
            None => {
                let rebound: Vec<Value> = members
                    .as_array()
                    .map(|items| items.iter().map(cache_member).collect())
                    .unwrap_or_default();
                let mut built = waypoints::build_area(path, &Value::Array(rebound), "cache")
                    .map_err(waypoint_error)?;
                if let Some(map) = built.as_object_mut() {
                    map.remove("validation");
                }
                store(connection, &summary_key, "area", &built)?;
                stats.add("areasComputed");
                as_map(built)
            }
        };
        if summary.get("path") != Some(&json!(path))
            || summary.get("kind") != Some(&json!("area"))
            || summary.get("summaryKey") != Some(&json!(summary_key))
        {
            return Err(StoreError::value(
                "Cached area waypoint does not match its children",
            ));
        }
        let mut validation = waypoints::area_validation(&members).map_err(waypoint_error)?;
        let mut missing: Vec<(String, String)> = unavailable
            .iter()
            .filter(|source| path == "." || source.starts_with(&format!("{path}/")))
            .map(|source| {
                (
                    digest(&json!(source)),
                    known
                        .get(source)
                        .map(|file| file.analysis_id.to_owned())
                        .unwrap_or_default(),
                )
            })
            .collect();
        missing.sort();
        if !missing.is_empty() {
            let identities = Value::Array(missing.iter().map(|(a, b)| json!([a, b])).collect());
            validation = incomplete(
                &validation,
                &identities,
                "Description coverage",
                &format!(
                    "{} indexed files have no display-safe waypoint; this area description covers available files only.",
                    missing.len()
                ),
            );
        }
        let validation_key = digest(&json!([
            "area-validation",
            request.repository,
            path,
            validation["fingerprint"]
        ]));
        let validation = match cached(connection, &validation_key, "validation")? {
            None => {
                store(connection, &validation_key, "validation", &validation)?;
                stats.add("areaValidationsComputed");
                as_map(validation)
            }
            Some(found) => {
                if found.get("fingerprint") != Some(&validation["fingerprint"])
                    || found.get("ruleVersion") != Some(&json!(waypoints::VALIDATOR_VERSION))
                {
                    return Err(StoreError::value(
                        "Cached area checks do not match their children",
                    ));
                }
                stats.add("areaValidationsReused");
                found
            }
        };
        bind(
            connection,
            request.snapshot_id,
            path,
            "area",
            &summary_key,
            &validation_key,
        )?;
        let record = public(summary, validation, path, request.revision)?;
        public_records.insert(path.clone(), record);
    }
    Ok(())
}

/// A member with its evidence stamped `cache`, so the cached area never names a revision.
fn cache_member(member: &Value) -> Value {
    let mut result = member.clone();
    if let Some(map) = result.as_object_mut() {
        let items: Vec<Value> = member["evidence"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .map(|item| {
                        let mut copy = item.clone();
                        if let Some(inner) = copy.as_object_mut() {
                            inner.insert("revision".into(), json!("cache"));
                        }
                        copy
                    })
                    .collect()
            })
            .unwrap_or_default();
        map.insert("evidence".into(), Value::Array(items));
    }
    result
}
