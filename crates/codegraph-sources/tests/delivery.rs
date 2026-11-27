//! Descriptions served from stored metadata are checked, never trusted: a tampered or
//! misplaced record is refused rather than shown.

use codegraph_sources::{
    IndexOptions, SourceFile, area_waypoints, file_waypoint, index_files, overview_waypoints,
};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::path::PathBuf;

fn indexed(name: &str) -> (Value, String, Value) {
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("sources-delivery");
    std::fs::create_dir_all(&folder).expect("folder");
    let database = folder.join(format!("{name}.sqlite"));
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", database.display()));
    }
    let result = index_files(
        [Ok(SourceFile::bytes(
            "src/a.py",
            b"def a():\n    return 1\n",
        ))],
        &database,
        "owner/repo",
        &IndexOptions::default(),
    )
    .expect("index");
    let connection = Connection::open(&database).expect("open");
    let file: String = connection
        .query_row(
            "SELECT metadata FROM nodes WHERE kind='file' AND path='src/a.py'",
            [],
            |row| row.get(0),
        )
        .expect("file");
    let repository: String = connection
        .query_row(
            "SELECT metadata FROM nodes WHERE kind='repository'",
            [],
            |row| row.get(0),
        )
        .expect("repository");
    (
        serde_json::from_str(&file).expect("JSON"),
        result.revision,
        serde_json::from_str(&repository).expect("JSON"),
    )
}

#[test]
fn a_stored_description_is_served_only_when_it_matches_its_file_and_revision() {
    let (file, revision, _) = indexed("file");
    let served = file_waypoint(&file, "src/a.py", &revision)
        .expect("ok")
        .expect("some");
    assert_eq!(served["kind"], "file");
    assert!(
        file_waypoint(&json!({"language": "python"}), "src/a.py", &revision)
            .expect("ok")
            .is_none()
    );
    // Another path, or another file's content hash, is refused.
    assert!(file_waypoint(&file, "src/b.py", &revision).is_err());
    let mut swapped = file.clone();
    swapped["sha256"] = json!("0".repeat(64));
    let error = file_waypoint(&swapped, "src/a.py", &revision).expect_err("hash");
    assert_eq!(
        error.message,
        "Waypoint source content differs from its indexed file"
    );
    let error = file_waypoint(&json!([1]), "src/a.py", &revision).expect_err("shape");
    assert_eq!(
        (error.kind, error.message.as_str()),
        ("TypeError", "Invalid waypoint metadata")
    );
}

#[test]
fn area_descriptions_are_unique_areas_within_a_budget() {
    let (_, revision, repository) = indexed("areas");
    let areas = area_waypoints(&repository, &revision).expect("areas");
    assert_eq!(areas.len(), 2);
    let mut doubled = repository.clone();
    let first = doubled["areaWaypoints"][0].clone();
    doubled["areaWaypoints"]
        .as_array_mut()
        .expect("list")
        .push(first);
    let error = area_waypoints(&doubled, &revision).expect_err("duplicate");
    assert_eq!(error.message, "Invalid or duplicated area waypoint");
    let (all, partial) = overview_waypoints(&repository, &revision, 8000).expect("overview");
    assert_eq!(
        (all.len(), partial),
        (2, false),
        "the root and src are both top-level"
    );
    let (none, partial) = overview_waypoints(&repository, &revision, 10).expect("tiny budget");
    assert_eq!((none.len(), partial), (0, true));
    let mut huge = repository.clone();
    huge["areaWaypoints"] = Value::Array(vec![json!({}); 10_001]);
    assert!(area_waypoints(&huge, &revision).is_err());
}
