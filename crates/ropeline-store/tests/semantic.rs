//! The Rust semantic cache against what Python's `SemanticStore` recorded.
//!
//! Same method as `parity.rs`: `tools/record_store_parity.py` replays each case in
//! `tests/parity/cases/semantic.json` through Python; here the same steps run through
//! `ropeline-store` and every outcome, and every database state, must match.

#[path = "parity/runner.rs"]
mod runner;
#[path = "parity/semantic_runner.rs"]
mod semantic_runner;
#[path = "parity/value.rs"]
mod value;

use ropeline_store::{SemanticStore, current_month, fsum, month_of};
use runner::case_folder;
use semantic_runner::SemanticRun;
use serde_json::Value;

fn parse(text: &str) -> Value {
    serde_json::from_str(text).expect("committed JSON")
}

#[test]
fn rust_matches_python_on_every_semantic_case() {
    let cases = parse(include_str!("parity/cases/semantic.json"));
    let expected = parse(include_str!("parity/expected/semantic.json"));
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
        let mut run = SemanticRun::new(case_folder("semantic", "semantic", id));
        let wants_state = case.get("noState").and_then(Value::as_bool) != Some(true);
        for (index, step) in case["steps"].as_array().expect("steps").iter().enumerate() {
            let actual = run.step(step);
            if actual != record["steps"][index] {
                problems.push(format!(
                    "semantic/{id} step {index}:\n  rust   {actual}\n  python {}",
                    record["steps"][index]
                ));
            }
            if step.get("state").and_then(Value::as_bool) == Some(true) && wants_state {
                let label = format!("semantic/{id} state after step {index}");
                value::compare_state(
                    &label,
                    &run.state(),
                    &record["states"][index.to_string()],
                    &expected["schemas"],
                    &mut problems,
                );
            }
        }
        if wants_state {
            // Python records the final state while its stores are still open.
            value::compare_state(
                &format!("semantic/{id} final state"),
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

#[test]
fn the_month_key_follows_the_clock_in_utc() {
    let key = current_month();
    assert_eq!(key.len(), 7);
    assert_eq!(&key[4..5], "-");
    assert_eq!(SemanticStore::month(), key);
    assert_eq!(month_of(0), "1970-01");
}

#[test]
fn the_unit_length_check_uses_an_exactly_rounded_sum() {
    // Ten tenths drift in a running sum; the cache's bound must not.
    let tenths = [0.1; 10];
    assert_eq!(fsum(&tenths), 1.0);
    assert_ne!(tenths.iter().sum::<f64>(), 1.0);
    assert_eq!(fsum(&[]), 0.0);
    assert_eq!(fsum(&[1e100, 1.0, -1e100]), 1.0);
}
