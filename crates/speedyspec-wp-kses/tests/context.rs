use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_wp_hooks::{Callback, HookApi, HookName, HookValue, IgnoreDeprecations, Key};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_kses::{
    DefaultGlobals, KsesApi, KsesDeps, NoValueCallbacks, UploadsLocation, allowed_from_value,
    default_post_value, default_tags_value,
};
use speedyspec_wp_kses_contracts::{
    AllowedAttrs, AttrRules, AttributeEscaper, Context, KsesError, UnfilteredHtmlPolicy,
};

struct Escaper;
impl AttributeEscaper for Escaper {
    fn esc_attr(&self, v: &[u8]) -> Result<Vec<u8>, KsesError> {
        Ok(v.to_vec())
    }
}
struct Policy;
impl UnfilteredHtmlPolicy for Policy {
    fn can_unfiltered_html(&self) -> Result<bool, KsesError> {
        Ok(false)
    }
}
struct Uploads;
impl UploadsLocation for Uploads {
    fn upload_url(&self) -> Result<Vec<u8>, KsesError> {
        Ok(b"http://example.org/wp-content/uploads".to_vec())
    }
}

fn setup() -> (Rc<HookApi>, KsesApi) {
    let hooks = Rc::new(HookApi::new(
        Rc::new(MemoryHookStore::new()),
        Rc::new(IgnoreDeprecations),
    ));
    let api = KsesApi::new(KsesDeps {
        hooks: hooks.clone(),
        escaper: Rc::new(Escaper),
        policy: Rc::new(Policy),
        globals: Rc::new(DefaultGlobals::default()),
        uploads: Rc::new(Uploads),
        callbacks: Rc::new(NoValueCallbacks),
    });
    (hooks, api)
}

fn filter(hooks: &HookApi, name: &str, run: impl Fn(&[HookValue]) -> HookValue + 'static) {
    hooks
        .add_filter(
            &HookName::from(name),
            Callback::rust(name, move |a: &[HookValue]| Ok(run(a))),
            &HookValue::Int(10),
            4,
        )
        .unwrap();
}

fn no_callbacks(_: &HookValue) -> Option<speedyspec_wp_kses_contracts::ValueCallback> {
    None
}

#[test]
fn post_context_is_the_defaults() {
    let (_, api) = setup();
    let post = api.allowed_html(&Context::Post).unwrap();
    let expected = allowed_from_value(&default_post_value(), &no_callbacks).unwrap();
    assert_eq!(
        post.tags.keys().collect::<Vec<_>>(),
        expected.tags.keys().collect::<Vec<_>>()
    );
    assert!(post.get(b"form").is_none());
}

#[test]
fn post_context_adds_form_when_a_filter_adds_input() {
    let (hooks, api) = setup();
    filter(&hooks, "wp_kses_allowed_html", |a| {
        let mut v = a[0].clone();
        if let HookValue::Array(items) = &mut v {
            if !items
                .iter()
                .any(|(k, _)| matches!(k, Key::Str(s) if s == "input"))
            {
                items.push((Key::Str("input".into()), HookValue::Bool(true)));
            }
        }
        v
    });
    let post = api.allowed_html(&Context::Post).unwrap();
    assert!(post.get(b"form").is_some());
    assert_eq!(
        hooks
            .did_filter(&HookName::from("wp_kses_allowed_html"))
            .unwrap(),
        2
    );
}

#[test]
fn strip_is_empty() {
    let (_, api) = setup();
    assert!(api.allowed_html(&Context::Strip).unwrap().tags.is_empty());
}

#[test]
fn named_context_falls_to_data() {
    let (hooks, api) = setup();
    let seen = Rc::new(RefCell::new(None));
    let s = seen.clone();
    filter(&hooks, "wp_kses_allowed_html", move |a| {
        *s.borrow_mut() = Some(a[1].clone());
        a[0].clone()
    });
    let named = api.allowed_html(&Context::from_bytes(b"foo")).unwrap();
    let data = allowed_from_value(&default_tags_value(), &no_callbacks).unwrap();
    assert_eq!(
        named.tags.keys().collect::<Vec<_>>(),
        data.tags.keys().collect::<Vec<_>>()
    );
    assert_eq!(*seen.borrow(), Some(HookValue::Str("foo".into())));
}

#[test]
fn filter_changes_result() {
    let (hooks, api) = setup();
    filter(&hooks, "wp_kses_allowed_html", |a| {
        let mut v = a[0].clone();
        if let HookValue::Array(items) = &mut v {
            items.push((Key::Str("x-tag".into()), HookValue::Bool(true)));
        }
        v
    });
    assert_eq!(
        api.allowed_html(&Context::Data).unwrap().get(b"x-tag"),
        Some(&AllowedAttrs::Empty)
    );
}

#[test]
fn fast_path_still_counts() {
    let (hooks, api) = setup();
    api.allowed_html(&Context::Data).unwrap();
    assert_eq!(
        hooks
            .did_filter(&HookName::from("wp_kses_allowed_html"))
            .unwrap(),
        1
    );
}

#[test]
fn user_description_adds_rel_and_target() {
    let (_, api) = setup();
    let tags = api.allowed_html(&Context::UserDescription).unwrap();
    let Some(AllowedAttrs::Attrs(a)) = tags.get(b"a") else {
        panic!("a")
    };
    assert_eq!(a.get(&b"rel"[..]), Some(&AttrRules::Allowed));
    assert_eq!(a.get(&b"target"[..]), Some(&AttrRules::Allowed));
}

#[test]
fn uri_attributes_are_filtered() {
    let (hooks, api) = setup();
    assert_eq!(api.uri_attributes().unwrap().0.len(), 17);
    filter(&hooks, "wp_kses_uri_attributes", |a| {
        let mut v = a[0].clone();
        if let HookValue::Array(items) = &mut v {
            items.push((Key::Int(17), HookValue::Str("data-src".into())));
        }
        v
    });
    assert!(
        api.uri_attributes()
            .unwrap()
            .0
            .iter()
            .any(|u| &**u == b"data-src")
    );
}

#[test]
fn protocols_are_cached_after_wp_loaded() {
    let (hooks, api) = setup();
    filter(&hooks, "kses_allowed_protocols", |a| {
        let mut v = a[0].clone();
        if let HookValue::Array(items) = &mut v {
            items.push((Key::Int(99), HookValue::Str("x".into())));
        }
        v
    });
    assert!(
        api.allowed_protocols()
            .unwrap()
            .0
            .iter()
            .any(|p| &**p == b"x")
    );
    hooks
        .do_action(&HookName::from("wp_loaded"), vec![])
        .unwrap();
    hooks
        .remove_all_filters(&HookName::from("kses_allowed_protocols"), None)
        .unwrap();
    assert!(
        api.allowed_protocols()
            .unwrap()
            .0
            .iter()
            .any(|p| &**p == b"x")
    );
}
