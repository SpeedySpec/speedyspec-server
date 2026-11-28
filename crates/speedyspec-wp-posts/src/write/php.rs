//! The PHP array functions the write use cases apply to `$postarr` and `$data`, as pure functions on PHP values:
//! `wp_parse_args()`, `get_object_vars()`, `wp_slash()`, `wp_unslash()`, `array_filter()`, `wp_list_pluck()` and
//! `===` on arrays.

use speedyspec_php_serialize::semantics::{is_empty, to_array};
use speedyspec_php_serialize::{PhpKey, PhpValue};

/// A PHP array.
pub(crate) type Items = Vec<(PhpKey, PhpValue)>;

/// `$array['name']`.
pub(crate) fn at<'a>(items: &'a [(PhpKey, PhpValue)], name: &str) -> Option<&'a PhpValue> {
    speedyspec_php_serialize::semantics::get(items, name)
}

/// `$array['name'] ?? $default`.
pub(crate) fn or(items: &[(PhpKey, PhpValue)], name: &str, default: PhpValue) -> PhpValue {
    speedyspec_php_serialize::semantics::isset(items, name)
        .cloned()
        .unwrap_or(default)
}

/// `empty( $array['name'] )`.
pub(crate) fn empty_at(items: &[(PhpKey, PhpValue)], name: &str) -> bool {
    at(items, name).is_none_or(is_empty)
}

/// A string key.
pub(crate) fn key(name: &str) -> PhpKey {
    PhpKey::Str(name.as_bytes().to_vec())
}

/// A PHP string.
pub(crate) fn s(bytes: impl AsRef<[u8]>) -> PhpValue {
    PhpValue::Str(bytes.as_ref().to_vec())
}

/// `$value === 'text'`.
pub(crate) fn is_str(value: &PhpValue, text: &str) -> bool {
    matches!(value, PhpValue::Str(b) if b.as_slice() == text.as_bytes())
}

/// `$object->name` for an object value (`$post_type_object->cap->publish_posts`); `None` for a missing property or a
/// value that is not an object.
pub(crate) fn object_property(value: &PhpValue, name: &str) -> Option<PhpValue> {
    match value {
        PhpValue::Object { properties, .. } => at(properties, name).cloned(),
        _ => None,
    }
}

/// `urldecode()`: `+` is a space, `%XX` a byte, anything else as it is.
fn urldecode(bytes: &[u8]) -> Vec<u8> {
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                (Some(h), Some(l)) => {
                    out.push(h * 16 + l);
                    i += 2;
                }
                _ => out.push(b'%'),
            },
            b => out.push(b),
        }
        i += 1;
    }
    out
}

/// `parse_str( $string, $result )` for flat names: `a=1&b=2`, each name and value `urldecode()`d, a later name
/// replacing an earlier one.
///
/// A known divergence: names with brackets (`a[]=1`, `a[b]=1`) are kept as flat names, where PHP builds nested
/// arrays, and PHP's name mangling (spaces and dots to `_`) is not applied.
pub(crate) fn parse_str(text: &[u8]) -> Items {
    let mut items: Items = Vec::new();
    for pair in text.split(|&b| b == b'&') {
        if pair.is_empty() {
            continue;
        }
        let (name, value) = match pair.iter().position(|&b| b == b'=') {
            Some(at) => (&pair[..at], &pair[at + 1..]),
            None => (pair, &b""[..]),
        };
        let name = urldecode(name);
        if name.is_empty() {
            continue;
        }
        let k = speedyspec_php_serialize::semantics::to_array_key(&PhpValue::Str(name))
            .expect("a string is a key");
        let v = PhpValue::Str(urldecode(value));
        match items.iter_mut().find(|(ik, _)| *ik == k) {
            Some(slot) => slot.1 = v,
            None => items.push((k, v)),
        }
    }
    items
}

/// `addslashes()`: a backslash before `'`, `"`, `\` and NUL (which becomes `\0`).
fn addslashes(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    for &b in bytes {
        match b {
            b'\'' | b'"' | b'\\' => {
                out.push(b'\\');
                out.push(b);
            }
            0 => out.extend_from_slice(b"\\0"),
            b => out.push(b),
        }
    }
    out
}

/// `stripslashes()`: each backslash escapes the byte after it (`\0` is NUL); a lone trailing backslash is dropped.
fn stripslashes(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            if let Some(&next) = bytes.get(i + 1) {
                out.push(if next == b'0' { 0 } else { next });
            }
            i += 2;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

/// `wp_slash( $value )`: every string, at any depth of an array, through `addslashes()`; anything else as it is.
pub(crate) fn wp_slash(value: PhpValue) -> PhpValue {
    match value {
        PhpValue::Str(b) => PhpValue::Str(addslashes(&b)),
        PhpValue::Array(items) => {
            PhpValue::Array(items.into_iter().map(|(k, v)| (k, wp_slash(v))).collect())
        }
        other => other,
    }
}

/// `wp_unslash( $value )` (`stripslashes_deep()`, through `map_deep()`): every string in an array or an object's
/// properties through `stripslashes()`; anything else as it is.
pub(crate) fn wp_unslash(value: PhpValue) -> PhpValue {
    match value {
        PhpValue::Str(b) => PhpValue::Str(stripslashes(&b)),
        PhpValue::Array(items) => {
            PhpValue::Array(items.into_iter().map(|(k, v)| (k, wp_unslash(v))).collect())
        }
        PhpValue::Object { class, properties } => PhpValue::Object {
            class,
            properties: properties
                .into_iter()
                .map(|(k, v)| (k, wp_unslash(v)))
                .collect(),
        },
        other => other,
    }
}

/// `array_filter( $array )`: the entries PHP holds true, keys kept. A value that is not an array is returned as it
/// is (PHP 8 throws a `TypeError` there; WordPress never passes one).
pub(crate) fn array_filter(value: PhpValue) -> PhpValue {
    match value {
        PhpValue::Array(items) => {
            PhpValue::Array(items.into_iter().filter(|(_, v)| !is_empty(v)).collect())
        }
        other => other,
    }
}

/// `wp_list_pluck( $list, $field )`: each entry's field (an object's property or an array's key), keys kept;
/// entries without it are skipped.
pub(crate) fn list_pluck(list: &PhpValue, field: &str) -> PhpValue {
    let PhpValue::Array(items) = list else {
        return PhpValue::Array(Vec::new());
    };
    PhpValue::Array(
        items
            .iter()
            .filter_map(|(k, v)| {
                let fields = match v {
                    PhpValue::Object { properties, .. } => properties,
                    PhpValue::Array(items) => items,
                    _ => return None,
                };
                at(fields, field).map(|f| (k.clone(), f.clone()))
            })
            .collect(),
    )
}

/// PHP's `===`: same type and value; arrays with the same pairs in the same order, each `===`.
pub(crate) fn identical(a: &PhpValue, b: &PhpValue) -> bool {
    match (a, b) {
        (PhpValue::Array(x), PhpValue::Array(y)) => {
            x.len() == y.len()
                && x.iter()
                    .zip(y)
                    .all(|((ka, va), (kb, vb))| ka == kb && identical(va, vb))
        }
        (PhpValue::Float(x), PhpValue::Float(y)) => x == y,
        _ => a == b,
    }
}

/// `wp_parse_args( $args, $defaults )`: an object's properties, an array as it is, or a query string through
/// [`parse_str`] and the `wp_parse_str` filter (`filtered`), merged over the defaults with `array_merge()`.
pub(crate) fn wp_parse_args(
    args: PhpValue,
    defaults: Items,
    filtered: impl FnOnce(PhpValue) -> Result<PhpValue, speedyspec_wp_posts_contracts::PostError>,
) -> Result<Items, speedyspec_wp_posts_contracts::PostError> {
    let parsed = match args {
        PhpValue::Object { properties, .. } => properties,
        PhpValue::Array(items) => items,
        other => {
            let text = speedyspec_php_serialize::semantics::to_php_string(&other);
            to_array(filtered(PhpValue::Array(parse_str(&text)))?)
        }
    };
    Ok(speedyspec_php_serialize::semantics::array_merge(
        defaults, parsed,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slashes_round_trip() {
        let raw = b"it's \"q\" \\ \0".to_vec();
        let slashed = addslashes(&raw);
        assert_eq!(slashed, b"it\\'s \\\"q\\\" \\\\ \\0".to_vec());
        assert_eq!(stripslashes(&slashed), raw);
        assert_eq!(stripslashes(b"a\\"), b"a".to_vec());
    }

    #[test]
    fn query_strings_decode() {
        let items = parse_str(b"post_title=Hello+World&menu_order=%33&x");
        assert_eq!(at(&items, "post_title"), Some(&s("Hello World")));
        assert_eq!(at(&items, "menu_order"), Some(&s("3")));
        assert_eq!(at(&items, "x"), Some(&s("")));
    }

    #[test]
    fn identity_compares_types_and_order() {
        let a = PhpValue::Array(vec![(PhpKey::Int(0), s("a")), (PhpKey::Int(1), s("b"))]);
        let b = PhpValue::Array(vec![(PhpKey::Int(1), s("b")), (PhpKey::Int(0), s("a"))]);
        assert!(identical(&a, &a.clone()));
        assert!(!identical(&a, &b));
        assert!(!identical(&PhpValue::Int(1), &s("1")));
    }
}
