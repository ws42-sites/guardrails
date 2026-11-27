//! The Rust half of `architecture_structure`: type and function declarations, and the
//! EXTENDS, IMPLEMENTS and INJECTS relations that follow from them.
//!
//! Only what the file itself says is recorded. An `impl` block is matched to a type
//! by name, and only when the type was declared earlier in the same file.

use super::PyError;
use super::structure_facts::{descendants, identifier, named_children, node_text, relation};
use crate::parsing::parse_tree;
use serde_json::{Value, json};
use std::collections::HashMap;
use tree_sitter::Node;

const METHOD_LIMIT: usize = 64;
const PARAMETER_LIMIT: usize = 32;
/// Python's message when a parameter has no pattern to ask the type of.
const NO_PATTERN: &str = "'NoneType' object has no attribute 'type'";

/// How a Rust item maps onto the shared declaration kinds.
fn type_kind(node_kind: &str) -> Option<&'static str> {
    match node_kind {
        "struct_item" | "enum_item" | "union_item" => Some("class"),
        "trait_item" => Some("interface"),
        _ => None,
    }
}

fn exported(node: Node<'_>) -> bool {
    (0..node.child_count())
        .filter_map(|index| node.child(index))
        .any(|child| child.kind() == "visibility_modifier")
}

/// `&T` and `&mut T` inject `T`; the borrow is not part of the dependency.
fn borrowed(node: Option<Node<'_>>) -> Option<Node<'_>> {
    let mut node = node;
    while let Some(current) = node.filter(|found| found.kind() == "reference_type") {
        node = current.child_by_field_name("type");
    }
    node
}

/// Names of the functions and bare signatures directly inside a body.
fn functions(data: &[u8], body: Option<Node<'_>>) -> Result<Vec<String>, PyError> {
    let Some(body) = body else {
        return Ok(Vec::new());
    };
    named_children(body)
        .into_iter()
        .filter(|child| matches!(child.kind(), "function_item" | "function_signature_item"))
        .map(|child| node_text(data, child.child_by_field_name("name")))
        .collect()
}

/// A function's parameters as `(name, type)`, leaving out `self` and unwrapping borrows.
fn injected_parameters(data: &[u8], function: Node<'_>) -> Result<Vec<(String, String)>, PyError> {
    let Some(parameters) = function.child_by_field_name("parameters") else {
        return Ok(Vec::new());
    };
    let mut injected = Vec::new();
    for param in named_children(parameters) {
        if param.kind() != "parameter" {
            continue;
        }
        let pattern = param
            .child_by_field_name("pattern")
            .ok_or_else(|| PyError::new("AttributeError", NO_PATTERN))?;
        if pattern.kind() == "self" {
            continue;
        }
        let kind = borrowed(param.child_by_field_name("type"));
        injected.push((
            node_text(data, Some(pattern))?,
            identifier(&node_text(data, kind)?),
        ));
    }
    Ok(injected)
}

/// Facts for one Rust source.
pub fn rust_facts(data: &[u8]) -> Result<Value, PyError> {
    let tree = parse_tree("rust", data)
        .map_err(|error| PyError::new("RuntimeError", error.to_string()))?;
    let root = tree.root_node();
    let text_of = |node: Option<Node<'_>>| node_text(data, node);

    let mut declarations: Vec<Value> = Vec::new();
    let mut relations: Vec<Value> = Vec::new();
    // The first declaration of a name, as an index into `declarations`.
    let mut types: HashMap<String, usize> = HashMap::new();
    for node in descendants(Some(root)) {
        let name = text_of(node.child_by_field_name("name"))?;
        let line = node.start_position().row + 1;
        if let Some(kind) = type_kind(node.kind()) {
            let mut methods: Vec<String> = Vec::new();
            if node.kind() == "trait_item" {
                methods = functions(data, node.child_by_field_name("body"))?;
                methods.truncate(METHOD_LIMIT);
            }
            declarations.push(json!({
                "name": name,
                "kind": kind,
                "line": line,
                "methods": methods,
                "exported": exported(node),
            }));
            types.entry(name.clone()).or_insert(declarations.len() - 1);
            if let Some(bounds) = node.child_by_field_name("bounds") {
                for base in named_children(bounds) {
                    relations.push(relation(
                        "EXTENDS",
                        &name,
                        &identifier(&text_of(Some(base))?),
                        line,
                        "declared-base",
                        "observed",
                    ));
                }
            }
        } else if node.kind() == "impl_item" {
            let mut owner = node.child_by_field_name("type");
            if owner.is_some_and(|found| found.kind() == "generic_type") {
                owner = owner.and_then(|found| found.child_by_field_name("type"));
            }
            let owner_name = text_of(owner)?;
            if let Some(&index) = types.get(&owner_name)
                && declarations[index]["kind"] == "class"
            {
                // Methods are listed on a type only when it is declared earlier in this file.
                let mut merged: Vec<Value> = declarations[index]["methods"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                merged.extend(
                    functions(data, node.child_by_field_name("body"))?
                        .into_iter()
                        .map(Value::String),
                );
                merged.truncate(METHOD_LIMIT);
                declarations[index]["methods"] = Value::Array(merged);
            }
            if let Some(trait_node) = node.child_by_field_name("trait") {
                relations.push(relation(
                    "IMPLEMENTS",
                    &identifier(&owner_name),
                    &identifier(&text_of(Some(trait_node))?),
                    line,
                    "declared-heritage",
                    "observed",
                ));
            }
        } else if node.kind() == "function_item" {
            let injected = injected_parameters(data, node)?;
            declarations.push(json!({
                "name": name,
                "kind": "function",
                "line": line,
                "parameters": injected
                    .iter()
                    .take(PARAMETER_LIMIT)
                    .map(|(parameter, kind)| json!({"name": parameter, "type": kind}))
                    .collect::<Vec<_>>(),
                "exported": exported(node),
            }));
            for (_, kind) in &injected {
                relations.push(relation(
                    "INJECTS",
                    &name,
                    kind,
                    line,
                    "annotated-parameter",
                    "observed",
                ));
            }
        }
    }
    Ok(json!({
        "declarations": declarations,
        "relations": relations,
        "effects": [],
        "parse_error": root.has_error(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(source: &str) -> Value {
        rust_facts(source.as_bytes()).expect("facts")
    }

    fn methods_of(facts: &Value, name: &str) -> Vec<String> {
        facts["declarations"]
            .as_array()
            .expect("declarations")
            .iter()
            .find(|item| item["name"] == name)
            .and_then(|item| item["methods"].as_array())
            .expect("methods")
            .iter()
            .map(|method| method.as_str().expect("name").to_owned())
            .collect()
    }

    #[test]
    fn impl_methods_join_only_a_type_declared_earlier() {
        let found =
            facts("impl S { fn early(&self) {} }\nstruct S;\nimpl S { fn late(&self) {} }\n");
        assert_eq!(methods_of(&found, "S"), ["late"]);
    }

    #[test]
    fn traits_list_their_methods_and_keep_them_apart_from_impls() {
        let found = facts("trait T { fn a(&self); fn b(&self) {} }\nimpl T { fn c(&self) {} }\n");
        assert_eq!(methods_of(&found, "T"), ["a", "b"]);
    }

    #[test]
    fn heritage_and_parameters_become_relations() {
        let found =
            facts("trait T: A + B {}\nstruct S;\nimpl T for S {}\nfn f(a: &mut S, b: &&T) {}\n");
        let relations: Vec<(&str, &str, &str)> = found["relations"]
            .as_array()
            .expect("relations")
            .iter()
            .map(|item| {
                (
                    item["kind"].as_str().expect("kind"),
                    item["source"].as_str().expect("source"),
                    item["target"].as_str().expect("target"),
                )
            })
            .collect();
        assert_eq!(
            relations,
            [
                ("EXTENDS", "T", "A"),
                ("EXTENDS", "T", "B"),
                ("IMPLEMENTS", "S", "T"),
                ("INJECTS", "f", "S"),
                ("INJECTS", "f", "T"),
            ]
        );
    }

    #[test]
    fn long_method_lists_and_parameter_lists_are_cut() {
        let methods: String = (0..70)
            .map(|index| format!("fn m{index}(&self);"))
            .collect();
        let found = facts(&format!("trait T {{ {methods} }}"));
        assert_eq!(methods_of(&found, "T").len(), METHOD_LIMIT);
        let parameters: Vec<String> = (0..40).map(|index| format!("p{index}: D{index}")).collect();
        let found = facts(&format!("fn f({}) {{}}", parameters.join(", ")));
        let listed = found["declarations"][0]["parameters"]
            .as_array()
            .expect("list");
        assert_eq!(listed.len(), PARAMETER_LIMIT);
        // Every parameter still makes a relation; only the listing is cut.
        assert_eq!(found["relations"].as_array().expect("relations").len(), 40);
    }

    #[test]
    fn bytes_that_are_not_utf8_inside_a_name_are_an_error() {
        let error = rust_facts(b"trait T: Fn(\xff) {}\n").expect_err("refused");
        assert_eq!(error.kind, "UnicodeDecodeError");
    }
}
