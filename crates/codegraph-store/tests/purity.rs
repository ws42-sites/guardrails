//! The store opens the database path it is given and does nothing else: no network,
//! process or environment access and no unsafe code. Three narrow calls are allowed
//! because Python does the same: the semantic cache makes the folder its file lives in,
//! the report cache refuses a link, and its journal switch retries after a pause.
//! The engine may never depend on this crate.

const ALLOWED: [(&str, &str); 3] = [
    ("semantic_store.rs", "std::fs::create_dir_all("),
    ("architecture_store.rs", "std::fs::symlink_metadata("),
    ("architecture_store.rs", "std::thread::sleep("),
];

const FORBIDDEN: [&str; 7] = [
    "std::fs",
    "std::net",
    "std::process",
    "std::env",
    "unsafe ",
    "libc::",
    "std::thread",
];

#[test]
fn source_files_touch_nothing_but_the_database() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut checked = 0;
    for entry in std::fs::read_dir(&root).expect("src is readable") {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|ext| ext == "rs") {
            let mut source = std::fs::read_to_string(&path).expect("readable");
            for (file, call) in ALLOWED {
                if path.file_name().is_some_and(|name| name == file) {
                    assert_eq!(
                        source.matches(call).count(),
                        1,
                        "{file} must use {call} once"
                    );
                    source = source.replace(call, "");
                }
            }
            for forbidden in FORBIDDEN {
                // The crate attribute that forbids unsafe code names the word once.
                let hits = source
                    .lines()
                    .filter(|line| {
                        line.contains(forbidden) && !line.contains("forbid(unsafe_code)")
                    })
                    .count();
                assert_eq!(hits, 0, "{} uses {forbidden}", path.display());
            }
            checked += 1;
        }
    }
    assert!(checked >= 12);
}

#[test]
fn the_engine_does_not_depend_on_the_store() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("codegraph-engine")
        .join("Cargo.toml");
    let text = std::fs::read_to_string(manifest).expect("engine manifest");
    assert!(!text.contains("codegraph-store"));
}
