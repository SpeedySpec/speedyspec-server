//! `is_protected_meta()`'s built-in rule, before the `is_protected_meta` filter.

use speedyspec_wp_meta_contracts::{MetaKey, ProtectedMeta};

/// `preg_replace( '/[^\x20-\x7E\p{L}]/', '', $key )` has no `u` flag, so it works on bytes: bytes 0x20-0x7E survive,
/// and of 0x80-0xFF only the Latin-1 letter bytes do.
fn survives(byte: u8) -> bool {
    matches!(
        byte,
        0x20..=0x7E | 0xAA | 0xB5 | 0xBA | 0xC0..=0xD6 | 0xD8..=0xF6 | 0xF8..=0xFF
    )
}

/// Whether the key is protected: its first surviving byte is `_`. The `is_protected_meta` filter is not applied.
pub fn is_protected_key(key: &MetaKey) -> bool {
    key.as_bytes().iter().copied().find(|b| survives(*b)) == Some(b'_')
}

/// `is_protected_meta()` with no filter attached: the built-in rule alone.
pub struct UnfilteredProtection;

impl ProtectedMeta for UnfilteredProtection {
    fn is_protected(&self, key: &MetaKey, _meta_type: &str) -> bool {
        is_protected_key(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leading_underscore_is_protected() {
        assert!(is_protected_key(&MetaKey::from("_edit_lock")));
        assert!(!is_protected_key(&MetaKey::from("color")));
        assert!(!is_protected_key(&MetaKey::from("")));
    }

    #[test]
    fn control_bytes_are_dropped_first() {
        assert!(is_protected_key(&MetaKey::new(b"\x00_secret".to_vec())));
    }

    #[test]
    fn non_letter_high_bytes_are_dropped_letters_are_kept() {
        // C2 is a Latin-1 letter byte (A circumflex), so it survives and is not an underscore.
        assert!(!is_protected_key(&MetaKey::new(b"\xC2\xA0_x".to_vec())));
        // D7 (multiplication sign) does not survive.
        assert!(is_protected_key(&MetaKey::new(b"\xD7_x".to_vec())));
    }
}
