//! Rust `use` paths, followed by Cargo file layout. Port of the Rust half of
//! `codegraph.relationships`.
//!
//! Nothing here asks Cargo anything. A `use` path is matched against where files
//! sit in the snapshot, and a path that matches no file makes no link. `mod`
//! declarations, macros and paths written inline are not followed.

use crate::parsing::parse_tree;
use crate::posix_path::{parts, stem};
use crate::{EngineError, text};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use tree_sitter::Node;

const PATH_ROOTS: [&str; 3] = ["crate", "self", "super"];
const TARGET_DIRS: [&str; 3] = ["tests", "examples", "benches"];
/// Crates that ship with the toolchain; a repository folder never stands in for them.
const BUILTIN_CRATES: [&str; 5] = ["std", "core", "alloc", "proc_macro", "test"];
const MAX_SEGMENTS: usize = 32;
const MAX_SEGMENT_LENGTH: usize = 128;
/// How many `use` tree levels Python follows before it raises `RecursionError`.
///
/// Python's own limit is 1000 frames, shared with whoever called it, so the exact
/// depth moves with the caller. 994 is what a shallow caller measures.
const MAX_USE_FRAMES: usize = 994;

/// Facts about `Cargo.toml` files, keyed by the crate name `use` paths spell.
pub type CrateRoots = HashMap<String, Vec<String>>;

/// One path a `use` or `extern crate` line names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RustUse {
    pub path: Vec<String>,
    pub line: usize,
    /// How many inline `mod name { ... }` blocks surround the line.
    pub depth: usize,
    /// Written with a leading `::`, or `extern crate`: the first name is a crate.
    #[serde(rename = "extern", skip_serializing_if = "is_false")]
    pub external: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn named_children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

fn source_text(node: Node<'_>, data: &[u8]) -> String {
    text::decode_replace(&data[node.start_byte()..node.end_byte()])
}

/// Flatten `a::b::c` into segments; a leading `::` is kept as an empty first one.
fn scoped_path(node: Option<Node<'_>>, data: &[u8]) -> Option<Vec<String>> {
    let mut parts = Vec::new();
    let mut node = node;
    while let Some(current) = node.filter(|found| found.kind() == "scoped_identifier") {
        parts.push(source_text(current.child_by_field_name("name")?, data));
        node = current.child_by_field_name("path");
        if node.is_none() {
            parts.push(String::new());
            parts.reverse();
            return Some(parts);
        }
    }
    let node =
        node.filter(|found| found.kind() == "identifier" || PATH_ROOTS.contains(&found.kind()))?;
    parts.push(source_text(node, data));
    parts.reverse();
    Some(parts)
}

/// List the plain paths one `use` tree names; unusual syntax names nothing.
fn tree_paths(
    node: Option<Node<'_>>,
    data: &[u8],
    prefix: &[String],
    frames: usize,
) -> Result<Vec<Vec<String>>, EngineError> {
    if frames >= MAX_USE_FRAMES {
        return Err(EngineError::Nesting);
    }
    let Some(node) = node else {
        return Ok(Vec::new());
    };
    let kind = node.kind();
    if matches!(kind, "identifier" | "scoped_identifier") || PATH_ROOTS.contains(&kind) {
        if kind == "self" && !prefix.is_empty() {
            return Ok(vec![prefix.to_vec()]);
        }
        return Ok(scoped_path(Some(node), data)
            .map(|path| vec![[prefix, path.as_slice()].concat()])
            .unwrap_or_default());
    }
    match kind {
        "use_as_clause" => tree_paths(node.child_by_field_name("path"), data, prefix, frames + 1),
        "use_wildcard" => match named_children(node).first() {
            None if prefix.is_empty() => Ok(Vec::new()),
            None => Ok(vec![prefix.to_vec()]),
            Some(first) => tree_paths(Some(*first), data, prefix, frames + 1),
        },
        "use_list" => {
            let mut found = Vec::new();
            for child in named_children(node) {
                found.extend(tree_paths(Some(child), data, prefix, frames + 1)?);
            }
            Ok(found)
        }
        "scoped_use_list" => {
            let base = match node.child_by_field_name("path") {
                Some(head) => scoped_path(Some(head), data),
                None => Some(vec![String::new()]),
            };
            let Some(base) = base else {
                return Ok(Vec::new());
            };
            let full = [prefix, base.as_slice()].concat();
            tree_paths(node.child_by_field_name("list"), data, &full, frames + 1)
        }
        _ => Ok(Vec::new()),
    }
}

/// `(?:r#)?[A-Za-z_][A-Za-z0-9_]{0,127}`, whole string.
fn segment_ok(part: &str) -> bool {
    let name = part.strip_prefix("r#").unwrap_or(part);
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == b'_')
        && name.len() <= MAX_SEGMENT_LENGTH
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// Every `use` and `extern crate` path in a source, in source order.
pub fn references(source: &str) -> Result<Vec<RustUse>, EngineError> {
    let data = source.as_bytes();
    let tree = parse_tree("rust", data)?;
    let mut imports = Vec::new();
    let mut pending = vec![(tree.root_node(), 0usize)];
    while let Some((node, depth)) = pending.pop() {
        let mut paths: Vec<Vec<String>> = Vec::new();
        if node.kind() == "use_declaration" {
            paths = tree_paths(node.child_by_field_name("argument"), data, &[], 0)?;
        } else if node.kind() == "extern_crate_declaration"
            && let Some(name) = node
                .child_by_field_name("name")
                .filter(|found| found.kind() == "identifier")
        {
            paths = vec![vec![String::new(), source_text(name, data)]];
        }
        for path in paths {
            let external = path.first().is_some_and(String::is_empty);
            let raw = if external { &path[1..] } else { &path[..] };
            if raw.is_empty()
                || raw.len() > MAX_SEGMENTS
                || !raw.iter().all(|part| segment_ok(part))
            {
                continue;
            }
            imports.push(RustUse {
                path: raw
                    .iter()
                    .map(|part| part.strip_prefix("r#").unwrap_or(part).to_owned())
                    .collect(),
                line: node.start_position().row + 1,
                depth,
                external,
            });
        }
        // A `use` inside `mod name { ... }` is relative to that inline module.
        let inline = node.kind() == "mod_item" && node.child_by_field_name("body").is_some();
        let below = depth + usize::from(inline);
        pending.extend(
            named_children(node)
                .into_iter()
                .rev()
                .map(|child| (child, below)),
        );
    }
    Ok(imports)
}

fn file_name(path: &str) -> &str {
    path.split('/')
        .rfind(|part| !part.is_empty() && *part != ".")
        .unwrap_or("")
}

/// `[A-Za-z0-9_-]{1,64}`, whole string.
fn crate_name_ok(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

/// Cargo library crates in one snapshot, keyed by the name `use` paths spell.
///
/// `manifests` holds each file's path and its stored `facts`; `known` says which
/// paths exist. A manifest whose stored facts have the wrong shape is an error,
/// where Python raises `AttributeError`.
pub fn crate_roots(
    manifests: &[(String, Value)],
    known: &dyn Fn(&str) -> bool,
) -> Result<CrateRoots, EngineError> {
    let mut crates = CrateRoots::new();
    for (path, facts) in manifests {
        if file_name(path) != "Cargo.toml" {
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
        let Some(name) = manifest
            .and_then(|found| found.get("name"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        let directory = &path[..path.len() - "Cargo.toml".len()];
        if crate_name_ok(name) && known(&format!("{directory}src/lib.rs")) {
            crates
                .entry(name.replace('-', "_"))
                .or_default()
                .push(format!("{directory}src"));
        }
    }
    Ok(crates)
}

/// Where a file sits in its Cargo package: the root's folder and file, and the
/// folder its own child modules live in.
struct Placement {
    root_dir: String,
    root_file: Option<String>,
    module_dir: String,
}

fn join_dir(items: &[&str]) -> String {
    items.join("/")
}

/// Place a file in its Cargo package by layout.
fn placement(path: &str, known: &dyn Fn(&str) -> bool) -> Option<Placement> {
    let parts = parts(path);
    let size = (0..parts.len()).rev().find(|size| {
        let manifest: Vec<&str> = parts[..*size]
            .iter()
            .map(String::as_str)
            .chain(["Cargo.toml"])
            .collect();
        known(&join_dir(&manifest))
    })?;
    let inner: Vec<&str> = parts[size..].iter().map(String::as_str).collect();
    let join = |items: &[&str]| -> String {
        let all: Vec<&str> = parts[..size]
            .iter()
            .map(String::as_str)
            .chain(items.iter().copied())
            .collect();
        join_dir(&all)
    };
    let (root_dir, mut root_file): (String, Option<String>);
    if inner[0] == "src" && inner.get(1) == Some(&"bin") && inner.len() > 2 {
        // Every file or directory under src/bin is its own program.
        let single = inner.len() == 3;
        root_dir = if single {
            join(&["src", "bin"])
        } else {
            join(&["src", "bin", inner[2]])
        };
        root_file = Some(if single {
            path.to_owned()
        } else {
            format!("{root_dir}/main.rs")
        });
    } else if inner[0] == "src" && inner.len() > 1 {
        root_dir = join(&["src"]);
        root_file = if inner == ["src", "main.rs"] {
            Some(path.to_owned())
        } else {
            [join(&["src", "lib.rs"]), join(&["src", "main.rs"])]
                .into_iter()
                .find(|candidate| known(candidate))
        };
    } else if TARGET_DIRS.contains(&inner[0]) && inner.len() == 2 {
        root_dir = join(&[inner[0]]);
        root_file = Some(path.to_owned());
    } else if TARGET_DIRS.contains(&inner[0]) && inner.len() > 2 {
        let nested = join(&[inner[0], inner[1]]);
        if known(&format!("{nested}/main.rs")) {
            root_file = Some(format!("{nested}/main.rs"));
            root_dir = nested;
        } else {
            // A helper shared by several test programs has no single crate root.
            root_dir = join(&[inner[0]]);
            root_file = None;
        }
    } else if inner == ["build.rs"] {
        root_dir = join(&[]);
        root_file = Some(path.to_owned());
    } else {
        return None;
    }
    if root_file.as_deref().is_some_and(|file| !known(file)) {
        root_file = None;
    }
    let parent = parts[..parts.len() - 1].join("/");
    let name = stem(&parts[parts.len() - 1]);
    let own_dir = root_file.as_deref() == Some(path) || name == "mod";
    let module_dir = if own_dir {
        parent
    } else {
        [parent.as_str(), name]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("/")
    };
    Some(Placement {
        root_dir,
        root_file,
        module_dir,
    })
}

fn joined(items: &[&str]) -> String {
    items
        .iter()
        .copied()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

/// Deepest module file a path names below a directory, else the file of that directory.
fn descend(
    directory: &str,
    segments: &[String],
    root: (&str, Option<&str>),
    known: &dyn Fn(&str) -> bool,
) -> Option<String> {
    for count in (1..=segments.len()).rev() {
        let mut items: Vec<&str> = vec![directory];
        items.extend(segments[..count].iter().map(String::as_str));
        let stem = joined(&items);
        for candidate in [format!("{stem}.rs"), format!("{stem}/mod.rs")] {
            if known(&candidate) {
                return Some(candidate);
            }
        }
    }
    if directory == root.0 {
        return root.1.map(str::to_owned);
    }
    [format!("{directory}.rs"), format!("{directory}/mod.rs")]
        .into_iter()
        .find(|candidate| known(candidate))
}

/// Resolve one `use` path by Cargo layout: the target file and how sure the link is.
/// No match means no edge.
pub fn rust_target(
    path: &str,
    item: &RustUse,
    known: &dyn Fn(&str) -> bool,
    crates: &CrateRoots,
) -> Option<(String, &'static str)> {
    let segments = &item.path;
    let head = segments.first()?.as_str();
    let context = placement(path, known);
    if !item.external && PATH_ROOTS.contains(&head) {
        let context = context?;
        let root = (context.root_dir.as_str(), context.root_file.as_deref());
        if head == "crate" {
            let target = descend(&context.root_dir, &segments[1..], root, known)?;
            return Some((target, "resolved-module"));
        }
        let rest = if head == "self" {
            &segments[1..]
        } else {
            &segments[..]
        };
        let ups = rest.iter().take_while(|part| *part == "super").count();
        // `super` first leaves the inline modules around the `use`.
        if ups < item.depth || (item.depth != 0 && ups == 0) {
            return None;
        }
        let mut directory = context.module_dir.clone();
        for _ in 0..ups - item.depth {
            if !directory.starts_with(&format!("{}/", context.root_dir)) {
                return None;
            }
            directory = directory
                .rsplit_once('/')
                .map_or(directory.as_str(), |(above, _)| above)
                .to_owned();
        }
        let target = descend(&directory, &rest[ups..], root, known)?;
        return Some((target, "resolved-relative"));
    }
    if !item.external
        && item.depth == 0
        && let Some(context) = &context
    {
        // Rust 2018 paths may start at a child module; only an existing file counts.
        let stem = joined(&[context.module_dir.as_str(), head]);
        if known(&format!("{stem}.rs")) || known(&format!("{stem}/mod.rs")) {
            let root = (context.root_dir.as_str(), context.root_file.as_deref());
            let target = descend(&context.module_dir, segments, root, known)?;
            return Some((target, "resolved-relative"));
        }
    }
    let sources: &[String] = if BUILTIN_CRATES.contains(&head) {
        &[]
    } else {
        crates.get(head).map_or(&[], Vec::as_slice)
    };
    let [source] = sources else {
        return None;
    };
    let library = format!("{source}/lib.rs");
    let target = descend(
        source,
        &segments[1..],
        (source.as_str(), Some(library.as_str())),
        known,
    )?;
    Some((target, "resolved-module"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashSet;

    fn files(paths: &[&str]) -> HashSet<String> {
        paths.iter().map(|path| (*path).to_owned()).collect()
    }

    fn paths_of(source: &str) -> Vec<Vec<String>> {
        references(source)
            .expect("parses")
            .into_iter()
            .map(|item| item.path)
            .collect()
    }

    fn resolve_all(path: &str, source: &str, known: &HashSet<String>) -> Vec<String> {
        let crates = CrateRoots::new();
        references(source)
            .expect("parses")
            .iter()
            .filter_map(|item| rust_target(path, item, &|p| known.contains(p), &crates))
            .map(|(target, _)| target)
            .collect()
    }

    #[test]
    fn use_trees_flatten_to_plain_paths() {
        let found =
            paths_of("use a::{b::{c, d::e}, f as g, h::*};\nuse ::root::x;\nuse r#type::r#fn;\n");
        let spelled: Vec<String> = found.iter().map(|path| path.join("::")).collect();
        assert_eq!(
            spelled,
            [
                "a::b::c",
                "a::b::d::e",
                "a::f",
                "a::h",
                "root::x",
                "type::fn"
            ]
        );
        let extern_flags: Vec<bool> =
            references("use ::root::x;\nextern crate serde;\nuse y::z;\n")
                .expect("parses")
                .iter()
                .map(|item| item.external)
                .collect();
        assert_eq!(extern_flags, [true, true, false]);
    }

    #[test]
    fn unusual_syntax_names_nothing() {
        assert!(paths_of("use café::x;\nuse crate::módulo::y;\nuse ;\n").is_empty());
        let long_path = format!("use {};\n", ["s"; 33].join("::"));
        assert!(paths_of(&long_path).is_empty());
        let long_name = format!("use {};\n", "a".repeat(129));
        assert!(paths_of(&long_name).is_empty());
    }

    #[test]
    fn inline_modules_count_their_depth() {
        let found = references(
            "use a::x;\nmod m {\n    use b::y;\n    mod n {\n        use c::z;\n    }\n}\n",
        )
        .expect("parses");
        let depths: Vec<usize> = found.iter().map(|item| item.depth).collect();
        assert_eq!(depths, [0, 1, 2]);
    }

    #[test]
    fn use_trees_nested_past_the_python_limit_are_refused() {
        // Python raises RecursionError at these depths when its caller is shallow.
        let scoped =
            |depth: usize| format!("use {}x{};\n", "a::{".repeat(depth), "}".repeat(depth));
        let lists = |depth: usize| format!("use {}x{};\n", "{".repeat(depth), "}".repeat(depth));
        assert!(references(&scoped(496)).is_ok());
        assert_eq!(references(&scoped(497)), Err(EngineError::Nesting));
        assert!(references(&lists(993)).is_ok());
        assert_eq!(references(&lists(994)), Err(EngineError::Nesting));
    }

    #[test]
    fn links_stay_inside_the_snapshot_and_the_crate() {
        let known = files(&["Cargo.toml", "src/lib.rs", "src/a.rs", "secret.rs"]);
        // `super` cannot climb above the crate root.
        let targets = resolve_all(
            "src/a.rs",
            "use super::super::secret::x;\nuse super::super::super::y;\n",
            &known,
        );
        assert!(targets.is_empty(), "{targets:?}");
        // Whatever resolves is a file that exists.
        for target in resolve_all(
            "src/a.rs",
            "use crate::a::b;\nuse crate::nothing;\n",
            &known,
        ) {
            assert!(known.contains(&target));
        }
    }

    #[test]
    fn a_file_without_a_manifest_resolves_no_relative_paths() {
        let known = files(&["src/lib.rs", "src/a.rs"]);
        assert!(
            resolve_all(
                "src/a.rs",
                "use crate::a::x;\nuse super::a;\nuse self::b;\n",
                &known
            )
            .is_empty()
        );
    }

    #[test]
    fn crate_roots_read_names_and_refuse_odd_ones() {
        let manifest = |name: Value| json!({"architecture": {"manifest": {"name": name}}});
        let known = files(&[
            "a/src/lib.rs",
            "b/src/lib.rs",
            "c/src/lib.rs",
            "d/src/main.rs",
        ]);
        let roots = crate_roots(
            &[
                ("a/Cargo.toml".into(), manifest(json!("my-crate"))),
                ("b/Cargo.toml".into(), manifest(json!("bad name"))),
                ("c/Cargo.toml".into(), manifest(json!(5))),
                ("d/Cargo.toml".into(), manifest(json!("binary-only"))),
                ("a/MyCargo.toml".into(), manifest(json!("not-a-manifest"))),
            ],
            &|path| known.contains(path),
        )
        .expect("roots");
        assert_eq!(roots.len(), 1);
        assert_eq!(roots["my_crate"], ["a/src"]);
    }

    #[test]
    fn stored_facts_of_the_wrong_shape_are_an_error() {
        let none = |_: &str| false;
        for facts in [
            json!([]),
            json!({"architecture": []}),
            json!({"architecture": {"manifest": 3}}),
        ] {
            assert!(crate_roots(&[("Cargo.toml".into(), facts)], &none).is_err());
        }
        // Other files are never read, whatever they hold.
        assert!(crate_roots(&[("src/lib.rs".into(), json!([]))], &none).is_ok());
    }
}
