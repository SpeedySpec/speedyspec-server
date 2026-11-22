//! Conversions and the WordPress hooks adapter.

use std::rc::Rc;

use speedyspec_operation_contracts::*;
use speedyspec_transport_contracts::TransportHooks;
use speedyspec_transport_hooks_wp::*;
use speedyspec_wp_hooks::{Callback, HookApi, HookName, HookValue, IgnoreDeprecations, Key};
use speedyspec_wp_hooks_memory::MemoryHookStore;

fn api() -> Rc<HookApi> {
    Rc::new(HookApi::new(
        Rc::new(MemoryHookStore::new()),
        Rc::new(IgnoreDeprecations),
    ))
}

fn name(s: &str) -> HookName {
    HookName::new(HookValue::Str(s.into()))
}

#[test]
fn values_round_trip_through_hook_values() {
    let v = Value::map([
        ("ids", Value::List(vec![Value::Int(1), Value::Int(2)])),
        ("t", Value::from("x")),
        ("f", Value::Float(1.5)),
    ]);
    assert_eq!(from_hook(&to_hook(&v)).unwrap(), v);
    assert_eq!(
        from_hook(&to_hook(&Value::Binary(vec![0, 255]))).unwrap(),
        Value::Binary(vec![0, 255])
    );
}

#[test]
fn php_array_shapes() {
    assert_eq!(
        from_hook(&HookValue::Array(vec![])).unwrap(),
        Value::List(vec![])
    );
    let sparse = HookValue::Array(vec![
        (Key::Int(1), HookValue::Int(9)),
        (Key::Int(0), HookValue::Int(8)),
    ]);
    assert_eq!(
        from_hook(&sparse).unwrap(),
        Value::map([("1", Value::Int(9)), ("0", Value::Int(8))])
    );
    let dt = chrono::NaiveDate::from_ymd_opt(2026, 11, 21)
        .unwrap()
        .and_hms_opt(9, 5, 0)
        .unwrap();
    assert_eq!(
        to_hook(&Value::DateTime(dt)),
        HookValue::Str("2026-11-21 09:05:00".into())
    );
}

#[test]
fn opaque_values_cannot_cross() {
    let opaque = HookValue::Opaque(Rc::new(5u8));
    assert_eq!(from_hook(&opaque).unwrap_err().code, "opaque_hook_value");
}

#[test]
fn filters_and_actions_run_through_the_hook_api() {
    let api = api();
    api.add_filter(
        &name("rest_pre_dispatch"),
        Callback::rust("short", |args: &[HookValue]| {
            Ok(match &args[1] {
                HookValue::Str(s) if s == "a/b" => HookValue::Str("bypassed".into()),
                _ => args[0].clone(),
            })
        }),
        &HookValue::Int(10),
        2,
    )
    .unwrap();
    let hooks = WpTransportHooks::new(api.clone());
    assert_eq!(
        hooks
            .filter("rest_pre_dispatch", Value::Null, vec![Value::from("a/b")])
            .unwrap(),
        Value::from("bypassed")
    );
    assert_eq!(
        hooks
            .filter("rest_pre_dispatch", Value::Null, vec![Value::from("c/d")])
            .unwrap(),
        Value::Null
    );
    hooks
        .action("xmlrpc_call", vec![Value::from("wp.getPost")])
        .unwrap();
    assert_eq!(api.did_action(&name("xmlrpc_call")).unwrap(), 1);
}

#[test]
fn a_filter_that_changes_nothing_returns_the_value_it_was_given() {
    let api = api();
    let hooks = WpTransportHooks::new(api.clone());
    let dt = chrono::NaiveDate::from_ymd_opt(2026, 1, 2)
        .unwrap()
        .and_hms_micro_opt(3, 4, 5, 678)
        .unwrap();
    let value = Value::map([
        ("empty", Value::map::<&str>([])),
        ("when", Value::DateTime(dt)),
        (
            "keys",
            Value::map([("0", Value::Int(1)), ("1", Value::Int(2))]),
        ),
    ]);
    // No callbacks at all.
    assert_eq!(
        hooks
            .filter("rest_post_dispatch", value.clone(), vec![])
            .unwrap(),
        value
    );
    // A callback that hands the value straight back.
    api.add_filter(
        &name("rest_post_dispatch"),
        Callback::rust("same", |args: &[HookValue]| Ok(args[0].clone())),
        &HookValue::Int(10),
        1,
    )
    .unwrap();
    assert_eq!(
        hooks
            .filter("rest_post_dispatch", value.clone(), vec![])
            .unwrap(),
        value
    );
}
