//! `OptionCronStore` over the options port's memory repository: versions, conflicts, reloads.

use std::rc::Rc;

use speedyspec_wp_cron_contracts::{CronStore, SaveOutcome, Version};
use speedyspec_wp_cron_options::OptionCronStore;
use speedyspec_wp_hooks::{Callback, HookApi, HookName, HookValue, IgnoreDeprecations, Key};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_object_cache::{CurrentObjectCache, ObjectCachePorts};
use speedyspec_wp_options::{
    OptionName, OptionRepository, OptionsApi, OptionsDeps, SerializedValue,
};
use speedyspec_wp_options_memory::MemoryOptionRepository;

struct Rig {
    store: OptionCronStore,
    hooks: Rc<HookApi>,
    repo: Rc<MemoryOptionRepository>,
}

fn rig_over(repo: Rc<MemoryOptionRepository>) -> Rig {
    let hooks = Rc::new(HookApi::new(
        Rc::new(MemoryHookStore::new()),
        Rc::new(IgnoreDeprecations),
    ));
    let cache = Rc::new(CurrentObjectCache::new(ObjectCachePorts::rust(), None));
    let options = Rc::new(OptionsApi::new(OptionsDeps::rust(
        hooks.clone(),
        cache,
        repo.clone(),
    )));
    Rig {
        store: OptionCronStore::new(options),
        hooks,
        repo,
    }
}

fn rig() -> Rig {
    rig_over(Rc::new(MemoryOptionRepository::new()))
}

fn cron(n: i64) -> HookValue {
    HookValue::Array(vec![
        (Key::Int(n), HookValue::Array(vec![])),
        (Key::Str("version".into()), HookValue::Int(2)),
    ])
}

/// Another process's write, behind every cache.
fn write_behind(r: &Rig, value: &[u8]) {
    r.repo
        .update(
            &OptionName::new("cron").unwrap(),
            Some(&SerializedValue::new(value.to_vec())),
            None,
        )
        .unwrap();
}

#[test]
fn load_save_if_round_trip() {
    let r = rig();
    let (value, version) = r.store.load().unwrap();
    assert_eq!(value, HookValue::Bool(false));
    assert_eq!(version, Version(None));
    assert_eq!(
        r.store.save_if(&version, cron(1)).unwrap(),
        SaveOutcome::Written
    );
    let (value, version) = r.store.load().unwrap();
    assert_eq!(value, cron(1));
    assert_eq!(
        r.store.save_if(&version, cron(1)).unwrap(),
        SaveOutcome::Unchanged
    );
    assert_eq!(
        r.store.save_if(&version, cron(2)).unwrap(),
        SaveOutcome::Written
    );
    // A stale version conflicts.
    assert_eq!(
        r.store.save_if(&version, cron(3)).unwrap(),
        SaveOutcome::Conflict
    );
}

#[test]
fn stale_cache_conflicts_then_reload_reads_fresh() {
    let r = rig();
    let (_, v) = r.store.load().unwrap();
    r.store.save_if(&v, cron(1)).unwrap();
    let (_, version) = r.store.load().unwrap();
    write_behind(&r, b"a:2:{i:7;a:0:{}s:7:\"version\";i:2;}");
    assert_eq!(
        r.store.save_if(&version, cron(9)).unwrap(),
        SaveOutcome::Conflict
    );
    let (value, fresh, stored) = r.store.reload().unwrap();
    assert!(stored);
    assert_eq!(value, cron(7));
    assert_eq!(
        r.store.save_if(&fresh, cron(9)).unwrap(),
        SaveOutcome::Written
    );
}

#[test]
fn filtered_value_reports_not_stored() {
    let r = rig();
    let (_, v) = r.store.load().unwrap();
    r.store.save_if(&v, cron(1)).unwrap();
    r.hooks
        .add_filter(
            &HookName::from("option_cron"),
            Callback::rust("rewrite", |_: &[HookValue]| Ok(cron(5))),
            &HookValue::Int(10),
            1,
        )
        .unwrap();
    let (value, _, stored) = r.store.reload().unwrap();
    assert_eq!(value, cron(5));
    assert!(!stored);
    assert!(r.store.save(cron(6)).unwrap());
}

#[test]
fn write_between_reads_is_not_mistaken_for_a_filter() {
    let r = rig();
    let (_, v) = r.store.load().unwrap();
    r.store.save_if(&v, cron(1)).unwrap();
    // Another process writes while this one reads: the callback returns the value it got, unchanged.
    let repo = r.repo.clone();
    r.hooks
        .add_filter(
            &HookName::from("option_cron"),
            Callback::rust("write_behind", move |args: &[HookValue]| {
                repo.update(
                    &OptionName::new("cron").unwrap(),
                    Some(&SerializedValue::new(
                        b"a:2:{i:8;a:0:{}s:7:\"version\";i:2;}".to_vec(),
                    )),
                    None,
                )
                .unwrap();
                Ok(args[0].clone())
            }),
            &HookValue::Int(10),
            1,
        )
        .unwrap();
    let (_, version, stored) = r.store.reload().unwrap();
    // Reported as stored, so the caller keeps to compare-and-swap, which then conflicts instead of overwriting.
    assert!(stored);
    assert_eq!(
        r.store.save_if(&version, cron(9)).unwrap(),
        SaveOutcome::Conflict
    );
}
