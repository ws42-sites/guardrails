//! `architecture_recognition`: structural graph, subsystems and their comparison.

use super::{approvals, outcome, parse, shared_inputs, shrink};
use crate::Mismatches;
use ropeline_engine::architecture::recognition::{
    self, compare_recognition, discover_subsystems, structural_graph,
};
use serde_json::{Value, json};

fn cases() -> Value {
    parse(include_str!("recognition.json"))
}

fn wanted() -> Value {
    parse(include_str!("../expected/architecture-recognition.json"))
}

#[test]
fn recognition_matches_python() {
    let wanted = wanted();
    let cases = cases();
    let (graphs, contracts) = shared_inputs();
    let mut mismatches = Mismatches::default();
    for project in cases["structural"].as_array().expect("projects") {
        let id = project.as_str().expect("id");
        mismatches.check(
            &format!("recognition/structural/{id}"),
            &shrink(outcome(structural_graph(&graphs[id]))),
            &wanted["structural"][id],
        );
    }
    let subsystems = |project: &str, contract: &Value, mode: &str| {
        let chosen = contract.as_str().map(|name| &contracts[name]);
        let approved = chosen
            .map(|found| approvals(found, mode))
            .unwrap_or_default();
        discover_subsystems(&graphs[project], chosen, &approved)
    };
    for item in cases["subsystems"].as_array().expect("subsystems") {
        let (project, contract, mode) = (
            item["project"].as_str().expect("project"),
            &item["contract"],
            item["approved"].as_str().expect("mode"),
        );
        let key = format!("{project}|{}|{mode}", contract.as_str().unwrap_or("None"));
        mismatches.check(
            &format!("recognition/subsystems/{key}"),
            &shrink(outcome(subsystems(project, contract, mode))),
            &wanted["subsystems"][&key],
        );
    }
    for (index, item) in cases["compares"]
        .as_array()
        .expect("compares")
        .iter()
        .enumerate()
    {
        let side = |name: &str| {
            subsystems(
                item[name][0].as_str().expect("project"),
                &item[name][1],
                "contract",
            )
        };
        let actual = outcome(side("before").and_then(|before| {
            side("after").and_then(|after| compare_recognition(&before, &after))
        }));
        mismatches.check(
            &format!("recognition/compares/{index}"),
            &actual,
            &wanted["compares"][index],
        );
    }
    for item in cases["rawCompares"].as_array().expect("raw") {
        let id = item["id"].as_str().expect("id");
        mismatches.check(
            &format!("recognition/rawCompares/{id}"),
            &outcome(compare_recognition(&item["before"], &item["after"])),
            &wanted["rawCompares"][id],
        );
    }
    mismatches.check(
        "recognition/constants",
        &json!({"VERSION": recognition::VERSION, "SCOPE_LIMIT": recognition::SCOPE_LIMIT}),
        &wanted["constants"],
    );
    mismatches.finish("recognition");
}
