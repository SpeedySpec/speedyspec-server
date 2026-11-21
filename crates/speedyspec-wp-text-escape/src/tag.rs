//! The core of `tag_escape()`.

use std::borrow::Cow;

static KEEP: [bool; 256] = {
    let mut table = [false; 256];
    let mut b = 0;
    while b < 256 {
        let c = b as u8;
        table[b] = c.is_ascii_alphanumeric() || c == b'-' || c == b'_' || c == b':';
        b += 1;
    }
    table
};

/// `tag_escape()` before its filter: keeps `[a-zA-Z0-9_:-]` and lowercases it, so the result is a safe tag name.
pub fn tag_escape_core(tag: &[u8]) -> Cow<'_, [u8]> {
    if tag.iter().all(|&b| KEEP[b as usize] && !b.is_ascii_uppercase()) {
        return Cow::Borrowed(tag);
    }
    Cow::Owned(
        tag.iter()
            .filter(|&&b| KEEP[b as usize])
            .map(u8::to_ascii_lowercase)
            .collect(),
    )
}
