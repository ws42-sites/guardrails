//! Which repository paths the waypoint cache may name, and which configuration applies where.

use ropeline_engine::posix_path::parts;
use ropeline_engine::text;
use ropeline_engine::waypoints;
use ropeline_engine::waypoints::digest;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};

pub(crate) fn is_digest(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Which paths may carry configuration fingerprints: a fixed list of file names that
/// change how their subtree is checked.
pub fn configuration_path(path: &str) -> bool {
    if path.is_empty()
        || text::char_len(path) > 4096
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains(':')
        || path.split('/').any(|part| matches!(part, "" | "." | ".."))
        || path.chars().any(|c| u32::from(c) < 32)
    {
        return false;
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    path == ".codegraph/architecture.json"
        || matches!(
            name,
            "package.json"
                | "pyproject.toml"
                | "ruff.toml"
                | "codegraph.json"
                | "architecture.json"
        )
        || (name.starts_with("tsconfig") && name.ends_with(".json"))
        || name.starts_with("eslint.config.")
}

/// `str(PurePosixPath(path).parent)`.
pub(crate) fn parent_of(path: &str) -> String {
    let found = parts(path);
    if found.len() <= 1 {
        ".".to_owned()
    } else {
        found[..found.len() - 1].join("/")
    }
}

/// `str(PurePosixPath(path).parents)` as a set: every ancestor, ending with `.`.
fn ancestors(path: &str) -> HashSet<String> {
    let found = parts(path);
    let mut result: HashSet<String> = (1..found.len()).map(|n| found[..n].join("/")).collect();
    result.insert(".".to_owned());
    result
}

pub(crate) fn policy_for(
    path: &str,
    configuration: &BTreeMap<String, String>,
    policy_digest: &str,
) -> String {
    let parents = ancestors(path);
    let relevant: Vec<Value> = configuration
        .iter()
        .filter(|(name, _)| {
            name.as_str() == ".codegraph/architecture.json" || parents.contains(&parent_of(name))
        })
        .map(|(name, value)| json!([name, value]))
        .collect();
    digest(&json!([waypoints::VERSION, policy_digest, relevant]))
}

/// Whether a path is safe to show in an annotation: no control or format characters and
/// no spelling the public contract refuses.
pub(crate) fn annotation_path(path: &str, root: bool) -> bool {
    let length = text::char_len(path);
    let shape = if root && path == "." {
        true
    } else {
        !path.starts_with('/')
            && !path.contains('\\')
            && !path.contains(':')
            && path.split('/').all(|part| !matches!(part, "" | "." | ".."))
    };
    0 < length && length <= 4096 && shape && !path.chars().any(text::is_other)
}
