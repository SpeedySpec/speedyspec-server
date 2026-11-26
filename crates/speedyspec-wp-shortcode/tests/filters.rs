//! The shortcode filters through the Rust Hook API, and every function as a message on the buses.

mod support;

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{CommandBus, QueryBus};
use speedyspec_wp_hooks::{WpHookApi, IgnoreDeprecations};
use speedyspec_wp_hooks_contracts::{Callback, CallbackId, HasFilterResult, HookName, HookValue};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_shortcode::{AddShortcode, DoShortcode, HookApiFilters, ShortcodeExists, register_shortcode_handlers};
use speedyspec_wp_shortcode_contracts::{PhpValue, ShortcodeError};
use support::doubles::{handler, harness_with};

fn hooks() -> Rc<WpHookApi> {
    Rc::new(WpHookApi::new(Rc::new(MemoryHookStore::new()), Rc::new(IgnoreDeprecations)))
}

fn context_attached(hooks: &WpHookApi) -> bool {
    hooks
        .has_filter(
            &HookName::from("wp_get_attachment_image_context"),
            Some(&CallbackId(Some("_filter_do_shortcode_context".into()))),
            None,
        )
        .unwrap()
        != HasFilterResult::No
}

#[test]
fn did_filter_counts_without_callbacks() {
    let hooks = hooks();
    let h = harness_with(Rc::new(HookApiFilters::new(hooks.clone())));
    h.api.add_shortcode(b"a", handler(|_, _, _| Ok(PhpValue::Str("A".into()))));
    assert_eq!(h.api.do_shortcode(b"[a]", false).unwrap(), b"A".to_vec());
    assert_eq!(hooks.did_filter(&HookName::from("pre_do_shortcode_tag")).unwrap(), 1);
    assert_eq!(hooks.did_filter(&HookName::from("do_shortcode_tag")).unwrap(), 1);
}

#[test]
fn all_hook_runs() {
    let hooks = hooks();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let record = seen.clone();
    hooks
        .add_filter(
            &HookName::from("all"),
            Callback::rust("record", move |args: &[HookValue]| {
                record.borrow_mut().push(args[0].clone());
                Ok(HookValue::Null)
            }),
            &HookValue::Int(10),
            1,
        )
        .unwrap();
    let h = harness_with(Rc::new(HookApiFilters::new(hooks.clone())));
    h.api.add_shortcode(b"a", handler(|_, _, _| Ok(PhpValue::Str("A".into()))));
    h.api.do_shortcode(b"[a]", false).unwrap();
    assert!(seen.borrow().contains(&HookValue::Str("pre_do_shortcode_tag".into())));
    assert!(seen.borrow().contains(&HookValue::Str("do_shortcode_tag".into())));
}

#[test]
fn filters_change_output_and_short_circuit() {
    let hooks = hooks();
    let h = harness_with(Rc::new(HookApiFilters::new(hooks.clone())));
    h.api.add_shortcode(b"t", handler(|_, _, _| Ok(PhpValue::Str("foo".into()))));
    hooks
        .add_filter(
            &HookName::from("do_shortcode_tag"),
            Callback::rust("replace", |args: &[HookValue]| match &args[0] {
                HookValue::Str(s) => Ok(HookValue::Str(s.replace("oo", "ee"))),
                other => Ok(other.clone()),
            }),
            &HookValue::Int(10),
            1,
        )
        .unwrap();
    assert_eq!(h.api.do_shortcode(b"[t]", false).unwrap(), b"fee".to_vec());
    hooks
        .add_filter(&HookName::from("pre_do_shortcode_tag"), Callback::rust("bar", |_: &[HookValue]| Ok(HookValue::Str("bar".into()))), &HookValue::Int(10), 1)
        .unwrap();
    assert_eq!(h.api.do_shortcode(b"[t]", false).unwrap(), b"bar".to_vec());
}

#[test]
fn shortcode_atts_uses_the_dynamic_hook_name() {
    let hooks = hooks();
    let h = harness_with(Rc::new(HookApiFilters::new(hooks.clone())));
    hooks
        .add_filter(
            &HookName::from("shortcode_atts_bartag"),
            Callback::rust("override", |_: &[HookValue]| Ok(HookValue::Str("filtered".into()))),
            &HookValue::Int(10),
            4,
        )
        .unwrap();
    assert_eq!(h.api.shortcode_atts(&PhpValue::Array(vec![]), &PhpValue::Null, b"bartag").unwrap(), PhpValue::Str("filtered".into()));
    assert_eq!(h.api.shortcode_atts(&PhpValue::Array(vec![]), &PhpValue::Null, b"other").unwrap(), PhpValue::Array(vec![]));
}

#[test]
fn context_filter_added_once_nested_and_answers_by_name() {
    let hooks = hooks();
    let h = harness_with(Rc::new(HookApiFilters::new(hooks.clone())));
    let api = Rc::downgrade(&h.api);
    let inner_hooks = hooks.clone();
    let checks = Rc::new(RefCell::new(Vec::new()));
    let record = checks.clone();
    h.api.add_shortcode(b"leaf", handler(|_, _, _| Ok(PhpValue::Str("L".into()))));
    h.api.add_shortcode(
        b"outer",
        handler(move |_, content, _| {
            record.borrow_mut().push(context_attached(&inner_hooks));
            let context = inner_hooks
                .apply_filters(&HookName::from("wp_get_attachment_image_context"), HookValue::Str("x".into()), vec![])
                .unwrap();
            record.borrow_mut().push(context == HookValue::Str("do_shortcode".into()));
            let nested = api.upgrade().unwrap().do_shortcode(content.unwrap(), false)?;
            record.borrow_mut().push(context_attached(&inner_hooks));
            Ok(speedyspec_wp_shortcode_contracts::php_bytes(nested))
        }),
    );
    assert_eq!(h.api.do_shortcode(b"[outer][leaf][/outer]", false).unwrap(), b"L".to_vec());
    assert_eq!(*checks.borrow(), vec![true, true, true]);
    assert!(!context_attached(&hooks));
}

#[test]
fn a_throwing_filter_keeps_its_payload() {
    let hooks = hooks();
    let h = harness_with(Rc::new(HookApiFilters::new(hooks.clone())));
    h.api.add_shortcode(b"t", handler(|_, _, _| Ok(PhpValue::Str("x".into()))));
    hooks
        .add_filter(
            &HookName::from("do_shortcode_tag"),
            Callback::rust("throw", |_: &[HookValue]| {
                Err(speedyspec_wp_hooks_contracts::HookError { message: "boom".into(), payload: Some(Rc::new(42_u8)) })
            }),
            &HookValue::Int(10),
            1,
        )
        .unwrap();
    match h.api.do_shortcode(b"[t]", false) {
        Err(ShortcodeError::Thrown(t)) => assert_eq!(t.payload.downcast_ref::<u8>(), Some(&42)),
        other => panic!("expected Thrown, got {other:?}"),
    }
    assert!(context_attached(&hooks), "WordPress leaves the context filter attached when an exception escapes");
}

#[test]
fn messages_dispatch() {
    let container = Rc::new(Container::new());
    let h = harness_with(Rc::new(speedyspec_wp_shortcode::NoFilters));
    register_shortcode_handlers(&container, h.api.clone());
    CommandBus::new(container.clone())
        .dispatch(AddShortcode { tag: b"m".to_vec(), handler: handler(|_, _, _| Ok(PhpValue::Str("M".into()))) })
        .unwrap();
    let queries = QueryBus::new(container);
    assert!(queries.ask(ShortcodeExists { tag: b"m".to_vec() }).unwrap());
    assert_eq!(queries.ask(DoShortcode { content: b"[m]".to_vec(), ignore_html: false }).unwrap().unwrap(), b"M".to_vec());
}
