mod support;

use speedyspec_wp_hooks::{HookValue, Key};
use speedyspec_wp_options::{Autoload, OptionsError};
use support::*;

const READ_HOOKS: &[&str] = &[
    "pre_option_foo",
    "pre_option",
    "pre_wp_load_alloptions",
    "wp_autoload_values_to_autoload",
    "pre_cache_alloptions",
    "alloptions",
    "option_foo",
    "default_option_foo",
];

#[test]
fn get_option_hook_order() {
    let f = fixture();
    seed(&f, "foo", "bar", Autoload::Off);
    seed(&f, "a", "1", Autoload::On);
    for h in READ_HOOKS {
        record(&f, h);
    }
    let before = f.repo.statements();
    assert_eq!(f.api.get_option("foo", None).unwrap(), s("bar"));
    assert_eq!(
        hooks_run(&f),
        [
            "pre_option_foo",
            "pre_option",
            "pre_wp_load_alloptions",
            "wp_autoload_values_to_autoload",
            "pre_cache_alloptions",
            "alloptions",
            "option_foo"
        ]
    );
    let log = f.log.borrow();
    assert_eq!(
        log[0].1,
        vec![HookValue::Bool(false), s("foo"), HookValue::Bool(false)]
    );
    assert_eq!(
        log[1].1,
        vec![HookValue::Bool(false), s("foo"), HookValue::Bool(false)]
    );
    assert_eq!(log[2].1, vec![HookValue::Null, HookValue::Bool(false)]);
    assert_eq!(log[4].1, vec![assoc(&[("a", s("1"))])]);
    assert_eq!(log[6].1, vec![s("bar"), s("foo")]);
    // `autoloaded`, then `value`.
    assert_eq!(f.repo.statements() - before, 2);
}

#[test]
fn pre_option_short_circuits() {
    let f = fixture();
    answer(&f, "pre_option", s("x"));
    let before = f.repo.statements();
    assert_eq!(f.api.get_option("foo", None).unwrap(), s("x"));
    assert_eq!(f.repo.statements(), before);
}

#[test]
fn missing_goes_to_notoptions_once() {
    let f = fixture();
    // An empty `alloptions` is falsy, so WordPress queries it again on every read: give it a row.
    seed(&f, "a", "1", Autoload::On);
    record(&f, "default_option_nope");
    assert_eq!(f.api.get_option("nope", Some(s("d"))).unwrap(), s("d"));
    let after_first = f.repo.statements();
    assert_eq!(f.api.get_option("nope", Some(s("d"))).unwrap(), s("d"));
    // The second read sends nothing: alloptions and notoptions are cached.
    assert_eq!(f.repo.statements(), after_first);
    assert_eq!(
        f.cache.get(&s("notoptions"), &options(), false).unwrap(),
        Some(assoc(&[("nope", HookValue::Bool(true))]))
    );
    let log = f.log.borrow();
    assert_eq!(log.len(), 2);
    assert_eq!(log[0].1, vec![s("d"), s("nope"), HookValue::Bool(true)]);
}

#[test]
fn blank_and_zero_names() {
    let f = fixture();
    record(&f, "pre_option");
    for name in ["", "0", " 0 ", "  "] {
        assert_eq!(
            f.api.get_option(name, None).unwrap(),
            HookValue::Bool(false),
            "{name:?}"
        );
    }
    assert!(hooks_run(&f).is_empty());
}

#[test]
fn trimmed_name() {
    let f = fixture();
    record(&f, "pre_option_foo");
    f.api.get_option(" foo ", None).unwrap();
    assert_eq!(hooks_run(&f), ["pre_option_foo"]);
}

#[test]
fn deprecated_name_maps() {
    let f = fixture();
    seed(&f, "disallowed_keys", "spam", Autoload::On);
    record(&f, "pre_option_disallowed_keys");
    record(&f, "pre_option_blacklist_keys");
    assert_eq!(f.api.get_option("blacklist_keys", None).unwrap(), s("spam"));
    assert_eq!(hooks_run(&f), ["pre_option_disallowed_keys"]);
    // The recursion passes the default on, so `passed_default` is true there.
    f.log.borrow_mut().clear();
    f.installing.set(true);
    record(&f, "default_option_blacklist_keys");
    f.api.get_option("blacklist_keys", None).unwrap();
    assert_eq!(
        hooks_run(&f),
        ["pre_option_blacklist_keys", "default_option_blacklist_keys"]
    );
}

#[test]
fn installing_skips_cache() {
    let f = fixture();
    seed(&f, "foo", "bar", Autoload::On);
    f.installing.set(true);
    let before = f.repo.statements();
    assert_eq!(f.api.get_option("foo", None).unwrap(), s("bar"));
    assert_eq!(f.api.get_option("foo", None).unwrap(), s("bar"));
    assert_eq!(f.repo.statements() - before, 2);
    assert_eq!(
        f.cache.get(&s("alloptions"), &options(), false).unwrap(),
        None
    );
    assert_eq!(f.cache.get(&s("foo"), &options(), false).unwrap(), None);
}

#[test]
fn home_falls_back_to_siteurl() {
    let f = fixture();
    seed(&f, "home", "", Autoload::On);
    seed(&f, "siteurl", "http://x/", Autoload::On);
    assert_eq!(f.api.get_option("home", None).unwrap(), s("http://x"));
    seed(&f, "tag_base", "/tag\\/", Autoload::On);
    f.cache.flush();
    assert_eq!(f.api.get_option("tag_base", None).unwrap(), s("/tag"));
}

#[test]
fn alloptions_filter_non_array() {
    let f = fixture();
    seed(&f, "foo", "bar", Autoload::On);
    answer(&f, "alloptions", HookValue::Bool(false));
    assert_eq!(f.api.get_option("foo", None).unwrap(), s("bar"));
    // A second read finds it in the single-option cache.
    let before = f.repo.statements();
    assert_eq!(f.api.get_option("foo", None).unwrap(), s("bar"));
    assert_eq!(f.repo.statements(), before);
    answer(&f, "alloptions", s("not an array"));
    assert_eq!(f.api.get_option("foo", None).unwrap(), s("bar"));
}

#[test]
fn prime_skips_known() {
    let f = fixture();
    seed(&f, "a", "1", Autoload::On);
    seed(&f, "d", "4", Autoload::Off);
    f.api.get_option("b", None).unwrap();
    f.cache.set(&s("c"), s("3"), &options(), 0).unwrap();
    let before = f.repo.statements();
    let names: Vec<String> = ["a", "b", "c", "d", "e"]
        .iter()
        .map(|n| n.to_string())
        .collect();
    f.api.prime_option_caches(&names).unwrap();
    assert_eq!(f.repo.statements() - before, 1);
    assert_eq!(
        f.cache.get(&s("d"), &options(), false).unwrap(),
        Some(s("4"))
    );
    assert_eq!(
        f.cache.get(&s("notoptions"), &options(), false).unwrap(),
        Some(assoc(&[
            ("b", HookValue::Bool(true)),
            ("e", HookValue::Bool(true))
        ]))
    );
}

#[test]
fn get_options_returns_in_order() {
    let f = fixture();
    seed(&f, "x", "1", Autoload::Off);
    seed(&f, "y", "2", Autoload::Off);
    let names: Vec<String> = ["y", "missing", "x"]
        .iter()
        .map(|n| n.to_string())
        .collect();
    let got = f.api.get_options(&names).unwrap();
    assert_eq!(
        got,
        vec![
            ("y".to_string(), s("2")),
            ("missing".to_string(), HookValue::Bool(false)),
            ("x".to_string(), s("1"))
        ]
    );
}

#[test]
fn protect_special_option_errors() {
    let f = fixture();
    assert!(matches!(
        f.api.protect_special_option("alloptions"),
        Err(OptionsError::Protected(_))
    ));
    assert!(f.api.protect_special_option("siteurl").is_ok());
}

#[test]
fn numeric_names_are_integer_keys() {
    let f = fixture();
    seed(&f, "123", "n", Autoload::On);
    let all = f.api.load_alloptions(false).unwrap();
    assert_eq!(all, HookValue::Array(vec![(Key::Int(123), s("n"))]));
    assert_eq!(f.api.get_option("123", None).unwrap(), s("n"));
}

#[test]
fn follows_the_current_cache() {
    // `wp_cache_init()` and each request end select a new cache: the options API must use the one current now.
    let f = fixture();
    seed(&f, "a", "1", Autoload::On);
    f.api.get_option("a", None).unwrap();
    let fresh = f.current.create();
    f.current.select(fresh.clone());
    f.api.get_option("a", None).unwrap();
    assert!(fresh.exists(&s("alloptions"), &options()));
}

#[test]
fn cached_read_does_not_copy_alloptions() {
    // A read of one cached option costs the same whatever else alloptions holds (WordPress reads it in place).
    let time = |big: usize| {
        let f = fixture();
        for i in 0..50 {
            seed(&f, &format!("o{i}"), "v", Autoload::On);
        }
        seed(&f, "big", &"x".repeat(big), Autoload::On);
        f.api.get_option("o1", None).unwrap();
        let started = std::time::Instant::now();
        for _ in 0..200 {
            f.api.get_option("o1", None).unwrap();
        }
        started.elapsed()
    };
    let small = time(10);
    let large = time(4 * 1024 * 1024);
    assert!(
        large < small * 3 + std::time::Duration::from_millis(2),
        "{small:?} vs {large:?}"
    );
}
