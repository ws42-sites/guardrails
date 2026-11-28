//! The write half of the waypoint cache, on its own: derive what is missing, reuse what is
//! there, refuse what does not fit. The indexer's parity tests hold the whole write path to
//! Python; these pin the store's own contract with small hand-made inputs.

use ropeline_engine::parsing::symbols;
use ropeline_engine::relationships::references;
use ropeline_engine::waypoints::{build_file_facts, symbols_as_values};
use ropeline_store::{Link, Refresh, RefreshFile, connect, refresh_waypoints, waypoint_records};
use rusqlite::Connection;
use serde_json::{Value, json};
use sha2::Digest;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

fn database(name: &str) -> PathBuf {
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("store-refresh");
    std::fs::create_dir_all(&folder).expect("folder");
    let path = folder.join(format!("{name}.sqlite"));
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
    path
}

const SOURCE: &[u8] = b"def read():
    return 1
";

fn facts_for(source: &str) -> Value {
    let data = source.as_bytes();
    let mut facts =
        serde_json::to_value(references(Some("python"), source).expect("refs")).expect("facts");
    let rows = symbols_as_values(&symbols("python", data).expect("symbols"));
    let waypoint = build_file_facts("python", data, &rows, &facts).expect("waypoint facts");
    facts["waypoint"] = waypoint;
    facts
}

fn hash(text: &str) -> String {
    ropeline_engine::waypoints::digest(&json!(text))
}

struct Prepared {
    connection: Connection,
    facts: Value,
    id: String,
    content: String,
}

fn prepared(name: &str) -> Prepared {
    let connection = connect(&database(name)).expect("connect");
    connection
        .execute(
            "INSERT INTO snapshots(repo,revision,metadata,view_key) VALUES ('owner/repo','r1','{}','default')",
            [],
        )
        .expect("snapshot");
    Prepared {
        connection,
        facts: facts_for("def read():\n    return 1\n"),
        id: hash("analysis"),
        content: format!("{:x}", sha2::Sha256::digest(SOURCE)),
    }
}

fn request<'a>(
    files: &'a [RefreshFile<'a>],
    configuration: &'a BTreeMap<String, String>,
    dependencies: &'a HashMap<String, Vec<Link>>,
    snapshot: i64,
) -> Refresh<'a> {
    Refresh {
        snapshot_id: snapshot,
        files,
        dependencies,
        revision: "r1",
        repository: "owner/repo",
        configuration,
        policy_digest: "",
        summaries_computed: 1,
    }
}

#[test]
fn a_second_snapshot_over_the_same_content_reuses_every_record() {
    let p = prepared("reuse");
    let files = [RefreshFile {
        path: "src/read.py",
        analysis_id: &p.id,
        content_hash: &p.content,
        facts: &p.facts,
    }];
    let none = HashMap::new();
    let config = BTreeMap::new();
    let first =
        refresh_waypoints(&p.connection, &request(&files, &config, &none, 1)).expect("first");
    assert_eq!(first["validationsComputed"], 1);
    assert_eq!(first["areasComputed"], 2);
    p.connection
        .execute(
            "INSERT INTO snapshots(repo,revision,metadata,view_key) VALUES ('owner/repo','r2','{}','default')",
            [],
        )
        .expect("snapshot");
    let again =
        refresh_waypoints(&p.connection, &request(&files, &config, &none, 2)).expect("again");
    assert_eq!(again["validationsComputed"], 0);
    assert_eq!(again["validationsReused"], 1);
    assert_eq!(again["areasComputed"], 0);
    assert_eq!(again["areasReused"], 2);
    let records = waypoint_records(&p.connection, 2, None).expect("records");
    assert_eq!(records.len(), 3, "the file, its folder and the root");
}

#[test]
fn a_name_without_a_displayable_path_is_counted_and_makes_its_areas_incomplete() {
    let p = prepared("unsafe-name");
    let files = [
        RefreshFile {
            path: "src/read.py",
            analysis_id: &p.id,
            content_hash: &p.content,
            facts: &p.facts,
        },
        RefreshFile {
            path: "src/tab\tname.py",
            analysis_id: &p.id,
            content_hash: &p.content,
            facts: &p.facts,
        },
    ];
    let none = HashMap::new();
    let config = BTreeMap::new();
    let stats =
        refresh_waypoints(&p.connection, &request(&files, &config, &none, 1)).expect("stats");
    assert_eq!(stats["unavailableFiles"], 1);
    assert_eq!(stats["partial"], true);
    let records = waypoint_records(&p.connection, 1, None).expect("records");
    let root = records.iter().find(|r| r["path"] == ".").expect("root");
    assert_eq!(root["validation"]["status"], "inconclusive");
}

#[test]
fn configuration_must_be_a_known_file_name_with_a_sha256() {
    let p = prepared("configuration");
    let files = [RefreshFile {
        path: "a.py",
        analysis_id: &p.id,
        content_hash: &p.content,
        facts: &p.facts,
    }];
    let none = HashMap::new();
    for (name, value) in [
        ("../package.json", "a".repeat(64)),
        ("package.json", "not-a-hash".to_owned()),
        (".env", "a".repeat(64)),
        ("a.py", "a".repeat(64)),
    ] {
        let config = BTreeMap::from([(name.to_owned(), value)]);
        let error =
            refresh_waypoints(&p.connection, &request(&files, &config, &none, 1)).expect_err(name);
        assert_eq!(error.message, "Invalid waypoint configuration fingerprints");
    }
}

#[test]
fn facts_without_waypoint_facts_fail_closed_and_a_tampered_cache_is_refused() {
    let p = prepared("tampered");
    let bare = json!({"imports": [], "calls": []});
    let files = [RefreshFile {
        path: "a.py",
        analysis_id: &p.id,
        content_hash: &p.content,
        facts: &bare,
    }];
    let none = HashMap::new();
    let config = BTreeMap::new();
    let error = refresh_waypoints(&p.connection, &request(&files, &config, &none, 1))
        .expect_err("no facts");
    assert_eq!(error.kind, "TypeError");

    let files = [RefreshFile {
        path: "a.py",
        analysis_id: &p.id,
        content_hash: &p.content,
        facts: &p.facts,
    }];
    refresh_waypoints(&p.connection, &request(&files, &config, &none, 1)).expect("first");
    p.connection
        .execute(
            "UPDATE waypoint_cache SET payload='{}' WHERE kind='validation'",
            [],
        )
        .expect("tamper");
    let error = refresh_waypoints(&p.connection, &request(&files, &config, &none, 1))
        .expect_err("tampered");
    assert_eq!(error.message, "Cached waypoint identity is invalid");
}
