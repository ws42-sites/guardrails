//! `architecture_ci`: shell splitting, quality commands, YAML documents and CI facts.

use super::{outcome, parse, shrink};
use crate::Mismatches;
use codegraph_engine::architecture::ci::{ci_facts, quality_commands};
use codegraph_engine::architecture::yaml::{Node, configuration_documents};
use codegraph_engine::py_shlex;
use serde_json::{Value, json};

fn cases() -> Value {
    parse(include_str!("ci.json"))
}

fn wanted() -> Value {
    parse(include_str!("../expected/architecture-ci.json"))
}

#[test]
fn shell_text_matches_python() {
    let wanted = wanted();
    let mut mismatches = Mismatches::default();
    for (index, line) in cases()["shell"]
        .as_array()
        .expect("lines")
        .iter()
        .enumerate()
    {
        let line = line.as_str().expect("text");
        mismatches.check(
            &format!("ci/quality/{index} {:.60?}", line),
            &json!({"value": quality_commands(line)}),
            &wanted["quality"][index],
        );
        let tokens = match py_shlex::split(line) {
            Ok(tokens) => json!({"value": tokens}),
            Err(error) => json!({"error": "ValueError", "message": error.0}),
        };
        mismatches.check(
            &format!("ci/shlex/{index} {:.60?}", line),
            &tokens,
            &wanted["shlex"][index],
        );
    }
    mismatches.finish("shell");
}

fn tree(node: &Node) -> Value {
    match node {
        Node::Scalar { value, line } => json!({"k": "s", "l": line, "v": value}),
        Node::Sequence { items, line } => {
            json!({"k": "q", "l": line, "v": items.iter().map(tree).collect::<Vec<_>>()})
        }
        Node::Mapping { pairs, line } => json!({
            "k": "m",
            "l": line,
            "v": pairs.iter().map(|(key, value)| json!([tree(key), tree(value)])).collect::<Vec<_>>(),
        }),
    }
}

#[test]
fn yaml_documents_match_python() {
    let wanted = wanted();
    let mut mismatches = Mismatches::default();
    for case in cases()["yaml"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("id");
        let actual = match configuration_documents(case["source"].as_str().expect("source")) {
            Ok(roots) => {
                let trees = Value::Array(roots.iter().map(tree).collect());
                json!({"value": codegraph_engine::text::python_json(&trees)})
            }
            Err(_) => json!({"error": "ConfigurationError"}),
        };
        mismatches.check(
            &format!("ci/yaml/{id}"),
            &shrink(actual),
            &wanted["yaml"][id],
        );
    }
    mismatches.finish("yaml");
}

#[test]
fn ci_facts_match_python() {
    let wanted = wanted();
    let mut mismatches = Mismatches::default();
    for case in cases()["facts"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("id");
        let actual = shrink(outcome(Ok(ci_facts(
            case["source"].as_str().expect("source"),
        ))));
        mismatches.check(&format!("ci/facts/{id}"), &actual, &wanted["facts"][id]);
    }
    mismatches.finish("ci facts");
}
