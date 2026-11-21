//! UTF-8 validation: `wp_is_valid_utf8` and `_wp_is_valid_utf8_fallback`.

use crate::scan::scan_utf8;

/// Whether `bytes` is valid UTF-8 (surrogates, overlong forms and code points past U+10FFFF are not).
pub fn is_valid_utf8(bytes: &[u8]) -> bool {
    simdutf8::basic::from_utf8(bytes).is_ok()
}

/// `_wp_is_valid_utf8_fallback`, kept as the reference the fast path is checked against.
pub fn is_valid_utf8_reference(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return true;
    }
    let s = scan_utf8(bytes, 0, None, None);
    s.at == bytes.len() && s.invalid_length == 0
}
