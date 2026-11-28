//! `architecture_extract`: per-file facts and artifact roles.

use super::{outcome, parse, shrink};
use crate::Mismatches;
use base64::Engine as _;
use ropeline_engine::architecture::extract::{artifact_role, extract_facts};
use serde_json::{Value, json};

fn cases() -> Value {
    parse(include_str!("extract.json"))
}

fn wanted() -> Value {
    parse(include_str!("../expected/architecture-extract.json"))
}

/// The bytes of a case; `fill` replaces `{fill}` with a repeated character.
fn input(case: &Value) -> Vec<u8> {
    if let Some(encoded) = case["base64"].as_str() {
        return base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .expect("valid base64");
    }
    let mut text = case["source"].as_str().expect("source").to_owned();
    if let Some(fill) = case.get("fill") {
        let character = fill[0].as_str().expect("character");
        let count = fill[1].as_u64().expect("count") as usize;
        text = text.replace("{fill}", &character.repeat(count));
    }
    text.into_bytes()
}

#[test]
fn facts_match_python() {
    let wanted = wanted();
    let mut mismatches = Mismatches::default();
    for case in cases()["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("id");
        let references = case.get("references").cloned().unwrap_or(Value::Null);
        let actual = shrink(outcome(extract_facts(
            case["language"].as_str().expect("language"),
            &input(case),
            &references,
        )));
        mismatches.check(&format!("extract/{id}"), &actual, &wanted["cases"][id]);
    }
    mismatches.finish("extract");
}

#[test]
fn roles_match_python() {
    let wanted = wanted();
    let mut mismatches = Mismatches::default();
    for (index, path) in cases()["roles"]
        .as_array()
        .expect("roles")
        .iter()
        .enumerate()
    {
        let role = artifact_role(path.as_str().expect("path"));
        mismatches.check(
            &format!("extract/roles/{index} {path}"),
            &json!(role),
            &wanted["roles"][index],
        );
    }
    mismatches.finish("roles");
}
