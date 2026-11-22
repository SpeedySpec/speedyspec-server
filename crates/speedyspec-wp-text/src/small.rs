//! The small content filters: whitespace, ampersands, entity tables, `capital_P_dangit`, `format_for_editor`.

use std::borrow::Cow;
use std::sync::LazyLock;

use memchr::memmem;
use speedyspec_wp_text_escape::{Charset, EntFlags, htmlspecialchars};

use crate::ent2ncr_table::ENT2NCR;

use crate::strings::{PHP_TRIM_DEFAULT, StrReplace, StrtrTable, php_trim, replace_one, strtr};

/// The fragment `wp_spaces_regexp()` returns by default, before its filter: what counts as a space in
/// `wptexturize` and `shortcode_unautop`.
///
/// It is the literal backslash text PHP's single-quoted string holds; PCRE reads `\xC2\xA0` as those two bytes.
pub const DEFAULT_SPACES: &[u8] = br"[\r\n\t ]|\xC2\xA0|&nbsp;";

/// WordPress's `normalize_whitespace()`: trims, turns `\r` into `\n`, and collapses runs of newlines and of
/// spaces and tabs.
///
/// @param s
///     The text.
/// @return
///     The normalized text, borrowed (possibly trimmed) when nothing else changes.
pub fn normalize_whitespace(s: &[u8]) -> Cow<'_, [u8]> {
    let trimmed = php_trim(s, PHP_TRIM_DEFAULT);
    let needs_work = trimmed.windows(2).any(|w| (w[0] == b'\n' && w[1] == b'\n') || (matches!(w[0], b' ' | b'\t') && matches!(w[1], b' ' | b'\t')))
        || trimmed.iter().any(|&b| b == b'\r' || b == b'\t');
    if !needs_work {
        return Cow::Borrowed(trimmed);
    }
    let mut out = Vec::with_capacity(trimmed.len());
    for &b in trimmed {
        let b = if b == b'\r' { b'\n' } else { b };
        match b {
            b'\n' if out.last() == Some(&b'\n') => {}
            b' ' | b'\t' if out.last() == Some(&b' ') => {}
            b'\t' => out.push(b' '),
            other => out.push(other),
        }
    }
    Cow::Owned(out)
}

/// WordPress's `convert_chars()`: a bare `&` that doesn't start a reference becomes `&#038;`.
///
/// The deprecated second argument is the facade's to report.
///
/// @param content
///     The text.
/// @return
///     The converted text, borrowed when there is nothing to convert.
pub fn convert_chars(content: &[u8]) -> Cow<'_, [u8]> {
    // `/&([^#])(?![a-z1-4]{1,8};)/i` by hand: an `&` and the byte after it (not `#`) match unless that byte is
    // followed by 1 to 8 of `[a-zA-Z1-4]` and then `;`; a match is replaced by `&#038;` and that byte.
    let entity_like = |rest: &[u8]| {
        let run = rest.iter().take(9).take_while(|b| b.is_ascii_alphabetic() || (b'1'..=b'4').contains(b)).count();
        (1..=8).contains(&run) && rest.get(run) == Some(&b';')
    };
    let mut out: Option<Vec<u8>> = None;
    let mut copied = 0;
    let mut i = match memchr::memchr(b'&', content) {
        Some(i) => i,
        None => return Cow::Borrowed(content),
    };
    while i < content.len() {
        if content[i] == b'&' && content.get(i + 1).is_some_and(|&c| c != b'#') && !entity_like(&content[i + 2..]) {
            let buf = out.get_or_insert_with(|| Vec::with_capacity(content.len() + 16));
            buf.extend_from_slice(&content[copied..i]);
            buf.extend_from_slice(b"&#038;");
            buf.push(content[i + 1]);
            i += 2;
            copied = i;
            continue;
        }
        i += 1;
        match memchr::memchr(b'&', &content[i.min(content.len())..]) {
            Some(next) => i += next,
            None => break,
        }
    }
    match out {
        None => Cow::Borrowed(content),
        Some(mut buf) => {
            buf.extend_from_slice(&content[copied..]);
            Cow::Owned(buf)
        }
    }
}

/// `convert_invalid_entities()`'s `$wp_htmltranswinuni`: Windows-1252 code points written as references to the
/// C1 controls, and what they should be.
static WINUNI: LazyLock<StrtrTable> = LazyLock::new(|| {
    const TO: [&str; 32] = [
        "&#8364;", "", "&#8218;", "&#402;", "&#8222;", "&#8230;", "&#8224;", "&#8225;", "&#710;", "&#8240;", "&#352;",
        "&#8249;", "&#338;", "", "&#381;", "", "", "&#8216;", "&#8217;", "&#8220;", "&#8221;", "&#8226;", "&#8211;",
        "&#8212;", "&#732;", "&#8482;", "&#353;", "&#8250;", "&#339;", "", "&#382;", "&#376;",
    ];
    StrtrTable::new(
        TO.iter().enumerate().map(|(i, to)| (format!("&#{};", 128 + i).into_bytes(), to.as_bytes().to_vec())).collect(),
    )
});

/// WordPress's `convert_invalid_entities()`: `&#128;` to `&#159;` become the Windows-1252 characters they meant.
///
/// @param content
///     The text.
/// @return
///     The converted text, borrowed when it has no `&#1`.
pub fn convert_invalid_entities(content: &[u8]) -> Cow<'_, [u8]> {
    if memmem::find(content, b"&#1").is_none() {
        return Cow::Borrowed(content);
    }
    strtr(content, &WINUNI)
}

static ENT2NCR_REPLACE: LazyLock<StrReplace> = LazyLock::new(|| {
    StrReplace::new(ENT2NCR.iter().map(|(e, n)| (e.as_bytes().to_vec(), n.as_bytes().to_vec())).collect())
});

/// `ent2ncr()`'s conversion: named entities become numeric references. The `pre_ent2ncr` filter is the facade's.
///
/// @param text
///     The text.
/// @return
///     The converted text, borrowed when it has no `&`.
pub fn ent2ncr(text: &[u8]) -> Cow<'_, [u8]> {
    if memchr::memchr(b'&', text).is_none() {
        return Cow::Borrowed(text);
    }
    ENT2NCR_REPLACE.apply(text)
}

/// WordPress's `capital_P_dangit()`.
///
/// @param text
///     The text.
/// @param in_title
///     Whether the current filter is `the_title` or `wp_title`, where every `Wordpress` is corrected.
/// @param opening_double_quote
///     `_x( '&#8220;', 'opening curly double quote' )`, after which `Wordpress` is corrected elsewhere.
/// @return
///     The corrected text, borrowed when it has no `Wordpress`.
pub fn capital_p_dangit<'a>(text: &'a [u8], in_title: bool, opening_double_quote: &[u8]) -> Cow<'a, [u8]> {
    if memmem::find(text, b"Wordpress").is_none() {
        return Cow::Borrowed(text);
    }
    if in_title {
        return replace_one(text, b"Wordpress", b"WordPress");
    }
    capital_p_dangit_replace(opening_double_quote).apply(text)
}

/// `capital_P_dangit()`'s `str_replace()` outside titles, for one opening double quote; build it once and reuse it.
///
/// @param opening_double_quote
///     The translated opening double quote.
/// @return
///     The prepared replacement.
pub fn capital_p_dangit_replace(opening_double_quote: &[u8]) -> StrReplace {
    let quoted = |prefix: &[u8], word: &[u8]| [prefix, word].concat();
    StrReplace::new(
        [&b" "[..], b"&#8216;", opening_double_quote, b">", b"("]
            .iter()
            .map(|p| (quoted(p, b"Wordpress"), quoted(p, b"WordPress")))
            .collect(),
    )
}

/// `format_for_editor()`'s escaping: a truthy text gets `htmlspecialchars( $text, ENT_NOQUOTES, $charset )`.
///
/// The `format_for_editor` filter is the facade's.
///
/// @param text
///     The text.
/// @param charset
///     The site's charset, from `blog_charset`.
/// @return
///     The escaped text; `""` and `"0"` are returned as they are.
pub fn format_for_editor_core<'a>(text: &'a [u8], charset: &Charset) -> Cow<'a, [u8]> {
    if text.is_empty() || text == b"0" {
        return Cow::Borrowed(text);
    }
    htmlspecialchars(text, EntFlags::from_php(0), charset, true)
}
