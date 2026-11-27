//! Parse source data without importing or executing connected repositories.
//!
//! Port of `codegraph.parsing`. The grammars are the ones the Python service
//! loads through `tree_sitter_language_pack`.

use crate::EngineError;
use crate::parsing_rust;
use crate::text;
use serde::Serialize;
use tree_sitter::{Language, Node, Parser, Tree};

const SYMBOL_TYPES: &[&str] = &[
    "function_definition",
    "class_definition",
    "function_declaration",
    "class_declaration",
    "interface_declaration",
    "method_definition",
    "method_declaration",
    "type_alias_declaration",
    "variable_declarator",
    "import_statement",
    "import_from_statement",
    "import_declaration",
    "function_item",
    "function_signature_item",
    "struct_item",
    "enum_item",
    "union_item",
    "trait_item",
    "mod_item",
    "type_item",
    "const_item",
    "static_item",
    "macro_definition",
];

/// One symbol; serialises as the golden fixtures record it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Symbol {
    pub kind: String,
    pub name: String,
    pub line: usize,
    pub parent: Option<String>,
}

fn grammar(language: &str) -> Result<Language, EngineError> {
    match language {
        "python" => Ok(tree_sitter_python::LANGUAGE.into()),
        "javascript" => Ok(tree_sitter_javascript::LANGUAGE.into()),
        "typescript" => Ok(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
        "tsx" => Ok(tree_sitter_typescript::LANGUAGE_TSX.into()),
        "rust" => Ok(tree_sitter_rust::LANGUAGE.into()),
        other => Err(EngineError::UnsupportedLanguage(other.to_owned())),
    }
}

/// Parse `content` with the grammar for `language`.
pub fn parse_tree(language: &str, content: &[u8]) -> Result<Tree, EngineError> {
    let mut parser = Parser::new();
    parser
        .set_language(&grammar(language)?)
        .map_err(|error| EngineError::Parser(error.to_string()))?;
    parser
        .parse(content, None)
        .ok_or_else(|| EngineError::Parser("the parser produced no tree".into()))
}

/// Whether the grammar recovered from a syntax error (`root.has_error`).
pub fn has_error(language: &str, content: &[u8]) -> Result<bool, EngineError> {
    Ok(parse_tree(language, content)?.root_node().has_error())
}

/// Text of a node's source bytes, decoded as Python's `errors="replace"` does.
pub fn node_text(node: Node<'_>, content: &[u8]) -> String {
    text::decode_replace(&content[node.start_byte()..node.end_byte()])
}

/// Symbols in depth-first source order; `"text"` has none.
pub fn symbols(language: &str, content: &[u8]) -> Result<Vec<Symbol>, EngineError> {
    if language == "text" {
        return Ok(Vec::new());
    }
    let tree = parse_tree(language, content)?;
    let mut result = Vec::new();
    let mut pending: Vec<(Node<'_>, Option<String>)> = vec![(tree.root_node(), None)];
    while let Some((node, mut parent)) = pending.pop() {
        let kind = node.kind();
        if language == "rust" && parsing_rust::IMPORT_TYPES.contains(&kind) {
            let raw = node_text(node, content);
            result.push(Symbol {
                kind: "import".to_owned(),
                name: text::char_slice(&raw, 0, Some(300)).to_owned(),
                line: node.start_position().row + 1,
                parent: parent.clone(),
            });
        } else if language == "rust" && kind == "impl_item" {
            // An impl block is not a definition; its methods belong to the type.
            if let Some(owner) = parsing_rust::impl_owner(node, content) {
                parent = Some(owner);
            }
        } else if SYMBOL_TYPES.contains(&kind) {
            let name_node = node.child_by_field_name("name").filter(|name| {
                matches!(
                    name.kind(),
                    "identifier" | "type_identifier" | "property_identifier"
                )
            });
            if let Some(name_node) = name_node {
                let name = node_text(name_node, content);
                let mut symbol_kind = kind;
                if kind == "variable_declarator"
                    && node
                        .child_by_field_name("value")
                        .is_some_and(|value| value.kind() == "arrow_function")
                {
                    symbol_kind = "arrow_function";
                }
                result.push(Symbol {
                    kind: symbol_kind.to_owned(),
                    name: name.clone(),
                    line: node.start_position().row + 1,
                    parent: parent.clone(),
                });
                parent = Some(name);
            } else if kind.starts_with("import") {
                let raw = node_text(node, content);
                result.push(Symbol {
                    kind: "import".to_owned(),
                    name: text::char_slice(&raw, 0, Some(300)).to_owned(),
                    line: node.start_position().row + 1,
                    parent: parent.clone(),
                });
            }
        }
        let mut cursor = node.walk();
        let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
        for child in children.into_iter().rev() {
            pending.push((child, parent.clone()));
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(language: &str, source: &str) -> Vec<(String, String, usize, Option<String>)> {
        symbols(language, source.as_bytes())
            .expect("parses")
            .into_iter()
            .map(|s| (s.kind, s.name, s.line, s.parent))
            .collect()
    }

    #[test]
    fn python_classes_and_methods_keep_their_parent() {
        let found = names(
            "python",
            "class A:\n    def run(self):\n        pass\n\ndef top():\n    pass\n",
        );
        assert_eq!(found[0], ("class_definition".into(), "A".into(), 1, None));
        assert_eq!(
            found[1],
            (
                "function_definition".into(),
                "run".into(),
                2,
                Some("A".into())
            )
        );
        assert_eq!(
            found[2],
            ("function_definition".into(), "top".into(), 5, None)
        );
    }

    #[test]
    fn typescript_arrow_functions_and_imports() {
        let found = names(
            "typescript",
            "import { a } from './a';\nconst f = () => 1;\ninterface I {}\n",
        );
        assert_eq!(found[0].0, "import");
        assert_eq!(found[0].1, "import { a } from './a';");
        assert_eq!(found[1].0, "arrow_function");
        assert_eq!(found[2].0, "interface_declaration");
    }

    #[test]
    fn rust_methods_belong_to_the_type_their_impl_names() {
        let found = names(
            "rust",
            "struct S;\nimpl S {\n    fn run(&self) {}\n}\nimpl<T> Tr for Gen<T> {\n    fn go(&self) {}\n}\nimpl crate::a::Far {\n    fn near(&self) {}\n}\n",
        );
        let owners: Vec<(&str, Option<&str>)> = found
            .iter()
            .filter(|row| row.0 == "function_item")
            .map(|row| (row.1.as_str(), row.3.as_deref()))
            .collect();
        assert_eq!(
            owners,
            [
                ("run", Some("S")),
                ("go", Some("Gen")),
                ("near", Some("Far"))
            ]
        );
    }

    #[test]
    fn rust_imports_keep_their_text_and_the_rest_are_definitions() {
        let found = names(
            "rust",
            "use std::fmt;\nextern crate alloc;\nmacro_rules! m { () => {}; }\nconst N: u8 = 1;\n",
        );
        assert_eq!(found[0], ("import".into(), "use std::fmt;".into(), 1, None));
        assert_eq!(found[1].1, "extern crate alloc;");
        assert_eq!(found[2].0, "macro_definition");
        assert_eq!(found[3].0, "const_item");
    }

    #[test]
    fn a_rust_impl_for_an_unnamed_type_keeps_the_outer_owner() {
        let found = names(
            "rust",
            "mod m {\n    impl Tr for (u8, u8) {\n        fn f(&self) {}\n    }\n}\n",
        );
        assert_eq!(found[1].3.as_deref(), Some("m"));
    }

    #[test]
    fn text_has_no_symbols_and_unknown_languages_are_refused() {
        assert!(symbols("text", b"anything").expect("ok").is_empty());
        assert!(symbols("go", b"package x").is_err());
    }

    #[test]
    fn invalid_utf8_names_are_replaced_not_rejected() {
        let found = symbols("python", b"def f\xff():\n    pass\n").expect("parses");
        assert!(found.iter().all(|symbol| !symbol.name.is_empty()));
    }
}
