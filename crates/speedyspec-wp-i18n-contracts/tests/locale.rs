//! The locale values: PHP truthiness, word count types and port errors.

use speedyspec_wp_i18n_contracts::{PhpValue, PortError, WordCountType, php_truthy};

#[test]
fn truthiness_matches_php() {
    let falsy = [
        PhpValue::Null,
        PhpValue::Bool(false),
        PhpValue::Int(0),
        PhpValue::Float(0.0),
        PhpValue::Str(String::new()),
        PhpValue::Str("0".into()),
        PhpValue::Bytes(b"0".to_vec()),
        PhpValue::Array(Vec::new()),
    ];
    for v in &falsy {
        assert!(!php_truthy(v), "{v:?} is falsy");
    }
    let truthy = [
        PhpValue::Bool(true),
        PhpValue::Int(-1),
        PhpValue::Str("0.0".into()),
        PhpValue::Str("de_DE".into()),
        PhpValue::Bytes(vec![0xff]),
    ];
    for v in &truthy {
        assert!(php_truthy(v), "{v:?} is truthy");
    }
}

#[test]
fn word_count_types_round_trip() {
    for t in [
        WordCountType::Words,
        WordCountType::CharactersExcludingSpaces,
        WordCountType::CharactersIncludingSpaces,
    ] {
        assert_eq!(WordCountType::from_translation(t.as_str().as_bytes()), t);
    }
    assert_eq!(
        WordCountType::from_translation(b"Words"),
        WordCountType::Words
    );
}

#[test]
fn port_error_carries_its_message() {
    assert_eq!(
        PortError::message("callback threw").to_string(),
        "callback threw"
    );
}
