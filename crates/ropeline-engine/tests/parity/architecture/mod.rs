//! Architecture analysis against what Python recorded (see `record_architecture_parity.py`).
//!
//! One file per Python module. Cases live in `tests/parity/architecture/<name>.json`
//! and Python's answers in `tests/parity/expected/architecture-<name>.json`.

mod analysis;
mod anomalies;
mod ci;
mod contract;
mod extract;
mod patterns;
mod presentation;
mod recognition;
mod rules;
mod structure;
mod topology;

use ropeline_engine::architecture::PyError;
use ropeline_engine::py_value::PyValue;
use serde_json::{Value, json};

/// A result in the recorded shape: `{"value": ...}` or the exception.
pub fn outcome(result: Result<Value, PyError>) -> Value {
    match result {
        Ok(value) => json!({"value": value}),
        Err(error) => error.to_value(),
    }
}

pub fn parse(raw: &str) -> Value {
    serde_json::from_str(raw).expect("recorded file is JSON")
}

/// Spec files carry integers beyond 64 bits as `{"$bigint": "digits"}`, which JSON
/// readers here would round to floats; both recorders and this test revive them.
pub fn revive(value: &Value) -> PyValue {
    match value {
        Value::Object(map) if map.len() == 1 && map.contains_key("$bigint") => {
            let digits = map["$bigint"].as_str().expect("digits");
            digits
                .parse::<i64>()
                .map_or_else(|_| PyValue::BigInt(digits.to_owned()), PyValue::Int)
        }
        Value::Array(items) => PyValue::List(items.iter().map(revive).collect()),
        Value::Object(map) => PyValue::Dict(
            map.iter()
                .map(|(key, item)| (key.clone(), revive(item)))
                .collect(),
        ),
        other => PyValue::from_value(other),
    }
}

/// Large results are recorded as their size, list lengths and a digest (see the recorder).
pub fn shrink(result: Value) -> Value {
    use sha2::{Digest, Sha256};
    let text = ropeline_engine::text::python_json(&result);
    if text.len() <= 20_000 {
        return result;
    }
    let counts: serde_json::Map<String, Value> = result["value"]
        .as_object()
        .map(|object| {
            object
                .iter()
                .filter_map(|(key, item)| Some((key.clone(), json!(item.as_array()?.len()))))
                .collect()
        })
        .unwrap_or_default();
    json!({"bulky": {
        "size": text.len(),
        "counts": counts,
        "sha256": format!("{:x}", Sha256::digest(text.as_bytes())),
    }})
}

/// The bytes of a case: base64, or text with `{i}` expanded `repeat` times.
pub fn source_bytes(case: &Value) -> Vec<u8> {
    use base64::Engine as _;
    if let Some(encoded) = case["base64"].as_str() {
        return base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .expect("valid base64");
    }
    let text = case["source"].as_str().expect("source");
    match case["repeat"].as_u64() {
        Some(count) => (0..count)
            .map(|index| text.replace("{i}", &index.to_string()))
            .collect::<String>()
            .into_bytes(),
        None => text.as_bytes().to_vec(),
    }
}

/// Recorded Python results the architecture engine cannot reproduce.
pub fn known_difference(id: &str) -> Option<&'static str> {
    match id {
        "ci/yaml/scalar-single" | "ci/yaml/ls-in-quotes" => Some(
            "yaml-rust2 refuses a quoted scalar whose continuation line is not indented past its block; PyYAML accepts it",
        ),
        "ci/yaml/scalar-double-surrogate" => Some(
            "an escaped surrogate pair makes lone surrogates in Python, which a Rust string cannot hold",
        ),
        "ci/yaml/nested-seq-in-flow-pair" => Some(
            "yaml-rust2 mis-nests a single-pair mapping inside a flow sequence when its value is a collection",
        ),
        "extract/text-toml-too-big-int" => Some(
            "the toml crate reads integers as 64-bit; tomllib reads any size, so such a file is not config here",
        ),
        _ => None,
    }
}

/// A snapshot whose files carry the facts the extraction gives their source.
pub fn build_graph(project: &Value) -> Value {
    use ropeline_engine::architecture::extract::extract_facts;
    let files: Vec<Value> = project["files"]
        .as_array()
        .expect("files")
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let facts = extract_facts(
                item["language"].as_str().expect("language"),
                item["source"].as_str().expect("source").as_bytes(),
                &json!({"imports": []}),
            )
            .expect("extraction succeeds");
            json!({
                "id": format!("n{index}"),
                "path": item["path"],
                "facts": facts,
                "metadata": {"language": item["language"]},
            })
        })
        .collect();
    let id_of = |path: &Value| -> Value {
        files
            .iter()
            .find(|file| file["path"] == *path)
            .map(|file| file["id"].clone())
            .expect("edge names a file")
    };
    let edges: Vec<Value> = project["edges"]
        .as_array()
        .expect("edges")
        .iter()
        .enumerate()
        .map(|(index, edge)| {
            json!({
                "id": format!("e{index}"),
                "source": id_of(&edge[0]),
                "target": id_of(&edge[1]),
                "kind": "IMPORTS",
                "evidence": {"line": 1},
            })
        })
        .collect();
    let mut graph = json!({
        "repository": project["repository"],
        "revision": project["revision"],
        "files": files,
        "edges": edges,
    });
    if !project["partial"].is_null() {
        graph["partial"] = project["partial"].clone();
    }
    graph
}

/// The shared snapshots and parsed contracts, by id.
pub fn shared_inputs() -> (
    std::collections::BTreeMap<String, Value>,
    std::collections::BTreeMap<String, ropeline_engine::architecture::contract::Contract>,
) {
    use ropeline_engine::architecture::contract::{ContractInput, parse_contract};
    let spec = parse(include_str!("projects.json"));
    let graphs = spec["projects"]
        .as_array()
        .expect("projects")
        .iter()
        .map(|item| {
            (
                item["id"].as_str().expect("id").to_owned(),
                build_graph(item),
            )
        })
        .collect();
    let contracts = spec["contracts"]
        .as_array()
        .expect("contracts")
        .iter()
        .map(|item| {
            (
                item["id"].as_str().expect("id").to_owned(),
                parse_contract(ContractInput::Value(&item["value"])).expect("valid contract"),
            )
        })
        .collect();
    (graphs, contracts)
}

/// The set of approved policy digests a mode names.
pub fn approvals(
    contract: &ropeline_engine::architecture::contract::Contract,
    mode: &str,
) -> std::collections::BTreeSet<String> {
    use ropeline_engine::architecture::contract::contract_digest;
    match mode {
        "contract" => [contract_digest(contract)].into(),
        "other" => ["x".repeat(64)].into(),
        _ => Default::default(),
    }
}
