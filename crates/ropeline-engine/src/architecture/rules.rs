//! Deterministic, bounded architecture rules with stable cross-revision identities:
//! port of `architecture_rules.py`.

use super::PyError;
use super::contract::{Contract, Rule, digest};
use super::util::{array, get, lookup, py_int, text_of, truthy};
use crate::file_categories::file_category;
use crate::py_fnmatch::fnmatchcase;
use crate::py_json::{self, JsonFailure};
use crate::py_value::PyValue;
use crate::text;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// `matches(path, patterns)`: any glob matches, or `dir/**` names the directory itself.
pub fn matches(path: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|pattern| {
        fnmatchcase(path, pattern)
            || pattern
                .strip_suffix("/**")
                .is_some_and(|directory| path == directory)
    })
}

/// `file_category(path)["category"]`.
pub fn category_of(path: &str) -> Result<&'static str, PyError> {
    file_category(Some(path), "file")
        .map(|found| found.category)
        .ok_or_else(|| PyError::new("TypeError", "'NoneType' object is not subscriptable"))
}

/// The import edges between files as `(adjacency, evidence)`.
#[derive(Debug, Default)]
pub struct ImportGraph {
    /// Every file path, in snapshot order (a dict's insertion order).
    pub order: Vec<String>,
    pub edges: HashMap<String, BTreeSet<String>>,
    pub proofs: HashMap<(String, String), Value>,
}

static NO_TARGETS: BTreeSet<String> = BTreeSet::new();

impl ImportGraph {
    /// `adjacency.get(path, set())`.
    pub fn targets(&self, path: &str) -> &BTreeSet<String> {
        self.edges.get(path).unwrap_or(&NO_TARGETS)
    }

    /// The `line` of an edge's evidence, as `int(proof.get("line", 1))`.
    pub fn line(&self, source: &str, target: &str) -> Result<i64, PyError> {
        let proof = self
            .proofs
            .get(&(source.to_owned(), target.to_owned()))
            .ok_or_else(|| PyError::new("KeyError", format!("('{source}', '{target}')")))?;
        match lookup(proof, "line") {
            Some(found) => py_int(found),
            None => Ok(1),
        }
    }
}

/// A dict key for a decoded value: equal values give equal keys.
fn key_of(value: &Value) -> String {
    text::python_json(value)
}

/// `import_graph(graph, include_type_only=...)`.
pub fn import_graph(graph: &Value, include_type_only: bool) -> Result<ImportGraph, PyError> {
    // `{f["id"]: f["path"] for f in files}`: a repeated id keeps its place and takes the later path.
    let mut ids: Vec<(String, String)> = Vec::new();
    let mut result = ImportGraph::default();
    for file in array(get(graph, "files")?) {
        let (key, path) = (
            key_of(get(file, "id")?),
            text_of(get(file, "path")?).to_owned(),
        );
        match ids.iter_mut().find(|(existing, _)| *existing == key) {
            Some(slot) => slot.1 = path,
            None => ids.push((key, path)),
        }
    }
    let paths: HashMap<String, String> = ids.iter().cloned().collect();
    for (_, path) in &ids {
        if !result.edges.contains_key(path) {
            result.order.push(path.clone());
            result.edges.insert(path.clone(), BTreeSet::new());
        }
    }
    for edge in array(get(graph, "edges")?) {
        if text_of(get(edge, "kind")?) != "IMPORTS"
            || !paths.contains_key(&key_of(get(edge, "source")?))
            || !paths.contains_key(&key_of(get(edge, "target")?))
        {
            continue;
        }
        let source = paths[&key_of(get(edge, "source")?)].clone();
        let target = paths[&key_of(get(edge, "target")?)].clone();
        let proof = match lookup(edge, "evidence") {
            Some(Value::String(encoded)) => decode_proof(encoded)?,
            Some(other) => other.clone(),
            None => json!({}),
        };
        if !proof.is_object() {
            return Err(PyError::new(
                "AttributeError",
                format!(
                    "'{}' object has no attribute 'get'",
                    crate::py_value::PyValue::from_value(&proof).type_name()
                ),
            ));
        }
        let flag = |key: &str| lookup(&proof, key).is_some_and(truthy);
        if flag("inferred") || (flag("type_only") && !include_type_only) {
            continue;
        }
        result
            .edges
            .entry(source.clone())
            .or_default()
            .insert(target.clone());
        result.proofs.insert((source, target), proof);
    }
    Ok(result)
}

/// `json.loads(proof)` for evidence stored as text.
fn decode_proof(encoded: &str) -> Result<Value, PyError> {
    match py_json::loads(encoded, None) {
        Ok(value) => Ok(value.to_value().unwrap_or(Value::Null)),
        Err(JsonFailure::Decode(message)) => Err(PyError::new("JSONDecodeError", message)),
        Err(JsonFailure::Value(message)) => Err(PyError::value(message)),
        Err(JsonFailure::Recursion) => Err(PyError::new(
            "RecursionError",
            "maximum recursion depth exceeded while decoding a JSON document",
        )),
    }
}

/// Strongly connected components (iterative Kosaraju, so large repositories cannot hit
/// a recursion limit), each sorted, the list sorted. Self-imports count.
pub fn components(adjacency: &ImportGraph) -> Vec<Vec<String>> {
    let mut visited: BTreeSet<String> = BTreeSet::new();
    let mut order: Vec<String> = Vec::new();
    let mut reverse: BTreeMap<String, BTreeSet<String>> = adjacency
        .edges
        .keys()
        .map(|path| (path.clone(), BTreeSet::new()))
        .collect();
    for (path, targets) in &adjacency.edges {
        for target in targets {
            reverse
                .entry(target.clone())
                .or_default()
                .insert(path.clone());
        }
    }
    let mut starts: Vec<&String> = adjacency.edges.keys().collect();
    starts.sort();
    for start in starts {
        if visited.contains(start) {
            continue;
        }
        let mut pending: Vec<(String, bool)> = vec![(start.clone(), false)];
        while let Some((path, done)) = pending.pop() {
            if done {
                order.push(path);
            } else if !visited.contains(&path) {
                visited.insert(path.clone());
                pending.push((path.clone(), true));
                let next: Vec<(String, bool)> = adjacency
                    .targets(&path)
                    .iter()
                    .rev()
                    .filter(|candidate| !visited.contains(*candidate))
                    .map(|candidate| (candidate.clone(), false))
                    .collect();
                pending.extend(next);
            }
        }
    }
    visited.clear();
    let mut result: Vec<Vec<String>> = Vec::new();
    for start in order.iter().rev() {
        if visited.contains(start) {
            continue;
        }
        let mut members: Vec<String> = Vec::new();
        let mut stack = vec![start.clone()];
        visited.insert(start.clone());
        while let Some(path) = stack.pop() {
            members.push(path.clone());
            if let Some(parents) = reverse.get(&path) {
                for parent in parents {
                    if !visited.contains(parent) {
                        visited.insert(parent.clone());
                        stack.push(parent.clone());
                    }
                }
            }
        }
        if members.len() > 1 || adjacency.targets(start).contains(start) {
            members.sort();
            result.push(members);
        }
    }
    result.sort();
    result
}

/// `effective_rules(contract, path)`: the rules that govern a path, narrowest scope last,
/// one per id.
pub fn effective_rules<'a>(contract: &'a Contract, path: &str) -> Vec<&'a Rule> {
    let mut ordered: Vec<&Rule> = contract.rules.iter().collect();
    ordered.sort_by(|left, right| {
        (text::char_len(&left.scope), &left.scope)
            .cmp(&(text::char_len(&right.scope), &right.scope))
    });
    let mut selected: Vec<(&str, &Rule)> = Vec::new();
    for rule in ordered {
        if matches(path, std::slice::from_ref(&rule.scope)) && matches(path, &rule.source) {
            // A repeated id keeps its first position and takes the later rule.
            match selected.iter_mut().find(|(id, _)| *id == rule.id) {
                Some(slot) => slot.1 = rule,
                None => selected.push((&rule.id, rule)),
            }
        }
    }
    selected.into_iter().map(|(_, rule)| rule).collect()
}

/// What `FindingBuilder.make` takes besides the rule and paths.
pub struct Options<'a> {
    pub target: &'a str,
    pub line: i64,
    pub measure: Value,
    pub detail: &'a str,
    pub baseline: Option<&'a Value>,
}

impl Default for Options<'_> {
    fn default() -> Self {
        Self {
            target: "",
            line: 1,
            measure: json!(1),
            detail: "",
            baseline: None,
        }
    }
}

/// Keeps source identity and evidence consistent across static and delta rules.
pub struct FindingBuilder<'a> {
    graph: &'a Value,
    files: HashMap<String, &'a Value>,
    adjacency: ImportGraph,
}

impl<'a> FindingBuilder<'a> {
    pub fn new(graph: &'a Value) -> Result<Self, PyError> {
        let mut files = HashMap::new();
        for file in array(get(graph, "files")?) {
            files.insert(text_of(get(file, "path")?).to_owned(), file);
        }
        Ok(Self {
            graph,
            files,
            adjacency: import_graph(graph, true)?,
        })
    }

    pub fn make(
        &self,
        rule: &Rule,
        paths: &[String],
        options: &Options<'_>,
    ) -> Result<Value, PyError> {
        let dependency = matches!(
            rule.kind.as_str(),
            "forbidden-dependency" | "canonical-authority" | "cycles"
        );
        let mut proofs: Vec<Value> = Vec::new();
        let sides: Vec<&Value> = options.baseline.into_iter().chain([self.graph]).collect();
        for side in sides {
            for file in array(get(side, "files")?) {
                let path = get(file, "path")?;
                if paths.iter().any(|candidate| candidate == text_of(path)) {
                    proofs.push(json!({
                        "repository": get(side, "repository")?,
                        "revision": get(side, "revision")?,
                        "path": path,
                        "line": options.line,
                        "method": if dependency { "static-import" } else { "architecture-facts-v1" },
                        "sha256": lookup(get(file, "facts")?, "sha256").cloned().unwrap_or(Value::Null),
                    }));
                }
            }
        }
        let mut sorted_paths: Vec<String> = paths.to_vec();
        sorted_paths.sort();
        let file_id = |path: &str| -> Result<Value, PyError> {
            let file = self
                .files
                .get(path)
                .ok_or_else(|| PyError::new("KeyError", crate::py_value::str_repr(path)))?;
            Ok(get(file, "id")?.clone())
        };
        let mut nodes: Vec<Value> = Vec::new();
        for path in &sorted_paths {
            if self.files.contains_key(path) {
                nodes.push(file_id(path)?);
            }
        }
        let mut edges: Vec<Value> = Vec::new();
        'collect: for source in paths {
            for target in self.adjacency.targets(source) {
                let wanted = dependency
                    && (paths.contains(target)
                        || target == options.target
                        || self.adjacency.line(source, target)? == options.line);
                if wanted {
                    edges.push(json!({
                        "source": file_id(source)?,
                        "target": file_id(target)?,
                        "kind": "IMPORTS",
                    }));
                    if edges.len() >= 256 {
                        break 'collect;
                    }
                }
            }
        }
        let mut finding = Map::new();
        finding.insert(
            "id".into(),
            json!(digest(&json!([rule.id, sorted_paths, options.target]))),
        );
        finding.insert("rule_id".into(), json!(rule.id));
        finding.insert("rule_kind".into(), json!(rule.kind));
        finding.insert("requested_severity".into(), json!(rule.severity));
        finding.insert("origin".into(), json!("observed"));
        finding.insert("confidence".into(), json!(1.0));
        finding.insert("paths".into(), json!(sorted_paths));
        finding.insert("target".into(), json!(options.target));
        finding.insert("measure".into(), options.measure.clone());
        finding.insert(
            "explanation".into(),
            json!(if options.detail.is_empty() {
                rule.reason.clone()
            } else {
                format!("{} {}", rule.reason, options.detail)
            }),
        );
        finding.insert("remediation".into(), json!(rule.remediation));
        finding.insert("affected_nodes".into(), Value::Array(nodes));
        finding.insert("affected_edges".into(), Value::Array(edges));
        finding.insert("evidence".into(), Value::Array(proofs));
        Ok(Value::Object(finding))
    }
}

fn number(value: Option<&Value>, default: i64) -> i64 {
    match value {
        Some(found) => found
            .as_i64()
            .or_else(|| found.as_f64().map(|float| float as i64))
            .unwrap_or(default),
        None => default,
    }
}

/// A fact's numeric value for a comparison; absent counts as zero.
fn quantity(value: Option<&Value>) -> f64 {
    value.and_then(Value::as_f64).unwrap_or(0.0)
}

/// `violations(graph, contract)`: every static policy failure, in a stable order.
pub fn violations(graph: &Value, contract: &Contract) -> Result<Vec<Value>, PyError> {
    let adjacency = import_graph(graph, false)?;
    let all = import_graph(graph, true)?;
    let mut by_path: Vec<(String, &Value)> = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    for file in array(get(graph, "files")?) {
        let path = text_of(get(file, "path")?).to_owned();
        match seen.get(&path) {
            Some(slot) => by_path[*slot].1 = file,
            None => {
                seen.insert(path.clone(), by_path.len());
                by_path.push((path, file));
            }
        }
    }
    let builder = FindingBuilder::new(graph)?;
    let mut found: Vec<(String, Value)> = Vec::new();
    let mut add = |rule: &Rule, paths: &[String], options: Options<'_>| -> Result<(), PyError> {
        let finding = builder.make(rule, paths, &options)?;
        let id = text_of(get(&finding, "id")?).to_owned();
        match found.iter_mut().find(|(existing, _)| *existing == id) {
            Some(slot) => slot.1 = finding,
            None => found.push((id, finding)),
        }
        Ok(())
    };

    let cycles = components(&adjacency);
    let mut selected_rules: HashMap<&str, Vec<&Rule>> = HashMap::new();
    for (path, _) in &by_path {
        selected_rules.insert(path, effective_rules(contract, path));
    }
    let mut cycle_anchors: HashMap<(String, String, String), &Vec<String>> = HashMap::new();
    for rule in contract.rules.iter().filter(|rule| rule.kind == "cycles") {
        for members in &cycles {
            let eligible: Vec<&String> = members
                .iter()
                .filter(|member| {
                    selected_rules.get(member.as_str()).is_some_and(|rules| {
                        rules
                            .iter()
                            .any(|other| other.id == rule.id && other.scope == rule.scope)
                    })
                })
                .collect();
            if let Some(anchor) = eligible.into_iter().min() {
                cycle_anchors.insert(
                    (rule.id.clone(), rule.scope.clone(), anchor.clone()),
                    members,
                );
            }
        }
    }
    let mut clones: Vec<(String, Vec<String>)> = Vec::new();
    let imported: BTreeSet<&String> = adjacency.edges.values().flatten().collect();
    for (path, file) in &by_path {
        let facts = get(file, "facts")?;
        let category = category_of(path)?;
        let sha = lookup(facts, "sha256").filter(|found| truthy(found));
        if category == "code"
            && quantity(lookup(facts, "nonblank_lines")) >= 25.0
            && let Some(sha) = sha
        {
            let key = text_of(sha).to_owned();
            match clones.iter_mut().find(|(existing, _)| *existing == key) {
                Some(slot) => slot.1.push(path.clone()),
                None => clones.push((key, vec![path.clone()])),
            }
        }
        for rule in &selected_rules[path.as_str()] {
            let rule = *rule;
            match rule.kind.as_str() {
                "forbidden-dependency" | "canonical-authority" => {
                    if rule.kind == "canonical-authority" && matches(path, &rule.allowed_sources) {
                        continue;
                    }
                    let source = if rule.include_type_only {
                        &all
                    } else {
                        &adjacency
                    };
                    let mut targets: Vec<(String, i64, bool)> = Vec::new();
                    for target in source.targets(path) {
                        targets.push((target.clone(), source.line(path, target)?, false));
                    }
                    let resolved: BTreeSet<i64> = targets
                        .iter()
                        .filter(|(target, _, _)| matches(target, &rule.target))
                        .map(|(_, line, _)| *line)
                        .collect();
                    for fact in lookup(facts, "imports").map(array).unwrap_or_default() {
                        let line = get(fact, "line")?;
                        if line.as_i64().is_some_and(|line| resolved.contains(&line)) {
                            continue;
                        }
                        let line_number = number(Some(line), 1);
                        if let Some(specifier) = lookup(fact, "specifier") {
                            let type_only = lookup(fact, "type_only").is_some_and(truthy);
                            targets.push((text_of(specifier).to_owned(), line_number, type_only));
                        } else if let Some(module) =
                            lookup(fact, "module").filter(|found| truthy(found))
                        {
                            targets.push((text_of(module).replace('.', "/"), line_number, false));
                        } else if lookup(fact, "kind").is_some_and(|kind| kind == "rust_use") {
                            let path: Vec<&str> =
                                array(get(fact, "path")?).iter().map(text_of).collect();
                            targets.push((path.join("/"), line_number, false));
                        } else if lookup(fact, "kind").is_some_and(|kind| kind == "python_import") {
                            for name in array(get(fact, "names")?) {
                                targets.push((text_of(name).replace('.', "/"), line_number, false));
                            }
                        }
                    }
                    for (target, line, type_only) in targets {
                        if (!type_only || rule.include_type_only) && matches(&target, &rule.target)
                        {
                            add(
                                rule,
                                std::slice::from_ref(path),
                                Options {
                                    target: &target,
                                    line,
                                    ..Options::default()
                                },
                            )?;
                        }
                    }
                }
                "max-module-lines"
                    if category == "code"
                        && quantity(lookup(facts, "lines")) > rule.limit as f64 =>
                {
                    let lines = get(facts, "lines")?;
                    add(
                        rule,
                        std::slice::from_ref(path),
                        Options {
                            measure: lines.clone(),
                            detail: &format!(
                                "{} lines; approved limit {}.",
                                PyValue::from_value(lines).py_str(),
                                rule.limit
                            ),
                            ..Options::default()
                        },
                    )?;
                }
                "test-integrity" if category == "test" => {
                    let vacuous =
                        lookup(facts, "vacuous_assertions").map_or(0, |list| array(list).len());
                    for (kind, count) in [
                        (
                            "skipped-tests",
                            lookup(facts, "test_skips").cloned().unwrap_or(json!(0)),
                        ),
                        (
                            "focused-tests",
                            lookup(facts, "test_focused").cloned().unwrap_or(json!(0)),
                        ),
                        ("vacuous-assertions", json!(vacuous)),
                    ] {
                        if truthy(&count) {
                            add(
                                rule,
                                std::slice::from_ref(path),
                                Options {
                                    target: kind,
                                    measure: count.clone(),
                                    detail: &format!(
                                        "Observed {kind}; compare the baseline and legitimate portability exceptions."
                                    ),
                                    ..Options::default()
                                },
                            )?;
                        }
                    }
                }
                "coupling" if adjacency.targets(path).len() as i64 > rule.limit => {
                    add(
                        rule,
                        std::slice::from_ref(path),
                        Options {
                            measure: json!(adjacency.targets(path).len()),
                            detail: "Resolved import fan-out exceeds the configured limit.",
                            ..Options::default()
                        },
                    )?;
                }
                "orphan-module"
                    if category == "code"
                        && adjacency.targets(path).is_empty()
                        && !imported.contains(path) =>
                {
                    add(
                        rule,
                        std::slice::from_ref(path),
                        Options {
                            detail: "No resolved static import connection; this may be an entry point or dynamically loaded module.",
                            ..Options::default()
                        },
                    )?;
                }
                "cycles" => {
                    let key = (rule.id.clone(), rule.scope.clone(), path.clone());
                    if let Some(members) = cycle_anchors.get(&key) {
                        add(
                            rule,
                            members,
                            Options {
                                measure: json!(members.len()),
                                detail: "Resolved value imports form a strongly connected component.",
                                ..Options::default()
                            },
                        )?;
                    }
                }
                _ => {}
            }
        }
    }
    for (_, members) in &clones {
        if members.len() < 2 {
            continue;
        }
        let first = members.iter().min().expect("at least two members");
        for rule in effective_rules(contract, first) {
            if rule.kind == "duplicate-code" {
                let mut sorted = members.clone();
                sorted.sort();
                add(
                    rule,
                    &sorted,
                    Options {
                        measure: json!(members.len()),
                        detail: "Production files contain identical bytes (at least 25 non-blank lines); shared responsibility still requires judgement.",
                        ..Options::default()
                    },
                )?;
            }
        }
    }
    let mut findings: Vec<Value> = found.into_iter().map(|(_, finding)| finding).collect();
    findings.sort_by_key(|finding| {
        (
            text_of(&finding["rule_id"]).to_owned(),
            array(&finding["paths"])
                .iter()
                .map(|path| text_of(path).to_owned())
                .collect::<Vec<_>>(),
            text_of(&finding["target"]).to_owned(),
        )
    });
    Ok(findings)
}
