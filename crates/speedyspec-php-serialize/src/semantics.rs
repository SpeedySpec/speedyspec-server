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
pub fn get<'a>(items: &'a [(PhpKey, PhpValue)], name: &str) -> Option<&'a PhpValue> {
    items
        .iter()
        .find(|(k, _)| matches!(k, PhpKey::Str(s) if s.as_slice() == name.as_bytes()))
        .map(|(_, v)| v)
}

/// `isset( $array['name'] )`: present and not null.
pub fn isset<'a>(items: &'a [(PhpKey, PhpValue)], name: &str) -> Option<&'a PhpValue> {
    get(items, name).filter(|v| !matches!(v, PhpValue::Null))
}

/// `$array['name'] = $value`: replaces in place, or appends.
pub fn set(items: &mut Vec<(PhpKey, PhpValue)>, name: &str, value: PhpValue) {
    let key = PhpKey::Str(name.as_bytes().to_vec());
    match items.iter_mut().find(|(k, _)| *k == key) {
        Some(slot) => slot.1 = value,
        None => items.push((key, value)),
    }
}

/// `unset( $array['name'] )`.
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

/// `(array) $value`: an array as it is, an object's properties, an enum's `name` (and `value`), null as `array()`,
/// and any other value as `array( 0 => $value )`.
pub fn to_array(value: PhpValue) -> Vec<(PhpKey, PhpValue)> {
    match value {
        PhpValue::Null => Vec::new(),
        PhpValue::Array(items) => items,
        PhpValue::Object { properties, .. } => properties,
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
            match numeric(s) {
                Some(Numeric::Float { overflow, .. })
                    if overflow != 0 && matches!(n, Numeric::Int(_)) =>
                {
                    false
                }
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
    }
}
