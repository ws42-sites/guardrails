//! Pure architecture fingerprints, baseline comparisons and bounded agent guidance:
//! port of `architecture_analysis.py` (see `analysis_check` and `analysis_budget` for the rest).

use super::PyError;
use super::anomalies::peer_anomalies;
use super::contract::{
    Contract, ContractInput, ENGINE_VERSION, MAX_EDGES, MAX_FILES, contract_digest, digest,
    ontology, parse_contract, safe_path,
};
use super::contract_models::{validate_pattern_match, validate_structural_relationship};
use super::extract::artifact_role;
use super::patterns::{VERSION as PATTERN_VERSION, recognize};
use super::recognition::{compare_recognition, discover_subsystems, structural_graph};
use super::rules::{category_of, import_graph};
use super::topology::{compare_topology, topology};
use super::util::{array, get, lookup, py_int, text_of, truthy};
use crate::py_value::PyValue;
use crate::text;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const IMPACT_NOTICE: &str = "Potential blast radius follows resolved static imports in both directions; it does not prove runtime use or test coverage.";
const ACCEPTED_EXCEPTIONS: &str = "Consult the revision-pinned contract and finding suppressions; existing debt does not approve architecture.";
const FINGERPRINT_LIMITATIONS: [&str; 3] = [
    "Python, JavaScript/TypeScript and Rust only; dynamic and unresolved package imports do not establish confirmed reachability.",
    "Rust links follow `use` paths by Cargo file layout; `mod` declarations, macros, `#[path]` attributes and inline paths are not followed.",
    "File names discover document roles, not approval or semantic intent.",
];

const PERSISTENCE: [&str; 8] = [
    "psycopg",
    "pg",
    "sqlalchemy",
    "prisma",
    "@prisma/client",
    "mongoose",
    "redis",
    "sqlite3",
];
const INTEGRATION: [&str; 8] = [
    "httpx",
    "requests",
    "axios",
    "boto3",
    "@google-cloud/pubsub",
    "amqplib",
    "kafkajs",
    "mcp",
];
const FRAMEWORKS: [&str; 7] = [
    "react",
    "next",
    "express",
    "fastify",
    "@nestjs/core",
    "vue",
    "svelte",
];

/// `DEFAULT_CONTRACT`: the policy used when a repository declares none.
pub fn default_contract() -> Value {
    json!({
        "version": 1,
        "kind": "ArchitectureContract",
        "rules": [
            {
                "id": "CG-CYCLE",
                "kind": "cycles",
                "severity": "block",
                "reason": "New or grown resolved import cycle.",
                "remediation": "Move the shared contract to its existing lower-level owner or invert the dependency.",
            },
            {
                "id": "CG-SIZE",
                "kind": "max-module-lines",
                "reason": "Module grew beyond the repository size threshold.",
                "remediation": "Move a coherent responsibility to its canonical owner; preserve public contracts.",
            },
            {
                "id": "CG-GATE",
                "kind": "gate-change",
                "source": [".github/workflows/**", "tools/**", ".codegraph/**"],
                "reason": "A governed gate or policy file changed relative to the trusted baseline.",
                "remediation": "Review the exact gate diff using trusted tooling; accept an explicit, reviewed exception if intentional.",
            },
            {
                "id": "CG-TEST",
                "kind": "test-integrity",
                "reason": "Test-integrity signals increased.",
                "remediation": "Restore executable assertions and the required tests; document legitimate portability skips.",
            },
            {
                "id": "CG-DUPLICATE",
                "kind": "duplicate-code",
                "reason": "Identical production files may duplicate a responsibility.",
                "remediation": "Compare the responsibilities and reuse the existing owner when appropriate.",
            },
            {
                "id": "CG-COUPLING",
                "kind": "coupling",
                "limit": 50,
                "reason": "Resolved import fan-out grew beyond the configured threshold.",
                "remediation": "Inspect the dependency growth and preserve the existing module boundary.",
            },
            {
                "id": "CG-ORPHAN",
                "kind": "orphan-module",
                "severity": "inform",
                "reason": "A module has no resolved static import connections.",
                "remediation": "Check entry-point and dynamic-loading conventions before removing or connecting this module.",
            },
            {
                "id": "CG-GATE-WEAKENING",
                "kind": "gate-weakening",
                "source": [".github/workflows/**", ".gitlab-ci.yml", "package.json", "**/package.json"],
                "reason": "Recognised quality commands were removed or literal CI bypasses increased.",
                "remediation": "Restore the check or review an equivalent replacement using the trusted baseline.",
            },
        ],
    })
}

/// `validate_graph(graph)`: a pinned identity, bounded size, safe unique paths, facts on every file.
pub fn validate_graph(graph: &Value) -> Result<(), PyError> {
    let pinned = |key: &str| {
        lookup(graph, key)
            .and_then(Value::as_str)
            .is_some_and(|text| !text.is_empty())
    };
    if !pinned("repository") || !pinned("revision") {
        return Err(PyError::value(
            "Architecture analysis requires a pinned repository identity",
        ));
    }
    let count = |key: &str| lookup(graph, key).map_or(0, |list| array(list).len());
    if count("files") > MAX_FILES || count("edges") > MAX_EDGES {
        return Err(PyError::value("Architecture graph exceeds analysis limits"));
    }
    let (mut ids, mut paths): (Vec<String>, BTreeSet<String>) = (Vec::new(), BTreeSet::new());
    for file in array(get(graph, "files")?) {
        let path = text_of(get(file, "path")?);
        safe_path(path, false)?;
        let id = text::python_json(get(file, "id")?);
        let facts_ok = lookup(file, "facts").is_some_and(Value::is_object);
        if ids.contains(&id) || paths.contains(path) || !facts_ok {
            return Err(PyError::value("Duplicate or invalid architecture file"));
        }
        ids.push(id);
        paths.insert(path.to_owned());
    }
    Ok(())
}

/// `discovered_contract(graph)`: the policy a repository declares in `.codegraph/architecture.json`.
pub fn discovered_contract(graph: &Value) -> Result<Option<Contract>, PyError> {
    for file in array(get(graph, "files")?) {
        if text_of(get(file, "path")?) == ".codegraph/architecture.json" {
            let declared = lookup(get(file, "facts")?, "contract").filter(|found| truthy(found));
            if let Some(declared) = declared {
                return parse_contract(ContractInput::Value(declared)).map(Some);
            }
        }
    }
    Ok(None)
}

struct Dimensions {
    names: Vec<&'static str>,
    items: BTreeMap<&'static str, Vec<Value>>,
}

impl Dimensions {
    fn new() -> Self {
        let names = [
            "architectural-style",
            "design-patterns",
            "languages-frameworks",
            "dependency-structure",
            "domain-service-boundaries",
            "persistence",
            "integration",
            "deployment",
            "ci-testing",
            "governance",
        ];
        Self {
            names: names.to_vec(),
            items: names.iter().map(|name| (*name, Vec::new())).collect(),
        }
    }

    fn len(&self, name: &str) -> usize {
        self.items.get(name).map_or(0, Vec::len)
    }

    fn into_value(self) -> Value {
        let mut map = Map::new();
        for name in self.names {
            map.insert(name.to_owned(), Value::Array(self.items[name].clone()));
        }
        Value::Object(map)
    }
}

/// `fingerprint(graph, approved, contract)`: what the repository is built like, with evidence.
pub fn fingerprint(
    graph: &Value,
    approved: &BTreeSet<String>,
    contract: Option<&Contract>,
) -> Result<Value, PyError> {
    validate_graph(graph)?;
    let mut dimensions = Dimensions::new();
    let files = array(get(graph, "files")?);
    let adjacency = import_graph(graph, false)?;
    let mut artifacts: Vec<Value> = Vec::new();
    let mut seen_languages: BTreeSet<String> = BTreeSet::new();
    // Python rebinds the `contract` parameter when a file declares one; so does this.
    let mut contract: Option<Contract> = contract.cloned();

    let classify = |dimensions: &mut Dimensions,
                    dimension: &'static str,
                    label: &str,
                    file: &Value,
                    origin: &str,
                    reviewed: bool|
     -> Result<(), PyError> {
        let review = if reviewed {
            "approved"
        } else if origin == "declared" {
            "unreviewed"
        } else {
            "not-required"
        };
        let entry = json!({
            "dimension": dimension,
            "label": label,
            "origin": origin,
            "confidence": 1.0,
            "model_version": null,
            "review_status": review,
            "evidence": [{
                "repository": get(graph, "repository")?,
                "revision": get(graph, "revision")?,
                "path": get(file, "path")?,
                "line": 1,
                "method": if origin == "declared" { "architecture-contract" } else { "static-structure" },
                "sha256": lookup(get(file, "facts")?, "sha256").cloned().unwrap_or(Value::Null),
            }],
        });
        dimensions
            .items
            .get_mut(dimension)
            .expect("known dimension")
            .push(entry);
        Ok(())
    };

    for file in files {
        let path = text_of(get(file, "path")?);
        let facts = get(file, "facts")?;
        let language = lookup(get(file, "metadata")?, "language").filter(|found| truthy(found));
        if let Some(language) = language {
            let name = text_of(language);
            if name != "text" && !seen_languages.contains(name) {
                classify(
                    &mut dimensions,
                    "languages-frameworks",
                    name,
                    file,
                    "observed",
                    false,
                )?;
                seen_languages.insert(name.to_owned());
            }
        }
        if let Some(role) = artifact_role(path) {
            artifacts.push(json!({
                "path": path,
                "role": role,
                "node_id": get(file, "id")?,
                "sha256": lookup(facts, "sha256").cloned().unwrap_or(Value::Null),
            }));
            let dimension = match role {
                "deployment-configuration" => "deployment",
                "ci-configuration" => "ci-testing",
                _ => "governance",
            };
            if dimensions.len(dimension) < 32 {
                classify(&mut dimensions, dimension, role, file, "observed", false)?;
            }
        }
        let dependencies = lookup(facts, "manifest")
            .and_then(|manifest| lookup(manifest, "dependencies"))
            .map(array)
            .unwrap_or_default();
        for dependency in dependencies {
            let name = text_of(dependency);
            if PERSISTENCE.contains(&name) {
                classify(
                    &mut dimensions,
                    "persistence",
                    &format!("{name} dependency (configuration evidence)"),
                    file,
                    "observed",
                    false,
                )?;
            }
            if INTEGRATION.contains(&name) {
                classify(
                    &mut dimensions,
                    "integration",
                    &format!("{name} dependency (configuration evidence)"),
                    file,
                    "observed",
                    false,
                )?;
            }
            if FRAMEWORKS.contains(&name) {
                classify(
                    &mut dimensions,
                    "languages-frameworks",
                    name,
                    file,
                    "observed",
                    false,
                )?;
            }
        }
        if !adjacency.targets(path).is_empty() && dimensions.len("dependency-structure") == 0 {
            classify(
                &mut dimensions,
                "dependency-structure",
                "Resolved static imports; see bounded impact for connected files",
                file,
                "observed",
                false,
            )?;
        }
        if category_of(path)? == "test" && dimensions.len("ci-testing") == 0 {
            classify(
                &mut dimensions,
                "ci-testing",
                "Test files discovered by source convention",
                file,
                "observed",
                false,
            )?;
        }
        if path == ".codegraph/architecture.json"
            && let Some(declared) = lookup(facts, "contract").filter(|found| truthy(found))
        {
            let parsed = parse_contract(ContractInput::Value(declared))?;
            let reviewed = approved.contains(&contract_digest(&parsed));
            for style in &parsed.styles {
                classify(
                    &mut dimensions,
                    "architectural-style",
                    style,
                    file,
                    "declared",
                    reviewed,
                )?;
            }
            for (domain, owner) in &parsed.domains {
                classify(
                    &mut dimensions,
                    "domain-service-boundaries",
                    &format!("{domain}: {}", owner.owner),
                    file,
                    "declared",
                    reviewed,
                )?;
            }
            contract = Some(parsed);
        }
    }

    let mut errors: Vec<String> = Vec::new();
    for file in files {
        let facts = get(file, "facts")?;
        let path = text_of(get(file, "path")?);
        let flagged = lookup(facts, "parse_error").is_some_and(truthy)
            || lookup(facts, "invalid_contract").is_some_and(truthy)
            || lookup(facts, "ci")
                .and_then(|ci| lookup(ci, "partial"))
                .is_some_and(truthy)
            || (path == ".codegraph/architecture.json"
                && !lookup(facts, "contract").is_some_and(truthy))
            || !truthy(facts);
        if flagged {
            errors.push(path.to_owned());
        }
    }
    let partial = lookup(graph, "partial").is_some_and(truthy) || !errors.is_empty();
    let intent = match &contract {
        Some(found) => Some(found.clone()),
        None => discovered_contract(graph)?,
    };
    let subsystems = discover_subsystems(graph, intent.as_ref(), approved)?;
    let structure = structural_graph(graph)?;
    let deployment = topology(graph)?;
    let scopes = array(get(&subsystems, "subsystems")?);
    let pattern_matches = recognize(graph, scopes, &structure, Some(&deployment))?;
    for found in &pattern_matches {
        if text_of(get(found, "outcome")?) == "supported" {
            let evidence: Vec<&Value> = array(get(found, "supporting")?)
                .iter()
                .flat_map(|signal| array(&signal["evidence"]).iter())
                .take(8)
                .collect();
            dimensions
                .items
                .get_mut("design-patterns")
                .expect("known")
                .push(json!({
                    "dimension": "design-patterns",
                    "label": get(found, "pattern_id")?,
                    "origin": "inferred",
                    "confidence": 0.6,
                    "confidence_calibration": "uncalibrated",
                    "model_version": PATTERN_VERSION,
                    "review_status": "unreviewed",
                    "evidence": evidence,
                    "scope_id": get(found, "scope_id")?,
                }));
        }
    }
    let mut validated_matches: Vec<Value> = Vec::new();
    for found in &pattern_matches {
        validated_matches.push(validate_pattern_match(&PyValue::from_value(found))?);
    }
    let mut relationships: Vec<Value> = Vec::new();
    for relation in array(get(&structure, "relations")?).iter().take(128) {
        relationships.push(validate_structural_relationship(&PyValue::from_value(
            relation,
        ))?);
    }
    let public_scopes: Vec<Value> = scopes
        .iter()
        .map(|scope| {
            let mut kept = scope.as_object().cloned().unwrap_or_default();
            kept.retain(|key, _| !key.starts_with('_'));
            Value::Object(kept)
        })
        .collect();
    let anomalies = peer_anomalies(graph, Some(&structure))?;
    let recognition_partial = truthy(get(&structure, "partial")?)
        || truthy(get(&subsystems, "partial")?)
        || truthy(get(&deployment, "partial")?);
    let structural_partial =
        truthy(get(&structure, "partial")?) || array(get(&structure, "relations")?).len() > 128;
    let mut coverage = get(&structure, "coverage")?
        .as_object()
        .cloned()
        .unwrap_or_default();
    coverage.insert("partial".into(), json!(structural_partial));
    let ontology_version = get(&ontology(None)?, "version")?.clone();
    let resolved_imports: usize = adjacency.edges.values().map(BTreeSet::len).sum();
    Ok(json!({
        "schema_version": "1",
        "engine_version": ENGINE_VERSION,
        "repository": get(graph, "repository")?,
        "revision": get(graph, "revision")?,
        "ontology_version": ontology_version,
        "pattern_library_version": PATTERN_VERSION,
        "classifier_version": get(&subsystems, "version")?,
        "recognition_partial": recognition_partial,
        "pattern_matches": validated_matches,
        "deployment_topology": deployment,
        "anomaly_analysis": anomalies,
        "policy_digest": intent.as_ref().map(contract_digest),
        "subsystems": public_scopes,
        "subsystems_partial": get(&subsystems, "partial")?,
        "subsystem_count": get(&subsystems, "scope_count")?,
        "structural_relationships": relationships,
        "structural_coverage": coverage,
        "dimensions": dimensions.into_value(),
        "metrics": {
            "files": files.len(),
            "resolved_imports": resolved_imports,
            "languages": seen_languages,
        },
        "artifacts": artifacts.iter().take(256).collect::<Vec<_>>(),
        "partial": partial,
        "coverage": {
            "files_seen": files.len(),
            "artifact_count": artifacts.len(),
            "parse_errors": errors.iter().take(100).collect::<Vec<_>>(),
            "parse_error_count": errors.len(),
            "limitations": FINGERPRINT_LIMITATIONS,
        },
        "fingerprint_id": digest(&json!([get(graph, "repository")?, get(graph, "revision")?, ENGINE_VERSION])),
    }))
}

fn missing_topology() -> Value {
    json!({"entities": [], "partial": true})
}

/// `compare_fingerprints(before, after)`: how patterns, scopes, anomalies and deployment moved.
pub fn compare_fingerprints(before: &Value, after: &Value) -> Result<Value, PyError> {
    if get(before, "repository")? != get(after, "repository")? {
        return Err(PyError::new(
            "PermissionError",
            "Fingerprint comparison belongs to another repository",
        ));
    }
    let patterns = |value: &Value| -> Result<BTreeSet<(String, String, String)>, PyError> {
        let mut found = BTreeSet::new();
        for item in lookup(value, "pattern_matches")
            .map(array)
            .unwrap_or_default()
        {
            found.insert((
                text_of(get(item, "scope_id")?).to_owned(),
                text_of(get(item, "pattern_id")?).to_owned(),
                text_of(get(item, "outcome")?).to_owned(),
            ));
        }
        Ok(found)
    };
    let (old_patterns, new_patterns) = (patterns(before)?, patterns(after)?);
    let anomalies = |value: &Value| -> Vec<Value> {
        lookup(value, "anomaly_analysis")
            .and_then(|found| lookup(found, "anomalies"))
            .map(|list| array(list).to_vec())
            .unwrap_or_default()
    };
    let old_anomalies: BTreeMap<String, Value> = anomalies(before)
        .into_iter()
        .map(|item| (text_of(&item["id"]).to_owned(), item))
        .collect();
    let mut anomaly_changes: Vec<Value> = Vec::new();
    for item in anomalies(after) {
        let id = text_of(get(&item, "id")?).to_owned();
        let previous = old_anomalies.get(&id);
        let status = match previous {
            None => "new",
            Some(old) => {
                let (now, then) = (get(&item, "value")?.as_f64(), get(old, "value")?.as_f64());
                if now > then { "grew" } else { "existing" }
            }
        };
        anomaly_changes.push(json!({
            "id": id,
            "path": get(&item, "path")?,
            "metric": get(&item, "metric")?,
            "value": get(&item, "value")?,
            "previous_value": previous.and_then(|old| lookup(old, "value")).cloned().unwrap_or(Value::Null),
            "status": status,
        }));
    }
    let recognition = compare_recognition(before, after)?;
    let missing = missing_topology();
    let deployment = compare_topology(
        lookup(before, "deployment_topology").unwrap_or(&missing),
        lookup(after, "deployment_topology").unwrap_or(&missing),
    )?;
    let mut pattern_changes: Vec<Value> = Vec::new();
    for (change, items) in [
        ("added", new_patterns.difference(&old_patterns)),
        ("removed", old_patterns.difference(&new_patterns)),
    ] {
        for (scope, pattern, outcome) in items {
            pattern_changes.push(json!({
                "change": change,
                "scope_id": scope,
                "pattern_id": pattern,
                "outcome": outcome,
            }));
        }
    }
    let changed = new_patterns.symmetric_difference(&old_patterns).count();
    let flag = |value: &Value, key: &str| lookup(value, key).is_some_and(truthy);
    let mut out = Map::new();
    out.insert("base_revision".into(), get(before, "revision")?.clone());
    out.insert("candidate_revision".into(), get(after, "revision")?.clone());
    out.insert("base_metrics".into(), get(before, "metrics")?.clone());
    out.insert("candidate_metrics".into(), get(after, "metrics")?.clone());
    for source in [&recognition, &deployment] {
        for (key, value) in source.as_object().cloned().unwrap_or_default() {
            out.insert(key, value);
        }
    }
    out.insert(
        "pattern_changes".into(),
        json!(pattern_changes.iter().take(128).collect::<Vec<_>>()),
    );
    out.insert(
        "anomaly_changes".into(),
        json!(anomaly_changes.iter().take(128).collect::<Vec<_>>()),
    );
    out.insert("accepted_exceptions".into(), json!(ACCEPTED_EXCEPTIONS));
    out.insert(
        "partial".into(),
        json!(
            flag(before, "partial")
                || flag(after, "partial")
                || flag(before, "subsystems_partial")
                || flag(after, "subsystems_partial")
                || changed > 128
        ),
    );
    Ok(Value::Object(out))
}

/// `impact(graph, paths, limit, depth)`: the files an edit could reach through resolved imports.
pub fn impact(
    graph: &Value,
    paths: &[String],
    limit: usize,
    depth: usize,
) -> Result<Value, PyError> {
    validate_graph(graph)?;
    if !(1..=200).contains(&limit) || depth > 4 || !(1..=50).contains(&paths.len()) {
        return Err(PyError::value("Invalid architecture impact bounds"));
    }
    let mut known: BTreeMap<String, &Value> = BTreeMap::new();
    for file in array(get(graph, "files")?) {
        known.insert(text_of(get(file, "path")?).to_owned(), file);
    }
    for path in paths {
        safe_path(path, false)?;
        if !known.contains_key(path) {
            return Err(PyError::value("Impact path is not in the pinned snapshot"));
        }
    }
    let adjacency = import_graph(graph, false)?;
    let mut reverse: BTreeMap<&str, BTreeSet<&str>> = known
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
    let mut seen: BTreeSet<String> = paths.iter().cloned().collect();
    let mut sorted_paths: Vec<&String> = paths.iter().collect();
    sorted_paths.sort();
    let mut queue: VecDeque<(String, usize)> = sorted_paths
        .into_iter()
        .map(|path| (path.clone(), 0))
        .collect();
    let mut connections: Vec<Value> = Vec::new();
    while !queue.is_empty() && seen.len() <= limit {
        let (current, distance) = queue.pop_front().expect("not empty");
        if distance == depth {
            continue;
        }
        let outgoing = adjacency.targets(&current);
        let mut neighbours: BTreeSet<&str> = outgoing.iter().map(String::as_str).collect();
        neighbours.extend(reverse[current.as_str()].iter().copied());
        for other in neighbours {
            let (source, target) = if outgoing.contains(other) {
                (current.as_str(), other)
            } else {
                (other, current.as_str())
            };
            let proof = adjacency
                .proofs
                .get(&(source.to_owned(), target.to_owned()))
                .ok_or_else(|| PyError::new("KeyError", format!("('{source}', '{target}')")))?;
            let mut evidence = proof.as_object().cloned().unwrap_or_default();
            evidence.insert("repository".into(), get(graph, "repository")?.clone());
            evidence.insert("revision".into(), get(graph, "revision")?.clone());
            connections.push(json!({
                "source": get(known[source], "id")?,
                "target": get(known[target], "id")?,
                "kind": "IMPORTS",
                "evidence": evidence,
            }));
            if !seen.contains(other) {
                seen.insert(other.to_owned());
                queue.push_back((other.to_owned(), distance + 1));
            }
            if seen.len() >= limit {
                break;
            }
        }
    }
    let included: Vec<&String> = seen.iter().take(limit).collect();
    let mut nodes: Vec<Value> = Vec::new();
    let mut tests: Vec<&String> = Vec::new();
    for path in &included {
        let category = category_of(path)?;
        nodes.push(
            json!({"id": get(known[path.as_str()], "id")?, "path": path, "category": category}),
        );
        if category == "test" {
            tests.push(path);
        }
    }
    Ok(json!({
        "schema_version": "1",
        "repository": get(graph, "repository")?,
        "revision": get(graph, "revision")?,
        "paths": included,
        "nodes": nodes,
        "relationships": connections.iter().take(500).collect::<Vec<_>>(),
        "related_tests": tests,
        "partial": !queue.is_empty() || lookup(graph, "partial").is_some_and(truthy),
        "notice": IMPACT_NOTICE,
    }))
}

/// `select_findings(report, arguments)`: the report with its findings filtered.
pub fn select_findings(report: &Value, arguments: &Value) -> Result<Value, PyError> {
    let wanted = |key: &str| lookup(arguments, key).filter(|found| truthy(found));
    let mut selected: Vec<Value> = Vec::new();
    for finding in array(get(report, "findings")?) {
        if let Some(severity) = wanted("severity")
            && get(finding, "severity")? != severity
        {
            continue;
        }
        if let Some(prefix) = wanted("path") {
            let prefix = text_of(prefix);
            if !array(get(finding, "paths")?)
                .iter()
                .any(|path| text_of(path).starts_with(prefix))
            {
                continue;
            }
        }
        if let Some(status) = wanted("status") {
            let matches_status = if status == "suppressed" {
                get(get(finding, "suppression")?, "status")? == "active"
            } else {
                get(get(finding, "baseline")?, "status")? == status
            };
            if !matches_status {
                continue;
            }
        }
        selected.push(finding.clone());
    }
    let mut out = report.as_object().cloned().unwrap_or_default();
    out.insert("findings".into(), Value::Array(selected));
    Ok(Value::Object(out))
}

/// Used by callers that hold line numbers as decoded values.
pub fn line_number(value: &Value) -> Result<i64, PyError> {
    py_int(value)
}
