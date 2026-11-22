//! WordPress's character sanitizers: the cores, before their filters.
//!
//! The character-class ones are a 256-entry table each, one pass and no allocation when nothing changes; their
//! patterns are kept for the differential tests. `TextApi` fires the filters around them.

use std::borrow::Cow;
use std::sync::LazyLock;

use crate::preg::{Pattern, preg_match};

/// Keeps the bytes `keep` marks; borrows when it keeps them all.
fn keep_only<'a>(text: &'a [u8], keep: &[bool; 256]) -> Cow<'a, [u8]> {
    match text.iter().position(|&b| !keep[usize::from(b)]) {
        None => Cow::Borrowed(text),
        Some(first) => {
            let mut out = text[..first].to_vec();
            out.extend(text[first..].iter().copied().filter(|&b| keep[usize::from(b)]));
            Cow::Owned(out)
        }
    }
}

/// A table that keeps ASCII letters, digits and the given punctuation.
const fn class(lower: bool, upper: bool, punctuation: &[u8]) -> [bool; 256] {
    let mut table = [false; 256];
    let mut b = 0;
    while b < 256 {
        let c = b as u8;
        table[b] = c.is_ascii_digit() || (lower && c.is_ascii_lowercase()) || (upper && c.is_ascii_uppercase());
        b += 1;
    }
    let mut i = 0;
    while i < punctuation.len() {
        table[punctuation[i] as usize] = true;
        i += 1;
    }
    table
}

/// `[a-z0-9_\-]`: what `sanitize_key()` keeps after lowercasing.
static KEY: [bool; 256] = class(true, false, b"_-");
/// `[A-Za-z0-9_-]`: `sanitize_html_class()` and `sanitize_locale_name()`.
static CLASS: [bool; 256] = class(true, true, b"_-");
/// `[-+*.a-zA-Z0-9\/]`: `sanitize_mime_type()`.
static MIME: [bool; 256] = class(true, true, b"-+*./");

/// `sanitize_key()`'s `preg_replace( '/[^a-z0-9_\-]/', '', strtolower( $key ) )`.
///
/// @param key
///     The key, already a string.
/// @return
///     Lowercase letters, digits, `_` and `-`.
pub fn sanitize_key_core(key: &[u8]) -> Cow<'_, [u8]> {
    if key.iter().all(|&b| KEY[usize::from(b)]) {
        return Cow::Borrowed(key);
    }
    Cow::Owned(key.iter().map(u8::to_ascii_lowercase).filter(|&b| KEY[usize::from(b)]).collect())
}

/// `sanitize_html_class()`'s two replacements: drop `%XX` escapes, then everything but `[A-Za-z0-9_-]`.
///
/// @param classname
///     The class name.
/// @return
///     The sanitized class; the fallback and the filter are the caller's.
pub fn sanitize_html_class_core(classname: &[u8]) -> Cow<'_, [u8]> {
    if classname.iter().all(|&b| CLASS[usize::from(b)]) {
        return Cow::Borrowed(classname);
    }
    let mut out = Vec::with_capacity(classname.len());
    let mut i = 0;
    while i < classname.len() {
        let b = classname[i];
        if b == b'%' && classname.get(i + 1).is_some_and(u8::is_ascii_hexdigit)
            && classname.get(i + 2).is_some_and(u8::is_ascii_hexdigit)
        {
            i += 3;
            continue;
        }
        if CLASS[usize::from(b)] {
            out.push(b);
        }
        i += 1;
    }
    Cow::Owned(out)
}

/// `sanitize_locale_name()`'s `preg_replace( '/[^A-Za-z0-9_-]/', '', ... )`.
///
/// @param locale_name
///     The locale name.
/// @return
///     Letters, digits, `_` and `-`.
pub fn sanitize_locale_name_core(locale_name: &[u8]) -> Cow<'_, [u8]> {
    keep_only(locale_name, &CLASS)
}

/// `sanitize_mime_type()`'s `preg_replace( '/[^-+*.a-zA-Z0-9\/]/', '', ... )`.
///
/// @param mime_type
///     The MIME type.
/// @return
///     Letters, digits and `-+*./`.
pub fn sanitize_mime_type_core(mime_type: &[u8]) -> Cow<'_, [u8]> {
    keep_only(mime_type, &MIME)
}

/// `sanitize_hex_color()`'s pattern.
static HEX_COLOR: LazyLock<Pattern> = LazyLock::new(|| Pattern::php("|^#([A-Fa-f0-9]{3}){1,2}$|"));

/// WordPress's `sanitize_hex_color()`.
///
/// @param color
///     The colour.
/// @return
///     `Some("")` for `""`, the colour when it is `#` and 3 or 6 hex digits (PCRE's `$` also allows one final
///     newline), otherwise `None` (PHP's `null`).
pub fn sanitize_hex_color(color: &[u8]) -> Option<&[u8]> {
    if color.is_empty() || matches!(preg_match(&HEX_COLOR, color), Ok(Some(_))) {
        Some(color)
    } else {
        None
    }
}

/// WordPress's `sanitize_hex_color_no_hash()`.
///
/// @param color
///     The colour, with or without leading `#`s.
/// @return
///     The colour without them when valid, `Some("")` when nothing is left, otherwise `None`.
pub fn sanitize_hex_color_no_hash(color: &[u8]) -> Option<&[u8]> {
    let unhashed = &color[color.iter().take_while(|&&b| b == b'#').count()..];
    if unhashed.is_empty() {
        return Some(unhashed);
    }
    let hashed = [b"#", unhashed].concat();
    // PHP tests the truthiness of the result, which for a valid colour is never "" or "0".
    sanitize_hex_color(&hashed).is_some().then_some(unhashed)
}

/// WordPress's `maybe_hash_hex_color()`.
///
/// @param color
///     The colour.
/// @return
///     `#` and the colour when it is a valid colour without a hash, otherwise the colour as it was.
pub fn maybe_hash_hex_color(color: &[u8]) -> Cow<'_, [u8]> {
    match sanitize_hex_color_no_hash(color) {
        Some(unhashed) if !unhashed.is_empty() && unhashed != b"0" => Cow::Owned([b"#", unhashed].concat()),
        _ => Cow::Borrowed(color),
    }
}

/// `sanitize_sql_orderby()`'s two patterns.
static ORDERBY: LazyLock<(Pattern, Pattern)> = LazyLock::new(|| {
    (
        Pattern::php("/^\\s*(([a-z0-9_]+|`[a-z0-9_]+`)(\\s+(ASC|DESC))?\\s*(,\\s*(?=[a-z0-9_`])|$))+$/i"),
        Pattern::php("/^\\s*RAND\\(\\s*\\)\\s*$/i"),
    )
});

/// WordPress's `sanitize_sql_orderby()`.
///
/// @param orderby
///     The `ORDER BY` clause.
/// @return
///     The clause when it is a list of columns with optional `ASC`/`DESC`, or `RAND()`; otherwise `None` (`false`).
pub fn sanitize_sql_orderby(orderby: &[u8]) -> Option<&[u8]> {
    let (list, rand) = &*ORDERBY;
    let matches = |p: &Pattern| matches!(preg_match(p, orderby), Ok(Some(_)));
    (matches(list) || matches(rand)).then_some(orderby)
}

/// Every run of bytes `in_run` accepts as one space, as `_sanitize_text_fields()`'s
/// `preg_replace( '/[\r\n\t ]+/', ' ', ... )` and `preg_replace( '/ +/', ' ', ... )` do.
///
/// @param text
///     The text.
/// @param in_run
///     Which bytes form runs.
/// @return
///     The text with each run as one space.
pub fn collapse_runs(text: &[u8], in_run: impl Fn(u8) -> bool) -> Vec<u8> {
    collapse_runs_to(text, in_run, b' ')
}

/// Every run of bytes `in_run` accepts as one `replacement` byte, as `preg_replace( '/[...]+/', $r, ... )` does
/// (and `/\.{2,}/` to `.`, since one `.` is already `.`).
///
/// @param text
///     The text.
/// @param in_run
///     Which bytes form runs.
/// @param replacement
///     The byte each run becomes.
/// @return
///     The text with each run replaced.
pub fn collapse_runs_to(text: &[u8], in_run: impl Fn(u8) -> bool, replacement: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut run = false;
    for &b in text {
        if in_run(b) {
            if !run {
                out.push(replacement);
            }
            run = true;
        } else {
            out.push(b);
            run = false;
        }
    }
    out
}

/// The first `%XX` (hex digits in either case), as `_sanitize_text_fields()`'s `/%[a-f0-9]{2}/i` finds it.
///
/// @param text
///     The text.
/// @return
///     The three bytes, or `None`.
pub fn first_octet(text: &[u8]) -> Option<[u8; 3]> {
    memchr::memchr_iter(b'%', text).find_map(|at| match text.get(at + 1..at + 3) {
        Some(&[a, b]) if a.is_ascii_hexdigit() && b.is_ascii_hexdigit() => Some([b'%', a, b]),
        _ => None,
    })
}
