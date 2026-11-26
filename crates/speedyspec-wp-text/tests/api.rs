//! `TextApi`'s own behaviour: PHP's statics, the filters' arguments and odd filter values.

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use speedyspec_wp_hooks_contracts::{Callback, HookName, HookValue, Key};
use speedyspec_wp_shortcode_contracts::{ShortcodeTagNames, TagName};
use speedyspec_wp_text_memory::FixedSiteSettings;

#[path = "common/api.rs"]
mod api;

fn add(h: &api::Harness, hook: &str, run: impl Fn(&[HookValue]) -> HookValue + 'static) {
    h.hooks
        .add_filter(&HookName::from(hook), Callback::rust(hook, move |args: &[HookValue]| Ok(run(args))), &HookValue::Int(10), 4)
        .unwrap();
}

fn str_value(s: &str) -> HookValue {
    HookValue::Str(s.into())
}

fn bytes_of(value: &HookValue) -> Vec<u8> {
    match value {
        HookValue::Str(s) => s.as_bytes().to_vec(),
        HookValue::Bytes(b) => b.clone(),
        other => panic!("not a string: {other:?}"),
    }
}

#[test]
fn run_wptexturize_false_is_sticky() {
    let h = api::harness();
    add(&h, "run_wptexturize", |_| HookValue::Bool(false));
    assert_eq!(bytes_of(&h.api.wptexturize(&str_value("'a'"), false).unwrap()), b"'a'");
    h.hooks.remove_all_filters(&HookName::from("run_wptexturize"), None).unwrap();
    assert_eq!(bytes_of(&h.api.wptexturize(&str_value("'a'"), true).unwrap()), b"'a'");
}

#[test]
fn zero_does_not_build_tables() {
    let h = api::harness();
    let fired = Rc::new(Cell::new(0));
    let seen = fired.clone();
    add(&h, "run_wptexturize", move |args| {
        seen.set(seen.get() + 1);
        args[0].clone()
    });
    assert_eq!(bytes_of(&h.api.wptexturize(&str_value("0"), true).unwrap()), b"0");
    assert_eq!(fired.get(), 0);
    h.api.wptexturize(&str_value("a"), false).unwrap();
    assert_eq!(fired.get(), 1);
    h.api.wptexturize(&str_value("b"), false).unwrap();
    assert_eq!(fired.get(), 1);
    h.api.wptexturize(&str_value("c"), true).unwrap();
    assert_eq!(fired.get(), 2);
}

#[test]
fn no_texturize_tags_odd_values() {
    let h = api::harness();
    add(&h, "no_texturize_tags", |_| HookValue::Int(3));
    assert_eq!(bytes_of(&h.api.wptexturize(&str_value("<code>'a'</code>"), false).unwrap()), b"<code>&#8216;a&#8217;</code>");

    let h = api::harness();
    add(&h, "no_texturize_tags", |_| HookValue::Array(vec![(Key::Int(0), str_value("code")), (Key::Int(1), HookValue::Int(5))]));
    assert_eq!(bytes_of(&h.api.wptexturize(&str_value("<code>'a'</code>"), false).unwrap()), b"<code>'a'</code>");
}

#[test]
fn custom_spaces_fragment() {
    let h = api::harness();
    add(&h, "wp_spaces_regexp", |_| str_value("[\\r\\n\\t ]|_"));
    assert_eq!(bytes_of(&h.api.wptexturize(&str_value("_'a'_"), false).unwrap()), b"_&#8216;a&#8217;_");

    let h = api::harness();
    let fired = Rc::new(Cell::new(0));
    let seen = fired.clone();
    add(&h, "wp_spaces_regexp", move |_| {
        seen.set(seen.get() + 1);
        str_value("")
    });
    h.api.wp_spaces_regexp().unwrap();
    h.api.wp_spaces_regexp().unwrap();
    assert_eq!(fired.get(), 2);
}

#[test]
fn pre_ent2ncr_short_circuits() {
    let h = api::harness();
    add(&h, "pre_ent2ncr", |_| str_value("X"));
    assert_eq!(h.api.ent2ncr(b"&quot;").unwrap(), str_value("X"));
}

#[test]
fn capital_p_outside_a_title() {
    let h = api::harness();
    assert_eq!(&*h.api.capital_p_dangit(b"Wordpress").unwrap(), b"Wordpress");
    assert_eq!(&*h.api.capital_p_dangit(b"(Wordpress").unwrap(), b"(WordPress");
}

/// Shortcode names that a test can change between calls.
struct Changing(RefCell<Vec<TagName>>);

impl ShortcodeTagNames for Changing {
    fn tag_names(&self) -> Vec<TagName> {
        self.0.borrow().clone()
    }

    fn generation(&self) -> u64 {
        self.0.borrow().len() as u64
    }
}

#[test]
fn unautop_sees_new_tags() {
    let tags = Rc::new(Changing(RefCell::new(vec![TagName::new(b"gallery").unwrap()])));
    let h = api::harness_with(FixedSiteSettings::default(), tags.clone());
    assert_eq!(&*h.api.shortcode_unautop(b"<p>[later]</p>").unwrap(), b"<p>[later]</p>");
    tags.0.borrow_mut().push(TagName::new(b"later").unwrap());
    assert_eq!(&*h.api.shortcode_unautop(b"<p>[later]</p>").unwrap(), b"[later]");
}

#[test]
fn trim_words_filter_args() {
    let h = api::harness();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let kept = seen.clone();
    add(&h, "wp_trim_words", move |args| {
        *kept.borrow_mut() = args.to_vec();
        args[0].clone()
    });
    h.api.wp_trim_words(b"<b>a b</b>", 55, None).unwrap();
    assert_eq!(*seen.borrow(), vec![str_value("a b"), HookValue::Int(55), str_value("&hellip;"), str_value("<b>a b</b>")]);
}

#[test]
fn format_for_editor_filter_args() {
    let h = api::harness();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let kept = seen.clone();
    add(&h, "format_for_editor", move |args| {
        *kept.borrow_mut() = args.to_vec();
        args[0].clone()
    });
    h.api.format_for_editor(b"<b>", &str_value("html")).unwrap();
    assert_eq!(*seen.borrow(), vec![str_value("&lt;b&gt;"), str_value("html")]);
}

#[test]
fn strip_all_tags_reports_objects_and_arrays() {
    let h = api::harness();
    assert_eq!(h.api.wp_strip_all_tags(&HookValue::Array(Vec::new()), false).unwrap(), b"");
    let message = "Warning: wp_strip_all_tags expects parameter #1 ($text) to be a string, array given.";
    assert_eq!(
        h.diagnostics.taken(),
        vec![speedyspec_wp_text_memory::Diagnostic::TriggeredError {
            function: String::new(),
            message: message.into(),
            level: speedyspec_wp_text_contracts::ErrorLevel::Warning,
        }]
    );
    assert_eq!(h.api.wp_strip_all_tags(&HookValue::Null, false).unwrap(), b"");
    assert!(h.diagnostics.taken().is_empty());
}

#[test]
fn hook_error_payload_survives() {
    use speedyspec_wp_hooks_contracts::HookError;
    let h = api::harness();
    h.hooks
        .add_filter(
            &HookName::from("clean_url"),
            Callback::rust("boom", |_: &[HookValue]| {
                Err(HookError { message: "boom".into(), payload: Some(Rc::new(String::from("the exception"))) })
            }),
            &HookValue::Int(10),
            3,
        )
        .unwrap();
    let error = h.api.esc_url(b"http://a.b", None, b"display").unwrap_err();
    let payload = error.payload.expect("the payload");
    assert_eq!(payload.downcast_ref::<String>().map(String::as_str), Some("the exception"));
}

#[test]
fn new_request_forgets_run_wptexturize_false() {
    let h = api::harness();
    add(&h, "run_wptexturize", |_| HookValue::Bool(false));
    assert_eq!(bytes_of(&h.api.wptexturize(&str_value("'a'"), false).unwrap()), b"'a'");
    h.hooks.remove_all_filters(&HookName::from("run_wptexturize"), None).unwrap();
    assert_eq!(bytes_of(&h.api.wptexturize(&str_value("'a'"), false).unwrap()), b"'a'");
    h.api.new_request();
    assert_eq!(bytes_of(&h.api.wptexturize(&str_value("'a'"), false).unwrap()), "&#8216;a&#8217;".as_bytes());
}
