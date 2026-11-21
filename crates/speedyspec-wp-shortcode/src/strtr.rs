//! The fixed `strtr()` tables of `shortcodes.php`, each done in one `memchr` pass.

/// `strtr( $content, array( '[' => '&#91;', ']' => '&#93;' ) )`.
pub(crate) fn encode_brackets(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() + 16);
    let mut last = 0;
    for i in memchr::memchr2_iter(b'[', b']', s) {
        out.extend_from_slice(&s[last..i]);
        out.extend_from_slice(if s[i] == b'[' { b"&#91;" } else { b"&#93;" });
        last = i + 1;
    }
    out.extend_from_slice(&s[last..]);
    out
}

/// Replaces `&#91;` and `&#93;` with `with_91` and `with_93`, as `strtr()` with those two keys does.
fn entities(s: &[u8], with_91: &[u8], with_93: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut last = 0;
    for i in memchr::memmem::find_iter(s, b"&#9") {
        if i < last {
            continue;
        }
        let rest = &s[i + 3..];
        let with = if rest.starts_with(b"1;") {
            with_91
        } else if rest.starts_with(b"3;") {
            with_93
        } else {
            continue;
        };
        out.extend_from_slice(&s[last..i]);
        out.extend_from_slice(with);
        last = i + 5;
    }
    out.extend_from_slice(&s[last..]);
    out
}

/// `strtr( $content, array( '&#91;' => '&#091;', '&#93;' => '&#093;' ) )`.
pub(crate) fn normalize_entities(s: &[u8]) -> Vec<u8> {
    entities(s, b"&#091;", b"&#093;")
}

/// `unescape_invalid_shortcodes()`: `strtr( $content, array( '&#91;' => '[', '&#93;' => ']' ) )`.
pub(crate) fn unescape(s: &[u8]) -> Vec<u8> {
    entities(s, b"[", b"]")
}
