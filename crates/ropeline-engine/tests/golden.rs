//! The engine-level artifacts in `tests/golden/expected/<repo>/`, reproduced by Rust.
//!
//! Inputs are the committed fixture manifests (`tests/golden/repos`). The recorded
//! `files.json` supplies which files the indexer analysed and in which language, because
//! that classification lives in `core.py` and the workspace code, which are later phases.
//!
//! Python, JavaScript, TypeScript and Rust fixtures are all covered.
//!
//! Reproduced exactly: `symbols`, `references`, `relationships`, `codeowners`, the
//! size, hash and category of every `files` row, and every waypoint field except the
//! validation fingerprint and the dependency count (both come from the graph store).
//! Not reproduced: `files.language` and `files.status` (core.py rules), the waypoint
//! `overview` (core.overview budgeting), and everything that needs SQLite, Git, the
//! explorer or the MCP tools (`explorer-*`, `architecture-report`, `mcp-*`).

use base64::Engine as _;
use ropeline_engine::architecture::extract::extract_facts;
use ropeline_engine::codeowners::{self, CANDIDATES};
use ropeline_engine::file_categories::file_category;
use ropeline_engine::{parsing, pyast, relationships, text, waypoints};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

fn json_of(raw: &str) -> Value {
    serde_json::from_str(raw).expect("valid JSON")
}

struct Fixture {
    name: &'static str,
    manifest: Value,
    files: Vec<Value>,
    symbols: Value,
    references: Value,
    relationships: Value,
    codeowners: Value,
    waypoints: Value,
}

macro_rules! fixture {
    ($name:literal) => {
        Fixture {
            name: $name,
            manifest: json_of(include_str!(concat!(
                "../../../tests/golden/repos/",
                $name,
                ".json"
            ))),
            files: json_of(include_str!(concat!(
                "../../../tests/golden/expected/",
                $name,
                "/files.json"
            )))
            .as_array()
            .expect("rows")
            .clone(),
            symbols: json_of(include_str!(concat!(
                "../../../tests/golden/expected/",
                $name,
                "/symbols.json"
            ))),
            references: json_of(include_str!(concat!(
                "../../../tests/golden/expected/",
                $name,
                "/references.json"
            ))),
            relationships: json_of(include_str!(concat!(
                "../../../tests/golden/expected/",
                $name,
                "/relationships.json"
            ))),
            codeowners: json_of(include_str!(concat!(
                "../../../tests/golden/expected/",
                $name,
                "/codeowners.json"
            ))),
            waypoints: json_of(include_str!(concat!(
                "../../../tests/golden/expected/",
                $name,
                "/waypoints.json"
            ))),
        }
    };
}

fn fixtures() -> Vec<Fixture> {
    vec![
        fixture!("polyglot-app"),
        fixture!("owners-architecture"),
        fixture!("unsupported-cases"),
        fixture!("rust-crate"),
    ]
}

impl Fixture {
    fn content(&self, path: &str) -> Vec<u8> {
        match &self.manifest["files"][path] {
            Value::String(text) => text.clone().into_bytes(),
            other => base64::engine::general_purpose::STANDARD
                .decode(other["base64"].as_str().expect("base64"))
                .expect("valid base64"),
        }
    }

    /// Paths the indexer analysed, with their language, in recorded order.
    fn indexed(&self) -> Vec<(String, String)> {
        self.files
            .iter()
            .filter(|row| row["status"] == "indexed")
            .map(|row| {
                (
                    row["path"].as_str().expect("path").to_owned(),
                    row["language"].as_str().expect("language").to_owned(),
                )
            })
            .collect()
    }
}

fn sha(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

fn assert_same(context: &str, actual: &Value, wanted: &Value) {
    assert_eq!(actual, wanted, "{context}");
}

#[test]
fn files_rows_match_for_size_hash_and_category() {
    for fixture in fixtures() {
        for row in &fixture.files {
            let path = row["path"].as_str().expect("path");
            let data = fixture.content(path);
            let actual = json!({
                "bytes": data.len(),
                "sha256": sha(&data),
                "category": file_category(Some(path), "file"),
            });
            let wanted = json!({
                "bytes": row["bytes"], "sha256": row["sha256"], "category": row["category"],
            });
            assert_same(&format!("{}: files/{path}", fixture.name), &actual, &wanted);
        }
    }
}

#[test]
fn symbols_and_references_match() {
    for fixture in fixtures() {
        let mut symbols = Map::new();
        let mut references = Map::new();
        for (path, language) in fixture.indexed() {
            let data = fixture.content(&path);
            symbols.insert(
                path.clone(),
                serde_json::to_value(parsing::symbols(&language, &data).expect("symbols"))
                    .expect("serialises"),
            );
            let source = String::from_utf8(data).expect("fixtures are UTF-8");
            let found = relationships::references(Some(&language), &source).expect("references");
            references.insert(
                path,
                json!({
                    "imports": found.imports,
                    "calls": found.calls.iter()
                        .map(|c| json!({"name": c.0, "line": c.1, "confidence": c.2}))
                        .collect::<Vec<_>>(),
                }),
            );
        }
        assert_same(
            &format!("{}: symbols", fixture.name),
            &Value::Object(symbols),
            &fixture.symbols,
        );
        assert_same(
            &format!("{}: references", fixture.name),
            &Value::Object(references),
            &fixture.references,
        );
    }
}

/// The `exports` alias facts `architecture_extract` stores for a `package.json`.
fn manifest_facts(data: &[u8]) -> Option<Value> {
    let config: Value = serde_json::from_slice(data).ok()?;
    let object = config.as_object()?;
    let exports: Map<String, Value> = match object.get("exports").and_then(Value::as_object) {
        Some(found) => found
            .iter()
            .map(|(key, value)| {
                let kept = value.as_str().filter(|text| text.len() <= 4096);
                (key.clone(), kept.map_or(Value::Null, |text| json!(text)))
            })
            .collect(),
        None => Map::new(),
    };
    Some(
        json!({"architecture": {"manifest": {"name": object.get("name").filter(|n| n.is_string()), "exports": exports}}}),
    )
}

/// The `[package]` name facts `architecture_extract` stores for a `Cargo.toml`.
fn cargo_facts(data: &[u8]) -> Value {
    let facts = extract_facts("text", data, &json!({"imports": []})).expect("manifest facts");
    json!({"architecture": {"manifest": facts["manifest"]}})
}

#[test]
fn relationships_match() {
    for fixture in fixtures() {
        let indexed = fixture.indexed();
        let known: BTreeMap<String, ()> =
            indexed.iter().map(|(path, _)| (path.clone(), ())).collect();
        let manifests: Vec<(String, Value)> = indexed
            .iter()
            .map(|(path, _)| {
                let facts = if path.ends_with("package.json") {
                    manifest_facts(&fixture.content(path)).unwrap_or_else(|| json!({}))
                } else if path.ends_with("Cargo.toml") {
                    cargo_facts(&fixture.content(path))
                } else {
                    json!({})
                };
                (path.clone(), facts)
            })
            .collect();
        let aliases = relationships::module_aliases(&manifests).expect("aliases");
        let crates = relationships::crate_roots(&manifests, &|path| known.contains_key(path))
            .expect("crates");
        let mut actual = Map::new();
        for (path, language) in &indexed {
            let source = String::from_utf8(fixture.content(path)).expect("UTF-8");
            let facts = relationships::references(Some(language), &source).expect("references");
            let (imports, calls) =
                relationships::resolve(path, &facts, &known, Some(&aliases), Some(&crates));
            let symbols = parsing::symbols(language, source.as_bytes()).expect("symbols");
            let mut by_name: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
            for (ordinal, symbol) in symbols.iter().enumerate() {
                if symbol.kind == "function_definition" {
                    by_name
                        .entry(symbol.name.as_str())
                        .or_default()
                        .push(ordinal);
                }
            }
            // Only a name with exactly one definition in the file becomes an edge.
            let defined = |name: &str| by_name.get(name).map_or(0, Vec::len);
            actual.insert(
                path.clone(),
                json!({
                    "imports": imports.iter()
                        .map(|i| json!({"target": i.0, "line": i.1, "confidence": i.2}))
                        .collect::<Vec<_>>(),
                    "possible_calls": calls.iter().filter(|c| defined(&c.0) == 1)
                        .map(|c| json!({"name": c.0, "line": c.1, "confidence": c.2,
                            "target_symbol_index": by_name[c.0.as_str()][0]}))
                        .collect::<Vec<_>>(),
                    "dropped_calls": calls.iter().filter(|c| defined(&c.0) != 1)
                        .map(|c| json!({"name": c.0, "line": c.1, "reason": "not exactly one definition"}))
                        .collect::<Vec<_>>(),
                }),
            );
        }
        assert_same(
            &format!("{}: relationships", fixture.name),
            &Value::Object(actual),
            &fixture.relationships,
        );
    }
}

#[test]
fn codeowners_match() {
    for fixture in fixtures() {
        let listed: Vec<&str> = fixture
            .files
            .iter()
            .filter_map(|row| row["path"].as_str())
            .collect();
        let found = CANDIDATES
            .iter()
            .find(|candidate| listed.contains(candidate))
            .and_then(|candidate| codeowners::parse(&fixture.content(candidate), candidate));
        let actual = match found {
            None => json!({"present": false, "rules": [], "invalid_lines": [], "owners": {}}),
            Some(ownership) => {
                let content = String::from_utf8(fixture.content(&ownership.path)).expect("UTF-8");
                let kept: Vec<i64> = ownership.rules.iter().map(|rule| rule.line).collect();
                let invalid: Vec<Value> = content
                    .split('\n')
                    .enumerate()
                    .filter(|(index, line)| {
                        !text::strip(line).is_empty()
                            && !text::lstrip(line).starts_with('#')
                            && !kept.contains(&(*index as i64 + 1))
                    })
                    .map(|(index, line)| json!({"line": index + 1, "text": line}))
                    .collect();
                let owners: Map<String, Value> = listed
                    .iter()
                    .map(|path| {
                        let value = ownership.lookup(path).map_or(
                            Value::Null,
                            |(owners, line)| json!({"owners": owners, "rule_line": line}),
                        );
                        ((*path).to_owned(), value)
                    })
                    .collect();
                json!({
                    "present": true, "path": ownership.path, "sha256": ownership.digest,
                    "rules": ownership.record()["rules"], "invalid_lines": invalid, "owners": owners,
                })
            }
        };
        assert_same(
            &format!("{}: codeowners", fixture.name),
            &actual,
            &fixture.codeowners,
        );
    }
}

/// The recorded validation carries a normalised fingerprint; swap in a valid hash so
/// the record can be fed back through the validators.
fn with_real_fingerprint(waypoint: &Value) -> Value {
    let mut copy = waypoint.clone();
    copy["validation"]["fingerprint"] = json!("0".repeat(64));
    copy
}

fn comparable(waypoint: &Value) -> Value {
    let mut copy = waypoint.clone();
    let validation = copy["validation"].take();
    let checks: Vec<Value> = validation["checks"]
        .as_array()
        .expect("checks")
        .iter()
        .filter(|check| check["name"] != "Dependency context")
        .cloned()
        .collect();
    copy["validation"] = json!({
        "status": validation["status"], "kind": validation["kind"],
        "ruleVersion": validation["ruleVersion"], "checks": checks,
    });
    copy
}

#[test]
fn waypoints_match_apart_from_graph_dependent_validation() {
    for fixture in fixtures() {
        let revision = fixture.waypoints["areas"][0]["evidence"][0]["revision"]
            .as_str()
            .expect("revision")
            .to_owned();
        let zero = "0".repeat(64);
        let mut built: BTreeMap<String, Value> = BTreeMap::new();
        for (path, language) in fixture.indexed() {
            let Some(recorded) = fixture.waypoints["files"].get(&path) else {
                continue;
            };
            let data = fixture.content(&path);
            let symbols =
                waypoints::symbols_as_values(&parsing::symbols(&language, &data).expect("symbols"));
            let evidence = match language.as_str() {
                // Python records a parse error only when the source is rejected.
                "python" => pyast::with_module(&String::from_utf8_lossy(&data), |_| ())
                    .is_err()
                    .then_some(true),
                "typescript" | "tsx" | "javascript" | "rust" => {
                    Some(parsing::has_error(&language, &data).expect("parse"))
                }
                _ => None,
            };
            let facts_input = evidence.map_or(
                json!({}),
                |flag| json!({"architecture": {"parse_error": flag}}),
            );
            let facts = waypoints::build_file_facts(&language, &data, &symbols, &facts_input)
                .expect("facts");
            let hash = sha(&data);
            let validation =
                waypoints::validate_file(&facts, &json!([]), &zero, &zero).expect("validation");
            let bound = waypoints::bind_file(&facts, &path, &revision, &hash, Some(&validation))
                .expect("bound");
            assert_same(
                &format!("{}: waypoint {path}", fixture.name),
                &comparable(&bound),
                &comparable(recorded),
            );
            // Children of an area use the recorded status, which depends on the graph.
            built.insert(path, with_real_fingerprint(recorded));
        }
        let mut areas: Vec<&Value> = fixture.waypoints["areas"]
            .as_array()
            .expect("areas")
            .iter()
            .collect();
        areas.sort_by_key(|area| {
            std::cmp::Reverse(area["path"].as_str().unwrap_or("").matches('/').count())
        });
        areas.sort_by_key(|area| {
            let path = area["path"].as_str().unwrap_or("");
            std::cmp::Reverse(if path == "." {
                0
            } else {
                path.split('/').count()
            })
        });
        let mut made: BTreeMap<String, Value> = BTreeMap::new();
        for area in areas {
            let path = area["path"].as_str().expect("path");
            let parent_of = |child: &str| {
                child
                    .rsplit_once('/')
                    .map_or(".", |(parent, _)| parent)
                    .to_owned()
            };
            let members: Vec<Value> = built
                .iter()
                .chain(made.iter())
                .filter(|(child, _)| child.as_str() != path && parent_of(child) == path)
                .map(|(_, value)| value.clone())
                .collect();
            let area_built =
                waypoints::build_area(path, &Value::Array(members), &revision).expect("area");
            assert_same(
                &format!("{}: area {path}", fixture.name),
                &comparable(&area_built),
                &comparable(area),
            );
            made.insert(path.to_owned(), with_real_fingerprint(&area_built));
        }
    }
}
