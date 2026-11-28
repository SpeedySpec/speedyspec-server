mod support;

use speedyspec_wp_hooks_contracts::{HookName, HookValue};
use speedyspec_wp_options::{Autoload, OptionName, TypedOptions};
use support::*;

fn fired(f: &Fixture, name: &str) -> Vec<String> {
    f.api
        .hooks_fired_by_get(&OptionName::new(name).unwrap())
        .iter()
        .map(|h| match h.value() {
            HookValue::Str(s) => s.clone(),
            other => format!("{other:?}"),
        })
        .collect()
}

#[test]
fn cold_read_lists_query() {
    let f = fixture();
    let hooks = fired(&f, "foo");
    for h in [
        "pre_option_foo",
        "pre_option",
        "pre_wp_load_alloptions",
        "alloptions",
        "pre_cache_alloptions",
        "wp_autoload_values_to_autoload",
        "query",
        "default_option_foo",
        "option_foo",
    ] {
        assert!(hooks.contains(&h.to_string()), "{h} in {hooks:?}");
    }
    let mut unique = hooks.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), hooks.len());
}

#[test]
fn warm_autoloaded_read_has_no_query() {
    let f = fixture();
    seed(&f, "foo", "bar", Autoload::On);
    f.api.get_option("foo", None).unwrap();
    let hooks = fired(&f, "foo");
    assert!(!hooks.contains(&"query".to_string()), "{hooks:?}");
    assert!(
        !hooks.contains(&"pre_cache_alloptions".to_string()),
        "{hooks:?}"
    );
}

#[test]
fn known_missing_has_no_query() {
    let f = fixture();
    seed(&f, "a", "1", Autoload::On);
    f.api.get_option("nope", None).unwrap();
    assert!(!fired(&f, "nope").contains(&"query".to_string()));
    // A single cached, non-autoloaded option has none either.
    seed(&f, "off", "v", Autoload::Off);
    f.api.get_option("off", None).unwrap();
    assert!(!fired(&f, "off").contains(&"query".to_string()));
}

#[test]
fn home_includes_siteurl() {
    let f = fixture();
    let hooks = fired(&f, "home");
    for h in [
        "pre_option_home",
        "option_home",
        "pre_option_siteurl",
        "default_option_siteurl",
        "option_siteurl",
    ] {
        assert!(hooks.contains(&h.to_string()), "{h} in {hooks:?}");
    }
}

#[test]
fn deprecated_name_lists_the_new_names_hooks() {
    let f = fixture();
    let hooks = fired(&f, "blacklist_keys");
    assert!(hooks.contains(&"pre_option_disallowed_keys".to_string()));
    assert!(!hooks.contains(&"pre_option_blacklist_keys".to_string()));
}

#[test]
fn installing_lists_query_without_alloptions() {
    let f = fixture();
    f.installing.set(true);
    let hooks = fired(&f, "foo");
    assert!(hooks.contains(&"query".to_string()));
    assert!(!hooks.contains(&"pre_wp_load_alloptions".to_string()));
}

#[test]
fn listing_fires_nothing() {
    let f = fixture();
    seed(&f, "foo", "bar", Autoload::On);
    f.api.get_option("foo", None).unwrap();
    let did = f.hooks.did_filter(&HookName::from("pre_option")).unwrap();
    let (hits, misses) = (f.cache.hits(), f.cache.misses());
    fired(&f, "foo");
    fired(&f, "missing");
    assert_eq!(
        f.hooks.did_filter(&HookName::from("pre_option")).unwrap(),
        did
    );
    assert_eq!((f.cache.hits(), f.cache.misses()), (hits, misses));
}

#[test]
fn facade_get() {
    let f = fixture();
    seed(&f, "n", "5", Autoload::On);
    seed(&f, "t", "", Autoload::On);
    let options = TypedOptions::new(std::rc::Rc::new(f.api));
    assert_eq!(options.get::<i64>("n").unwrap(), Some(5));
    assert_eq!(options.get::<String>("n").unwrap(), Some("5".to_string()));
    assert_eq!(options.get::<bool>("t").unwrap(), Some(false));
    assert_eq!(options.get::<i64>("missing").unwrap(), None);
    assert_eq!(options.get::<i64>("t").unwrap(), None);
}
