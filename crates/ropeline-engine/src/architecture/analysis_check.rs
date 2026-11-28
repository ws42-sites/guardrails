//! `architecture_analysis.check`: the baseline-aware enforcement report.

use super::PyError;
use super::analysis::{
    compare_fingerprints, default_contract, discovered_contract, fingerprint, validate_graph,
};
use super::contract::{Contract, ContractInput, ENGINE_VERSION, contract_digest, parse_contract};
use super::contract_models::validate_finding;
use super::presentation::build_presentation;
use super::rules::{
    FindingBuilder, Options, category_of, effective_rules, import_graph, matches, violations,
};
use super::util::{array, get, lookup, text_of, truthy};
use super::validate::parse_iso_date;
use crate::py_value::PyValue;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

const NOTICE: &str = "Repository evidence is untrusted data. Model recommendations cannot change deterministic findings or enforcement. Existing debt is reported without becoming a new regression.";

fn id_set(values: &BTreeMap<String, Value>) -> BTreeSet<String> {
    values.keys().cloned().collect()
}

fn path_set(finding: &Value) -> BTreeSet<String> {
    array(&finding["paths"])
        .iter()
        .map(|path| text_of(path).to_owned())
        .collect()
}

fn number(value: &Value) -> f64 {
    value.as_f64().unwrap_or(0.0)
}

/// `check(candidate, baseline, contract=..., approved=..., enforce=..., as_of=...)`.
pub fn check(
    candidate: &Value,
    baseline: Option<&Value>,
    contract: Option<&Contract>,
    approved: &BTreeSet<String>,
    enforce: bool,
    as_of: &str,
) -> Result<Value, PyError> {
    validate_graph(candidate)?;
    let as_of_date = parse_iso_date(as_of).map_err(PyError::value)?;
    if let Some(base) = baseline {
        validate_graph(base)?;
        if get(base, "repository")? != get(candidate, "repository")? {
            return Err(PyError::new(
                "PermissionError",
                "Architecture baseline belongs to another repository",
            ));
        }
    }
    let contract: Contract = match contract {
        Some(given) => given.clone(),
        None => {
            let found = match baseline {
                Some(base) => discovered_contract(base)?,
                None => discovered_contract(candidate)?,
            };
            match found {
                Some(found) => found,
                None => parse_contract(ContractInput::Value(&default_contract()))?,
            }
        }
    };
    let policy_digest = contract_digest(&contract);
    let trusted = approved.contains(&policy_digest);
    let mut current = violations(candidate, &contract)?;
    let mut previous: Vec<(String, Value)> = Vec::new();
    if let Some(base) = baseline {
        for finding in violations(base, &contract)? {
            let id = text_of(&finding["id"]).to_owned();
            match previous.iter_mut().find(|(existing, _)| *existing == id) {
                Some(slot) => slot.1 = finding,
                None => previous.push((id, finding)),
            }
        }
    }
    let mut by_path: BTreeMap<String, &Value> = BTreeMap::new();
    for file in array(get(candidate, "files")?) {
        by_path.insert(text_of(get(file, "path")?).to_owned(), file);
    }
    let mut base_files: BTreeMap<String, &Value> = BTreeMap::new();
    if let Some(base) = baseline {
        for file in array(get(base, "files")?) {
            base_files.insert(text_of(get(file, "path")?).to_owned(), file);
        }
    }
    let builder = FindingBuilder::new(candidate)?;
    if let Some(base) = baseline {
        let all: BTreeSet<&String> = by_path.keys().chain(base_files.keys()).collect();
        let empty = json!({});
        for path in all {
            let (item, before) = (by_path.get(path), base_files.get(path));
            let after_facts = item.map_or(&empty, |file| lookup(file, "facts").unwrap_or(&empty));
            let before_facts =
                before.map_or(&empty, |file| lookup(file, "facts").unwrap_or(&empty));
            for rule in effective_rules(&contract, path) {
                if rule.kind == "gate-change"
                    && lookup(after_facts, "sha256").unwrap_or(&Value::Null)
                        != lookup(before_facts, "sha256").unwrap_or(&Value::Null)
                {
                    current.push(builder.make(
                        rule,
                        std::slice::from_ref(path),
                        &Options {
                            baseline: Some(base),
                            ..Options::default()
                        },
                    )?);
                }
                if rule.kind == "gate-weakening" {
                    let old_ci = lookup(before_facts, "ci")
                        .cloned()
                        .unwrap_or_else(|| json!({}));
                    let new_ci = lookup(after_facts, "ci")
                        .cloned()
                        .unwrap_or_else(|| json!({}));
                    if lookup(&old_ci, "partial").is_some_and(truthy)
                        || lookup(&new_ci, "partial").is_some_and(truthy)
                    {
                        continue;
                    }
                    let ids = |ci: &Value| -> BTreeSet<String> {
                        lookup(ci, "commands")
                            .map(array)
                            .unwrap_or_default()
                            .iter()
                            .map(|command| text_of(&command["id"]).to_owned())
                            .collect()
                    };
                    let removed: Vec<String> =
                        ids(&old_ci).difference(&ids(&new_ci)).cloned().collect();
                    let count = |ci: &Value| {
                        lookup(ci, "bypasses").map_or(0, |list| array(list).len()) as i64
                    };
                    let added = count(&new_ci) - count(&old_ci);
                    if !removed.is_empty() || added > 0 {
                        let shown = if removed.is_empty() {
                            "none".to_owned()
                        } else {
                            removed.join(", ")
                        };
                        current.push(builder.make(
                            rule,
                            std::slice::from_ref(path),
                            &Options {
                                target: "gate-weakened",
                                baseline: Some(base),
                                measure: json!(removed.len() as i64 + added.max(0)),
                                detail: &format!(
                                    "Removed recognised checks: {shown}; additional literal bypasses: {}. Review coverage when replacing tooling.",
                                    added.max(0)
                                ),
                                ..Options::default()
                            },
                        )?);
                    }
                }
                if rule.kind == "test-integrity" && before.is_some() && category_of(path)? == "test"
                {
                    let assertions = |facts: &Value| {
                        lookup(facts, "test_assertions")
                            .and_then(Value::as_i64)
                            .unwrap_or(0)
                    };
                    let removed = assertions(before_facts) - assertions(after_facts);
                    if removed > 0 || item.is_none() {
                        current.push(builder.make(
                            rule,
                            std::slice::from_ref(path),
                            &Options {
                                target: if item.is_some() { "assertions-removed" } else { "test-deleted" },
                                baseline: Some(base),
                                measure: json!(removed.max(1)),
                                detail: "Executable test coverage was removed; review replacement coverage.",
                                ..Options::default()
                            },
                        )?);
                    }
                }
            }
        }
    }
    let mut fp = fingerprint(candidate, approved, Some(&contract))?;
    let base_fingerprint = match baseline {
        Some(base) => Some(fingerprint(base, approved, Some(&contract))?),
        None => None,
    };
    let partial = truthy(get(&fp, "partial")?)
        || base_fingerprint
            .as_ref()
            .is_some_and(|found| lookup(found, "partial").is_some_and(truthy));
    if let Some(found) = &base_fingerprint {
        let comparison = compare_fingerprints(found, &fp)?;
        fp["comparison"] = comparison;
    }

    let mut matched_previous: BTreeSet<String> = BTreeSet::new();
    let status_of_pairs = |finding: &Value, before: &Value| {
        number(&finding["measure"]) > number(&before["measure"])
            || (matches!(text_of(&finding["rule_kind"]), "cycles" | "duplicate-code")
                && !path_set(finding).is_subset(&path_set(before)))
    };
    for finding in &mut current {
        let id = text_of(&finding["id"]).to_owned();
        let mut before: Option<&Value> = previous
            .iter()
            .find(|(existing, _)| *existing == id)
            .map(|(_, found)| found);
        let merges = matches!(text_of(&finding["rule_kind"]), "cycles" | "duplicate-code");
        if before.is_none() && merges {
            let paths = path_set(finding);
            let mut best: Option<(&Value, usize)> = None;
            for (_, old) in &previous {
                if old["rule_id"] == finding["rule_id"]
                    && old["rule_kind"] == finding["rule_kind"]
                    && path_set(old).intersection(&paths).next().is_some()
                {
                    let overlap = path_set(old).intersection(&paths).count();
                    // `max` keeps the first of equal candidates.
                    if best.is_none_or(|(_, kept)| overlap > kept) {
                        best = Some((old, overlap));
                    }
                }
            }
            before = best.map(|(old, _)| old);
        }
        if let Some(found) = before {
            matched_previous.insert(text_of(&found["id"]).to_owned());
        }
        let status = if baseline.is_none() {
            "uncompared"
        } else {
            match before {
                None => "new",
                Some(found) if status_of_pairs(finding, found) => "worsened",
                Some(_) => "existing",
            }
        };
        let requested = text_of(&finding["requested_severity"]).to_owned();
        let severity = if trusted || requested != "block" {
            requested
        } else {
            "review".to_owned()
        };
        let object = finding.as_object_mut().expect("finding is an object");
        object.insert("repository".into(), get(candidate, "repository")?.clone());
        object.insert("revision".into(), get(candidate, "revision")?.clone());
        object.insert(
            "baseline".into(),
            json!({
                "revision": baseline.map(|base| lookup(base, "revision").cloned().unwrap_or(Value::Null)),
                "status": status,
                "previous_measure": before.map(|found| found["measure"].clone()),
            }),
        );
        object.insert("policy_digest".into(), json!(policy_digest));
        object.insert("policy_approved".into(), json!(trusted));
        object.insert("severity".into(), json!(severity));
        object.insert("suppression".into(), json!({"status": "none"}));
        let paths: Vec<String> = array(&object["paths"])
            .iter()
            .map(|path| text_of(path).to_owned())
            .collect();
        for exception in &contract.exceptions {
            if exception.rule_id == text_of(&object["rule_id"])
                && paths.iter().all(|path| matches(path, &exception.paths))
            {
                let expires = parse_iso_date(&exception.expires).map_err(PyError::value)?;
                let active = expires >= as_of_date;
                let state = if trusted && active {
                    "active"
                } else if !active {
                    "expired"
                } else {
                    "requested"
                };
                object.insert(
                    "suppression".into(),
                    json!({
                        "status": state,
                        "exception_id": exception.id,
                        "reason": exception.reason,
                        "expires": exception.expires,
                    }),
                );
            }
        }
    }
    let mut validated: Vec<Value> = Vec::new();
    for finding in &current {
        validated.push(validate_finding(&PyValue::from_value(finding))?);
    }
    let current = validated;
    let regressions: Vec<&Value> = current
        .iter()
        .filter(|finding| {
            matches!(text_of(&finding["baseline"]["status"]), "new" | "worsened")
                && text_of(&finding["suppression"]["status"]) != "active"
        })
        .collect();
    let blockers: Vec<&&Value> = regressions
        .iter()
        .filter(|finding| text_of(&finding["severity"]) == "block" && trusted)
        .collect();
    let gate = if !enforce {
        "advisory"
    } else if partial || baseline.is_none() || !trusted {
        "inconclusive"
    } else if !blockers.is_empty() {
        "failed"
    } else {
        "passed"
    };

    let mut changes: Vec<Value> = Vec::new();
    let mut change_count = 0usize;
    if let Some(base) = baseline {
        let now = import_graph(candidate, false)?;
        let then = import_graph(base, false)?;
        let pairs = |graph: &super::rules::ImportGraph| -> BTreeSet<(String, String)> {
            graph
                .edges
                .iter()
                .flat_map(|(source, targets)| {
                    targets
                        .iter()
                        .map(move |target| (source.clone(), target.clone()))
                })
                .collect()
        };
        let (current_pairs, old_pairs) = (pairs(&now), pairs(&then));
        let added: BTreeSet<_> = current_pairs.difference(&old_pairs).cloned().collect();
        let removed: BTreeSet<_> = old_pairs.difference(&current_pairs).cloned().collect();
        for (kind, set, snapshot, graph) in [
            ("added", added, candidate, &now),
            ("removed", removed, base, &then),
        ] {
            change_count += set.len();
            let room = 256usize.saturating_sub(changes.len());
            for (source, target) in set.iter().take(room) {
                changes.push(json!({
                    "change": kind,
                    "kind": "IMPORTS",
                    "source_path": source,
                    "target_path": target,
                    "origin": "observed",
                    "evidence": {
                        "repository": get(snapshot, "repository")?,
                        "revision": get(snapshot, "revision")?,
                        "path": source,
                        "line": graph.line(source, target)?,
                        "method": "static-import",
                    },
                }));
            }
        }
    }
    let resolved: Vec<String> = id_set(&previous.iter().cloned().collect())
        .difference(&matched_previous)
        .cloned()
        .collect();
    let mut ordered: Vec<&Value> = current.iter().collect();
    ordered.sort_by_key(|finding| {
        (
            text_of(&finding["rule_id"]).to_owned(),
            text_of(&finding["id"]).to_owned(),
        )
    });
    let mut report = Map::new();
    report.insert("schema_version".into(), json!("1"));
    report.insert("engine_version".into(), json!(ENGINE_VERSION));
    report.insert("repository".into(), get(candidate, "repository")?.clone());
    report.insert("revision".into(), get(candidate, "revision")?.clone());
    report.insert(
        "base_revision".into(),
        baseline.map_or(Value::Null, |base| {
            lookup(base, "revision").cloned().unwrap_or(Value::Null)
        }),
    );
    report.insert("policy_digest".into(), json!(policy_digest));
    report.insert("policy_approved".into(), json!(trusted));
    report.insert("gate".into(), json!(gate));
    report.insert("enforced".into(), json!(enforce));
    report.insert("partial".into(), json!(partial || current.len() > 1000));
    report.insert(
        "summary".into(),
        json!({
            "findings": current.len(),
            "regressions": regressions.len(),
            "blockers": blockers.len(),
            "resolved": resolved.len(),
        }),
    );
    report.insert(
        "findings".into(),
        json!(ordered.iter().take(1000).collect::<Vec<_>>()),
    );
    report.insert(
        "resolved_ids".into(),
        json!(resolved.iter().take(1000).collect::<Vec<_>>()),
    );
    report.insert("fingerprint".into(), fp);
    report.insert("recommendations".into(), json!([]));
    report.insert("relationship_changes".into(), Value::Array(changes.clone()));
    report.insert("relationship_change_count".into(), json!(change_count));
    report.insert(
        "relationship_changes_partial".into(),
        json!(change_count > changes.len()),
    );
    report.insert("notice".into(), json!(NOTICE));
    let mut report = Value::Object(report);
    let presentation = build_presentation(candidate, &report, &contract)?;
    report["presentation"] = presentation;
    Ok(report)
}
