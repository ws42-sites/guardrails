//! The JavaScript and TypeScript half of `architecture_structure._javascript`.

use super::PyError;
use super::structure_facts::{
    Counter, descendants, effect, effect_fact, identifier, named_children, node_text, relation,
};
use crate::parsing::parse_tree;
use crate::text;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

struct Source<'a> {
    data: &'a [u8],
}

impl Source<'_> {
    /// `text(node)`: up to 512 characters of the node's source; empty for no node.
    /// Python decodes the slice strictly, so invalid UTF-8 in a node is an error.
    fn text(&self, node: Option<Node<'_>>) -> Result<String, PyError> {
        node_text(self.data, node)
    }
}

fn field<'t>(node: Node<'t>, name: &str) -> Option<Node<'t>> {
    node.child_by_field_name(name)
}

fn parent_kind(node: Node<'_>) -> &str {
    node.parent().map_or("", |parent| parent.kind())
}

fn is_binding_identifier(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "identifier" | "shorthand_property_identifier_pattern"
    )
}

/// Facts for one script source.
pub fn script_facts(language: &str, data: &[u8]) -> Result<Value, PyError> {
    let tree = parse_tree(language, data)
        .map_err(|error| PyError::new("RuntimeError", error.to_string()))?;
    let root = tree.root_node();
    let mut nodes: Vec<Node<'_>> = Vec::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        nodes.push(node);
        pending.extend(named_children(node).into_iter().rev());
    }
    let source = Source { data };
    let text_of = |node: Option<Node<'_>>| source.text(node);

    let mut bindings = Counter::default();
    let mut aliases: HashMap<String, String> = HashMap::new();
    for node in &nodes {
        let node = *node;
        let kind = node.kind();
        if matches!(
            kind,
            "function_declaration"
                | "class_declaration"
                | "variable_declarator"
                | "required_parameter"
                | "optional_parameter"
        ) {
            let name = field(node, "name").or_else(|| field(node, "pattern"));
            if let Some(name) = name {
                if matches!(name.kind(), "identifier" | "type_identifier") {
                    bindings.bump(&text_of(Some(name))?);
                } else {
                    for binding in descendants(Some(name)) {
                        if is_binding_identifier(binding) {
                            bindings.bump(&text_of(Some(binding))?);
                        }
                    }
                }
            }
        }
        if matches!(
            kind,
            "assignment_expression" | "augmented_assignment_expression"
        ) {
            bindings.bump(&text_of(field(node, "left"))?);
        }
        if kind == "identifier"
            && matches!(
                parent_kind(node),
                "formal_parameters" | "rest_pattern" | "arrow_function"
            )
        {
            bindings.bump(&text_of(Some(node))?);
        }
        if kind == "import_statement"
            && let Some(dependency) = field(node, "source")
        {
            let shown = text_of(Some(dependency))?;
            // `text(dependency)[1:-1]`: the quotes come off.
            let length = shown.chars().count();
            let module = text::char_slice(&shown, 1, Some(length.saturating_sub(1)));
            let module = if length < 2 { "" } else { module };
            for child in descendants(Some(node)) {
                if child.kind() == "identifier" {
                    let local = text_of(Some(child))?;
                    aliases.insert(local.clone(), module.to_owned());
                    bindings.bump(&local);
                }
            }
        }
        let pattern_in_parameter = matches!(kind, "object_pattern" | "array_pattern")
            && matches!(
                parent_kind(node),
                "formal_parameters" | "required_parameter" | "optional_parameter"
            );
        if kind == "catch_clause" || pattern_in_parameter {
            let parameter = if kind == "catch_clause" {
                field(node, "parameter")
            } else {
                Some(node)
            };
            for binding in descendants(parameter) {
                if is_binding_identifier(binding) {
                    bindings.bump(&text_of(Some(binding))?);
                }
            }
        }
    }

    for node in &nodes {
        if node.kind() != "variable_declarator" {
            continue;
        }
        let value = field(*node, "value");
        let local = text_of(field(*node, "name"))?;
        if let Some(value) = value
            && matches!(value.kind(), "call_expression" | "new_expression")
            && bindings.get(&local) == 1
        {
            let constructor =
                text_of(field(value, "constructor").or_else(|| field(value, "function")))?;
            let first = constructor.split('.').next().unwrap_or_default();
            if let Some(dependency) = aliases.get(first).filter(|found| !found.is_empty()) {
                let dependency = dependency.clone();
                aliases.insert(local, dependency);
            }
        }
    }

    let mut local_functions: HashSet<String> = HashSet::new();
    for node in &nodes {
        if node.kind() == "function_declaration"
            && matches!(parent_kind(*node), "program" | "export_statement")
        {
            let name = text_of(field(*node, "name"))?;
            if bindings.get(&name) == 1 {
                local_functions.insert(name);
            }
        }
    }
    for node in &nodes {
        if node.kind() == "call_expression" && text_of(field(*node, "function"))? == "eval" {
            local_functions.clear();
            break;
        }
    }

    let mut declarations: Vec<Value> = Vec::new();
    let mut relations: Vec<Value> = Vec::new();
    let mut effects: Vec<Value> = Vec::new();
    for node in &nodes {
        let node = *node;
        let kind = node.kind();
        let mut name = text_of(field(node, "name"))?;
        let line = node.start_position().row + 1;
        let exported = parent_kind(node) == "export_statement";
        if matches!(kind, "class_declaration" | "interface_declaration") {
            let mut methods: Vec<String> = Vec::new();
            for member in descendants(field(node, "body")) {
                if matches!(member.kind(), "method_definition" | "method_signature") {
                    methods.push(text_of(field(member, "name"))?);
                }
            }
            declarations.push(json!({
                "name": name,
                "kind": if kind == "interface_declaration" { "interface" } else { "class" },
                "line": line,
                "methods": methods.iter().take(64).collect::<Vec<_>>(),
                "exported": exported,
            }));
            for clause in named_children(node) {
                if clause.kind() != "class_heritage" {
                    continue;
                }
                for group in named_children(clause) {
                    for base in named_children(group) {
                        if matches!(
                            base.kind(),
                            "identifier" | "type_identifier" | "generic_type"
                        ) {
                            relations.push(relation(
                                if group.kind() == "implements_clause" {
                                    "IMPLEMENTS"
                                } else {
                                    "EXTENDS"
                                },
                                &name,
                                &text_of(Some(base))?,
                                line,
                                "declared-heritage",
                                "observed",
                            ));
                        }
                    }
                }
            }
        }
        if matches!(
            kind,
            "function_declaration" | "method_definition" | "arrow_function"
        ) {
            if name.is_empty()
                && parent_kind(node) == "variable_declarator"
                && let Some(parent) = node.parent()
            {
                name = text_of(field(parent, "name"))?;
            }
            let mut injected: Vec<(String, String)> = Vec::new();
            if let Some(parameters) = field(node, "parameters") {
                for parameter in named_children(parameters) {
                    if let Some(annotation) = field(parameter, "type") {
                        let written = text_of(Some(annotation))?;
                        let stripped = written.trim_start_matches([':', ' ']);
                        injected
                            .push((text_of(field(parameter, "pattern"))?, identifier(stripped)));
                    }
                }
            }
            let shown = if name.is_empty() {
                "(anonymous)"
            } else {
                &name
            };
            declarations.push(json!({
                "name": shown,
                "kind": "function",
                "line": line,
                "parameters": injected
                    .iter()
                    .take(32)
                    .map(|(parameter, kind)| json!({"name": parameter, "type": kind}))
                    .collect::<Vec<_>>(),
                "exported": exported,
            }));
            for (_, target) in &injected {
                relations.push(relation(
                    "INJECTS",
                    shown,
                    target,
                    line,
                    "annotated-parameter",
                    "observed",
                ));
            }
        }
        if matches!(kind, "call_expression" | "new_expression") {
            let function = field(node, "function").or_else(|| field(node, "constructor"));
            let target = identifier(&text_of(function)?);
            let resolved = function.is_some_and(|found| found.kind() == "identifier")
                && local_functions.contains(&target);
            let constructs = kind == "new_expression";
            let relation_kind = if constructs {
                "CONSTRUCTS"
            } else if resolved {
                "CALLS"
            } else {
                "UNRESOLVED_CALL"
            };
            relations.push(relation(
                relation_kind,
                "module",
                &target,
                line,
                if resolved {
                    "unique-module-lexical-binding"
                } else if constructs {
                    "syntactic-construction"
                } else {
                    "dispatch-not-resolved"
                },
                if resolved || constructs {
                    "observed"
                } else {
                    "inferred"
                },
            ));
            if let Some(function) = function
                && function.kind() == "member_expression"
            {
                let receiver = text_of(field(function, "object"))?;
                let method = text_of(field(function, "property"))?;
                let dependency = aliases.get(&receiver).cloned().unwrap_or_default();
                let mut found = effect(&dependency, &method);
                if matches!(dependency.as_str(), "express" | "fastify")
                    && matches!(
                        method.as_str(),
                        "get" | "post" | "route" | "put" | "delete" | "patch"
                    )
                {
                    found = Some("api-route");
                }
                if let Some(found) = found {
                    effects.push(effect_fact(
                        found,
                        line,
                        &dependency,
                        "import-bound-call-syntax",
                    ));
                }
            }
        }
    }
    Ok(json!({
        "declarations": declarations,
        "relations": relations,
        "effects": effects,
        "parse_error": root.has_error(),
    }))
}
