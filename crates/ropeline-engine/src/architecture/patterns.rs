//! Curated executable signatures: port of `architecture_patterns.py`.
//! Matches are advisory structural hypotheses.

use super::PyError;
use super::evidence::evidence;
use super::rules::{ImportGraph, import_graph};
use super::util::{array, get, lookup, text_of, truthy};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub const VERSION: &str = "patterns-2";

const MISSING_COMPLETE: &str = "complete-scope-evidence";
const EXPLANATION_SUPPORTED: &str = "Structural candidate; review behaviour and declared intent.";
const EXPLANATION_CONTRADICTED: &str = "Contradictory static dependencies require review.";
const EXPLANATION_ABSTAINED: &str = "Insufficient structural evidence; abstained.";
const LIMITATION: &str = "Static signatures do not prove runtime behaviour, security guarantees or semantic equivalence.";

/// `(name, recognition criteria, negative example, legitimate exception)`, in catalogue order.
const DEFINITIONS: [(&str, &str, &str, &str); 19] = [
    (
        "layered",
        "Dependencies follow declared layer direction.",
        "A domain module imports a presentation adapter.",
        "A composition root may depend on every layer.",
    ),
    (
        "clean-architecture",
        "Business rules depend on contracts rather than delivery details.",
        "A use case directly creates an HTTP framework response.",
        "Small applications need not use the canonical folder names.",
    ),
    (
        "hexagonal",
        "Core-owned ports separate application logic from host adapters.",
        "Core imports its database adapter implementation.",
        "Composition wiring may import both port and adapter.",
    ),
    (
        "modular-monolith",
        "One deployment contains modules with explicit public contracts.",
        "A module bypasses another module's private store.",
        "One process alone does not prove modularity.",
    ),
    (
        "microservices",
        "Independent deployments communicate through versioned contracts.",
        "A service imports another service's private persistence code.",
        "Several service-named classes may still be one deployment.",
    ),
    (
        "event-driven",
        "Producers and consumers share explicit event contracts.",
        "A consumer changes ownership rules based on unvalidated event payload.",
        "In-process events need not require a broker.",
    ),
    (
        "mvc",
        "Request controllers coordinate models and presentation views.",
        "Business policy is duplicated in controllers and views.",
        "A headless API may deliberately omit views.",
    ),
    (
        "ports-and-adapters",
        "Ports describe core needs; adapters implement host effects.",
        "An adapter defines a competing domain contract.",
        "An adapter may legitimately implement several small ports.",
    ),
    (
        "adapter",
        "A concrete implementation translates an external dependency behind a port.",
        "A similarly named class does not implement a port.",
        "A pure adapter may have no external dependency.",
    ),
    (
        "repository",
        "Persistence access sits behind an intended domain contract.",
        "A second store silently owns the same records.",
        "Direct database reads may be explicitly approved for reporting.",
    ),
    (
        "factory",
        "Construction choices have a demonstrated common contract.",
        "A wrapper adds indirection without construction variation.",
        "A single implementation may isolate an important host effect.",
    ),
    (
        "strategy",
        "Interchangeable algorithms satisfy the same behavioural contract.",
        "Each strategy changes the caller's authority semantics.",
        "Some strategies require explicitly different capabilities.",
    ),
    (
        "dependency-injection",
        "The composition owner supplies concrete dependencies.",
        "Core locates arbitrary implementations from global mutable state.",
        "Constructor parameters are valid injection without a framework.",
    ),
    (
        "cqrs",
        "Write decisions and read projections have explicit owners.",
        "A read projection bypasses command-side authority to mutate data.",
        "CQRS does not require separate databases.",
    ),
    (
        "domain-boundaries",
        "Each domain has a declared responsibility and public boundary.",
        "Two modules independently decide the same business invariant.",
        "Cross-domain workflows may have an explicit coordinator.",
    ),
    (
        "transactional-outbox",
        "Business state and outgoing event intent commit atomically.",
        "A database commit succeeds while the external publish is silently lost.",
        "An in-memory outbox cannot establish durable delivery.",
    ),
    (
        "idempotency",
        "An operation key and durable outcome prevent duplicate effects.",
        "A retry creates a second externally visible result.",
        "Pure reads may not need a durable deduplication record.",
    ),
    (
        "least-privilege",
        "Capabilities and data scope are checked at the authority boundary.",
        "A read token can invoke a repository mutation.",
        "An operator migration may deliberately have broader scoped privileges.",
    ),
    (
        "dependency-inversion",
        "High-level policy owns the abstraction consumed by low-level details.",
        "A policy module depends on a concrete transport or persistence adapter.",
        "A composition root chooses concrete implementations.",
    ),
];

/// The required signals of each pattern, in the order `match_signatures` reports them.
const SIGNATURES: [(&str, &[&str]); 19] = [
    (
        "layered",
        &[
            "declared-layer-intent",
            "dependency-chain",
            "no-reverse-imports",
        ],
    ),
    (
        "clean-architecture",
        &["port-loop", "no-core-to-concrete", "composition"],
    ),
    (
        "hexagonal",
        &["port-loop", "no-core-to-concrete", "composition"],
    ),
    ("ports-and-adapters", &["port-loop", "no-core-to-concrete"]),
    (
        "modular-monolith",
        &["multiple-public-packages", "single-configured-application"],
    ),
    (
        "microservices",
        &[
            "independent-deployment-targets",
            "versioned-api-contract",
            "api-route",
        ],
    ),
    ("event-driven", &["event-producer", "event-consumer"]),
    ("mvc", &["api-route", "view-render", "database-read"]),
    ("cqrs", &["separate-read-write", "public-interface"]),
    (
        "domain-boundaries",
        &["declared-domains", "public-interface"],
    ),
    ("repository", &["port-loop", "persistence-call"]),
    (
        "factory",
        &["alternative-construction", "behavioural-alternatives"],
    ),
    ("strategy", &["behavioural-alternatives", "port-injection"]),
    ("dependency-injection", &["port-injection", "composition"]),
    ("adapter", &["implementation", "external-import"]),
    (
        "transactional-outbox",
        &["atomic-outbox-candidate", "persistence-call"],
    ),
    (
        "idempotency",
        &["durable-unique-key", "conflict-guard", "persistence-call"],
    ),
    (
        "dependency-inversion",
        &["port-loop", "no-core-to-concrete"],
    ),
    ("least-privilege", &["scoped-read-grant", "explicit-revoke"]),
];

fn signature(name: &str) -> &'static [&'static str] {
    SIGNATURES
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .map_or(&[], |(_, required)| *required)
}

/// `catalogue()`: the pattern library as data.
pub fn catalogue() -> Value {
    let patterns: Vec<Value> = DEFINITIONS
        .iter()
        .map(|(name, intent, negative, exception)| {
            json!({
                "id": name,
                "recognition_criteria": intent,
                "required_structural_signals": signature(name),
                "version": VERSION,
                "scope": ["repository", "subsystem", "package"],
                "contradictory_signals": ["core-to-concrete"],
                "missing_evidence": "inconclusive",
                "enforcement": "advisory",
                "examples": ["See repository-family classification fixtures."],
                "detection_limitations": [
                    "Static structure cannot prove behavioural semantics or runtime deployment."
                ],
                "optional_structural_signals": [
                    "Declared architecture contract",
                    "Source-backed implementation and tests"
                ],
                "negative_examples": [negative],
                "legitimate_exceptions": [exception],
                "languages": ["python", "javascript", "typescript", "sql-configuration"],
                "frameworks": ["framework-neutral"],
                "evidence_requirements": [
                    "Pinned repository revision and source locations",
                    "Structural signals alone are insufficient for approval"
                ],
                "potential_violations": [negative],
                "remediation": "Compare the repository's declared intent and nearby canonical implementation before suggesting a change.",
            })
        })
        .collect();
    json!({"version": VERSION, "patterns": patterns})
}

/// Signals found so far: each key keeps at most six pieces of evidence.
#[derive(Default)]
struct Found {
    order: Vec<String>,
    signals: BTreeMap<String, Vec<Value>>,
}

impl Found {
    /// `add(key, proofs)`: nothing for no proofs; otherwise up to three more, six in all.
    fn add(&mut self, key: &str, proofs: &[Value]) {
        if proofs.is_empty() {
            return;
        }
        if !self.signals.contains_key(key) {
            self.order.push(key.to_owned());
        }
        let list = self.signals.entry(key.to_owned()).or_default();
        list.extend(proofs.iter().take(3).cloned());
        list.truncate(6);
    }

    fn get(&self, key: &str) -> &[Value] {
        self.signals.get(key).map(Vec::as_slice).unwrap_or_default()
    }

    fn into_value(self) -> Value {
        let mut map = Map::new();
        for key in self.order {
            map.insert(key.clone(), Value::Array(self.signals[&key].clone()));
        }
        Value::Object(map)
    }
}

/// An interface declaration or an implementing relation, keyed by `(path, name)`.
type Target = (Option<String>, String);

/// `structural_signals(graph, scope, structure, topology, adjacency)`.
pub fn structural_signals(
    graph: &Value,
    scope: &Value,
    structure: &Value,
    topology: Option<&Value>,
    adjacency: Option<&ImportGraph>,
) -> Result<Value, PyError> {
    let listed =
        lookup(scope, "_analysis_members").map_or_else(|| get(scope, "member_files"), Ok)?;
    let members: BTreeSet<String> = array(listed)
        .iter()
        .map(|item| text_of(item).to_owned())
        .collect();
    let mut files: Vec<(String, &Value)> = Vec::new();
    for file in array(get(graph, "files")?) {
        let path = text_of(get(file, "path")?).to_owned();
        if members.contains(&path) {
            match files.iter_mut().find(|(existing, _)| *existing == path) {
                Some(slot) => slot.1 = file,
                None => files.push((path, file)),
            }
        }
    }
    let owned;
    let adjacency = match adjacency {
        Some(given) => given,
        None => {
            owned = import_graph(graph, false)?;
            &owned
        }
    };
    let file_of = |path: &str| -> Result<&Value, PyError> {
        files
            .iter()
            .find(|(existing, _)| existing == path)
            .map(|(_, file)| *file)
            .ok_or_else(|| PyError::new("KeyError", crate::py_value::str_repr(path)))
    };
    let mut found = Found::default();
    let line_one = json!(1);
    let proof = |file: &Value, line: &Value, method: &str| evidence(graph, file, line, method);

    let mut interfaces: Vec<(Target, Value)> = Vec::new();
    for (path, file) in &files {
        let declarations = lookup(get(file, "facts")?, "structure")
            .and_then(|found| lookup(found, "declarations"))
            .map(array)
            .unwrap_or_default();
        for declaration in declarations {
            if text_of(get(declaration, "kind")?) == "interface" {
                let key = (
                    Some(path.clone()),
                    text_of(get(declaration, "name")?).to_owned(),
                );
                match interfaces.iter_mut().find(|(existing, _)| *existing == key) {
                    Some(slot) => slot.1 = declaration.clone(),
                    None => interfaces.push((key, declaration.clone())),
                }
            }
        }
    }
    let is_interface = |key: &Target| interfaces.iter().any(|(existing, _)| existing == key);
    let mut impls: Vec<(Target, Vec<&Value>)> = Vec::new();
    let mut injections: Vec<(Target, Vec<&Value>)> = Vec::new();
    for relation in array(get(structure, "relations")?) {
        if !members.contains(text_of(get(relation, "source_path")?)) {
            continue;
        }
        let target: Target = (
            lookup(relation, "target_path")
                .and_then(Value::as_str)
                .map(str::to_owned),
            text_of(get(relation, "target")?).to_owned(),
        );
        let kind = text_of(get(relation, "kind")?);
        let proof_of = get(relation, "evidence")?;
        if kind == "IMPLEMENTS" && is_interface(&target) {
            push_group(&mut impls, &target, relation);
            found.add("implementation", std::slice::from_ref(proof_of));
        }
        if kind == "INJECTS" && is_interface(&target) {
            push_group(&mut injections, &target, relation);
            found.add("port-injection", std::slice::from_ref(proof_of));
        }
        if kind == "CONSTRUCTS" {
            found.add("composition", std::slice::from_ref(proof_of));
        }
    }
    // Python walks a set of keys; the walk here is in the order the loop found them.
    for (target, implementing) in &impls {
        let Some((_, injecting)) = injections.iter().find(|(key, _)| key == target) else {
            continue;
        };
        let proofs: Vec<Value> = implementing
            .iter()
            .chain(injecting.iter())
            .map(|relation| relation["evidence"].clone())
            .collect();
        found.add("port-loop", &proofs);
        let contradictions: Vec<Value> = injecting
            .iter()
            .filter(|injected| {
                let source = text_of(&injected["source_path"]);
                implementing.iter().any(|implemented| {
                    let other = text_of(&implemented["source_path"]);
                    other != source && adjacency.targets(source).contains(other)
                })
            })
            .map(|injected| injected["evidence"].clone())
            .collect();
        if contradictions.is_empty() {
            found.add("no-core-to-concrete", &proofs);
        } else {
            found.add("core-to-concrete", &contradictions);
        }
    }
    for (_, relations) in &impls {
        let distinct: BTreeSet<(&str, &str)> = relations
            .iter()
            .map(|relation| {
                (
                    text_of(&relation["source_path"]),
                    text_of(&relation["source"]),
                )
            })
            .collect();
        if distinct.len() >= 2 {
            let proofs: Vec<Value> = relations
                .iter()
                .map(|relation| relation["evidence"].clone())
                .collect();
            found.add("behavioural-alternatives", &proofs);
        }
    }
    let (mut reads, mut writes): (BTreeSet<String>, BTreeSet<String>) =
        (BTreeSet::new(), BTreeSet::new());
    for effect in array(get(structure, "effects")?) {
        let path = text_of(get(effect, "path")?);
        if members.contains(path) {
            let kind = text_of(get(effect, "kind")?);
            found.add(kind, std::slice::from_ref(get(effect, "evidence")?));
            if kind == "database-read" {
                reads.insert(path.to_owned());
            }
            if kind == "database-write" {
                writes.insert(path.to_owned());
            }
        }
    }
    if reads.difference(&writes).next().is_some() && writes.difference(&reads).next().is_some() {
        let mut proofs = Vec::new();
        for path in reads.union(&writes).take(3) {
            proofs.push(proof(
                file_of(path)?,
                &line_one,
                super::recognition::VERSION,
            )?);
        }
        found.add("separate-read-write", &proofs);
    }
    for (path, file) in &files {
        let facts = get(file, "facts")?;
        let contract = lookup(facts, "contract")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if path == ".codegraph/architecture.json" {
            let layered = array(lookup(&contract, "styles").unwrap_or(&Value::Null))
                .iter()
                .any(|style| matches!(style.as_str(), Some("layered" | "layered-architecture")));
            let forbids = array(lookup(&contract, "rules").unwrap_or(&Value::Null))
                .iter()
                .any(|rule| text_of(&rule["kind"]) == "forbidden-dependency");
            if layered && forbids {
                found.add(
                    "declared-layer-intent",
                    &[proof(file, &line_one, "architecture-contract")?],
                );
            }
        }
        if path.ends_with(".sql") {
            let grants = lookup(facts, "configured_grants")
                .map(array)
                .unwrap_or_default();
            let revoked: BTreeSet<&str> = grants
                .iter()
                .filter(|grant| text_of(&grant["kind"]) == "explicit-revoke")
                .map(|grant| text_of(&grant["principal_digest"]))
                .collect();
            for grant in grants {
                let kind = text_of(get(grant, "kind")?);
                if kind == "broad-grant"
                    || revoked.contains(text_of(get(grant, "principal_digest")?))
                {
                    found.add(
                        kind,
                        &[proof(file, get(grant, "line")?, "configured-sql-grant")?],
                    );
                }
            }
        }
        let declarations = lookup(facts, "structure")
            .and_then(|found| lookup(found, "declarations"))
            .map(array)
            .unwrap_or_default();
        for declaration in declarations {
            if lookup(declaration, "exported").is_some_and(truthy)
                && text_of(get(declaration, "kind")?) == "interface"
            {
                found.add(
                    "public-interface",
                    &[proof(
                        file,
                        get(declaration, "line")?,
                        super::recognition::VERSION,
                    )?],
                );
            }
            let constructed =
                lookup(declaration, "returns_constructed").map_or(0, |list| array(list).len());
            if constructed >= 2 {
                found.add(
                    "alternative-construction",
                    &[proof(
                        file,
                        get(declaration, "line")?,
                        super::recognition::VERSION,
                    )?],
                );
            }
        }
        let imports = lookup(facts, "imports").map(array).unwrap_or_default();
        let external = imports.iter().any(|item| {
            let specifier = lookup(item, "specifier")
                .and_then(Value::as_str)
                .unwrap_or("");
            specifier.starts_with('@')
                || specifier.starts_with("node:")
                || matches!(
                    lookup(item, "module").and_then(Value::as_str),
                    Some("sqlite3" | "psycopg" | "sqlalchemy" | "requests" | "httpx")
                )
        });
        if external {
            found.add(
                "external-import",
                &[proof(file, &line_one, super::recognition::VERSION)?],
            );
        }
        for dependency_path in adjacency.targets(path) {
            if !members.contains(dependency_path) {
                continue;
            }
            let chained = adjacency
                .targets(dependency_path)
                .iter()
                .any(|next| members.contains(next) && next != path);
            if chained {
                found.add(
                    "dependency-chain",
                    &[
                        proof(file, &line_one, super::recognition::VERSION)?,
                        proof(
                            file_of(dependency_path)?,
                            &line_one,
                            super::recognition::VERSION,
                        )?,
                    ],
                );
            }
        }
        if lookup(facts, "manifest").is_some_and(truthy) {
            found.add(
                "package",
                &[proof(file, &line_one, super::recognition::VERSION)?],
            );
        }
    }
    let mut pairs: BTreeSet<(&String, &String)> = BTreeSet::new();
    for source in &members {
        for target in adjacency.targets(source) {
            if members.contains(target) {
                pairs.insert((source, target));
            }
        }
    }
    if !pairs.is_empty()
        && !pairs
            .iter()
            .any(|(source, target)| pairs.contains(&(*target, *source)))
    {
        let mut proofs = Vec::new();
        for (source, _) in pairs.iter().take(3) {
            proofs.push(proof(
                file_of(source)?,
                &line_one,
                super::recognition::VERSION,
            )?);
        }
        found.add("no-reverse-imports", &proofs);
    }
    if found.get("package").len() >= 2 && !found.get("public-interface").is_empty() {
        let mut proofs = found.get("package").to_vec();
        proofs.extend(found.get("public-interface").iter().cloned());
        found.add("multiple-public-packages", &proofs);
    }
    let empty = json!([]);
    let subsystems = topology
        .and_then(|found| lookup(found, "subsystems"))
        .unwrap_or(&empty);
    let domains: Vec<&Value> = array(subsystems)
        .iter()
        .filter(|scope| text_of(&scope["origin"]) == "declared")
        .collect();
    if domains.len() >= 2 {
        let proofs: Vec<Value> = domains
            .iter()
            .flat_map(|scope| array(&scope["evidence"]).iter().cloned())
            .collect();
        found.add("declared-domains", &proofs);
    }
    let entities = topology
        .and_then(|found| lookup(found, "entities"))
        .unwrap_or(&empty);
    let applications: Vec<&Value> = array(entities)
        .iter()
        .filter(|entity| {
            text_of(&entity["kind"]) == "Application" && members.contains(text_of(&entity["path"]))
        })
        .collect();
    if !applications.is_empty() {
        let proofs: Vec<Value> = applications
            .iter()
            .map(|entity| entity["evidence"].clone())
            .collect();
        found.add(
            if applications.len() == 1 {
                "single-configured-application"
            } else {
                "multiple-configured-applications"
            },
            &proofs,
        );
    }
    let independent: Vec<&&Value> = applications
        .iter()
        .filter(|entity| {
            matches!(
                lookup(entity, "configured_type").and_then(Value::as_str),
                Some(
                    "Deployment"
                        | "google_cloud_run_v2_service"
                        | "google_cloud_run_service"
                        | "aws_ecs_service"
                        | "azurerm_linux_web_app"
                )
            )
        })
        .collect();
    let distinct: BTreeSet<(&str, &str, &str)> = independent
        .iter()
        .map(|entity| {
            (
                text_of(&entity["path"]),
                text_of(&entity["name"]),
                text_of(&entity["environment"]),
            )
        })
        .collect();
    if distinct.len() >= 2 {
        let proofs: Vec<Value> = independent
            .iter()
            .map(|entity| entity["evidence"].clone())
            .collect();
        found.add("independent-deployment-targets", &proofs);
    }
    let api_contracts: Vec<&Value> = array(entities)
        .iter()
        .filter(|entity| {
            matches!(
                lookup(entity, "configured_type").and_then(Value::as_str),
                Some("openapi" | "asyncapi")
            ) && members.contains(text_of(&entity["path"]))
        })
        .collect();
    if !api_contracts.is_empty() {
        let proofs: Vec<Value> = api_contracts
            .iter()
            .map(|entity| entity["evidence"].clone())
            .collect();
        found.add("versioned-api-contract", &proofs);
    }
    Ok(found.into_value())
}

fn push_group<'a>(
    groups: &mut Vec<(Target, Vec<&'a Value>)>,
    target: &Target,
    relation: &'a Value,
) {
    match groups.iter_mut().find(|(key, _)| key == target) {
        Some((_, list)) => list.push(relation),
        None => groups.push((target.clone(), vec![relation])),
    }
}

fn signal_entries(signals: &Value, keys: &[&str]) -> Vec<Value> {
    keys.iter()
        .filter_map(|key| {
            let found = array(lookup(signals, key)?);
            (!found.is_empty()).then(
                || json!({"signal": key, "evidence": found.iter().take(3).collect::<Vec<_>>()}),
            )
        })
        .collect()
}

/// `match_signatures(signals, scope_id, partial)`: one verdict per pattern.
pub fn match_signatures(signals: &Value, scope_id: &str, partial: bool) -> Vec<Value> {
    let mut results = Vec::new();
    for (name, required) in SIGNATURES {
        let supporting = signal_entries(signals, required);
        let missing: Vec<&str> = required
            .iter()
            .copied()
            .filter(|key| lookup(signals, key).is_none_or(|found| array(found).is_empty()))
            .collect();
        let contradiction_keys: &[&str] = match name {
            "hexagonal" | "clean-architecture" | "ports-and-adapters" | "dependency-inversion" => {
                &["core-to-concrete"]
            }
            "least-privilege" => &["broad-grant"],
            _ => &[],
        };
        let contradicting = signal_entries(signals, contradiction_keys);
        let outcome = if !contradicting.is_empty() {
            "contradicted"
        } else if missing.is_empty() && !partial {
            "supported"
        } else {
            "inconclusive"
        };
        let mut missing_evidence: Vec<&str> = missing;
        if partial {
            missing_evidence.push(MISSING_COMPLETE);
        }
        results.push(json!({
            "pattern_id": name,
            "pattern_library_version": VERSION,
            "scope_id": scope_id,
            "outcome": outcome,
            "supporting": supporting,
            "contradictory": contradicting,
            "missing_evidence": missing_evidence,
            "origin": "inferred",
            "classifier_version": VERSION,
            "model_version": null,
            "confidence_calibration": "uncalibrated",
            "review_status": "unreviewed",
            "enforcement_authority": false,
            "explanation": if outcome == "supported" {
                EXPLANATION_SUPPORTED
            } else if outcome == "contradicted" {
                EXPLANATION_CONTRADICTED
            } else {
                EXPLANATION_ABSTAINED
            },
            "limitations": [LIMITATION],
        }));
    }
    results
}

/// `recognize(graph, scopes, structure, topology)`: every pattern verdict for every scope.
pub fn recognize(
    graph: &Value,
    scopes: &[Value],
    structure: &Value,
    topology: Option<&Value>,
) -> Result<Vec<Value>, PyError> {
    let adjacency = import_graph(graph, false)?;
    let mut merged = topology
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    merged.insert("subsystems".into(), Value::Array(scopes.to_vec()));
    let merged = Value::Object(merged);
    let mut out = Vec::new();
    for scope in scopes {
        let signals = structural_signals(graph, scope, structure, Some(&merged), Some(&adjacency))?;
        let scope_partial = lookup(scope, "partial").is_some_and(truthy);
        let coverage = lookup(structure, "signature_coverage_partial")
            .or_else(|| lookup(structure, "partial"))
            .ok_or_else(|| PyError::new("KeyError", "'partial'"))?;
        let partial = scope_partial || truthy(coverage);
        out.extend(match_signatures(
            &signals,
            text_of(get(scope, "id")?),
            partial,
        ));
    }
    Ok(out)
}
