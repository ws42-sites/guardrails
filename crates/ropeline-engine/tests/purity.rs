//! The engine does no I/O: no filesystem, network, process or environment access in `src/`.
//! `clippy.toml` bans the common entry points; this scan also catches paths used by name.

#![allow(clippy::disallowed_methods)]

const FORBIDDEN: [&str; 6] = [
    "std::fs",
    "std::net",
    "std::process",
    "std::env",
    "unsafe ",
    "libc::",
];

/// Every `.rs` file under `folder`, including sub-modules.
fn sources(folder: &std::path::Path, found: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(folder).expect("folder is readable") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
}

#[test]
fn source_files_touch_no_io() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    sources(&root, &mut files);
    for path in &files {
        let source = std::fs::read_to_string(path).expect("readable");
        for forbidden in FORBIDDEN {
            // The crate attribute that forbids unsafe code names the word once.
            let hits = source
                .lines()
                .filter(|line| line.contains(forbidden) && !line.contains("forbid(unsafe_code)"))
                .count();
            assert_eq!(hits, 0, "{} uses {forbidden}", path.display());
        }
    }
    assert!(files.len() >= 8);
}
