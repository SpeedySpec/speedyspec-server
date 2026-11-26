//! The subsites to call: which other sites of the network have an event due now (each is then run by its own
//! request, booted as that site).

mod support;

use speedyspec_wp_cron::memory::{CollectingErrors, MemoryDoingCron, MemoryNetwork, RecordingWake};
use speedyspec_wp_cron::{CronApi, CronParts, RustArgsKeyer};
use speedyspec_wp_cron_contracts::{CronError, NetworkSites, SiteRef, SiteSwitcher};
use speedyspec_wp_hooks::{WpHookApi, IgnoreDeprecations};
use speedyspec_wp_hooks_contracts::{Callback, HookName, HookValue};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use std::cell::Cell;
use std::rc::Rc;
use support::*;

struct Net {
    api: CronApi,
    network: Rc<MemoryNetwork>,
    hooks: Rc<WpHookApi>,
    wake: Rc<RecordingWake>,
}

fn net(sites: &[u64]) -> Net {
    let hooks = Rc::new(WpHookApi::new(
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

fn ids(sites: &[SiteRef]) -> Vec<u64> {
    sites.iter().map(|s| s.id).collect()
}

/// A single site that fails the test if asked anything but `is_multisite`.
struct OnlyIsMultisite;

impl NetworkSites for OnlyIsMultisite {
    fn is_multisite(&self) -> bool {
        false
    }
    fn sites(&self) -> Result<Vec<SiteRef>, CronError> {
        panic!("sites() on a single site")
    }
    fn current(&self) -> u64 {
        panic!("current() on a single site")
    }
}

impl SiteSwitcher for OnlyIsMultisite {
    fn switch_to(&self, _: u64) -> Result<(), CronError> {
        panic!("switch_to() on a single site")
    }
    fn restore(&self) -> Result<(), CronError> {
        panic!("restore() on a single site")
    }
}

#[test]
fn single_site_lists_nothing_and_calls_no_port() {
    let hooks = Rc::new(WpHookApi::new(
        Rc::new(MemoryHookStore::new()),
        Rc::new(IgnoreDeprecations),
    ));
    let api = CronApi::new(CronParts {
        hooks,
        store: Rc::new(speedyspec_wp_cron::memory::MemoryCronStore::new()),
        keyer: Rc::new(RustArgsKeyer),
        clock: Rc::new(FixedClock(Cell::new(NOW as f64 + 0.5))),
        translator: Rc::new(SameText),
        objects: Rc::new(ArrayObjects),
        wake: Rc::new(RecordingWake::new()),
        doing_cron: Rc::new(MemoryDoingCron::new()),
        errors: Rc::new(CollectingErrors::new()),
        network: Rc::new(OnlyIsMultisite),
        switcher: Rc::new(OnlyIsMultisite),
    });
    assert_eq!(api.due_sites().unwrap(), Vec::new());
}

#[test]
fn lists_due_sites_in_id_order() {
    let n = net(&[1, 2, 3, 4]);
    seed(&n, 2, &[(-10, "due")]);
    seed(&n, 3, &[(600, "later")]);
    seed(&n, 4, &[(0, "now"), (900, "later")]);
    let due = n.api.due_sites().unwrap();
    assert_eq!(ids(&due), vec![2, 4]);
    assert_eq!(
        due[0],
        SiteRef {
            id: 2,
            domain: "site2.test".into(),
            path: "/".into()
        }
    );
    assert_eq!(n.network.switches(), vec![2, 3, 4]);
    assert_eq!(n.network.depth(), 0, "every switch restored");
}

#[test]
fn never_lists_the_current_site() {
    let n = net(&[1, 2]);
    seed(&n, 1, &[(-10, "due")]);
    assert_eq!(n.api.due_sites().unwrap(), Vec::new());
    assert_eq!(n.network.switches(), vec![2]);
}

#[test]
fn future_site_wakes_not_listed() {
    let n = net(&[1, 2]);
    seed(&n, 2, &[(300, "soon")]);
    assert_eq!(n.api.due_sites().unwrap(), Vec::new());
    assert_eq!(n.wake.earliest(), Some(NOW + 300));
}

#[test]
fn wake_is_earliest_across_sites() {
    let n = net(&[1, 2, 3]);
    seed(&n, 2, &[(-5, "due"), (900, "late")]);
    seed(&n, 3, &[(300, "soon")]);
    assert_eq!(ids(&n.api.due_sites().unwrap()), vec![2]);
    assert_eq!(n.wake.earliest(), Some(NOW + 300));
}

#[test]
fn failed_switch_restores_and_skips_the_site() {
    let n = net(&[1, 2, 3]);
    seed(&n, 3, &[(-10, "due")]);
    n.network.fail_switch_to(2);
    assert_eq!(
        ids(&n.api.due_sites().unwrap()),
        vec![3],
        "the failing site is skipped, the rest listed"
    );
    assert_eq!(n.network.depth(), 0, "the failed switch is restored");
    assert_eq!(n.network.current(), 1);
}

#[test]
fn nothing_runs_while_switched() {
    let n = net(&[1, 2]);
    seed(&n, 2, &[(-10, "due")]);
    let ran = Rc::new(Cell::new(0));
    let counter = ran.clone();
    n.hooks
        .add_filter(
            &HookName::from("due"),
            Callback::rust("count", move |_: &[HookValue]| {
                counter.set(counter.get() + 1);
                Ok(HookValue::Null)
            }),
            &HookValue::Int(10),
            0,
        )
        .unwrap();
    assert_eq!(ids(&n.api.due_sites().unwrap()), vec![2]);
    assert_eq!(ran.get(), 0);
    assert_eq!(
        n.network.store(2).raw(),
        Some(option_with(&[(NOW - 10, "due")])),
        "the event is still there for site 2's own call"
    );
}

#[test]
fn legacy_option_is_upgraded_in_its_own_site() {
    let n = net(&[1, 2]);
    // A version-less (pre-2.1) option: `_get_cron_array()` upgrades and saves it.
    let legacy = HookValue::Array(vec![(
        speedyspec_wp_hooks_contracts::Key::Int(NOW - 10),
        HookValue::Array(vec![(
            speedyspec_wp_hooks_contracts::Key::Str("legacy".into()),
            HookValue::Array(vec![(
                speedyspec_wp_hooks_contracts::Key::Str("args".into()),
                HookValue::Array(vec![]),
            )]),
        )]),
    )]);
    n.network.store(2).set_raw(Some(legacy));
    assert_eq!(ids(&n.api.due_sites().unwrap()), vec![2]);
    assert_eq!(n.network.depth(), 0);
    assert_eq!(
        n.network.store(1).raw(),
        None,
        "the main site's option is untouched"
    );
    let upgraded = n.network.store(2).raw().expect("site 2 saved");
    let HookValue::Array(items) = upgraded else {
        panic!("an array")
    };
    assert!(
        items
            .iter()
            .any(|(k, _)| *k == speedyspec_wp_hooks_contracts::Key::Str("version".into())),
        "upgraded in site 2's own store"
    );
}
