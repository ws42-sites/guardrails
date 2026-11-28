//! `architecture_rules`: glob matching, the import graph, components, rules and findings.

use super::{outcome, parse, shrink};
use crate::Mismatches;
use ropeline_engine::architecture::PyError;
use ropeline_engine::architecture::contract::{Contract, ContractInput, parse_contract};
use ropeline_engine::architecture::rules::{
    FindingBuilder, ImportGraph, Options, components, effective_rules, import_graph, matches,
    violations,
};
use ropeline_engine::py_fnmatch::fnmatchcase;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

fn cases() -> Value {
    parse(include_str!("rules.json"))
}

fn wanted() -> Value {
    parse(include_str!("../expected/architecture-rules.json"))
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("list")
        .iter()
        .map(|item| item.as_str().expect("text").to_owned())
        .collect()
}

#[test]
fn glob_matching_matches_python() {
    let wanted = wanted();
    let cases = cases();
    let mut mismatches = Mismatches::default();
    let spec = &cases["fnmatch"][0];
    let names = strings(&spec["names"]);
    for (index, pattern) in strings(&spec["patterns"]).iter().enumerate() {
        let actual: String = names
            .iter()
            .map(|name| if fnmatchcase(name, pattern) { '1' } else { '0' })
            .collect();
        mismatches.check(
            &format!("rules/fnmatch/{pattern:?}"),
            &json!(actual),
            &wanted["fnmatch"][index],
        );
    }
    for (index, item) in cases["matches"]
        .as_array()
        .expect("cases")
        .iter()
        .enumerate()
    {
        let actual = matches(
            item["path"].as_str().expect("path"),
            &strings(&item["patterns"]),
        );
        mismatches.check(
            &format!("rules/matches/{index} {item}"),
            &json!(actual),
            &wanted["matches"][index],
        );
    }
    mismatches.finish("glob matching");
}

fn view(graph: &ImportGraph) -> Value {
    let adjacency: Map<String, Value> = graph
        .edges
        .iter()
        .map(|(path, targets)| (path.clone(), json!(targets)))
        .collect();
    let mut proofs: Vec<(&(String, String), &Value)> = graph.proofs.iter().collect();
    proofs.sort_by_key(|(key, _)| (*key).clone());
    json!({
        "adjacency": adjacency,
        "order": graph.order,
        "proofs": proofs.iter().map(|((s, t), proof)| json!([s, t, proof])).collect::<Vec<_>>(),
    })
}

#[test]
fn import_graphs_and_components_match_python() {
    let wanted = wanted();
    let mut mismatches = Mismatches::default();
    for item in cases()["graphs"].as_array().expect("graphs") {
        let id = item["id"].as_str().expect("id");
        let graph = &item["graph"];
        for (label, typed) in [("plain", false), ("typed", true)] {
            mismatches.check(
                &format!("rules/importGraph/{id}/{label}"),
                &outcome(import_graph(graph, typed).map(|found| view(&found))),
                &wanted["importGraph"][id][label],
            );
        }
        let actual = outcome(import_graph(graph, false).map(|found| json!(components(&found))));
        mismatches.check(
            &format!("rules/components/{id}"),
            &actual,
            &wanted["components"][id],
        );
    }
    mismatches.finish("import graphs");
}

fn contract_of(value: &Value) -> Contract {
    parse_contract(ContractInput::Value(value)).expect("valid contract")
}

#[test]
fn violations_match_python() {
    let wanted = wanted();
    let cases = cases();
    let graphs: BTreeMap<&str, &Value> = cases["graphs"]
        .as_array()
        .expect("graphs")
        .iter()
        .map(|item| (item["id"].as_str().expect("id"), &item["graph"]))
        .collect();
    let contracts: BTreeMap<&str, Contract> = cases["contracts"]
        .as_array()
        .expect("contracts")
        .iter()
        .map(|item| {
            (
                item["id"].as_str().expect("id"),
                contract_of(&item["value"]),
            )
        })
        .collect();
    let mut mismatches = Mismatches::default();
    for item in cases["pairs"].as_array().expect("pairs") {
        let (graph, contract) = (
            item["graph"].as_str().expect("graph"),
            item["contract"].as_str().expect("contract"),
        );
        let key = format!("{graph}|{contract}");
        let actual = shrink(outcome(
            violations(graphs[graph], &contracts[contract]).map(Value::Array),
        ));
        mismatches.check(
            &format!("rules/violations/{key}"),
            &actual,
            &wanted["violations"][&key],
        );
    }
    for item in cases["effective"].as_array().expect("effective") {
        let contract = item["contract"].as_str().expect("contract");
        let mut actual = Map::new();
        for path in strings(&item["paths"]) {
            let found: Vec<Value> = effective_rules(&contracts[contract], &path)
                .iter()
                .map(|rule| json!([rule.id, rule.scope]))
                .collect();
            actual.insert(path, Value::Array(found));
        }
        mismatches.check(
            &format!("rules/effective/{contract}"),
            &Value::Object(actual),
            &wanted["effective"][contract],
        );
    }
    mismatches.finish("violations");
}

#[test]
fn finding_builder_matches_python() {
    let wanted = wanted();
    let cases = cases();
    let graphs: BTreeMap<&str, &Value> = cases["graphs"]
        .as_array()
        .expect("graphs")
        .iter()
        .map(|item| (item["id"].as_str().expect("id"), &item["graph"]))
        .collect();
    let rules: BTreeMap<&str, Contract> = cases["buildRules"]
        .as_array()
        .expect("rules")
        .iter()
        .map(|item| {
            (
                item["id"].as_str().expect("id"),
                contract_of(&json!({"rules": [item["value"]]})),
            )
        })
        .collect();
    let build = |item: &Value, baseline: Option<&Value>| -> Result<Value, PyError> {
        let builder = FindingBuilder::new(graphs[item["graph"].as_str().expect("graph")])?;
        let options = &item["options"];
        let detail = options["detail"].as_str().unwrap_or("");
        builder.make(
            &rules[item["rule"].as_str().expect("rule")].rules[0],
            &strings(&item["paths"]),
            &Options {
                target: options["target"].as_str().unwrap_or(""),
                line: options["line"].as_i64().unwrap_or(1),
                measure: options.get("measure").cloned().unwrap_or(json!(1)),
                detail,
                baseline,
            },
        )
    };
    let mut mismatches = Mismatches::default();
    for (index, item) in cases["builds"]
        .as_array()
        .expect("builds")
        .iter()
        .enumerate()
    {
        mismatches.check(
            &format!("rules/builds/{index}"),
            &shrink(outcome(build(item, None))),
            &wanted["builds"][index],
        );
    }
    for (index, item) in cases["baselineBuilds"]
        .as_array()
        .expect("builds")
        .iter()
        .enumerate()
    {
        let baseline = graphs[item["baseline"].as_str().expect("baseline")];
        mismatches.check(
            &format!("rules/baselineBuilds/{index}"),
            &shrink(outcome(build(item, Some(baseline)))),
            &wanted["baselineBuilds"][index],
        );
    }
    mismatches.finish("finding builder");
}
