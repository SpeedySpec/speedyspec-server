mod support;

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use speedyspec_wp_cron::memory::MemoryCronStore;
use speedyspec_wp_cron_contracts::{CronStep, RunSummary};
use speedyspec_wp_hooks::{Callback, HookName, HookValue, Key};
use support::*;

fn budget() -> Duration {
    Duration::from_secs(30)
}

/// Counts runs of `hook` on this fixture.
fn count(f: &Fixture, hook: &str) -> Rc<Cell<u32>> {
    let runs = Rc::new(Cell::new(0));
    let r = runs.clone();
    f.hooks
        .add_filter(
            &HookName::from(hook),
            Callback::rust(&format!("count_{hook}"), move |_: &[HookValue]| {
                r.set(r.get() + 1);
                Ok(HookValue::Null)
            }),
            &HookValue::Int(10),
            99,
        )
        .unwrap();
    runs
}

fn t(v: i64) -> HookValue {
    HookValue::Int(v)
}

fn empty() -> HookValue {
    arr(vec![])
}

#[test]
fn runs_due_in_order_and_skips_future() {
    let f = fixture();
    f.store.set_raw(Some(option_with(&[
        (NOW - 20, "a"),
        (NOW - 10, "b"),
        (NOW + 100, "c"),
    ])));
    for h in ["a", "b", "c"] {
        f.record(h);
    }
    let summary = f.api.run_due_events(budget()).unwrap();
    assert_eq!(f.ran(), ["a", "b"]);
    assert_eq!(
        summary,
        RunSummary {
            ran: 2,
            claimed_by_others: 0,
            unclaimed: 0,
            sites: 1,
            next: Some(NOW + 100)
        }
    );
    // The run reports when cron is next due, not the due events its claims left behind for a moment.
    assert_eq!(f.wake.earliest(), Some(NOW + 100));
    let left: Vec<String> = f
        .api
        .get_cron_array()
        .unwrap()
        .events()
        .into_iter()
        .map(|e| e.hook)
        .collect();
    assert_eq!(left, ["c"]);
}

#[test]
fn recurring_gets_one_next_occurrence() {
    let f = fixture();
    f.api
        .schedule_event(&t(NOW - 100), &s("hourly"), &s("r"), &empty(), true)
        .unwrap();
    let runs = count(&f, "r");
    f.api.run_due_events(budget()).unwrap();
    assert_eq!(runs.get(), 1);
    let events = f.api.get_cron_array().unwrap().events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].timestamp, NOW + 3500);
    assert_eq!(events[0].data.interval, Some(3600));
}

#[test]
fn two_runners_run_each_event_once() {
    let store = Rc::new(MemoryCronStore::new());
    let a = fixture_over(store.clone());
    let b = Rc::new(fixture_over(store.clone()));
    a.store.set_raw(Some(option_with(&[
        (NOW - 30, "x"),
        (NOW - 20, "y"),
        (NOW - 10, "z"),
    ])));
    let runs_a: Vec<_> = ["x", "y", "z"].iter().map(|h| count(&a, h)).collect();
    let runs_b: Vec<_> = ["x", "y", "z"].iter().map(|h| count(&b, h)).collect();
    // Runner B runs everything between runner A's first read and its first claim.
    let other = b.clone();
    store.before_next_save(move |_| {
        other.api.run_due_events(Duration::from_secs(30)).unwrap();
    });
    let summary = a.api.run_due_events(budget()).unwrap();
    for i in 0..3 {
        assert_eq!(runs_a[i].get() + runs_b[i].get(), 1, "event {i}");
    }
    assert_eq!(summary.ran + summary.claimed_by_others, 3);
    assert!(summary.claimed_by_others >= 1);
}

#[test]
fn claimed_by_others_fires_no_hooks() {
    let f = fixture();
    f.store.set_raw(Some(option_with(&[(NOW - 10, "x")])));
    let store = f.store.clone();
    // Another process claims the event between this runner's read and its write.
    f.store
        .before_next_save(move |s| s.set_raw(Some(option_with(&[]))));
    let _ = store;
    let runs = count(&f, "x");
    f.record("cron_unschedule_event_error");
    let summary = f.api.run_due_events(budget()).unwrap();
    assert_eq!(runs.get(), 0);
    assert_eq!(summary.claimed_by_others, 1);
    assert!(f.ran().is_empty());
}

#[test]
fn reschedule_onto_existing_slot_keeps_one() {
    let f = fixture();
    f.api
        .schedule_event(&t(NOW - 100), &s("hourly"), &s("r"), &empty(), true)
        .unwrap();
    // The next occurrence already exists: WordPress overwrites the same key.
    f.api
        .schedule_event(&t(NOW + 3500), &s("hourly"), &s("r"), &empty(), true)
        .unwrap();
    f.api.run_due_events(budget()).unwrap();
    let events = f.api.get_cron_array().unwrap().events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].timestamp, NOW + 3500);
}

#[test]
fn callback_unscheduling_itself_sticks() {
    let f = Rc::new(fixture());
    f.api
        .schedule_event(&t(NOW - 100), &s("hourly"), &s("r"), &empty(), true)
        .unwrap();
    let me = f.clone();
    f.hooks
        .add_filter(
            &HookName::from("r"),
            Callback::rust("unschedule_self", move |_: &[HookValue]| {
                me.api
                    .unschedule_hook(&HookValue::Str("r".into()), false)
                    .unwrap();
                Ok(HookValue::Null)
            }),
            &HookValue::Int(10),
            0,
        )
        .unwrap();
    f.api.run_due_events(budget()).unwrap();
    assert!(f.api.get_cron_array().unwrap().events().is_empty());
}

#[test]
fn no_callback_still_claimed() {
    let f = fixture();
    f.store.set_raw(Some(option_with(&[(NOW - 10, "nobody")])));
    let summary = f.api.run_due_events(budget()).unwrap();
    assert_eq!(summary.ran, 1);
    assert_eq!(f.hooks.did_action(&HookName::from("nobody")).unwrap(), 1);
    assert!(f.api.get_cron_array().unwrap().is_empty());
}

#[test]
fn pre_get_ready_cron_jobs_uses_wordpress_sequence() {
    let f = fixture();
    f.store.set_raw(Some(option_with(&[(NOW - 10, "a")])));
    // A plugin hands back the same list itself: no claim is possible, so WordPress's two calls run.
    let ready = f.api.get_ready_cron_jobs().unwrap();
    f.answer("pre_get_ready_cron_jobs", ready);
    f.record("pre_unschedule_event");
    let runs = count(&f, "a");
    f.api.run_due_events(budget()).unwrap();
    assert_eq!(runs.get(), 1);
    assert_eq!(f.ran(), ["pre_unschedule_event"]);
}

#[test]
fn pre_unschedule_short_circuit_uses_wordpress_sequence() {
    let f = fixture();
    f.store.set_raw(Some(option_with(&[(NOW - 10, "a")])));
    f.answer("pre_unschedule_event", HookValue::Bool(true));
    let runs = count(&f, "a");
    f.api.run_due_events(budget()).unwrap();
    // The plugin said it unscheduled it; the event runs and the store is not ours to change.
    assert_eq!(runs.get(), 1);
    assert_eq!(f.api.get_cron_array().unwrap().events().len(), 1);
}

#[test]
fn reschedule_error_logs_and_fires_action() {
    let f = fixture();
    f.api
        .schedule_event(&t(NOW - 100), &s("hourly"), &s("r"), &empty(), true)
        .unwrap();
    f.answer("pre_reschedule_event", HookValue::Bool(false));
    f.record("cron_reschedule_event_error");
    let runs = count(&f, "r");
    f.api.run_due_events(budget()).unwrap();
    assert_eq!(runs.get(), 1);
    assert_eq!(
        f.errors.reported(),
        vec![(
            CronStep::Reschedule,
            "r".to_string(),
            "pre_reschedule_event_false".to_string()
        )]
    );
    assert_eq!(f.ran(), ["cron_reschedule_event_error"]);
    let args = &f.log.borrow()[0].1;
    assert_eq!(args[1], s("r"));
    assert!(
        matches!(&args[2], HookValue::Array(items) if items.iter().any(|(k, _)| *k == Key::Str("interval".into())))
    );
    // Unscheduled all the same.
    assert!(f.api.get_cron_array().unwrap().is_empty());
}

#[test]
fn budget_stops_and_reports_now() {
    let f = fixture();
    f.store
        .set_raw(Some(option_with(&[(NOW - 20, "a"), (NOW - 10, "b")])));
    let summary = f.api.run_due_events(Duration::ZERO).unwrap();
    assert_eq!(summary.ran, 1);
    assert_eq!(summary.next, Some(NOW));
    assert_eq!(f.wake.earliest(), Some(NOW));
}

#[test]
fn doing_cron_set_then_deleted() {
    let f = fixture();
    f.store.set_raw(Some(option_with(&[(NOW - 10, "a")])));
    f.api.run_due_events(budget()).unwrap();
    assert_eq!(
        f.doing.history(),
        vec![Some("1700000000.5000000000000000000000".to_string()), None]
    );
}

#[test]
fn writes_report_earliest_to_wake() {
    let f = fixture();
    f.api
        .schedule_single_event(&t(NOW + 900), &s("w"), &empty(), true)
        .unwrap();
    assert_eq!(f.wake.earliest(), Some(NOW + 900));
    f.api
        .schedule_single_event(&t(NOW + 300), &s("v"), &empty(), true)
        .unwrap();
    assert_eq!(f.wake.earliest(), Some(NOW + 300));
}

#[test]
fn nothing_due_reports_earliest() {
    let f = fixture();
    f.store.set_raw(Some(option_with(&[(NOW + 600, "later")])));
    let summary = f.api.run_due_events(budget()).unwrap();
    assert_eq!(
        summary,
        RunSummary {
            ran: 0,
            claimed_by_others: 0,
            unclaimed: 0,
            sites: 1,
            next: Some(NOW + 600)
        }
    );
    assert!(f.doing.history().is_empty());
}

#[test]
fn reschedule_error_is_reported_before_unschedule() {
    let f = fixture();
    f.api
        .schedule_event(&t(NOW - 100), &s("hourly"), &s("r"), &empty(), true)
        .unwrap();
    f.answer("pre_reschedule_event", HookValue::Bool(false));
    f.record("cron_reschedule_event_error");
    f.record("pre_unschedule_event");
    f.api.run_due_events(budget()).unwrap();
    // wp-cron.php reports the reschedule's error before it unschedules.
    assert_eq!(
        f.ran(),
        ["cron_reschedule_event_error", "pre_unschedule_event"]
    );
}

#[test]
fn a_claim_that_cannot_be_written_is_left_for_the_next_call() {
    let f = fixture();
    f.store.set_raw(Some(option_with(&[(NOW - 10, "x")])));
    let runs = count(&f, "x");
    f.store.force_conflicts(5);
    let summary = f.api.run_due_events(budget()).unwrap();
    // Other runners are writing: one of them may claim it; this runner must not run it unclaimed.
    assert_eq!(runs.get(), 0);
    assert_eq!(summary.unclaimed, 1);
    assert_eq!(summary.ran, 0);
    assert_eq!(f.api.get_cron_array().unwrap().events().len(), 1);
    assert_eq!(summary.next, Some(NOW - 10));
}
