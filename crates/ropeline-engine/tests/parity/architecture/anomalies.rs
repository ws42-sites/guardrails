//! `architecture_anomalies`: peer statistics and competing implementations.

use super::{outcome, parse, shared_inputs, shrink};
use crate::Mismatches;
use ropeline_engine::architecture::anomalies::{self, peer_anomalies};
use ropeline_engine::architecture::recognition::structural_graph;
use ropeline_engine::py_value::format_g;
use serde_json::{Value, json};

fn cases() -> Value {
    parse(include_str!("anomalies.json"))
}

fn wanted() -> Value {
    parse(include_str!("../expected/architecture-anomalies.json"))
}

#[test]
fn anomalies_match_python() {
    let wanted = wanted();
    let (graphs, _) = shared_inputs();
    let mut mismatches = Mismatches::default();
    for (name, graph) in &graphs {
        mismatches.check(
            &format!("anomalies/plain/{name}"),
            &shrink(outcome(peer_anomalies(graph, None))),
            &wanted["plain"][name],
        );
        let projected =
            structural_graph(graph).and_then(|projection| peer_anomalies(graph, Some(&projection)));
        mismatches.check(
            &format!("anomalies/projected/{name}"),
            &shrink(outcome(projected)),
            &wanted["projected"][name],
        );
    }
    for (index, value) in cases()["gFormats"]
        .as_array()
        .expect("values")
        .iter()
        .enumerate()
    {
        mismatches.check(
            &format!("anomalies/gFormats/{index} {value}"),
            &json!(format_g(value.as_f64().expect("number"))),
            &wanted["gFormats"][index],
        );
    }
    mismatches.check(
        "anomalies/constants",
        &json!({"VERSION": anomalies::VERSION}),
        &wanted["constants"],
    );
    mismatches.finish("anomalies");
}
