//! WordPress 7.2's `wp_kses()`: the `wp_kses_force_legacy_parser` filter hands the call to `wp_sanitize_html_kses()`,
//! and `$wp_kses_operating_mode` is `legacy` while the `pre_kses` filters run.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{Escaper, Policy, Uploads, filter};
use speedyspec_wp_hooks::{IgnoreDeprecations, WpHookApi};
use speedyspec_wp_hooks_contracts::HookValue;
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_kses::{
    DefaultGlobals, KsesApi, KsesDeps, MemoryOperatingMode, NoHtmlApiSanitizer, NoValueCallbacks,
};
use speedyspec_wp_kses_contracts::{
    AllowedSpec, Context, HtmlApiSanitizer, KsesError, OperatingMode, Protocols,
};

/// Answers every call with a fixed marker and keeps what it was given.
#[derive(Default)]
struct Marker(RefCell<Vec<(Vec<u8>, bool)>>);

impl HtmlApiSanitizer for Marker {
    fn sanitize(
        &self,
        content: &[u8],
        allowed: &AllowedSpec,
        protocols: Option<&Protocols>,
    ) -> Result<Option<Vec<u8>>, KsesError> {
        let post = matches!(allowed, AllowedSpec::Context(Context::Post));
        self.0
            .borrow_mut()
            .push((content.to_vec(), post && protocols.is_none()));
        Ok(Some(b"html-api".to_vec()))
    }
}

fn api(html_api: Rc<dyn HtmlApiSanitizer>, mode: Rc<dyn OperatingMode>) -> (Rc<WpHookApi>, KsesApi) {
    let hooks = Rc::new(WpHookApi::new(
        Rc::new(MemoryHookStore::new()),
        Rc::new(IgnoreDeprecations),
    ));
    let api = KsesApi::new(KsesDeps {
        hooks: hooks.clone(),
        escaper: Rc::new(Escaper),
        policy: Rc::new(Policy(false)),
        globals: Rc::new(DefaultGlobals::default()),
        uploads: Rc::new(Uploads),
        callbacks: Rc::new(NoValueCallbacks),
        html_api,
        mode,
    });
    (hooks, api)
}

#[test]
fn legacy_parser_runs_by_default_and_fires_the_filter() {
    let marker = Rc::new(Marker::default());
    let (hooks, api) = api(marker.clone(), Rc::new(MemoryOperatingMode::default()));
    let out = api
        .kses(
            b"<b>x</b><script>y</script>",
            &AllowedSpec::Context(Context::Post),
            None,
        )
        .unwrap();
    assert_eq!(out, b"<b>x</b>y");
    assert_eq!(common::did_filter(&hooks, "wp_kses_force_legacy_parser"), 1);
    assert!(marker.0.borrow().is_empty());
}

#[test]
fn a_false_filter_hands_the_call_to_the_html_api() {
    let marker = Rc::new(Marker::default());
    let (hooks, api) = api(marker.clone(), Rc::new(MemoryOperatingMode::default()));
    filter(&hooks, "wp_kses_force_legacy_parser", |_| {
        HookValue::Bool(false)
    });
    let out = api
        .kses(b"<b>x</b>", &AllowedSpec::Context(Context::Post), None)
        .unwrap();
    assert_eq!(out, b"html-api");
    assert_eq!(*marker.0.borrow(), vec![(b"<b>x</b>".to_vec(), true)]);
}

#[test]
fn without_an_html_api_the_legacy_parser_runs() {
    let (hooks, api) = api(
        Rc::new(NoHtmlApiSanitizer),
        Rc::new(MemoryOperatingMode::default()),
    );
    filter(&hooks, "wp_kses_force_legacy_parser", |_| HookValue::Int(0));
    let out = api
        .kses(
            b"<b>x</b><script>y</script>",
            &AllowedSpec::Context(Context::Post),
            None,
        )
        .unwrap();
    assert_eq!(out, b"<b>x</b>y");
}

#[test]
fn pre_kses_runs_in_legacy_mode_and_the_mode_is_restored() {
    let mode = Rc::new(MemoryOperatingMode::default());
    let (hooks, api) = api(Rc::new(NoHtmlApiSanitizer), mode.clone());
    let seen = Rc::new(RefCell::new(Vec::new()));
    let (reader, log) = (mode.clone(), seen.clone());
    filter(&hooks, "pre_kses", move |a| {
        log.borrow_mut().push(reader.get());
        a[0].clone()
    });
    let previous = mode.enter("html-api");
    api.kses(b"<b>x</b>", &AllowedSpec::Context(Context::Post), None)
        .unwrap();
    assert_eq!(*seen.borrow(), vec![HookValue::Str("legacy".into())]);
    assert_eq!(mode.get(), HookValue::Str("html-api".into()));
    mode.restore(previous);
    assert_eq!(mode.get(), HookValue::Str("legacy".into()));
}

#[test]
fn kses_hook_alone_leaves_the_mode() {
    let mode = Rc::new(MemoryOperatingMode::default());
    let (hooks, api) = api(Rc::new(NoHtmlApiSanitizer), mode.clone());
    let seen = Rc::new(RefCell::new(Vec::new()));
    let (reader, log) = (mode.clone(), seen.clone());
    filter(&hooks, "pre_kses", move |a| {
        log.borrow_mut().push(reader.get());
        a[0].clone()
    });
    let _previous = mode.enter("html-api");
    api.kses_hook(
        b"x",
        &AllowedSpec::Context(Context::Post),
        &Protocols(Vec::new()),
    )
    .unwrap();
    assert_eq!(*seen.borrow(), vec![HookValue::Str("html-api".into())]);
}
