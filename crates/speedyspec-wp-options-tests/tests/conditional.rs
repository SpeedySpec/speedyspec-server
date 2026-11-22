//! `update_option_if_unchanged`: `update_option` whose write happens only if the stored bytes are still the ones the
//! caller read (compare-and-swap, for writers such as cron).

mod support;

use speedyspec_wp_hooks::HookValue;
use speedyspec_wp_options::{
    Autoload, ConditionalWrite, OptionName, OptionRepository, SerializedValue,
};
use support::*;

fn null() -> HookValue {
    HookValue::Null
}

/// Another process writes `value` behind every cache.
fn write_behind(f: &Fixture, name: &str, value: &str) {
    f.repo
        .update(
            &OptionName::new(name).unwrap(),
            Some(&SerializedValue::new(value.as_bytes().to_vec())),
            None,
        )
        .unwrap();
}

#[test]
fn matching_bytes_write_and_fire_hooks() {
    let f = fixture();
    seed(&f, "x", "old", Autoload::On);
    record(&f, "updated_option");
    let got = f
        .api
        .update_option_if_unchanged("x", Some(b"old"), s("new"), null())
        .unwrap();
    assert_eq!(got, ConditionalWrite::Written);
    assert_eq!(hooks_run(&f), ["updated_option"]);
    assert_eq!(stored(&f, "x"), Some(b"new".to_vec()));
    assert_eq!(f.api.get_option("x", None).unwrap(), s("new"));
}

#[test]
fn different_bytes_conflict_without_hooks_or_cache() {
    let f = fixture();
    seed(&f, "x", "old", Autoload::On);
    assert_eq!(f.api.get_option("x", None).unwrap(), s("old"));
    write_behind(&f, "x", "theirs");
    for h in ["update_option_x", "updated_option"] {
        record(&f, h);
    }
    let got = f
        .api
        .update_option_if_unchanged("x", Some(b"old"), s("mine"), null())
        .unwrap();
    assert_eq!(got, ConditionalWrite::Conflict);
    assert!(hooks_run(&f).is_empty(), "{:?}", hooks_run(&f));
    assert_eq!(stored(&f, "x"), Some(b"theirs".to_vec()));
    // The cache still holds what this process read.
    assert_eq!(f.api.get_option("x", None).unwrap(), s("old"));
}

#[test]
fn equal_value_is_unchanged() {
    let f = fixture();
    seed(&f, "x", "same", Autoload::On);
    let got = f
        .api
        .update_option_if_unchanged("x", Some(b"same"), s("same"), null())
        .unwrap();
    assert_eq!(got, ConditionalWrite::Unchanged);
}

#[test]
fn absent_expected_inserts_once() {
    let f = fixture();
    let first = f
        .api
        .update_option_if_unchanged("x", None, s("a"), null())
        .unwrap();
    assert_eq!(first, ConditionalWrite::Written);
    assert_eq!(stored(&f, "x"), Some(b"a".to_vec()));
    // A second writer that also read "absent", whose cache does not know about the first write.
    let g = fixture();
    seed(&g, "x", "a", Autoload::On);
    let mut notoptions = HookValue::Array(vec![]);
    if let HookValue::Array(items) = &mut notoptions {
        items.push((
            speedyspec_wp_hooks::Key::Str("x".into()),
            HookValue::Bool(true),
        ));
    }
    g.cache
        .set(&s("notoptions"), notoptions, &options(), 0)
        .unwrap();
    let second = g
        .api
        .update_option_if_unchanged("x", None, s("b"), null())
        .unwrap();
    assert_eq!(second, ConditionalWrite::Conflict);
    assert_eq!(stored(&g, "x"), Some(b"a".to_vec()));
}

#[test]
fn forget_cached_rereads() {
    let f = fixture();
    seed(&f, "x", "old", Autoload::On);
    assert_eq!(f.api.get_option("x", None).unwrap(), s("old"));
    write_behind(&f, "x", "new");
    assert_eq!(f.api.get_option("x", None).unwrap(), s("old"));
    f.api.forget_cached("x").unwrap();
    assert_eq!(f.api.get_option("x", None).unwrap(), s("new"));
}

#[test]
fn stored_bytes_bypass_the_cache() {
    let f = fixture();
    assert_eq!(f.api.stored_bytes("x").unwrap(), None);
    seed(&f, "x", "old", Autoload::On);
    f.api.get_option("x", None).unwrap();
    write_behind(&f, "x", "new");
    assert_eq!(f.api.stored_bytes("x").unwrap(), Some(b"new".to_vec()));
}

#[test]
fn serialized_form_matches_update() {
    let f = fixture();
    seed(&f, "x", "old", Autoload::On);
    let value = assoc(&[("a", HookValue::Int(1)), ("b", s("two"))]);
    assert!(f.api.update_option("x", value.clone(), null()).unwrap());
    assert_eq!(
        f.api.serialized_form(&value).unwrap(),
        stored(&f, "x").unwrap()
    );
    assert_eq!(
        f.api.serialized_form(&s("plain")).unwrap(),
        b"plain".to_vec()
    );
}
