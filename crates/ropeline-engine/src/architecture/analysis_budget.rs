//! `architecture_analysis.context` and `bounded`: task-sized agent guidance within a
//! character budget.
//!
//! `bounded` removes items from lists wherever it finds them, and protects particular
//! list objects, so the packet is a `tree::J` whose lists have identity, and `context`
//! builds its packet the way Python does: one list of relevant scopes held under two keys.

use super::PyError;
use super::contract::Contract;
use super::presentation::protected_response_lists;
use super::rules::{effective_rules, matches};
use super::tree::{J, dict, list};
use super::util::{array, get, lookup, text_of, truthy};
use crate::text;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};

fn value_list(items: Vec<Value>) -> J {
    list(items.iter().map(J::from_value).collect())
}

/// `context(graph, report, contract, task, paths)`.
pub fn context(
    graph: &Value,
    report: &Value,
    contract: &Contract,
    task: &str,
    paths: &[String],
) -> Result<J, PyError> {
    let terms: BTreeSet<String> = text::split_words(task)
        .into_iter()
        .filter(|word| text::char_len(word) > 2)
        .map(text::casefold)
        .collect();
    let files = array(get(graph, "files")?);
    let mut candidates: Vec<(&Value, String)> = Vec::new();
    for file in files {
        candidates.push((file, text_of(get(file, "path")?).to_owned()));
    }
    let score = |path: &str| -> i64 {
        let folded = text::casefold(path);
        terms
            .iter()
            .filter(|term| folded.contains(term.as_str()))
            .count() as i64
    };
    candidates.sort_by(|left, right| (-score(&left.1), &left.1).cmp(&(-score(&right.1), &right.1)));
    let selected: Vec<String> = if paths.is_empty() {
        candidates
            .iter()
            .take(8)
            .map(|(_, path)| path.clone())
            .collect()
    } else {
        paths.to_vec()
    };
    let known: BTreeSet<&str> = candidates.iter().map(|(_, path)| path.as_str()).collect();
    if selected.iter().any(|path| !known.contains(path.as_str())) {
        return Err(PyError::value("Context path is not in the pinned snapshot"));
    }
    let mut rules: Vec<((String, String), &super::contract::Rule)> = Vec::new();
    for path in &selected {
        for rule in effective_rules(contract, path) {
            let key = (rule.id.clone(), rule.scope.clone());
            match rules.iter_mut().find(|(existing, _)| *existing == key) {
                Some(slot) => slot.1 = rule,
                None => rules.push((key, rule)),
            }
        }
    }
    let fingerprint = get(report, "fingerprint")?;
    let scopes = lookup(fingerprint, "subsystems")
        .map(array)
        .unwrap_or_default();
    let mut manifests: Vec<String> = Vec::new();
    for file in files {
        let named = lookup(get(file, "facts")?, "manifest")
            .and_then(|manifest| lookup(manifest, "name"))
            .is_some_and(truthy);
        if named {
            let path = text_of(get(file, "path")?);
            let directory = match path.rsplit_once('/') {
                Some((head, _)) => head.to_owned(),
                None => ".".to_owned(),
            };
            if !manifests.contains(&directory) {
                manifests.push(directory);
            }
        }
    }
    manifests.sort_by(|left, right| {
        (-(text::char_len(left) as i64), left).cmp(&(-(text::char_len(right) as i64), right))
    });

    let relevant = |scope: &Value| -> Result<bool, PyError> {
        if text_of(get(scope, "scope")?) == "repository" {
            return Ok(false);
        }
        let key = text_of(get(scope, "key")?);
        if let Some(name) = key.strip_prefix("domain:")
            && let Some(domain) = contract.domain(name)
        {
            return Ok(selected.iter().any(|path| matches(path, &domain.paths)));
        }
        let anchor = text_of(get(scope, "anchor")?);
        if text_of(get(scope, "scope")?) == "package" {
            return Ok(selected.iter().any(|path| {
                let owner = manifests.iter().find(|manifest| {
                    manifest.as_str() == "." || path.starts_with(&format!("{manifest}/"))
                });
                owner.is_some_and(|found| found == anchor)
            }));
        }
        Ok(selected
            .iter()
            .any(|path| path.starts_with(&format!("{anchor}/"))))
    };
    let mut relevant_scopes: Vec<Value> = Vec::new();
    for scope in scopes {
        if relevant(scope)? {
            relevant_scopes.push(scope.clone());
        }
    }
    relevant_scopes.truncate(8);
    let scope_ids: BTreeSet<&str> = relevant_scopes
        .iter()
        .map(|scope| text_of(&scope["id"]))
        .collect();

    let in_selection = |path: &str| selected.iter().any(|chosen| chosen == path);
    let pattern_matches: Vec<Value> = lookup(fingerprint, "pattern_matches")
        .map(array)
        .unwrap_or_default()
        .iter()
        .filter(|found| {
            scope_ids.contains(text_of(&found["scope_id"]))
                && (truthy(&found["supporting"]) || truthy(&found["contradictory"]))
        })
        .take(12)
        .cloned()
        .collect();
    let relationships: Vec<Value> = lookup(fingerprint, "structural_relationships")
        .map(array)
        .unwrap_or_default()
        .iter()
        .filter(|relation| {
            in_selection(text_of(&relation["source_path"]))
                || lookup(relation, "target_path")
                    .and_then(Value::as_str)
                    .is_some_and(in_selection)
        })
        .take(12)
        .cloned()
        .collect();

    // The relevant scopes are one list, held under two keys.
    let shared = value_list(relevant_scopes);
    let mut fp: Vec<(String, J)> = Vec::new();
    for (key, item) in get(report, "fingerprint")?
        .as_object()
        .cloned()
        .unwrap_or_default()
    {
        fp.push((key, J::from_value(&item)));
    }
    let put = |entries: &mut Vec<(String, J)>, key: &str, value: J| match entries
        .iter_mut()
        .find(|(name, _)| name == key)
    {
        Some(slot) => slot.1 = value,
        None => entries.push((key.to_owned(), value)),
    };
    put(&mut fp, "subsystems", shared.clone());
    put(&mut fp, "pattern_matches", value_list(pattern_matches));
    put(
        &mut fp,
        "structural_relationships",
        value_list(relationships),
    );
    put(
        &mut fp,
        "context_paths",
        list(selected.iter().map(|path| J::Str(path.clone())).collect()),
    );

    let owners: Vec<Value> = contract
        .domains
        .iter()
        .filter(|(_, domain)| selected.iter().any(|path| matches(path, &domain.paths)))
        .map(|(name, domain)| {
            let mut entry = serde_json::Map::new();
            entry.insert("domain".into(), Value::String(name.clone()));
            for (key, value) in domain.dump().as_object().cloned().unwrap_or_default() {
                entry.insert(key, value);
            }
            Value::Object(entry)
        })
        .collect();
    let examples: Vec<Value> = candidates
        .iter()
        .filter(|(_, path)| in_selection(path))
        .map(|(file, path)| {
            serde_json::json!({
                "path": path,
                "node_id": file["id"],
                "revision": graph["revision"],
                "origin": "observed",
            })
        })
        .collect();
    let findings: Vec<Value> = array(get(report, "findings")?)
        .iter()
        .filter(|finding| {
            array(&finding["paths"])
                .iter()
                .any(|path| in_selection(text_of(path)))
        })
        .cloned()
        .collect();
    let entries: Vec<(String, J)> = vec![
        ("schema_version".into(), J::Str("1".into())),
        (
            "repository".into(),
            J::from_value(get(graph, "repository")?),
        ),
        ("revision".into(), J::from_value(get(graph, "revision")?)),
        ("task".into(), J::Str(task.to_owned())),
        ("fingerprint".into(), dict(fp)),
        (
            "policy_digest".into(),
            J::from_value(get(report, "policy_digest")?),
        ),
        (
            "policy_approved".into(),
            J::from_value(get(report, "policy_approved")?),
        ),
        ("owners".into(), value_list(owners)),
        ("relevant_subsystems".into(), shared),
        (
            "rules".into(),
            value_list(rules.iter().map(|(_, rule)| rule.dump()).collect()),
        ),
        ("examples".into(), value_list(examples)),
        ("findings".into(), value_list(findings)),
        ("partial".into(), J::from_value(get(report, "partial")?)),
        ("notice".into(), J::from_value(get(report, "notice")?)),
    ];
    Ok(dict(entries))
}

const KEEP: [&str; 17] = [
    "id",
    "key",
    "name",
    "scope",
    "origin",
    "repository",
    "revision",
    "owner",
    "owner_approved",
    "member_count",
    "member_files",
    "public_interfaces",
    "evidence",
    "partial",
    "review_status",
    "boundary_confidence",
    "confidence_calibration",
];

fn mark_partial(result: &J, presentation: &Option<J>) {
    result.set("partial", J::Bool(true));
    if let Some(shown) = presentation
        && let Some(coverage) = shown.get("coverage")
    {
        coverage.set("response_partial", J::Bool(true));
    }
}

fn dict_keys(value: &J) -> Vec<String> {
    value
        .as_dict()
        .map(|items| items.borrow().iter().map(|(key, _)| key.clone()).collect())
        .unwrap_or_default()
}

/// `bounded(packet, budget)`: the packet within `budget` characters, with what was cut named.
pub fn bounded(packet: &J, budget: i64) -> Result<J, PyError> {
    if !(2000..=24000).contains(&budget) {
        return Err(PyError::value(
            "Architecture response budget must be 2000 to 24000 characters",
        ));
    }
    let result = packet.deep_copy();
    let mut presentation = result.get("presentation");
    // Established small-agent budgets stay as they were; presentation is a larger
    // Inspector projection, not a replacement for source guidance.
    if presentation.is_some() && budget < 16000 {
        result.remove("presentation");
        result.set("presentation_omitted", J::Bool(true));
        presentation = None;
    }
    let fingerprint = result.get("fingerprint").unwrap_or_else(|| result.clone());
    let mut scopes: Vec<J> = Vec::new();
    for source in [
        fingerprint.get("subsystems"),
        result.get("relevant_subsystems"),
    ]
    .into_iter()
    .flatten()
    {
        if let Some(items) = source.as_list() {
            scopes.extend(items.borrow().iter().cloned());
        }
    }
    for scope in &scopes {
        for field in dict_keys(scope) {
            if !KEEP.contains(&field.as_str()) {
                scope.remove(&field);
                scope.set("partial", J::Bool(true));
                result.set("partial", J::Bool(true));
                if let Some(shown) = &presentation
                    && let Some(coverage) = shown.get("coverage")
                {
                    coverage.set("response_partial", J::Bool(true));
                }
            }
        }
        for field in [
            "member_files",
            "public_interfaces",
            "external_dependencies",
            "internal_dependencies",
            "evidence",
        ] {
            let Some(values) = scope.get(field).and_then(|found| found.as_list().cloned()) else {
                continue;
            };
            let limit = if field == "evidence" { 1 } else { 2 };
            if values.borrow().len() > limit {
                // A slice: the field gets a new list, as Python assigns one.
                let head: Vec<J> = values.borrow().iter().take(limit).cloned().collect();
                scope.set(field, list(head));
                scope.set("partial", J::Bool(true));
                mark_partial(&result, &presentation);
            }
        }
    }
    let mut essential = protected_response_lists(&result, &scopes, budget);
    result.set("budget_limit", J::Number(budget.into()));
    result.set("budget_used", J::Number(0.into()));

    // Every list in the packet except the presentation, found depth first; a list seen
    // twice is listed twice.
    let mut removable: Vec<J> = Vec::new();
    let mut pending: Vec<J> = vec![result.clone()];
    while let Some(item) = pending.pop() {
        if let (Some(shown), Some(address)) = (&presentation, item.identity())
            && shown.identity() == Some(address)
        {
            continue;
        }
        match &item {
            J::Dict(entries) => {
                pending.extend(entries.borrow().iter().map(|(_, value)| value.clone()))
            }
            J::List(entries) => {
                removable.push(item.clone());
                pending.extend(entries.borrow().iter().cloned());
            }
            _ => {}
        }
    }
    loop {
        if let Some(shown) = &presentation
            && let Some(coverage) = shown.get("coverage")
        {
            let returned = result
                .get("findings")
                .and_then(|found| found.as_list().map(|items| items.borrow().len()))
                .unwrap_or(0);
            coverage.set("findings_returned", J::Number(returned.into()));
            let kept = shown
                .get("scopes")
                .and_then(|found| found.as_list().map(|items| items.borrow().len()))
                .unwrap_or(0);
            coverage.set("scopes_returned", J::Number(kept.into()));
        }
        let measured = result.dumps().chars().count() as i64;
        let used = result.get("budget_used").and_then(|found| match found {
            J::Number(number) => number.as_i64(),
            _ => None,
        });
        if used == Some(measured) && measured <= budget {
            return Ok(result);
        }
        result.set("budget_used", J::Number(measured.into()));
        if measured <= budget {
            continue;
        }
        let protected = |value: &J| {
            value
                .identity()
                .and_then(|key| essential.get(&key))
                .copied()
                .unwrap_or(0)
        };
        let lists: Vec<&J> = removable
            .iter()
            .filter(|value| {
                value.as_list().is_some_and(|items| {
                    let length = items.borrow().len();
                    length > 0 && length > protected(value)
                })
            })
            .collect();
        if lists.is_empty() {
            let fp = result.get("fingerprint").unwrap_or_else(|| result.clone());
            let optional = [
                "deployment_topology",
                "anomaly_analysis",
                "structural_coverage",
                "comparison",
            ]
            .into_iter()
            .find(|key| fp.has(key));
            if let Some(key) = optional {
                fp.remove(key);
                fp.set("recognition_partial", J::Bool(true));
                mark_partial(&result, &presentation);
                continue;
            }
            if presentation.is_some() {
                result.remove("presentation");
                result.set("presentation_omitted", J::Bool(true));
                presentation = None;
                continue;
            }
            if let Some(findings) = result.get("findings").filter(|found| found.truthy()) {
                let address = findings.identity().unwrap_or(0);
                if essential.get(&address).copied().unwrap_or(0) > 1 {
                    essential.insert(address, 1);
                } else {
                    if let Some(items) = findings.as_list() {
                        items.borrow_mut().clear();
                    }
                    result.set("findings_omitted", J::Bool(true));
                }
                result.set("partial", J::Bool(true));
                continue;
            }
            return Err(PyError::value(
                "Architecture response envelope exceeds budget",
            ));
        }
        // `max` keeps the first of equal sizes.
        let mut largest = lists[0];
        let mut size = largest.dumps().chars().count();
        for candidate in &lists[1..] {
            let candidate_size = candidate.dumps().chars().count();
            if candidate_size > size {
                largest = candidate;
                size = candidate_size;
            }
        }
        if let Some(items) = largest.as_list() {
            let length = items.borrow().len();
            let keep = protected(largest).max(length / 2);
            items.borrow_mut().truncate(keep);
        }
        mark_partial(&result, &presentation);
    }
}

/// A check report as a tree, with the one object Python shares: the candidate
/// fingerprint's `metrics` is also its comparison's `candidate_metrics`. `bounded`
/// trims by object, so a report must keep that sharing to be trimmed as Python trims it.
pub fn report_tree(report: &Value) -> J {
    let tree = J::from_value(report);
    if let Some(fingerprint) = tree.get("fingerprint")
        && let Some(metrics) = fingerprint.get("metrics")
        && let Some(comparison) = fingerprint.get("comparison")
        && comparison.has("candidate_metrics")
    {
        comparison.set("candidate_metrics", metrics);
    }
    tree
}

/// `bounded` for a packet that is a plain value: lists are not shared.
pub fn bounded_value(packet: &Value, budget: i64) -> Result<Value, PyError> {
    bounded(&J::from_value(packet), budget).map(|found| found.to_value())
}

/// Counts of the lists in a packet by address, for tests of sharing.
pub fn shared_list_count(packet: &J) -> usize {
    let mut counts: HashMap<usize, usize> = HashMap::new();
    let mut pending = vec![packet.clone()];
    while let Some(item) = pending.pop() {
        match &item {
            J::Dict(entries) => {
                pending.extend(entries.borrow().iter().map(|(_, value)| value.clone()))
            }
            J::List(entries) => {
                *counts.entry(item.identity().unwrap_or(0)).or_insert(0) += 1;
                pending.extend(entries.borrow().iter().cloned());
            }
            _ => {}
        }
    }
    counts.values().filter(|count| **count > 1).count()
}

/// Decoded view of a `BTreeMap`, kept so callers can build packets without importing it.
pub fn empty_map() -> BTreeMap<String, Value> {
    BTreeMap::new()
}
