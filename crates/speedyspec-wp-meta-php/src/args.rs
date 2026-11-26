//! What `meta.php` does to its arguments before any work: `is_numeric()`, `absint()`, `(int)`, `! $value` and the
//! string a value becomes, over the PHP values the native functions receive.

use speedyspec_php_ext::Value;
use speedyspec_php_serialize::PhpValue;
use speedyspec_php_serialize::semantics::{to_php_int, to_php_string};
use speedyspec_wp_meta_contracts::{MetaKey, MetaType};

pub(crate) use crate::adapters::truthy;

/// The `i`th argument, `null` when it was not passed.
pub(crate) fn a(args: &[Value], i: usize) -> Value {
    args.get(i).cloned().unwrap_or(Value::Null)
}

/// A scalar as `PhpValue`; arrays and objects become `null` (only their truthiness is ever asked).
fn scalar(value: &Value) -> PhpValue {
    match &*value.read() {
        Value::Bool(b) => PhpValue::Bool(*b),
        Value::Int(i) => PhpValue::Int(*i),
        Value::Float(f) => PhpValue::Float(*f),
        Value::Str(s) => PhpValue::Str(s.as_bytes().to_vec()),
        Value::Bytes(b) => PhpValue::Str(b.clone()),
        _ => PhpValue::Null,
    }
}

/// The string PHP makes of a scalar (`(string) $value`); an array is `"Array"`, as PHP prints it.
pub(crate) fn bytes(value: &Value) -> Vec<u8> {
    match &*value.read() {
        Value::Array(_) => b"Array".to_vec(),
        _ => to_php_string(&scalar(value)),
    }
}

/// `(string) $value` as text.
pub(crate) fn text(value: &Value) -> String {
    String::from_utf8_lossy(&bytes(value)).into_owned()
}

/// A meta key as given.
pub(crate) fn key(value: &Value) -> MetaKey {
    MetaKey::new(bytes(value))
}

/// `$meta_type` past `! $meta_type`; an array or object never names a type with a table.
pub(crate) fn meta_type(value: &Value) -> Option<MetaType> {
    if !truthy(value) || matches!(&*value.read(), Value::Array(_) | Value::Opaque(_)) {
        return None;
    }
    MetaType::new(&text(value))
}

/// PHP 8's numeric string: optional whitespace, a sign, digits with an optional fraction (or a fraction alone), an
/// optional exponent, optional trailing whitespace. Its value as a float.
fn numeric_string(s: &[u8]) -> Option<f64> {
    let ws = |b: &u8| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c);
    let start = s.iter().position(|b| !ws(b))?;
    let end = s.iter().rposition(|b| !ws(b))? + 1;
    let t = &s[start..end];
    let mut i = 0;
    if matches!(t.first(), Some(b'+' | b'-')) {
        i += 1;
    }
    let int_digits = t[i..].iter().take_while(|b| b.is_ascii_digit()).count();
    i += int_digits;
    let mut frac_digits = 0;
    if t.get(i) == Some(&b'.') {
        i += 1;
        frac_digits = t[i..].iter().take_while(|b| b.is_ascii_digit()).count();
        i += frac_digits;
    }
    if int_digits + frac_digits == 0 {
        return None;
    }
    if matches!(t.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1;
        if matches!(t.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        let exp_digits = t[j..].iter().take_while(|b| b.is_ascii_digit()).count();
        if exp_digits == 0 {
            return None;
        }
        i = j + exp_digits;
    }
    if i != t.len() {
        return None;
    }
    std::str::from_utf8(t).ok()?.parse().ok()
}

/// `is_numeric( $value )`, with the number when it is one.
pub(crate) fn numeric(value: &Value) -> Option<f64> {
    match &*value.read() {
        Value::Int(i) => Some(*i as f64),
        Value::Float(f) => Some(*f),
        Value::Str(s) => numeric_string(s.as_bytes()),
        Value::Bytes(b) => numeric_string(b),
        _ => None,
    }
}

/// `(int) $value`.
pub(crate) fn int(value: &Value) -> i64 {
    match &*value.read() {
        Value::Array(items) => i64::from(!items.is_empty()),
        Value::Opaque(_) => 1,
        _ => to_php_int(&scalar(value)),
    }
}

/// `absint( $value )`.
pub(crate) fn absint(value: &Value) -> u64 {
    int(value).unsigned_abs()
}

/// A meta id past `! is_numeric( $meta_id ) || floor( $meta_id ) != $meta_id` and `(int) $meta_id <= 0`.
pub(crate) fn meta_id(value: &Value) -> Option<u64> {
    let n = numeric(value)?;
    if n.floor() != n {
        return None;
    }
    u64::try_from(int(value)).ok().filter(|id| *id > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> Value {
        Value::Str(v.into())
    }

    #[test]
    fn numeric_strings_as_php_8() {
        for yes in ["1", " 1", "1 ", "-1.5", ".5", "1.", "1e3", "+2", "0"] {
            assert!(numeric(&s(yes)).is_some(), "{yes}");
        }
        for no in ["", " ", "1a", "0x1A", ".", "e3", "1e", "- 1", "abc"] {
            assert!(numeric(&s(no)).is_none(), "{no}");
        }
        assert!(numeric(&Value::Bool(true)).is_none());
        assert!(numeric(&Value::Null).is_none());
    }

    #[test]
    fn meta_ids_are_whole_and_positive() {
        assert_eq!(meta_id(&s("3")), Some(3));
        assert_eq!(meta_id(&Value::Float(2.0)), Some(2));
        assert_eq!(meta_id(&s("1.5")), None);
        assert_eq!(meta_id(&Value::Int(0)), None);
        assert_eq!(meta_id(&Value::Int(-4)), None);
        assert_eq!(meta_id(&s("x")), None);
    }

    #[test]
    fn absint_and_keys() {
        assert_eq!(absint(&s("-7")), 7);
        assert_eq!(absint(&s("12abc")), 12);
        assert_eq!(bytes(&Value::Int(10)), b"10");
        assert!(meta_type(&s("")).is_none());
        assert!(meta_type(&s("0")).is_none());
        assert_eq!(meta_type(&s("post")).unwrap().name(), "post");
    }
}
