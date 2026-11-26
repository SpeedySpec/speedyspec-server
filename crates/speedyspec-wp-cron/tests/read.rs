mod support;

use speedyspec_wp_hooks_contracts::{HookValue, Key};
use support::*;

#[test]
fn version_less_array_is_upgraded_and_saved() {
    let f = fixture();
    // WordPress < 2.1's shape: [timestamp => [hook => ['args' => ..., 'schedule' => ...]]].
    let old = arr(vec![(
        Key::Int(NOW + 60),
        assoc(&[(
            "old_hook",
            assoc(&[("schedule", HookValue::Bool(false)), ("args", arr(vec![]))]),
        )]),
    )]);
    f.store.set_raw(Some(old));
    let cron = f.api.get_cron_array().unwrap();
    let events = cron.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].key.as_str(), EMPTY_ARGS_KEY);
    // The upgrade wrote the new shape, version included.
    let stored = f.store.raw().unwrap();
    assert!(
        matches!(&stored, HookValue::Array(items) if items.last() == Some(&(Key::Str("version".into()), HookValue::Int(2))))
    );
}

#[test]
fn non_array_option_is_empty() {
    let f = fixture();
    f.store.set_raw(Some(s("nonsense")));
    assert!(f.api.get_cron_array().unwrap().is_empty());
    f.store.set_raw(None);
    assert!(f.api.get_cron_array().unwrap().is_empty());
}

#[test]
fn schedules_have_wordpress_intervals() {
    let f = fixture();
    let schedules = f.api.get_schedules().unwrap();
    let HookValue::Array(items) = schedules else {
        panic!("not an array")
    };
    let intervals: Vec<(Key, HookValue)> = items
        .into_iter()
        .map(|(k, v)| match v {
            HookValue::Array(fields) => (k, fields[0].1.clone()),
            _ => panic!(),
        })
        .collect();
    assert_eq!(
        intervals,
        vec![
            (Key::Str("hourly".into()), HookValue::Int(3600)),
            (Key::Str("twicedaily".into()), HookValue::Int(43200)),
            (Key::Str("daily".into()), HookValue::Int(86400)),
            (Key::Str("weekly".into()), HookValue::Int(604800)),
        ]
    );
}

#[test]
fn cron_schedules_filter_adds_but_cannot_override_builtins() {
    let f = fixture();
    f.answer(
        "cron_schedules",
        assoc(&[
            (
                "minute",
                assoc(&[("interval", HookValue::Int(60)), ("display", s("Minute"))]),
            ),
            (
                "hourly",
                assoc(&[("interval", HookValue::Int(1)), ("display", s("Fake"))]),
            ),
        ]),
    );
    let HookValue::Array(items) = f.api.get_schedules().unwrap() else {
        panic!()
    };
    let names: Vec<Key> = items.iter().map(|(k, _)| k.clone()).collect();
    // array_merge( $filtered, $builtins ): the filter's keys first, the built-ins win.
    assert_eq!(names[0], Key::Str("minute".into()));
    assert_eq!(names[1], Key::Str("hourly".into()));
    assert_eq!(names.len(), 5);
    let hourly = &items[1].1;
    assert!(matches!(hourly, HookValue::Array(f) if f[0].1 == HookValue::Int(3600)));
}

#[test]
fn get_schedule_false_when_absent() {
    let f = fixture();
    f.record("get_schedule");
    assert_eq!(
        f.api.get_schedule(&s("nope"), &arr(vec![])).unwrap(),
        HookValue::Bool(false)
    );
    assert_eq!(f.ran(), ["get_schedule"]);
}

#[test]
fn scheduled_event_null_timestamp_finds_next() {
    let f = fixture();
    f.store.set_raw(Some(option_with(&[
        (NOW + 10, "a"),
        (NOW + 20, "b"),
        (NOW + 30, "a"),
    ])));
    let event = f
        .api
        .get_scheduled_event(&s("a"), &arr(vec![]), &HookValue::Null)
        .unwrap();
    assert_eq!(prop(&event, "timestamp"), Some(HookValue::Int(NOW + 10)));
    assert_eq!(prop(&event, "schedule"), Some(HookValue::Bool(false)));
    assert_eq!(prop(&event, "hook"), Some(s("a")));
    assert_eq!(prop(&event, "interval"), None);
    let at = f
        .api
        .get_scheduled_event(&s("a"), &arr(vec![]), &HookValue::Int(NOW + 30))
        .unwrap();
    assert_eq!(prop(&at, "timestamp"), Some(HookValue::Int(NOW + 30)));
    assert_eq!(
        f.api
            .get_scheduled_event(&s("a"), &arr(vec![]), &HookValue::Int(NOW + 20))
            .unwrap(),
        HookValue::Bool(false)
    );
    assert_eq!(
        f.api
            .get_scheduled_event(&s("a"), &arr(vec![]), &s("soon"))
            .unwrap(),
        HookValue::Bool(false)
    );
}

#[test]
fn pre_get_scheduled_event_short_circuits() {
    let f = fixture();
    f.answer("pre_get_scheduled_event", s("plugin"));
    assert_eq!(
        f.api
            .get_scheduled_event(&s("a"), &arr(vec![]), &HookValue::Null)
            .unwrap(),
        s("plugin")
    );
}

#[test]
fn ready_jobs_stop_at_now() {
    let f = fixture();
    f.store.set_raw(Some(option_with(&[
        (NOW - 10, "a"),
        (NOW, "b"),
        (NOW + 10, "c"),
    ])));
    let HookValue::Array(ready) = f.api.get_ready_cron_jobs().unwrap() else {
        panic!()
    };
    let keys: Vec<Key> = ready.into_iter().map(|(k, _)| k).collect();
    assert_eq!(keys, vec![Key::Int(NOW - 10), Key::Int(NOW)]);
}

#[test]
fn pre_get_ready_cron_jobs_short_circuits() {
    let f = fixture();
    f.answer("pre_get_ready_cron_jobs", arr(vec![]));
    f.store.set_raw(Some(option_with(&[(NOW - 10, "a")])));
    assert_eq!(f.api.get_ready_cron_jobs().unwrap(), arr(vec![]));
}

#[test]
fn next_scheduled_filters_timestamp() {
    let f = fixture();
    f.store.set_raw(Some(option_with(&[(NOW + 10, "a")])));
    f.record("wp_next_scheduled");
    assert_eq!(
        f.api.next_scheduled(&s("a"), &arr(vec![])).unwrap(),
        HookValue::Int(NOW + 10)
    );
    let args = &f.log.borrow()[0].1;
    assert_eq!(args[0], HookValue::Int(NOW + 10));
    assert_eq!(prop(&args[1], "hook"), Some(s("a")));
    assert_eq!(args[2], s("a"));
    assert_eq!(
        f.api.next_scheduled(&s("none"), &arr(vec![])).unwrap(),
        HookValue::Bool(false)
    );
}
