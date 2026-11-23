//! The network walk: every active site's due events in one call, resuming where the last call stopped.

mod support;

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use speedyspec_wp_cron::memory::{CollectingErrors, MemoryDoingCron, MemoryNetwork, RecordingWake};
use speedyspec_wp_cron::{CronApi, CronParts, RustArgsKeyer};
use speedyspec_wp_hooks::{Callback, HookApi, HookName, HookValue, IgnoreDeprecations};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use support::*;

struct Net {
    api: CronApi,
    network: Rc<MemoryNetwork>,
    hooks: Rc<HookApi>,
    wake: Rc<RecordingWake>,
}

fn net(sites: &[u64]) -> Net {
    let hooks = Rc::new(HookApi::new(
        Rc::new(MemoryHookStore::new()),
        Rc::new(IgnoreDeprecations),
    ));
    let network = Rc::new(MemoryNetwork::new(sites));
    let wake = Rc::new(RecordingWake::new());
    let api = CronApi::new(CronParts {
        hooks: hooks.clone(),
        store: network.clone(),
        keyer: Rc::new(RustArgsKeyer),
        clock: Rc::new(FixedClock(Cell::new(NOW as f64 + 0.5))),
        translator: Rc::new(SameText),
        objects: Rc::new(ArrayObjects),
        wake: wake.clone(),
        doing_cron: Rc::new(MemoryDoingCron::new()),
        errors: Rc::new(CollectingErrors::new()),
        network: network.clone(),
        switcher: network.clone(),
    });
    Net {
        api,
        network,
        hooks,
        wake,
    }
}

/// Site `site` holds one event per `(offset from now, hook)`.
fn seed(n: &Net, site: u64, events: &[(i64, &str)]) {
    let rows: Vec<(i64, &str)> = events.iter().map(|(o, h)| (NOW + o, *h)).collect();
    n.network.store(site).set_raw(Some(option_with(&rows)));
}

fn budget() -> Duration {
    Duration::from_secs(30)
}

#[test]
fn single_site_calls_no_port() {
    let f = fixture();
    f.store.set_raw(Some(option_with(&[(NOW - 10, "a")])));
    let summary = f.api.run_network_due_events(budget()).unwrap();
    assert_eq!((summary.ran, summary.sites), (1, 1));
}

#[test]
fn every_site_runs() {
    let n = net(&[1, 2, 3]);
    for site in [1, 2, 3] {
        seed(&n, site, &[(-10, "due"), (500 + site as i64, "later")]);
    }
    let summary = n.api.run_network_due_events(budget()).unwrap();
    assert_eq!((summary.ran, summary.sites), (3, 3));
    for site in [1, 2, 3] {
        assert_eq!(
            n.network.store(site).raw(),
            Some(option_with(&[(NOW + 500 + site as i64, "later")]))
        );
    }
    assert_eq!(n.network.switches(), vec![1, 2, 3]);
    assert_eq!(n.network.current(), 1, "the starting site is restored");
}

#[test]
fn budget_resumes_at_the_next_site() {
    let n = net(&[1, 2, 3]);
    for site in [1, 2, 3] {
        seed(&n, site, &[(-10, "due")]);
    }
    let first = n.api.run_network_due_events(Duration::ZERO).unwrap();
    assert_eq!((first.ran, first.sites, first.next), (1, 1, Some(NOW)));
    assert_eq!(n.network.cursor_value(), Some(1));
    let second = n.api.run_network_due_events(Duration::ZERO).unwrap();
    assert_eq!(second.ran, 1);
    assert_eq!(n.network.switches(), vec![1, 2]);
}

#[test]
fn missing_cursor_site_starts_at_the_next() {
    let n = net(&[1, 3, 5]);
    n.network.set_cursor_value(Some(4));
    n.api.run_network_due_events(budget()).unwrap();
    assert_eq!(n.network.switches(), vec![5, 1, 3]);
}

#[test]
fn one_site_runs_once_per_call() {
    let n = net(&[7]);
    seed(&n, 7, &[(-10, "due")]);
    n.network.set_cursor_value(Some(7));
    let summary = n.api.run_network_due_events(budget()).unwrap();
    assert_eq!((summary.ran, summary.sites), (1, 1));
    assert_eq!(n.network.switches(), vec![7]);
}

#[test]
fn throwing_site_restores_and_moves_the_cursor() {
    let n = net(&[1, 2]);
    seed(&n, 1, &[(-10, "boom")]);
    seed(&n, 2, &[(-10, "fine")]);
    n.hooks
        .add_filter(
            &HookName::from("boom"),
            Callback::rust("throw", |_: &[HookValue]| {
                Err(speedyspec_wp_hooks::HookError::message("boom"))
            }),
            &HookValue::Int(10),
            0,
        )
        .unwrap();
    assert!(n.api.run_network_due_events(budget()).is_err());
    assert_eq!(n.network.current(), 1, "restored");
    assert_eq!(n.network.depth(), 0, "every switch restored");
    assert_eq!(n.network.cursor_value(), Some(1));
    // The next call starts after the throwing site, then wraps to it (its event was claimed before it threw).
    let next = n.api.run_network_due_events(budget()).unwrap();
    assert_eq!(n.network.switches(), vec![1, 2, 1]);
    assert_eq!(next.ran, 1);
}

#[test]
fn budget_spent_after_the_last_site() {
    let n = net(&[1, 2]);
    seed(&n, 2, &[(-10, "due")]);
    n.network.set_cursor_value(Some(1));
    let summary = n.api.run_network_due_events(Duration::ZERO).unwrap();
    assert_eq!((summary.ran, summary.next), (1, Some(NOW)));
    assert_eq!(n.network.cursor_value(), Some(2));
    let again = n.api.run_network_due_events(Duration::ZERO).unwrap();
    assert_eq!(again.sites, 1);
    assert_eq!(
        n.network.switches(),
        vec![2, 1],
        "wrapped to the first site"
    );
}

#[test]
fn wake_is_earliest_across_sites() {
    let n = net(&[1, 2]);
    seed(&n, 1, &[(900, "late")]);
    seed(&n, 2, &[(300, "soon")]);
    let summary = n.api.run_network_due_events(budget()).unwrap();
    assert_eq!(summary.next, Some(NOW + 300));
    assert_eq!(n.wake.earliest(), Some(NOW + 300));
}
