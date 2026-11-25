//! Replays what `compat/meta/record.php` recorded from WordPress 7.1.5: `is_protected_meta()` with no filter added.

mod support;

use speedyspec_php_serialize::PhpValue;
use speedyspec_wp_meta::is_protected_key;
use speedyspec_wp_meta_contracts::MetaKey;

#[test]
fn protected_keys_match_wordpress() {
    let cases = support::cases();
    let protected = cases["protected"].as_array().unwrap();
    assert_eq!(protected.len(), 28);
    for case in protected {
        let key = MetaKey::new(hex::decode(case["key_hex"].as_str().unwrap()).unwrap());
        let PhpValue::Bool(want) = support::decode(&case["result"]) else {
            panic!("result is a bool");
        };
        assert_eq!(
            is_protected_key(&key),
            want,
            "key {} (meta_type {:?})",
            case["key_hex"],
            case["meta_type"]
        );
    }
}
