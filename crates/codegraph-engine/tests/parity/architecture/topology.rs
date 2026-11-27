//! `architecture_topology`: deployment facts, the snapshot topology and its comparison.

use super::{outcome, parse, shrink};
use crate::Mismatches;
use codegraph_engine::architecture::topology::{
    self, compare_topology, deployment_facts, topology,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

fn cases() -> Value {
    parse(include_str!("topology.json"))
}

fn wanted() -> Value {
    parse(include_str!("../expected/architecture-topology.json"))
}

/// A snapshot whose files carry the facts extraction gives them, unless the case sets them.
fn graph(case: &Value) -> Value {
    let files: Vec<Value> = case["files"]
        .as_array()
        .expect("files")
        .iter()
        .map(|item| {
            let facts = if item.get("facts").is_some() {
                item["facts"].clone()
            } else {
                let source = item["source"].as_str().expect("source");
                json!({
                    "sha256": format!("{:x}", Sha256::digest(source.as_bytes())),
                    "deployment": deployment_facts(source),
                })
            };
            json!({"id": item["id"], "path": item["path"], "facts": facts, "metadata": {}})
        })
        .collect();
    json!({"repository": case["repository"], "revision": case["revision"], "files": files})
}

#[test]
fn deployment_facts_match_python() {
    let wanted = wanted();
    let mut mismatches = Mismatches::default();
    for case in cases()["facts"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("id");
        let actual = shrink(outcome(Ok(deployment_facts(
            case["source"].as_str().expect("source"),
        ))));
        mismatches.check(
            &format!("topology/facts/{id}"),
            &actual,
            &wanted["facts"][id],
        );
    }
    mismatches.finish("deployment facts");
}

#[test]
fn topology_and_comparison_match_python() {
    let wanted = wanted();
    let cases = cases();
    let mut mismatches = Mismatches::default();
    let mut graphs: BTreeMap<String, Value> = BTreeMap::new();
    for case in cases["graphs"].as_array().expect("graphs") {
        let id = case["id"].as_str().expect("id");
        let snapshot = graph(case);
        mismatches.check(
            &format!("topology/graphs/{id}"),
            &shrink(outcome(topology(&snapshot))),
            &wanted["graphs"][id],
        );
        graphs.insert(id.to_owned(), snapshot);
    }
    for case in cases["compares"].as_array().expect("compares") {
        let id = case["id"].as_str().expect("id");
        let side = |key: &str| topology(&graphs[case[key].as_str().expect("name")]);
        let actual =
            outcome(side("before").and_then(|before| {
                side("after").and_then(|after| compare_topology(&before, &after))
            }));
        mismatches.check(
            &format!("topology/compares/{id}"),
            &actual,
            &wanted["compares"][id],
        );
    }
    for case in cases["rawCompares"].as_array().expect("raw") {
        let id = case["id"].as_str().expect("id");
        let actual = outcome(compare_topology(&case["before"], &case["after"]));
        mismatches.check(
            &format!("topology/rawCompares/{id}"),
            &actual,
            &wanted["rawCompares"][id],
        );
    }
    mismatches.check(
        "topology/constants",
        &json!({"VERSION": topology::VERSION}),
        &wanted["constants"],
    );
    mismatches.finish("topology");
}
