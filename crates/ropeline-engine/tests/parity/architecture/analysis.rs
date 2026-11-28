//! `architecture_analysis`: fingerprints, checks, context, impact and the response budget.

use super::{build_graph, outcome, parse, shared_inputs, shrink};
use crate::Mismatches;
use ropeline_engine::architecture::PyError;
use ropeline_engine::architecture::analysis::{
    compare_fingerprints, default_contract, discovered_contract, fingerprint, impact,
    select_findings, validate_graph,
};
use ropeline_engine::architecture::analysis_budget::{bounded, context, report_tree};
use ropeline_engine::architecture::analysis_check::check;
use ropeline_engine::architecture::contract::{
    self, Contract, ContractInput, contract_digest, parse_contract,
};
use ropeline_engine::architecture::tree::J;
use ropeline_engine::text::python_json;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

fn cases() -> Value {
    parse(include_str!("analysis.json"))
}

fn wanted() -> Value {
    parse(include_str!("../expected/architecture-analysis.json"))
}

/// Large outputs are recorded as a digest of their canonical JSON, plus a little to read.
fn digested(result: Result<Value, PyError>) -> Value {
    let value = match result {
        Ok(value) => value,
        Err(error) => return error.to_value(),
    };
    let text = python_json(&value);
    let mut shown = json!({
        "digest": format!("{:x}", Sha256::digest(text.as_bytes())),
        "size": text.chars().count(),
    });
    if let Some(object) = value.as_object() {
        let mut keys: Vec<&String> = object.keys().collect();
        keys.sort();
        shown["keys"] = json!(keys);
        shown["partial"] = object.get("partial").cloned().unwrap_or(Value::Null);
        shown["used"] = object.get("budget_used").cloned().unwrap_or(Value::Null);
    }
    json!({"digested": shown})
}

struct Inputs {
    graphs: BTreeMap<String, Value>,
    contracts: BTreeMap<String, Contract>,
    default: Contract,
}

fn inputs(spec: &Value) -> Inputs {
    let (mut graphs, mut contracts) = shared_inputs();
    for item in spec["projects"].as_array().expect("projects") {
        graphs.insert(
            item["id"].as_str().expect("id").to_owned(),
            build_graph(item),
        );
    }
    for item in spec["contracts"].as_array().expect("contracts") {
        contracts.insert(
            item["id"].as_str().expect("id").to_owned(),
            parse_contract(ContractInput::Value(&item["value"])).expect("valid contract"),
        );
    }
    let default =
        parse_contract(ContractInput::Value(&default_contract())).expect("default is valid");
    Inputs {
        graphs,
        contracts,
        default,
    }
}

impl Inputs {
    fn chosen(&self, item: &Value) -> Result<Contract, PyError> {
        if let Some(name) = item["contract"].as_str() {
            return Ok(self.contracts[name].clone());
        }
        let source = item["baseline"]
            .as_str()
            .or(item["candidate"].as_str())
            .expect("graph");
        Ok(discovered_contract(&self.graphs[source])?.unwrap_or_else(|| self.default.clone()))
    }

    fn approved(&self, mode: &str, contract: &Contract) -> BTreeSet<String> {
        match mode {
            "contract" => [contract_digest(contract)].into(),
            "default" => [contract_digest(&self.default)].into(),
            _ => BTreeSet::new(),
        }
    }

    fn run_check(&self, item: &Value) -> Result<Value, PyError> {
        let contract = self.chosen(item)?;
        let baseline = item["baseline"].as_str().map(|name| &self.graphs[name]);
        let explicit = item["contract"].as_str().map(|name| &self.contracts[name]);
        check(
            &self.graphs[item["candidate"].as_str().expect("candidate")],
            baseline,
            explicit,
            &self.approved(item["approved"].as_str().expect("mode"), &contract),
            item["enforce"].as_bool().expect("enforce"),
            item["asOf"].as_str().expect("date"),
        )
    }
}

#[test]
fn checks_and_fingerprints_match_python() {
    let wanted = wanted();
    let cases = cases();
    let all = inputs(&cases);
    let mut mismatches = Mismatches::default();
    for item in cases["checks"].as_array().expect("checks") {
        let id = item["id"].as_str().expect("id");
        mismatches.check(
            &format!("analysis/checks/{id}"),
            &shrink(outcome(all.run_check(item))),
            &wanted["checks"][id],
        );
    }
    let mut runs: Vec<(String, Option<&str>, &str)> = all
        .graphs
        .keys()
        .map(|name| (name.clone(), None, "none"))
        .collect();
    runs.extend([
        ("an-cand-declared".to_owned(), Some("domains"), "contract"),
        ("an-cand-declared".to_owned(), Some("domains"), "none"),
        ("an-ports".to_owned(), Some("boundary"), "contract"),
    ]);
    for (name, contract_id, mode) in &runs {
        let key = format!("{name}|{}|{mode}", contract_id.unwrap_or("None"));
        let contract = contract_id.map(|id| &all.contracts[id]);
        let approved = contract
            .map(|found| all.approved(mode, found))
            .unwrap_or_default();
        mismatches.check(
            &format!("analysis/fingerprints/{key}"),
            &shrink(outcome(fingerprint(&all.graphs[name], &approved, contract))),
            &wanted["fingerprints"][&key],
        );
    }
    for (index, pair) in cases["compares"]
        .as_array()
        .expect("pairs")
        .iter()
        .enumerate()
    {
        let side = |position: usize| {
            fingerprint(
                &all.graphs[pair[position].as_str().expect("graph")],
                &BTreeSet::new(),
                None,
            )
        };
        let actual =
            outcome(side(0).and_then(|before| {
                side(1).and_then(|after| compare_fingerprints(&before, &after))
            }));
        mismatches.check(
            &format!("analysis/compareFingerprints/{index}"),
            &actual,
            &wanted["compareFingerprints"][index],
        );
    }
    mismatches.check(
        "analysis/defaultContract",
        &default_contract(),
        &wanted["defaultContract"],
    );
    mismatches.check(
        "analysis/constants",
        &json!({"ENGINE_VERSION": contract::ENGINE_VERSION}),
        &wanted["constants"],
    );
    mismatches.finish("checks and fingerprints");
}

#[test]
fn graphs_impact_and_selection_match_python() {
    let wanted = wanted();
    let cases = cases();
    let all = inputs(&cases);
    let mut mismatches = Mismatches::default();
    for item in cases["rawGraphs"].as_array().expect("raw graphs") {
        let id = item["id"].as_str().expect("id");
        let graph = &item["graph"];
        let actual = json!({
            "validate": outcome(validate_graph(graph).map(|()| Value::Null)),
            "fingerprint": shrink(outcome(fingerprint(graph, &BTreeSet::new(), None))),
            "impact": outcome(impact(graph, &["a.py".to_owned()], 100, 4)),
        });
        mismatches.check(
            &format!("analysis/rawGraphs/{id}"),
            &actual,
            &wanted["rawGraphs"][id],
        );
    }
    for (index, item) in cases["impacts"]
        .as_array()
        .expect("impacts")
        .iter()
        .enumerate()
    {
        let paths: Vec<String> = item["paths"]
            .as_array()
            .expect("paths")
            .iter()
            .map(|path| path.as_str().expect("path").to_owned())
            .collect();
        // Negative bounds are refused with the same message as ones that are too large.
        let limit = item["limit"].as_i64().expect("limit").max(0) as usize;
        let depth = usize::try_from(item["depth"].as_i64().expect("depth")).unwrap_or(5);
        let actual = shrink(outcome(impact(
            &all.graphs[item["graph"].as_str().expect("graph")],
            &paths,
            limit,
            depth,
        )));
        mismatches.check(
            &format!("analysis/impacts/{index}"),
            &actual,
            &wanted["impacts"][index],
        );
    }
    let report = all
        .run_check(
            cases["checks"]
                .as_array()
                .expect("checks")
                .iter()
                .find(|item| item["id"] == "an-cand-forbidden|boundary|none")
                .expect("check exists"),
        )
        .expect("report");
    for (index, arguments) in cases["selects"]
        .as_array()
        .expect("selects")
        .iter()
        .enumerate()
    {
        let actual = outcome(select_findings(&report, arguments).map(|found| {
            json!(
                found["findings"]
                    .as_array()
                    .expect("findings")
                    .iter()
                    .map(|finding| finding["id"].clone())
                    .collect::<Vec<_>>()
            )
        }));
        mismatches.check(
            &format!("analysis/selects/{index}"),
            &actual,
            &wanted["selects"][index],
        );
    }
    mismatches.finish("graphs, impact and selection");
}

#[test]
fn context_and_response_budget_match_python() {
    let wanted = wanted();
    let cases = cases();
    let all = inputs(&cases);
    let mut mismatches = Mismatches::default();
    let packet = |item: &Value| -> Result<J, PyError> {
        let contract = item["contract"]
            .as_str()
            .map_or_else(|| all.default.clone(), |name| all.contracts[name].clone());
        let report = all.run_check(&json!({
            "candidate": item["candidate"],
            "baseline": item["baseline"],
            "contract": item["contract"],
            "approved": "contract",
            "enforce": true,
            "asOf": "2026-11-04",
        }))?;
        let paths: Vec<String> = item["paths"]
            .as_array()
            .expect("paths")
            .iter()
            .map(|path| path.as_str().expect("path").to_owned())
            .collect();
        context(
            &all.graphs[item["candidate"].as_str().expect("candidate")],
            &report,
            &contract,
            item["task"].as_str().expect("task"),
            &paths,
        )
    };
    let budgets: Vec<i64> = cases["budgets"]
        .as_array()
        .expect("budgets")
        .iter()
        .map(|budget| budget.as_i64().expect("budget"))
        .collect();
    for (index, item) in cases["contexts"]
        .as_array()
        .expect("contexts")
        .iter()
        .enumerate()
    {
        mismatches.check(
            &format!("analysis/contexts/{index}"),
            &shrink(outcome(packet(item).map(|found| found.to_value()))),
            &wanted["contexts"][index],
        );
        for budget in &budgets {
            let key = format!("{index}|{budget}");
            let actual = digested(
                packet(item)
                    .and_then(|found| bounded(&found, *budget))
                    .map(|found| found.to_value()),
            );
            mismatches.check(
                &format!("analysis/boundedContexts/{key}"),
                &actual,
                &wanted["boundedContexts"][&key],
            );
        }
    }
    for name in [
        "an-cand-forbidden|boundary|contract",
        "an-cand-big|boundary|contract",
        "ports",
    ] {
        let item = cases["checks"]
            .as_array()
            .expect("checks")
            .iter()
            .find(|item| item["id"] == name)
            .expect("check exists");
        let report = all.run_check(item).expect("report");
        for budget in &budgets {
            let key = format!("{name}|{budget}");
            let actual =
                digested(bounded(&report_tree(&report), *budget).map(|found| found.to_value()));
            mismatches.check(
                &format!("analysis/boundedReports/{key}"),
                &actual,
                &wanted["boundedReports"][&key],
            );
        }
    }
    for item in cases["packets"].as_array().expect("packets") {
        let id = item["id"].as_str().expect("id");
        for budget in &budgets {
            let key = format!("{id}|{budget}");
            let actual = digested(
                bounded(&J::from_value(&item["packet"]), *budget).map(|found| found.to_value()),
            );
            mismatches.check(
                &format!("analysis/boundedPackets/{key}"),
                &actual,
                &wanted["boundedPackets"][&key],
            );
        }
    }
    mismatches.finish("context and response budget");
}
