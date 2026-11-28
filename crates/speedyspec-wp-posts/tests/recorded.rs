//! Replays what `compat/posts/record.php` recorded from the pinned WordPress: `get_post_type_labels()` over plain
//! objects, with no `post_type_labels_{$post_type}` filter added; `get_post_type_capabilities()` with the meta
//! capabilities it remembers; `new WP_Post_Type()`'s `set_props()`, under each recorded permalink structure;
//! the `post_types` scenarios: register_post_type(), unregister_post_type() and the supports functions replayed on
//! one `PostTypeRegistry` each; the `statuses` scenarios: register_post_status() and get_post_status_object()
//! replayed on one `PostStatusRegistry` each; the `lists` scenario: get_post_types(), get_post_stati(),
//! get_post_types_by_support() and the exists, hierarchical and viewable predicates over registered types,
//! statuses and features; and get_post_statuses(), get_page_statuses() and
//! _wp_privacy_statuses() untranslated; and the `initial` runs: create_initial_post_types() from a clean state,
//! with and without permalinks and attachment pages, the globals, actions and taxonomies it left, and the queries
//! run after it.

mod support;

use speedyspec_php_serialize::semantics::is_empty;
use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_posts::{
    InitialOptions, LabelSource, Untranslated, create_initial_post_types, merge_filtered_labels,
    meta_capabilities, page_statuses, post_statuses, post_type_capabilities, post_type_labels,
    privacy_statuses, resolve_post_type,
};
use speedyspec_wp_posts_contracts::{PostStatuses, PostTypeName, PostTypeSupports, PostTypes};

#[test]
fn labels_match_wordpress() {
    let cases = support::cases();
    let labels = cases["labels"].as_array().unwrap();
    assert_eq!(labels.len(), 12);
    for case in labels {
        let object = support::decode(&case["object"]);
        let PhpValue::Str(name) = support::property(&object, "name").unwrap() else {
            panic!("name is a string");
        };
        let name = String::from_utf8(name.clone()).unwrap();
        let source = LabelSource {
            name: &name,
            label: support::property(&object, "label").filter(|v| !matches!(v, PhpValue::Null)),
            labels: support::property(&object, "labels").unwrap(),
            hierarchical: support::property(&object, "hierarchical").is_some_and(|v| !is_empty(v)),
        };
        let defaults = post_type_labels(&source, &Untranslated).unwrap();
        // No filter: the filtered labels are the defaults themselves.
        let got = merge_filtered_labels(defaults.clone(), defaults);
        let got = support::normalize_keys(PhpValue::Object {
            class: "stdClass".to_owned(),
            properties: got,
        });
        let want = support::normalize_keys(support::decode(&case["result"]));
        assert_eq!(got, want, "labels/{}", case["name"]);
    }
}

/// A recorded `array( custom => core )` of meta capabilities as pairs.
fn recorded_meta_caps(value: PhpValue) -> Vec<(String, String)> {
    let PhpValue::Array(items) = support::normalize_keys(value) else {
        panic!("meta_caps is an array");
    };
    items
        .into_iter()
        .map(|(key, value)| {
            let (PhpKey::Str(key), PhpValue::Str(value)) = (key, value) else {
                panic!("meta caps are strings");
            };
            (
                String::from_utf8(key).unwrap(),
                String::from_utf8(value).unwrap(),
            )
        })
        .collect()
}

#[test]
fn capabilities_match_wordpress() {
    let cases = support::cases();
    let caps = cases["caps"].as_array().unwrap();
    assert_eq!(caps.len(), 9);
    for case in caps {
        let name = &case["name"];
        let capability_type = support::decode(&case["capability_type"]);
        let PhpValue::Bool(map) = support::decode(&case["map_meta_cap"]) else {
            panic!("map_meta_cap is a bool");
        };
        let capabilities = support::decode(&case["capabilities"]);
        let got = post_type_capabilities(&capability_type, map, &capabilities);
        let object = PhpValue::Object {
            class: "stdClass".to_owned(),
            properties: got.clone(),
        };
        assert_eq!(object, support::decode(&case["result"]), "caps/{name}");

        let meta: Vec<(String, String)> = if map {
            meta_capabilities(&got)
                .into_iter()
                .map(|(custom, core)| (custom, core.to_owned()))
                .collect()
        } else {
            Vec::new()
        };
        assert_eq!(
            meta,
            recorded_meta_caps(support::decode(&case["meta_caps"])),
            "caps/{name} meta caps"
        );
    }
}

#[test]
fn set_props_matches_wordpress() {
    let cases = support::cases();
    let set_props = cases["set_props"].as_array().unwrap();
    assert_eq!(set_props.len(), 27);
    for case in set_props {
        let name = &case["name"];
        let PhpValue::Array(args) = support::decode(&case["args"]) else {
            panic!("args is an array");
        };
        let permalinks = !case["permalinks"].as_str().unwrap().is_empty();
        let resolved = resolve_post_type(
            PostTypeName::new(case["post_type"].as_str().unwrap()),
            args,
            &support::ports(permalinks),
        )
        .unwrap();
        // The recording is get_object_vars(): the properties as an array.
        let PhpValue::Object { class, properties } = resolved.post_type.to_php() else {
            panic!("to_php() gives an object");
        };
        assert_eq!(class, "WP_Post_Type");
        let got = support::normalize_keys(PhpValue::Array(properties));
        let want = support::normalize_keys(support::decode(&case["object"]));
        assert_eq!(got, want, "set_props/{name}");
    }
}

/// `$post_type_meta_caps` as the registry keeps it, in the recording's terms.
fn registry_meta_caps(registry: &speedyspec_wp_posts::PostTypeRegistry) -> Vec<(String, String)> {
    registry
        .meta_caps()
        .into_iter()
        .map(|(custom, core)| (custom, core.to_owned()))
        .collect()
}

/// The globals a scenario left: `$wp_post_types`, `$_wp_post_type_features`, `$post_type_meta_caps` and
/// `$wp_post_statuses`.
fn assert_scenario_state(scenario: &serde_json::Value, registries: &support::Registries) {
    let scenario_name = scenario["name"].as_str().unwrap();
    let registry = &registries.post_types;
    let post_types = PhpValue::Array(
        registry
            .post_types()
            .iter()
            .map(|t| {
                (
                    PhpKey::Str(t.name().as_str().as_bytes().to_vec()),
                    t.to_php(),
                )
            })
            .collect(),
    );
    assert_eq!(
        post_types,
        support::normalize_keys(support::decode(&scenario["wp_post_types"])),
        "{scenario_name} wp_post_types"
    );
    let features = PhpValue::Array(
        registry
            .features()
            .into_iter()
            .map(|(post_type, features)| {
                (
                    PhpKey::Str(post_type.into_bytes()),
                    PhpValue::Array(
                        features
                            .into_iter()
                            .map(|(f, v)| (PhpKey::Str(f.into_bytes()), v))
                            .collect(),
                    ),
                )
            })
            .collect(),
    );
    let want_features = match support::normalize_keys(support::decode(&scenario["features"])) {
        PhpValue::Array(items) => PhpValue::Array(
            items
                .into_iter()
                .map(|(k, v)| (k, support::normalize_keys(v)))
                .collect(),
        ),
        other => other,
    };
    assert_eq!(features, want_features, "{scenario_name} features");
    assert_eq!(
        registry_meta_caps(registry),
        recorded_meta_caps(support::decode(&scenario["meta_caps"])),
        "{scenario_name} meta_caps"
    );
    let statuses = PhpValue::Array(
        registries
            .statuses
            .statuses()
            .iter()
            .map(|s| {
                (
                    PhpKey::Str(s.name().as_str().as_bytes().to_vec()),
                    s.to_php(),
                )
            })
            .collect(),
    );
    assert_eq!(
        statuses,
        support::normalize_keys(support::decode(&scenario["wp_post_statuses"])),
        "{scenario_name} wp_post_statuses"
    );
}

#[test]
fn post_type_scenarios_match_wordpress() {
    let cases = support::cases();
    let scenarios = cases["post_types"].as_array().unwrap();
    assert_eq!(scenarios.len(), 12);
    let mut calls = 0;
    for scenario in scenarios {
        let scenario_name = scenario["name"].as_str().unwrap();
        let registries = support::replay(scenario, |call, got, registries| {
            calls += 1;
            let at = format!("{scenario_name}/{calls}/{}", call["fn"]);
            assert_eq!(
                support::normalize_keys(got.result),
                support::normalize_keys(support::decode(&call["result"])),
                "{at} result"
            );
            assert_eq!(
                got.doing_it_wrong, call["doing_it_wrong"],
                "{at} doing_it_wrong"
            );
            assert_eq!(
                got.actions,
                support::recorded_actions(&call["actions"]),
                "{at} actions"
            );
            assert_eq!(
                got.actions_removed,
                support::recorded_actions(&call["actions_removed"]),
                "{at} actions_removed"
            );
            assert_eq!(got.taxonomies, call["taxonomies"], "{at} taxonomies");
            assert_eq!(
                registry_meta_caps(&registries.post_types),
                recorded_meta_caps(support::decode(&call["meta_caps"])),
                "{at} meta_caps"
            );
        });
        assert_scenario_state(scenario, &registries);
    }
    assert_eq!(calls, 38);
}

#[test]
fn status_scenarios_match_wordpress() {
    let cases = support::cases();
    let scenarios = cases["statuses"].as_array().unwrap();
    assert_eq!(scenarios.len(), 14);
    let mut calls = 0;
    for scenario in scenarios {
        let scenario_name = scenario["name"].as_str().unwrap();
        let registries = support::replay(scenario, |call, got, _| {
            calls += 1;
            let at = format!("{scenario_name}/{calls}/{}", call["fn"]);
            assert_eq!(
                support::normalize_keys(got.result),
                support::normalize_keys(support::decode(&call["result"])),
                "{at} result"
            );
            assert_eq!(
                got.doing_it_wrong, call["doing_it_wrong"],
                "{at} doing_it_wrong"
            );
            assert_eq!(
                got.actions,
                support::recorded_actions(&call["actions"]),
                "{at} actions"
            );
            assert_eq!(got.taxonomies, call["taxonomies"], "{at} taxonomies");
        });
        assert_scenario_state(scenario, &registries);
    }
    assert_eq!(calls, 19);
}

/// A recorded `array( status => label )` as pairs.
fn recorded_table(value: PhpValue) -> Vec<(String, String)> {
    let PhpValue::Array(items) = value else {
        panic!("a status table is an array");
    };
    items
        .into_iter()
        .map(|(key, value)| {
            let (PhpKey::Str(key), PhpValue::Str(value)) = (key, value) else {
                panic!("status tables are strings");
            };
            (
                String::from_utf8(key).unwrap(),
                String::from_utf8(value).unwrap(),
            )
        })
        .collect()
}

fn owned_table(table: Vec<(&'static str, String)>) -> Vec<(String, String)> {
    table.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
}

#[test]
fn status_label_tables_match_wordpress() {
    let cases = support::cases();
    let tables = &cases["status_labels"];
    assert_eq!(
        owned_table(post_statuses(&Untranslated).unwrap()),
        recorded_table(support::decode(&tables["post"])),
        "get_post_statuses()"
    );
    assert_eq!(
        owned_table(page_statuses(&Untranslated).unwrap()),
        recorded_table(support::decode(&tables["page"])),
        "get_page_statuses()"
    );
    assert_eq!(
        owned_table(privacy_statuses(&Untranslated).unwrap()),
        recorded_table(support::decode(&tables["privacy"])),
        "_wp_privacy_statuses()"
    );
}

#[test]
fn list_scenarios_match_wordpress() {
    let cases = support::cases();
    let scenarios = cases["lists"].as_array().unwrap();
    assert_eq!(scenarios.len(), 1);
    let mut calls = 0;
    for scenario in scenarios {
        let scenario_name = scenario["name"].as_str().unwrap();
        let registries = support::replay(scenario, |call, got, _| {
            calls += 1;
            let at = format!("{scenario_name}/{calls}/{}", call["fn"]);
            assert_eq!(
                support::normalize_keys(got.result),
                support::normalize_keys(support::decode(&call["result"])),
                "{at} result"
            );
            assert_eq!(
                got.doing_it_wrong, call["doing_it_wrong"],
                "{at} doing_it_wrong"
            );
            assert_eq!(
                got.actions,
                support::recorded_actions(&call["actions"]),
                "{at} actions"
            );
            assert_eq!(
                got.actions_removed,
                support::recorded_actions(&call["actions_removed"]),
                "{at} actions_removed"
            );
            assert_eq!(got.taxonomies, call["taxonomies"], "{at} taxonomies");
        });
        assert_scenario_state(scenario, &registries);
    }
    assert_eq!(calls, 87);
}

#[test]
fn initial_post_types_match_wordpress() {
    let cases = support::cases();
    let runs = cases["initial"].as_array().unwrap();
    assert_eq!(runs.len(), 2);
    let mut queries = 0;
    for run in runs {
        let run_name = run["name"].as_str().unwrap();
        let ports = support::ports(!run["permalinks"].as_str().unwrap().is_empty());
        let options = InitialOptions {
            attachment_pages_enabled: run["attachment_pages"].as_str().unwrap() == "1",
        };
        let mut registries = support::Registries::default();
        let registrations = create_initial_post_types(
            &mut registries.post_types,
            &mut registries.statuses,
            &options,
            &ports,
        )
        .unwrap();
        assert_scenario_state(run, &registries);

        let actions: Vec<support::Action> = registrations
            .iter()
            .flat_map(|r| r.actions.iter())
            .map(|a| {
                (
                    PhpValue::Str(a.hook.as_bytes().to_vec()),
                    a.callback.clone(),
                    a.priority,
                    i64::from(a.accepted_args),
                )
            })
            .collect();
        assert_eq!(
            actions,
            support::recorded_actions(&run["actions"]),
            "initial/{run_name} actions"
        );
        let taxonomies: Vec<serde_json::Value> = registrations
            .iter()
            .flat_map(|r| {
                r.taxonomies.iter().map(|t| {
                    serde_json::json!([
                        String::from_utf8(speedyspec_php_serialize::semantics::to_php_string(t))
                            .unwrap(),
                        r.post_type.name().as_str()
                    ])
                })
            })
            .collect();
        assert_eq!(
            serde_json::Value::Array(taxonomies),
            run["taxonomies"],
            "initial/{run_name} taxonomies"
        );

        support::replay_calls(registries, &run["queries"], &ports, |call, got, _| {
            queries += 1;
            let at = format!("initial/{run_name}/{queries}/{}", call["fn"]);
            assert_eq!(
                support::normalize_keys(got.result),
                support::normalize_keys(support::decode(&call["result"])),
                "{at} result"
            );
            assert_eq!(
                got.doing_it_wrong, call["doing_it_wrong"],
                "{at} doing_it_wrong"
            );
        });
    }
    assert_eq!(queries, 68);
}
