//! The deprecated `seems_utf8()`.

/// `seems_utf8()` as WordPress 7.1.5 writes it: a byte-pattern check that accepts the obsolete 5- and 6-byte forms
/// and checks neither overlong forms nor surrogates. Kept only because callers still use it; the facade reports its
/// deprecation.
pub fn seems_utf8_core(text: &[u8]) -> bool {
    let mut i = 0;
    while i < text.len() {
        let c = text[i];
        let n = if c < 0x80 {
            0
        } else if c & 0xE0 == 0xC0 {
            1
        } else if c & 0xF0 == 0xE0 {
            2
        } else if c & 0xF8 == 0xF0 {
            3
        } else if c & 0xFC == 0xF8 {
            4
        } else if c & 0xFE == 0xFC {
            5
        } else {
            return false;
        };
        for _ in 0..n {
            i += 1;
            if i == text.len() || text[i] & 0xC0 != 0x80 {
                return false;
            }
        }
        i += 1;
    }
    true
}
