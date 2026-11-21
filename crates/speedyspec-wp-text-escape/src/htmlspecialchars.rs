//! PHP's `htmlspecialchars()`, as `ext/standard/html.c` implements it, for the flags WordPress passes.

use std::borrow::Cow;

use memchr::memchr3;

use crate::Charset;
use crate::html401::HTML401_NAMES;

/// The `htmlspecialchars()` flags WordPress uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntFlags {
    /// `ENT_COMPAT` (2): `"` becomes `&quot;`.
    pub double_quotes: bool,
    /// The single-quote bit of `ENT_QUOTES` (1): `'` becomes `&#039;`, or `&apos;` in XML.
    pub single_quotes: bool,
    /// `ENT_XML1` (16): XML's entity names, and `&apos;`.
    pub xml1: bool,
}

impl EntFlags {
    /// Reads PHP's integer flags; bits WordPress never passes are ignored.
    pub fn from_php(flags: i64) -> EntFlags {
        EntFlags {
            double_quotes: flags & 2 != 0,
            single_quotes: flags & 1 != 0,
            xml1: flags & 16 != 0,
        }
    }
}

const XML_NAMES: &[&str] = &["amp", "apos", "gt", "lt", "quot"];

/// `htmlspecialchars( $text, $flags, $charset, $double_encode )`.
///
/// On UTF-8, input that is not valid UTF-8 gives an empty string, as PHP does without `ENT_SUBSTITUTE`. Otherwise
/// `<`, `>`, `&` and (by `flags`) the quotes become references; with `double_encode` false, a reference that is
/// already valid is kept as it is. Input with nothing to escape is borrowed.
pub fn htmlspecialchars<'a>(
    text: &'a [u8],
    flags: EntFlags,
    charset: &Charset,
    double_encode: bool,
) -> Cow<'a, [u8]> {
    if *charset == Charset::Utf8 && std::str::from_utf8(text).is_err() {
        return Cow::Borrowed(&[]);
    }
    let special = |b: u8| {
        matches!(b, b'<' | b'>' | b'&')
            || (b == b'"' && flags.double_quotes)
            || (b == b'\'' && flags.single_quotes)
    };
    let Some(first) = first_special(text, flags) else {
        return Cow::Borrowed(text);
    };
    let mut out = Vec::with_capacity(text.len() + text.len() / 8 + 8);
    out.extend_from_slice(&text[..first]);
    let mut i = first;
    while i < text.len() {
        let b = text[i];
        if !special(b) {
            out.push(b);
            i += 1;
            continue;
        }
        match b {
            b'<' => out.extend_from_slice(b"&lt;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            b'"' => out.extend_from_slice(b"&quot;"),
            b'\'' if flags.xml1 => out.extend_from_slice(b"&apos;"),
            b'\'' => out.extend_from_slice(b"&#039;"),
            _ => {
                let kept = if double_encode {
                    0
                } else {
                    valid_reference(&text[i..], flags.xml1)
                };
                if kept > 0 {
                    out.extend_from_slice(&text[i..i + kept]);
                    i += kept;
                    continue;
                }
                out.extend_from_slice(b"&amp;");
            }
        }
        i += 1;
    }
    Cow::Owned(out)
}

/// The first byte that needs escaping under `flags`.
fn first_special(text: &[u8], flags: EntFlags) -> Option<usize> {
    let angle_or_amp = memchr3(b'<', b'>', b'&', text);
    let quote = match (flags.double_quotes, flags.single_quotes) {
        (true, true) => memchr::memchr2(b'"', b'\'', text),
        (true, false) => memchr::memchr(b'"', text),
        (false, true) => memchr::memchr(b'\'', text),
        (false, false) => None,
    };
    match (angle_or_amp, quote) {
        (Some(a), Some(q)) => Some(a.min(q)),
        (a, q) => a.or(q),
    }
}

/// The length of the valid reference at the start of `s` (which starts with `&`), or 0 when there is none.
fn valid_reference(s: &[u8], xml1: bool) -> usize {
    if s.get(1) == Some(&b'#') {
        return numeric_reference(s);
    }
    let name_len = s[1..]
        .iter()
        .take_while(|b| b.is_ascii_alphanumeric())
        .count();
    if name_len == 0 || s.get(1 + name_len) != Some(&b';') {
        return 0;
    }
    let name = std::str::from_utf8(&s[1..1 + name_len]).unwrap_or_default();
    let known = if xml1 {
        XML_NAMES.contains(&name)
    } else {
        HTML401_NAMES.binary_search(&name).is_ok()
    };
    if known { name_len + 2 } else { 0 }
}

/// `&#` and decimal digits, or `&#x` and hex digits, then `;`, naming a code point up to U+10FFFF. Like C's
/// `strtol`, a hex value may carry its own `0x` prefix.
fn numeric_reference(s: &[u8]) -> usize {
    let mut i = 2;
    let hex = matches!(s.get(i), Some(b'x' | b'X'));
    if hex {
        i += 1;
    }
    let is_digit = |b: &u8| if hex { b.is_ascii_hexdigit() } else { b.is_ascii_digit() };
    if !s.get(i).is_some_and(is_digit) {
        return 0;
    }
    if hex
        && s.get(i) == Some(&b'0')
        && matches!(s.get(i + 1), Some(b'x' | b'X'))
        && s.get(i + 2).is_some_and(u8::is_ascii_hexdigit)
    {
        i += 2;
    }
    let start = i;
    while s.get(i).is_some_and(is_digit) {
        i += 1;
    }
    if s.get(i) != Some(&b';') {
        return 0;
    }
    let digits = std::str::from_utf8(&s[start..i]).unwrap_or_default();
    let value = u64::from_str_radix(digits, if hex { 16 } else { 10 }).unwrap_or(u64::MAX);
    if value > 0x10FFFF { 0 } else { i + 1 }
}
