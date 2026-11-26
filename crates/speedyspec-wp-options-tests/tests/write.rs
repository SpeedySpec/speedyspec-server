mod support;

use speedyspec_wp_hooks_contracts::{HookValue, Key};
use speedyspec_wp_options::{
    Autoload, OptionName, OptionRepository, OptionsError, register_default_filters,
};
use support::*;

fn null() -> HookValue {
    HookValue::Null
}

fn autoload_of(f: &Fixture, name: &str) -> Option<Autoload> {
    f.repo.autoload_of(&OptionName::new(name).unwrap()).unwrap()
}

#[test]
fn add_hook_order_and_caches() {
    let f = fixture();
    seed(&f, "a", "1", Autoload::On);
    f.api.get_option("x", None).unwrap();
    assert!(cached(&f, "notoptions").is_some_and(|n| n != HookValue::Array(vec![])));
    for h in ["add_option", "add_option_x", "added_option"] {
        record(&f, h);
    }
    assert!(f.api.add_option("x", s("v"), s(""), null()).unwrap());
    assert_eq!(
        hooks_run(&f),
        ["add_option", "add_option_x", "added_option"]
    );
    assert_eq!(f.log.borrow()[0].1, vec![s("x"), s("v")]);
    assert_eq!(in_alloptions(&f, "x"), Some(s("v")));
    assert_eq!(cached(&f, "notoptions"), Some(HookValue::Array(vec![])));
    assert_eq!(autoload_of(&f, "x"), Some(Autoload::Auto));
    assert_eq!(stored(&f, "x"), Some(b"v".to_vec()));
}

#[test]
fn add_existing_is_false() {
    let f = fixture();
    seed(&f, "x", "old", Autoload::On);
    record(&f, "add_option");
    assert!(!f.api.add_option("x", s("new"), s(""), null()).unwrap());
    assert!(hooks_run(&f).is_empty());
    assert_eq!(stored(&f, "x"), Some(b"old".to_vec()));
}

#[test]
fn add_deprecated_argument_warns() {
    let f = fixture();
    assert!(f.api.add_option("x", s("v"), s("old"), null()).unwrap());
    assert_eq!(
        *f.deprecations.0.borrow(),
        vec![("add_option".into(), "2.3.0".into(), String::new())]
    );
    // `empty()` deprecated arguments are silent.
    assert!(f.api.add_option("y", s("v"), s("0"), null()).unwrap());
    assert_eq!(f.deprecations.0.borrow().len(), 1);
}

#[test]
fn update_hook_order() {
    let f = fixture();
    seed(&f, "x", "old", Autoload::On);
    for h in [
        "pre_update_option_x",
        "pre_update_option",
        "update_option",
        "update_option_x",
        "updated_option",
    ] {
        record(&f, h);
    }
    assert!(f.api.update_option("x", s("new"), null()).unwrap());
    assert_eq!(
        hooks_run(&f),
        [
            "pre_update_option_x",
            "pre_update_option",
            "update_option",
            "update_option_x",
            "updated_option"
        ]
    );
    let log = f.log.borrow();
    assert_eq!(log[0].1, vec![s("new"), s("old"), s("x")]);
    assert_eq!(log[1].1, vec![s("new"), s("x"), s("old")]);
    assert_eq!(log[2].1, vec![s("x"), s("old"), s("new")]);
    assert_eq!(log[3].1, vec![s("old"), s("new"), s("x")]);
    assert_eq!(log[4].1, vec![s("x"), s("old"), s("new")]);
    assert_eq!(in_alloptions(&f, "x"), Some(s("new")));
    assert_eq!(autoload_of(&f, "x"), Some(Autoload::On));
}

#[test]
fn update_unchanged_is_false() {
    let f = fixture();
    seed(&f, "x", "v", Autoload::On);
    record(&f, "update_option");
    assert!(!f.api.update_option("x", s("v"), null()).unwrap());
    assert!(hooks_run(&f).is_empty());
}

#[test]
fn update_missing_adds() {
    let updated = fixture();
    let added = fixture();
    for f in [&updated, &added] {
        seed(f, "a", "1", Autoload::On);
        for h in ["add_option", "add_option_m", "added_option"] {
            record(f, h);
        }
    }
    let before = (updated.repo.statements(), added.repo.statements());
    assert!(updated.api.update_option("m", s("v"), null()).unwrap());
    assert!(added.api.add_option("m", s("v"), s(""), null()).unwrap());
    assert_eq!(
        updated.repo.statements() - before.0,
        added.repo.statements() - before.1
    );
    assert_eq!(hooks_run(&updated), hooks_run(&added));
    assert_eq!(stored(&updated, "m"), Some(b"v".to_vec()));
}

#[test]
fn update_reevaluates_auto_only() {
    let f = fixture();
    register_default_filters(&f.hooks).unwrap();
    seed(&f, "b", "small", Autoload::Auto);
    seed(&f, "k", "small", Autoload::On);
    let big = "x".repeat(150001);
    assert!(f.api.update_option("b", s(&big), null()).unwrap());
    assert_eq!(autoload_of(&f, "b"), Some(Autoload::AutoOff));
    assert_eq!(in_alloptions(&f, "b"), None);
    assert_eq!(cached(&f, "b"), Some(s(&big)));
    assert!(f.api.update_option("k", s(&big), null()).unwrap());
    assert_eq!(autoload_of(&f, "k"), Some(Autoload::On));
    assert_eq!(in_alloptions(&f, "k"), Some(s(&big)));
}

#[test]
fn update_caches_raw_stored_value() {
    let f = fixture();
    seed(&f, "n", "1", Autoload::On);
    assert!(f.api.update_option("n", HookValue::Int(5), null()).unwrap());
    assert_eq!(in_alloptions(&f, "n"), Some(HookValue::Int(5)));
    assert_eq!(stored(&f, "n"), Some(b"5".to_vec()));
}

#[test]
fn update_double_serializes() {
    let f = fixture();
    seed(&f, "s", "x", Autoload::On);
    assert!(f.api.update_option("s", s("a:0:{}"), null()).unwrap());
    assert_eq!(stored(&f, "s"), Some(b"s:6:\"a:0:{}\";".to_vec()));
    assert_eq!(f.api.get_option("s", None).unwrap(), s("a:0:{}"));
}

#[test]
fn update_with_explicit_autoload_moves_the_value() {
    let f = fixture();
    seed(&f, "a", "1", Autoload::On);
    seed(&f, "x", "v", Autoload::On);
    assert!(
        f.api
            .update_option("x", s("w"), HookValue::Bool(false))
            .unwrap()
    );
    assert_eq!(autoload_of(&f, "x"), Some(Autoload::Off));
    assert_eq!(in_alloptions(&f, "x"), None);
    assert_eq!(cached(&f, "x"), Some(s("w")));
}

#[test]
fn delete_hook_order_and_notoptions() {
    let f = fixture();
    seed(&f, "a", "1", Autoload::On);
    seed(&f, "d", "v", Autoload::On);
    f.api.get_option("d", None).unwrap();
    for h in ["delete_option", "delete_option_d", "deleted_option"] {
        record(&f, h);
    }
    assert!(f.api.delete_option("d").unwrap());
    assert_eq!(
        hooks_run(&f),
        ["delete_option", "delete_option_d", "deleted_option"]
    );
    assert_eq!(in_alloptions(&f, "d"), None);
    assert_eq!(
        cached(&f, "notoptions"),
        Some(HookValue::Array(vec![(
            Key::Str("d".into()),
            HookValue::Bool(true)
        )]))
    );
    assert_eq!(stored(&f, "d"), None);
}

#[test]
fn delete_missing_is_false_without_hooks() {
    let f = fixture();
    record(&f, "delete_option");
    assert!(!f.api.delete_option("nope").unwrap());
    assert!(hooks_run(&f).is_empty());
}

#[test]
fn set_autoload_values_groups() {
    let f = fixture();
    for (n, a) in [
        ("a", Autoload::On),
        ("b", Autoload::Off),
        ("c", Autoload::On),
        ("d", Autoload::Off),
    ] {
        seed(&f, n, "v", a);
    }
    f.api.load_alloptions(false).unwrap();
    assert!(cached(&f, "alloptions").is_some());
    let before = f.repo.statements();
    let results = f
        .api
        .set_option_autoload_values(vec![
            ("a".into(), HookValue::Bool(true)),
            ("b".into(), s("no")),
            ("c".into(), HookValue::Bool(false)),
            ("d".into(), s("yes")),
        ])
        .unwrap();
    assert_eq!(
        results,
        vec![
            ("a".into(), false),
            ("b".into(), false),
            ("c".into(), true),
            ("d".into(), true)
        ]
    );
    assert_eq!(f.repo.statements() - before, 3);
    assert_eq!(autoload_of(&f, "c"), Some(Autoload::Off));
    assert_eq!(autoload_of(&f, "d"), Some(Autoload::On));
    assert_eq!(cached(&f, "alloptions"), None);
}

#[test]
fn set_autoload_values_off_only_edits_alloptions() {
    let f = fixture();
    seed(&f, "a", "1", Autoload::On);
    seed(&f, "c", "3", Autoload::On);
    f.api.load_alloptions(false).unwrap();
    let results = f
        .api
        .set_options_autoload(&["c".to_string()], s("off"))
        .unwrap();
    assert_eq!(results, vec![("c".into(), true)]);
    assert_eq!(in_alloptions(&f, "c"), None);
    assert_eq!(in_alloptions(&f, "a"), Some(s("1")));
}

#[test]
fn set_autoload_values_empty_is_empty() {
    let f = fixture();
    let before = f.repo.statements();
    assert!(
        f.api
            .set_option_autoload_values(Vec::new())
            .unwrap()
            .is_empty()
    );
    assert_eq!(f.repo.statements(), before);
}

#[test]
fn set_option_autoload_missing_is_false() {
    let f = fixture();
    assert!(
        !f.api
            .set_option_autoload("nope", HookValue::Bool(true))
            .unwrap()
    );
}

#[test]
fn protected_name_errors_before_any_write() {
    let f = fixture();
    let before = f.repo.statements();
    assert!(matches!(
        f.api.add_option("alloptions", s("v"), s(""), null()),
        Err(OptionsError::Protected(_))
    ));
    assert!(matches!(
        f.api.update_option("notoptions", s("v"), null()),
        Err(OptionsError::Protected(_))
    ));
    assert!(matches!(
        f.api.delete_option("alloptions"),
        Err(OptionsError::Protected(_))
    ));
    assert!(matches!(
        f.api
            .set_option_autoload_values(vec![("notoptions".into(), HookValue::Bool(true))]),
        Err(OptionsError::Protected(_))
    ));
    assert_eq!(f.repo.statements(), before);
}
