//! `wp_rand()` and `wp_generate_password()`.

use std::rc::Rc;

use speedyspec_hooks::Value;
use speedyspec_wp_crypto_contracts::RandomBytes;
use speedyspec_wp_password_contracts::{
    PasswordError, PasswordFilters, PasswordGenerator, RandomNumber,
};

use crate::algorithm::php_long;

/// A uniform random integer in `min..=max`, as PHP's `random_int()` draws it: rejection sampling over 32-bit draws
/// when the range fits, 64-bit otherwise, so no value is more likely than another.
pub fn uniform(random: &dyn RandomBytes, min: i64, max: i64) -> i64 {
    let span = (max as u64).wrapping_sub(min as u64);
    let offset = if span <= u64::from(u32::MAX) {
        u64::from(draw32(random, span as u32))
    } else {
        draw64(random, span)
    };
    min.wrapping_add(offset as i64)
}

fn draw32(random: &dyn RandomBytes, span: u32) -> u32 {
    let next = || {
        let mut bytes = [0u8; 4];
        random.fill(&mut bytes);
        u32::from_le_bytes(bytes)
    };
    if span == u32::MAX {
        return next();
    }
    let size = span + 1;
    if size & span == 0 {
        return next() & span;
    }
    let limit = u32::MAX - (u32::MAX % size) - 1;
    loop {
        let draw = next();
        if draw <= limit {
            return draw % size;
        }
    }
}

fn draw64(random: &dyn RandomBytes, span: u64) -> u64 {
    let next = || {
        let mut bytes = [0u8; 8];
        random.fill(&mut bytes);
        u64::from_le_bytes(bytes)
    };
    if span == u64::MAX {
        return next();
    }
    let size = span + 1;
    if size & span == 0 {
        return next() & span;
    }
    let limit = u64::MAX - (u64::MAX % size) - 1;
    loop {
        let draw = next();
        if draw <= limit {
            return draw % size;
        }
    }
}

/// WordPress's `wp_rand()`, after its `null` defaults and `(int)` casts: arguments in either order, a CSPRNG draw,
/// then `absint()`. WordPress's md5/sha1 fallback for a missing CSPRNG is gone: the runtime always has one.
pub struct DefaultRandomNumber {
    random: Rc<dyn RandomBytes>,
}

impl DefaultRandomNumber {
    pub fn new(random: Rc<dyn RandomBytes>) -> DefaultRandomNumber {
        DefaultRandomNumber { random }
    }
}

impl RandomNumber for DefaultRandomNumber {
    fn rand(&self, min: i64, max: i64) -> Result<Value, PasswordError> {
        let value = uniform(&*self.random, min.min(max), min.max(max));
        // absint(): PHP's abs() of PHP_INT_MIN is a float.
        Ok(match value.checked_abs() {
            Some(n) => Value::Int(n),
            None => Value::Float(-(value as f64)),
        })
    }
}

/// WordPress's letters and digits.
pub const CHARS: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
/// Added when `$special_chars` is true.
pub const SPECIAL: &str = "!@#$%^&*()";
/// Added when `$extra_special_chars` is true.
pub const EXTRA_SPECIAL: &str = "-_ []{}<>~`+=,.;:/?|";

/// Resolves the `wp_rand()` bound now, so a replacement is honoured.
pub type RandomNumberResolver = Rc<dyn Fn() -> Result<Rc<dyn RandomNumber>, PasswordError>>;

/// WordPress's `wp_generate_password()`: each character drawn through whatever `wp_rand()` is bound.
pub struct DefaultPasswordGenerator {
    numbers: RandomNumberResolver,
    filters: Rc<dyn PasswordFilters>,
}

impl DefaultPasswordGenerator {
    pub fn new(
        numbers: RandomNumberResolver,
        filters: Rc<dyn PasswordFilters>,
    ) -> DefaultPasswordGenerator {
        DefaultPasswordGenerator { numbers, filters }
    }
}

impl PasswordGenerator for DefaultPasswordGenerator {
    fn generate(
        &self,
        length: &Value,
        special: &Value,
        extra: &Value,
    ) -> Result<Value, PasswordError> {
        let mut chars = CHARS.to_string();
        if php_truthy(special) {
            chars.push_str(SPECIAL);
        }
        if php_truthy(extra) {
            chars.push_str(EXTRA_SPECIAL);
        }
        let chars = chars.as_bytes();
        let mut password = Vec::new();
        let count = iterations(length);
        if count > 0 {
            let numbers = (self.numbers)()?;
            for _ in 0..count {
                let index = php_long(&numbers.rand(0, chars.len() as i64 - 1)?);
                password.extend(substr_one(chars, index));
            }
        }
        self.filters
            .random_password(password, length, special, extra)
    }
}

/// `substr( $chars, $index, 1 )`: a negative index counts from the end; out of range is empty.
fn substr_one(chars: &[u8], index: i64) -> Option<u8> {
    let len = chars.len() as i64;
    let at = if index < 0 { len + index } else { index };
    (0..len).contains(&at).then(|| chars[at as usize])
}

/// How many times `for ( $i = 0; $i < $length; $i++ )` runs: integers and numeric strings as numbers, floats
/// rounded up, `true` once. Values PHP would compare as always greater (non-numeric strings, arrays, infinity) would
/// never stop there; SpeedySpec runs zero times instead.
fn iterations(length: &Value) -> u64 {
    let from_float = |f: f64| {
        if f.is_finite() && f > 0.0 {
            f.ceil() as u64
        } else {
            0
        }
    };
    match &*length.read() {
        Value::Int(n) => (*n).max(0) as u64,
        Value::Float(f) => from_float(*f),
        Value::Bool(b) => u64::from(*b),
        Value::Str(s) => numeric(s.as_bytes()).map_or(0, from_float),
        Value::Bytes(b) => numeric(b).map_or(0, from_float),
        Value::Null | Value::Array(_) | Value::Opaque(_) => 0,
    }
}

/// A PHP numeric string's value (whitespace around it allowed), or `None`.
fn numeric(text: &[u8]) -> Option<f64> {
    std::str::from_utf8(text).ok()?.trim().parse().ok()
}

/// PHP's truth: `null`, `false`, `0`, `0.0`, `""`, `"0"` and `[]` are false.
pub fn php_truthy(value: &Value) -> bool {
    match &*value.read() {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Int(n) => *n != 0,
        Value::Float(f) => *f != 0.0,
        Value::Str(s) => !(s.is_empty() || s == "0"),
        Value::Bytes(b) => !(b.is_empty() || b == b"0"),
        Value::Array(a) => !a.is_empty(),
        Value::Opaque(_) => true,
    }
}
