//! The indexer reads files and folders and nothing else: no process, network or
//! environment access and no unsafe code. Only `capture.rs` and `ownership.rs` may reach
//! the file system, and only through the calls named here. Everything else, including the
//! database, goes through `codegraph-store`.

use std::path::{Path, PathBuf};

/// Files that may name a `std::fs` item, and the items they may name.
const FILE_SYSTEM: [(&str, &[&str]); 2] = [
    (
        "capture.rs",
        &[
            "read_dir",
            "symlink_metadata",
            "metadata",
            "canonicalize",
            "File",
            "Metadata",
        ],
    ),
    ("ownership.rs", &["symlink_metadata"]),
];

const FORBIDDEN: [&str; 12] = [
    "std::net",
    "std::process",
    "std::env",
    "std::thread",
    "unsafe ",
    "libc::",
    "Command::",
    "Connection::open",
    "rusqlite::Connection::",
    "OpenOptions",
    "TcpStream",
    "UdpSocket",
];

/// Path methods that read the file system without naming `std::fs`.
const FILE_SYSTEM_METHODS: [&str; 9] = [
    ".canonicalize(",
    ".metadata(",
    ".read_dir(",
    ".exists(",
    ".is_dir(",
    ".is_file(",
    ".is_symlink(",
    ".read_link(",
    ".symlink_metadata(",
];

fn sources(folder: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(folder).expect("folder is readable") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
}

fn used_items(source: &str) -> Vec<String> {
    source
        .match_indices("std::fs")
        .map(|(at, _)| {
            let rest = &source[at + "std::fs".len()..];
            let rest = rest.strip_prefix("::").unwrap_or("");
            rest.chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect()
        })
        .collect()
}

#[test]
fn source_files_touch_nothing_but_the_folder_they_are_given() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    sources(&root, &mut files);
    for path in &files {
        let source = std::fs::read_to_string(path).expect("readable");
        let name = path.file_name().and_then(|n| n.to_str()).expect("name");
        for forbidden in FORBIDDEN {
            // The crate attribute that forbids unsafe code names the word once.
            let hits = source
                .lines()
                .filter(|line| line.contains(forbidden) && !line.contains("forbid(unsafe_code)"))
                .count();
            assert_eq!(hits, 0, "{} uses {forbidden}", path.display());
        }
        let allowed = FILE_SYSTEM
            .iter()
            .find(|(file, _)| *file == name)
            .map(|(_, items)| *items);
        for item in used_items(&source) {
            let permitted = allowed.is_some_and(|items| items.contains(&item.as_str()));
            assert!(permitted, "{} uses std::fs::{item}", path.display());
        }
        if allowed.is_none() {
            for method in FILE_SYSTEM_METHODS {
                assert!(
                    !source.contains(method),
                    "{} calls {method} outside the capture code",
                    path.display()
                );
            }
        }
    }
}

#[test]
fn the_engine_and_the_store_do_not_depend_on_this_crate() {
    for sibling in ["codegraph-engine", "codegraph-store"] {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(sibling)
            .join("Cargo.toml");
        let text = std::fs::read_to_string(manifest).expect("sibling manifest");
        assert!(
            !text.contains("codegraph-sources"),
            "{sibling} depends on sources"
        );
    }
}
