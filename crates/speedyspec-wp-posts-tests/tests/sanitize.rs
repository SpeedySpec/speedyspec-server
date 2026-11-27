//! Replays the recorded `sanitize-*` scenarios of compat/posts-read (WordPress 7.2-RC2) against `sanitize_post()` and
//! `sanitize_post_field()` in Rust: `WpHookApi` over the memory store with the recorded fixture callbacks, and the
//! Rust escaping as the `Formatting` port. Each step's return value, fixture invocations (labels and arguments, in
//! order) and kept values must match the recording.
//!
//! `get_post( 1 )` (in `sanitize-post-wp-post`) is not run here: its object comes from the recording's kept value,
//! and the get_post use case's own tests cover the read.

mod support;

use std::collections::BTreeMap;
use std::rc::Rc;

use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_posts::{
    SanitizePorts, sanitize_post_field, sanitize_post_field_word, sanitize_post_properties,
    sanitize_post_value,
};
use speedyspec_wp_posts_contracts::{Filter, PostId};

use support::{Invocations, RustFormatting};

/// A scenario argument as the driver's `SpeedySpecCompat::args()` reads it: JSON objects are PHP arrays.
fn arg(value: &serde_json::Value, vars: &BTreeMap<String, PhpValue>) -> PhpValue {
    match value {
        serde_json::Value::Object(map) if map.len() == 1 && map.contains_key("var") => {
            vars[map["var"].as_str().unwrap()].clone()
        }
        serde_json::Value::Object(map) => PhpValue::Array(
            map.iter()
                .map(|(k, v)| (support::php_key(k), arg(v, vars)))
                .collect(),
        ),
        serde_json::Value::Array(items) => PhpValue::Array(
            items
                .iter()
                .enumerate()
                .map(|(i, v)| (PhpKey::Int(i as i64), arg(v, vars)))
                .collect(),
        ),
        other => support::decode(other).unwrap(),
    }
}

fn context(args: &[serde_json::Value], index: usize) -> String {
    args.get(index)
        .map_or("display", |c| c.as_str().unwrap())
        .to_owned()
}

/// What a step returned: a value, or a kept variable (an object the recorder reports by its label).
#[derive(Debug, PartialEq)]
enum Returned {
    Value(PhpValue),
    Var(String),
}

#[test]
fn sanitize_scenarios_match_wordpress() {
    let scenarios = support::scenarios("sanitize-");
    assert_eq!(scenarios.len(), 22);
    let mut matched = 0;
    for (name, scenario, recorded) in scenarios {
        let hooks = support::hooks();
        let formatting = RustFormatting::new(hooks.clone());
        let ports = SanitizePorts {
            hooks: &*hooks,
            formatting: &formatting,
        };
        let log: Invocations = Rc::default();
        let mut vars: BTreeMap<String, PhpValue> = BTreeMap::new();

        let steps = scenario["steps"].as_array().unwrap();
        assert_eq!(steps.len(), recorded.len(), "{name}");
        for (i, (step, want)) in steps.iter().zip(&recorded).enumerate() {
            let at = format!("{name} step {i} ({})", step["call"]);
            log.borrow_mut().clear();
            let args = step["args"].as_array().cloned().unwrap_or_default();
            let before = vars.clone();
            let kept = support::kept(want);

            let returned = match step["call"].as_str().unwrap() {
                "add_filter" => {
                    support::add_fixture(
                        &hooks,
                        &log,
                        args[0].as_str().unwrap(),
                        &args[1],
                        args[2].as_i64().unwrap(),
                        args[3].as_i64().unwrap(),
                    );
                    Returned::Value(PhpValue::Bool(true))
                }
                "speedyspec_compat_object" => {
                    let PhpValue::Array(properties) = arg(&args[0], &vars) else {
                        panic!("{at}: an object is built from an array");
                    };
                    let var = step["as"].as_str().unwrap().to_owned();
                    vars.insert(
                        var.clone(),
                        PhpValue::Object {
                            class: "stdClass".to_owned(),
                            properties,
                        },
                    );
                    Returned::Var(var)
                }
                // The recorder's user is logged in; user_can_richedit() still starts from false (no browser).
                "speedyspec_compat_login" => Returned::Value(arg(&args[0], &vars)),
                "get_post" => {
                    let var = step["as"].as_str().unwrap().to_owned();
                    let (_, object) = kept
                        .iter()
                        .find(|(k, _)| *k == var)
                        .unwrap_or_else(|| panic!("{at}: the recording keeps {var}"));
                    vars.insert(var.clone(), object.clone());
                    Returned::Var(var)
                }
                "sanitize_post" => {
                    let out = sanitize_post_value(&ports, arg(&args[0], &vars), &context(&args, 1))
                        .unwrap();
                    match args[0].get("var").and_then(|v| v.as_str()) {
                        // PHP sanitizes the object in place, through its handle.
                        Some(var) if matches!(out, PhpValue::Object { .. }) => {
                            vars.insert(var.to_owned(), out);
                            Returned::Var(var.to_owned())
                        }
                        _ => Returned::Value(out),
                    }
                }
                "sanitize_post_field" => {
                    let field = args[0].as_str().unwrap();
                    let value = arg(&args[1], &vars);
                    let post_id = args[2].as_i64().unwrap();
                    Returned::Value(
                        sanitize_post_field_word(
                            &ports,
                            field.as_bytes(),
                            value,
                            post_id,
                            &context(&args, 3),
                        )
                        .unwrap(),
                    )
                }
                other => panic!("{at}: {other} is not a sanitize step"),
            };

            let want_returned = match support::decode(&want["returned"]) {
                Ok(value) => Returned::Value(value),
                Err(label) => Returned::Var(label.trim_start_matches('$').to_owned()),
            };
            assert_eq!(returned, want_returned, "{at}: returned");
            assert_eq!(
                *log.borrow(),
                support::recorded_invocations(want),
                "{at}: invocations"
            );

            // Every kept value the recorder reported as new or changed, and no other change.
            for (var, object) in &kept {
                assert_eq!(vars.get(var), Some(object), "{at}: kept {var}");
            }
            for (var, value) in &vars {
                if before.get(var) != Some(value) {
                    assert!(
                        kept.iter().any(|(k, _)| k == var),
                        "{at}: {var} changed, but the recording keeps no new value"
                    );
                }
            }
        }
        matched += 1;
    }
    assert_eq!(matched, 22);
}

/// `sanitize_post_properties()` over the recorded `get_post( 1 )` object's properties: `edit` gives the recording's
/// `sanitize_post( $p, 'edit' )`, and a second `edit` changes nothing (the `filter` check), so nothing is escaped
/// twice.
#[test]
fn properties_sanitize_once_per_context() {
    let (_, _, recorded) = support::scenarios("sanitize-post-wp-post").remove(0);
    let PhpValue::Object {
        properties: mut props,
        ..
    } = support::kept(&recorded[0]).remove(0).1
    else {
        panic!("an object");
    };
    let PhpValue::Object {
        properties: edited, ..
    } = support::kept(&recorded[1]).remove(0).1
    else {
        panic!("an object");
    };

    let hooks = support::hooks();
    let formatting = RustFormatting::new(hooks.clone());
    let ports = SanitizePorts {
        hooks: &*hooks,
        formatting: &formatting,
    };
    assert!(!sanitize_post_properties(&ports, &mut props, "raw").unwrap());
    assert!(sanitize_post_properties(&ports, &mut props, "edit").unwrap());
    assert_eq!(props, edited);
    assert!(!sanitize_post_properties(&ports, &mut props, "edit").unwrap());
    assert_eq!(props, edited);
}

/// The typed field surface passes the context's word and the post ID as PHP's int.
#[test]
fn typed_field_runs_the_display_filter() {
    let hooks = support::hooks();
    let log: Invocations = Rc::default();
    support::add_fixture(
        &hooks,
        &log,
        "post_title",
        &serde_json::json!({"callback": "return_arg", "label": "rec"}),
        10,
        99,
    );
    let formatting = RustFormatting::new(hooks.clone());
    let ports = SanitizePorts {
        hooks: &*hooks,
        formatting: &formatting,
    };
    let out = sanitize_post_field(
        &ports,
        "post_title",
        PhpValue::Str(b"a & b".to_vec()),
        PostId(9),
        Filter::Attribute,
    )
    .unwrap();
    assert_eq!(out, PhpValue::Str(b"a &amp; b".to_vec()));
    assert_eq!(
        log.borrow()[0].1,
        PhpValue::Array(vec![
            (PhpKey::Int(0), PhpValue::Str(b"a & b".to_vec())),
            (PhpKey::Int(1), PhpValue::Int(9)),
            (PhpKey::Int(2), PhpValue::Str(b"attribute".to_vec())),
        ])
    );
}
