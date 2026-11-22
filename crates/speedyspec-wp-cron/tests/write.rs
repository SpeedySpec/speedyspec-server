mod support;

use speedyspec_wp_cron_contracts::{CronResult, EventKey};
use speedyspec_wp_hooks::{HookValue, Key};
use support::*;

fn t() -> bool {
    true
}

fn err(r: CronResult) -> &'static str {
    match r {
        CronResult::Error { code, .. } => code,
        other => panic!("expected an error, got {other:?}"),
    }
}

fn val(r: CronResult) -> HookValue {
    match r {
        CronResult::Value(v) => v,
        other => panic!("expected a value, got {other:?}"),
    }
}

fn empty() -> HookValue {
    arr(vec![])
}

fn key() -> EventKey {
    EventKey::parse(EMPTY_ARGS_KEY).unwrap()
}

fn has(f: &Fixture, ts: i64, hook: &str) -> bool {
    f.api.get_cron_array().unwrap().contains(ts, hook, &key())
}

#[test]
fn single_event_is_scheduled() {
    let f = fixture();
    assert_eq!(
        val(f
            .api
            .schedule_single_event(&HookValue::Int(NOW + 60), &s("h"), &empty(), false)
            .unwrap()),
        HookValue::Bool(true)
    );
    assert!(has(&f, NOW + 60, "h"));
    // `version` 2 is written last.
    assert!(
        matches!(f.store.raw().unwrap(), HookValue::Array(items) if items.last() == Some(&(Key::Str("version".into()), HookValue::Int(2))))
    );
}

#[test]
fn invalid_timestamp() {
    let f = fixture();
    for bad in [HookValue::Int(0), HookValue::Int(-1), s("soon")] {
        assert_eq!(
            err(f
                .api
                .schedule_single_event(&bad, &s("h"), &empty(), t())
                .unwrap()),
            "invalid_timestamp"
        );
        assert_eq!(
            val(f
                .api
                .schedule_single_event(&bad, &s("h"), &empty(), false)
                .unwrap()),
            HookValue::Bool(false)
        );
        assert_eq!(
            err(f
                .api
                .schedule_event(&bad, &s("hourly"), &s("h"), &empty(), t())
                .unwrap()),
            "invalid_timestamp"
        );
        assert_eq!(
            err(f
                .api
                .reschedule_event(&bad, &s("hourly"), &s("h"), &empty(), t())
                .unwrap()),
            "invalid_timestamp"
        );
        assert_eq!(
            err(f
                .api
                .unschedule_event(&bad, &s("h"), &empty(), t())
                .unwrap()),
            "invalid_timestamp"
        );
    }
}

#[test]
fn invalid_schedule() {
    let f = fixture();
    let ts = HookValue::Int(NOW + 60);
    assert_eq!(
        err(f
            .api
            .schedule_event(&ts, &s("nope"), &s("h"), &empty(), t())
            .unwrap()),
        "invalid_schedule"
    );
    assert_eq!(
        val(f
            .api
            .schedule_event(&ts, &s("nope"), &s("h"), &empty(), false)
            .unwrap()),
        HookValue::Bool(false)
    );
    assert_eq!(
        err(f
            .api
            .reschedule_event(&ts, &s("nope"), &s("h"), &empty(), t())
            .unwrap()),
        "invalid_schedule"
    );
}

#[test]
fn duplicate_event() {
    let f = fixture();
    let ts = HookValue::Int(NOW + 60);
    f.api
        .schedule_single_event(&ts, &s("h"), &empty(), false)
        .unwrap();
    assert_eq!(
        err(f
            .api
            .schedule_single_event(&HookValue::Int(NOW + 300), &s("h"), &empty(), t())
            .unwrap()),
        "duplicate_event"
    );
    assert_eq!(
        val(f
            .api
            .schedule_single_event(&HookValue::Int(NOW + 300), &s("h"), &empty(), false)
            .unwrap()),
        HookValue::Bool(false)
    );
}

#[test]
fn duplicate_window_past_near_far() {
    // Past events: anything up to ten minutes from now is a duplicate.
    let f = fixture();
    f.api
        .schedule_single_event(&HookValue::Int(NOW - 100), &s("h"), &empty(), false)
        .unwrap();
    assert_eq!(
        err(f
            .api
            .schedule_single_event(&HookValue::Int(NOW + 590), &s("h"), &empty(), t())
            .unwrap()),
        "duplicate_event"
    );
    // Far future: only within ten minutes either side.
    let g = fixture();
    let far = NOW + 3600;
    g.api
        .schedule_single_event(&HookValue::Int(far), &s("h"), &empty(), false)
        .unwrap();
    assert_eq!(
        err(g
            .api
            .schedule_single_event(&HookValue::Int(far + 599), &s("h"), &empty(), t())
            .unwrap()),
        "duplicate_event"
    );
    assert_eq!(
        val(g
            .api
            .schedule_single_event(&HookValue::Int(far + 601), &s("h"), &empty(), t())
            .unwrap()),
        HookValue::Bool(true)
    );
    assert_eq!(
        val(g
            .api
            .schedule_single_event(&HookValue::Int(far - 1201), &s("h"), &empty(), t())
            .unwrap()),
        HookValue::Bool(true)
    );
}

#[test]
fn schedule_event_false() {
    let f = fixture();
    f.answer("schedule_event", HookValue::Bool(false));
    let ts = HookValue::Int(NOW + 60);
    assert_eq!(
        err(f
            .api
            .schedule_single_event(&ts, &s("h"), &empty(), t())
            .unwrap()),
        "schedule_event_false"
    );
    assert_eq!(
        err(f
            .api
            .schedule_event(&ts, &s("hourly"), &s("h"), &empty(), t())
            .unwrap()),
        "schedule_event_false"
    );
    assert_eq!(
        val(f
            .api
            .schedule_event(&ts, &s("hourly"), &s("h"), &empty(), false)
            .unwrap()),
        HookValue::Bool(false)
    );
}

#[test]
fn pre_filters_false() {
    let ts = HookValue::Int(NOW + 60);
    let f = fixture();
    f.answer("pre_schedule_event", HookValue::Bool(false));
    assert_eq!(
        err(f
            .api
            .schedule_single_event(&ts, &s("h"), &empty(), t())
            .unwrap()),
        "pre_schedule_event_false"
    );
    assert_eq!(
        err(f
            .api
            .schedule_event(&ts, &s("hourly"), &s("h"), &empty(), t())
            .unwrap()),
        "pre_schedule_event_false"
    );
    assert_eq!(
        val(f
            .api
            .schedule_event(&ts, &s("hourly"), &s("h"), &empty(), false)
            .unwrap()),
        HookValue::Bool(false)
    );
    let f = fixture();
    f.answer("pre_reschedule_event", HookValue::Bool(false));
    assert_eq!(
        err(f
            .api
            .reschedule_event(&ts, &s("hourly"), &s("h"), &empty(), t())
            .unwrap()),
        "pre_reschedule_event_false"
    );
    let f = fixture();
    f.answer("pre_unschedule_event", HookValue::Bool(false));
    assert_eq!(
        err(f.api.unschedule_event(&ts, &s("h"), &empty(), t()).unwrap()),
        "pre_unschedule_event_false"
    );
    let f = fixture();
    f.answer("pre_clear_scheduled_hook", HookValue::Bool(false));
    assert_eq!(
        err(f.api.clear_scheduled_hook(&s("h"), &empty(), t()).unwrap()),
        "pre_clear_scheduled_hook_false"
    );
    let f = fixture();
    f.answer("pre_unschedule_hook", HookValue::Bool(false));
    assert_eq!(
        err(f.api.unschedule_hook(&s("h"), t()).unwrap()),
        "pre_unschedule_hook_false"
    );
    assert_eq!(
        val(f.api.unschedule_hook(&s("h"), false).unwrap()),
        HookValue::Bool(false)
    );
}

#[test]
fn pre_filter_wp_error_passes_through_or_false() {
    let ts = HookValue::Int(NOW + 60);
    let f = fixture();
    f.answer("pre_schedule_event", wp_error());
    assert_eq!(
        val(f
            .api
            .schedule_single_event(&ts, &s("h"), &empty(), t())
            .unwrap()),
        wp_error()
    );
    assert_eq!(
        val(f
            .api
            .schedule_single_event(&ts, &s("h"), &empty(), false)
            .unwrap()),
        HookValue::Bool(false)
    );
    let f = fixture();
    f.answer("pre_schedule_event", s("plugin"));
    assert_eq!(
        val(f
            .api
            .schedule_single_event(&ts, &s("h"), &empty(), false)
            .unwrap()),
        s("plugin")
    );
    assert_eq!(f.store.writes(), 0);
}

#[test]
fn could_not_set() {
    let f = fixture();
    // As WordPress's tests start: _set_cron_array( array() ).
    f.store.set_raw(Some(option_with(&[])));
    let ts = HookValue::Int(NOW + 60);
    // Unscheduling an event that is not there changes nothing: update_option() returns false.
    assert_eq!(
        err(f.api.unschedule_event(&ts, &s("h"), &empty(), t()).unwrap()),
        "could_not_set"
    );
    assert_eq!(
        val(f
            .api
            .unschedule_event(&ts, &s("h"), &empty(), false)
            .unwrap()),
        HookValue::Bool(false)
    );
}

#[test]
fn unschedule_removes() {
    let f = fixture();
    let ts = HookValue::Int(NOW + 60);
    f.api
        .schedule_single_event(&ts, &s("h"), &empty(), false)
        .unwrap();
    assert_eq!(
        val(f
            .api
            .unschedule_event(&s(&(NOW + 60).to_string()), &s("h"), &empty(), t())
            .unwrap()),
        HookValue::Bool(true)
    );
    assert!(f.api.get_cron_array().unwrap().is_empty());
}

#[test]
fn clear_and_unschedule_hook_count() {
    let f = fixture();
    f.store.set_raw(Some(option_with(&[
        (NOW + 10, "a"),
        (NOW + 20, "a"),
        (NOW + 30, "b"),
    ])));
    assert_eq!(
        val(f.api.clear_scheduled_hook(&s("a"), &empty(), t()).unwrap()),
        HookValue::Int(2)
    );
    assert_eq!(
        val(f.api.clear_scheduled_hook(&s("a"), &empty(), t()).unwrap()),
        HookValue::Int(0)
    );
    assert_eq!(
        val(f.api.unschedule_hook(&s("b"), t()).unwrap()),
        HookValue::Int(1)
    );
    assert_eq!(
        val(f.api.unschedule_hook(&s("b"), t()).unwrap()),
        HookValue::Int(0)
    );
}

#[test]
fn clear_merges_unschedule_errors() {
    let f = fixture();
    f.store
        .set_raw(Some(option_with(&[(NOW + 10, "a"), (NOW + 20, "a")])));
    f.answer("pre_unschedule_event", HookValue::Bool(false));
    match f.api.clear_scheduled_hook(&s("a"), &empty(), t()).unwrap() {
        CronResult::Merge(errors) => {
            assert_eq!(errors.len(), 2);
            assert!(matches!(
                errors[0],
                CronResult::Error {
                    code: "pre_unschedule_event_false",
                    ..
                }
            ));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        val(f
            .api
            .clear_scheduled_hook(&s("a"), &empty(), false)
            .unwrap()),
        HookValue::Bool(false)
    );
}

#[test]
fn conflict_retries_and_keeps_both() {
    let f = fixture();
    f.store
        .set_raw(Some(option_with(&[(NOW + 100, "theirs_old")])));
    // Another process adds an event between this process's read and its write.
    f.store.before_next_save(|store| {
        store.set_raw(Some(option_with(&[
            (NOW + 100, "theirs_old"),
            (NOW + 200, "theirs_new"),
        ])))
    });
    assert_eq!(
        val(f
            .api
            .schedule_single_event(&HookValue::Int(NOW + 5000), &s("mine"), &empty(), t())
            .unwrap()),
        HookValue::Bool(true)
    );
    assert!(has(&f, NOW + 200, "theirs_new"));
    assert!(has(&f, NOW + 5000, "mine"));
}

#[test]
fn retry_reruns_duplicate_check() {
    let f = fixture();
    f.store.set_raw(Some(option_with(&[])));
    f.store
        .before_next_save(|store| store.set_raw(Some(option_with(&[(NOW + 60, "h")]))));
    assert_eq!(
        err(f
            .api
            .schedule_single_event(&HookValue::Int(NOW + 120), &s("h"), &empty(), t())
            .unwrap()),
        "duplicate_event"
    );
    assert!(!has(&f, NOW + 120, "h"));
}

#[test]
fn five_conflicts_give_could_not_set() {
    let f = fixture();
    f.store.force_conflicts(5);
    assert_eq!(
        err(f
            .api
            .schedule_single_event(&HookValue::Int(NOW + 60), &s("h"), &empty(), t())
            .unwrap()),
        "could_not_set"
    );
    let g = fixture();
    g.store.force_conflicts(4);
    assert_eq!(
        val(g
            .api
            .schedule_single_event(&HookValue::Int(NOW + 60), &s("h"), &empty(), t())
            .unwrap()),
        HookValue::Bool(true)
    );
}

#[test]
fn filtered_reads_write_as_wordpress_does() {
    let f = fixture();
    f.store.force_conflicts(1);
    f.store.reload_reports_filtered(true);
    assert_eq!(
        val(f
            .api
            .schedule_single_event(&HookValue::Int(NOW + 60), &s("h"), &empty(), t())
            .unwrap()),
        HookValue::Bool(true)
    );
    assert!(has(&f, NOW + 60, "h"));
    assert_eq!(f.store.unconditional_saves(), 1);
}

#[test]
fn schedule_sorts_naturally_unschedule_does_not() {
    let f = fixture();
    f.store
        .set_raw(Some(option_with(&[(NOW + 30, "c"), (NOW + 10, "a")])));
    f.api
        .schedule_single_event(&HookValue::Int(NOW + 2000), &s("b"), &empty(), t())
        .unwrap();
    let keys = |f: &Fixture| -> Vec<Key> {
        f.api
            .get_cron_array()
            .unwrap()
            .entries()
            .iter()
            .map(|(k, _)| k.clone())
            .collect()
    };
    assert_eq!(
        keys(&f),
        vec![Key::Int(NOW + 10), Key::Int(NOW + 30), Key::Int(NOW + 2000)]
    );
    let g = fixture();
    g.store.set_raw(Some(option_with(&[
        (NOW + 30, "c"),
        (NOW + 10, "a"),
        (NOW + 20, "b"),
    ])));
    g.api
        .unschedule_event(&HookValue::Int(NOW + 20), &s("b"), &empty(), t())
        .unwrap();
    assert_eq!(keys(&g), vec![Key::Int(NOW + 30), Key::Int(NOW + 10)]);
}

#[test]
fn reschedule_next_time_matches_wordpress() {
    // Past: now + ( interval - ( ( now - timestamp ) % interval ) ).
    let f = fixture();
    f.api
        .reschedule_event(
            &HookValue::Int(NOW - 100),
            &s("hourly"),
            &s("h"),
            &empty(),
            t(),
        )
        .unwrap();
    assert!(has(&f, NOW + 3500, "h"));
    // Future: now + interval.
    let g = fixture();
    g.api
        .reschedule_event(
            &HookValue::Int(NOW + 100),
            &s("hourly"),
            &s("h"),
            &empty(),
            t(),
        )
        .unwrap();
    assert!(has(&g, NOW + 3600, "h"));
    // An unknown schedule borrows the scheduled event's interval.
    let h = fixture();
    h.api
        .schedule_event(
            &HookValue::Int(NOW + 100),
            &s("hourly"),
            &s("h"),
            &empty(),
            t(),
        )
        .unwrap();
    // ... but wp_schedule_event() then refuses the unknown schedule.
    assert_eq!(
        err(h
            .api
            .reschedule_event(
                &HookValue::Int(NOW + 100),
                &s("gone"),
                &s("h"),
                &empty(),
                t()
            )
            .unwrap()),
        "invalid_schedule"
    );
}

#[test]
fn junk_survives_a_native_write() {
    let f = fixture();
    let mut raw = match option_with(&[(NOW + 10, "a")]) {
        HookValue::Array(items) => items,
        _ => unreachable!(),
    };
    raw.insert(0, (Key::Str("junk".into()), s("kept")));
    f.store.set_raw(Some(HookValue::Array(raw)));
    f.api
        .schedule_single_event(&HookValue::Int(NOW + 5000), &s("b"), &empty(), t())
        .unwrap();
    let entries = f.api.get_cron_array().unwrap().entries().to_vec();
    assert!(entries.contains(&(Key::Str("junk".into()), s("kept"))));
}

#[test]
fn set_cron_array_is_unconditional() {
    let f = fixture();
    assert_eq!(
        val(f
            .api
            .set_cron_array(&option_with(&[(NOW, "x")]), t())
            .unwrap()),
        HookValue::Bool(true)
    );
    assert_eq!(
        err(f
            .api
            .set_cron_array(&option_with(&[(NOW, "x")]), t())
            .unwrap()),
        "could_not_set"
    );
    assert_eq!(
        val(f.api.set_cron_array(&s("not an array"), false).unwrap()),
        HookValue::Bool(true)
    );
    assert_eq!(
        f.store.raw(),
        Some(arr(vec![(Key::Str("version".into()), HookValue::Int(2))]))
    );
}

#[test]
fn a_write_between_two_reads_is_never_overwritten() {
    let f = fixture();
    f.store.set_raw(Some(option_with(&[(NOW + 100, "first")])));
    // Another process writes after this one's first read of the option.
    f.store.before_load(2, |store| {
        let HookValue::Array(mut items) = store.raw().unwrap() else {
            unreachable!()
        };
        let HookValue::Array(theirs) = option_with(&[(NOW + 200, "theirs")]) else {
            unreachable!()
        };
        items.insert(0, theirs[0].clone());
        store.set_raw(Some(HookValue::Array(items)));
    });
    assert_eq!(
        val(f
            .api
            .schedule_single_event(&HookValue::Int(NOW + 5000), &s("mine"), &empty(), t())
            .unwrap()),
        HookValue::Bool(true)
    );
    assert!(has(&f, NOW + 200, "theirs"));
    assert!(has(&f, NOW + 5000, "mine"));
}
