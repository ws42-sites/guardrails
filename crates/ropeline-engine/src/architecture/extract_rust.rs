//! Test signals in Rust sources: assertion macros, always-true assertions and
//! `#[ignore]`d tests. The Rust branch of `architecture_extract.extract_facts`.

use super::PyError;
use super::decode_utf8;
use super::structure_facts::named_children;
use crate::parsing::parse_tree;
use serde_json::{Map, Value, json};
use tree_sitter::Node;

const ASSERT_PREFIXES: [&str; 2] = ["assert", "debug_assert"];

fn source_of<'a>(node: Node<'_>, data: &'a [u8]) -> &'a [u8] {
    &data[node.start_byte()..node.end_byte()]
}

/// Whether `assert!(true)` is all a macro call says.
fn is_vacuous(call: Node<'_>, data: &[u8]) -> bool {
    let tokens: Vec<Node<'_>> = named_children(call)
        .into_iter()
        .filter(|child| child.kind() == "token_tree")
        .flat_map(named_children)
        .collect();
    tokens.len() == 1 && source_of(tokens[0], data) == b"true"
}

/// `#[ignore]` and `#[ignore = "reason"]` switch a test off.
fn is_ignore(attribute: Node<'_>, data: &[u8]) -> bool {
    let marker = named_children(attribute)
        .first()
        .and_then(|first| named_children(*first).first().copied());
    marker.is_some_and(|found| source_of(found, data) == b"ignore")
}

/// Fill `parse_error` and the assertion and skip counts for one Rust source.
pub fn rust_counts(data: &[u8], result: &mut Map<String, Value>) -> Result<(), PyError> {
    let tree = parse_tree("rust", data)
        .map_err(|error| PyError::new("RuntimeError", error.to_string()))?;
    let root = tree.root_node();
    result.insert("parse_error".into(), Value::Bool(root.has_error()));
    let (mut assertions, mut skips) = (0u64, 0u64);
    let mut vacuous: Vec<Value> = Vec::new();
    let mut pending: Vec<Node<'_>> = vec![root];
    while let Some(node) = pending.pop() {
        if node.kind() == "macro_invocation" {
            let written = match node.child_by_field_name("macro") {
                Some(found) => decode_utf8(source_of(found, data))?,
                None => String::new(),
            };
            let name = written.rsplit("::").next().unwrap_or("");
            if ASSERT_PREFIXES
                .iter()
                .any(|prefix| name.starts_with(prefix))
            {
                assertions += 1;
                if name == "assert" && is_vacuous(node, data) {
                    vacuous.push(json!(node.start_position().row + 1));
                }
            }
        } else if node.kind() == "attribute_item" && is_ignore(node, data) {
            skips += 1;
        }
        pending.extend(named_children(node));
    }
    result.insert("test_assertions".into(), json!(assertions));
    result.insert("test_skips".into(), json!(skips));
    result.insert("vacuous_assertions".into(), Value::Array(vacuous));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(source: &str) -> Map<String, Value> {
        let mut result = Map::new();
        rust_counts(source.as_bytes(), &mut result).expect("counts");
        result
    }

    #[test]
    fn assertion_macros_are_counted_by_their_last_path_segment() {
        let found = counts(
            "fn t() { assert!(a); std::assert_eq!(a, b); debug_assert_ne!(a, b); my_assert!(a); assertion!(a); }",
        );
        assert_eq!(found["test_assertions"], 4);
    }

    #[test]
    fn only_a_bare_assert_of_true_is_vacuous() {
        let found = counts(
            "fn t() {\n    assert!(true);\n    assert!(true, \"m\");\n    debug_assert!(true);\n    assert!(false);\n    assert_eq!(true);\n}\n",
        );
        assert_eq!(found["vacuous_assertions"], json!([2]));
    }

    #[test]
    fn ignored_tests_are_skips_with_or_without_a_reason() {
        let found = counts(
            "#[ignore]\nfn a() {}\n#[ignore = \"slow\"]\nfn b() {}\n#[ignored]\nfn c() {}\n#![ignore]\n",
        );
        assert_eq!(found["test_skips"], 2);
    }

    #[test]
    fn broken_source_is_flagged_not_refused() {
        let found = counts("fn t() { assert!(true;\n");
        assert_eq!(found["parse_error"], true);
    }
}
