//! `wp_unslash()` (`stripslashes_deep()`) over PHP values, for `add_metadata()`, `update_metadata()` and
//! `delete_metadata()`.
//!
//! The text seam has `stripslashes_deep()` over hook values (`speedyspec-wp-text`), but it is an implementation crate
//! of another seam, so meta may not depend on it. These are pure functions for the formatting seam to adopt.

use speedyspec_php_serialize::{PhpKey, PhpValue};

/// PHP's `stripslashes()`: `\0` becomes NUL, `\x` becomes `x`, and a trailing lone `\` is dropped.
pub fn stripslashes(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut bytes = text.iter().copied();
    while let Some(b) = bytes.next() {
        if b != b'\\' {
            out.push(b);
            continue;
        }
        match bytes.next() {
            Some(b'0') => out.push(0),
            Some(next) => out.push(next),
            None => {}
        }
    }
    out
}

/// `wp_unslash()`, which is `stripslashes_deep()`: `map_deep()` with `stripslashes_from_strings_only()`.
///
/// Strings lose their slashes; other scalars stay as they are. Arrays keep their keys and order. An object's
/// accessible properties are walked, as `get_object_vars()` from outside the class sees them: private and protected
/// ones (mangled names starting with NUL) are left alone.
pub fn wp_unslash(value: &PhpValue) -> PhpValue {
    match value {
        PhpValue::Str(s) => PhpValue::Str(stripslashes(s)),
        PhpValue::Array(items) => PhpValue::Array(map_items(items, false)),
        PhpValue::Object { class, properties } => PhpValue::Object {
            class: class.clone(),
            properties: map_items(properties, true),
        },
        other => other.clone(),
    }
}

fn map_items(items: &[(PhpKey, PhpValue)], object: bool) -> Vec<(PhpKey, PhpValue)> {
    items
        .iter()
        .map(|(k, v)| {
            let hidden = object && matches!(k, PhpKey::Str(name) if name.first() == Some(&0));
            (k.clone(), if hidden { v.clone() } else { wp_unslash(v) })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stripslashes_as_php() {
        assert_eq!(stripslashes(br"a\'b\\c\0d\"), b"a'b\\c\0d");
        assert_eq!(stripslashes(b"plain"), b"plain");
    }

    #[test]
    fn deep_only_strings() {
        let value = PhpValue::Array(vec![
            (
                PhpKey::Str(br"k\'".to_vec()),
                PhpValue::Str(br"v\'".to_vec()),
            ),
            (PhpKey::Int(1), PhpValue::Int(5)),
        ]);
        assert_eq!(
            wp_unslash(&value),
            PhpValue::Array(vec![
                (PhpKey::Str(br"k\'".to_vec()), PhpValue::Str(b"v'".to_vec())),
                (PhpKey::Int(1), PhpValue::Int(5)),
            ])
        );
    }

    #[test]
    fn object_public_properties_only() {
        let value = PhpValue::Object {
            class: "stdClass".into(),
            properties: vec![
                (PhpKey::Str(b"a".to_vec()), PhpValue::Str(br"x\'".to_vec())),
                (
                    PhpKey::Str(b"\0*\0p".to_vec()),
                    PhpValue::Str(br"x\'".to_vec()),
                ),
            ],
        };
        let PhpValue::Object { properties, .. } = wp_unslash(&value) else {
            panic!("an object");
        };
        assert_eq!(properties[0].1, PhpValue::Str(b"x'".to_vec()));
        assert_eq!(properties[1].1, PhpValue::Str(br"x\'".to_vec()));
    }
}
