//! The analyser signature: Rust names its own engine and grammars, so neither runtime ever
//! serves the other's cached analysis as its own.
//!
//! Cache keys hash the signature. A row written under another signature has another key,
//! so it is simply not found: a miss, which is correct. A false hit would let one runtime
//! trust analyses the other produced from different grammars.

use ropeline_index::{
    IndexOptions, SourceFile, analysis_version, cached_git_analyses, cached_git_analyses_in,
    git_blob_oid, index_files,
};
use serde_json::Value;
use std::path::PathBuf;

fn parity(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/parity")
        .join(name);
    serde_json::from_str(&std::fs::read_to_string(path).expect("file")).expect("JSON")
}

fn workspace_file(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(name);
    std::fs::read_to_string(path).expect("workspace file")
}

fn database(name: &str) -> PathBuf {
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("sources-signature");
    std::fs::create_dir_all(&folder).expect("folder");
    let path = folder.join(format!("{name}.sqlite"));
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
    path
}

const SOURCE: &[u8] = b"def read():\n    return 1\n";

fn files() -> Vec<Result<SourceFile, ropeline_index::SourceError>> {
    vec![Ok(SourceFile::bytes("a.py", SOURCE))]
}

#[test]
fn the_committed_rust_signature_is_the_one_in_the_code() {
    let committed = parity("cases/signatures.json");
    assert_eq!(committed["rust"].as_str(), Some(analysis_version()));
}

#[test]
fn the_signature_names_its_engine_and_never_looks_like_pythons() {
    let rust: Value = serde_json::from_str(analysis_version()).expect("JSON");
    let python: Value = serde_json::from_str(
        parity("expected/signatures.json")["oracle"]
            .as_str()
            .expect("text"),
    )
    .expect("JSON");
    assert_eq!(rust["engine"], "rust");
    assert!(python.get("engine").is_none());
    assert_ne!(
        rust.as_object().map(|m| m.keys().collect::<Vec<_>>()),
        python.as_object().map(|m| m.keys().collect::<Vec<_>>())
    );
    // The semantic format and the waypoint version move together with Python's.
    assert_eq!(rust["format"], python["format"]);
    assert_eq!(rust["waypoints"], ropeline_engine::waypoints::VERSION);
}

#[test]
fn the_grammar_versions_are_the_ones_the_lock_file_resolves() {
    let lock = workspace_file("Cargo.lock");
    let rust: Value = serde_json::from_str(analysis_version()).expect("JSON");
    let version_of = |name: &str| -> String {
        let at = lock
            .find(&format!("name = \"{name}\""))
            .expect("locked crate");
        let tail = &lock[at..];
        let line = tail.lines().nth(1).expect("version line");
        line.trim_start_matches("version = \"")
            .trim_end_matches('"')
            .to_owned()
    };
    assert_eq!(rust["tree-sitter"], version_of("tree-sitter").as_str());
    for language in ["javascript", "python", "rust", "typescript"] {
        assert_eq!(
            rust["grammars"][language],
            version_of(&format!("tree-sitter-{language}")).as_str(),
            "{language}"
        );
    }
}

#[test]
fn rows_written_under_another_signature_are_misses_and_the_other_way_round() {
    let python_like = parity("expected/signatures.json")["oracle"]
        .as_str()
        .expect("text")
        .to_owned();
    let path = database("two-writers");
    let oid = git_blob_oid(SOURCE);
    let wanted = vec![(oid.clone(), "python".to_owned())];

    // A Python-shaped writer goes first; Rust does not see its rows.
    let clock = ropeline_index::MonotonicClock::new();
    let environment = |signature: &'static str| ropeline_index::Environment {
        clock: &clock,
        signature,
        before_commit: None,
    };
    let leaked: &'static str = Box::leak(python_like.into_boxed_str());
    let first = ropeline_index::index_files_in(
        files(),
        &path,
        "owner/repo",
        &IndexOptions::default(),
        environment(leaked),
    )
    .expect("python-shaped index");
    assert_eq!(first.parsed_files, 1);
    assert!(
        cached_git_analyses(&path, &wanted)
            .expect("lookup")
            .is_empty()
    );
    assert_eq!(
        cached_git_analyses_in(&path, &wanted, leaked)
            .expect("lookup")
            .len(),
        1
    );

    // Rust indexes the same bytes: nothing is reused, so it parses again.
    let second =
        index_files(files(), &path, "owner/repo", &IndexOptions::default()).expect("rust index");
    assert_eq!((second.parsed_files, second.reused_files), (1, 0));
    assert_eq!(
        cached_git_analyses(&path, &wanted).expect("lookup").len(),
        1
    );

    // And the Python-shaped writer still finds only its own row, not Rust's.
    let third = ropeline_index::index_files_in(
        files(),
        &path,
        "owner/repo",
        &IndexOptions::default(),
        environment(leaked),
    )
    .expect("python-shaped again");
    assert_eq!((third.parsed_files, third.reused_files), (0, 1));
    let connection = rusqlite::Connection::open(&path).expect("open");
    let rows: i64 = connection
        .query_row("SELECT count(*) FROM analyses", [], |row| row.get(0))
        .expect("count");
    assert_eq!(rows, 2, "one analysis per writer, neither overwritten");
}
