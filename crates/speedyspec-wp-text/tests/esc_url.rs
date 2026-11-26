//! `esc_url`, `esc_url_raw` and `sanitize_url`: the recorded corpus through `TextApi`, the character filter
//! against its pattern, and the `clean_url` filter.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::text;
use proptest::prelude::*;
use speedyspec_wp_hooks_contracts::{Callback, HookName, HookValue};
use speedyspec_wp_kses_contracts::Protocols;
use speedyspec_wp_text::esc_url::esc_url_characters;
use speedyspec_wp_text::preg::{Pattern, preg_replace};
use speedyspec_wp_text_corpus::{Php, assert_corpus, from_value};

#[path = "common/api.rs"]
mod api;

/// A recorded `$protocols` argument: only an array counts, as `is_array()` decides.
fn protocols(arg: Option<&Php>) -> Option<Protocols> {
    match arg {
        Some(Php::Array(items)) => {
            Some(Protocols(items.iter().filter_map(|(_, v)| v.bytes().map(|b| b.to_vec().into_boxed_slice())).collect()))
        }
        _ => None,
    }
}

fn add(h: &api::Harness, hook: &str, run: impl Fn(&[HookValue]) -> HookValue + 'static) {
    h.hooks
        .add_filter(&HookName::from(hook), Callback::rust(hook, move |args: &[HookValue]| Ok(run(args))), &HookValue::Int(10), 4)
        .unwrap();
}

#[test]
fn esc_url_corpus() {
    let h = api::harness();
    assert_corpus("esc_url", |args| {
        let context = args.get(2).map_or(&b"display"[..], text);
        from_value(&h.api.esc_url(text(&args[0]), protocols(args.get(1)).as_ref(), context).unwrap())
    });
    assert_corpus("sanitize_url", |args| from_value(&h.api.sanitize_url(text(&args[0]), protocols(args.get(1)).as_ref()).unwrap()));
}

#[test]
fn esc_url_raw_is_the_db_context() {
    let h = api::harness();
    assert_eq!(h.api.esc_url_raw(b"http://a/?b=1&c='d'", None).unwrap(), HookValue::Str("http://a/?b=1&c='d'".into()));
    assert_eq!(h.api.esc_url(b"http://a/?b=1&c='d'", None, b"display").unwrap(), HookValue::Str("http://a/?b=1&#038;c=&#039;d&#039;".into()));
}

#[test]
fn clean_url_gets_original_and_context() {
    let h = api::harness();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = seen.clone();
    add(&h, "clean_url", move |args| {
        log.borrow_mut().push(args.to_vec());
        args[0].clone()
    });
    h.api.esc_url(b" http://a.b/c d", None, b"display").unwrap();
    let args = &seen.borrow()[0];
    assert_eq!(args[0], HookValue::Str("http://a.b/c%20d".into()));
    assert_eq!(args[1], HookValue::Str(" http://a.b/c d".into()));
    assert_eq!(args[2], HookValue::Str("display".into()));
}

#[test]
fn clean_url_non_string_passes_through() {
    let h = api::harness();
    add(&h, "clean_url", |_| HookValue::Int(3));
    assert_eq!(h.api.esc_url(b"http://a.b", None, b"display").unwrap(), HookValue::Int(3));
}

#[test]
fn empty_url_returns_before_any_filter() {
    let h = api::harness();
    add(&h, "clean_url", |_| HookValue::Int(3));
    assert_eq!(h.api.esc_url(b"", None, b"display").unwrap(), HookValue::Str(String::new()));
}

#[test]
fn allowed_protocols_filter_before_wp_loaded() {
    let h = api::harness();
    assert_eq!(h.api.esc_url(b"ftp://a", None, b"display").unwrap(), HookValue::Str("ftp://a".into()));
    add(&h, "kses_allowed_protocols", |_| {
        HookValue::Array(vec![(speedyspec_wp_hooks_contracts::Key::Int(0), HookValue::Str("http".into()))])
    });
    assert_eq!(h.api.esc_url(b"ftp://a", None, b"display").unwrap(), HookValue::Str(String::new()));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4096))]

    #[test]
    fn esc_url_class_matches_pattern(input in proptest::collection::vec(any::<u8>(), 0..24)) {
        let pattern = Pattern::php("|[^a-z0-9-~+_.?#=!&;,/:%@$\\|*\\'()\\[\\]\\x80-\\xff]|i");
        let expected = preg_replace(&pattern, &input, b"", None).unwrap().0;
        prop_assert_eq!(&*esc_url_characters(&input), &*expected);
    }
}
