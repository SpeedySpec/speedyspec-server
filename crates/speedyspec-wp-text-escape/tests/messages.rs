//! The escaping use cases on the bus: each is bound, answers through the container, and can be replaced.

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{Query, QueryHandler, QueryRef, bind_query_handler};
use speedyspec_wp_hooks::{WpHookApi, IgnoreDeprecations};
use speedyspec_wp_hooks_contracts::HookValue;
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_text_contracts::TextError;
use speedyspec_wp_text_escape::messages::*;
use speedyspec_wp_text_escape::{EscapeApi, QuoteStyle, register_escape_handlers};
use speedyspec_wp_text_memory::{FixedSiteSettings, RecordingDiagnostics};

fn bound() -> Rc<Container> {
    let hooks = Rc::new(WpHookApi::new(Rc::new(MemoryHookStore::new()), Rc::new(IgnoreDeprecations)));
    let api = Rc::new(EscapeApi::new(hooks, Rc::new(FixedSiteSettings::default()), Rc::new(RecordingDiagnostics::default())));
    let container = Rc::new(Container::new());
    register_escape_handlers(&container, api);
    container
}

#[test]
fn every_handler_resolves() {
    let c = bound();
    let t = || b"<a>".to_vec();
    let s = |v: &str| HookValue::Str(v.into());
    assert_eq!(QueryRef::<EscHtml>::new(c.clone()).call(EscHtml { text: t() }).unwrap().unwrap(), s("&lt;a&gt;"));
    assert_eq!(QueryRef::<EscAttr>::new(c.clone()).call(EscAttr { text: t() }).unwrap().unwrap(), s("&lt;a&gt;"));
    assert_eq!(QueryRef::<EscTextarea>::new(c.clone()).call(EscTextarea { text: t() }).unwrap().unwrap(), s("&lt;a&gt;"));
    assert_eq!(QueryRef::<EscXml>::new(c.clone()).call(EscXml { text: t() }).unwrap().unwrap(), s("&lt;a&gt;"));
    assert_eq!(QueryRef::<EscJs>::new(c.clone()).call(EscJs { text: t() }).unwrap().unwrap(), s("&lt;a&gt;"));
    assert_eq!(QueryRef::<TagEscape>::new(c.clone()).call(TagEscape { text: t() }).unwrap().unwrap(), s("a"));
    assert!(QueryRef::<SeemsUtf8>::new(c.clone()).call(SeemsUtf8 { text: t() }).unwrap().unwrap());
    assert_eq!(QueryRef::<AddslashesGpc>::new(c.clone()).call(AddslashesGpc { value: s("'") }).unwrap().unwrap(), s("\\'"));
    let special = WpSpecialchars { text: t(), style: QuoteStyle::NoQuotes, charset: None, double_encode: false };
    assert_eq!(QueryRef::<WpSpecialchars>::new(c.clone()).call(special).unwrap().unwrap(), b"&lt;a&gt;");
    let check = WpCheckInvalidUtf8 { text: b"a\xC0".to_vec(), strip: true };
    assert_eq!(QueryRef::<WpCheckInvalidUtf8>::new(c).call(check).unwrap().unwrap(), "a\u{FFFD}".as_bytes());
}

/// A replacement handler, as a plugin's binding would be.
struct Shout;

impl QueryHandler<EscAttr> for Shout {
    fn handle(&self, q: EscAttr) -> Result<HookValue, TextError> {
        Ok(HookValue::Bytes(q.text.to_ascii_uppercase()))
    }
}

#[test]
fn a_bound_handler_can_be_replaced() {
    let c = bound();
    bind_query_handler::<EscAttr>(&c, |_, _| Ok(Rc::new(Shout)));
    assert_eq!(QueryRef::<EscAttr>::new(c).call(EscAttr { text: b"ab".to_vec() }).unwrap().unwrap(), HookValue::Bytes(b"AB".to_vec()));
    assert_eq!(<EscAttr as Query>::KEY, "SpeedySpec\\WpTextEscape\\EscAttrHandler");
}
