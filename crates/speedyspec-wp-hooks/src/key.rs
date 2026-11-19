//! PHP array keys, and the PHP 8 comparisons WordPress's hook code relies on.
//!
//! Priorities and hook names are PHP array keys: `"5"` is the integer key 5, `" 7"` and `"abc"` stay strings, a float
//! is truncated. WordPress sorts priorities with `ksort(SORT_NUMERIC)` ([`PriorityOrder`]) and compares them with
//! `<`, `min()` and `===` while re-positioning running iterations; each is reproduced here.

use std::cmp::Ordering;

use crate::{HookError, HookValue};

/// A PHP array key.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ArrayKey {
    Int(i64),
    Str(String),
}

impl ArrayKey {
    /// The key PHP uses for `$array[$value]`; arrays and objects are not keys.
    pub fn from_value(value: &HookValue) -> Result<ArrayKey, HookError> {
        Ok(match value {
            HookValue::Null => ArrayKey::Str(String::new()),
            HookValue::Bool(b) => ArrayKey::Int(i64::from(*b)),
            HookValue::Int(n) => ArrayKey::Int(*n),
            HookValue::Float(f) => ArrayKey::Int(float_to_key(*f)),
            HookValue::Str(s) => ArrayKey::from_str_key(s),
            HookValue::Bytes(b) => ArrayKey::from_str_key(&String::from_utf8_lossy(b)),
            HookValue::Array(_) => {
                return Err(HookError::message(
                    "Cannot access offset of type array on array",
                ));
            }
            HookValue::Opaque(_) => {
                return Err(HookError::message(
                    "Cannot access offset of type object on array",
                ));
            }
        })
    }

    /// A string key: an integer when it is the canonical decimal form of one (PHP's `ZEND_HANDLE_NUMERIC_STR`).
    pub fn from_str_key(s: &str) -> ArrayKey {
        let digits = s.strip_prefix('-').unwrap_or(s);
        let canonical = !digits.is_empty()
            && digits.bytes().all(|b| b.is_ascii_digit())
            && (digits == "0" || !digits.starts_with('0'))
            && s != "-0";
        match canonical.then(|| s.parse::<i64>().ok()).flatten() {
            Some(n) => ArrayKey::Int(n),
            None => ArrayKey::Str(s.to_string()),
        }
    }

    pub fn to_value(&self) -> HookValue {
        match self {
            ArrayKey::Int(n) => HookValue::Int(*n),
            ArrayKey::Str(s) => HookValue::Str(s.clone()),
        }
    }

    /// `$raw === $key`, for a value that was used as this key.
    pub fn identical(raw: &HookValue, key: &ArrayKey) -> bool {
        match (raw, key) {
            (HookValue::Int(a), ArrayKey::Int(b)) => a == b,
            (HookValue::Str(a), ArrayKey::Str(b)) => a == b,
            _ => false,
        }
    }
}

/// PHP's float-to-integer key conversion: truncation, and 0 when out of range or not finite.
fn float_to_key(f: f64) -> i64 {
    const LIMIT: f64 = 9_223_372_036_854_775_808.0;
    if f.is_finite() && (-LIMIT..LIMIT).contains(&f) {
        f.trunc() as i64
    } else {
        0
    }
}

/// A number parsed from a numeric string.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Number {
    Int(i64),
    Float(f64),
}

/// PHP 8's `is_numeric_string()`: optional surrounding whitespace, a sign, digits with an optional fraction and
/// exponent. `None` when the string is not numeric.
fn numeric(s: &str) -> Option<Number> {
    let trimmed =
        s.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{b}' | '\u{c}'));
    let body = trimmed;
    if body.is_empty() {
        return None;
    }
    let (len, integral) = number_prefix(body.as_bytes());
    if len != body.len() || len == 0 {
        return None;
    }
    if integral && let Ok(n) = body.parse::<i64>() {
        return Some(Number::Int(n));
    }
    body.parse::<f64>().ok().map(Number::Float)
}

/// The length of the longest number at the start of `b` (sign, digits, fraction, exponent), and whether it is
/// integral (no fraction or exponent).
fn number_prefix(b: &[u8]) -> (usize, bool) {
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let int_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let mut digits = i - int_start;
    let mut integral = true;
    if i < b.len() && b[i] == b'.' {
        let frac_start = i + 1;
        let mut j = frac_start;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if digits > 0 || j > frac_start {
            digits += j - frac_start;
            i = j;
            integral = false;
        }
    }
    if digits == 0 {
        return (0, true);
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        let exp_start = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > exp_start {
            i = j;
            integral = false;
        }
    }
    (i, integral)
}

/// `zend_strtod()` as `SORT_NUMERIC` uses it on a string key: leading whitespace, then the longest number, else 0.
fn strtod(s: &str) -> f64 {
    let body = s.trim_start_matches([' ', '\t', '\n', '\r', '\u{b}', '\u{c}']);
    let (len, _) = number_prefix(body.as_bytes());
    body[..len].parse::<f64>().unwrap_or(0.0)
}

fn threeway(a: f64, b: f64) -> Ordering {
    // ZEND_THREEWAY_COMPARE: equal, less, or else greater (NaN included).
    if a == b {
        Ordering::Equal
    } else if a < b {
        Ordering::Less
    } else {
        Ordering::Greater
    }
}

/// The order `ksort($callbacks, SORT_NUMERIC)` puts two priorities in (the sort is stable).
pub struct PriorityOrder;

impl PriorityOrder {
    pub fn compare(a: &ArrayKey, b: &ArrayKey) -> Ordering {
        match (a, b) {
            (ArrayKey::Int(x), ArrayKey::Int(y)) => {
                if x > y {
                    Ordering::Greater
                } else {
                    Ordering::Less
                }
            }
            _ => threeway(Self::number(a), Self::number(b)),
        }
    }

    fn number(key: &ArrayKey) -> f64 {
        match key {
            ArrayKey::Int(n) => *n as f64,
            ArrayKey::Str(s) => strtod(s),
        }
    }

    /// PHP 8's `$a <=> $b` between two keys (as `<` and `min()` use it).
    pub fn php_compare(a: &ArrayKey, b: &ArrayKey) -> Ordering {
        match (a, b) {
            (ArrayKey::Int(x), ArrayKey::Int(y)) => x.cmp(y),
            (ArrayKey::Int(x), ArrayKey::Str(s)) => compare_long_to_string(*x, s),
            (ArrayKey::Str(s), ArrayKey::Int(x)) => compare_long_to_string(*x, s).reverse(),
            (ArrayKey::Str(x), ArrayKey::Str(y)) => match (numeric(x), numeric(y)) {
                (Some(Number::Int(a)), Some(Number::Int(b))) => a.cmp(&b),
                (Some(a), Some(b)) => threeway(as_float(a), as_float(b)),
                _ => binary_strcmp(x, y),
            },
        }
    }

    pub fn php_less(a: &ArrayKey, b: &ArrayKey) -> bool {
        Self::php_compare(a, b) == Ordering::Less
    }

    /// PHP's `min()` of a list of keys: the first smallest.
    pub fn min(keys: &[ArrayKey]) -> Option<&ArrayKey> {
        let mut best = keys.first()?;
        for key in &keys[1..] {
            if Self::php_less(key, best) {
                best = key;
            }
        }
        Some(best)
    }
}

fn as_float(n: Number) -> f64 {
    match n {
        Number::Int(i) => i as f64,
        Number::Float(f) => f,
    }
}

fn compare_long_to_string(l: i64, s: &str) -> Ordering {
    match numeric(s) {
        Some(Number::Int(n)) => l.cmp(&n),
        Some(Number::Float(d)) => threeway(l as f64, d),
        None => binary_strcmp(&l.to_string(), s),
    }
}

fn binary_strcmp(a: &str, b: &str) -> Ordering {
    a.as_bytes().cmp(b.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn sort_numeric_reads_leading_numbers() {
        assert_eq!(
            PriorityOrder::compare(&k(" 7"), &ArrayKey::Int(10)),
            Ordering::Less
        );
        assert_eq!(
            PriorityOrder::compare(&k(" 7"), &ArrayKey::Int(5)),
            Ordering::Greater
        );
        assert_eq!(
            PriorityOrder::compare(&k("abc"), &ArrayKey::Int(0)),
            Ordering::Equal
        );
        assert_eq!(
            PriorityOrder::compare(&k("7abc"), &ArrayKey::Int(8)),
            Ordering::Less
        );
    }

    #[test]
    fn php8_comparisons() {
        assert!(PriorityOrder::php_less(&ArrayKey::Int(5), &k(" 7")));
        assert!(
            PriorityOrder::php_less(&ArrayKey::Int(5), &k("abc")),
            "\"5\" < \"abc\" as strings"
        );
        assert!(
            PriorityOrder::php_less(&k("10"), &k("9a")),
            "non-numeric: string comparison"
        );
        assert!(!PriorityOrder::php_less(&ArrayKey::Int(10), &k(" 9 ")));
        assert_eq!(
            PriorityOrder::min(&[ArrayKey::Int(3), k("abc"), ArrayKey::Int(-1)]),
            Some(&ArrayKey::Int(-1))
        );
    }
}
