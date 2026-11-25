//! The few PHP array and value semantics `register_meta()` and `$wp_meta_keys` lean on.

use speedyspec_php_serialize::{PhpKey, PhpValue};

/// PHP's `empty()`: null, false, 0, 0.0, `""`, `"0"` and `array()` are empty.
pub(crate) fn is_empty(value: &PhpValue) -> bool {
    match value {
        PhpValue::Null => true,
        PhpValue::Bool(b) => !b,
        PhpValue::Int(i) => *i == 0,
        PhpValue::Float(f) => *f == 0.0,
        PhpValue::Str(s) => s.is_empty() || s.as_slice() == b"0",
        PhpValue::Array(items) => items.is_empty(),
        _ => false,
    }
}

/// `$array['name']` for a string key.
pub(crate) fn get<'a>(items: &'a [(PhpKey, PhpValue)], name: &str) -> Option<&'a PhpValue> {
    items
        .iter()
        .find(|(k, _)| matches!(k, PhpKey::Str(s) if s.as_slice() == name.as_bytes()))
        .map(|(_, v)| v)
}

/// `isset( $array['name'] )`: present and not null.
pub(crate) fn isset<'a>(items: &'a [(PhpKey, PhpValue)], name: &str) -> Option<&'a PhpValue> {
    get(items, name).filter(|v| !matches!(v, PhpValue::Null))
}

/// `$array['name'] = $value`: replaces in place, or appends.
pub(crate) fn set(items: &mut Vec<(PhpKey, PhpValue)>, name: &str, value: PhpValue) {
    let key = PhpKey::Str(name.as_bytes().to_vec());
    match items.iter_mut().find(|(k, _)| *k == key) {
        Some(slot) => slot.1 = value,
        None => items.push((key, value)),
    }
}

/// `unset( $array['name'] )`.
pub(crate) fn unset(items: &mut Vec<(PhpKey, PhpValue)>, name: &str) {
    items.retain(|(k, _)| !matches!(k, PhpKey::Str(s) if s.as_slice() == name.as_bytes()));
}

/// `array_merge( $base, $over )`: string keys of `$over` replace in place or append; int keys are renumbered from
/// 0 across both arrays.
pub(crate) fn array_merge(
    base: Vec<(PhpKey, PhpValue)>,
    over: Vec<(PhpKey, PhpValue)>,
) -> Vec<(PhpKey, PhpValue)> {
    let mut out: Vec<(PhpKey, PhpValue)> = Vec::with_capacity(base.len() + over.len());
    let mut next = 0;
    for (key, value) in base.into_iter().chain(over) {
        match key {
            PhpKey::Int(_) => {
                out.push((PhpKey::Int(next), value));
                next += 1;
            }
            PhpKey::Str(_) => match out.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 = value,
                None => out.push((key, value)),
            },
        }
    }
    out
}

/// Whether PHP turns this string array key into an int key (`"10"` becomes `10`; `"010"`, `"-0"` and `"1e3"` stay).
pub(crate) fn is_integer_key(bytes: &[u8]) -> bool {
    let digits = bytes.strip_prefix(b"-").unwrap_or(bytes);
    let canonical = match digits {
        [b'0'] => bytes.len() == 1,
        [b'1'..=b'9', rest @ ..] => rest.iter().all(u8::is_ascii_digit),
        _ => false,
    };
    canonical
        && std::str::from_utf8(bytes)
            .ok()
            .and_then(|s| s.parse::<i64>().ok())
            .is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> PhpValue {
        PhpValue::Str(v.as_bytes().to_vec())
    }

    fn k(v: &str) -> PhpKey {
        PhpKey::Str(v.as_bytes().to_vec())
    }

    #[test]
    fn empty_follows_php() {
        for v in [
            PhpValue::Null,
            PhpValue::Bool(false),
            PhpValue::Int(0),
            PhpValue::Float(0.0),
            s(""),
            s("0"),
            PhpValue::Array(vec![]),
        ] {
            assert!(is_empty(&v), "{v:?}");
        }
        for v in [
            PhpValue::Bool(true),
            PhpValue::Int(-1),
            s("00"),
            s(" "),
            s("a"),
            PhpValue::Float(0.1),
        ] {
            assert!(!is_empty(&v), "{v:?}");
        }
    }

    #[test]
    fn merge_replaces_in_place_and_renumbers_ints() {
        let base = vec![(k("a"), s("1")), (k("b"), s("2"))];
        let over = vec![(PhpKey::Int(7), s("x")), (k("b"), s("3")), (k("c"), s("4"))];
        assert_eq!(
            array_merge(base, over),
            vec![
                (k("a"), s("1")),
                (k("b"), s("3")),
                (PhpKey::Int(0), s("x")),
                (k("c"), s("4"))
            ]
        );
    }

    #[test]
    fn integer_keys_follow_php() {
        for yes in ["0", "10", "-5", "9223372036854775807"] {
            assert!(is_integer_key(yes.as_bytes()), "{yes}");
        }
        for no in [
            "",
            "-0",
            "010",
            "1e3",
            " 1",
            "1.5",
            "9223372036854775808",
            "color",
        ] {
            assert!(!is_integer_key(no.as_bytes()), "{no}");
        }
    }
}
