//! Hook names and priorities are PHP array keys, hashed as PHP hashes them.

use speedyspec_wp_hooks_contracts::{ArrayKey, HookKey, HookName, HookValue, php_string_hash};

fn k(s: &str) -> ArrayKey {
    ArrayKey::from_str_key(s)
}

#[test]
fn string_keys_follow_php() {
    assert_eq!(k("5"), ArrayKey::Int(5));
    assert_eq!(k("-5"), ArrayKey::Int(-5));
    assert_eq!(k("05"), ArrayKey::Str("05".into()));
    assert_eq!(k("-0"), ArrayKey::Str("-0".into()));
    assert_eq!(k(" 7"), ArrayKey::Str(" 7".into()));
    assert_eq!(
        k("9223372036854775808"),
        ArrayKey::Str("9223372036854775808".into())
    );
    assert_eq!(k("-9223372036854775808"), ArrayKey::Int(i64::MIN));
    assert_eq!(
        ArrayKey::from_value(&HookValue::Float(1.5)).unwrap(),
        ArrayKey::Int(1)
    );
    assert_eq!(
        ArrayKey::from_value(&HookValue::Null).unwrap(),
        ArrayKey::Str(String::new())
    );
    assert_eq!(
        ArrayKey::from_value(&HookValue::Bool(true)).unwrap(),
        ArrayKey::Int(1)
    );
}

#[test]
fn hook_keys_hash_as_php_does() {
    // zend_inline_hash_func(""), as PHP 8 computes it on 64-bit. That Rust-made keys find hooks PHP names (and
    // the other way round) is shown end to end by the PHP tests.
    assert_eq!(php_string_hash(b""), 5381 | 0x8000_0000_0000_0000);
    assert_eq!(
        php_string_hash(b"a"),
        (5381 * 33 + 97) | 0x8000_0000_0000_0000
    );
    assert_eq!(HookKey::new(ArrayKey::Int(7)).hash(), 7);
    assert_eq!(
        HookKey::new(k("init")),
        HookKey::prehashed(k("init"), php_string_hash(b"init"))
    );
    assert!(HookName::new(HookValue::Array(Vec::new())).key().is_err());
}
