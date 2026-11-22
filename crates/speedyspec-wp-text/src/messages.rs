//! Every `TextApi` function as a query on the bus (Command / Query), bound in the container, as kses does.

use std::borrow::Cow;
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{Query, QueryHandler, bind_query_handler};
use speedyspec_hooks::Value as HookValue;
use speedyspec_wp_kses_contracts::Protocols;
use speedyspec_wp_text_contracts::TextError;

use crate::api::TextApi;
use crate::title::SanitizeContext;

macro_rules! message {
    ($name:ident { $($field:ident : $ty:ty),* } -> $result:ty, |$a:ident, $m:ident| $body:expr) => {
        #[doc = concat!("The `", stringify!($name), "` use case: one formatting function as a query, so a container binding can")]
        #[doc = "replace its handler."]
        #[derive(Clone, Debug)]
        pub struct $name { $(pub $field: $ty),* }
        impl Query for $name {
            type Result = $result;
            const KEY: &'static str = concat!("SpeedySpec\\WpText\\", stringify!($name), "Handler");
        }
        impl QueryHandler<$name> for TextApi {
            #[allow(unused_variables)]
            fn handle(&self, $m: $name) -> $result {
                let $a = self;
                $body
            }
        }
    };
}

type R<T> = Result<T, TextError>;

message!(WpSpacesRegexp {} -> R<Vec<u8>>, |a, m| a.wp_spaces_regexp());
message!(Wptexturize { text: HookValue, reset: bool } -> R<HookValue>, |a, m| a.wptexturize(&m.text, m.reset));
message!(Wpautop { text: Vec<u8>, br: bool } -> R<Vec<u8>>, |a, m| a.wpautop(&m.text, m.br).map(Cow::into_owned));
message!(ShortcodeUnautop { text: Vec<u8> } -> R<Vec<u8>>, |a, m| a.shortcode_unautop(&m.text).map(Cow::into_owned));
message!(CapitalPDangit { text: Vec<u8> } -> R<Vec<u8>>, |a, m| a.capital_p_dangit(&m.text).map(Cow::into_owned));
message!(ConvertChars { content: Vec<u8>, deprecated: HookValue } -> R<Vec<u8>>,
    |a, m| a.convert_chars(&m.content, &m.deprecated).map(Cow::into_owned));
message!(BalanceTags { text: Vec<u8>, force: bool } -> R<Vec<u8>>, |a, m| a.balance_tags(&m.text, m.force).map(Cow::into_owned));
message!(Ent2ncr { text: Vec<u8> } -> R<HookValue>, |a, m| a.ent2ncr(&m.text));
message!(WpStripAllTags { text: HookValue, remove_breaks: bool } -> R<Vec<u8>>, |a, m| a.wp_strip_all_tags(&m.text, m.remove_breaks));
message!(WpTrimWords { text: Vec<u8>, num_words: i64, more: Option<Vec<u8>> } -> R<HookValue>,
    |a, m| a.wp_trim_words(&m.text, m.num_words, m.more.as_deref()));
message!(WpHtmlExcerpt { text: Vec<u8>, count: i64, more: Option<Vec<u8>> } -> R<Vec<u8>>,
    |a, m| a.wp_html_excerpt(&m.text, m.count, m.more.as_deref()));
message!(FormatForEditor { text: Vec<u8>, default_editor: HookValue } -> R<HookValue>,
    |a, m| a.format_for_editor(&m.text, &m.default_editor));
message!(SanitizeKey { key: HookValue } -> R<HookValue>, |a, m| a.sanitize_key(&m.key));
message!(SanitizeHtmlClass { classname: Vec<u8>, fallback: Vec<u8> } -> R<HookValue>,
    |a, m| a.sanitize_html_class(&m.classname, &m.fallback));
message!(SanitizeLocaleName { locale_name: Vec<u8> } -> R<HookValue>, |a, m| a.sanitize_locale_name(&m.locale_name));
message!(SanitizeMimeType { mime_type: Vec<u8> } -> R<HookValue>, |a, m| a.sanitize_mime_type(&m.mime_type));
message!(RemoveAccents { text: Vec<u8>, locale: Option<String> } -> R<Vec<u8>>,
    |a, m| a.remove_accents(&m.text, m.locale.as_deref()).map(Cow::into_owned));
message!(SanitizeTitle { title: HookValue, fallback: Vec<u8>, context: SanitizeContext } -> R<HookValue>,
    |a, m| a.sanitize_title(&m.title, &m.fallback, &m.context));
message!(SanitizeTitleForQuery { title: HookValue } -> R<HookValue>, |a, m| a.sanitize_title_for_query(&m.title));
message!(SanitizeUser { username: Vec<u8>, strict: bool } -> R<HookValue>, |a, m| a.sanitize_user(&m.username, m.strict));
message!(SanitizeFileName { filename: Vec<u8> } -> R<HookValue>, |a, m| a.sanitize_file_name(&m.filename));
message!(Antispambot { email: Vec<u8>, hex_encoding: i64 } -> R<Vec<u8>>, |a, m| a.antispambot(&m.email, m.hex_encoding));
message!(EscUrl { url: Vec<u8>, protocols: Option<Protocols>, context: Vec<u8> } -> R<HookValue>,
    |a, m| a.esc_url(&m.url, m.protocols.as_ref(), &m.context));
message!(EscUrlRaw { url: Vec<u8>, protocols: Option<Protocols> } -> R<HookValue>, |a, m| a.esc_url_raw(&m.url, m.protocols.as_ref()));
message!(SanitizeUrl { url: Vec<u8>, protocols: Option<Protocols> } -> R<HookValue>, |a, m| a.sanitize_url(&m.url, m.protocols.as_ref()));
message!(WpPreKsesLessThan { content: Vec<u8> } -> R<Vec<u8>>, |a, m| a.wp_pre_kses_less_than(&m.content));
message!(SanitizeTextField { value: HookValue } -> R<HookValue>, |a, m| a.sanitize_text_field(&m.value));
message!(SanitizeTextareaField { value: HookValue } -> R<HookValue>, |a, m| a.sanitize_textarea_field(&m.value));
message!(FormatToEdit { content: Vec<u8>, rich_text: bool } -> R<HookValue>, |a, m| a.format_to_edit(&m.content, m.rich_text));
message!(IsEmail { email: HookValue, deprecated: HookValue } -> R<HookValue>, |a, m| a.is_email(&m.email, &m.deprecated));
message!(SanitizeEmail { email: Vec<u8> } -> R<HookValue>, |a, m| a.sanitize_email(&m.email));
message!(WpParseStr { input: HookValue } -> R<HookValue>, |a, m| a.wp_parse_str(&m.input));
message!(SanitizeTrackbackUrls { to_ping: Vec<u8> } -> R<HookValue>, |a, m| a.sanitize_trackback_urls(&m.to_ping));
message!(MakeClickable { text: Vec<u8> } -> R<Vec<u8>>, |a, m| a.make_clickable(&m.text));
message!(WpRelCallback { inner: Vec<u8>, rel: Vec<u8> } -> R<Vec<u8>>, |a, m| a.wp_rel_callback(&m.inner, &m.rel));
message!(WpRelNofollowCallback { inner: Vec<u8> } -> R<Vec<u8>>, |a, m| a.wp_rel_nofollow_callback(&m.inner));
message!(WpRelNofollow { text: Vec<u8> } -> R<Vec<u8>>, |a, m| a.wp_rel_nofollow(&m.text));
message!(WpRelUgc { text: Vec<u8> } -> R<Vec<u8>>, |a, m| a.wp_rel_ugc(&m.text));
message!(WpTargetedLinkRelCallback { link_html: Vec<u8> } -> R<Vec<u8>>, |a, m| a.wp_targeted_link_rel_callback(&m.link_html));
message!(WpTargetedLinkRel { text: Vec<u8> } -> R<Vec<u8>>, |a, m| a.wp_targeted_link_rel(&m.text));
message!(LinksAddBaseUrl { content: Vec<u8>, base: Vec<u8>, attrs: Vec<Vec<u8>> } -> R<Vec<u8>>, |a, m| {
    let attrs: Vec<&[u8]> = m.attrs.iter().map(Vec::as_slice).collect();
    a.links_add_base_url(&m.content, &m.base, &attrs)
});
message!(LinksAddTarget { content: Vec<u8>, target: Vec<u8>, tags: Vec<Vec<u8>> } -> R<Vec<u8>>, |a, m| {
    let tags: Vec<&[u8]> = m.tags.iter().map(Vec::as_slice).collect();
    a.links_add_target(&m.content, &m.target, &tags)
});
message!(TranslateSmiley { matched: Vec<u8> } -> R<Vec<u8>>, |a, m| a.translate_smiley(&m.matched));
message!(ConvertSmilies { text: Vec<u8> } -> R<Vec<u8>>, |a, m| a.convert_smilies(&m.text).map(Cow::into_owned));

macro_rules! bind_all {
    ($container:expr, $api:expr; $($q:ty),*) => {
        $( { let a = $api.clone(); bind_query_handler::<$q>($container, move |_, _| Ok(a.clone())); } )*
    };
}

/// Binds every formatting use case of `TextApi` to `api` in the container, so the PHP surface dispatches through
/// handlers PHP may replace.
///
/// @param container
///     Where to bind them.
/// @param api
///     The handler of each.
pub fn register_text_handlers(container: &Container, api: Rc<TextApi>) {
    bind_all!(container, api;
        WpSpacesRegexp, Wptexturize, Wpautop, ShortcodeUnautop, CapitalPDangit, ConvertChars, BalanceTags, Ent2ncr,
        WpStripAllTags, WpTrimWords, WpHtmlExcerpt, FormatForEditor, SanitizeKey, SanitizeHtmlClass, SanitizeLocaleName,
        SanitizeMimeType, RemoveAccents, SanitizeTitle, SanitizeTitleForQuery, SanitizeUser, SanitizeFileName, Antispambot,
        EscUrl, EscUrlRaw, SanitizeUrl, WpPreKsesLessThan, SanitizeTextField, SanitizeTextareaField, FormatToEdit, IsEmail,
        SanitizeEmail, WpParseStr, SanitizeTrackbackUrls, MakeClickable, WpRelCallback, WpRelNofollowCallback, WpRelNofollow,
        WpRelUgc, WpTargetedLinkRelCallback, WpTargetedLinkRel, LinksAddBaseUrl, LinksAddTarget, TranslateSmiley,
        ConvertSmilies);
}
