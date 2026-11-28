//! `architecture_structure`, and the `ast.unparse` it leans on.

use super::{outcome, parse, shrink, source_bytes};
use crate::Mismatches;
use ropeline_engine::architecture::structure::{self, extract_structure};
use ropeline_engine::py_unparse::unparse;
use ropeline_engine::pyast;
use ruff_python_ast::Stmt;
use serde_json::{Value, json};

fn cases() -> Value {
    parse(include_str!("structure.json"))
}

fn wanted() -> Value {
    parse(include_str!("../expected/architecture-structure.json"))
}

#[test]
fn structure_matches_python() {
    let wanted = wanted();
    let mut mismatches = Mismatches::default();
    for case in cases()["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("id");
        let actual = shrink(outcome(extract_structure(
            case["language"].as_str().expect("language"),
            &source_bytes(case),
        )));
        mismatches.check(&format!("structure/{id}"), &actual, &wanted["cases"][id]);
    }
    mismatches.check(
        "structure/constants",
        &json!({"VERSION": structure::VERSION, "LIMIT": 512}),
        &wanted["constants"],
    );
    mismatches.finish("structure");
}

/// `ast.unparse(ast.parse(text).body[0].value)`, or `None` where the text is not a module.
fn unparsed(text: &str) -> Option<String> {
    pyast::with_module(text, |module| match module.module().body.first() {
        Some(Stmt::Expr(statement)) => Some(unparse(module.source(), &statement.value)),
        _ => None,
    })
    .ok()
    .flatten()
}

#[test]
fn unparse_matches_python() {
    let wanted = wanted();
    let mut mismatches = Mismatches::default();
    let expressions = cases()["expressions"].clone();
    for (index, text) in expressions.as_array().expect("list").iter().enumerate() {
        let text = text.as_str().expect("text");
        let actual = match unparsed(text) {
            Some(shown) => json!({"value": shown}),
            None => json!({"error": "SyntaxError"}),
        };
        mismatches.check(
            &format!("unparse/{index} {text:?}"),
            &actual,
            &wanted["unparse"][index],
        );
    }
    mismatches.finish("unparse");
}
