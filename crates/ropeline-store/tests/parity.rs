//! The Rust store crate against what Python's stores recorded for the same steps.
//!
//! `tools/record_store_parity.py` replays each case in `tests/parity/cases` through
//! Python and records every step's value or error and the database afterwards;
//! `tests/test_store_parity.py` keeps those files honest. Here the same steps run
//! through `ropeline-store` and every outcome must match as parsed JSON.
//!
//! Two directions are held:
//! - Rust writes what Python writes: each case is replayed into a fresh database and
//!   its schema, pragmas, rows and step results are compared.
//! - Rust reads what Python writes: Python's recorded SQL dump is loaded as text and
//!   the case's reads are repeated against it.
//!
//! A difference that cannot be removed is listed in `KNOWN`. Each one
//! is asserted, so it fails when Rust starts to match.

#[path = "parity/runner.rs"]
mod runner;
#[path = "parity/value.rs"]
mod value;

use runner::{Run, case_folder};
use serde_json::{Value, json};

const SECTIONS: [&str; 5] = [
    "graph-open",
    "graph-migrate",
    "graph-reads",
    "graph-waypoints",
    "graph-constraints",
];

fn parse(text: &str) -> Value {
    serde_json::from_str(text).expect("committed JSON")
}

fn cases_of(section: &str) -> Value {
    parse(match section {
        "graph-open" => include_str!("parity/cases/graph-open.json"),
        "graph-migrate" => include_str!("parity/cases/graph-migrate.json"),
        "graph-reads" => include_str!("parity/cases/graph-reads.json"),
        "graph-waypoints" => include_str!("parity/cases/graph-waypoints.json"),
        "graph-constraints" => include_str!("parity/cases/graph-constraints.json"),
        other => panic!("unknown section {other}"),
    })
}

fn expected_of(section: &str) -> Value {
    parse(match section {
        "graph-open" => include_str!("parity/expected/graph-open.json"),
        "graph-migrate" => include_str!("parity/expected/graph-migrate.json"),
        "graph-reads" => include_str!("parity/expected/graph-reads.json"),
        "graph-waypoints" => include_str!("parity/expected/graph-waypoints.json"),
        "graph-constraints" => include_str!("parity/expected/graph-constraints.json"),
        other => panic!("unknown section {other}"),
    })
}

fn bases() -> Value {
    parse(include_str!("parity/expected/bases.json"))
}

/// A recorded difference: Python's answer is in the recording, Rust's is written here.
struct Known {
    section: &'static str,
    case: &'static str,
    /// Steps whose Rust outcome is `rust` and must differ from Python's.
    steps: &'static [usize],
    rust: (&'static str, &'static str),
}

const KNOWN: &[Known] = &[Known {
    section: "graph-waypoints",
    case: "huge-integer-in-payload",
    steps: &[2, 3, 4],
    // Python keeps integers of any size and its contract check rejects the record for
    // another reason; Rust has no exact form for a number past 64 bits and says so.
    rust: (
        "ValueError",
        "Cached waypoint holds a number outside the supported range",
    ),
}];

fn known_for(section: &str, case: &str, step: usize) -> Option<&'static Known> {
    KNOWN
        .iter()
        .find(|known| known.section == section && known.case == case && known.steps.contains(&step))
}

fn compare_step(
    section: &str,
    case: &str,
    step: usize,
    actual: &Value,
    expected: &Value,
    problems: &mut Vec<String>,
) {
    if let Some(known) = known_for(section, case, step) {
        let rust = json!({ "error": known.rust.0, "message": known.rust.1 });
        if *actual != rust {
            problems.push(format!(
                "{section}/{case} step {step}: known difference moved: {actual}"
            ));
        }
        if actual == expected {
            problems.push(format!(
                "{section}/{case} step {step}: listed as different but now matches"
            ));
        }
    } else if actual != expected {
        problems.push(format!(
            "{section}/{case} step {step}:\n  rust   {actual}\n  python {expected}"
        ));
    }
}

fn cases(section: &str) -> Vec<Value> {
    cases_of(section)["cases"]
        .as_array()
        .expect("cases")
        .clone()
}

fn replay(
    section: &str,
    case: &Value,
    expected: &Value,
    bases: &Value,
    problems: &mut Vec<String>,
) {
    let id = case["id"].as_str().expect("id");
    let record = &expected["cases"][id];
    let mut run = Run::new(case_folder("write", section, id));
    if let Some(base) = case.get("base").and_then(Value::as_str) {
        let base = &bases[base];
        run.load_dump(
            base["sqlDump"].as_array().expect("dump"),
            base["userVersion"].as_i64().expect("version"),
        );
    }
    let schemas = &expected["schemas"];
    let wants_state = case.get("noState").and_then(Value::as_bool) != Some(true);
    for (index, step) in case["steps"].as_array().expect("steps").iter().enumerate() {
        let actual = run.step(step);
        compare_step(
            section,
            id,
            index,
            &actual,
            &record["steps"][index],
            problems,
        );
        if step.get("state").and_then(Value::as_bool) == Some(true) && wants_state {
            let label = format!("{section}/{id} state after step {index}");
            value::compare_state(
                &label,
                &run.state(),
                &record["states"][index.to_string()],
                schemas,
                problems,
            );
        }
    }
    if wants_state {
        let label = format!("{section}/{id} final state");
        value::compare_state(&label, &run.state(), &record["final"], schemas, problems);
    }
    run.close_all();
}

#[test]
fn rust_writes_what_python_writes() {
    let bases = bases();
    let mut problems = Vec::new();
    let mut count = 0;
    for section in SECTIONS {
        let expected = expected_of(section);
        let manifest = cases(section);
        assert_eq!(
            manifest.len(),
            expected["cases"].as_object().expect("recorded").len(),
            "{section}: case set changed"
        );
        for case in &manifest {
            replay(section, case, &expected, &bases, &mut problems);
            count += 1;
        }
    }
    assert!(count >= 60, "only {count} cases ran");
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn rust_reads_what_python_writes() {
    let bases = bases();
    let mut problems = Vec::new();
    let mut loaded = 0;
    for section in SECTIONS {
        let expected = expected_of(section);
        for case in cases(section) {
            let id = case["id"].as_str().expect("id");
            let record = &expected["cases"][id];
            // A case that only reads a Python-indexed base is read straight from that base.
            let read_only = case["steps"]
                .as_array()
                .expect("steps")
                .iter()
                .all(|step| step["op"] == "connect" || step["op"] == "read");
            let (dump, version) = if let Some(dump) = record.get("sqlDump") {
                (dump, record["final"]["user_version"].as_i64())
            } else if let (Some(base), true) = (case.get("base").and_then(Value::as_str), read_only)
            {
                (&bases[base]["sqlDump"], bases[base]["userVersion"].as_i64())
            } else {
                continue;
            };
            let mut run = Run::new(case_folder("read", section, id));
            run.load_dump(dump.as_array().expect("dump"), version.expect("version"));
            let mut names: Vec<String> = case["steps"]
                .as_array()
                .expect("steps")
                .iter()
                .filter(|step| step["op"] == "read")
                .map(|step| step["as"].as_str().unwrap_or("main").to_owned())
                .collect();
            names.sort();
            names.dedup();
            for name in names {
                run.open_store(&name)
                    .expect("store opens Python's database");
            }
            for (index, step) in case["steps"].as_array().expect("steps").iter().enumerate() {
                if step["op"] == "read" {
                    let actual = run.step(step);
                    compare_step(
                        section,
                        id,
                        index,
                        &actual,
                        &record["steps"][index],
                        &mut problems,
                    );
                }
            }
            loaded += 1;
        }
    }
    assert!(loaded >= 5, "only {loaded} dumps were read");
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn every_known_difference_is_exercised() {
    let mut seen = 0;
    for known in KNOWN {
        let expected = expected_of(known.section);
        for step in known.steps {
            let recorded = &expected["cases"][known.case]["steps"][step];
            assert!(
                recorded.get("error").is_some(),
                "Python's answer is recorded"
            );
            seen += 1;
        }
    }
    assert!(seen > 0);
}

#[test]
fn rejected_writes_leave_no_token_in_errors_or_files() {
    let expected = expected_of("graph-constraints");
    let record = &expected["cases"]["token-never-reaches-errors-or-file"];
    let steps = record["steps"].as_array().expect("steps");
    for step in steps {
        let message = step["message"].as_str().unwrap_or("");
        assert!(!message.contains("READ-TOKEN-MARKER"), "{message}");
    }
    assert_eq!(steps[5]["value"]["found"], json!(false));
    assert_eq!(steps[6]["value"]["matches"], json!([]));
}
