//! The snapshot's revision and the workspace it was captured from.

use crate::error::{Result, SourceError};
use crate::identity::{sha256_hex, stable_id};
use codegraph_engine::text;
use codegraph_store::json_text::{dumps_ordered, dumps_sorted};
use serde_json::{Map, Value, json};

/// A full commit id: forty lowercase hex digits and nothing else.
pub fn is_commit(text: &str) -> bool {
    text.len() == 40 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn is_digest(value: &Value) -> bool {
    value.as_str().is_some_and(|text| {
        text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    })
}

fn invalid() -> SourceError {
    SourceError::value("Invalid workspace snapshot context")
}

/// The context for a snapshot: its revision string, its metadata and its view key.
pub struct SnapshotContext {
    pub revision: String,
    pub metadata: Map<String, Value>,
    pub view_key: String,
}

fn check_shape(context: &Value, source_revision: Option<&str>) -> Result<(String, String)> {
    let Some(map) = context.as_object() else {
        return Err(invalid());
    };
    let keys: Vec<&str> = map.keys().map(String::as_str).collect();
    let basic = ["branch", "fingerprint", "workspace_id"];
    let exact =
        |wanted: &[&str]| keys.len() == wanted.len() && wanted.iter().all(|key| keys.contains(key));
    if source_revision.is_none()
        || !(exact(&basic) || exact(&["branch", "fingerprint", "workspace_id", "view"]))
        || !is_digest(&map["fingerprint"])
        || !is_digest(&map["workspace_id"])
    {
        return Err(invalid());
    }
    let view = map.get("view").and_then(Value::as_str);
    if map.contains_key("view") && !matches!(view, Some("branch" | "working_tree")) {
        return Err(invalid());
    }
    match &map["branch"] {
        Value::Null => {}
        Value::String(branch) => {
            let length = text::char_len(branch);
            if length == 0 || length > 1024 || branch.contains('\0') {
                return Err(invalid());
            }
        }
        _ => return Err(invalid()),
    }
    Ok((
        view.unwrap_or("working_tree").to_owned(),
        map["fingerprint"].as_str().unwrap_or_default().to_owned(),
    ))
}

/// `_context` plus the view key: validate the workspace context and build what the
/// snapshot row starts with. A context needs a source revision to be pinned to.
pub fn snapshot_context(
    source_revision: Option<&str>,
    workspace_context: Option<&Value>,
) -> Result<SnapshotContext> {
    let mut metadata = Map::new();
    let mut revision = source_revision.unwrap_or("working-tree").to_owned();
    let mut view_key = "default".to_owned();
    if let Some(context) = workspace_context {
        let (view, fingerprint) = check_shape(context, source_revision)?;
        revision.push_str(if view == "branch" {
            "-branch-"
        } else {
            "-worktree-"
        });
        revision.push_str(&fingerprint);
        // `{**context, "head": ..., "view": ...}`: the caller's key order survives, a
        // missing `view` lands last.
        let mut workspace = context.as_object().cloned().unwrap_or_default();
        workspace.insert("head".into(), json!(source_revision));
        workspace.insert("view".into(), json!(view));
        view_key = sha256_hex(
            dumps_ordered(&json!([context["workspace_id"], context["branch"], view])).as_bytes(),
        );
        metadata.insert("workspace".into(), Value::Object(workspace));
    }
    Ok(SnapshotContext {
        revision,
        metadata,
        view_key,
    })
}

/// The snapshot's node prefix: a hash of its repository, revision and metadata.
pub fn node_prefix(
    snapshot: i64,
    repository: &str,
    revision: &str,
    metadata: &Map<String, Value>,
) -> String {
    let tail = format!(
        "{revision}{}",
        dumps_sorted(&Value::Object(metadata.clone()))
    );
    stable_id(&snapshot.to_string(), "snapshot", repository, &tail, None)
}
