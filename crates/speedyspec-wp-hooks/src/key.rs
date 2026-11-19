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

/// A hook name as the hook store keys it: the array key and its hash, PHP's own (`zend_string`'s `h`, or the integer
/// itself for an integer key).
///
/// A name that comes from PHP carries the hash PHP already computed ([`HookKey::prehashed`]); only a name made in
/// Rust is hashed here ([`HookKey::new`], the same DJBX33A PHP uses), so both look a hook up in the same table.
#[derive(Clone, Debug)]
pub struct HookKey {
    key: ArrayKey,
    hash: u64,
}

impl HookKey {
    /// Hashes `key` as PHP does.
    pub fn new(key: ArrayKey) -> HookKey {
        let hash = match &key {
            ArrayKey::Int(n) => *n as u64,
            ArrayKey::Str(s) => php_string_hash(s.as_bytes()),
        };
        HookKey { key, hash }
    }

    /// A key whose hash PHP already computed. `hash` must be PHP's hash of `key` (for an integer key, the integer).
    pub fn prehashed(key: ArrayKey, hash: u64) -> HookKey {
        HookKey { key, hash }
    }

    pub fn key(&self) -> &ArrayKey {
        &self.key
    }

    pub fn into_key(self) -> ArrayKey {
        self.key
    }

    pub fn hash(&self) -> u64 {
        self.hash
    }
}

impl PartialEq for HookKey {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash && self.key == other.key
    }
}

impl Eq for HookKey {}

impl std::hash::Hash for HookKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u64(self.hash);
    }
}

/// `zend_inline_hash_func()`: DJBX33A, with the high bit set so a string's hash is never 0.
pub fn php_string_hash(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 5381;
    for &b in bytes {
        hash = hash.wrapping_mul(33).wrapping_add(u64::from(b));
    }
    hash | 0x8000_0000_0000_0000
}

/// A hasher that takes a [`HookKey`]'s hash as it is, so a lookup never hashes the name again.
#[derive(Default)]
pub struct PrehashedHasher(u64);

impl std::hash::Hasher for PrehashedHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        // Only `write_u64` is expected; anything else is folded in so the hasher stays correct.
        for &b in bytes {
            self.0 = self.0.wrapping_mul(33).wrapping_add(u64::from(b));
        }
    }

    fn write_u64(&mut self, n: u64) {
        self.0 = n;
    }
}

/// The build hasher of maps keyed by [`HookKey`].
pub type Prehashed = std::hash::BuildHasherDefault<PrehashedHasher>;

/// A hook name as the Hook API gets it: the value WordPress passed (pushed on `$wp_current_filter`, shown to
/// deprecation notices) and its store key, or the error PHP would raise using it as an array key.
#[derive(Clone, Debug)]
pub struct HookName {
    value: HookValue,
    key: Result<HookKey, HookError>,
}

impl HookName {
    /// A name made in Rust: its key is hashed here.
    pub fn new(value: HookValue) -> HookName {
        let key = ArrayKey::from_value(&value).map(HookKey::new);
        HookName { value, key }
    }

    /// A name whose key (and hash) PHP already computed.
    pub fn prehashed(value: HookValue, key: HookKey) -> HookName {
        HookName {
            value,
            key: Ok(key),
        }
    }

    pub fn value(&self) -> &HookValue {
        &self.value
    }

    /// The store key, or PHP's error for a name that cannot be an array key.
    pub fn key(&self) -> Result<&HookKey, HookError> {
        self.key.as_ref().map_err(Clone::clone)
    }
}

impl From<HookValue> for HookName {
    fn from(value: HookValue) -> HookName {
        HookName::new(value)
    }
}

impl From<&str> for HookName {
    fn from(name: &str) -> HookName {
        HookName::new(HookValue::Str(name.to_owned()))
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
