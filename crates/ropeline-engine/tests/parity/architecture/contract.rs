//! `architecture_contract`: policy parsing, models, paths, digests and the ontology.

use super::{outcome, parse, revive};
use crate::Mismatches;
use base64::Engine as _;
use ropeline_engine::architecture::PyError;
use ropeline_engine::architecture::contract::{
    self, ContractInput, contract_digest, digest, is_safe_path, ontology, parse_contract,
    valid_source_files,
};
use ropeline_engine::architecture::contract_models as models;
use serde_json::{Value, json};

fn cases() -> Value {
    parse(include_str!("contract.json"))
}

fn wanted() -> Value {
    parse(include_str!("../expected/architecture-contract.json"))
}

fn parsed(case: &Value) -> Result<Value, PyError> {
    let pad = " ".repeat(case["pad"].as_u64().unwrap_or(0) as usize);
    let data = &case["data"];
    let contract = match case["kind"].as_str().expect("kind") {
        "text" => {
            // `repeat` builds deep inputs here, as the recorder does.
            let repeat = case["repeat"].as_u64().unwrap_or(1) as usize;
            let text = format!("{pad}{}", data.as_str().expect("text").repeat(repeat));
            parse_contract(ContractInput::Text(&text))
        }
        "bytes" => {
            let mut bytes = pad.into_bytes();
            bytes.extend(
                base64::engine::general_purpose::STANDARD
                    .decode(data.as_str().expect("base64"))
                    .expect("valid base64"),
            );
            parse_contract(ContractInput::Bytes(&bytes))
        }
        // Python hands a `str` to the JSON decoder, so a string value is text, not a value.
        _ if data.is_string() => parse_contract(ContractInput::Text(data.as_str().expect("text"))),
        _ => parse_contract(ContractInput::Decoded(&revive(data))),
    }?;
    Ok(json!({"dump": contract.dump, "digest": contract_digest(&contract)}))
}

fn model(case: &Value) -> Result<Value, PyError> {
    let value = revive(&case["value"]);
    match case["model"].as_str().expect("model") {
        "Evidence" => models::validate_evidence(&value),
        "Classification" => models::validate_classification(&value),
        "StructuralRelationship" => models::validate_structural_relationship(&value),
        "PatternMatch" => models::validate_pattern_match(&value),
        "ArchitectureFinding" => models::validate_finding(&value),
        other => panic!("unknown model {other}"),
    }
}

#[test]
fn contract_parsing_matches_python() {
    let wanted = wanted();
    let mut mismatches = Mismatches::default();
    for case in cases()["parse"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("id");
        mismatches.check(
            &format!("contract/parse/{id}"),
            &outcome(parsed(case)),
            &wanted["parse"][id],
        );
    }
    mismatches.finish("contract parse");
}

#[test]
fn models_match_python() {
    let wanted = wanted();
    let mut mismatches = Mismatches::default();
    for case in cases()["models"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("id");
        mismatches.check(
            &format!("contract/models/{id}"),
            &outcome(model(case)),
            &wanted["models"][id],
        );
    }
    mismatches.finish("contract models");
}

#[test]
fn paths_digests_ontology_and_files_match_python() {
    let wanted = wanted();
    let cases = cases();
    let mut mismatches = Mismatches::default();
    for (index, case) in cases["paths"].as_array().expect("paths").iter().enumerate() {
        let value = case["value"]
            .as_str()
            .expect("text")
            .repeat(case["repeat"].as_u64().unwrap_or(1) as usize);
        let actual = is_safe_path(&value, case["glob"].as_bool().expect("glob"));
        mismatches.check(
            &format!("contract/paths/{index} {:?}", case["value"]),
            &json!(actual),
            &wanted["paths"][index],
        );
    }
    for (index, value) in cases["digests"]
        .as_array()
        .expect("digests")
        .iter()
        .enumerate()
    {
        mismatches.check(
            &format!("contract/digests/{index}"),
            &outcome(Ok(json!(digest(value)))),
            &wanted["digests"][index],
        );
    }
    for (index, value) in cases["ontologies"]
        .as_array()
        .expect("ontologies")
        .iter()
        .enumerate()
    {
        let extensions = (!value.is_null()).then_some(value);
        mismatches.check(
            &format!("contract/ontologies/{index}"),
            &outcome(ontology(extensions)),
            &wanted["ontologies"][index],
        );
    }
    for (index, files) in cases["sourceFiles"]
        .as_array()
        .expect("files")
        .iter()
        .enumerate()
    {
        let list = files.as_array().expect("list");
        mismatches.check(
            &format!("contract/sourceFiles/{index}"),
            &outcome(valid_source_files(list).map(|(kept, skipped)| json!([kept, skipped]))),
            &wanted["sourceFiles"][index],
        );
    }
    let constants = &wanted["constants"];
    let actual = json!({
        "VERSION": contract::VERSION,
        "ENGINE_VERSION": contract::ENGINE_VERSION,
        "MAX_FILES": contract::MAX_FILES,
        "MAX_EDGES": contract::MAX_EDGES,
        "MAX_REPORT_BYTES": contract::MAX_REPORT_BYTES,
    });
    mismatches.check("contract/constants", &actual, constants);
    mismatches.finish("contract paths, digests, ontology, files");
}
