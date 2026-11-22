//! `EscapeApi`'s own behaviour: its filters' arguments, the cached charset, other charsets.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use speedyspec_wp_hooks::{Callback, HookApi, HookName, HookValue, IgnoreDeprecations};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_text_contracts::{SiteSettings, TextError, WordCountType};
use speedyspec_wp_text_escape::EscapeApi;
use speedyspec_wp_text_memory::{FixedSiteSettings, RecordingDiagnostics};

fn api_with(settings: Rc<dyn SiteSettings>) -> (Rc<HookApi>, EscapeApi) {
    let hooks = Rc::new(HookApi::new(Rc::new(MemoryHookStore::new()), Rc::new(IgnoreDeprecations)));
    let api = EscapeApi::new(hooks.clone(), settings, Rc::new(RecordingDiagnostics::default()));
    (hooks, api)
}

fn recording(hooks: &HookApi, hook: &str, result: Option<HookValue>) -> Rc<RefCell<Vec<Vec<HookValue>>>> {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let kept = seen.clone();
    hooks
        .add_filter(
            &HookName::from(hook),
            Callback::rust(hook, move |args: &[HookValue]| {
                kept.borrow_mut().push(args.to_vec());
                Ok(result.clone().unwrap_or_else(|| args[0].clone()))
            }),
            &HookValue::Int(10),
            2,
        )
        .unwrap();
    seen
}

fn s(text: &str) -> HookValue {
    HookValue::Str(text.into())
}

#[test]
fn esc_html_fires_its_filter() {
    let (hooks, api) = api_with(Rc::new(FixedSiteSettings::default()));
    let seen = recording(&hooks, "esc_html", Some(HookValue::Int(7)));
    assert_eq!(api.esc_html(b"<a>").unwrap(), HookValue::Int(7));
    assert_eq!(*seen.borrow(), vec![vec![s("&lt;a&gt;"), s("<a>")]]);
}

#[test]
fn empty_input_still_fires() {
    let (hooks, api) = api_with(Rc::new(FixedSiteSettings::default()));
    let seen = recording(&hooks, "attribute_escape", None);
    assert_eq!(api.esc_attr(b"").unwrap(), s(""));
    assert_eq!(*seen.borrow(), vec![vec![s(""), s("")]]);
}

/// A charset that changes after the first read.
struct Changing(Cell<u32>);

impl SiteSettings for Changing {
    fn blog_charset(&self) -> Result<String, TextError> {
        self.0.set(self.0.get() + 1);
        Ok(if self.0.get() == 1 { "UTF-8" } else { "ISO-8859-1" }.into())
    }
    fn use_smilies(&self) -> Result<bool, TextError> {
        Ok(true)
    }
    fn use_balance_tags(&self) -> Result<bool, TextError> {
        Ok(false)
    }
    fn word_count_type(&self) -> Result<WordCountType, TextError> {
        Ok(WordCountType::Words)
    }
}

#[test]
fn charset_is_read_once() {
    let (_, api) = api_with(Rc::new(Changing(Cell::new(0))));
    assert_eq!(api.check_invalid_utf8(b"a\xC0b", false).unwrap(), b"");
    assert_eq!(api.check_invalid_utf8(b"a\xC0b", false).unwrap(), b"");
}

#[test]
fn iso_site_keeps_bytes() {
    let settings = FixedSiteSettings { blog_charset: "ISO-8859-1".into(), ..FixedSiteSettings::default() };
    let (_, api) = api_with(Rc::new(settings));
    assert_eq!(api.esc_html(b"\xE9<").unwrap(), HookValue::Bytes(b"\xE9&lt;".to_vec()));
}

#[test]
fn escape_new_request_rereads_charset() {
    let (_, api) = api_with(Rc::new(Changing(Cell::new(0))));
    assert_eq!(api.check_invalid_utf8(b"a\xC0b", false).unwrap(), b"");
    api.new_request();
    assert_eq!(api.check_invalid_utf8(b"a\xC0b", false).unwrap(), b"a\xC0b");
}

#[test]
fn text_error_keeps_the_payload() {
    use speedyspec_wp_hooks::HookError;
    let error = speedyspec_wp_text_escape::text_error(HookError { message: "x".into(), payload: Some(Rc::new(7u8)) });
    assert_eq!(error.payload.and_then(|p| p.downcast_ref::<u8>().copied()), Some(7));
}
