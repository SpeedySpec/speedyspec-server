//! kses.php's small string functions.

use std::borrow::Cow;

use speedyspec_wp_kses_contracts::{AllowedAttrs, AllowedHtml, ByteMap};

use crate::scan::is_space;

/// The bytes `wp_kses_no_null()` removes: `[\x00-\x08\x0B\x0C\x0E-\x1F]`.
#[inline]
fn is_removed_control(b: u8) -> bool {
    // Bitwise, not short-circuiting, so a loop over it compiles to vector instructions.
    (b < 0x20) & (b != b'\t') & (b != b'\n') & (b != b'\r')
}

/// Whether `content` holds a byte `wp_kses_no_null()` removes, 32 bytes at a time.
fn has_removed_control(content: &[u8]) -> bool {
    let (chunks, rest) = content.as_chunks::<32>();
    for chunk in chunks {
        let found = chunk
            .iter()
            .fold(0u8, |found, b| found | u8::from(is_removed_control(*b)));
        if found != 0 {
            return true;
        }
    }
    rest.iter().any(|b| is_removed_control(*b))
}

/// `wp_kses_no_null()`: removes control bytes and, unless `keep_slash_zero`, every `\\+0+` run.
///
/// Borrows when nothing is removed, the common case.
pub fn no_null(content: &[u8], keep_slash_zero: bool) -> Cow<'_, [u8]> {
    let has_control = has_removed_control(content);
    let has_slash = !keep_slash_zero && memchr::memchr(b'\\', content).is_some();
    if !has_control && !has_slash {
        return Cow::Borrowed(content);
    }
    let mut out: Vec<u8> = content
        .iter()
        .copied()
        .filter(|b| !is_removed_control(*b))
        .collect();
    if has_slash {
        out = remove_slash_zero(&out);
    }
    Cow::Owned(out)
}

/// `preg_replace( '/\\\\+0+/', '', $content )`.
fn remove_slash_zero(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'\\' {
            let slashes = s[i..].iter().take_while(|b| **b == b'\\').count();
            let zeros = s[i + slashes..].iter().take_while(|b| **b == b'0').count();
            if zeros > 0 {
                i += slashes + zeros;
                continue;
            }
            out.extend_from_slice(&s[i..i + slashes]);
            i += slashes;
            continue;
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

/// `wp_kses_stripslashes()`: `\"` becomes `"`, left to right.
pub fn stripslashes(content: &[u8]) -> Cow<'_, [u8]> {
    let quoted = memchr::memchr_iter(b'\\', content).any(|i| content.get(i + 1) == Some(&b'"'));
    if !quoted {
        return Cow::Borrowed(content);
    }
    let mut out = Vec::with_capacity(content.len());
    let mut i = 0;
    while i < content.len() {
        if content[i] == b'\\' && content.get(i + 1) == Some(&b'"') {
            out.push(b'"');
            i += 2;
        } else {
            out.push(content[i]);
            i += 1;
        }
    }
    Cow::Owned(out)
}

/// `wp_kses_html_error()`: drops the first attribute-like run (`^("[^"]*("|$)|'[^']*('|$)|\S)*\s*`).
pub fn html_error(attr: &[u8]) -> &[u8] {
    let mut i = 0;
    while i < attr.len() {
        match attr[i] {
            q @ (b'"' | b'\'') => {
                i = memchr::memchr(q, &attr[i + 1..]).map_or(attr.len(), |e| i + 1 + e + 1);
            }
            b if !is_space(b) => i += 1,
            _ => break,
        }
    }
    while i < attr.len() && is_space(attr[i]) {
        i += 1;
    }
    &attr[i..]
}

/// `wp_kses_array_lc()`: tag and attribute names lowercased (ASCII, as PHP 8's `strtolower()`).
///
/// A bare tag becomes an empty attribute list; PHP's `(array) true` would add an attribute named `0`, which no
/// attribute name can match.
pub fn array_lc(allowed: &AllowedHtml) -> AllowedHtml {
    let mut tags = ByteMap::default();
    for (tag, attrs) in &allowed.tags {
        let lowered = match attrs {
            AllowedAttrs::Empty => ByteMap::default(),
            AllowedAttrs::Attrs(map) => map
                .iter()
                .map(|(name, rules)| (name.to_ascii_lowercase().into(), rules.clone()))
                .collect(),
        };
        tags.insert(
            tag.to_ascii_lowercase().into(),
            AllowedAttrs::Attrs(lowered),
        );
    }
    AllowedHtml { tags }
}
