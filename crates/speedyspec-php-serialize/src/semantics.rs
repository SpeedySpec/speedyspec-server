//! PHP value semantics the WordPress ports lean on: `empty()`, array reads and writes, `array_merge()`, integer
//! keys, the `(array)` and `(string)` casts and PHP 8's loose `==`.

use crate::{PhpKey, PhpValue};

/// PHP's `empty()`: null, false, 0, 0.0, `""`, `"0"` and `array()` are empty.
pub fn is_empty(value: &PhpValue) -> bool {
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
///
/// `name` must not be integer-like (`"5"`): PHP stores such a key as an int, and this matches string keys only.
pub fn get<'a>(items: &'a [(PhpKey, PhpValue)], name: &str) -> Option<&'a PhpValue> {
    items
        .iter()
        .find(|(k, _)| matches!(k, PhpKey::Str(s) if s.as_slice() == name.as_bytes()))
        .map(|(_, v)| v)
}

/// `isset( $array['name'] )`: present and not null.
///
/// `name` must not be integer-like (`"5"`): PHP stores such a key as an int, and this matches string keys only.
pub fn isset<'a>(items: &'a [(PhpKey, PhpValue)], name: &str) -> Option<&'a PhpValue> {
    get(items, name).filter(|v| !matches!(v, PhpValue::Null))
}

/// `$array['name'] = $value`: replaces in place, or appends.
///
/// `name` must not be integer-like (`"5"`): PHP stores such a key as an int, and this matches string keys only,
/// so it would append a string key beside PHP's int one.
pub fn set(items: &mut Vec<(PhpKey, PhpValue)>, name: &str, value: PhpValue) {
    let key = PhpKey::Str(name.as_bytes().to_vec());
    match items.iter_mut().find(|(k, _)| *k == key) {
        Some(slot) => slot.1 = value,
        None => items.push((key, value)),
    }
}

/// `unset( $array['name'] )`: drops every entry with that string key.
///
/// `name` must not be integer-like (`"5"`): PHP stores such a key as an int, and this matches string keys only.
pub fn unset(items: &mut Vec<(PhpKey, PhpValue)>, name: &str) {
    items.retain(|(k, _)| !matches!(k, PhpKey::Str(s) if s.as_slice() == name.as_bytes()));
}

/// `array_merge( $base, $over )`: string keys of `$over` replace in place or append; int keys are renumbered from
/// 0 across both arrays.
pub fn array_merge(
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
pub fn is_integer_key(bytes: &[u8]) -> bool {
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

/// `$array[ $value ]`'s key: null is `""`, a bool `0` or `1`, a float truncated toward zero (a finite float outside
/// the int range wraps modulo 2^64, as PHP 8.5's `zend_dval_to_lval()` does: `1e20` is `7766279631452241920`,
/// `2^63` is `i64::MIN`; NaN and infinities are `0`), an integer-like string an int; `None` for an array or object,
/// where PHP throws "Illegal offset type".
pub fn to_array_key(value: &PhpValue) -> Option<PhpKey> {
    match value {
        PhpValue::Null => Some(PhpKey::Str(Vec::new())),
        PhpValue::Bool(b) => Some(PhpKey::Int(i64::from(*b))),
        PhpValue::Int(i) => Some(PhpKey::Int(*i)),
        PhpValue::Float(f) if !f.is_finite() => Some(PhpKey::Int(0)),
        PhpValue::Float(f) => {
            // `%` is exact, so the remainder is an integer in (-2^64, 2^64): i128 holds it, and `as i64` wraps it.
            let wrapped = f.trunc() % 18_446_744_073_709_551_616.0;
            Some(PhpKey::Int(wrapped as i128 as i64))
        }
        PhpValue::Str(s) if is_integer_key(s) => std::str::from_utf8(s)
            .ok()
            .and_then(|s| s.parse().ok())
            .map(PhpKey::Int),
        PhpValue::Str(s) => Some(PhpKey::Str(s.clone())),
        _ => None,
    }
}

/// An array key as text: an int in decimal, a string as it is (lossily, when it is not UTF-8).
pub fn key_to_string(key: &PhpKey) -> String {
    match key {
        PhpKey::Int(i) => i.to_string(),
        PhpKey::Str(s) => String::from_utf8_lossy(s).into_owned(),
    }
}

/// `(array) $value`: an array as it is, an object's properties (integer-like names as int keys), an enum as its
/// `name` alone (no `value`, even for a backed enum: [`PhpValue::Enum`] carries no value), null as `array()`, and any
/// other value as `array( 0 => $value )`.
pub fn to_array(value: PhpValue) -> Vec<(PhpKey, PhpValue)> {
    match value {
        PhpValue::Null => Vec::new(),
        PhpValue::Array(items) => items,
        // PHP 7.2+: canonical integer property names become int keys.
        PhpValue::Object { properties, .. } => properties
            .into_iter()
            .map(|(key, value)| match key {
                PhpKey::Str(name) if is_integer_key(&name) => {
                    let int = std::str::from_utf8(&name)
                        .ok()
                        .and_then(|s| s.parse().ok())
                        .expect("an integer key parses");
                    (PhpKey::Int(int), value)
                }
                key => (key, value),
            })
            .collect(),
        PhpValue::Enum { case, .. } => vec![(
            PhpKey::Str(b"name".to_vec()),
            PhpValue::Str(case.into_bytes()),
        )],
        other => vec![(PhpKey::Int(0), other)],
    }
}

/// `(string) $value`: null and false `""`, true `"1"`, ints in decimal, floats as PHP 8 prints them (`precision`
/// 14), arrays `"Array"`. Objects have no `__toString()` here; they print their class name.
pub fn to_php_string(value: &PhpValue) -> Vec<u8> {
    match value {
        PhpValue::Null | PhpValue::Bool(false) => Vec::new(),
        PhpValue::Bool(true) => b"1".to_vec(),
        PhpValue::Int(i) => i.to_string().into_bytes(),
        PhpValue::Float(f) => float_to_string(*f).into_bytes(),
        PhpValue::Str(s) => s.clone(),
        PhpValue::Array(_) => b"Array".to_vec(),
        PhpValue::Object { class, .. }
        | PhpValue::Custom { class, .. }
        | PhpValue::Enum { class, .. } => class.clone().into_bytes(),
        PhpValue::Ref(_) | PhpValue::ObjectRef(_) => Vec::new(),
    }
}

/// A float as `(string)` and `echo` write it: `%.14G` with PHP's `1.0E+25` exponent form.
fn float_to_string(f: f64) -> String {
    const PRECISION: i32 = 14;
    if f.is_nan() {
        return "NAN".into();
    }
    if f.is_infinite() {
        return if f > 0.0 { "INF".into() } else { "-INF".into() };
    }
    if f == 0.0 {
        return if f.is_sign_negative() {
            "-0".into()
        } else {
            "0".into()
        };
    }
    let sci = format!("{:.*e}", (PRECISION - 1) as usize, f.abs());
    let (mantissa, exp) = sci.split_once('e').expect("{:e} has an exponent");
    let exp: i32 = exp.parse().expect("an integer exponent");
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let sign = if f < 0.0 { "-" } else { "" };
    let n = digits.len() as i32;
    if !(-4..PRECISION).contains(&exp) {
        let frac = if n > 1 { &digits[1..] } else { "0" };
        return format!(
            "{sign}{}.{frac}E{}{}",
            &digits[..1],
            if exp < 0 { '-' } else { '+' },
            exp.abs()
        );
    }
    let body = if exp >= 0 {
        if n > exp + 1 {
            format!(
                "{}.{}",
                &digits[..(exp + 1) as usize],
                &digits[(exp + 1) as usize..]
            )
        } else {
            format!("{digits}{}", "0".repeat((exp + 1 - n) as usize))
        }
    } else {
        format!("0.{}{digits}", "0".repeat((-exp - 1) as usize))
    };
    format!("{sign}{body}")
}

/// A numeric string's value. `overflow` marks an integer string too large for an int (its sign).
#[derive(Clone, Copy)]
enum Numeric {
    Int(i64),
    Float { value: f64, overflow: i8 },
}

impl Numeric {
    fn as_f64(self) -> f64 {
        match self {
            Numeric::Int(i) => i as f64,
            Numeric::Float { value, .. } => value,
        }
    }
}

/// PHP 8's `is_numeric()` reading: leading and trailing whitespace, a sign, digits with an optional fraction and
/// exponent. No hex, no octal, no binary.
fn numeric(bytes: &[u8]) -> Option<Numeric> {
    let ws = |b: &u8| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | b'\x0b' | b'\x0c');
    let start = bytes.iter().position(|b| !ws(b))?;
    let end = bytes.iter().rposition(|b| !ws(b))? + 1;
    let s = &bytes[start..end];
    let mut i = 0;
    if matches!(s.first(), Some(b'+' | b'-')) {
        i += 1;
    }
    let int_digits = s[i..].iter().take_while(|b| b.is_ascii_digit()).count();
    i += int_digits;
    let mut frac_digits = 0;
    let mut is_float = false;
    if s.get(i) == Some(&b'.') {
        is_float = true;
        i += 1;
        frac_digits = s[i..].iter().take_while(|b| b.is_ascii_digit()).count();
        i += frac_digits;
    }
    if int_digits + frac_digits == 0 {
        return None;
    }
    if matches!(s.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1;
        if matches!(s.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        let exp_digits = s[j..].iter().take_while(|b| b.is_ascii_digit()).count();
        if exp_digits > 0 {
            is_float = true;
            i = j + exp_digits;
        }
    }
    if i != s.len() {
        return None;
    }
    let text = std::str::from_utf8(s).ok()?;
    if !is_float {
        if let Ok(int) = text.parse::<i64>() {
            return Some(Numeric::Int(int));
        }
        let value: f64 = text.parse().ok()?;
        let overflow = if value < 0.0 { -1 } else { 1 };
        return Some(Numeric::Float { value, overflow });
    }
    let value: f64 = text.parse().ok()?;
    Some(Numeric::Float { value, overflow: 0 })
}

/// PHP's truthiness, `(bool) $value`.
fn truthy(value: &PhpValue) -> bool {
    !is_empty(value)
}

/// A number's value, when it is one.
fn number(value: &PhpValue) -> Option<Numeric> {
    match value {
        PhpValue::Int(i) => Some(Numeric::Int(*i)),
        PhpValue::Float(f) => Some(Numeric::Float {
            value: *f,
            overflow: 0,
        }),
        _ => None,
    }
}

fn numbers_eq(a: Numeric, b: Numeric) -> bool {
    match (a, b) {
        (Numeric::Int(x), Numeric::Int(y)) => x == y,
        _ => a.as_f64() == b.as_f64(),
    }
}

/// Two strings under `==`: numerically when both are numeric, else byte for byte (`zendi_smart_str_equals()`).
fn strings_eq(a: &[u8], b: &[u8]) -> bool {
    let (Some(x), Some(y)) = (numeric(a), numeric(b)) else {
        return a == b;
    };
    match (x, y) {
        (Numeric::Int(x), Numeric::Int(y)) => x == y,
        (
            Numeric::Float {
                value: v1,
                overflow: o1,
            },
            Numeric::Float {
                value: v2,
                overflow: o2,
            },
        ) => {
            // Two integer strings past the int range, or two infinities, compare as strings.
            if (o1 != 0 && o1 == o2 && v1 == v2) || (v1 == v2 && !v1.is_finite()) {
                a == b
            } else {
                v1 == v2
            }
        }
        (Numeric::Int(_), Numeric::Float { overflow, .. })
        | (Numeric::Float { overflow, .. }, Numeric::Int(_))
            if overflow != 0 =>
        {
            false
        }
        _ => x.as_f64() == y.as_f64(),
    }
}

/// Key sets equal, and each value loosely equal to the same key's value.
fn hashes_eq(a: &[(PhpKey, PhpValue)], b: &[(PhpKey, PhpValue)]) -> bool {
    a.len() == b.len()
        && a.iter().all(|(key, value)| {
            b.iter()
                .find(|(k, _)| k == key)
                .is_some_and(|(_, other)| loose_eq(value, other))
        })
}

/// PHP 8's `$a == $b`.
///
/// A bool or null against anything compares truthiness, except null against a string, which compares with `""`;
/// numbers against numbers numerically; a number against a numeric string numerically, else the number's string
/// against the string; two numeric strings numerically, else their bytes; arrays by equal key sets with loosely
/// equal values; objects by class and properties. Anything else is not equal.
pub fn loose_eq(a: &PhpValue, b: &PhpValue) -> bool {
    use PhpValue::{Array, Bool, Enum, Null, Object, Str};
    match (a, b) {
        (Null, Null) => true,
        (Null, Str(s)) | (Str(s), Null) => s.is_empty(),
        (Null | Bool(_), _) | (_, Null | Bool(_)) => truthy(a) == truthy(b),
        (Str(x), Str(y)) => strings_eq(x, y),
        (Str(s), n) | (n, Str(s)) if number(n).is_some() => {
            let n = number(n).expect("checked");
            // NaN equals no string, numeric or not (PHP 8.5).
            if matches!(n, Numeric::Float { value, .. } if value.is_nan()) {
                return false;
            }
            // `compare_longs_to_string()` has no overflow rule: an overflowing integer string compares as a double.
            match numeric(s) {
                Some(parsed) => numbers_eq(n, parsed),
                None => {
                    to_php_string(&match n {
                        Numeric::Int(i) => PhpValue::Int(i),
                        Numeric::Float { value, .. } => PhpValue::Float(value),
                    }) == *s
                }
            }
        }
        (x, y) if number(x).is_some() && number(y).is_some() => {
            numbers_eq(number(x).expect("checked"), number(y).expect("checked"))
        }
        (Array(x), Array(y)) => hashes_eq(x, y),
        (
            Object {
                class: c1,
                properties: p1,
            },
            Object {
                class: c2,
                properties: p2,
            },
        ) => c1 == c2 && hashes_eq(p1, p2),
        (Enum { .. }, Enum { .. }) => a == b,
        _ => false,
    }
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
    fn array_keys_convert_as_php_does() {
        use PhpValue::{Bool, Float, Int, Null};
        let cases = [
            (Null, Some(k(""))),
            (Bool(false), Some(PhpKey::Int(0))),
            (Bool(true), Some(PhpKey::Int(1))),
            (Int(-7), Some(PhpKey::Int(-7))),
            (Float(1.9), Some(PhpKey::Int(1))),
            (Float(-2.5), Some(PhpKey::Int(-2))),
            (Float(f64::NAN), Some(PhpKey::Int(0))),
            (Float(f64::INFINITY), Some(PhpKey::Int(0))),
            (Float(f64::NEG_INFINITY), Some(PhpKey::Int(0))),
            // PHP 8.5 wraps a finite float outside the int range modulo 2^64.
            (Float(1e20), Some(PhpKey::Int(7766279631452241920))),
            (Float(-1e20), Some(PhpKey::Int(-7766279631452241920))),
            (Float(9223372036854775808.0), Some(PhpKey::Int(i64::MIN))),
            (Float(1e19), Some(PhpKey::Int(-8446744073709551616))),
            (Float(18446744073709551616.0), Some(PhpKey::Int(0))),
            (Float(1.5e300), Some(PhpKey::Int(0))),
            (s("12"), Some(PhpKey::Int(12))),
            (s("012"), Some(k("012"))),
            (s("edit_book"), Some(k("edit_book"))),
            (PhpValue::Array(vec![]), None),
        ];
        for (value, want) in cases {
            assert_eq!(to_array_key(&value), want, "{value:?}");
        }
        assert_eq!(key_to_string(&PhpKey::Int(-3)), "-3");
        assert_eq!(key_to_string(&k("a")), "a");
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

    fn arr(items: Vec<(PhpKey, PhpValue)>) -> PhpValue {
        PhpValue::Array(items)
    }

    #[test]
    fn loose_eq_follows_php8() {
        use PhpValue::{Bool, Float, Int, Null};
        let t = Bool(true);
        let f = Bool(false);
        let cases: Vec<(PhpValue, PhpValue, bool)> = vec![
            (t.clone(), s("yes"), true),
            (Int(1), s("1"), true),
            (Int(1), s("01"), true),
            (s("1"), s("01"), true),
            (s("1e1"), s("10"), true),
            (s(" 1"), s("1"), true),
            (s("1 "), s("1"), true),
            (s("abc"), Int(0), false),
            (Int(0), s(""), false),
            (Null, f.clone(), true),
            (Null, s(""), true),
            (Null, s("0"), false),
            (Null, Int(0), true),
            (s("0"), f.clone(), true),
            (arr(vec![]), f.clone(), true),
            (
                arr(vec![(PhpKey::Int(0), Int(1))]),
                arr(vec![(PhpKey::Int(0), Int(1))]),
                true,
            ),
            (
                arr(vec![(k("a"), Int(1)), (k("b"), Int(2))]),
                arr(vec![(k("b"), Int(2)), (k("a"), s("1"))]),
                true,
            ),
            (arr(vec![(PhpKey::Int(0), Int(1))]), Int(1), false),
            (Float(1.0), s("1"), true),
            // Beyond the brief, as the static PHP 8 answers.
            (s("9223372036854775807"), s("9223372036854775808"), false),
            (s("1."), s("1"), true),
            (s(".5"), s("0.5"), true),
            (s("0x1"), s("1"), false),
            (Float(1.5), s("1.5 "), true),
            (Float(f64::INFINITY), s("INF"), true),
            (s("abc"), s("ABC"), false),
            (Int(1), Float(1.0), true),
            (Null, arr(vec![]), true),
            (t.clone(), arr(vec![]), false),
            // Fix round 1: an int against an overflowing integer string compares as doubles; NaN equals no string.
            (Int(i64::MAX), s("9223372036854775808"), true),
            (Int(i64::MIN), s("-9223372036854775809"), true),
            (Float(f64::NAN), s("NAN"), false),
            (Float(f64::NAN), s("abc"), false),
            (Float(f64::NAN), s("1"), false),
            (Float(f64::NAN), s(""), false),
        ];
        for (a, b, want) in cases {
            assert_eq!(loose_eq(&a, &b), want, "{a:?} == {b:?}");
            assert_eq!(loose_eq(&b, &a), want, "{b:?} == {a:?}");
        }
    }

    #[test]
    fn to_php_string_follows_php() {
        use PhpValue::{Bool, Float, Int, Null};
        let cases: Vec<(PhpValue, &str)> = vec![
            (Null, ""),
            (Bool(false), ""),
            (Bool(true), "1"),
            (Int(-42), "-42"),
            (s("x"), "x"),
            (Float(1.0), "1"),
            (Float(0.1), "0.1"),
            (Float(0.1 + 0.2), "0.3"),
            (Float(1.0 / 3.0), "0.33333333333333"),
            (Float(1e14), "1.0E+14"),
            (Float(1e13), "10000000000000"),
            (Float(123456789012345.0), "1.2345678901234E+14"),
            (Float(1.5e20), "1.5E+20"),
            (Float(1e-5), "1.0E-5"),
            (Float(0.0001), "0.0001"),
            (Float(-0.0), "-0"),
            (Float(-1.25), "-1.25"),
            (Float(2.5e-7), "2.5E-7"),
            (Float(f64::INFINITY), "INF"),
            (Float(f64::NEG_INFINITY), "-INF"),
            (Float(f64::NAN), "NAN"),
            (arr(vec![]), "Array"),
        ];
        for (v, want) in cases {
            assert_eq!(to_php_string(&v), want.as_bytes(), "{v:?}");
        }
    }

    #[test]
    fn to_array_follows_the_cast() {
        assert_eq!(to_array(PhpValue::Null), vec![]);
        assert_eq!(to_array(s("x")), vec![(PhpKey::Int(0), s("x"))]);
        let items = vec![(k("a"), PhpValue::Int(1))];
        assert_eq!(to_array(arr(items.clone())), items);
        let object = PhpValue::Object {
            class: "stdClass".into(),
            properties: items.clone(),
        };
        assert_eq!(to_array(object), items);
        // (array) on an object turns canonical integer property names into int keys, as PHP 7.2+ does.
        let object = PhpValue::Object {
            class: "stdClass".into(),
            properties: vec![
                (k("0"), s("a")),
                (k("10"), s("b")),
                (k("010"), s("c")),
                (k("x"), s("d")),
            ],
        };
        assert_eq!(
            to_array(object),
            vec![
                (PhpKey::Int(0), s("a")),
                (PhpKey::Int(10), s("b")),
                (k("010"), s("c")),
                (k("x"), s("d")),
            ]
        );
    }
}
