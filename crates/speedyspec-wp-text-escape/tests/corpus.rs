//! The filtered escaping functions through `EscapeApi`, against the corpus, diagnostics included.

mod support;

use std::rc::Rc;

use speedyspec_wp_hooks::{HookApi, IgnoreDeprecations};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_text_corpus::{Php, assert_corpus_reporting, diagnostics, from_value};
use speedyspec_wp_text_escape::EscapeApi;
use speedyspec_wp_text_memory::{FixedSiteSettings, RecordingDiagnostics};

struct Harness {
    api: EscapeApi,
    diagnostics: Rc<RecordingDiagnostics>,
}

fn harness() -> Harness {
    let diagnostics = Rc::new(RecordingDiagnostics::default());
    let api = EscapeApi::new(
        Rc::new(HookApi::new(Rc::new(MemoryHookStore::new()), Rc::new(IgnoreDeprecations))),
        Rc::new(FixedSiteSettings::default()),
        diagnostics.clone(),
    );
    Harness { api, diagnostics }
}

fn check(function: &str, run: impl Fn(&EscapeApi, &[u8]) -> Php) {
    let h = harness();
    assert_corpus_reporting(function, |args| {
        let out = run(&h.api, args[0].bytes().expect("a string"));
        (out, diagnostics(&h.diagnostics.taken()))
    });
}

#[test]
fn esc_html() {
    check("esc_html", |api, t| from_value(&api.esc_html(t).unwrap()));
}

#[test]
fn esc_attr() {
    check("esc_attr", |api, t| from_value(&api.esc_attr(t).unwrap()));
}

#[test]
fn esc_textarea() {
    check("esc_textarea", |api, t| from_value(&api.esc_textarea(t).unwrap()));
}

#[test]
fn esc_xml() {
    check("esc_xml", |api, t| from_value(&api.esc_xml(t).unwrap()));
}

#[test]
fn esc_js() {
    check("esc_js", |api, t| from_value(&api.esc_js(t).unwrap()));
}

#[test]
fn tag_escape() {
    check("tag_escape", |api, t| from_value(&api.tag_escape(t).unwrap()));
}

#[test]
fn seems_utf8() {
    check("seems_utf8", |api, t| Php::Bool(api.seems_utf8(t).unwrap()));
}

#[test]
fn addslashes_gpc() {
    let h = harness();
    assert_corpus_reporting("addslashes_gpc", |args| {
        let out = from_value(&h.api.addslashes_gpc(&support::value(&args[0])).unwrap());
        (out, diagnostics(&h.diagnostics.taken()))
    });
}
