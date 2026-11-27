//! The Rust engine against what the Python engine recorded for the same inputs.
//!
//! `tools/record_engine_parity.py` records Python's output into `tests/parity/expected`;
//! `tests/test_engine_parity.py` keeps those files honest. Every case here must match
//! exactly (as parsed JSON, so line endings never matter). Differences that cannot be
//! removed are listed in `known_differences`, and each one is asserted
//! so it cannot grow unnoticed. Nothing is loosened to make a test pass.

#[path = "parity/architecture/mod.rs"]
mod architecture;

use base64::Engine as _;
use codegraph_engine::EngineError;
use codegraph_engine::codeowners;
use codegraph_engine::file_categories::file_category;
use codegraph_engine::parsing::{self, Symbol};
use codegraph_engine::pyast::{self, ParseFailure};
use codegraph_engine::relationships::{
    self, Aliases, CallFact, ImportFact, References, ResolvedImport,
};
use codegraph_engine::text;
use codegraph_engine::waypoints;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};

const SOURCES: &str = include_str!("parity/sources.json");
const SPECS: &str = include_str!("parity/specs.json");

fn expected(section: &str) -> Value {
    let raw = match section {
        "symbols" => include_str!("parity/expected/symbols.json"),
        "references" => include_str!("parity/expected/references.json"),
        "extract" => include_str!("parity/expected/extract.json"),
        "relative-targets" => include_str!("parity/expected/relative-targets.json"),
        "aliases" => include_str!("parity/expected/aliases.json"),
        "rust-crates" => include_str!("parity/expected/rust-crates.json"),
        "file-categories" => include_str!("parity/expected/file-categories.json"),
        "codeowners" => include_str!("parity/expected/codeowners.json"),
        "waypoints" => include_str!("parity/expected/waypoints.json"),
        "ast-walk" => include_str!("parity/expected/ast-walk.json"),
        "text" => include_str!("parity/expected/text.json"),
        other => panic!("unknown section {other}"),
    };
    serde_json::from_str(raw).expect("expected file is JSON")
}

fn specs() -> Value {
    serde_json::from_str(SPECS).expect("specs are JSON")
}

/// The input manifest: name to bytes, decoding `{"base64": ...}` entries.
fn sources() -> BTreeMap<String, Vec<u8>> {
    let manifest: Value = serde_json::from_str(SOURCES).expect("sources are JSON");
    manifest["files"]
        .as_object()
        .expect("files")
        .iter()
        .map(|(name, content)| {
            let bytes = match content {
                Value::String(text) => text.clone().into_bytes(),
                other => base64::engine::general_purpose::STANDARD
                    .decode(other["base64"].as_str().expect("base64 entry"))
                    .expect("valid base64"),
            };
            (name.clone(), bytes)
        })
        .collect()
}

fn names_in<'a>(
    files: &'a BTreeMap<String, Vec<u8>>,
    folders: &'a [&str],
) -> impl Iterator<Item = &'a String> {
    files
        .keys()
        .filter(move |name| folders.contains(&name.split('/').next().unwrap_or("")))
}

fn languages_for(name: &str) -> &'static [&'static str] {
    if name.ends_with(".rs") {
        &["rust"]
    } else if name.ends_with(".py") {
        &["python"]
    } else {
        &["typescript", "tsx", "javascript"]
    }
}

/// Collects every mismatch in a section, so one run reports all of them.
#[derive(Default)]
struct Mismatches(Vec<String>);

impl Mismatches {
    fn check(&mut self, id: &str, actual: &Value, wanted: &Value) {
        if let Some(reason) = known_difference(id) {
            // A documented difference must still differ, or the entry is stale.
            if actual == wanted {
                self.0.push(format!(
                    "{id}: matches Python now; remove it from known_differences ({reason})"
                ));
            }
            return;
        }
        if actual != wanted {
            self.0.push(format!(
                "{id}\n  python: {}\n  rust:   {}",
                clip(wanted),
                clip(actual)
            ));
        }
    }

    fn finish(self, section: &str) {
        assert!(
            self.0.is_empty(),
            "{section}: {} mismatch(es)\n{}",
            self.0.len(),
            self.0
                .iter()
                .take(15)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}

fn clip(value: &Value) -> String {
    let rendered = value.to_string();
    if rendered.chars().count() > 600 {
        format!("{}...", rendered.chars().take(600).collect::<String>())
    } else {
        rendered
    }
}

/// Recorded Python results the engine cannot reproduce.
fn known_difference(id: &str) -> Option<&'static str> {
    match id {
        "symbols/script/syntax_error.ts:javascript"
        | "symbols/snippet/empty_decorator.py:python" => Some(
            "tree-sitter error recovery differs between the grammar versions in tree_sitter_language_pack 1.23.1 and the pinned crates",
        ),
        other => architecture::known_difference(other),
    }
}

fn recursion_message() -> String {
    "maximum recursion depth exceeded during ast construction".to_owned()
}

/// The message Python gives when a `use` tree is walked too deep.
fn nesting_message() -> String {
    "maximum recursion depth exceeded".to_owned()
}

fn outcome_of_references(result: Result<References, EngineError>) -> Value {
    match result {
        Ok(found) => json!({"value": serde_json::to_value(found).expect("serialises")}),
        Err(EngineError::Recursion) => {
            json!({"error": "RecursionError", "message": recursion_message()})
        }
        Err(EngineError::Nesting) => {
            json!({"error": "RecursionError", "message": nesting_message()})
        }
        Err(other) => json!({"error": "EngineError", "message": other.to_string()}),
    }
}

fn symbol_values(symbols: Vec<Symbol>) -> Value {
    Value::Array(
        symbols
            .into_iter()
            .map(|symbol| json!([symbol.kind, symbol.name, symbol.line, symbol.parent]))
            .collect(),
    )
}

#[test]
fn symbols_match_python() {
    let files = sources();
    let mut mismatches = Mismatches::default();
    let wanted = expected("symbols");
    for (key, want) in wanted.as_object().expect("object") {
        let actual = if key == "text" {
            json!({"value": symbol_values(parsing::symbols("text", b"anything").expect("text"))})
        } else {
            let (name, language) = key.rsplit_once(':').expect("name:language");
            match parsing::symbols(language, &files[name]) {
                Ok(symbols) => json!({"value": symbol_values(symbols)}),
                Err(error) => json!({"error": "EngineError", "message": error.to_string()}),
            }
        };
        mismatches.check(&format!("symbols/{key}"), &actual, want);
    }
    assert_eq!(
        wanted.as_object().expect("object").len(),
        names_in(&files, &["python", "script", "snippet", "rust"])
            .map(|name| languages_for(name).len())
            .sum::<usize>()
            + 1,
        "the recording covers every input"
    );
    mismatches.finish("symbols");
}

#[test]
fn references_match_python() {
    let files = sources();
    let mut mismatches = Mismatches::default();
    for (key, want) in expected("references").as_object().expect("object") {
        let actual = match key.as_str() {
            "none" => outcome_of_references(relationships::references(None, "import x")),
            "unknown" => outcome_of_references(relationships::references(Some("go"), "import x")),
            _ => {
                let (name, language) = key.rsplit_once(':').expect("name:language");
                let source = text::decode_replace(&files[name]);
                outcome_of_references(relationships::references(Some(language), &source))
            }
        };
        mismatches.check(&format!("references/{key}"), &actual, want);
    }
    mismatches.finish("references");
}

fn known_set(paths: &Value) -> HashSet<String> {
    paths
        .as_array()
        .expect("array")
        .iter()
        .map(|path| path.as_str().expect("string").to_owned())
        .collect()
}

fn outcome_pair(result: Result<(Vec<ResolvedImport>, Vec<CallFact>), EngineError>) -> Value {
    match result {
        Ok((imports, calls)) => json!({"value": [imports, calls]}),
        Err(EngineError::Recursion) => {
            json!({"error": "RecursionError", "message": recursion_message()})
        }
        Err(EngineError::Nesting) => {
            json!({"error": "RecursionError", "message": nesting_message()})
        }
        Err(other) => json!({"error": "EngineError", "message": other.to_string()}),
    }
}

#[test]
fn extract_matches_python() {
    let files = sources();
    let spec = specs();
    let relationships_spec = &spec["relationships"];
    let shared = known_set(&relationships_spec["known"]);
    let wanted = expected("extract");
    let mut mismatches = Mismatches::default();
    for case in relationships_spec["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("id");
        let source = text::decode_replace(&files[case["source"].as_str().expect("source")]);
        // Rust links depend on the layout, so a case may bring its own files.
        let own = case.get("known").map(known_set);
        let actual = outcome_pair(relationships::extract(
            case["path"].as_str().expect("path"),
            &source,
            own.as_ref().unwrap_or(&shared),
        ));
        mismatches.check(&format!("extract/{id}"), &actual, &wanted[id]);
    }
    mismatches.finish("extract");
}

/// Rust `use` paths that name another crate in the same repository.
#[test]
fn rust_crates_match_python() {
    let files = sources();
    let spec = specs();
    let wanted = expected("rust-crates");
    let mut mismatches = Mismatches::default();
    for case in spec["relationships"]["rustCrateCases"]
        .as_array()
        .expect("cases")
    {
        let id = case["id"].as_str().expect("id");
        let known = case["known"].as_object().expect("known files");
        let manifests: Vec<(String, Value)> = known
            .iter()
            .map(|(path, facts)| {
                (
                    path.clone(),
                    facts.as_object().map_or(json!({}), |_| facts.clone()),
                )
            })
            .collect();
        let paths: HashSet<String> = known.keys().cloned().collect();
        let crates = relationships::crate_roots(&manifests, &|path| paths.contains(path))
            .expect("crates build");
        let resolve = |item: &Value, with: Option<&relationships::CrateRoots>| {
            let source = text::decode_replace(&files[item["source"].as_str().expect("source")]);
            let facts = relationships::references(Some("rust"), &source).expect("references");
            let (imports, calls) = relationships::resolve(
                item["path"].as_str().expect("path"),
                &facts,
                &paths,
                None,
                with,
            );
            json!({"value": [imports, calls]})
        };
        let items = case["sources"].as_array().expect("sources");
        let actual = json!({
            "crates": crates,
            "resolved": items.iter().map(|item| resolve(item, Some(&crates))).collect::<Vec<_>>(),
            "resolvedWithoutCrates": items.iter().map(|item| resolve(item, None)).collect::<Vec<_>>(),
        });
        mismatches.check(&format!("rust-crates/{id}"), &actual, &wanted[id]);
    }
    mismatches.finish("rust-crates");
}

#[test]
fn relative_targets_match_python() {
    let spec = specs();
    let wanted = expected("relative-targets");
    let cases = spec["relationships"]["relativeTargets"]
        .as_array()
        .expect("cases");
    assert_eq!(cases.len(), wanted.as_array().expect("list").len());
    let mut mismatches = Mismatches::default();
    for (index, case) in cases.iter().enumerate() {
        let known = known_set(&case["known"]);
        let target = relationships::relative_target(
            case["path"].as_str().expect("path"),
            case["specifier"].as_str().expect("specifier"),
            &known,
        );
        mismatches.check(
            &format!(
                "relative-targets/{index} {} {}",
                case["path"], case["specifier"]
            ),
            &json!({"value": target}),
            &wanted[index],
        );
    }
    mismatches.finish("relative-targets");
}

#[test]
fn package_aliases_match_python() {
    let spec = specs();
    let wanted = expected("aliases");
    let mut mismatches = Mismatches::default();
    for case in spec["relationships"]["aliasCases"]
        .as_array()
        .expect("cases")
    {
        let id = case["id"].as_str().expect("id");
        let known_entries: Option<Vec<(String, Value)>> = case["known"].as_object().map(|object| {
            object
                .iter()
                .map(|(path, entry)| (path.clone(), entry[1]["facts"].clone()))
                .collect()
        });
        let aliases = relationships::module_aliases(known_entries.as_deref().unwrap_or(&[]))
            .expect("aliases build");
        let paths: HashSet<String> = known_entries
            .iter()
            .flatten()
            .map(|(path, _)| path.clone())
            .collect();
        let specifiers: Vec<&str> = case["specifiers"]
            .as_array()
            .expect("specifiers")
            .iter()
            .map(|value| value.as_str().expect("string"))
            .collect();
        let facts = References {
            imports: specifiers
                .iter()
                .enumerate()
                .map(|(index, specifier)| ImportFact::Relative {
                    specifier: (*specifier).to_owned(),
                    line: index + 1,
                    type_only: false,
                })
                .collect(),
            calls: Vec::new(),
        };
        let alias_map: Map<String, Value> = aliases
            .iter()
            .map(|(alias, entries)| {
                (
                    alias.to_owned(),
                    Value::Array(
                        entries
                            .iter()
                            .map(|(path, target)| json!([path, target]))
                            .collect(),
                    ),
                )
            })
            .collect();
        let resolve = |path: &str, with: Option<&Aliases>| {
            let (imports, calls) = relationships::resolve(path, &facts, &paths, with, None);
            json!({"value": [imports, calls]})
        };
        let actual = json!({
            "aliases": alias_map,
            "targets": specifiers
                .iter()
                .map(|specifier| json!({"value": relationships::package_target(specifier, &paths, &aliases)}))
                .collect::<Vec<_>>(),
            "resolved": case["sources"]
                .as_array()
                .expect("sources")
                .iter()
                .map(|source| resolve(source["path"].as_str().expect("path"), Some(&aliases)))
                .collect::<Vec<_>>(),
            "resolvedWithoutAliases": resolve(
                case["sources"][0]["path"].as_str().expect("path"),
                None,
            ),
        });
        mismatches.check(&format!("aliases/{id}"), &actual, &wanted[id]);
    }
    mismatches.finish("aliases");
}

#[test]
fn file_categories_match_python() {
    let spec = specs();
    let spec = &spec["fileCategories"];
    let strings = |key: &str| -> Vec<String> {
        spec[key]
            .as_array()
            .expect("array")
            .iter()
            .map(|value| value.as_str().expect("string").to_owned())
            .collect()
    };
    let mut inputs: Vec<(Option<String>, String)> = Vec::new();
    inputs.extend(
        strings("paths")
            .into_iter()
            .map(|path| (Some(path), "file".to_owned())),
    );
    inputs.extend(
        strings("kinds")
            .into_iter()
            .map(|kind| (Some("tests".to_owned()), kind)),
    );
    inputs.extend(
        strings("kinds")
            .into_iter()
            .map(|kind| (Some("tests/a.py".to_owned()), kind)),
    );
    for length in spec["longPaths"].as_array().expect("lengths") {
        inputs.push((
            Some("a".repeat(length.as_u64().expect("int") as usize)),
            "file".into(),
        ));
    }
    inputs.push((Some(format!("{}b", "a/".repeat(2047))), "file".into()));
    inputs.push((Some(format!("{}b", "a/".repeat(2048))), "file".into()));
    for value in spec["nonStrings"].as_array().expect("values") {
        inputs.push((value.as_str().map(str::to_owned), "file".into()));
    }
    let wanted = expected("file-categories");
    let wanted = wanted.as_array().expect("list");
    assert_eq!(inputs.len(), wanted.len(), "same inputs as the recording");
    let mut mismatches = Mismatches::default();
    for (index, (path, kind)) in inputs.iter().enumerate() {
        if let (Some(path), Some(length)) = (path, wanted[index]["length"].as_u64()) {
            assert_eq!(text::char_len(path) as u64, length, "input {index}");
        }
        let actual = json!({"value": file_category(path.as_deref(), kind)});
        mismatches.check(
            &format!(
                "file-categories/{index} {:?} ({kind})",
                path.as_deref()
                    .map(|p| p.chars().take(40).collect::<String>())
            ),
            &actual,
            &wanted[index]["result"],
        );
    }
    mismatches.finish("file-categories");
}

fn describe_ownership(ownership: Option<codeowners::Ownership>, lookups: &[String]) -> Value {
    let Some(ownership) = ownership else {
        return Value::Null;
    };
    json!({
        "path": ownership.path,
        "rules": ownership.rules,
        "digest": ownership.digest,
        "record": ownership.record(),
        "lookups": lookups
            .iter()
            .map(|path| ownership.lookup(path))
            .collect::<Vec<_>>(),
    })
}

#[test]
fn codeowners_match_python() {
    let files = sources();
    let spec = specs();
    let spec = &spec["codeowners"];
    let wanted = expected("codeowners");
    let lookups: Vec<String> = spec["lookups"]
        .as_array()
        .expect("lookups")
        .iter()
        .map(|value| value.as_str().expect("string").to_owned())
        .collect();
    let mut mismatches = Mismatches::default();
    mismatches.check(
        "codeowners/constants",
        &json!({
            "CANDIDATES": codeowners::CANDIDATES,
            "MAX_BYTES": codeowners::MAX_BYTES,
            "MAX_RULES": codeowners::MAX_RULES,
            "MAX_OWNERS": codeowners::MAX_OWNERS,
            "MAX_TOKEN": codeowners::MAX_TOKEN,
            "MAX_PATTERN": codeowners::MAX_PATTERN,
            "MAX_DOUBLE_STARS": codeowners::MAX_DOUBLE_STARS,
        }),
        &wanted["constants"],
    );
    for case in spec["files"].as_array().expect("files") {
        let id = case["id"].as_str().expect("id");
        let data = &files[&format!("codeowners/{}", case["file"].as_str().expect("file"))];
        let ownership = codeowners::parse(data, case["path"].as_str().expect("path"));
        mismatches.check(
            &format!("codeowners/files/{id}"),
            &json!({"value": describe_ownership(ownership, &lookups)}),
            &wanted["files"][id],
        );
    }
    // Python refuses text where bytes are required; the Rust signature takes bytes only.
    assert_eq!(wanted["notBytes"], json!({"value": null}));
    let mut owners: Vec<Value> = spec["owners"]
        .as_array()
        .expect("owners")
        .iter()
        .map(|owner| {
            json!([
                owner,
                codeowners::valid_owner(owner.as_str().expect("string"))
            ])
        })
        .collect();
    for length in spec["ownerLengths"].as_array().expect("lengths") {
        let owner = format!("@{}", "a".repeat(length.as_u64().expect("int") as usize));
        owners.push(json!([owner, codeowners::valid_owner(&owner)]));
    }
    owners.push(json!([null, false]));
    mismatches.check(
        "codeowners/owners",
        &Value::Array(owners),
        &wanted["owners"],
    );
    for case in spec["metadata"].as_array().expect("metadata") {
        let id = case["id"].as_str().expect("id");
        mismatches.check(
            &format!("codeowners/metadata/{id}"),
            &json!({"value": describe_ownership(codeowners::from_metadata(&case["value"]), &lookups)}),
            &wanted["metadata"][id],
        );
    }
    mismatches.finish("codeowners");
}

fn outcome_of<T: serde::Serialize>(result: Result<T, waypoints::WaypointError>) -> Value {
    match result {
        Ok(value) => json!({"value": value}),
        Err(error) => json!({"error": error.kind, "message": error.message}),
    }
}

fn sha(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

#[test]
fn waypoints_match_python() {
    let files = sources();
    let spec = specs();
    let spec = &spec["waypoints"];
    let wanted = expected("waypoints");
    let identity = |seed: &str| sha(seed.as_bytes());
    let policy = identity(spec["policySeed"].as_str().expect("seed"));
    let analysis = identity(spec["analysisSeed"].as_str().expect("seed"));
    let dependencies: Value = Value::Array(
        spec["dependencies"]
            .as_array()
            .expect("dependencies")
            .iter()
            .map(|entry| {
                let id = entry[1].as_str().expect("id");
                let id = id.strip_prefix("seed:").map_or(id.to_owned(), identity);
                json!([entry[0], id, entry[2]])
            })
            .collect(),
    );
    let mut mismatches = Mismatches::default();
    mismatches.check(
        "waypoints/constants",
        &json!({
            "VERSION": waypoints::VERSION,
            "VALIDATOR_VERSION": waypoints::VALIDATOR_VERSION,
            "MAX_SOURCE_BYTES": waypoints::MAX_SOURCE_BYTES,
            "MAX_CHILDREN": waypoints::MAX_CHILDREN,
            "PURPOSES": waypoints::PURPOSES,
        }),
        &wanted["constants"],
    );
    let mut children: BTreeMap<String, Value> = BTreeMap::new();
    let mut sample: Option<(Value, String)> = None;
    for case in spec["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("id");
        let data = &files[case["source"].as_str().expect("source")];
        let content_hash = sha(data);
        let language = case["language"].as_str().expect("language");
        let symbols: Vec<Value> = if case["symbols"] == "indexed" {
            parsing::symbols(language, data)
                .map(|found| waypoints::symbols_as_values(&found))
                .unwrap_or_default()
        } else {
            case["symbols"].as_array().expect("symbols").clone()
        };
        let mut entry = Map::new();
        let built = waypoints::build_file_facts(language, data, &symbols, &case["facts"]);
        entry.insert("facts".into(), outcome_of(built.clone()));
        if let Ok(record) = built {
            if sample.is_none() {
                sample = Some((record.clone(), content_hash.clone()));
            }
            let validation = waypoints::validate_file(&record, &dependencies, &policy, &analysis);
            entry.insert("validation".into(), outcome_of(validation.clone()));
            let path = case["path"].as_str().expect("path");
            let revision = case["revision"].as_str().expect("revision");
            let bound = waypoints::bind_file(
                &record,
                path,
                revision,
                &content_hash,
                validation.as_ref().ok(),
            );
            entry.insert("waypoint".into(), outcome_of(bound.clone()));
            entry.insert(
                "unvalidated".into(),
                outcome_of(waypoints::bind_file(
                    &record,
                    path,
                    revision,
                    &content_hash,
                    None,
                )),
            );
            if revision == "rev-main"
                && !children.contains_key(path)
                && let Ok(waypoint) = bound
            {
                children.insert(path.to_owned(), waypoint);
            }
        }
        mismatches.check(
            &format!("waypoints/files/{id}"),
            &Value::Object(entry),
            &wanted["files"][id],
        );
    }
    for area in spec["areas"].as_array().expect("areas") {
        let area = area.as_str().expect("area");
        let members: Vec<Value> = children
            .iter()
            .filter(|(path, _)| area == "." || path.starts_with(&format!("{area}/")))
            .map(|(_, child)| child.clone())
            .collect();
        mismatches.check(
            &format!("waypoints/areas/{area}"),
            &outcome_of(waypoints::build_area(
                area,
                &Value::Array(members),
                "rev-main",
            )),
            &wanted["areas"][area],
        );
    }
    let (record, content_hash) = sample.expect("at least one case produced facts");
    for path in spec["invalidPaths"].as_array().expect("paths") {
        let path = path.as_str().expect("path");
        mismatches.check(
            &format!("waypoints/invalidPaths/{path:?}"),
            &outcome_of(waypoints::bind_file(
                &record,
                path,
                "rev-main",
                &content_hash,
                None,
            )),
            &wanted["invalidPaths"][path],
        );
    }
    mismatches.check(
        "waypoints/wrongContent",
        &outcome_of(waypoints::bind_file(
            &record,
            "src/a.ts",
            "rev-main",
            &identity("different"),
            None,
        )),
        &wanted["wrongContent"],
    );
    mismatches.finish("waypoints");
}

/// `ast.walk` over the parity inputs: the same node types and lines, in the same order.
fn walk_outcome(source: &str) -> Value {
    let walked = pyast::with_module(source, |module| {
        module
            .walk()
            .map(|node| {
                let visit = module.visit(node);
                json!([visit.kind, visit.line])
            })
            .collect::<Vec<Value>>()
    });
    match walked {
        Err(ParseFailure::Syntax(_)) => json!({"error": "SyntaxError"}),
        Err(ParseFailure::Recursion) => json!({"error": "RecursionError"}),
        Ok(nodes) if nodes.len() <= 400 => {
            json!({"value": {"count": nodes.len(), "walk": nodes}})
        }
        Ok(nodes) => {
            let compact = serde_json::to_string(&nodes).expect("serialises");
            json!({"value": {"count": nodes.len(), "sha256": sha(compact.as_bytes())}})
        }
    }
}

#[test]
fn ast_walk_matches_python() {
    let files = sources();
    let wanted = expected("ast-walk");
    let mut mismatches = Mismatches::default();
    let mut covered = 0;
    for name in names_in(&files, &["python", "snippet"]) {
        covered += 1;
        let source = text::decode_replace(&files[name]);
        mismatches.check(
            &format!("ast-walk/{name}"),
            &walk_outcome(&source),
            &wanted[name],
        );
    }
    assert_eq!(covered, wanted.as_object().expect("object").len());
    mismatches.finish("ast-walk");
}

fn string_list(items: &[&str]) -> Value {
    json!(items)
}

#[test]
fn python_string_behaviour_matches() {
    let wanted = expected("text");
    let mut mismatches = Mismatches::default();
    for sample in wanted["samples"].as_array().expect("samples") {
        let input = sample["input"].as_str().expect("input");
        let actual = json!({
            "casefold": text::casefold(input),
            "capitalize": text::capitalize(input),
            "split": string_list(&text::split_words(input)),
            "splitlines": string_list(&text::splitlines(input)),
            "strip": text::strip(input),
            "lstrip": text::lstrip(input),
            "rstrip": text::rstrip(input),
            "isidentifier": text::is_identifier(input),
            "expandtabs": text::expandtabs(input),
            "length": text::char_len(input),
        });
        let wanted_fields: Value = Value::Object(
            sample
                .as_object()
                .expect("object")
                .iter()
                .filter(|(key, _)| key.as_str() != "input")
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        );
        mismatches.check(&format!("text/{input:?}"), &actual, &wanted_fields);
    }
    for case in wanted["cleandoc"].as_array().expect("cleandoc") {
        let input = case["input"].as_str().expect("input");
        mismatches.check(
            &format!("cleandoc/{input:?}"),
            &json!(pyast::cleandoc(input)),
            &case["result"],
        );
    }
    mismatches.finish("text");
}

#[test]
fn unicode_tables_match_cpython_for_every_scalar_value() {
    let wanted = expected("text");
    assert_eq!(wanted["unicode"], codegraph_engine::UNICODE_VERSION);
    let scalars: Vec<char> = (0..0x11_0000u32).filter_map(char::from_u32).collect();
    let joined = |function: &dyn Fn(char) -> String| -> String {
        sha(scalars
            .iter()
            .map(|c| function(*c))
            .collect::<Vec<_>>()
            .join("\0")
            .as_bytes())
    };
    let flags = |function: &dyn Fn(char) -> bool| -> String {
        sha(&scalars
            .iter()
            .map(|c| u8::from(function(*c)))
            .collect::<Vec<_>>())
    };
    let actual = json!({
        "casefold": joined(&|c| text::casefold(&c.to_string())),
        "lower": joined(&|c| text::lower_char(c)),
        "upper": joined(&|c| text::upper_char(c)),
        "capitalize": joined(&|c| text::capitalize(&c.to_string())),
        "isspace": flags(&text::is_space),
        "word": flags(&text::is_word),
        "other": flags(&text::is_other),
        "assigned": flags(&text::is_assigned),
        "identifier_start": flags(&text::is_id_start),
        "identifier_continue": flags(&text::is_id_continue),
    });
    let mut mismatches = Mismatches::default();
    mismatches.check("text/digests", &actual, &wanted["digests"]);
    mismatches.finish("unicode tables");
}
