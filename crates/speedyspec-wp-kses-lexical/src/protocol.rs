//! `wp_kses_bad_protocol()` and its helpers.

use std::borrow::Cow;

use speedyspec_wp_kses_contracts::Protocols;

use crate::scan::is_space;
use crate::{decode_entities, no_null};

/// `wp_kses_bad_protocol()`: removes every URL scheme not in `protocols`, repeating until nothing changes (at most
/// six passes; content still changing after that is emptied).
pub fn bad_protocol<'a>(content: &'a [u8], protocols: &Protocols) -> Cow<'a, [u8]> {
    let content = no_null(content, false);
    let has = |p: &[u8]| protocols.0.iter().any(|q| &**q == p);
    if (content.starts_with(b"https://") && has(b"https"))
        || (content.starts_with(b"http://") && has(b"http"))
    {
        return content;
    }
    // A scheme needs a separator (`:` or a reference to one), so content with neither `:` nor `&` stays as it is.
    if memchr::memchr2(b':', b'&', &content).is_none() {
        return content;
    }
    let mut content = content.into_owned();
    let mut iterations = 0;
    loop {
        let next = bad_protocol_once(&content, protocols, 1);
        if next == content {
            return Cow::Owned(content);
        }
        content = next;
        iterations += 1;
        if iterations >= 6 {
            return Cow::Owned(Vec::new());
        }
    }
}

fn zeros(s: &[u8], from: usize) -> usize {
    s.get(from..)
        .unwrap_or_default()
        .iter()
        .take_while(|b| **b == b'0')
        .count()
}

/// The length of `&#0*58` or `&#x0*3a` (case-insensitive) at `s[0]`, or 0.
fn colon_reference(s: &[u8]) -> (usize, bool) {
    if !s.starts_with(b"&#") {
        return (0, false);
    }
    if matches!(s.get(2), Some(b'x' | b'X')) {
        let z = zeros(s, 3);
        let at = 3 + z;
        let ok = s.get(at) == Some(&b'3') && matches!(s.get(at + 1), Some(b'a' | b'A'));
        return if ok { (at + 2, true) } else { (0, true) };
    }
    let z = zeros(s, 2);
    let at = 2 + z;
    if s.get(at..at + 2) == Some(b"58") {
        (at + 2, false)
    } else {
        (0, false)
    }
}

/// `preg_replace( '/(&#0*58(?![;0-9])|&#x0*3a(?![;a-f0-9]))/i', '$1;', $content )`.
fn terminate_colon_references(content: &[u8]) -> Cow<'_, [u8]> {
    let mut out: Option<Vec<u8>> = None;
    let mut copied = 0;
    let mut at = 0;
    while let Some(i) = memchr::memmem::find(&content[at..], b"&#") {
        let start = at + i;
        let (len, hex) = colon_reference(&content[start..]);
        if len > 0 {
            let end = start + len;
            let next = content.get(end).copied();
            let continues = next.is_some_and(|b| {
                b == b';'
                    || if hex {
                        b.is_ascii_hexdigit()
                    } else {
                        b.is_ascii_digit()
                    }
            });
            if !continues {
                let o = out.get_or_insert_with(|| Vec::with_capacity(content.len() + 4));
                o.extend_from_slice(&content[copied..end]);
                o.push(b';');
                copied = end;
            }
            at = end;
        } else {
            at = start + 2;
        }
    }
    match out {
        None => Cow::Borrowed(content),
        Some(mut o) => {
            o.extend_from_slice(&content[copied..]);
            Cow::Owned(o)
        }
    }
}

/// The leftmost `:|&#0*58;|&#x0*3a;|&colon;` (case-insensitive): where it starts and how long it is.
fn find_separator(content: &[u8]) -> Option<(usize, usize)> {
    let mut at = 0;
    while let Some(i) = memchr::memchr2(b':', b'&', &content[at..]) {
        let p = at + i;
        if content[p] == b':' {
            return Some((p, 1));
        }
        let (len, _) = colon_reference(&content[p..]);
        if len > 0 && content.get(p + len) == Some(&b';') {
            return Some((p, len + 1));
        }
        if content.len() >= p + 7 && content[p..p + 7].eq_ignore_ascii_case(b"&colon;") {
            return Some((p, 7));
        }
        at = p + 1;
    }
    None
}

/// PHP's `trim()`: space, `\t`, `\n`, `\r`, `\0`, `\v`.
fn php_trim(s: &[u8]) -> &[u8] {
    let is_trim = |b: &u8| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0 | 0x0B);
    let start = s.iter().position(|b| !is_trim(b)).unwrap_or(s.len());
    let end = s.iter().rposition(|b| !is_trim(b)).map_or(start, |e| e + 1);
    &s[start..end]
}

/// PHP's `empty()` on a string.
fn php_empty(s: &[u8]) -> bool {
    s.is_empty() || s == b"0"
}

/// `wp_kses_bad_protocol_once()`.
pub fn bad_protocol_once(content: &[u8], protocols: &Protocols, count: u32) -> Vec<u8> {
    let content = terminate_colon_references(content);
    let Some((at, len)) = find_separator(&content) else {
        return content.into_owned();
    };
    let (scheme, rest) = (&content[..at], &content[at + len..]);
    if memchr::memmem::find(scheme, b"/?").is_some() {
        return content.into_owned();
    }
    let mut rest = php_trim(rest).to_vec();
    let protocol = bad_protocol_once2(scheme, protocols);
    if protocol == b"feed:" {
        if count > 2 {
            return Vec::new();
        }
        rest = bad_protocol_once(&rest, protocols, count + 1);
        if php_empty(&rest) {
            return rest;
        }
    }
    let mut out = protocol;
    out.extend_from_slice(&rest);
    out
}

/// `wp_kses_bad_protocol_once2()`: the scheme with `:` when allowed, else nothing.
pub fn bad_protocol_once2(scheme: &[u8], protocols: &Protocols) -> Vec<u8> {
    let decoded = decode_entities(scheme);
    let compact: Vec<u8> = decoded.iter().copied().filter(|b| !is_space(*b)).collect();
    let mut scheme = no_null(&compact, false).to_ascii_lowercase();
    if protocols.0.iter().any(|p| p.to_ascii_lowercase() == scheme) {
        scheme.push(b':');
        scheme
    } else {
        Vec::new()
    }
}
