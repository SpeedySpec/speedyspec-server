//! The formatting area keeps exact bytes: a string that is not valid UTF-8 is recorded and replayed as itself,
//! never as U+FFFD substitutes.

use serde_json::json;
use speedyspec_compat::{Scenario, ScenarioRunner, area_runner};

#[test]
fn invalid_utf8_round_trips_exactly() {
    let mut runner = match area_runner("formatting", "7.1.5") {
        Ok(runner) => runner,
        Err(why) if why.contains("fetch") || why.contains("download") => {
            println!("skipped: WordPress 7.1.5 cannot be fetched: {why}");
            return;
        }
        Err(why) => panic!("{why}"),
    };
    let scenario: Scenario = serde_json::from_value(json!({
        "name": "bytes",
        "steps": [{"call": "strval", "args": [{"bytes": "/w=="}]}]
    }))
    .unwrap();
    let steps = runner.run(&scenario).unwrap();
    assert_eq!(steps[0].returned, json!({"bytes": "/w=="}));
    assert_eq!(steps[0].state, json!({"array": []}));
}
