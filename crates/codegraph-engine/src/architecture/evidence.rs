//! Source evidence records: `evidence` from `architecture_recognition.py`, which
//! topology and the pattern and anomaly modules all build on.

use super::PyError;
use super::util::{get, lookup};
use serde_json::{Value, json};

pub const RECOGNITION_VERSION: &str = "recognition-1";

/// `evidence(graph, file, line, method)`: where a finding comes from, pinned to a revision.
pub fn evidence(graph: &Value, file: &Value, line: &Value, method: &str) -> Result<Value, PyError> {
    // Python builds the record in this order, so a missing key is reported the same way.
    let repository = get(graph, "repository")?;
    let revision = get(graph, "revision")?;
    let path = get(file, "path")?;
    let facts = get(file, "facts")?;
    Ok(json!({
        "repository": repository,
        "revision": revision,
        "path": path,
        "line": line,
        "method": method,
        "sha256": lookup(facts, "sha256").cloned().unwrap_or(Value::Null),
    }))
}
