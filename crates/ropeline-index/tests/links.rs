//! Links, special files and names that cannot be recorded because not every platform can
//! make them. The expected outcomes are declared in `walk-links.json` and checked against
//! Python's own behaviour by `tests/test_indexer_parity.py` wherever it can run; here the
//! same declarations are held to the Rust indexer.
//!
//! A test that cannot create a link on this machine says so and passes nothing: it never
//! reports a link as handled when it could not make one.

use ropeline_index::{RepoOptions, index_repo};
use rusqlite::Connection;
use serde_json::Value;
use std::path::{Path, PathBuf};

fn declared() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/parity/cases/walk-links.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("file")).expect("JSON")
}

fn scratch(name: &str) -> PathBuf {
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("sources-links")
        .join(name);
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).expect("scratch");
    folder
}

fn write(base: &Path, files: &Value) {
    for (relative, text) in files.as_object().into_iter().flatten() {
        let path = base.join(relative);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("folder");
        std::fs::write(path, text.as_str().expect("text")).expect("write");
    }
}

#[cfg(unix)]
fn link(target: &Path, at: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, at)
}

#[cfg(windows)]
fn link(target: &Path, at: &Path) -> std::io::Result<()> {
    let resolved = at.parent().expect("parent").join(target);
    if resolved.is_dir() {
        std::os::windows::fs::symlink_dir(target, at)
    } else {
        std::os::windows::fs::symlink_file(target, at)
    }
}

/// Whether this platform lets the test make links at all.
fn can_link(folder: &Path) -> bool {
    let probe = folder.join("probe-target");
    std::fs::create_dir_all(&probe).expect("probe");
    link(Path::new("probe-target"), &folder.join("probe")).is_ok()
}

fn paths(database: &Path) -> Vec<String> {
    let connection = Connection::open(database).expect("open");
    let mut statement = connection
        .prepare("SELECT path FROM snapshot_files ORDER BY path")
        .expect("prepare");
    statement
        .query_map([], |row| row.get(0))
        .expect("rows")
        .map(|row| row.expect("row"))
        .collect()
}

fn has_owners(database: &Path) -> bool {
    let connection = Connection::open(database).expect("open");
    let text: String = connection
        .query_row("SELECT metadata FROM snapshots", [], |row| row.get(0))
        .expect("metadata");
    serde_json::from_str::<Value>(&text)
        .expect("JSON")
        .get("codeowners")
        .is_some()
}

#[test]
fn declared_link_cases_hold_for_rust() {
    let cases = declared();
    for case in cases["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("id");
        let base = scratch(id);
        if !can_link(&base) {
            eprintln!("{id}: this platform cannot create symbolic links here; not checked");
            continue;
        }
        let root = base.join("root");
        write(&root, &case["files"]);
        write(&base.join("outside"), &case["outsideFiles"]);
        for (name, target) in case["links"].as_object().expect("links") {
            link(
                Path::new(target.as_str().expect("target")),
                &root.join(name),
            )
            .expect("link");
        }
        let database = base.join("graph.sqlite");
        let result = index_repo(&root, &database, Some("owner/repo"), RepoOptions::default())
            .expect("index");
        let mut want: Vec<String> = case["expect"]["files"]
            .as_array()
            .expect("files")
            .iter()
            .map(|path| path.as_str().expect("path").to_owned())
            .collect();
        want.sort();
        assert_eq!(paths(&database), want, "{id}");
        assert_eq!(
            result.skipped_files as u64, case["expect"]["skipped"],
            "{id}"
        );
        if let Some(owners) = case["expect"].get("ownership").and_then(Value::as_bool) {
            assert_eq!(has_owners(&database), owners, "{id}");
        }
    }
}

#[test]
fn declared_linked_root_holds_for_rust() {
    let spec = &declared()["linkedRoot"];
    let base = scratch("linked-root");
    if !can_link(&base) {
        eprintln!("linked root: this platform cannot create symbolic links here; not checked");
        return;
    }
    write(&base.join("real"), &spec["files"]);
    link(Path::new("real"), &base.join("linked")).expect("link");
    let result = index_repo(
        &base.join("linked"),
        &base.join("graph.sqlite"),
        Some("r"),
        RepoOptions::default(),
    )
    .expect("index");
    assert_eq!(
        result.files as u64,
        spec["expect"]["files"].as_array().expect("files").len() as u64
    );
    assert_eq!(result.skipped_files as u64, spec["expect"]["skipped"]);
}

#[cfg(unix)]
#[test]
fn declared_unix_names_abort_the_whole_index_as_python_does() {
    for name in declared()["unixNames"].as_array().expect("names") {
        let base = scratch("unix-name");
        std::fs::write(base.join("ok.py"), "def ok(): pass\n").expect("write");
        std::fs::write(base.join(name.as_str().expect("name")), "def odd(): pass\n")
            .expect("write");
        let database = base.join("graph.sqlite");
        let error =
            index_repo(&base, &database, Some("repo"), RepoOptions::default()).expect_err("aborts");
        assert_eq!(
            (error.kind, error.message.as_str()),
            ("ValueError", "Invalid source path")
        );
        let connection = Connection::open(&database).expect("open");
        let snapshots: i64 = connection
            .query_row("SELECT count(*) FROM snapshots", [], |row| row.get(0))
            .expect("count");
        assert_eq!(snapshots, 0);
    }
}

/// A socket file is not a regular file. Python opens it, fails, and counts it as skipped; Rust
/// looks at the file type first and counts it as skipped too. The same lookup is why a named
/// pipe, which Python would open and wait on forever, is skipped here (a pipe
/// cannot be made without starting a process, so that difference is not asserted).
#[cfg(unix)]
#[test]
fn a_socket_file_is_skipped_as_python_skips_it() {
    let spec = &declared()["socket"];
    let base = scratch("socket");
    write(&base, &spec["files"]);
    let listener =
        std::os::unix::net::UnixListener::bind(base.join(spec["name"].as_str().expect("name")))
            .expect("socket");
    let database = base.join("graph.sqlite");
    let result = index_repo(&base, &database, Some("repo"), RepoOptions::default()).expect("index");
    drop(listener);
    assert_eq!(
        result.files as u64,
        spec["expect"]["files"].as_array().expect("files").len() as u64
    );
    assert_eq!(result.skipped_files as u64, spec["expect"]["skipped"]);
}

#[test]
fn the_repository_name_defaults_to_the_folders_own_name() {
    let base = scratch("default-name");
    let root = base.join("my-project");
    write(&root, &serde_json::json!({"a.py": "def a(): pass\n"}));
    let database = base.join("graph.sqlite");
    let named = index_repo(&root, &database, None, RepoOptions::default()).expect("index");
    assert_eq!(named.repository, "my-project");
    let empty = index_repo(&root, &database, Some(""), RepoOptions::default()).expect("index");
    assert_eq!(empty.repository, "my-project");
}
