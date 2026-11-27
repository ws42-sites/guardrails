//! Conservative, source-backed import and call relationship extraction.
//!
//! Port of `codegraph.relationships`. Python is read through a CPython-shaped
//! tree (see `pyast`); JS and TS support explicit relative imports only.
//! Unresolved or ambiguous calls are not represented as confirmed edges.

use crate::parsing::parse_tree;
use crate::posix_path::posix;
use crate::pyast::{self, Node, ParseFailure};
use crate::relationships_rust::{self, RustUse};
use crate::{EngineError, text};
use ruff_python_ast::{Expr, Stmt};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

pub use crate::relationships_rust::{CrateRoots, crate_roots};

pub const JS_EXTS: [&str; 6] = [".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs"];

/// An import fact as `references` records it, tagged as the Python dictionaries are.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind")]
pub enum ImportFact {
    #[serde(rename = "python_from")]
    PythonFrom {
        level: u32,
        module: Option<String>,
        names: Vec<String>,
        line: usize,
    },
    #[serde(rename = "python_import")]
    PythonImport { names: Vec<String>, line: usize },
    #[serde(rename = "relative")]
    Relative {
        specifier: String,
        line: usize,
        type_only: bool,
    },
    #[serde(rename = "rust_use")]
    RustUse(RustUse),
}

/// A call candidate: `(name, line, basis)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CallFact(pub String, pub usize, pub &'static str);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct References {
    pub imports: Vec<ImportFact>,
    pub calls: Vec<CallFact>,
}

/// A resolved import: `(target path, line, confidence)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedImport(pub String, pub usize, pub &'static str);

/// Anything that can say whether a repository path is known: a set or a mapping.
pub trait KnownPaths {
    fn has(&self, path: &str) -> bool;
}

impl KnownPaths for HashSet<String> {
    fn has(&self, path: &str) -> bool {
        self.contains(path)
    }
}

impl KnownPaths for BTreeSet<String> {
    fn has(&self, path: &str) -> bool {
        self.contains(path)
    }
}

impl<V> KnownPaths for HashMap<String, V> {
    fn has(&self, path: &str) -> bool {
        self.contains_key(path)
    }
}

impl<V> KnownPaths for BTreeMap<String, V> {
    fn has(&self, path: &str) -> bool {
        self.contains_key(path)
    }
}

/// `list(PurePosixPath(path).parent.parts)`.
fn parent_parts(path: &str) -> Vec<String> {
    let (root, tail) = posix(path);
    let mut parts: Vec<String> = Vec::new();
    if !root.is_empty() {
        parts.push(root.to_owned());
    }
    if root.is_empty() || !tail.is_empty() {
        let keep = tail.len().saturating_sub(1);
        parts.extend(tail[..keep].iter().map(|part| (*part).to_owned()));
    }
    parts
}

/// `str(PurePosixPath(path).with_suffix(""))`. CPython 3.12 does not re-normalise
/// the result, so a name like `..js` becomes `.`.
fn without_suffix(path: &str) -> String {
    let (root, tail) = posix(path);
    let Some((name, directories)) = tail.split_last() else {
        return if root.is_empty() {
            ".".into()
        } else {
            root.into()
        };
    };
    let trimmed = match name.rfind('.') {
        Some(dot) if dot > 0 && dot < name.len() - 1 => &name[..dot],
        _ => name,
    };
    let mut joined: Vec<&str> = directories.to_vec();
    joined.push(trimmed);
    let text = format!("{root}{}", joined.join("/"));
    if text.is_empty() { ".".into() } else { text }
}

/// Resolve `./x`, `../x` and `.` against `path` the way Node-style imports do.
pub fn relative_target(path: &str, specifier: &str, known: &dyn KnownPaths) -> Option<String> {
    if !specifier.starts_with('.') {
        return None;
    }
    let mut parts = parent_parts(path);
    for part in specifier.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other.to_owned()),
        }
    }
    let base = parts.join("/");
    let stem = if [".js", ".mjs", ".cjs"]
        .iter()
        .any(|ext| base.ends_with(ext))
    {
        without_suffix(&base)
    } else {
        base.clone()
    };
    let mut candidates = vec![base.clone()];
    candidates.extend(JS_EXTS.iter().map(|ext| format!("{base}{ext}")));
    candidates.extend(JS_EXTS.iter().map(|ext| format!("{base}/index{ext}")));
    candidates.extend([".ts", ".tsx"].iter().map(|ext| format!("{stem}{ext}")));
    candidates
        .into_iter()
        .find(|candidate| known.has(candidate))
}

fn type_only_statement(node: tree_sitter::Node<'_>, data: &[u8]) -> bool {
    let body = &data[node.start_byte()..node.end_byte()];
    // `bytes.lstrip()` strips ASCII whitespace including vertical tab.
    let start = body
        .iter()
        .position(|byte| !matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | b'\x0b' | b'\x0c'))
        .unwrap_or(body.len());
    let head = &body[start..];
    if head.starts_with(b"import type ") || head.starts_with(b"export type ") {
        return true;
    }
    let mut cursor = node.walk();
    let clause = node
        .named_children(&mut cursor)
        .find(|child| matches!(child.kind(), "import_clause" | "export_clause"));
    let Some(clause) = clause else {
        return false;
    };
    let mut cursor = clause.walk();
    let mut bindings: Vec<tree_sitter::Node<'_>> = clause.named_children(&mut cursor).collect();
    if bindings.len() == 1 && bindings[0].kind() == "named_imports" {
        let named = bindings[0];
        let mut inner = named.walk();
        bindings = named.named_children(&mut inner).collect();
    }
    !bindings.is_empty()
        && bindings.iter().all(|binding| {
            matches!(binding.kind(), "import_specifier" | "export_specifier")
                && binding.child(0).is_some_and(|first| first.kind() == "type")
        })
}

fn python_references(source: &str) -> Result<References, EngineError> {
    match pyast::with_module(source, |module| {
        let mut local_names: HashSet<&str> = HashSet::new();
        for node in module.walk() {
            if let Node::Stmt(Stmt::FunctionDef(function)) = node {
                local_names.insert(function.name.as_str());
            }
        }
        let mut result = References::default();
        for node in module.walk() {
            match node {
                Node::Stmt(Stmt::ImportFrom(statement)) => {
                    result.imports.push(ImportFact::PythonFrom {
                        level: statement.level,
                        module: statement.module.as_ref().map(|name| name.to_string()),
                        names: statement
                            .names
                            .iter()
                            .map(|alias| alias.name.to_string())
                            .collect(),
                        line: module.line(statement.range.start()),
                    });
                }
                Node::Stmt(Stmt::Import(statement)) => {
                    result.imports.push(ImportFact::PythonImport {
                        names: statement
                            .names
                            .iter()
                            .map(|alias| alias.name.to_string())
                            .collect(),
                        line: module.line(statement.range.start()),
                    });
                }
                Node::Expr(expr @ Expr::Call(call)) => {
                    if let Expr::Name(name) = call.func.as_ref()
                        && local_names.contains(name.id.as_str())
                    {
                        let visit = module.visit(Node::Expr(expr));
                        result.calls.push(CallFact(
                            name.id.to_string(),
                            visit.line.unwrap_or(0),
                            "local-name-candidate",
                        ));
                    }
                }
                _ => {}
            }
        }
        result
    }) {
        Ok(result) => Ok(result),
        Err(ParseFailure::Syntax(_)) => Ok(References::default()),
        Err(ParseFailure::Recursion) => Err(EngineError::Recursion),
    }
}

fn script_references(language: &str, source: &str) -> Result<References, EngineError> {
    let data = source.as_bytes();
    let tree = parse_tree(language, data)?;
    let mut imports = Vec::new();
    let mut pending = vec![tree.root_node()];
    while let Some(node) = pending.pop() {
        let mut source_node = None;
        match node.kind() {
            "import_statement" | "export_statement" => {
                source_node = node.child_by_field_name("source");
            }
            "call_expression" => {
                let function = node.child_by_field_name("function");
                let arguments = node.child_by_field_name("arguments");
                if let (Some(function), Some(arguments)) = (function, arguments)
                    && matches!(
                        &data[function.start_byte()..function.end_byte()],
                        b"import" | b"require"
                    )
                {
                    let mut cursor = arguments.walk();
                    let mut named = arguments.named_children(&mut cursor);
                    if let (Some(only), None) = (named.next(), named.next()) {
                        source_node = Some(only);
                    }
                }
            }
            _ => {}
        }
        if let Some(string) = source_node.filter(|string| string.kind() == "string") {
            let start = string.start_byte() + 1;
            let end = string.end_byte().saturating_sub(1);
            let raw = if start < end {
                &data[start..end]
            } else {
                &[][..]
            };
            let raw =
                std::str::from_utf8(raw).map_err(|error| EngineError::Parser(error.to_string()))?;
            if !raw.contains('\\') {
                imports.push(ImportFact::Relative {
                    specifier: raw.to_owned(),
                    line: node.start_position().row + 1,
                    type_only: type_only_statement(node, data),
                });
            }
        }
        let mut cursor = node.walk();
        let children: Vec<_> = node.named_children(&mut cursor).collect();
        pending.extend(children.into_iter().rev());
    }
    Ok(References {
        imports,
        calls: Vec::new(),
    })
}

/// Extract content-only facts once; resolve their paths in each branch later.
pub fn references(language: Option<&str>, source: &str) -> Result<References, EngineError> {
    match language {
        Some("python") => python_references(source),
        Some(language @ ("javascript" | "typescript" | "tsx")) => {
            script_references(language, source)
        }
        Some("rust") => Ok(References {
            imports: relationships_rust::references(source)?
                .into_iter()
                .map(ImportFact::RustUse)
                .collect(),
            calls: Vec::new(),
        }),
        _ => Ok(References::default()),
    }
}

/// Package-export aliases gathered from `package.json` manifests, in discovery order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Aliases {
    order: Vec<String>,
    entries: HashMap<String, Vec<(String, Value)>>,
}

impl Aliases {
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    pub fn get(&self, alias: &str) -> Option<&[(String, Value)]> {
        self.entries.get(alias).map(Vec::as_slice)
    }

    /// Aliases in the order they were first recorded.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &[(String, Value)])> {
        self.order
            .iter()
            .map(|alias| (alias.as_str(), self.entries[alias].as_slice()))
    }
}

fn package_name_ok(name: &str) -> bool {
    let (scope, rest) = match name.strip_prefix('@') {
        Some(tail) => match tail.split_once('/') {
            Some((scope, rest)) => (Some(scope), rest),
            None => return false,
        },
        None => (None, name),
    };
    let part = |text: &str| {
        !text.is_empty()
            && text
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    };
    scope.is_none_or(part) && part(rest) && !matches!(name, "." | "..")
}

/// Collect `exports` aliases from every `package.json` among `known`, where each
/// entry is a path and the `facts` object stored for it.
pub fn module_aliases(known: &[(String, Value)]) -> Result<Aliases, EngineError> {
    let mut aliases = Aliases::default();
    for (path, facts) in known {
        if !path.ends_with("package.json") {
            continue;
        }
        let malformed = || EngineError::InvalidInput(format!("malformed facts for {path}"));
        let facts = facts.as_object().ok_or_else(malformed)?;
        let architecture = match facts.get("architecture") {
            None => None,
            Some(value) => Some(value.as_object().ok_or_else(malformed)?),
        };
        let manifest = match architecture.and_then(|found| found.get("manifest")) {
            None => None,
            Some(value) => Some(value.as_object().ok_or_else(malformed)?),
        };
        let Some(manifest) = manifest else { continue };
        let Some(name) = manifest.get("name").and_then(Value::as_str) else {
            continue;
        };
        if !package_name_ok(name) {
            continue;
        }
        let exports = match manifest.get("exports") {
            None => continue,
            Some(value) => value.as_object().ok_or_else(malformed)?,
        };
        for (key, target) in exports {
            let suffix = if key == "." {
                String::new()
            } else if let Some(tail) = key.strip_prefix("./") {
                format!("/{tail}")
            } else {
                continue;
            };
            let alias = format!("{name}{suffix}");
            if !aliases.entries.contains_key(&alias) {
                aliases.order.push(alias.clone());
            }
            aliases
                .entries
                .entry(alias)
                .or_default()
                .push((path.clone(), target.clone()));
        }
    }
    Ok(aliases)
}

/// Resolve a bare package specifier through `exports` aliases, exact or one `*` pattern.
pub fn package_target(
    specifier: &str,
    known: &dyn KnownPaths,
    aliases: &Aliases,
) -> Option<String> {
    let keys: Vec<&str> = if aliases.get(specifier).is_some() {
        vec![specifier]
    } else {
        let mut matching: Vec<&str> = aliases
            .order
            .iter()
            .map(String::as_str)
            .filter(|key| {
                key.matches('*').count() == 1 && {
                    let (prefix, suffix) = key.split_once('*').expect("one star");
                    specifier.starts_with(prefix) && specifier.ends_with(suffix)
                }
            })
            .collect();
        matching.sort_by(|a, b| {
            text::char_len(b)
                .cmp(&text::char_len(a))
                .then_with(|| a.cmp(b))
        });
        matching.truncate(1);
        matching
    };
    let mut candidates: BTreeSet<String> = BTreeSet::new();
    for key in keys {
        let capture = match key.split_once('*') {
            None => "",
            Some((prefix, suffix)) => {
                let end = (!suffix.is_empty())
                    .then(|| text::char_len(specifier).saturating_sub(text::char_len(suffix)));
                text::char_slice(specifier, text::char_len(prefix), end)
            }
        };
        for (path, target) in aliases.get(key).unwrap_or_default() {
            let Some(target) = target.as_str() else {
                continue;
            };
            if !target.starts_with("./") || target.split('/').any(|part| part == "..") {
                continue;
            }
            if let Some(resolved) = relative_target(path, &target.replace('*', capture), known) {
                candidates.insert(resolved);
            }
        }
    }
    if candidates.len() == 1 {
        candidates.into_iter().next()
    } else {
        None
    }
}

/// Resolve extracted facts to known paths. Returns the confirmed imports and the
/// call candidates unchanged; whether a call is kept is the caller's decision.
pub fn resolve(
    path: &str,
    facts: &References,
    known: &dyn KnownPaths,
    aliases: Option<&Aliases>,
    crates: Option<&CrateRoots>,
) -> (Vec<ResolvedImport>, Vec<CallFact>) {
    let empty = Aliases::default();
    let aliases = aliases.filter(|found| !found.is_empty()).unwrap_or(&empty);
    let mut imports: Vec<ResolvedImport> = Vec::new();
    let parts = parent_parts(path);
    for item in &facts.imports {
        match item {
            ImportFact::Relative {
                specifier, line, ..
            } => {
                let relative = specifier.starts_with('.');
                let target = if relative {
                    relative_target(path, specifier, known)
                } else {
                    package_target(specifier, known, aliases)
                };
                if let Some(target) = target {
                    imports.push(ResolvedImport(
                        target,
                        *line,
                        if relative {
                            "resolved-relative"
                        } else {
                            "resolved-package-export"
                        },
                    ));
                }
            }
            ImportFact::RustUse(item) => {
                let empty = CrateRoots::new();
                let found = relationships_rust::rust_target(
                    path,
                    item,
                    &|candidate| known.has(candidate),
                    crates.unwrap_or(&empty),
                );
                if let Some((target, confidence)) = found {
                    let entry = ResolvedImport(target, item.line, confidence);
                    if entry.0 != path && !imports.contains(&entry) {
                        imports.push(entry);
                    }
                }
            }
            ImportFact::PythonFrom {
                level,
                module,
                names,
                line,
            } => {
                let level = *level as usize;
                if level > parts.len() {
                    continue;
                }
                let prefix: &[String] = if level > 0 {
                    &parts[..parts.len() - (level - 1)]
                } else {
                    &[]
                };
                let mut segments: Vec<&str> = prefix.iter().map(String::as_str).collect();
                if let Some(module) = module.as_deref().filter(|module| !module.is_empty()) {
                    segments.extend(module.split('.'));
                }
                let module = segments.join("/");
                let module_targets = [format!("{module}.py"), format!("{module}/__init__.py")];
                let confidence = if level > 0 {
                    "resolved-relative"
                } else {
                    "resolved-module"
                };
                for name in names {
                    let child = if module.is_empty() {
                        name.clone()
                    } else {
                        format!("{module}/{name}")
                    };
                    let candidates = [format!("{child}.py"), format!("{child}/__init__.py")];
                    let target = candidates
                        .iter()
                        .chain(module_targets.iter())
                        .find(|candidate| known.has(candidate));
                    if let Some(target) = target {
                        let entry = ResolvedImport(target.clone(), *line, confidence);
                        if !imports.contains(&entry) {
                            imports.push(entry);
                        }
                    }
                }
            }
            ImportFact::PythonImport { names, line } => {
                for name in names {
                    let base = name.replace('.', "/");
                    let candidates = [format!("{base}.py"), format!("{base}/__init__.py")];
                    if let Some(target) = candidates.iter().find(|candidate| known.has(candidate)) {
                        imports.push(ResolvedImport(target.clone(), *line, "resolved-module"));
                    }
                }
            }
        }
    }
    (imports, facts.calls.clone())
}

/// The language the indexer assigns to a path for relationship extraction.
fn language_of(path: &str) -> Option<&'static str> {
    if path.ends_with(".py") {
        Some("python")
    } else if path.ends_with(".tsx") {
        Some("tsx")
    } else if path.ends_with(".ts") {
        Some("typescript")
    } else if JS_EXTS.iter().any(|ext| path.ends_with(ext)) {
        Some("javascript")
    } else if path.ends_with(".rs") {
        Some("rust")
    } else {
        None
    }
}

/// Extract and resolve in one step, without package aliases or crate roots.
pub fn extract(
    path: &str,
    source: &str,
    known: &dyn KnownPaths,
) -> Result<(Vec<ResolvedImport>, Vec<CallFact>), EngineError> {
    Ok(resolve(
        path,
        &references(language_of(path), source)?,
        known,
        None,
        None,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known(paths: &[&str]) -> HashSet<String> {
        paths.iter().map(|path| (*path).to_owned()).collect()
    }

    #[test]
    fn relative_targets_try_extensions_and_index_files() {
        let paths = known(&["src/services/router.ts", "src/lib/index.tsx"]);
        assert_eq!(
            relative_target("src/index.ts", "./services/router.js", &paths).as_deref(),
            Some("src/services/router.ts")
        );
        assert_eq!(
            relative_target("src/index.ts", "./lib", &paths).as_deref(),
            Some("src/lib/index.tsx")
        );
        assert_eq!(relative_target("src/index.ts", "react", &paths), None);
        assert_eq!(relative_target("index.ts", "../x", &known(&["x.ts"])), None);
    }

    #[test]
    fn python_imports_and_local_calls() {
        let source = "import os.path\nfrom . import sibling\nfrom pkg.sub import thing\n\ndef helper():\n    pass\n\nhelper()\nother()\n";
        let facts = references(Some("python"), source).expect("parses");
        assert_eq!(facts.imports.len(), 3);
        assert_eq!(
            facts.calls,
            vec![CallFact("helper".into(), 8, "local-name-candidate")]
        );
        let paths = known(&["pkg/sibling.py", "pkg/sub/thing.py", "os/path.py"]);
        let (resolved, _) = resolve("pkg/main.py", &facts, &paths, None, None);
        let targets: Vec<&str> = resolved.iter().map(|entry| entry.0.as_str()).collect();
        assert_eq!(
            targets,
            ["os/path.py", "pkg/sibling.py", "pkg/sub/thing.py"]
        );
    }

    #[test]
    fn syntax_errors_yield_no_facts_and_deep_nesting_is_an_error() {
        assert_eq!(
            references(Some("python"), "def (:").expect("ok"),
            References::default()
        );
        let deep = format!("x = {}1", "- ".repeat(5000));
        assert_eq!(
            references(Some("python"), &deep),
            Err(EngineError::Recursion)
        );
    }

    #[test]
    fn script_imports_record_type_only_and_skip_dynamic_specifiers() {
        let source = "import type { A } from './a';\nimport { type B, type C } from './b';\nimport D from './d';\nconst e = require('./e');\nconst f = import(`./f`);\nconst g = require(name);\n";
        let facts = references(Some("typescript"), source).expect("parses");
        let flags: Vec<(String, bool)> = facts
            .imports
            .iter()
            .map(|fact| match fact {
                ImportFact::Relative {
                    specifier,
                    type_only,
                    ..
                } => (specifier.clone(), *type_only),
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(
            flags,
            [
                ("./a".into(), true),
                ("./b".into(), true),
                ("./d".into(), false),
                ("./e".into(), false)
            ]
        );
    }

    #[test]
    fn package_exports_resolve_exact_and_wildcard_aliases() {
        let manifest = serde_json::json!({"architecture": {"manifest": {
            "name": "@scope/pkg",
            "exports": {".": "./src/index.ts", "./features/*": "./src/features/*.ts", "./bad": "../x.ts"}
        }}});
        let aliases =
            module_aliases(&[("packages/pkg/package.json".into(), manifest)]).expect("aliases");
        let paths = known(&[
            "packages/pkg/src/index.ts",
            "packages/pkg/src/features/a.ts",
        ]);
        assert_eq!(
            package_target("@scope/pkg", &paths, &aliases).as_deref(),
            Some("packages/pkg/src/index.ts")
        );
        assert_eq!(
            package_target("@scope/pkg/features/a", &paths, &aliases).as_deref(),
            Some("packages/pkg/src/features/a.ts")
        );
        assert_eq!(package_target("@scope/pkg/bad", &paths, &aliases), None);
    }
}
