//! The Python half of `architecture_structure._python`.

use super::PyError;
use super::structure_facts::{Counter, effect, effect_fact, identifier, relation, sql_effects};
use crate::py_unparse::{first_segment, unparse};
use crate::pyast::{self, Node, PyModule};
use ruff_python_ast::{Expr, ExprContext, Stmt};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap, HashSet};

/// Facts for one source, or `None` where `ast.parse` raises `SyntaxError` or `RecursionError`.
pub fn python_facts(source: &str) -> Result<Option<Value>, PyError> {
    Ok(pyast::with_module(source, python_module).ok())
}

fn string_argument(call: &ruff_python_ast::ExprCall) -> Option<&str> {
    match call.arguments.args.first() {
        Some(Expr::StringLiteral(literal)) => Some(literal.value.to_str()),
        _ => None,
    }
}

fn dotted_target(base: &str, aliases: &HashMap<String, String>) -> String {
    // `aliases.get(b.split(".")[0], "") + ("." + b.split(".", 1)[1] if "." in b else "")`
    let mut out = aliases
        .get(first_segment(base))
        .cloned()
        .unwrap_or_default();
    if let Some((_, rest)) = base.split_once('.') {
        out.push('.');
        out.push_str(rest);
    }
    out
}

fn python_module(module: &PyModule) -> Value {
    let nodes: Vec<Node<'_>> = module.walk().collect();
    let line_of = |node: Node<'_>| module.visit(node).line.unwrap_or(0);
    let mut bindings = Counter::default();
    let mut aliases: HashMap<String, String> = HashMap::new();

    for node in &nodes {
        match *node {
            Node::Stmt(Stmt::FunctionDef(function)) => bindings.bump(function.name.as_str()),
            Node::Stmt(Stmt::ClassDef(class)) => bindings.bump(class.name.as_str()),
            Node::Expr(Expr::Name(name)) if name.ctx == ExprContext::Store => {
                bindings.bump(name.id.as_str());
            }
            Node::Arg(parameter) => bindings.bump(parameter.name.as_str()),
            Node::Stmt(Stmt::Import(statement)) => {
                for alias in &statement.names {
                    let local = alias.asname.as_ref().map_or_else(
                        || first_segment(alias.name.as_str()).to_owned(),
                        |name| name.as_str().to_owned(),
                    );
                    bindings.bump(&local);
                    aliases.insert(local, alias.name.as_str().to_owned());
                }
            }
            Node::Stmt(Stmt::ImportFrom(statement)) => {
                let origin = statement.module.as_ref().map_or("", |name| name.as_str());
                for alias in &statement.names {
                    let local = alias.asname.as_ref().map_or_else(
                        || first_segment(alias.name.as_str()).to_owned(),
                        |name| name.as_str().to_owned(),
                    );
                    bindings.bump(&local);
                    aliases.insert(local, format!("{origin}.{}", alias.name.as_str()));
                }
            }
            _ => {}
        }
    }

    // A name bound once to the result of a call on an imported name inherits its dependency.
    for node in &nodes {
        let (call, targets): (Option<&ruff_python_ast::ExprCall>, Vec<&Expr>) = match *node {
            Node::Stmt(Stmt::Assign(statement)) => (
                match statement.value.as_ref() {
                    Expr::Call(call) => Some(call),
                    _ => None,
                },
                statement.targets.iter().collect(),
            ),
            Node::Stmt(Stmt::AnnAssign(statement)) => (
                match statement.value.as_deref() {
                    Some(Expr::Call(call)) => Some(call),
                    _ => None,
                },
                vec![statement.target.as_ref()],
            ),
            _ => continue,
        };
        let Some(call) = call else { continue };
        let function = unparse(module.source(), &call.func);
        let dependency = aliases
            .get(first_segment(&function))
            .cloned()
            .unwrap_or_default();
        for target in targets {
            if let Expr::Name(name) = target
                && bindings.get(name.id.as_str()) == 1
                && !dependency.is_empty()
            {
                aliases.insert(name.id.as_str().to_owned(), dependency.clone());
            }
        }
    }

    let mut module_functions: HashSet<String> = HashSet::new();
    let mut module_classes: HashSet<String> = HashSet::new();
    for statement in &module.module().body {
        match statement {
            Stmt::FunctionDef(function) if bindings.get(function.name.as_str()) == 1 => {
                module_functions.insert(function.name.as_str().to_owned());
            }
            Stmt::ClassDef(class) if bindings.get(class.name.as_str()) == 1 => {
                module_classes.insert(class.name.as_str().to_owned());
            }
            _ => {}
        }
    }
    let dynamic_binding = nodes.iter().any(|node| {
        matches!(
            *node,
            Node::Expr(Expr::Call(call))
                if matches!(call.func.as_ref(), Expr::Name(name) if matches!(name.id.as_str(), "exec" | "eval"))
        )
    });
    if dynamic_binding {
        module_functions.clear();
        module_classes.clear();
    }

    let mut declarations: Vec<Value> = Vec::new();
    let mut relations: Vec<Value> = Vec::new();
    let mut effects: Vec<Value> = Vec::new();

    for node in &nodes {
        match *node {
            Node::Stmt(Stmt::ClassDef(class)) => {
                let line = line_of(*node);
                let bases: Vec<String> = class
                    .arguments
                    .as_ref()
                    .map(|arguments| {
                        arguments
                            .args
                            .iter()
                            .map(|base| identifier(&unparse(module.source(), base)))
                            .collect()
                    })
                    .unwrap_or_default();
                let interface = bases.iter().any(|base| {
                    matches!(
                        dotted_target(base, &aliases).as_str(),
                        "typing.Protocol" | "typing_extensions.Protocol" | "abc.ABC"
                    )
                });
                let methods: Vec<&str> = class
                    .body
                    .iter()
                    .filter_map(|statement| match statement {
                        Stmt::FunctionDef(function) => Some(function.name.as_str()),
                        _ => None,
                    })
                    .take(64)
                    .collect();
                declarations.push(json!({
                    "name": class.name.as_str(),
                    "kind": if interface { "interface" } else { "class" },
                    "line": line,
                    "bases": bases.iter().take(32).collect::<Vec<_>>(),
                    "methods": methods,
                    "exported": !class.name.as_str().starts_with('_'),
                }));
                for base in &bases {
                    relations.push(relation(
                        "EXTENDS",
                        class.name.as_str(),
                        base,
                        line,
                        "declared-base",
                        "observed",
                    ));
                }
            }
            Node::Stmt(Stmt::FunctionDef(function)) => {
                let line = line_of(*node);
                let parameters = &function.parameters;
                let injected: Vec<(String, String)> = parameters
                    .posonlyargs
                    .iter()
                    .chain(parameters.args.iter())
                    .chain(parameters.kwonlyargs.iter())
                    .filter_map(|item| {
                        let annotation = item.parameter.annotation.as_ref()?;
                        let name = item.parameter.name.as_str();
                        (!matches!(name, "self" | "cls")).then(|| {
                            (
                                name.to_owned(),
                                identifier(&unparse(module.source(), annotation)),
                            )
                        })
                    })
                    .collect();
                let constructed: BTreeSet<String> = module
                    .walk_from(*node)
                    .filter_map(|inner| match inner {
                        Node::Stmt(Stmt::Return(statement)) => match statement.value.as_deref() {
                            Some(Expr::Call(call)) => match call.func.as_ref() {
                                Expr::Name(name) if module_classes.contains(name.id.as_str()) => {
                                    Some(name.id.as_str().to_owned())
                                }
                                _ => None,
                            },
                            _ => None,
                        },
                        _ => None,
                    })
                    .collect();
                declarations.push(json!({
                    "name": function.name.as_str(),
                    "kind": "function",
                    "line": line,
                    "parameters": injected
                        .iter()
                        .take(32)
                        .map(|(name, kind)| json!({"name": name, "type": kind}))
                        .collect::<Vec<_>>(),
                    "returns_constructed": constructed.iter().take(16).collect::<Vec<_>>(),
                    "exported": !function.name.as_str().starts_with('_'),
                }));
                for (_, kind) in &injected {
                    relations.push(relation(
                        "INJECTS",
                        function.name.as_str(),
                        kind,
                        line,
                        "annotated-parameter",
                        "observed",
                    ));
                }
            }
            Node::Expr(Expr::Call(call)) => {
                let line = line_of(*node);
                let name = identifier(&unparse(module.source(), &call.func));
                let is_name = matches!(call.func.as_ref(), Expr::Name(_));
                let resolved = is_name && module_functions.contains(&name);
                let constructs = is_name && module_classes.contains(&name);
                let (kind, resolution, origin) = if constructs {
                    ("CONSTRUCTS", "syntactic-construction", "observed")
                } else if resolved {
                    ("CALLS", "unique-module-lexical-binding", "observed")
                } else {
                    ("UNRESOLVED_CALL", "dispatch-not-resolved", "inferred")
                };
                relations.push(relation(kind, "module", &name, line, resolution, origin));
                if let Expr::Attribute(attribute) = call.func.as_ref() {
                    let receiver = unparse(module.source(), &attribute.value);
                    let dependency = aliases
                        .get(first_segment(&receiver))
                        .cloned()
                        .unwrap_or_default();
                    let method = attribute.attr.as_str();
                    let mut found = effect(&dependency, method);
                    if (dependency.starts_with("fastapi") || dependency.starts_with("flask"))
                        && matches!(
                            method,
                            "route" | "get" | "post" | "put" | "delete" | "patch"
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
                    if found == Some("persistence-call")
                        && let Some(sql) = string_argument(call)
                    {
                        effects.extend(sql_effects(sql, line, &dependency));
                    }
                }
                let dependency = aliases.get(&name).cloned().unwrap_or_default();
                if matches!(
                    dependency.as_str(),
                    "flask.render_template" | "django.shortcuts.render"
                ) {
                    effects.push(effect_fact(
                        "view-render",
                        line,
                        &dependency,
                        "import-bound-call-syntax",
                    ));
                }
            }
            Node::Stmt(Stmt::With(with)) => {
                let line = line_of(*node);
                let transactions = with
                    .items
                    .iter()
                    .filter_map(|item| match &item.context_expr {
                        Expr::Call(call) => match call.func.as_ref() {
                            Expr::Attribute(attribute)
                                if attribute.attr.as_str() == "transaction" =>
                            {
                                Some(attribute)
                            }
                            _ => None,
                        },
                        _ => None,
                    });
                for transaction in transactions {
                    let receiver = unparse(module.source(), &transaction.value);
                    let dependency = aliases.get(&receiver).cloned().unwrap_or_default();
                    let calls: Vec<&ruff_python_ast::ExprCall> = with
                        .body
                        .iter()
                        .filter_map(|statement| match statement {
                            Stmt::Expr(expression) => match expression.value.as_ref() {
                                Expr::Call(call) => Some(call),
                                _ => None,
                            },
                            _ => None,
                        })
                        .filter(|call| match call.func.as_ref() {
                            Expr::Attribute(attribute) => {
                                unparse(module.source(), &attribute.value) == receiver
                                    && matches!(attribute.attr.as_str(), "execute" | "query")
                                    && string_argument(call).is_some()
                            }
                            _ => false,
                        })
                        .collect();
                    let known = ["psycopg", "sqlite3", "sqlalchemy"]
                        .iter()
                        .any(|prefix| dependency.starts_with(prefix));
                    if known && calls.len() >= 2 {
                        let kinds_of = |call: &ruff_python_ast::ExprCall| -> Vec<String> {
                            let literal = string_argument(call).unwrap_or_default();
                            sql_effects(literal, 0, &dependency)
                                .iter()
                                .map(|item| item["kind"].as_str().unwrap_or_default().to_owned())
                                .collect()
                        };
                        let per_call: Vec<Vec<String>> =
                            calls.iter().map(|call| kinds_of(call)).collect();
                        let has = |kinds: &Vec<String>, kind: &str| kinds.iter().any(|k| k == kind);
                        let outbox = per_call.iter().any(|kinds| has(kinds, "outbox-insert"));
                        let non_outbox_write = per_call.iter().any(|kinds| {
                            has(kinds, "database-write") && !has(kinds, "outbox-insert")
                        });
                        if outbox && non_outbox_write {
                            effects.push(effect_fact(
                                "atomic-outbox-candidate",
                                line,
                                &dependency,
                                "same-receiver-transaction-scope",
                            ));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    json!({"declarations": declarations, "relations": relations, "effects": effects})
}
