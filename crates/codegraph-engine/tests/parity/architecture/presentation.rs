//! `architecture_presentation`: scopes a browser can join to node ids, and list protection.

use super::{build_graph, outcome, parse, shared_inputs, shrink};
use crate::Mismatches;
use codegraph_engine::architecture::PyError;
use codegraph_engine::architecture::contract::{ContractInput, contract_digest, parse_contract};
use codegraph_engine::architecture::presentation::{
    self, build_presentation, describe_protected, protected_response_lists,
};
use codegraph_engine::architecture::recognition::discover_subsystems;
use codegraph_engine::architecture::tree::J;
use serde_json::{Value, json};
use std::collections::BTreeSet;

fn cases() -> Value {
    parse(include_str!("presentation.json"))
}

fn wanted() -> Value {
    parse(include_str!("../expected/architecture-presentation.json"))
}

#[test]
fn presentation_matches_python() {
    let wanted = wanted();
    let cases = cases();
    let (mut graphs, mut contracts) = shared_inputs();
    graphs.insert("big".into(), build_graph(&cases["bigProject"]));
    contracts.insert(
        "big".into(),
        parse_contract(ContractInput::Value(&cases["bigContract"])).expect("valid"),
    );
    let mut mismatches = Mismatches::default();
    for item in cases["cases"].as_array().expect("cases") {
        let id = item["id"].as_str().expect("id");
        let built = || -> Result<Value, PyError> {
            let mut graph = graphs[item["project"].as_str().expect("project")].clone();
            for key in ["snapshot", "view_id", "revision", "repository"] {
                if let Some(value) = item.get(key) {
                    graph[key] = value.clone();
                }
            }
            let contract = &contracts[item["contract"].as_str().expect("contract")];
            let approved: BTreeSet<String> = if item["approved"] == "contract" {
                [contract_digest(contract)].into()
            } else {
                BTreeSet::new()
            };
            let found = discover_subsystems(&graph, Some(contract), &approved)?;
            let errors: Vec<String> = (0..item["errors"].as_u64().unwrap_or(0))
                .map(|index| format!("parse/errors/{}/{index:03}.py", "d".repeat(110)))
                .collect();
            let subsystems: Vec<Value> = found["subsystems"]
                .as_array()
                .expect("scopes")
                .iter()
                .map(|scope| {
                    let mut kept = scope.as_object().cloned().expect("scope");
                    kept.retain(|key, _| !key.starts_with('_'));
                    Value::Object(kept)
                })
                .collect();
            let report = json!({
                "engine_version": codegraph_engine::architecture::contract::ENGINE_VERSION,
                "base_revision": item.get("base_revision").cloned().unwrap_or(Value::Null),
                "policy_digest": contract_digest(contract),
                "policy_approved": item["approved"] == "contract",
                "enforced": item.get("enforced").cloned().unwrap_or(json!(false)),
                "summary": {"findings": 3},
                "findings": [{"id": "f1"}, {"id": "f2"}],
                "fingerprint": {
                    "subsystems": subsystems,
                    "subsystem_count": found["scope_count"],
                    "partial": item.get("partial").cloned().unwrap_or(json!(false)),
                    "recognition_partial": found["partial"],
                    "metrics": {"files": graph["files"].as_array().expect("files").len()},
                    "coverage": {"parse_errors": errors, "parse_error_count": errors.len()},
                },
            });
            build_presentation(&graph, &report, contract)
        };
        mismatches.check(
            &format!("presentation/build/{id}"),
            &shrink(outcome(built())),
            &wanted["build"][id],
        );
    }
    for item in cases["packets"].as_array().expect("packets") {
        let id = item["id"].as_str().expect("id");
        for budget in cases["budgets"].as_array().expect("budgets") {
            let budget = budget.as_i64().expect("budget");
            let packet = J::from_value(&item["packet"]).deep_copy();
            let fingerprint = packet.get("fingerprint").unwrap_or_else(|| packet.clone());
            let mut scopes: Vec<J> = Vec::new();
            for key in [
                fingerprint.get("subsystems"),
                packet.get("relevant_subsystems"),
            ]
            .into_iter()
            .flatten()
            {
                if let Some(list) = key.as_list() {
                    scopes.extend(list.borrow().iter().cloned());
                }
            }
            let protected = protected_response_lists(&packet, &scopes, budget);
            let actual = json!({
                "value": {
                    "protected": describe_protected(&packet, &protected),
                    "packet": packet.to_value(),
                }
            });
            mismatches.check(
                &format!("presentation/protected/{id}|{budget}"),
                &actual,
                &wanted["protected"][&format!("{id}|{budget}")],
            );
        }
    }
    mismatches.check(
        "presentation/constants",
        &json!({
            "PRESENTATION_VERSION": presentation::PRESENTATION_VERSION,
            "PRESENTATION_LIMIT": presentation::PRESENTATION_LIMIT,
        }),
        &wanted["constants"],
    );
    mismatches.finish("presentation");
}
