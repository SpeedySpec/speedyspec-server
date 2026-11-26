use std::rc::Rc;

use speedyspec_firewall::{RawSettings, SettingsSource};
use speedyspec_firewall_infra_options::OptionsSettingsSource;
use speedyspec_wp_hooks::{IgnoreDeprecations, WpHookApi};
use speedyspec_wp_hooks_contracts::HookValue;
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_object_cache::{CurrentObjectCache, ObjectCachePorts};
use speedyspec_wp_options::{Options, OptionsApi, OptionsDeps};
use speedyspec_wp_options_memory::MemoryOptionRepository;

fn api() -> Rc<OptionsApi> {
    let hooks = Rc::new(WpHookApi::new(
        Rc::new(MemoryHookStore::new()),
        Rc::new(IgnoreDeprecations),
    ));
    let cache = Rc::new(CurrentObjectCache::new(ObjectCachePorts::rust(), None));
    Rc::new(OptionsApi::new(OptionsDeps::rust(
        hooks,
        cache,
        Rc::new(MemoryOptionRepository::new()),
    )))
}

#[test]
fn missing_options_give_defaults() {
    let source = OptionsSettingsSource::new(Options::new(api()));
    assert_eq!(source.load().unwrap(), RawSettings::default());
}

#[test]
fn stored_options_are_read() {
    let api = api();
    let auto = HookValue::Null;
    api.update_option(
        "speedyspec_firewall_enabled",
        HookValue::Bool(true),
        auto.clone(),
    )
    .unwrap();
    api.update_option(
        "speedyspec_firewall_allow",
        HookValue::Str("10.0.0.0/8".into()),
        auto.clone(),
    )
    .unwrap();
    api.update_option(
        "speedyspec_firewall_rate_requests",
        HookValue::Int(30),
        auto.clone(),
    )
    .unwrap();
    api.update_option(
        "speedyspec_firewall_signin_paths",
        HookValue::Str("/auth/login".into()),
        auto.clone(),
    )
    .unwrap();
    api.update_option(
        "speedyspec_firewall_block_xmlrpc",
        HookValue::Bool(true),
        auto,
    )
    .unwrap();
    let raw = OptionsSettingsSource::new(Options::new(api))
        .load()
        .unwrap();
    assert_eq!(
        raw,
        RawSettings {
            enabled: true,
            allow: "10.0.0.0/8".into(),
            rate_requests: 30,
            signin_paths: "/auth/login".into(),
            block_xmlrpc: true,
            ..RawSettings::default()
        }
    );
}
