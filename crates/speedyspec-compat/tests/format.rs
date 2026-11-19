//! The scenario and fixture formats, and the replayer's comparison.

use serde_json::json;
use speedyspec_compat::{
    Breaks, Catalogue, Fixture, Globals, Invocation, Mismatch, Scenario, ScenarioRunner,
    StepResult, replay_with,
};

fn result(returned: serde_json::Value) -> StepResult {
    StepResult {
        returned,
        invocations: vec![Invocation {
            fixture: "a".into(),
            args: json!(["x"]),
            observed: None,
        }],
        globals: Globals {
            wp_filter: json!([]),
            wp_actions: json!([]),
            wp_filters: json!([]),
            wp_current_filter: json!([]),
        },
    }
}

fn catalogue() -> Catalogue {
    serde_json::from_value(json!({
        "area": "hooks",
        "scenarios": [
            { "name": "one", "steps": [
                { "call": "add_filter", "args": ["f", { "callback": "append", "with": { "suffix": "-a" }, "kind": "closure", "label": "a" }] },
                { "call": "apply_filters", "args": ["f", "x"] },
                { "call": "has_filter", "args": ["f"] }
            ] },
            { "name": "two", "steps": [ { "call": "did_filter", "args": ["f"] } ] }
        ]
    }))
    .expect("the catalogue parses")
}

fn fixture() -> Fixture {
    let mut results = std::collections::BTreeMap::new();
    results.insert(
        "one".to_string(),
        vec![
            result(json!(true)),
            result(json!("x-a")),
            result(json!(true)),
        ],
    );
    results.insert("two".to_string(), vec![result(json!(0))]);
    Fixture {
        wordpress: "7.1.5".into(),
        results,
    }
}

/// Answers with the fixture, except where told otherwise.
struct Fake(Vec<(&'static str, usize, serde_json::Value)>);

impl ScenarioRunner for Fake {
    fn run(&mut self, scenario: &Scenario) -> Result<Vec<StepResult>, String> {
        let mut results = fixture().results.remove(&scenario.name).unwrap();
        for (name, step, value) in &self.0 {
            if *name == scenario.name {
                results[*step].returned = value.clone();
            }
        }
        Ok(results)
    }
}

#[test]
fn scenario_format_round_trips() {
    let catalogue = catalogue();
    let text = serde_json::to_string(&catalogue).unwrap();
    let again: Catalogue = serde_json::from_str(&text).unwrap();
    assert_eq!(again, catalogue);
    let callback = &catalogue.scenarios[0].steps[0].args[1];
    assert_eq!(callback["callback"], "append");
    let fixture = fixture();
    let again: Fixture = serde_json::from_str(&serde_json::to_string(&fixture).unwrap()).unwrap();
    assert_eq!(again, fixture);
}

#[test]
fn the_hook_catalogue_parses_and_names_are_unique() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../compat/hooks/scenarios.json"
    ))
    .unwrap();
    let catalogue: Catalogue = serde_json::from_str(&text).unwrap();
    assert_eq!(catalogue.area, "hooks");
    let mut names: Vec<&str> = catalogue
        .scenarios
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    names.sort();
    let count = names.len();
    names.dedup();
    assert_eq!(names.len(), count, "scenario names are unique");
}

#[test]
fn replay_reports_first_mismatch_with_scenario_and_step() {
    let mut runner = Fake(vec![("one", 1, json!("x-b")), ("one", 2, json!(false))]);
    let mismatches = replay_with(&catalogue(), &fixture(), &Breaks::default(), &mut runner);
    assert_eq!(mismatches.len(), 1, "{mismatches:?}");
    let Mismatch { scenario, step, .. } = &mismatches[0];
    assert_eq!((scenario.as_str(), *step), ("one", 1));
    let text = mismatches[0].to_string();
    assert!(
        text.contains("one") && text.contains("step 1") && text.contains("x-b"),
        "{text}"
    );
    let mut clean = Fake(vec![]);
    assert!(replay_with(&catalogue(), &fixture(), &Breaks::default(), &mut clean).is_empty());
}

#[test]
fn listed_break_is_tolerated_and_unlisted_is_not() {
    let breaks: Breaks = serde_json::from_value(json!({
        "breaks": [ { "scenario": "one", "step": 1, "why": "replacing a hook global wholesale" } ]
    }))
    .unwrap();
    let mut listed = Fake(vec![("one", 1, json!("other"))]);
    assert!(replay_with(&catalogue(), &fixture(), &breaks, &mut listed).is_empty());
    let mut unlisted = Fake(vec![("one", 1, json!("other")), ("two", 0, json!(9))]);
    let mismatches = replay_with(&catalogue(), &fixture(), &breaks, &mut unlisted);
    assert_eq!(mismatches.len(), 1, "{mismatches:?}");
    assert_eq!(
        (mismatches[0].scenario.as_str(), mismatches[0].step),
        ("two", 0)
    );
}
