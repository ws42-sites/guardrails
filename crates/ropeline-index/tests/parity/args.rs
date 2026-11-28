//! Turning the inert case data into the values the crate takes.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use ropeline_engine::codeowners;
use ropeline_index::{Clock, IndexOptions, SourceError, SourceFile, source_language};
use serde_json::Value;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Bytes for a content spec: text, or `{hex}`, `{base64}` or `{repeat}`.
pub fn content_bytes(spec: &Value) -> Vec<u8> {
    if let Some(text) = spec.as_str() {
        return text.as_bytes().to_vec();
    }
    if let Some(hex) = spec.get("hex").and_then(Value::as_str) {
        return (0..hex.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).expect("hex"))
            .collect();
    }
    if let Some(encoded) = spec.get("base64").and_then(Value::as_str) {
        return STANDARD.decode(encoded).expect("base64");
    }
    let repeat = &spec["repeat"];
    let text = |key: &str| repeat.get(key).and_then(Value::as_str).unwrap_or("");
    let times = usize::try_from(repeat["times"].as_u64().expect("times")).expect("size");
    let mut out = text("prefix").to_owned();
    out.push_str(&text("text").repeat(times));
    out.push_str(text("suffix"));
    out.into_bytes()
}

/// The golden fixture manifests, which live with the Python goldens.
pub fn manifest(name: &str) -> Vec<(String, Value)> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/golden/repos")
        .join(format!("{name}.json"));
    let text = std::fs::read_to_string(path).expect("fixture manifest");
    let parsed: Value = serde_json::from_str(&text).expect("manifest JSON");
    parsed["files"]
        .as_object()
        .expect("files")
        .iter()
        .map(|(path, content)| (path.clone(), content.clone()))
        .collect()
}

/// A fixture with a step's edits, in manifest order; paths the indexer does not read are
/// dropped, as the recorder drops them.
pub fn listed_files(step: &Value) -> Vec<(String, Value)> {
    let Some(name) = step.get("fixture").and_then(Value::as_str) else {
        return step["files"]
            .as_array()
            .expect("files")
            .iter()
            .map(|item| {
                (
                    item["path"].as_str().expect("path").to_owned(),
                    item.clone(),
                )
            })
            .collect();
    };
    let mut files: Vec<(String, Value)> = manifest(name);
    if let Some(edits) = step.get("set").and_then(Value::as_object) {
        for (path, content) in edits {
            match files.iter_mut().find(|(known, _)| known == path) {
                Some(found) => found.1 = content.clone(),
                None => files.push((path.clone(), content.clone())),
            }
        }
    }
    for path in step
        .get("drop")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        files.retain(|(known, _)| Some(known.as_str()) != path.as_str());
    }
    files
        .into_iter()
        .filter(|(path, _)| source_language(path).is_some())
        .map(|(path, content)| {
            let item = serde_json::json!({ "path": path, "data": content });
            (path, item)
        })
        .collect()
}

pub fn source_files(
    step: &Value,
    resolve: &dyn Fn(&Value) -> Value,
) -> Vec<Result<SourceFile, SourceError>> {
    listed_files(step)
        .into_iter()
        .map(|(path, item)| {
            let mut file = SourceFile {
                path,
                ..SourceFile::default()
            };
            if let Some(id) = item.get("analysisId") {
                file.analysis_id = resolve(id).as_str().map(str::to_owned);
            } else {
                file.data = Some(content_bytes(&item["data"]));
            }
            if let Some(oid) = item.get("gitOid") {
                file.git_oid = resolve(oid).as_str().map(str::to_owned);
            }
            Ok(file)
        })
        .collect()
}

/// The recorder's keyword options, as `IndexOptions`.
pub fn index_options(step: &Value) -> IndexOptions {
    let mut options = IndexOptions::default();
    let Some(given) = step.get("options").and_then(Value::as_object) else {
        return with_ownership(options, step);
    };
    for (key, value) in given {
        match key.as_str() {
            "source_revision" => options.source_revision = value.as_str().map(str::to_owned),
            "workspaceContext" => options.workspace_context = Some(value.clone()),
            "max_files" => options.max_files = value.as_i64().expect("int"),
            "max_bytes" => options.max_bytes = value.as_i64().expect("int"),
            "max_symbols" => options.max_symbols = value.as_i64().expect("int"),
            "timeout_seconds" => options.timeout_seconds = value.as_f64().expect("number"),
            "waypoint_configuration" => {
                options.waypoint_configuration = strings(value);
            }
            "waypoint_policy_digest" => {
                options.waypoint_policy_digest = value.as_str().expect("text").to_owned();
            }
            // Folder options are read by `index_repo`.
            "include_dist" | "max_file_bytes" => {}
            other => panic!("unknown option {other}"),
        }
    }
    with_ownership(options, step)
}

fn strings(value: &Value) -> BTreeMap<String, String> {
    value
        .as_object()
        .expect("mapping")
        .iter()
        .map(|(key, item)| (key.clone(), item.as_str().expect("text").to_owned()))
        .collect()
}

fn with_ownership(mut options: IndexOptions, step: &Value) -> IndexOptions {
    if let Some(spec) = step.get("ownership") {
        options.ownership = codeowners::parse(
            spec["text"].as_str().expect("text").as_bytes(),
            spec["path"].as_str().expect("path"),
        );
    }
    options
}

/// Every call advances by `step`; the first returns 0, as the recorder's clock does.
pub struct CountingClock {
    step: f64,
    calls: Cell<u64>,
}

impl CountingClock {
    pub fn new(step: f64) -> Self {
        Self {
            step,
            calls: Cell::new(0),
        }
    }
}

impl Clock for CountingClock {
    fn now(&self) -> f64 {
        let calls = self.calls.get();
        self.calls.set(calls + 1);
        calls as f64 * self.step
    }
}
