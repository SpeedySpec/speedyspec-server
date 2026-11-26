//! PHP array keys and hook names.
//!
//! Priorities and hook names are PHP array keys: `"5"` is the integer key 5, `" 7"` and `"abc"` stay strings, a float
//! is truncated. A hook name carries PHP's own hash, so Rust and PHP look a hook up in the same table.

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
