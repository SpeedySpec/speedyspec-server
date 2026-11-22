//! The sanitizer facade: each function's corpus through `TextApi`, and its filters' arguments and odd values.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{php_int, text};
use speedyspec_wp_hooks::{Callback, HookName, HookValue};
use speedyspec_wp_text::adapters::PhpMtRand;
use speedyspec_wp_text::title::{SanitizeContext, sanitize_title_with_dashes};
use speedyspec_wp_text_corpus::{Php, assert_corpus, from_value, to_value};

#[path = "common/api.rs"]
mod api;

fn add(h: &api::Harness, hook: &str, run: impl Fn(&[HookValue]) -> HookValue + 'static) {
    h.hooks
        .add_filter(&HookName::from(hook), Callback::rust(hook, move |args: &[HookValue]| Ok(run(args))), &HookValue::Int(10), 4)
        .unwrap();
}

fn bytes_of(value: &HookValue) -> Vec<u8> {
    match value {
        HookValue::Str(s) => s.as_bytes().to_vec(),
        HookValue::Bytes(b) => b.clone(),
        other => panic!("not a string: {other:?}"),
    }
}

/// setup.php's `add_filter( 'sanitize_title', 'sanitize_title_with_dashes', 10, 3 )`, as a Rust callback.
fn with_dashes_filter(h: &api::Harness) {
    add(h, "sanitize_title", |args| {
        let context = SanitizeContext::from_bytes(&bytes_of(&args[2]));
        speedyspec_wp_text::strings::bytes_value(sanitize_title_with_dashes(&bytes_of(&args[0]), &context))
    });
}

#[test]
fn sanitizer_corpora_through_text_api() {
    let h = api::harness();
    let api = &h.api;
    assert_corpus("sanitize_key", |args| from_value(&api.sanitize_key(&to_value(&args[0])).unwrap()));
    assert_corpus("sanitize_html_class", |args| {
        from_value(&api.sanitize_html_class(text(&args[0]), args.get(1).map_or(&b""[..], text)).unwrap())
    });
    assert_corpus("sanitize_locale_name", |args| from_value(&api.sanitize_locale_name(text(&args[0])).unwrap()));
    assert_corpus("sanitize_mime_type", |args| from_value(&api.sanitize_mime_type(text(&args[0])).unwrap()));
    assert_corpus("remove_accents", |args| {
        let locale = args.get(1).map(|l| String::from_utf8(text(l).to_vec()).unwrap());
        Php::Str(api.remove_accents(text(&args[0]), locale.as_deref()).unwrap().into_owned())
    });
    assert_corpus("sanitize_user", |args| {
        from_value(&api.sanitize_user(text(&args[0]), matches!(args.get(1), Some(Php::Bool(true)))).unwrap())
    });
    assert_corpus("sanitize_file_name", |args| from_value(&api.sanitize_file_name(text(&args[0])).unwrap()));
}

#[test]
fn sanitize_title_corpora_through_text_api() {
    let h = api::harness();
    with_dashes_filter(&h);
    assert_corpus("sanitize_title", |args| {
        let fallback = args.get(1).map_or(&b""[..], text);
        let context = SanitizeContext::from_bytes(args.get(2).map_or(&b"save"[..], text));
        from_value(&h.api.sanitize_title(&to_value(&args[0]), fallback, &context).unwrap())
    });
    assert_corpus("sanitize_title_for_query", |args| from_value(&h.api.sanitize_title_for_query(&to_value(&args[0])).unwrap()));
}

#[test]
fn antispambot_corpus_with_php_seeds() {
    assert_corpus("speedyspec_antispambot_seeded", |args| {
        let seed = php_int(&args[2]) as u32;
        let h = api::harness_options(api::Options { random: Rc::new(PhpMtRand::seeded(seed)), ..Default::default() });
        Php::Str(h.api.antispambot(text(&args[0]), php_int(&args[1])).unwrap())
    });
}

#[test]
fn sanitize_title_filter_false_gives_fallback() {
    let h = api::harness();
    add(&h, "sanitize_title", |_| HookValue::Bool(false));
    let out = h.api.sanitize_title(&HookValue::Str("A".into()), b"fb", &SanitizeContext::Save).unwrap();
    assert_eq!(bytes_of(&out), b"fb");
}

#[test]
fn sanitize_title_filter_gets_raw_and_context() {
    let h = api::harness();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = seen.clone();
    add(&h, "sanitize_title", move |args| {
        log.borrow_mut().push(args.to_vec());
        args[0].clone()
    });
    h.api.sanitize_title(&HookValue::Str("Ünï".into()), b"", &SanitizeContext::Save).unwrap();
    let args = &seen.borrow()[0];
    assert_eq!((bytes_of(&args[0]), bytes_of(&args[1]), bytes_of(&args[2])), (b"Uni".to_vec(), "Ünï".as_bytes().to_vec(), b"save".to_vec()));
}

#[test]
fn sanitize_file_name_chars_string_is_one_search() {
    let h = api::harness();
    add(&h, "sanitize_file_name_chars", |_| HookValue::Str("ab".into()));
    assert_eq!(bytes_of(&h.api.sanitize_file_name(b"xaby?.txt").unwrap()), b"xy?.txt");
}

#[test]
fn sanitize_key_non_scalar_is_empty() {
    let h = api::harness();
    let seen = Rc::new(RefCell::new(None));
    let log = seen.clone();
    add(&h, "sanitize_key", move |args| {
        *log.borrow_mut() = Some(args[1].clone());
        args[0].clone()
    });
    let array = HookValue::Array(vec![(speedyspec_wp_hooks::Key::Int(0), HookValue::Str("Key".into()))]);
    assert_eq!(bytes_of(&h.api.sanitize_key(&array).unwrap()), b"");
    assert_eq!(seen.borrow().clone(), Some(array));
    assert_eq!(bytes_of(&h.api.sanitize_key(&HookValue::Null).unwrap()), b"");
}

/// A recorded argument as PHP's `(string)` would make it.
fn string_of(value: &Php) -> Vec<u8> {
    speedyspec_wp_text::strings::to_php_string(&to_value(value)).unwrap_or_default()
}

#[test]
fn email_and_field_corpora_through_text_api() {
    let h = api::harness();
    let api = &h.api;
    speedyspec_wp_text_corpus::assert_corpus_reporting("is_email", |args| {
        h.diagnostics.taken();
        let deprecated = args.get(1).map_or(HookValue::Bool(false), to_value);
        let out = from_value(&api.is_email(&to_value(&args[0]), &deprecated).unwrap());
        (out, speedyspec_wp_text_corpus::diagnostics(&h.diagnostics.taken()))
    });
    assert_corpus("sanitize_email", |args| from_value(&api.sanitize_email(&string_of(&args[0])).unwrap()));
    assert_corpus("sanitize_text_field", |args| from_value(&api.sanitize_text_field(&to_value(&args[0])).unwrap()));
    assert_corpus("sanitize_textarea_field", |args| from_value(&api.sanitize_textarea_field(&to_value(&args[0])).unwrap()));
    assert_corpus("format_to_edit", |args| {
        from_value(&api.format_to_edit(text(&args[0]), matches!(args.get(1), Some(Php::Bool(true)))).unwrap())
    });
    assert_corpus("wp_pre_kses_less_than", |args| Php::Str(api.wp_pre_kses_less_than(text(&args[0])).unwrap()));
    assert_corpus("speedyspec_wp_parse_str", |args| from_value(&api.wp_parse_str(&to_value(&args[0])).unwrap()));
    assert_corpus("sanitize_trackback_urls", |args| from_value(&api.sanitize_trackback_urls(text(&args[0])).unwrap()));
}

#[test]
fn deep_corpora() {
    use speedyspec_wp_text::deep;
    assert_corpus("stripslashes_deep", |args| from_value(&deep::stripslashes_deep(&to_value(&args[0]))));
    assert_corpus("stripslashes_from_strings_only", |args| {
        from_value(&deep::stripslashes_from_strings_only(&to_value(&args[0])))
    });
    assert_corpus("urlencode_deep", |args| from_value(&deep::urlencode_deep(&to_value(&args[0]))));
    assert_corpus("rawurlencode_deep", |args| from_value(&deep::rawurlencode_deep(&to_value(&args[0]))));
    assert_corpus("urldecode_deep", |args| from_value(&deep::urldecode_deep(&to_value(&args[0]))));
}

#[test]
fn is_email_context_reaches_filter() {
    let h = api::harness();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = seen.clone();
    add(&h, "is_email", move |args| {
        log.borrow_mut().push(args.to_vec());
        args[0].clone()
    });
    assert_eq!(h.api.is_email(&HookValue::Str("ab@cdef".into()), &HookValue::Bool(false)).unwrap(), HookValue::Bool(false));
    assert_eq!(seen.borrow()[0][2], HookValue::Str("domain_no_periods".into()));
    h.api.is_email(&HookValue::Str("ab@c.de".into()), &HookValue::Bool(false)).unwrap();
    assert_eq!(seen.borrow()[1][2], HookValue::Null);
}

#[test]
fn map_deep_leaves_objects_alone() {
    use speedyspec_wp_text::deep::map_deep;
    let object = HookValue::Opaque(Rc::new(5u8));
    let mapped = map_deep(&object, &mut |_| HookValue::Int(0));
    assert!(matches!(mapped, HookValue::Opaque(_)));
}
