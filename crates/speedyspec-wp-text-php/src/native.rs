//! The native functions: one per ported WordPress formatting function, registered under WordPress's own name.
//!
//! Each checks its arity as PHP checks a user function's, reads its arguments as the stock body does, and dispatches
//! its use case through the handler bound in the container (resolved once per request thread), so a binding can
//! replace it. Functions with no filters, settings or reports call their Rust cores directly.

use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{BusError, Query, QueryRef};
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::native::{self, NativeError};
use speedyspec_php_ext::{Key, Value};
use speedyspec_wp_kses_contracts::Protocols;
use speedyspec_wp_text::helpers;
use speedyspec_wp_text::links::split_str_by_whitespace;
use speedyspec_wp_text::messages::*;
use speedyspec_wp_text::sanitize::{maybe_hash_hex_color, sanitize_hex_color, sanitize_hex_color_no_hash, sanitize_sql_orderby};
use speedyspec_wp_text::strings::bytes_value;
use speedyspec_wp_text::title::{SanitizeContext, sanitize_title_with_dashes};
use speedyspec_wp_text::{convert_invalid_entities, force_balance_tags, normalize_whitespace, wp_replace_in_html_tags};
use speedyspec_wp_text_contracts::TextError;
use speedyspec_wp_text_escape::messages::*;
use speedyspec_wp_text_escape::{QuoteStyle, backslashit, htmlentities2, wp_slash_value, wp_specialchars_decode, wp_unslash_value};

use crate::args::{arg, bool_or, int_or, native_error, required, string, string_or, stringable};

type R = Result<Value, NativeError>;

/// A native's body: its converted arguments to its PHP return value.
pub(crate) type Native = fn(&[Value]) -> R;

/// The namespace of natives that a PHP declaration wraps (only `wp_parse_str`, for its by-reference parameter).
pub(crate) const NATIVE: &str = "SpeedySpec\\WpText\\Native\\";

/// What the natives share on the request thread: the container, and each use case's resolved handler.
pub(crate) struct Shared {
    pub container: Rc<Container>,
    pub api: Rc<speedyspec_wp_text::TextApi>,
    refs: RefCell<HashMap<TypeId, Rc<dyn Any>>>,
}

impl Shared {
    pub(crate) fn new(container: Rc<Container>, api: Rc<speedyspec_wp_text::TextApi>) -> Shared {
        Shared { container, api, refs: RefCell::new(HashMap::new()) }
    }
}

thread_local! {
    pub(crate) static SHARED: RefCell<Option<Rc<Shared>>> = const { RefCell::new(None) };
}

fn shared() -> Result<Rc<Shared>, NativeError> {
    SHARED.with(|s| s.borrow().clone()).ok_or_else(|| NativeError::throw("Error", "the formatting API is not available"))
}

/// The formatting API, for the fast path.
pub(crate) fn shared_api() -> Option<Rc<speedyspec_wp_text::TextApi>> {
    SHARED.with(|s| s.borrow().as_ref().map(|s| s.api.clone()))
}

fn bus(e: BusError) -> NativeError {
    NativeError::throw("Error", e.to_string())
}

/// Asks a use case through its bound handler; no borrow is held while the handler runs, since it may re-enter PHP.
fn ask<Q: Query<Result = Result<T, TextError>> + 'static, T>(q: Q) -> Result<T, NativeError> {
    let shared = shared()?;
    let handler: Rc<QueryRef<Q>> = {
        let mut refs = shared.refs.borrow_mut();
        let entry = refs
            .entry(TypeId::of::<Q>())
            .or_insert_with(|| Rc::new(QueryRef::<Q>::new(shared.container.clone())) as Rc<dyn Any>)
            .clone();
        entry.downcast::<QueryRef<Q>>().expect("one QueryRef per message type")
    };
    handler.call(q).map_err(bus)?.map_err(native_error)
}

fn bytes(v: Vec<u8>) -> R {
    Ok(bytes_value(v))
}

/// A PHP list of strings.
fn list<'a>(items: impl IntoIterator<Item = &'a [u8]>) -> Value {
    Value::Array(items.into_iter().enumerate().map(|(i, b)| (Key::Int(i as i64), bytes_value(b.to_vec()))).collect())
}

/// `(array) $value` as strings, in order.
fn strings_of(value: &Value) -> Vec<Vec<u8>> {
    match value {
        Value::Array(items) => {
            items.iter().filter_map(|(_, v)| speedyspec_wp_text::strings::to_php_string(&v.read())).collect()
        }
        Value::Null => Vec::new(),
        scalar => speedyspec_wp_text::strings::to_php_string(scalar).into_iter().collect(),
    }
}

/// An optional string argument: `null` (or missing) is `None`.
fn optional(args: &[Value], i: usize) -> Result<Option<Vec<u8>>, NativeError> {
    match arg(args, i) {
        Value::Null => Ok(None),
        _ => string(args, i).map(Some),
    }
}

/// `$matches[ $i ]` of a callback's matches array.
fn matched(args: &[Value], i: usize) -> Vec<u8> {
    match arg(args, 0) {
        Value::Array(items) => items
            .iter()
            .find(|(k, _)| *k == Key::Int(i as i64))
            .and_then(|(_, v)| speedyspec_wp_text::strings::to_php_string(&v.read()))
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// `$protocols` when the caller passed an array (`is_array()`); anything else means `wp_allowed_protocols()`.
fn protocols(args: &[Value], i: usize) -> Option<Protocols> {
    match arg(args, i) {
        value @ Value::Array(_) => Some(Protocols(strings_of(&value).into_iter().map(Vec::into_boxed_slice).collect())),
        _ => None,
    }
}

/// Whether a value holds an object anywhere (PHP's `map_deep()` walks its properties, which Rust can't).
fn holds_object(value: &Value) -> bool {
    match value {
        Value::Opaque(_) => true,
        Value::Array(items) => items.iter().any(|(_, v)| holds_object(&v.read())),
        _ => false,
    }
}

// ---------------------------------------------------------------------------------------------------------------
// html-split and escaping.

fn wp_html_split(a: &[Value]) -> R {
    let input = string(a, 0)?;
    Ok(list(speedyspec_wp_html_split::wp_html_split(&input)))
}

fn get_html_split_regex(_: &[Value]) -> R {
    Ok(Value::Str(speedyspec_wp_html_split::HTML_SPLIT_REGEX.to_string()))
}

fn wp_specialchars(a: &[Value]) -> R {
    let text = string(a, 0)?;
    if text.is_empty() {
        return Ok(Value::Str(String::new()));
    }
    // A falsy `$charset` means `blog_charset`.
    let charset = if bool_or(a, 2, false) { Some(String::from_utf8_lossy(&string(a, 2)?).into_owned()) } else { None };
    let style = QuoteStyle::for_specialchars(&arg(a, 1));
    bytes(ask(WpSpecialchars { text, style, charset, double_encode: bool_or(a, 3, false) })?)
}

fn wp_specialchars_decode_native(a: &[Value]) -> R {
    let text = string(a, 0)?;
    if text.is_empty() {
        return Ok(Value::Str(String::new()));
    }
    bytes(wp_specialchars_decode(&text, QuoteStyle::for_decode(&arg(a, 1))).into_owned())
}

fn wp_check_invalid_utf8(a: &[Value]) -> R {
    bytes(ask(WpCheckInvalidUtf8 { text: string(a, 0)?, strip: bool_or(a, 1, false) })?)
}

fn seems_utf8(a: &[Value]) -> R {
    Ok(Value::Bool(ask(SeemsUtf8 { text: string(a, 0)? })?))
}

fn esc_html(a: &[Value]) -> R {
    ask(EscHtml { text: string(a, 0)? })
}

fn esc_attr(a: &[Value]) -> R {
    ask(EscAttr { text: string(a, 0)? })
}

fn esc_textarea(a: &[Value]) -> R {
    ask(EscTextarea { text: string(a, 0)? })
}

fn esc_xml(a: &[Value]) -> R {
    ask(EscXml { text: string(a, 0)? })
}

fn esc_js(a: &[Value]) -> R {
    ask(EscJs { text: string(a, 0)? })
}

fn htmlentities2_native(a: &[Value]) -> R {
    bytes(htmlentities2(&string(a, 0)?))
}

fn wp_slash(a: &[Value]) -> R {
    Ok(wp_slash_value(&arg(a, 0)))
}

fn wp_unslash(a: &[Value]) -> R {
    let value = arg(a, 0);
    if holds_object(&value) {
        // stripslashes_deep() stays PHP: it walks object properties.
        return speedyspec_php::call_catching(&Value::Str("stripslashes_deep".into()), &[value])
            .map_err(|f| native_error(crate::adapters::call_error(f)));
    }
    Ok(wp_unslash_value(&value))
}

fn backslashit_native(a: &[Value]) -> R {
    bytes(backslashit(&string(a, 0)?))
}

fn tag_escape(a: &[Value]) -> R {
    ask(TagEscape { text: string(a, 0)? })
}

// ---------------------------------------------------------------------------------------------------------------
// Content filters.

fn wptexturize(a: &[Value]) -> R {
    ask(Wptexturize { text: arg(a, 0), reset: bool_or(a, 1, false) })
}

fn wpautop(a: &[Value]) -> R {
    bytes(ask(Wpautop { text: string(a, 0)?, br: bool_or(a, 1, true) })?)
}

fn wp_replace_in_html_tags_native(a: &[Value]) -> R {
    let haystack = string(a, 0)?;
    let pairs: Vec<(Vec<u8>, Vec<u8>)> = match arg(a, 1) {
        Value::Array(items) => items
            .iter()
            .map(|(k, v)| {
                let key = match k {
                    Key::Int(i) => i.to_string().into_bytes(),
                    Key::Str(s) => s.as_bytes().to_vec(),
                };
                (key, speedyspec_wp_text::strings::to_php_string(&v.read()).unwrap_or_default())
            })
            .collect(),
        _ => Vec::new(),
    };
    let pairs: Vec<(&[u8], &[u8])> = pairs.iter().map(|(k, v)| (k.as_slice(), v.as_slice())).collect();
    bytes(wp_replace_in_html_tags(&haystack, &pairs).into_owned())
}

fn shortcode_unautop(a: &[Value]) -> R {
    bytes(ask(ShortcodeUnautop { text: string(a, 0)? })?)
}

fn convert_chars(a: &[Value]) -> R {
    let deprecated = if a.len() > 1 { arg(a, 1) } else { Value::Str(String::new()) };
    bytes(ask(ConvertChars { content: string(a, 0)?, deprecated })?)
}

fn convert_invalid_entities_native(a: &[Value]) -> R {
    bytes(convert_invalid_entities(&string(a, 0)?).into_owned())
}

fn ent2ncr(a: &[Value]) -> R {
    ask(Ent2ncr { text: string(a, 0)? })
}

fn balance_tags(a: &[Value]) -> R {
    bytes(ask(BalanceTags { text: string(a, 0)?, force: bool_or(a, 1, false) })?)
}

fn force_balance_tags_native(a: &[Value]) -> R {
    bytes(force_balance_tags(&string(a, 0)?).into_owned())
}

fn capital_p_dangit(a: &[Value]) -> R {
    bytes(ask(CapitalPDangit { text: string(a, 0)? })?)
}

fn wp_trim_words(a: &[Value]) -> R {
    ask(WpTrimWords { text: string(a, 0)?, num_words: int_or(a, 1, 55), more: optional(a, 2)? })
}

fn wp_html_excerpt(a: &[Value]) -> R {
    bytes(ask(WpHtmlExcerpt { text: string(a, 0)?, count: int_or(a, 1, 0), more: optional(a, 2)? })?)
}

fn normalize_whitespace_native(a: &[Value]) -> R {
    bytes(normalize_whitespace(&string(a, 0)?).into_owned())
}

fn wp_strip_all_tags(a: &[Value]) -> R {
    bytes(ask(WpStripAllTags { text: stringable(a, 0)?, remove_breaks: bool_or(a, 1, false) })?)
}

fn format_for_editor(a: &[Value]) -> R {
    ask(FormatForEditor { text: string(a, 0)?, default_editor: arg(a, 1) })
}

fn wp_spaces_regexp(_: &[Value]) -> R {
    bytes(ask(WpSpacesRegexp {})?)
}

fn convert_smilies(a: &[Value]) -> R {
    bytes(ask(ConvertSmilies { text: string(a, 0)? })?)
}

fn translate_smiley(a: &[Value]) -> R {
    // `reset( $matches )`: the first element, whatever its key; an empty array gives "".
    let first = match arg(a, 0) {
        Value::Array(items) if !items.is_empty() => {
            speedyspec_wp_text::strings::to_php_string(&items[0].1.read()).unwrap_or_default()
        }
        Value::Array(_) => return Ok(Value::Str(String::new())),
        _ => Vec::new(),
    };
    bytes(ask(TranslateSmiley { matched: first })?)
}

fn make_clickable(a: &[Value]) -> R {
    bytes(ask(MakeClickable { text: string(a, 0)? })?)
}

fn split_str_by_whitespace_native(a: &[Value]) -> R {
    let text = string(a, 0)?;
    let goal = usize::try_from(int_or(a, 1, 0)).unwrap_or(0);
    Ok(list(split_str_by_whitespace(&text, goal)))
}

fn wp_rel_callback(a: &[Value]) -> R {
    bytes(ask(WpRelCallback { inner: matched(a, 1), rel: string(a, 1)? })?)
}

fn wp_rel_nofollow(a: &[Value]) -> R {
    bytes(ask(WpRelNofollow { text: string(a, 0)? })?)
}

fn wp_rel_nofollow_callback(a: &[Value]) -> R {
    bytes(ask(WpRelNofollowCallback { inner: matched(a, 1) })?)
}

fn wp_rel_ugc(a: &[Value]) -> R {
    bytes(ask(WpRelUgc { text: string(a, 0)? })?)
}

fn wp_targeted_link_rel(a: &[Value]) -> R {
    bytes(ask(WpTargetedLinkRel { text: string(a, 0)? })?)
}

fn wp_targeted_link_rel_callback(a: &[Value]) -> R {
    bytes(ask(WpTargetedLinkRelCallback { link_html: matched(a, 1) })?)
}

fn links_add_base_url(a: &[Value]) -> R {
    let attrs = if a.len() > 2 { strings_of(&arg(a, 2)) } else { vec![b"src".to_vec(), b"href".to_vec()] };
    bytes(ask(LinksAddBaseUrl { content: string(a, 0)?, base: string(a, 1)?, attrs })?)
}

fn links_add_target(a: &[Value]) -> R {
    let tags = if a.len() > 2 { strings_of(&arg(a, 2)) } else { vec![b"a".to_vec()] };
    bytes(ask(LinksAddTarget { content: string(a, 0)?, target: string_or(a, 1, b"_blank")?, tags })?)
}

fn format_to_edit(a: &[Value]) -> R {
    ask(FormatToEdit { content: string(a, 0)?, rich_text: bool_or(a, 1, false) })
}

fn wp_pre_kses_less_than(a: &[Value]) -> R {
    bytes(ask(WpPreKsesLessThan { content: string(a, 0)? })?)
}

// ---------------------------------------------------------------------------------------------------------------
// Sanitizers and URLs.

fn sanitize_title(a: &[Value]) -> R {
    let context = SanitizeContext::from_bytes(&string_or(a, 2, b"save")?);
    ask(SanitizeTitle { title: arg(a, 0), fallback: string_or(a, 1, b"")?, context })
}

fn sanitize_title_for_query(a: &[Value]) -> R {
    ask(SanitizeTitleForQuery { title: arg(a, 0) })
}

fn sanitize_title_with_dashes_native(a: &[Value]) -> R {
    let context = SanitizeContext::from_bytes(&string_or(a, 2, b"display")?);
    bytes(sanitize_title_with_dashes(&string(a, 0)?, &context))
}

fn remove_accents(a: &[Value]) -> R {
    let locale = string_or(a, 1, b"")?;
    let locale = (!locale.is_empty()).then(|| String::from_utf8_lossy(&locale).into_owned());
    bytes(ask(RemoveAccents { text: string(a, 0)?, locale })?)
}

fn utf8_uri_encode(a: &[Value]) -> R {
    let length = usize::try_from(int_or(a, 1, 0)).unwrap_or(0);
    bytes(helpers::utf8_uri_encode(&string(a, 0)?, length, bool_or(a, 2, false)))
}

fn sanitize_file_name(a: &[Value]) -> R {
    ask(SanitizeFileName { filename: string(a, 0)? })
}

fn sanitize_user(a: &[Value]) -> R {
    ask(SanitizeUser { username: string(a, 0)?, strict: bool_or(a, 1, false) })
}

fn sanitize_key(a: &[Value]) -> R {
    ask(SanitizeKey { key: arg(a, 0) })
}

fn sanitize_html_class(a: &[Value]) -> R {
    ask(SanitizeHtmlClass { classname: string(a, 0)?, fallback: string_or(a, 1, b"")? })
}

fn sanitize_locale_name(a: &[Value]) -> R {
    ask(SanitizeLocaleName { locale_name: string(a, 0)? })
}

fn sanitize_text_field(a: &[Value]) -> R {
    ask(SanitizeTextField { value: arg(a, 0) })
}

fn sanitize_textarea_field(a: &[Value]) -> R {
    ask(SanitizeTextareaField { value: arg(a, 0) })
}

fn sanitize_email(a: &[Value]) -> R {
    ask(SanitizeEmail { email: string(a, 0)? })
}

fn is_email(a: &[Value]) -> R {
    let deprecated = if a.len() > 1 { arg(a, 1) } else { Value::Bool(false) };
    ask(IsEmail { email: arg(a, 0), deprecated })
}

/// `''`, `null` or the colour, as the hex-colour functions return.
fn optional_color(v: Option<&[u8]>) -> R {
    Ok(v.map_or(Value::Null, |c| bytes_value(c.to_vec())))
}

fn sanitize_hex_color_native(a: &[Value]) -> R {
    optional_color(sanitize_hex_color(&string(a, 0)?))
}

fn sanitize_hex_color_no_hash_native(a: &[Value]) -> R {
    optional_color(sanitize_hex_color_no_hash(&string(a, 0)?))
}

fn maybe_hash_hex_color_native(a: &[Value]) -> R {
    bytes(maybe_hash_hex_color(&string(a, 0)?).into_owned())
}

fn sanitize_mime_type(a: &[Value]) -> R {
    ask(SanitizeMimeType { mime_type: string(a, 0)? })
}

fn sanitize_trackback_urls(a: &[Value]) -> R {
    ask(SanitizeTrackbackUrls { to_ping: string(a, 0)? })
}

fn sanitize_sql_orderby_native(a: &[Value]) -> R {
    Ok(sanitize_sql_orderby(&string(a, 0)?).map_or(Value::Bool(false), |v| bytes_value(v.to_vec())))
}

fn esc_url(a: &[Value]) -> R {
    ask(EscUrl { url: string(a, 0)?, protocols: protocols(a, 1), context: string_or(a, 2, b"display")? })
}

fn esc_url_raw(a: &[Value]) -> R {
    ask(EscUrlRaw { url: string(a, 0)?, protocols: protocols(a, 1) })
}

fn sanitize_url(a: &[Value]) -> R {
    ask(SanitizeUrl { url: string(a, 0)?, protocols: protocols(a, 1) })
}

fn deep_replace(a: &[Value]) -> R {
    let search = strings_of(&arg(a, 0));
    let search: Vec<&[u8]> = search.iter().map(Vec::as_slice).collect();
    bytes(helpers::deep_replace(&search, &string(a, 1)?))
}

fn wp_make_link_relative(a: &[Value]) -> R {
    bytes(helpers::wp_make_link_relative(&string(a, 0)?).into_owned())
}

fn url_shorten(a: &[Value]) -> R {
    bytes(helpers::url_shorten(&string(a, 0)?, int_or(a, 1, 35)))
}

fn antispambot(a: &[Value]) -> R {
    bytes(ask(Antispambot { email: string(a, 0)?, hex_encoding: int_or(a, 1, 0) })?)
}

// ---------------------------------------------------------------------------------------------------------------
// Helpers.

fn zeroise(a: &[Value]) -> R {
    bytes(helpers::zeroise(&string(a, 0)?, int_or(a, 1, 0)))
}

fn trailingslashit(a: &[Value]) -> R {
    bytes(helpers::trailingslashit(&string(a, 0)?))
}

fn untrailingslashit(a: &[Value]) -> R {
    bytes(helpers::untrailingslashit(&string(a, 0)?).to_vec())
}

fn wp_basename(a: &[Value]) -> R {
    bytes(helpers::wp_basename(&string(a, 0)?, &string_or(a, 1, b"")?))
}

fn wp_iso_descrambler(a: &[Value]) -> R {
    bytes(helpers::wp_iso_descrambler(&string(a, 0)?).into_owned())
}

fn wp_parse_str(a: &[Value]) -> R {
    ask(WpParseStr { input: stringable(a, 0)? })
}

/// Every native: (WordPress name, required arguments, all arguments, body).
pub(crate) fn entries() -> Vec<(&'static str, usize, usize, Native)> {
    vec![
        ("wp_html_split", 1, 1, wp_html_split as Native),
        ("get_html_split_regex", 0, 0, get_html_split_regex),
        ("_wp_specialchars", 1, 4, wp_specialchars),
        ("wp_specialchars_decode", 1, 2, wp_specialchars_decode_native),
        ("wp_check_invalid_utf8", 1, 2, wp_check_invalid_utf8),
        ("seems_utf8", 1, 1, seems_utf8),
        ("esc_html", 1, 1, esc_html),
        ("esc_attr", 1, 1, esc_attr),
        ("esc_textarea", 1, 1, esc_textarea),
        ("esc_xml", 1, 1, esc_xml),
        ("esc_js", 1, 1, esc_js),
        ("htmlentities2", 1, 1, htmlentities2_native),
        ("wp_slash", 1, 1, wp_slash),
        ("wp_unslash", 1, 1, wp_unslash),
        ("backslashit", 1, 1, backslashit_native),
        ("tag_escape", 1, 1, tag_escape),
        ("wptexturize", 1, 2, wptexturize),
        ("wpautop", 1, 2, wpautop),
        ("wp_replace_in_html_tags", 2, 2, wp_replace_in_html_tags_native),
        ("shortcode_unautop", 1, 1, shortcode_unautop),
        ("convert_chars", 1, 2, convert_chars),
        ("convert_invalid_entities", 1, 1, convert_invalid_entities_native),
        ("ent2ncr", 1, 1, ent2ncr),
        ("balanceTags", 1, 2, balance_tags),
        ("force_balance_tags", 1, 1, force_balance_tags_native),
        ("capital_P_dangit", 1, 1, capital_p_dangit),
        ("wp_trim_words", 1, 3, wp_trim_words),
        ("wp_html_excerpt", 2, 3, wp_html_excerpt),
        ("normalize_whitespace", 1, 1, normalize_whitespace_native),
        ("wp_strip_all_tags", 1, 2, wp_strip_all_tags),
        ("format_for_editor", 1, 2, format_for_editor),
        ("wp_spaces_regexp", 0, 0, wp_spaces_regexp),
        ("convert_smilies", 1, 1, convert_smilies),
        ("translate_smiley", 1, 1, translate_smiley),
        ("make_clickable", 1, 1, make_clickable),
        ("_split_str_by_whitespace", 2, 2, split_str_by_whitespace_native),
        ("wp_rel_callback", 2, 2, wp_rel_callback),
        ("wp_rel_nofollow", 1, 1, wp_rel_nofollow),
        ("wp_rel_nofollow_callback", 1, 1, wp_rel_nofollow_callback),
        ("wp_rel_ugc", 1, 1, wp_rel_ugc),
        ("wp_targeted_link_rel", 1, 1, wp_targeted_link_rel),
        ("wp_targeted_link_rel_callback", 1, 1, wp_targeted_link_rel_callback),
        ("links_add_base_url", 2, 3, links_add_base_url),
        ("links_add_target", 1, 3, links_add_target),
        ("format_to_edit", 1, 2, format_to_edit),
        ("wp_pre_kses_less_than", 1, 1, wp_pre_kses_less_than),
        ("sanitize_title", 1, 3, sanitize_title),
        ("sanitize_title_for_query", 1, 1, sanitize_title_for_query),
        ("sanitize_title_with_dashes", 1, 3, sanitize_title_with_dashes_native),
        ("remove_accents", 1, 2, remove_accents),
        ("utf8_uri_encode", 1, 3, utf8_uri_encode),
        ("sanitize_file_name", 1, 1, sanitize_file_name),
        ("sanitize_user", 1, 2, sanitize_user),
        ("sanitize_key", 1, 1, sanitize_key),
        ("sanitize_html_class", 1, 2, sanitize_html_class),
        ("sanitize_locale_name", 1, 1, sanitize_locale_name),
        ("sanitize_text_field", 1, 1, sanitize_text_field),
        ("sanitize_textarea_field", 1, 1, sanitize_textarea_field),
        ("sanitize_email", 1, 1, sanitize_email),
        ("is_email", 1, 2, is_email),
        ("sanitize_hex_color", 1, 1, sanitize_hex_color_native),
        ("sanitize_hex_color_no_hash", 1, 1, sanitize_hex_color_no_hash_native),
        ("maybe_hash_hex_color", 1, 1, maybe_hash_hex_color_native),
        ("sanitize_mime_type", 1, 1, sanitize_mime_type),
        ("sanitize_trackback_urls", 1, 1, sanitize_trackback_urls),
        ("sanitize_sql_orderby", 1, 1, sanitize_sql_orderby_native),
        ("esc_url", 1, 3, esc_url),
        ("esc_url_raw", 1, 2, esc_url_raw),
        ("sanitize_url", 1, 2, sanitize_url),
        ("_deep_replace", 2, 2, deep_replace),
        ("wp_make_link_relative", 1, 1, wp_make_link_relative),
        ("url_shorten", 1, 2, url_shorten),
        ("antispambot", 1, 2, antispambot),
        ("zeroise", 2, 2, zeroise),
        ("trailingslashit", 1, 1, trailingslashit),
        ("untrailingslashit", 1, 1, untrailingslashit),
        ("wp_basename", 1, 2, wp_basename),
        ("wp_iso_descrambler", 1, 1, wp_iso_descrambler),
    ]
}

/// The natives that a PHP declaration in `formatting.php` (this crate's) wraps, under [`NATIVE`].
fn wrapped() -> Vec<(&'static str, usize, usize, Native)> {
    vec![("wp_parse_str", 1, 1, wp_parse_str as Native)]
}

/// The PHP functions to register: the fast ones under WordPress's names with their full natives under
/// [`crate::fast::SLOW`], every other under WordPress's name.
pub(crate) fn functions() -> Vec<FunctionBuilder<'static>> {
    let direct = entries().into_iter().map(|(name, req, max, f)| {
        let php_name =
            if crate::fast::FAST.contains(&name) { format!("{}{name}", crate::fast::SLOW) } else { name.to_string() };
        (php_name, name, req, max, f)
    });
    let wrapped = wrapped().into_iter().map(|(name, req, max, f)| (format!("{NATIVE}{name}"), name, req, max, f));
    direct
        .chain(wrapped)
        .map(|(php_name, name, req, max, f)| {
            native::function(&php_name, move |args: &[Value]| {
                required(name, args, req, max)?;
                f(args)
            })
        })
        .chain(crate::fast::functions())
        .collect()
}
