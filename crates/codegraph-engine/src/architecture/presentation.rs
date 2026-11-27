//! Bounded presentation of existing architecture evidence, with no new classifier:
//! port of `architecture_presentation.py`.

use super::PyError;
use super::contract::{Contract, validate_contract};
use super::contract_models::validate_evidence_in;
use super::tree::{J, dumps};
use super::util::{array, get, lookup, posix_parent, text_of, truthy};
use super::validate::{ANY_TEXT, Loc, SHA256, Validator, optional, required, text, text_max};
use crate::py_value::PyValue;
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};

pub const PRESENTATION_VERSION: &str = "1";
pub const PRESENTATION_LIMIT: usize = 12_000;

fn string_of(value: &J, key: &str) -> Option<String> {
    value.get(key).and_then(|found| found.as_str())
}

/// `protected_response_lists(packet, scopes, budget)`: the list objects budgeting must keep a
/// useful, valid record in, with how many items. Sorts findings (and their evidence) in place
/// when the budget allows the larger response.
pub fn protected_response_lists(packet: &J, scopes: &[J], budget: i64) -> HashMap<usize, usize> {
    let mut protected: HashMap<usize, usize> = HashMap::new();
    if budget < 4000 {
        return protected;
    }
    let fingerprint = packet.get("fingerprint").unwrap_or_else(|| packet.clone());
    let named = [
        fingerprint.get("subsystems"),
        packet.get("relevant_subsystems"),
        packet.get("owners"),
        packet.get("examples"),
    ];
    for values in named.into_iter().flatten() {
        if values.truthy()
            && let Some(address) = values.identity()
        {
            protected.insert(address, 1);
        }
    }
    for scope in scopes {
        if let Some(evidence) = scope.get("evidence")
            && evidence.truthy()
            && let Some(address) = evidence.identity()
        {
            protected.insert(address, 1);
        }
    }
    let Some(findings) = packet.get("findings") else {
        return protected;
    };
    let Some(list) = findings.as_list() else {
        return protected;
    };
    if findings.truthy()
        && let Some(address) = findings.identity()
    {
        let limit = if budget >= 16000 { 3 } else { 1 };
        protected.insert(address, limit.min(list.borrow().len()));
    }
    if budget >= 16000 {
        let rank = |finding: &J| -> (u8, String, String) {
            let status_of = |key: &str| {
                finding
                    .get(key)
                    .and_then(|inner| string_of(&inner, "status"))
                    .unwrap_or_default()
            };
            let active = status_of("suppression") == "active";
            let changed = matches!(status_of("baseline").as_str(), "new" | "worsened");
            let severity = string_of(finding, "severity").unwrap_or_default();
            let approved = finding
                .get("policy_approved")
                .is_some_and(|flag| flag.truthy());
            let rank = if changed && severity == "block" && approved && !active {
                0
            } else if changed && severity == "review" && !active {
                1
            } else if matches!(severity.as_str(), "block" | "review") && !active {
                2
            } else if !active {
                3
            } else {
                4
            };
            (
                rank,
                string_of(finding, "rule_id").unwrap_or_default(),
                string_of(finding, "id").unwrap_or_default(),
            )
        };
        list.borrow_mut().sort_by_cached_key(rank);
        let revision = packet.get("revision");
        for finding in list.borrow().iter() {
            if let Some(evidence) = finding.get("evidence")
                && let Some(proofs) = evidence.as_list()
            {
                proofs.borrow_mut().sort_by_cached_key(|proof| {
                    // `proof.get("revision") != packet.get("revision")`: matching proofs first.
                    let same = match (proof.get("revision"), &revision) {
                        (Some(left), Some(right)) => left.same_scalar(right),
                        (None, None) => true,
                        _ => false,
                    };
                    !same
                });
            }
        }
    }
    for finding in list.borrow().iter() {
        for key in ["paths", "evidence", "affected_nodes"] {
            if let Some(values) = finding.get(key)
                && values.truthy()
                && let Some(address) = values.identity()
            {
                protected.insert(address, 1);
            }
        }
    }
    protected
}

/// The selector union, discriminated by `kind`.
fn validate_selector(v: &mut Validator, input: &PyValue) -> Option<Value> {
    let PyValue::Dict(entries) = input else {
        v.issue(
            "model_attributes_type",
            "Input should be a valid dictionary or object to extract fields from",
        );
        return None;
    };
    let Some((_, tag)) = entries.iter().find(|(key, _)| key == "kind") else {
        v.issue(
            "union_tag_not_found",
            "Unable to extract tag using discriminator 'kind'",
        );
        return None;
    };
    let name = tag.as_str().unwrap_or_default().to_owned();
    if !matches!(
        name.as_str(),
        "domain" | "package" | "directory" | "repository"
    ) {
        v.issue(
            "union_tag_invalid",
            format!(
                "Input tag '{}' found using 'kind' does not match any of the expected tags: 'domain', 'package', 'directory', 'repository'",
                tag.py_str()
            ),
        );
        return None;
    }
    v.at(Loc::Key(name.clone()), |v| {
        let (title, specs) = match name.as_str() {
            "domain" => (
                "DomainSelector",
                vec![optional("kind", json!("domain")), required("patterns")],
            ),
            "package" => (
                "PackageSelector",
                vec![
                    optional("kind", json!("package")),
                    required("anchor"),
                    required("exclude_prefixes"),
                ],
            ),
            "directory" => (
                "DirectorySelector",
                vec![
                    optional("kind", json!("directory")),
                    required("anchor"),
                    required("direct_children_only"),
                ],
            ),
            _ => (
                "RepositorySelector",
                vec![optional("kind", json!("repository"))],
            ),
        };
        v.model(title, input, &specs, |v, field, value| match field {
            "kind" => v.literal(value, &[name.as_str()]).map(Value::String),
            "patterns" => {
                let items = v.str_list(value, Some(1), Some(100), ANY_TEXT)?;
                if items.iter().any(|item| {
                    !super::contract::is_safe_path(item.as_str().unwrap_or_default(), true)
                }) {
                    v.value_error("Invalid architecture source path");
                    return None;
                }
                Some(Value::Array(items))
            }
            "anchor" => {
                let anchor = v.str_field(value, ANY_TEXT)?;
                let allowed = (name == "package" && anchor == ".")
                    || super::contract::is_safe_path(&anchor, false);
                if allowed {
                    Some(Value::String(anchor))
                } else {
                    v.value_error("Invalid architecture source path");
                    None
                }
            }
            "exclude_prefixes" => {
                let items = v.str_list(value, None, Some(20_000), ANY_TEXT)?;
                if items.iter().any(|item| {
                    !super::contract::is_safe_path(item.as_str().unwrap_or_default(), false)
                }) {
                    v.value_error("Invalid architecture source path");
                    return None;
                }
                Some(Value::Array(items))
            }
            _ => v.bool_field(value).map(Value::Bool),
        })
        .map(Value::Object)
    })
}

fn validate_scope(v: &mut Validator, input: &PyValue) -> Option<Value> {
    let specs = [
        required("id"),
        required("key"),
        required("name"),
        optional("role", json!("unclassified")),
        required("origin"),
        optional("owner", Value::Null),
        optional("owner_approved", json!(false)),
        required("member_count"),
        required("selector"),
        required("evidence"),
        required("partial"),
    ];
    v.model(
        "PresentedScope",
        input,
        &specs,
        |v, name, value| match name {
            "id" => v.str_field(value, text(1, 128)).map(Value::String),
            "key" | "name" => v.str_field(value, text(1, 4096)).map(Value::String),
            "role" => v.literal(value, &["unclassified"]).map(Value::String),
            "origin" => v
                .literal(value, &super::contract::ORIGINS)
                .map(Value::String),
            "owner" => v.nullable_str(value, text_max(128)),
            "owner_approved" | "partial" => v.bool_field(value).map(Value::Bool),
            "member_count" => v
                .int_field(value, Some(0), None)
                .map(|int| Value::Number(int.into())),
            "selector" => validate_selector(v, value),
            _ => v
                .list_field(value, None, Some(3), validate_evidence_in)
                .map(Value::Array),
        },
    )
    .map(Value::Object)
}

fn validate_coverage(v: &mut Validator, input: &PyValue) -> Option<Value> {
    let specs = [
        required("source_partial"),
        required("recognition_partial"),
        required("response_partial"),
        required("files_seen"),
        required("parse_error_count"),
        required("parse_error_paths"),
        required("scope_count"),
        required("scopes_returned"),
        required("findings_total"),
        required("findings_returned"),
    ];
    v.model(
        "PresentationCoverage",
        input,
        &specs,
        |v, name, value| match name {
            "source_partial" | "recognition_partial" | "response_partial" => {
                v.bool_field(value).map(Value::Bool)
            }
            "parse_error_paths" => v
                .str_list(value, None, Some(100), ANY_TEXT)
                .map(Value::Array),
            _ => v
                .int_field(value, Some(0), None)
                .map(|int| Value::Number(int.into())),
        },
    )
    .map(Value::Object)
}

fn validate_policy(v: &mut Validator, input: &PyValue) -> Option<Value> {
    let specs = [
        required("digest"),
        required("approved"),
        required("enforced"),
        required("contract"),
    ];
    v.model(
        "PresentationPolicy",
        input,
        &specs,
        |v, name, value| match name {
            "digest" => v.str_field(value, SHA256).map(Value::String),
            "approved" | "enforced" => v.bool_field(value).map(Value::Bool),
            _ if matches!(value, PyValue::Null) => Some(Value::Null),
            _ => validate_contract(v, value),
        },
    )
    .map(Value::Object)
}

/// `ArchitecturePresentation.model_validate(data).model_dump()`.
pub fn validate_presentation(input: &PyValue) -> Result<Value, PyError> {
    let mut v = Validator::new();
    let specs = [
        optional("schema_version", json!("1")),
        required("repository"),
        required("revision"),
        required("snapshot_id"),
        required("view_id"),
        required("engine_version"),
        required("base_revision"),
        required("coverage"),
        required("policy"),
        required("scopes"),
    ];
    let dumped = v
        .model(
            "ArchitecturePresentation",
            input,
            &specs,
            |v, name, value| match name {
                "schema_version" => v.literal(value, &["1"]).map(Value::String),
                "repository" => v.str_field(value, text(1, 1024)).map(Value::String),
                "revision" => v.str_field(value, text(1, 256)).map(Value::String),
                "snapshot_id" => v.str_field(value, text_max(32)).map(Value::String),
                "view_id" => v.str_field(value, text(1, 1100)).map(Value::String),
                "engine_version" => v.str_field(value, text(1, 128)).map(Value::String),
                "base_revision" => v.nullable_str(value, text_max(256)),
                "coverage" => validate_coverage(v, value),
                "policy" => validate_policy(v, value),
                _ => v
                    .list_field(value, None, Some(32), validate_scope)
                    .map(Value::Array),
            },
        )
        .map(Value::Object);
    match dumped {
        Some(found) if v.issues.is_empty() => Ok(found),
        _ => Err(PyError::validation("ArchitecturePresentation", v.issues)),
    }
}

fn priority_of(kind: &str) -> u8 {
    match kind {
        "domain" => 0,
        "package" => 1,
        "directory" => 2,
        _ => 3,
    }
}

/// `build_presentation(graph, report, contract)`: the scopes a browser can join to real node ids.
pub fn build_presentation(
    graph: &Value,
    report: &Value,
    contract: &Contract,
) -> Result<Value, PyError> {
    let fingerprint = get(report, "fingerprint")?;
    let mut packages: BTreeSet<String> = BTreeSet::new();
    for file in array(get(graph, "files")?) {
        let named = lookup(get(file, "facts")?, "manifest")
            .and_then(|manifest| lookup(manifest, "name"))
            .is_some_and(truthy);
        if named {
            packages.insert(posix_parent(text_of(get(file, "path")?)));
        }
    }
    let mut scopes: Vec<Value> = Vec::new();
    for scope in lookup(fingerprint, "subsystems")
        .map(array)
        .unwrap_or_default()
    {
        let key = text_of(get(scope, "key")?);
        let anchor = text_of(get(scope, "anchor")?);
        let selector = if key == "repository" {
            json!({"kind": "repository"})
        } else if let Some(domain) = key.strip_prefix("domain:") {
            let Some(found) = contract.domain(domain) else {
                continue;
            };
            json!({"kind": "domain", "patterns": found.paths})
        } else if key.starts_with("package:") {
            let excluded: Vec<&String> = packages
                .iter()
                .filter(|package| {
                    package.as_str() != anchor
                        && (anchor == "." || package.starts_with(&format!("{anchor}/")))
                })
                .collect();
            json!({"kind": "package", "anchor": anchor, "exclude_prefixes": excluded})
        } else {
            json!({
                "kind": "directory",
                "anchor": anchor,
                "direct_children_only": matches!(anchor, "src" | "apps" | "packages"),
            })
        };
        let evidence: Vec<&Value> = array(get(scope, "evidence")?).iter().take(1).collect();
        scopes.push(json!({
            "id": get(scope, "id")?,
            "key": key,
            "name": get(scope, "name")?,
            "role": "unclassified",
            "origin": get(scope, "origin")?,
            "owner": lookup(scope, "owner").cloned().unwrap_or(Value::Null),
            "owner_approved": lookup(scope, "owner_approved").cloned().unwrap_or(json!(false)),
            "member_count": get(scope, "member_count")?,
            "selector": selector,
            "evidence": evidence,
            "partial": truthy(get(scope, "partial")?),
        }));
    }
    scopes.sort_by(|left, right| {
        let rank = |scope: &Value| priority_of(text_of(&scope["selector"]["kind"]));
        (rank(left), text_of(&left["id"])).cmp(&(rank(right), text_of(&right["id"])))
    });
    let errors: Vec<Value> = lookup(fingerprint, "coverage")
        .and_then(|coverage| lookup(coverage, "parse_errors"))
        .map(|list| array(list).to_vec())
        .unwrap_or_default();
    let coverage = lookup(fingerprint, "coverage");
    let count_of = |key: &str, default: usize| -> Value {
        coverage
            .and_then(|found| lookup(found, key))
            .cloned()
            .unwrap_or_else(|| json!(default))
    };
    let snapshot = match lookup(graph, "snapshot") {
        Some(found) => PyValue::from_value(found).py_str(),
        None => String::new(),
    };
    let subsystem_count = lookup(fingerprint, "subsystem_count")
        .cloned()
        .unwrap_or_else(|| json!(scopes.len()));
    let response_partial = subsystem_count
        .as_u64()
        .is_some_and(|count| (scopes.len() as u64) < count);
    let candidate = json!({
        "repository": get(graph, "repository")?,
        "revision": get(graph, "revision")?,
        "snapshot_id": snapshot,
        "view_id": lookup(graph, "view_id").cloned().unwrap_or(json!("hosted")),
        "engine_version": get(report, "engine_version")?,
        "base_revision": get(report, "base_revision")?,
        "coverage": {
            "source_partial": lookup(fingerprint, "partial").is_some_and(truthy),
            "recognition_partial": lookup(fingerprint, "recognition_partial").is_some_and(truthy),
            "response_partial": response_partial,
            "files_seen": get(get(fingerprint, "metrics")?, "files")?,
            "parse_error_count": count_of("parse_error_count", errors.len()),
            "parse_error_paths": errors.iter().take(100).collect::<Vec<_>>(),
            "scope_count": subsystem_count,
            "scopes_returned": scopes.len(),
            "findings_total": get(get(report, "summary")?, "findings")?,
            "findings_returned": array(get(report, "findings")?).len(),
        },
        "policy": {
            "digest": get(report, "policy_digest")?,
            "approved": get(report, "policy_approved")?,
            "enforced": get(report, "enforced")?,
            "contract": contract.dump,
        },
        "scopes": scopes,
    });
    let mut data = validate_presentation(&PyValue::from_value(&candidate))?;
    let size = |value: &Value| dumps(value).chars().count();
    let set = |data: &mut Value, section: &str, key: &str, value: Value| {
        data[section][key] = value;
    };
    // The selector is atomic: omitting an exclusion would falsely assign files.
    if size(&data) > PRESENTATION_LIMIT {
        data["policy"]["contract"] = Value::Null;
        set(&mut data, "coverage", "response_partial", json!(true));
    }
    while size(&data) > PRESENTATION_LIMIT && !scopes.is_empty() {
        let last_is_repository = scopes
            .last()
            .is_some_and(|scope| text_of(&scope["selector"]["kind"]) == "repository");
        let position = if scopes.len() > 1 && last_is_repository {
            scopes.len() - 2
        } else {
            scopes.len() - 1
        };
        scopes.remove(position);
        data["scopes"] = Value::Array(scopes.clone());
        set(
            &mut data,
            "coverage",
            "scopes_returned",
            json!(scopes.len()),
        );
        set(&mut data, "coverage", "response_partial", json!(true));
    }
    if size(&data) > PRESENTATION_LIMIT {
        set(&mut data, "coverage", "parse_error_paths", json!([]));
        set(&mut data, "coverage", "response_partial", json!(true));
    }
    Ok(data)
}

/// The recorded shape of a protected-list map: each list's address becomes the places it sits.
pub fn describe_protected(packet: &J, protected: &HashMap<usize, usize>) -> Value {
    fn walk(value: &J, path: &mut Vec<String>, found: &mut Vec<(usize, String)>) {
        match value {
            J::List(items) => {
                found.push((Rc::as_ptr(items) as usize, path.join("/")));
                for (index, item) in items.borrow().iter().enumerate() {
                    path.push(index.to_string());
                    walk(item, path, found);
                    path.pop();
                }
            }
            J::Dict(items) => {
                for (key, item) in items.borrow().iter() {
                    path.push(key.clone());
                    walk(item, path, found);
                    path.pop();
                }
            }
            _ => {}
        }
    }
    use std::rc::Rc;
    let mut found = Vec::new();
    walk(packet, &mut Vec::new(), &mut found);
    let mut entries: Vec<Value> = Vec::new();
    for (address, count) in protected {
        let mut places: Vec<&str> = found
            .iter()
            .filter(|(candidate, _)| candidate == address)
            .map(|(_, place)| place.as_str())
            .collect();
        places.sort_unstable();
        entries.push(json!({"paths": places, "count": count}));
    }
    entries.sort_by_key(|entry| entry.to_string());
    Value::Array(entries)
}
