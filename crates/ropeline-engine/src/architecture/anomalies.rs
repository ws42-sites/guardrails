//! Peer-aware statistical advice, separate from deterministic policy failures:
//! port of `architecture_anomalies.py`.

use super::PyError;
use super::contract::digest;
use super::evidence::evidence;
use super::recognition::structural_graph;
use super::rules::{category_of, import_graph};
use super::util::{array, get, lookup, text_of, truthy};
use crate::py_value::format_g;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub const VERSION: &str = "peer-statistics-1";

const NOTICE: &str = "Small cohorts abstain. A structural outlier is review advice, not proof of poor design or an approved policy violation.";
const PEER_ALTERNATIVE: &str = "This may be an intentional public boundary, coordinator or composition root; inspect responsibility before splitting it.";
const OVERLAP_ALTERNATIVE: &str = "These may be legitimate strategies, platform adapters or test doubles; a shared interface does not prove duplicate business logic.";
const OVERLAP_EXPLANATION: &str = "Several implementations of one port perform similar kinds of effects; compare responsibility before adding another.";
const METRICS: [&str; 3] = ["fan_in", "fan_out", "effect_categories"];

struct Peer {
    path: String,
    role: &'static str,
    values: [usize; 3],
}

/// `statistics.median` of sorted values, as a float.
fn median(sorted: &[f64]) -> f64 {
    let middle = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        sorted[middle]
    } else {
        (sorted[middle - 1] + sorted[middle]) / 2.0
    }
}

fn sorted_floats(mut values: Vec<f64>) -> Vec<f64> {
    values.sort_by(f64::total_cmp);
    values
}

/// `peer_anomalies(graph, projection)`.
pub fn peer_anomalies(graph: &Value, projection: Option<&Value>) -> Result<Value, PyError> {
    let adjacency = import_graph(graph, false)?;
    let mut reverse: HashMap<&str, BTreeSet<&str>> = adjacency
        .edges
        .keys()
        .map(|path| (path.as_str(), BTreeSet::new()))
        .collect();
    for (source, targets) in &adjacency.edges {
        for target in targets {
            reverse
                .get_mut(target.as_str())
                .ok_or_else(|| PyError::new("KeyError", format!("'{target}'")))?
                .insert(source);
        }
    }
    let mut files: Vec<(String, &Value)> = Vec::new();
    for file in array(get(graph, "files")?) {
        let path = text_of(get(file, "path")?).to_owned();
        match files.iter_mut().find(|(existing, _)| *existing == path) {
            Some(slot) => slot.1 = file,
            None => files.push((path, file)),
        }
    }
    let file_of = |path: &str| {
        files
            .iter()
            .find(|(existing, _)| existing == path)
            .map(|(_, file)| *file)
    };
    let mut cohorts: BTreeMap<String, Vec<Peer>> = BTreeMap::new();
    for (path, file) in &files {
        if category_of(path)? != "code" {
            continue;
        }
        let structure = lookup(get(file, "facts")?, "structure")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if !truthy(&structure) {
            continue;
        }
        let declarations = lookup(&structure, "declarations")
            .map(array)
            .unwrap_or_default();
        let relations = lookup(&structure, "relations")
            .map(array)
            .unwrap_or_default();
        let all_hidden = declarations.iter().all(|declaration| {
            text_of(&declaration["kind"]) == "interface"
                || text_of(&declaration["name"]).starts_with('_')
        });
        let constructs = relations
            .iter()
            .filter(|relation| text_of(&relation["kind"]) == "CONSTRUCTS")
            .count();
        let role = if !declarations.is_empty() && all_hidden {
            "contract"
        } else if constructs >= 2 {
            "composition"
        } else {
            "implementation"
        };
        let parts: Vec<&str> = path.split('/').collect();
        let package: Vec<&str> = if path.starts_with("packages/") || path.starts_with("apps/") {
            parts.iter().take(2).copied().collect()
        } else if path.contains('/') {
            parts.iter().take(1).copied().collect()
        } else {
            vec!["."]
        };
        let language = match lookup(get(file, "metadata")?, "language") {
            None => "unknown",
            Some(Value::String(found)) => found.as_str(),
            Some(_) => {
                return Err(PyError::new(
                    "TypeError",
                    "can only concatenate str (not \"NoneType\") to str",
                ));
            }
        };
        let cohort = format!("{}:{language}:{role}", package.join("/"));
        let kinds: BTreeSet<&str> = lookup(&structure, "effects")
            .map(array)
            .unwrap_or_default()
            .iter()
            .map(|effect| text_of(&effect["kind"]))
            .collect();
        cohorts.entry(cohort).or_default().push(Peer {
            path: path.clone(),
            role,
            values: [
                reverse.get(path.as_str()).map_or(0, BTreeSet::len),
                adjacency.targets(path).len(),
                kinds.len(),
            ],
        });
    }

    let mut findings: Vec<Value> = Vec::new();
    let mut summaries: Vec<Value> = Vec::new();
    for (cohort, peers) in &cohorts {
        let mut metrics = serde_json::Map::new();
        for (index, metric) in METRICS.iter().enumerate() {
            let mut values: Vec<usize> = peers.iter().map(|peer| peer.values[index]).collect();
            values.sort_unstable();
            let floats: Vec<f64> = values.iter().map(|value| *value as f64).collect();
            let centre = median(&floats);
            let mad = median(&sorted_floats(
                floats.iter().map(|value| (value - centre).abs()).collect(),
            ));
            let position = ((values.len() - 1) as f64 * 0.95) as usize;
            let p95 = values[position.min(values.len() - 1)];
            let threshold = (centre + 3.0f64.max(6.0 * mad)).max(p95 as f64);
            metrics.insert(
                (*metric).to_owned(),
                json!({
                    "median": centre,
                    "median_absolute_deviation": mad,
                    "percentile_95": p95,
                    "review_above": threshold,
                }),
            );
            if peers.len() < 5 {
                continue;
            }
            for peer in peers {
                let value = peer.values[index];
                if value as f64 <= threshold {
                    continue;
                }
                let file = file_of(&peer.path).expect("peer came from the files");
                findings.push(json!({
                    "id": digest(&json!([get(graph, "repository")?, cohort, peer.path, metric])),
                    "kind": "peer-outlier",
                    "path": peer.path,
                    "origin": "inferred",
                    "severity": "review",
                    "metric": metric,
                    "value": value,
                    "peer_group": cohort,
                    "peer_count": peers.len(),
                    "median": centre,
                    "mad": mad,
                    "percentile_95": p95,
                    "role": peer.role,
                    "classifier_version": VERSION,
                    "confidence_calibration": "uncalibrated",
                    "review_status": "unreviewed",
                    "enforcement_authority": false,
                    "explanation": format!(
                        "{} is {value}, above the peer review threshold {}.",
                        metric.replace('_', " "),
                        format_g(threshold)
                    ),
                    "alternatives": [PEER_ALTERNATIVE],
                    "evidence": [evidence(graph, file, &json!(1), VERSION)?],
                }));
            }
        }
        summaries.push(json!({
            "id": cohort,
            "members": peers.len(),
            "status": if peers.len() >= 5 { "evaluated" } else { "inconclusive" },
            "metrics": metrics,
        }));
    }

    let computed;
    let structure_graph = match projection {
        Some(given) => given,
        None => {
            computed = structural_graph(graph)?;
            &computed
        }
    };
    let mut implementations: BTreeMap<(String, String), Vec<&Value>> = BTreeMap::new();
    for relation in array(get(structure_graph, "relations")?) {
        let target_path = lookup(relation, "target_path").filter(|found| truthy(found));
        if text_of(get(relation, "kind")?) == "IMPLEMENTS"
            && let Some(target_path) = target_path
        {
            implementations
                .entry((
                    text_of(target_path).to_owned(),
                    text_of(get(relation, "target")?).to_owned(),
                ))
                .or_default()
                .push(relation);
        }
    }
    for ((port_path, port), relations) in &implementations {
        let paths: BTreeSet<String> = relations
            .iter()
            .map(|relation| text_of(&relation["source_path"]).to_owned())
            .collect();
        if paths.len() < 2 {
            continue;
        }
        let mut shared: Option<BTreeSet<String>> = None;
        for path in &paths {
            let file =
                file_of(path).ok_or_else(|| PyError::new("KeyError", format!("'{path}'")))?;
            let kinds: BTreeSet<String> = lookup(get(file, "facts")?, "structure")
                .and_then(|structure| lookup(structure, "effects"))
                .map(array)
                .unwrap_or_default()
                .iter()
                .map(|effect| text_of(&effect["kind"]).to_owned())
                .collect();
            shared = Some(match shared {
                None => kinds,
                Some(found) => found.intersection(&kinds).cloned().collect(),
            });
        }
        let shared = shared.unwrap_or_default();
        if !["persistence-call", "event-producer", "api-route"]
            .iter()
            .any(|kind| shared.contains(*kind))
        {
            continue;
        }
        let paths: Vec<&String> = paths.iter().collect();
        findings.push(json!({
            "id": digest(&json!([get(graph, "repository")?, "implementation-overlap", port_path, port])),
            "kind": "competing-implementation-candidate",
            "path": paths[0],
            "paths": paths,
            "origin": "inferred",
            "severity": "review",
            "metric": "implementations-of-port",
            "value": paths.len(),
            "peer_group": format!("{port_path}:{port}"),
            "peer_count": paths.len(),
            "shared_effect_categories": shared,
            "classifier_version": VERSION,
            "confidence_calibration": "uncalibrated",
            "review_status": "unreviewed",
            "enforcement_authority": false,
            "explanation": OVERLAP_EXPLANATION,
            "alternatives": [OVERLAP_ALTERNATIVE],
            "evidence": relations.iter().take(4).map(|relation| relation["evidence"].clone()).collect::<Vec<_>>(),
        }));
    }
    let total = findings.len();
    findings.sort_by(|left, right| text_of(&left["id"]).cmp(text_of(&right["id"])));
    let partial =
        total > 128 || summaries.len() > 64 || lookup(graph, "partial").is_some_and(truthy);
    Ok(json!({
        "version": VERSION,
        "anomalies": findings.iter().take(128).collect::<Vec<_>>(),
        "peer_groups": summaries.iter().take(64).collect::<Vec<_>>(),
        "partial": partial,
        "notice": NOTICE,
    }))
}
