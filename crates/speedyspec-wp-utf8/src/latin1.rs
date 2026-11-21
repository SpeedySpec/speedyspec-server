//! `_wp_utf8_encode_fallback` and `_wp_utf8_decode_fallback`: ISO-8859-1 to and from UTF-8.

use crate::scan::scan_utf8;

/// Encodes ISO-8859-1 bytes as UTF-8.
pub fn utf8_encode_latin1(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() + s.len() / 4);
    for &b in s {
        if b < 0x80 {
            out.push(b);
        } else {
            out.push(0xC0 | (b >> 6));
            out.push(0x80 | (b & 0x3F));
        }
    }
    out
}

/// Decodes UTF-8 to ISO-8859-1: a code point above U+00FF becomes `?`, and so does each invalid span.
pub fn utf8_decode_latin1(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut at = 0;
    while at < s.len() {
        if s[at] < 0x80 {
            out.push(s[at]);
            at += 1;
            continue;
        }
        let scan = scan_utf8(s, at, None, Some(1));
        let span = scan.at - at;
        if scan.code_points != 1 {
            if scan.invalid_length == 0 {
                // WordPress's loop `break`s here and appends the rest unchanged.
                out.extend_from_slice(&s[at..]);
                return out;
            }
        } else {
            let b1 = s[at];
            let mut next = b'?';
            if b1 & 0xE0 == 0xC0 {
                let cp = (u32::from(b1 & 0x1F) << 6) | u32::from(s[at + 1] & 0x3F);
                if cp <= 0xFF {
                    next = cp as u8;
                }
            }
            out.push(next);
        }
        at += span;
        if scan.invalid_length > 0 {
            out.push(b'?');
            at += scan.invalid_length;
        }
    }
    out
}
