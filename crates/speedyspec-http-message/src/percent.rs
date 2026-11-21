//! Percent-encoding for URI components, as PSR-7 requires: characters outside the allowed set are encoded, and an
//! existing `%XX` escape is left alone so nothing is encoded twice.

/// Encodes every byte of `input` that is neither unreserved, a sub-delimiter, one of `extra`, nor part of a valid
/// `%XX` escape.
pub fn encode(input: &str, extra: &[u8]) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' && i + 2 < bytes.len() && is_hex(bytes[i + 1]) && is_hex(bytes[i + 2]) {
            out.push_str(&input[i..i + 3]);
            i += 3;
            continue;
        }
        if b.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=".contains(&b) || extra.contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
        i += 1;
    }
    out
}

fn is_hex(b: u8) -> bool {
    b.is_ascii_hexdigit()
}
