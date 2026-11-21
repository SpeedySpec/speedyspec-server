//! Shared test setup: a KsesApi over an in-memory Hook API with simple test doubles for the outbound ports.
#![allow(dead_code)]

use std::rc::Rc;

use speedyspec_wp_hooks::{Callback, HookApi, HookName, HookValue, IgnoreDeprecations};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_kses::{DefaultGlobals, KsesApi, KsesDeps, NoValueCallbacks, UploadsLocation};
use speedyspec_wp_kses_contracts::{AttributeEscaper, KsesError, UnfilteredHtmlPolicy};

/// `esc_attr()` as WordPress 7.1.5 behaves with no filter on valid UTF-8: entities normalized, then
/// `htmlspecialchars( ENT_QUOTES, double_encode false )`.
pub struct Escaper;
impl AttributeEscaper for Escaper {
    fn esc_attr(&self, v: &[u8]) -> Result<Vec<u8>, KsesError> {
        if std::str::from_utf8(v).is_err() {
            return Ok(Vec::new());
        }
        let normalized = speedyspec_wp_kses_lexical::normalize_entities(
            v,
            speedyspec_wp_kses_contracts::EntityContext::Html,
            speedyspec_wp_kses_lexical::default_html_entity_names(),
            speedyspec_wp_kses_lexical::default_xml_entity_names(),
        );
        let mut out = Vec::with_capacity(normalized.len());
        for b in normalized.iter() {
            match b {
                b'<' => out.extend_from_slice(b"&lt;"),
                b'>' => out.extend_from_slice(b"&gt;"),
                b'"' => out.extend_from_slice(b"&quot;"),
                b'\'' => out.extend_from_slice(b"&#039;"),
                b => out.push(*b),
            }
        }
        Ok(out)
    }
}

pub struct Policy(pub bool);
impl UnfilteredHtmlPolicy for Policy {
    fn can_unfiltered_html(&self) -> Result<bool, KsesError> {
        Ok(self.0)
    }
}

/// The oracle's `wp_upload_dir()` URL.
pub struct Uploads;
impl UploadsLocation for Uploads {
    fn upload_url(&self) -> Result<Vec<u8>, KsesError> {
        Ok(b"http://example.org:8080/wp-content/uploads".to_vec())
    }
}

pub fn setup() -> (Rc<HookApi>, KsesApi) {
    let hooks = Rc::new(HookApi::new(
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
    });
    (hooks, api)
}

pub fn did_filter(hooks: &HookApi, name: &str) -> i64 {
    hooks.did_filter(&HookName::from(name)).unwrap()
}

pub fn filter(hooks: &HookApi, name: &str, run: impl Fn(&[HookValue]) -> HookValue + 'static) {
    hooks
        .add_filter(
            &HookName::from(name),
            Callback::rust(name, move |a: &[HookValue]| Ok(run(a))),
            &HookValue::Int(10),
            4,
        )
        .unwrap();
}
