//! Hierarchical, source-backed scopes: port of `architecture_recognition.py`.
//! Communities are candidates, not domains.

use super::PyError;
use super::contract::{Contract, MAX_EDGES, contract_digest, digest};
use super::evidence::RECOGNITION_VERSION;
use super::rules::{category_of, import_graph, matches};
use super::util::{array, get, lookup, posix_parent, text_of, truthy};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub use super::evidence::evidence;

pub const VERSION: &str = RECOGNITION_VERSION;
pub const SCOPE_LIMIT: usize = 32;

const NOTICE: &str =
    "Source relationships describe static declarations and syntax, not observed runtime behaviour.";
const PORTFOLIO_SCOPE: &str = "This authorised repository only; no implicit cross-repository scan.";
const ALTERNATIVE: &str = "A package or folder may contain several responsibilities; dependency communities are not business domains.";

/// The files of a snapshot keyed by path, in order; a repeated path keeps its place and takes the later file.
fn files_by_path(graph: &Value) -> Result<Vec<(String, &Value)>, PyError> {
    let mut files: Vec<(String, &Value)> = Vec::new();
    for file in array(get(graph, "files")?) {
        let path = text_of(get(file, "path")?).to_owned();
        match files.iter_mut().find(|(existing, _)| *existing == path) {
            Some(slot) => slot.1 = file,
            None => files.push((path, file)),
        }
    }
    Ok(files)
}

fn structure_of(file: &Value) -> Result<Value, PyError> {
    Ok(lookup(get(file, "facts")?, "structure")
        .cloned()
        .unwrap_or_else(|| json!({})))
}

fn is_empty(value: &Value) -> bool {
    !truthy(value)
}

fn flag(value: &Value, key: &str) -> bool {
    lookup(value, key).is_some_and(truthy)
}

/// `structural_graph(graph)`: every declared relationship and effect, resolved against imports.
pub fn structural_graph(graph: &Value) -> Result<Value, PyError> {
    let adjacency = import_graph(graph, true)?;
    let files = files_by_path(graph)?;
    let index: HashMap<&str, &Value> = files
        .iter()
        .map(|(path, file)| (path.as_str(), *file))
        .collect();
    let mut declarations: HashMap<&str, Vec<Value>> = HashMap::new();
    let mut names_by_path: HashMap<&str, BTreeSet<String>> = HashMap::new();
    let (mut code_files, mut covered, mut unresolved, mut count) = (0u64, 0u64, 0u64, 0u64);
    let (mut partial, mut signature_partial) = (false, false);
    let (mut unresolved_samples, mut confirmed_count) = (0u64, 0u64);
    let mut relations: Vec<Value> = Vec::new();
    let mut effects: Vec<Value> = Vec::new();

    for (path, file) in &files {
        let structure = structure_of(file)?;
        let language = lookup(get(file, "metadata")?, "language");
        if language.is_none_or(|found| found != "text") {
            code_files += 1;
            covered += u64::from(flag(&structure, "supported"));
            partial |= flag(&structure, "partial") || is_empty(&structure);
            let confirmed = lookup(&structure, "confirmed_facts_partial")
                .or_else(|| lookup(&structure, "partial"))
                .is_some_and(truthy);
            signature_partial |= confirmed || is_empty(&structure);
        }
        let found: Vec<Value> = lookup(&structure, "declarations")
            .map(|list| array(list).to_vec())
            .unwrap_or_default();
        names_by_path.insert(
            path,
            found
                .iter()
                .map(|item| text_of(get(item, "name").unwrap_or(&Value::Null)).to_owned())
                .collect(),
        );
        declarations.insert(path, found);
    }

    for (path, file) in &files {
        let structure = structure_of(file)?;
        for item in lookup(&structure, "relations")
            .map(array)
            .unwrap_or_default()
        {
            let mut kind = text_of(get(item, "kind")?).to_owned();
            let target = text_of(get(item, "target")?).to_owned();
            count += 1;
            if kind == "UNRESOLVED_CALL" {
                unresolved += 1;
                if unresolved_samples >= 128 {
                    continue;
                }
                unresolved_samples += 1;
            } else {
                confirmed_count += 1;
                if confirmed_count > MAX_EDGES as u64 {
                    signature_partial = true;
                    continue;
                }
            }
            let candidates: Vec<&str> = std::iter::once(path.as_str())
                .chain(adjacency.targets(path).iter().map(String::as_str))
                .collect();
            let mut resolved_paths: Vec<&str> = Vec::new();
            for candidate in candidates {
                let names = names_by_path
                    .get(candidate)
                    .ok_or_else(|| PyError::new("KeyError", format!("'{candidate}'")))?;
                if names.contains(&target) {
                    resolved_paths.push(candidate);
                }
            }
            let target_path = (resolved_paths.len() == 1).then(|| resolved_paths[0]);
            if matches!(kind.as_str(), "IMPLEMENTS" | "EXTENDS") {
                let in_target: &[Value] = target_path
                    .and_then(|found| declarations.get(found))
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                let interfaces: Vec<&Value> = in_target
                    .iter()
                    .filter(|item| {
                        item["name"].as_str() == Some(target.as_str())
                            && item["kind"].as_str() == Some("interface")
                    })
                    .collect();
                if let Some(interface) = interfaces.first() {
                    let wanted: BTreeSet<&str> =
                        array(lookup(interface, "methods").unwrap_or(&Value::Null))
                            .iter()
                            .filter_map(Value::as_str)
                            .collect();
                    let source = text_of(get(item, "source")?);
                    let implemented = declarations[path.as_str()]
                        .iter()
                        .filter(|found| {
                            found["kind"].as_str() == Some("class")
                                && found["name"].as_str() == Some(source)
                        })
                        .any(|found| {
                            let offered: BTreeSet<&str> =
                                array(lookup(found, "methods").unwrap_or(&Value::Null))
                                    .iter()
                                    .filter_map(Value::as_str)
                                    .collect();
                            wanted.is_subset(&offered)
                        });
                    kind = if implemented {
                        "IMPLEMENTS".into()
                    } else {
                        "UNRESOLVED_IMPLEMENTATION".into()
                    };
                } else if kind == "IMPLEMENTS" {
                    kind = "UNRESOLVED_IMPLEMENTATION".into();
                }
            }
            if kind != "UNRESOLVED_CALL" && kind.starts_with("UNRESOLVED") {
                unresolved += 1;
            }
            if relations.len() < MAX_EDGES + 128 {
                let mut relation = item.as_object().cloned().unwrap_or_default();
                relation.insert("kind".into(), json!(kind));
                relation.insert("source_path".into(), json!(path));
                relation.insert("target_path".into(), json!(target_path));
                let target_node = match target_path {
                    Some(found) => get(index[found], "id")?.clone(),
                    None => Value::Null,
                };
                relation.insert("target_node".into(), target_node);
                relation.insert(
                    "evidence".into(),
                    evidence(
                        graph,
                        file,
                        get(item, "line")?,
                        lookup(&structure, "version")
                            .and_then(Value::as_str)
                            .unwrap_or(VERSION),
                    )?,
                );
                relations.push(Value::Object(relation));
            }
        }
        for effect in lookup(&structure, "effects").map(array).unwrap_or_default() {
            if effects.len() < 2048 {
                let mut found = effect.as_object().cloned().unwrap_or_default();
                found.insert("path".into(), json!(path));
                found.insert("origin".into(), json!("inferred"));
                found.insert(
                    "evidence".into(),
                    evidence(
                        graph,
                        file,
                        get(effect, "line")?,
                        "import-bound-call-syntax",
                    )?,
                );
                effects.push(Value::Object(found));
            } else {
                partial = true;
            }
        }
    }
    Ok(json!({
        "version": VERSION,
        "relations": relations,
        "effects": effects,
        "coverage": {
            "code_files": code_files,
            "structural_files": covered,
            "relations": count,
            "unresolved_relations": unresolved,
        },
        "partial": partial || count as usize > relations.len(),
        "signature_coverage_partial": signature_partial,
        "notice": NOTICE,
    }))
}

/// One candidate scope while subsystems are being found.
struct Group {
    members: BTreeSet<String>,
    meta: Map<String, Value>,
}

/// `PurePosixPath(path).parts` for a path with no empty or `.` parts.
fn path_parts(path: &str) -> Vec<&str> {
    path.split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect()
}

/// `discover_subsystems(graph, contract, approved)`.
pub fn discover_subsystems(
    graph: &Value,
    contract: Option<&Contract>,
    approved: &BTreeSet<String>,
) -> Result<Value, PyError> {
    let adjacency = import_graph(graph, false)?;
    let files = files_by_path(graph)?;
    let index: HashMap<&str, &Value> = files
        .iter()
        .map(|(path, file)| (path.as_str(), *file))
        .collect();
    let repository = get(graph, "repository")?.clone();
    let mut groups: Vec<(String, Group)> = Vec::new();

    let mut base_meta = Map::new();
    base_meta.insert("name".into(), repository.clone());
    base_meta.insert("scope".into(), json!("repository"));
    base_meta.insert("origin".into(), json!("observed"));
    base_meta.insert("anchor".into(), json!("."));
    groups.push((
        "repository".into(),
        Group {
            members: files.iter().map(|(path, _)| path.clone()).collect(),
            meta: base_meta,
        },
    ));
    let mut packages: Vec<&str> = Vec::new();
    for (path, file) in &files {
        let manifest = lookup(get(file, "facts")?, "manifest");
        if manifest
            .and_then(|found| lookup(found, "name"))
            .is_some_and(truthy)
        {
            packages.push(path);
        }
    }
    packages.sort_by_key(|path| (std::cmp::Reverse(path.matches('/').count()), *path));
    let declared = contract.is_some_and(|found| approved.contains(&contract_digest(found)));

    for (path, _) in &files {
        if let Some(found) = contract {
            for (domain, owner) in &found.domains {
                if matches(path, &owner.paths) {
                    let key = format!("domain:{domain}");
                    let mut meta = Map::new();
                    meta.insert("name".into(), json!(domain));
                    meta.insert("scope".into(), json!("subsystem"));
                    meta.insert("origin".into(), json!("declared"));
                    meta.insert("anchor".into(), json!(domain));
                    meta.insert("owner".into(), json!(owner.owner));
                    meta.insert("owner_approved".into(), json!(declared));
                    meta.insert("public_patterns".into(), json!(owner.public_interfaces));
                    add_member(&mut groups, &key, path, meta);
                }
            }
        }
        let package = packages.iter().find(|candidate| {
            let parent = posix_parent(candidate);
            **candidate == path || parent == "." || path.starts_with(&format!("{parent}/"))
        });
        if let Some(package) = package {
            let anchor = posix_parent(package);
            let manifest = get(index[package], "facts").and_then(|facts| get(facts, "manifest"))?;
            let mut meta = Map::new();
            meta.insert("name".into(), get(manifest, "name")?.clone());
            meta.insert("scope".into(), json!("package"));
            meta.insert("origin".into(), json!("observed"));
            meta.insert("anchor".into(), json!(anchor));
            meta.insert("manifest".into(), json!(package));
            add_member(&mut groups, &format!("package:{anchor}"), path, meta);
        }
        let parts = path_parts(path);
        if parts.len() > 1 {
            // A directory is a candidate boundary, never a business-domain claim.
            let wide = matches!(parts[0], "src" | "apps" | "packages");
            let depth = (if wide { 2 } else { 1 }).min(parts.len() - 1);
            let anchor = parts[..depth].join("/");
            let mut meta = Map::new();
            meta.insert("name".into(), json!(anchor));
            meta.insert("scope".into(), json!("subsystem"));
            meta.insert("origin".into(), json!("inferred"));
            meta.insert("anchor".into(), json!(anchor));
            add_member(&mut groups, &format!("directory:{anchor}"), path, meta);
        }
    }

    let rank = |key: &str| -> u8 {
        if key == "repository" {
            0
        } else if key.starts_with("domain:") {
            1
        } else if key.starts_with("package:") {
            2
        } else {
            3
        }
    };
    let mut order: Vec<&String> = groups.iter().map(|(key, _)| key).collect();
    order.sort_by(|left, right| (rank(left), left.as_str()).cmp(&(rank(right), right.as_str())));
    let total = groups.len();
    let by_key: HashMap<&str, &Group> = groups
        .iter()
        .map(|(key, group)| (key.as_str(), group))
        .collect();
    let parent_id = digest(&json!([repository, "repository"]))[..24].to_owned();
    let mut results: Vec<Value> = Vec::new();
    for key in order.into_iter().take(SCOPE_LIMIT) {
        let group = by_key[key.as_str()];
        let (members, meta) = (&group.members, &group.meta);
        let mut internal: Vec<(&String, &String)> = Vec::new();
        for source in members {
            for target in adjacency.targets(source) {
                if members.contains(target) {
                    internal.push((source, target));
                }
            }
        }
        let outgoing: BTreeSet<&String> = members
            .iter()
            .flat_map(|source| adjacency.targets(source))
            .filter(|target| !members.contains(*target))
            .collect();
        let mut inbound: BTreeSet<&String> = BTreeSet::new();
        for (path, targets) in &adjacency.edges {
            if !members.contains(path) && targets.iter().any(|target| members.contains(target)) {
                inbound.insert(path);
            }
        }
        let mut public: BTreeSet<String> = BTreeSet::new();
        for source in &inbound {
            for target in adjacency.targets(source) {
                if members.contains(target) {
                    public.insert(target.clone());
                }
            }
        }
        let patterns: Vec<String> = meta
            .get("public_patterns")
            .map(|list| {
                array(list)
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        if !patterns.is_empty() {
            for path in members {
                if matches(path, &patterns) {
                    public.insert(path.clone());
                }
            }
        }
        if key.starts_with("package:") {
            for path in members {
                let facts = get(index[path.as_str()], "facts")?;
                let declared_here = lookup(facts, "structure")
                    .and_then(|structure| lookup(structure, "declarations"))
                    .map(array)
                    .unwrap_or_default();
                if declared_here
                    .iter()
                    .any(|item| lookup(item, "exported").is_some_and(truthy))
                {
                    public.insert(path.clone());
                }
            }
        }
        let mut languages: BTreeMap<String, u64> = BTreeMap::new();
        for path in members {
            let language = lookup(get(index[path.as_str()], "metadata")?, "language")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned();
            *languages.entry(language).or_insert(0) += 1;
        }
        let sorted_members: Vec<&String> = members.iter().collect();
        let mut proofs: Vec<Value> = Vec::new();
        for path in sorted_members.iter().take(3) {
            proofs.push(evidence(graph, index[path.as_str()], &json!(1), VERSION)?);
        }
        if let Some(manifest) = meta
            .get("manifest")
            .and_then(Value::as_str)
            .filter(|found| !found.is_empty())
        {
            let first = evidence(graph, index[manifest], &json!(1), VERSION)?;
            proofs.truncate(2);
            proofs.insert(0, first);
        }
        let origin = text_of(&meta["origin"]);
        if origin == "declared" && index.contains_key(".codegraph/architecture.json") {
            proofs = vec![evidence(
                graph,
                index[".codegraph/architecture.json"],
                &json!(1),
                "architecture-contract",
            )?];
        }
        let mut responsibilities: BTreeMap<&str, i64> = BTreeMap::new();
        for path in members {
            *responsibilities.entry(category_of(path)?).or_insert(0) += 1;
        }
        let mut ranked: Vec<(&str, i64)> = responsibilities.into_iter().collect();
        ranked.sort_by(|left, right| (-left.1, left.0).cmp(&(-right.1, right.0)));
        let candidate = ranked
            .iter()
            .take(3)
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
            .join(" / ");

        let mut entry = Map::new();
        entry.insert("id".into(), json!(digest(&json!([repository, key]))[..24]));
        entry.insert("key".into(), json!(key));
        for (name, value) in meta {
            entry.insert(name.clone(), value.clone());
        }
        entry.insert("repository".into(), repository.clone());
        entry.insert("revision".into(), get(graph, "revision")?.clone());
        entry.insert(
            "parent_id".into(),
            if key == "repository" {
                Value::Null
            } else {
                json!(parent_id)
            },
        );
        entry.insert(
            "member_files".into(),
            json!(sorted_members.iter().take(128).collect::<Vec<_>>()),
        );
        entry.insert("_analysis_members".into(), json!(sorted_members));
        entry.insert("member_count".into(), json!(members.len()));
        entry.insert(
            "public_interfaces".into(),
            json!(public.iter().take(32).collect::<Vec<_>>()),
        );
        entry.insert(
            "external_dependencies".into(),
            json!(outgoing.iter().take(32).collect::<Vec<_>>()),
        );
        entry.insert(
            "internal_dependencies".into(),
            Value::Array(
                internal
                    .iter()
                    .take(32)
                    .map(|(source, target)| json!({"source": source, "target": target}))
                    .collect(),
            ),
        );
        entry.insert(
            "candidate_responsibility".into(),
            json!(format!(
                "{candidate} files; business responsibility requires review"
            )),
        );
        entry.insert(
            "signals".into(),
            json!({
                "incoming_files": inbound.len(),
                "outgoing_files": outgoing.len(),
                "internal_imports": internal.len(),
                "languages": languages,
            }),
        );
        let observed = matches!(origin, "declared" | "observed");
        entry.insert(
            "boundary_confidence".into(),
            json!(if observed { 1.0 } else { 0.35 }),
        );
        entry.insert(
            "confidence_calibration".into(),
            json!(if origin == "inferred" {
                "uncalibrated"
            } else {
                "not-applicable"
            }),
        );
        let approved_owner = meta.get("owner_approved").is_some_and(truthy);
        entry.insert(
            "review_status".into(),
            json!(if approved_owner {
                "approved"
            } else {
                "unreviewed"
            }),
        );
        entry.insert("evidence".into(), Value::Array(proofs));
        entry.insert(
            "partial".into(),
            json!(lookup(graph, "partial").is_some_and(truthy)),
        );
        entry.insert("members_partial".into(), json!(members.len() > 128));
        entry.insert("alternatives".into(), json!([ALTERNATIVE]));
        results.push(Value::Object(entry));
    }
    Ok(json!({
        "version": VERSION,
        "subsystems": results,
        "scope_count": total,
        "partial": total > SCOPE_LIMIT,
        "portfolio_scope": PORTFOLIO_SCOPE,
    }))
}

/// Add a path to a group, creating it on first sight; the latest metadata wins.
fn add_member(groups: &mut Vec<(String, Group)>, key: &str, path: &str, meta: Map<String, Value>) {
    match groups.iter_mut().find(|(name, _)| name == key) {
        Some((_, group)) => {
            group.members.insert(path.to_owned());
            group.meta = meta;
        }
        None => groups.push((
            key.to_owned(),
            Group {
                members: BTreeSet::from([path.to_owned()]),
                meta,
            },
        )),
    }
}

fn subsystems_by_id(value: &Value) -> BTreeMap<String, Value> {
    lookup(value, "subsystems")
        .map(array)
        .unwrap_or_default()
        .iter()
        .map(|item| (text_of(&item["id"]).to_owned(), item.clone()))
        .collect()
}

/// `compare_recognition(before, after)`: scopes that appeared, went, or changed membership or owner.
pub fn compare_recognition(before: &Value, after: &Value) -> Result<Value, PyError> {
    let (old, new) = (subsystems_by_id(before), subsystems_by_id(after));
    let keys: BTreeSet<&String> = old.keys().chain(new.keys()).collect();
    let mut changes: Vec<Value> = Vec::new();
    let names = |value: &Value, key: &str| -> BTreeSet<String> {
        array(lookup(value, key).unwrap_or(&Value::Null))
            .iter()
            .filter_map(|item| item.as_str().map(str::to_owned))
            .collect()
    };
    for key in keys {
        match (old.get(key), new.get(key)) {
            (Some(left), Some(right)) => {
                if get(left, "member_files")? != get(right, "member_files")?
                    || get(left, "member_count")? != get(right, "member_count")?
                    || lookup(left, "owner").unwrap_or(&Value::Null)
                        != lookup(right, "owner").unwrap_or(&Value::Null)
                {
                    let added: Vec<String> = names(right, "member_files")
                        .difference(&names(left, "member_files"))
                        .take(32)
                        .cloned()
                        .collect();
                    let removed: Vec<String> = names(left, "member_files")
                        .difference(&names(right, "member_files"))
                        .take(32)
                        .cloned()
                        .collect();
                    let partial = if truthy(get(left, "partial")?) {
                        get(left, "partial")?.clone()
                    } else {
                        get(right, "partial")?.clone()
                    };
                    changes.push(json!({
                        "id": key,
                        "change": "membership-or-owner",
                        "added_files": added,
                        "removed_files": removed,
                        "previous_owner": lookup(left, "owner").cloned().unwrap_or(Value::Null),
                        "owner": lookup(right, "owner").cloned().unwrap_or(Value::Null),
                        "partial": partial,
                    }));
                }
            }
            (left, right) => {
                let shown = right.or(left).expect("one side exists");
                changes.push(json!({
                    "id": key,
                    "change": if right.is_some() { "added" } else { "removed" },
                    "name": get(shown, "name")?,
                }));
            }
        }
    }
    let flag = |value: &Value| lookup(value, "partial").is_some_and(truthy);
    Ok(json!({
        "subsystem_changes": changes.iter().take(64).collect::<Vec<_>>(),
        "partial": flag(before) || flag(after) || changes.len() > 64,
    }))
}
