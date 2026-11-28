//! `architecture_patterns`: the catalogue, structural signals and pattern verdicts.

use super::{approvals, outcome, parse, shared_inputs, shrink};
use crate::Mismatches;
use ropeline_engine::architecture::PyError;
use ropeline_engine::architecture::patterns::{
    self, catalogue, match_signatures, recognize, structural_signals,
};
use ropeline_engine::architecture::recognition::{discover_subsystems, structural_graph};
use ropeline_engine::architecture::topology::topology;
use serde_json::{Map, Value, json};

fn cases() -> Value {
    parse(include_str!("patterns.json"))
}

fn wanted() -> Value {
    parse(include_str!("../expected/architecture-patterns.json"))
}

#[test]
fn patterns_match_python() {
    let wanted = wanted();
    let cases = cases();
    let (graphs, contracts) = shared_inputs();
    let mut mismatches = Mismatches::default();
    mismatches.check("patterns/catalogue", &catalogue(), &wanted["catalogue"]);
    // Structure, topology and scopes of one project, as the analysis computes them.
    let analysed = |run: &Value| -> Result<(Value, Value, Value, Vec<Value>), PyError> {
        let graph = &graphs[run["project"].as_str().expect("project")];
        let contract = run["contract"].as_str().map(|name| &contracts[name]);
        let approved = contract
            .map(|found| approvals(found, run["approved"].as_str().expect("mode")))
            .unwrap_or_default();
        let structure = structural_graph(graph)?;
        let deployment = topology(graph)?;
        let scopes = discover_subsystems(graph, contract, &approved)?;
        Ok((
            graph.clone(),
            structure,
            deployment,
            scopes["subsystems"].as_array().expect("scopes").clone(),
        ))
    };
    for run in cases["runs"].as_array().expect("runs") {
        let key = format!(
            "{}|{}|{}",
            run["project"].as_str().expect("project"),
            run["contract"].as_str().unwrap_or("None"),
            run["approved"].as_str().expect("mode"),
        );
        let verdicts = |with_topology: bool| {
            analysed(run).and_then(|(graph, structure, deployment, scopes)| {
                recognize(
                    &graph,
                    &scopes,
                    &structure,
                    with_topology.then_some(&deployment),
                )
                .map(Value::Array)
            })
        };
        mismatches.check(
            &format!("patterns/recognize/{key}"),
            &shrink(outcome(verdicts(true))),
            &wanted["recognize"][&key],
        );
        mismatches.check(
            &format!("patterns/recognizeBare/{key}"),
            &shrink(outcome(verdicts(false))),
            &wanted["recognizeBare"][&key],
        );
        let signals = analysed(run).and_then(|(graph, structure, deployment, scopes)| {
            let mut merged: Map<String, Value> =
                deployment.as_object().cloned().unwrap_or_default();
            merged.insert("subsystems".into(), Value::Array(scopes.clone()));
            let merged = Value::Object(merged);
            scopes
                .iter()
                .map(|scope| structural_signals(&graph, scope, &structure, Some(&merged), None))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array)
        });
        mismatches.check(
            &format!("patterns/signals/{key}"),
            &shrink(outcome(signals)),
            &wanted["signals"][&key],
        );
    }
    let evidence = &cases["evidence"];
    for item in cases["signalSets"].as_array().expect("sets") {
        let id = item["id"].as_str().expect("id");
        let signals: Map<String, Value> = item["keys"]
            .as_array()
            .expect("keys")
            .iter()
            .map(|name| (name.as_str().expect("name").to_owned(), json!([evidence])))
            .collect();
        let actual = match_signatures(
            &Value::Object(signals),
            "scope-1",
            item["partial"].as_bool().expect("partial"),
        );
        mismatches.check(
            &format!("patterns/matches/{id}"),
            &Value::Array(actual),
            &wanted["matches"][id],
        );
    }
    mismatches.check(
        "patterns/constants",
        &json!({"VERSION": patterns::VERSION}),
        &wanted["constants"],
    );
    mismatches.finish("patterns");
}
