use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_wp_hooks::{HookApi, IgnoreDeprecations};
use speedyspec_wp_hooks_contracts::{Callback, HookName, HookValue, Key};
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
        html_api: Rc::new(speedyspec_wp_kses::NoHtmlApiSanitizer),
        mode: Rc::new(speedyspec_wp_kses::MemoryOperatingMode::default()),
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
        if let HookValue::Array(items) = &mut v
            && !items
                .iter()
                .any(|(k, _)| matches!(k, Key::Str(s) if s == "input"))
        {
            items.push((Key::Str("input".into()), HookValue::Bool(true)));
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

/// `wp_kses_allowed_html()` as PHP returns it: the filtered array itself, value callbacks and all.
#[test]
fn allowed_html_value_is_the_filtered_array() {
    let (hooks, api) = setup();
    assert_eq!(
        api.allowed_html_value(&Context::Post).unwrap(),
        default_post_value()
    );
    assert_eq!(
        api.allowed_html_value(&Context::Strip).unwrap(),
        HookValue::Array(vec![])
    );
    let callable = HookValue::Str("my_check".into());
    let keep = callable.clone();
    filter(&hooks, "wp_kses_allowed_html", move |a| {
        let mut v = a[0].clone();
        if let HookValue::Array(items) = &mut v {
            let rules = HookValue::Array(vec![(Key::Str("value_callback".into()), keep.clone())]);
            items.push((
                Key::Str("x-tag".into()),
                HookValue::Array(vec![(Key::Str("data-x".into()), rules)]),
            ));
        }
        v
    });
    let HookValue::Array(items) = api.allowed_html_value(&Context::Data).unwrap() else {
        panic!("array")
    };
    let (_, x) = items
        .iter()
        .find(|(k, _)| *k == Key::Str("x-tag".into()))
        .expect("x-tag");
    let HookValue::Array(attrs) = x else {
        panic!("attrs")
    };
    assert_eq!(
        attrs[0].1,
        HookValue::Array(vec![(Key::Str("value_callback".into()), callable)])
    );
}

#[test]
fn allowed_html_explicit_value_runs_the_explicit_filter() {
    let (hooks, api) = setup();
    let seen = Rc::new(RefCell::new(None));
    let keep = seen.clone();
    filter(&hooks, "wp_kses_allowed_html", move |a| {
        *keep.borrow_mut() = a.get(1).cloned();
        a[0].clone()
    });
    let mine = HookValue::Array(vec![(Key::Str("b".into()), HookValue::Bool(true))]);
    assert_eq!(api.allowed_html_explicit_value(&mine).unwrap(), mine);
    assert_eq!(*seen.borrow(), Some(HookValue::Str("explicit".into())));
}

/// wp_kses() itself never filters an explicit array: split2 and attr only resolve a context string.
#[test]
fn allowed_from_runs_no_filter() {
    let (hooks, api) = setup();
    let mine = HookValue::Array(vec![(Key::Str("B".into()), HookValue::Bool(true))]);
    let allowed = api.allowed_from(&mine).unwrap();
    assert!(allowed.tags.contains_key(&b"B"[..]));
    assert_eq!(
        hooks
            .did_filter(&HookName::from("wp_kses_allowed_html"))
            .unwrap(),
        0
    );
}

/// A new request starts with WordPress's `static` caches empty again: the protocol list a filter narrowed before
/// `wp_loaded` goes back to the 22 defaults.
#[test]
fn new_request_empties_the_protocol_list() {
    let (hooks, api) = setup();
    filter(&hooks, "kses_allowed_protocols", |_| {
        HookValue::Array(vec![(Key::Int(0), HookValue::Str("http".into()))])
    });
    assert_eq!(api.allowed_protocols().unwrap().0.len(), 1);
    hooks
        .do_action(&HookName::from("wp_loaded"), vec![])
        .unwrap();
    assert_eq!(api.allowed_protocols().unwrap().0.len(), 1);
    api.new_request();
    assert_eq!(api.allowed_protocols().unwrap().0.len(), 22);
}

/// `wp_kses_allowed_html( 'entities' )` is `$allowedentitynames` itself, in WordPress's order.
#[test]
fn entities_context_keeps_wordpress_order() {
    let (_, api) = setup();
    let HookValue::Array(items) = api.allowed_html_value(&Context::Entities).unwrap() else {
        panic!("array")
    };
    let first: Vec<HookValue> = items.iter().take(3).map(|(_, v)| v.clone()).collect();
    assert_eq!(
        first,
        vec![
            HookValue::Str("nbsp".into()),
            HookValue::Str("iexcl".into()),
            HookValue::Str("cent".into())
        ]
    );
    assert_eq!(items.len(), 253);
}
