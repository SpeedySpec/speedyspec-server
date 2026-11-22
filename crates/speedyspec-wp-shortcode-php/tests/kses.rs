//! `RustKsesAttributes`: the shortcode `KsesAttributes` port over the Rust kses, with kses itself a test double.

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_wp_kses_contracts::{AllowedSpec, Kses, KsesError, Payload, Protocols};
use speedyspec_wp_shortcode_contracts::{KsesAttributes, ShortcodeError};
use speedyspec_wp_shortcode_php::RustKsesAttributes;

/// Records each `one_attr` call and answers with a scripted result.
struct ScriptedKses {
    answer: Result<Vec<u8>, KsesError>,
    calls: RefCell<Vec<(Vec<u8>, Vec<u8>)>>,
}

impl ScriptedKses {
    fn answering(answer: Result<Vec<u8>, KsesError>) -> Rc<Self> {
        Rc::new(Self { answer, calls: RefCell::new(Vec::new()) })
    }
}

impl Kses for ScriptedKses {
    fn kses(&self, _: &[u8], _: &AllowedSpec, _: Option<&Protocols>) -> Result<Vec<u8>, KsesError> {
        unreachable!("the shortcode port calls only one_attr")
    }
    fn one_attr(&self, attr: &[u8], element: &[u8]) -> Result<Vec<u8>, KsesError> {
        self.calls.borrow_mut().push((attr.to_vec(), element.to_vec()));
        self.answer.clone()
    }
    fn post(&self, _: &[u8]) -> Result<Vec<u8>, KsesError> {
        unreachable!("the shortcode port calls only one_attr")
    }
    fn data(&self, _: &[u8]) -> Result<Vec<u8>, KsesError> {
        unreachable!("the shortcode port calls only one_attr")
    }
}

#[test]
fn attr_parse_splits_an_element_into_slices_of_it() {
    let port = RustKsesAttributes::new(ScriptedKses::answering(Ok(Vec::new())));
    let element: &[u8] = br#"<a href="[url]" title='x'>"#;
    let parts = port.attr_parse(element).expect("an opening tag parses");
    assert_eq!(parts, vec![&b"<a "[..], &br#"href="[url]" "#[..], &b"title='x'"[..], &b">"[..]]);
    for part in parts {
        let range = element.as_ptr_range();
        assert!(range.contains(&part.as_ptr()), "every piece borrows from the element");
    }
}

#[test]
fn attr_parse_is_none_for_a_closing_tag() {
    let port = RustKsesAttributes::new(ScriptedKses::answering(Ok(Vec::new())));
    assert_eq!(port.attr_parse(b"</a>"), None);
}

#[test]
fn one_attr_returns_what_kses_returns_for_the_attribute_and_element() {
    let kses = ScriptedKses::answering(Ok(b"href=\"http://x/\"".to_vec()));
    let port = RustKsesAttributes::new(kses.clone());
    assert_eq!(port.one_attr(b"href=\"http://x/\"", b"a").unwrap(), b"href=\"http://x/\"".to_vec());
    assert_eq!(*kses.calls.borrow(), vec![(b"href=\"http://x/\"".to_vec(), b"a".to_vec())]);
}

#[test]
fn one_attr_carries_a_foreign_failure_unchanged_so_the_bridge_rethrows_it() {
    let payload = Payload(Rc::new(42_i64));
    let port = RustKsesAttributes::new(ScriptedKses::answering(Err(KsesError::Foreign {
        message: "a filter threw".into(),
        payload: payload.clone(),
    })));
    match port.one_attr(b"x", b"a") {
        Err(ShortcodeError::Thrown(thrown)) => {
            assert_eq!(thrown.message, "a filter threw");
            assert!(Rc::ptr_eq(&thrown.payload, &payload.0), "the same object is rethrown");
        }
        other => panic!("expected Thrown, got {other:?}"),
    }
}

#[test]
fn one_attr_reports_a_hook_failure_as_a_filter_error() {
    let port = RustKsesAttributes::new(ScriptedKses::answering(Err(KsesError::Hook("no hook".into()))));
    assert!(matches!(port.one_attr(b"x", b"a"), Err(ShortcodeError::Filter(m)) if m.contains("no hook")));
}

#[test]
fn one_attr_reports_a_callback_failure_as_a_filter_error() {
    let port = RustKsesAttributes::new(ScriptedKses::answering(Err(KsesError::Callback("bad".into()))));
    assert!(matches!(port.one_attr(b"x", b"a"), Err(ShortcodeError::Filter(m)) if m.contains("bad")));
}
