//! The plain values.

use speedyspec_wp_i18n_contracts::{RawEntry, RawTranslations, ReadError};

#[test]
fn raw_translations_ranges_slice_bytes() {
    let raw = RawTranslations {
        bytes: b"keyval".to_vec(),
        headers: vec![("plural-forms".into(), "nplurals=2; plural=n != 1;".into())],
        entries: vec![RawEntry {
            key: 0..3,
            value: 3..6,
        }],
    };
    assert_eq!(raw.key(&raw.entries[0]), b"key");
    assert_eq!(raw.value(&raw.entries[0]), b"val");
}

#[test]
fn read_error_messages_are_wordpress_text() {
    assert_eq!(
        ReadError::Invalid("Invalid data").to_string(),
        "Invalid data"
    );
    assert_eq!(
        ReadError::Invalid("Magic marker does not exist").to_string(),
        "Magic marker does not exist"
    );
}
