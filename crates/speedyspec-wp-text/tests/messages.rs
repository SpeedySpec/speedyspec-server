//! The formatting use cases on the bus: each is bound, answers through the container, and can be replaced.

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{Query, QueryHandler, QueryRef, bind_query_handler};
use speedyspec_wp_hooks_contracts::HookValue;
use speedyspec_wp_text::messages::*;
use speedyspec_wp_text::register_text_handlers;
use speedyspec_wp_text::title::SanitizeContext;
use speedyspec_wp_text_contracts::TextError;

#[path = "common/api.rs"]
mod api;

fn bound() -> Rc<Container> {
    let h = api::harness();
    let container = Rc::new(Container::new());
    register_text_handlers(&container, h.api.clone());
    container
}

fn s(v: &str) -> HookValue {
    HookValue::Str(v.into())
}

fn b(v: &str) -> Vec<u8> {
    v.as_bytes().to_vec()
}

/// Asks one query through the container.
fn ask<Q: Query<Result = Result<T, TextError>>, T>(c: &Rc<Container>, q: Q) -> T {
    QueryRef::<Q>::new(c.clone()).call(q).expect("bound").expect("answered")
}

#[test]
fn every_handler_resolves() {
    let c = bound();
    let none = HookValue::Null;
    assert_eq!(ask(&c, WpSpacesRegexp {}), b"[\\r\\n\\t ]|\\xC2\\xA0|&nbsp;");
    assert_eq!(ask(&c, Wptexturize { text: s("'a'"), reset: false }), s("&#8216;a&#8217;"));
    assert_eq!(ask(&c, Wpautop { text: b("a"), br: true }), b("<p>a</p>\n"));
    assert_eq!(ask(&c, ShortcodeUnautop { text: b("<p>[gallery]</p>") }), b("[gallery]"));
    assert_eq!(ask(&c, CapitalPDangit { text: b(" Wordpress") }), b(" WordPress"));
    assert_eq!(ask(&c, ConvertChars { content: b("&x"), deprecated: none.clone() }), b("&#038;x"));
    assert_eq!(ask(&c, BalanceTags { text: b("<b>"), force: true }), b("<b></b>"));
    assert_eq!(ask(&c, Ent2ncr { text: b("&nbsp;") }), s("&#160;"));
    assert_eq!(ask(&c, WpStripAllTags { text: s("<b>a</b>"), remove_breaks: false }), b("a"));
    assert_eq!(ask(&c, WpTrimWords { text: b("a b c"), num_words: 2, more: None }), s("a b&hellip;"));
    assert_eq!(ask(&c, WpHtmlExcerpt { text: b("<b>abc</b>"), count: 2, more: None }), b("ab"));
    assert_eq!(ask(&c, FormatForEditor { text: b("<"), default_editor: none.clone() }), s("&lt;"));
    assert_eq!(ask(&c, SanitizeKey { key: s("A b") }), s("ab"));
    assert_eq!(ask(&c, SanitizeHtmlClass { classname: b("a b"), fallback: b("") }), s("ab"));
    assert_eq!(ask(&c, SanitizeLocaleName { locale_name: b("en US") }), s("enUS"));
    assert_eq!(ask(&c, SanitizeMimeType { mime_type: b("text/ html") }), s("text/html"));
    assert_eq!(ask(&c, RemoveAccents { text: b("é"), locale: None }), b("e"));
    assert_eq!(ask(&c, SanitizeTitle { title: s("A"), fallback: b(""), context: SanitizeContext::Save }), s("A"));
    assert_eq!(ask(&c, SanitizeTitleForQuery { title: s("A") }), s("A"));
    assert_eq!(ask(&c, SanitizeUser { username: b("a<b>"), strict: false }), s("a"));
    assert_eq!(ask(&c, SanitizeFileName { filename: b("a b.txt") }), s("a-b.txt"));
    assert_eq!(ask(&c, Antispambot { email: b(""), hex_encoding: 0 }), b(""));
    assert_eq!(ask(&c, EscUrl { url: b("http://a.b/?c&d"), protocols: None, context: b("display") }), s("http://a.b/?c&#038;d"));
    assert_eq!(ask(&c, EscUrlRaw { url: b("http://a.b/?c&d"), protocols: None }), s("http://a.b/?c&d"));
    assert_eq!(ask(&c, SanitizeUrl { url: b("javascript:x"), protocols: None }), s(""));
    assert_eq!(ask(&c, WpPreKsesLessThan { content: b("a < b") }), b("a &lt; b"));
    assert_eq!(ask(&c, SanitizeTextField { value: s(" a  b ") }), s("a b"));
    assert_eq!(ask(&c, SanitizeTextareaField { value: s(" a\nb ") }), s("a\nb"));
    assert_eq!(ask(&c, FormatToEdit { content: b("<"), rich_text: false }), s("&lt;"));
    assert_eq!(ask(&c, IsEmail { email: s("ab@cd.ef"), deprecated: HookValue::Bool(false) }), s("ab@cd.ef"));
    assert_eq!(ask(&c, SanitizeEmail { email: b("ab@cd.ef") }), s("ab@cd.ef"));
    assert!(matches!(ask(&c, WpParseStr { input: s("a=1") }), HookValue::Array(_)));
    assert_eq!(ask(&c, SanitizeTrackbackUrls { to_ping: b("x") }), s(""));
    assert_eq!(ask(&c, MakeClickable { text: b("x") }), b("x"));
    assert_eq!(ask(&c, WpRelCallback { inner: b("href=\"/a\""), rel: b("x") }), b("<a href=\"/a\" rel=\"x\">"));
    assert_eq!(ask(&c, WpRelNofollowCallback { inner: b("href=\"/a\"") }), b("<a href=\"/a\" rel=\"nofollow\">"));
    assert_eq!(ask(&c, WpRelNofollow { text: b("x") }), b("x"));
    assert_eq!(ask(&c, WpRelUgc { text: b("x") }), b("x"));
    assert_eq!(ask(&c, WpTargetedLinkRelCallback { link_html: b("href=\"/a\"") }), b("<a href=\"/a\">"));
    assert_eq!(ask(&c, WpTargetedLinkRel { text: b("x") }), b("x"));
    assert_eq!(ask(&c, LinksAddBaseUrl { content: b("x"), base: b("http://a"), attrs: vec![b("src")] }), b("x"));
    assert_eq!(ask(&c, LinksAddTarget { content: b("<a>"), target: b("_blank"), tags: vec![b("a")] }), b("<a target=\"_blank\">"));
    assert_eq!(ask(&c, TranslateSmiley { matched: b(":nope:") }), b(""));
    assert_eq!(ask(&c, ConvertSmilies { text: b("x") }), b("x"));
}

/// A replacement handler, as a plugin's binding would be.
struct Upper;

impl QueryHandler<SanitizeKey> for Upper {
    fn handle(&self, _: SanitizeKey) -> Result<HookValue, TextError> {
        Ok(HookValue::Str("REPLACED".into()))
    }
}

#[test]
fn a_bound_handler_can_be_replaced() {
    let c = bound();
    bind_query_handler::<SanitizeKey>(&c, |_, _| Ok(Rc::new(Upper)));
    assert_eq!(ask(&c, SanitizeKey { key: s("a") }), s("REPLACED"));
    assert_eq!(<SanitizeKey as Query>::KEY, "SpeedySpec\\WpText\\SanitizeKeyHandler");
}
