//! The formatting area keeps exact bytes: a string that is not valid UTF-8, as a value or as an array key, is passed
//! and reported as itself, never as U+FFFD substitutes.
//!
//! PHP boots once per process, so every check shares one runner in one test.

use serde_json::json;
use speedyspec_compat::{Scenario, ScenarioRunner, area_runner};

fn returned(runner: &mut dyn ScenarioRunner, step: serde_json::Value) -> serde_json::Value {
    let scenario: Scenario =
        serde_json::from_value(json!({"name": "bytes", "steps": [step]})).unwrap();
    runner.run(&scenario).unwrap().remove(0).returned
}

#[test]
fn invalid_utf8_round_trips_exactly() {
    let mut runner = match area_runner("formatting", "7.1.5") {
        Ok(runner) => runner,
        Err(why) => panic!("{why}"),
    };

    let scenario: Scenario = serde_json::from_value(json!({
        "name": "bytes",
        "steps": [{"call": "strval", "args": [{"bytes": "/w=="}]}]
    }))
    .unwrap();
    let steps = runner.run(&scenario).unwrap();
    assert_eq!(steps[0].returned, json!({"bytes": "/w=="}), "a value");
    assert_eq!(steps[0].state, json!({"array": []}), "no diagnostics");

    assert_eq!(
        returned(
            &mut runner,
            json!({"call": "array_flip", "args": [{"array": [[0, {"bytes": "/w=="}]]}]})
        ),
        json!({"array": [[{"bytes": "/w=="}, 0]]}),
        "a reported key"
    );
    assert_eq!(
        returned(
            &mut runner,
            json!({"call": "array_keys", "args": [{"array": [[{"bytes": "/w=="}, 1]]}]})
        ),
        json!({"array": [[0, {"bytes": "/w=="}]]}),
        "a passed key"
    );
}
