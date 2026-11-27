//! The Rust report cache against what Python's `ReportStore` recorded.
//!
//! Same method as `parity.rs` and `semantic.rs`: the steps in
//! `tests/parity/cases/architecture.json` ran through Python when
//! `tools/record_store_parity.py` wrote `expected/architecture.json`; here they run
//! through `codegraph-store` and every outcome and database state must match.

#[path = "parity/architecture_runner.rs"]
mod architecture_runner;
#[path = "parity/runner.rs"]
mod runner;
#[path = "parity/value.rs"]
mod value;

use architecture_runner::ArchitectureRun;
use codegraph_store::ReportStore;
use runner::case_folder;
use serde_json::Value;

fn parse(text: &str) -> Value {
    serde_json::from_str(text).expect("committed JSON")
}

#[test]
fn rust_matches_python_on_every_report_cache_case() {
    let cases = parse(include_str!("parity/cases/architecture.json"));
    let expected = parse(include_str!("parity/expected/architecture.json"));
    let manifest = cases["cases"].as_array().expect("cases");
    assert_eq!(
        manifest.len(),
        expected["cases"].as_object().expect("recorded").len(),
        "case set changed"
    );
    let mut problems = Vec::new();
    for case in manifest {
        let id = case["id"].as_str().expect("id");
        let record = &expected["cases"][id];
        let mut run = ArchitectureRun::new(case_folder("architecture", "architecture", id));
        let wants_state = case.get("noState").and_then(Value::as_bool) != Some(true);
        for (index, step) in case["steps"].as_array().expect("steps").iter().enumerate() {
            let actual = run.step(step);
            if actual != record["steps"][index] {
                let shown = actual.to_string();
                let want = record["steps"][index].to_string();
                problems.push(format!(
                    "architecture/{id} step {index}:\n  rust   {}\n  python {}",
                    &shown[..shown.len().min(400)],
                    &want[..want.len().min(400)]
                ));
            }
            if step.get("state").and_then(Value::as_bool) == Some(true) && wants_state {
                value::compare_state(
                    &format!("architecture/{id} state after step {index}"),
                    &run.state(),
                    &record["states"][index.to_string()],
                    &expected["schemas"],
                    &mut problems,
                );
            }
        }
        if wants_state {
            value::compare_state(
                &format!("architecture/{id} final state"),
                &run.state(),
                &record["final"],
                &expected["schemas"],
                &mut problems,
            );
        }
        run.close_all();
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Python refuses a cache that is a link. Creating one needs a privilege Windows only
/// grants to developers, so the check is covered here rather than in the recording.
#[cfg(unix)]
#[test]
fn a_cache_that_is_a_link_is_refused() {
    let folder = case_folder("architecture", "links", "symlink");
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).expect("folder");
    let target = folder.join("real.sqlite");
    std::fs::write(&target, b"").expect("target");
    let link = folder.join("link.sqlite");
    std::os::unix::fs::symlink(&target, &link).expect("symlink");
    let error = ReportStore::new(&link).err().expect("refused");
    assert_eq!(error.kind, "ValueError");
    assert_eq!(error.message, "Architecture cache must not be a link");
    assert!(ReportStore::new(&target).is_ok());
    assert!(ReportStore::new(&folder.join("absent.sqlite")).is_ok());
}

#[test]
fn a_plain_or_missing_path_is_accepted() {
    let folder = case_folder("architecture", "links", "plain");
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).expect("folder");
    assert!(ReportStore::new(&folder.join("absent.sqlite")).is_ok());
}
