//! The Rust indexer against what Python's indexer core recorded for the same steps.
//!
//! `tools/record_indexer_parity.py` replays each case in `tests/parity/cases` through
//! Python and records every step's value or error and the database afterwards;
//! `tests/test_indexer_parity.py` keeps those files honest. Here the same steps run
//! through `ropeline-index` and every outcome must match as parsed JSON.
//!
//! Rust writes what Python writes: each case is replayed into a fresh database, and its
//! tables must equal the recorded dump (or its hash, where the recording keeps only one).
//! Both runtimes use the same pinned analyser signature for the comparison, because the
//! real one embeds installed Python package versions; `tests/signature.rs` covers the
//! real one.

#[path = "parity/args.rs"]
mod args;
#[path = "parity/reads.rs"]
mod reads;
#[path = "parity/runner.rs"]
mod runner;
#[path = "../../ropeline-store/tests/parity/value.rs"]
mod value;

use runner::{Run, Signatures, outcome_json};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use value::{State, compare_state, dump_state};

/// Every case in the recorded sections, counted when they were recorded.
const EXPECTED_CASES: usize = 135;

fn parse(text: &str) -> Value {
    serde_json::from_str(text).expect("committed JSON")
}

fn parity(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/parity")
        .join(name)
}

fn read(name: &str) -> Value {
    parse(&std::fs::read_to_string(parity(name)).expect("committed file"))
}

fn signatures() -> Arc<Signatures> {
    let pinned = read("expected/signatures.json");
    let cases = read("cases/signatures.json");
    Arc::new(Signatures {
        oracle: pinned["oracle"].as_str().expect("oracle").to_owned(),
        other: cases["other"].as_str().expect("other").to_owned(),
    })
}

/// `row.get(...)` of the recorder's digest: counts and a hash per table.
fn digest_of(state: &State) -> Value {
    let rest = &state.rest;
    let mut tables = serde_json::Map::new();
    for (name, table) in rest["tables"].as_object().expect("tables") {
        let rows = &table["rows"];
        tables.insert(
            name.clone(),
            json!({
                "columns": table["columns"],
                "rows": rows.as_array().expect("rows").len(),
                "sha256": value::sha256_hex(&ropeline_engine::text::python_json(rows)),
            }),
        );
    }
    let schema_id = &value::sha256_hex(&ropeline_engine::text::python_json(&state.schema))[..16];
    json!({
        "integrity": rest["integrity"],
        "journal_mode": rest["journal_mode"],
        "schemaId": schema_id,
        "user_version": rest["user_version"],
        "tables": tables,
    })
}

fn is_digest(expected: &Value) -> bool {
    expected["tables"]
        .as_object()
        .and_then(|tables| tables.values().next())
        .is_some_and(|table| table.get("sha256").is_some())
}

fn compare_recorded(
    label: &str,
    run: &Run,
    expected: &Value,
    schemas: &Value,
    problems: &mut Vec<String>,
) {
    if expected.is_null() {
        if run.db.exists()
            && std::fs::metadata(&run.db)
                .map(|m| m.len() > 0)
                .unwrap_or(false)
        {
            problems.push(format!("{label}: Python left no database but Rust did"));
        }
        return;
    }
    let state = dump_state(&run.db);
    if is_digest(expected) {
        let found = digest_of(&state);
        if found != *expected {
            let mut detail = String::new();
            for (name, table) in expected["tables"].as_object().expect("tables") {
                if found["tables"][name] != *table {
                    detail += &format!(
                        "\n  table {name}: rust {} python {table}",
                        found["tables"][name]
                    );
                }
            }
            problems.push(format!("{label}: state digest differs{detail}"));
        }
    } else {
        compare_state(label, &state, expected, schemas, problems);
    }
}

fn run_section(section: &str) -> usize {
    let cases = read(&format!("cases/{section}.json"));
    let expected = read(&format!("expected/{section}.json"));
    let signatures = signatures();
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("sources-parity")
        .join(section);
    let mut problems = Vec::new();
    let mut ran = 0;
    for case in cases["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("id");
        ran += 1;
        let recorded = &expected["cases"][id];
        let mut run = Run::new(root.join(id), Arc::clone(&signatures));
        let steps = case["steps"].as_array().expect("steps");
        for (index, step) in steps.iter().enumerate() {
            let actual = outcome_json(run.step(step));
            if actual != recorded["steps"][index] {
                problems.push(format!(
                    "{section}/{id} step {index}:\n  rust   {actual}\n  python {}",
                    recorded["steps"][index]
                ));
            }
            if let Some(mode) = step.get("state").and_then(Value::as_str)
                && run.db.exists()
            {
                let label = format!("{section}/{id} state after step {index}");
                let want = &recorded["states"][index.to_string()];
                if mode == "full" {
                    compare_state(
                        &label,
                        &dump_state(&run.db),
                        want,
                        &expected["schemas"],
                        &mut problems,
                    );
                } else {
                    compare_recorded(&label, &run, want, &expected["schemas"], &mut problems);
                }
            }
        }
        compare_recorded(
            &format!("{section}/{id} final state"),
            &run,
            &recorded["final"],
            &expected["schemas"],
            &mut problems,
        );
    }
    assert!(
        problems.is_empty(),
        "{} problems:\n{}",
        problems.len(),
        problems.join("\n")
    );
    ran
}

#[test]
fn rust_writes_what_python_writes() {
    let mut total = 0;
    for section in [
        "fixtures",
        "limits",
        "paths",
        "content",
        "walk",
        "cache",
        "waypoints",
        "timing",
        "reads-polyglot-app",
        "reads-owners-architecture",
        "reads-rust-crate",
        "reads-unsupported-cases",
        "reads-shapes",
    ] {
        total += run_section(section);
    }
    assert_eq!(total, EXPECTED_CASES, "every recorded case is replayed");
}

/// Insert a recorded final state, table by table, into a database the crate made.
fn rebuild(run: &Run, tables: &Value) {
    drop(ropeline_store::connect(&run.db).expect("schema"));
    let connection = runner::plain(&run.db).expect("open");
    for (name, table) in tables.as_object().expect("tables") {
        let columns: Vec<&str> = table["columns"]
            .as_array()
            .expect("columns")
            .iter()
            .map(|c| c.as_str().expect("column"))
            .collect();
        let sql = format!(
            "INSERT INTO \"{name}\"({}) VALUES ({})",
            columns
                .iter()
                .map(|c| format!("\"{c}\""))
                .collect::<Vec<_>>()
                .join(","),
            vec!["?"; columns.len()].join(",")
        );
        for row in table["rows"].as_array().expect("rows") {
            let params: Vec<rusqlite::types::Value> = row
                .as_array()
                .expect("row")
                .iter()
                .map(runner::to_sql_value)
                .collect();
            connection
                .execute(&sql, rusqlite::params_from_iter(params))
                .unwrap_or_else(|error| panic!("{name}: {error}"));
        }
    }
}

/// Reads can be repeated over the final state only when no write comes after the first read.
fn readable(steps: &[Value]) -> bool {
    let mut read = false;
    for step in steps {
        match step["op"].as_str().expect("op") {
            "read" | "count" => read = true,
            "indexFiles" | "indexRepo" | "prune" | "sql" | "editPayload" if read => return false,
            _ => {}
        }
    }
    read
}

/// Rust reads what Python writes: the recorded database, not one Rust made.
#[test]
fn rust_reads_what_python_writes() {
    let signatures = signatures();
    let mut checked = 0;
    for section in [
        "reads-polyglot-app",
        "reads-owners-architecture",
        "reads-rust-crate",
        "reads-unsupported-cases",
    ] {
        let cases = read(&format!("cases/{section}.json"));
        let expected = read(&format!("expected/{section}.json"));
        let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join("sources-from-python")
            .join(section);
        for case in cases["cases"].as_array().expect("cases") {
            let id = case["id"].as_str().expect("id");
            let recorded = &expected["cases"][id];
            let steps = case["steps"].as_array().expect("steps");
            assert!(!is_digest(&recorded["final"]) && readable(steps), "{id}");
            let run = &mut Run::new(root.join(id), Arc::clone(&signatures));
            rebuild(run, &recorded["final"]["tables"]);
            let mut problems = Vec::new();
            for (index, step) in steps.iter().enumerate() {
                if !matches!(step["op"].as_str(), Some("read" | "count")) {
                    continue;
                }
                let actual = outcome_json(run.step(step));
                if actual != recorded["steps"][index] {
                    problems.push(format!(
                        "{id} step {index}:\n  rust   {actual}\n  python {}",
                        recorded["steps"][index]
                    ));
                }
                checked += 1;
            }
            assert!(problems.is_empty(), "{}", problems.join("\n"));
        }
    }
    assert!(checked > 150, "only {checked} reads repeated");
}
