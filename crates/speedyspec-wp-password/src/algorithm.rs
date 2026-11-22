//! The algorithm and options WordPress's filters return, read as PHP's `password_hash()` reads its arguments.
//!
//! Filters return any PHP value, so these mirror PHP's own coercions and errors: the argument types are checked
//! first (a `TypeError`), then the algorithm is looked up (a `ValueError`); option values coerce like
//! `zval_get_long()`. Each strategy validates the options it uses. Values a bridge carries unconverted are read
//! ([`Value::read`]) here, where they are interpreted, and nowhere earlier.

use speedyspec_hooks::{Key, Value};
use speedyspec_wp_password_contracts::PasswordError;

/// A password hashing algorithm, as `password_hash()` knows them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Algorithm {
    /// `PASSWORD_BCRYPT` (`'2y'`).
    Bcrypt,
    /// `PASSWORD_ARGON2I` (`'argon2i'`).
    Argon2i,
    /// `PASSWORD_ARGON2ID` (`'argon2id'`).
    Argon2id,
}

impl Algorithm {
    /// The algorithm's identifier, as `password_get_info()` names it.
    pub fn id(self) -> &'static str {
        match self {
            Algorithm::Bcrypt => "2y",
            Algorithm::Argon2i => "argon2i",
            Algorithm::Argon2id => "argon2id",
        }
    }
}

/// Whether WordPress takes its own bcrypt path: the filtered algorithm is strictly `PASSWORD_BCRYPT`
/// (`PASSWORD_BCRYPT !== $algorithm` is false).
pub fn is_wp_bcrypt(algorithm: &Value) -> bool {
    matches!(&*algorithm.read(), Value::Str(s) if s == "2y")
}

/// PHP's name for a value's type in its error messages.
pub fn php_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(true) => "true",
        Value::Bool(false) => "false",
        Value::Int(_) => "int",
        Value::Float(_) => "float",
        Value::Str(_) | Value::Bytes(_) => "string",
        Value::Array(_) => "array",
        Value::Opaque(_) => "object",
    }
}

/// The `TypeError` `function()` raises for an algorithm that is not `string|int|null`, if any.
pub fn algorithm_type_error(algorithm: &Value, function: &str) -> Option<PasswordError> {
    let algorithm = &*algorithm.read();
    matches!(algorithm, Value::Array(_) | Value::Opaque(_)).then(|| {
        PasswordError::type_error(format!(
            "{function}(): Argument #2 ($algo) must be of type string|int|null, {} given",
            php_type_name(algorithm)
        ))
    })
}

/// The algorithm `function()` (`password_hash` or `password_needs_rehash`) runs for `value`.
///
/// `null` is the default (bcrypt); booleans and floats coerce to integers, which name algorithms by their legacy
/// numbers (0 and 1 bcrypt, 2 argon2i, 3 argon2id); strings name them by identifier.
///
/// # Errors
///
/// A `TypeError` for an array or object, a `ValueError` for anything that names no algorithm.
pub fn resolve_algorithm(value: &Value, function: &str) -> Result<Algorithm, PasswordError> {
    let value = &*value.read();
    if let Some(err) = algorithm_type_error(value, function) {
        return Err(err);
    }
    let invalid = || {
        PasswordError::value_error(format!(
            "{function}(): Argument #2 ($algo) must be a valid password hashing algorithm"
        ))
    };
    let number = match value {
        Value::Null => 0,
        Value::Bool(b) => i64::from(*b),
        Value::Int(n) => *n,
        Value::Float(f) => float_to_long(*f),
        Value::Str(s) => return by_name(s.as_bytes()).ok_or_else(invalid),
        Value::Bytes(b) => return by_name(b).ok_or_else(invalid),
        Value::Array(_) | Value::Opaque(_) => unreachable!("rejected above"),
    };
    match number {
        0 | 1 => Ok(Algorithm::Bcrypt),
        2 => Ok(Algorithm::Argon2i),
        3 => Ok(Algorithm::Argon2id),
        _ => Err(invalid()),
    }
}

fn by_name(name: &[u8]) -> Option<Algorithm> {
    match name {
        b"2y" => Some(Algorithm::Bcrypt),
        b"argon2i" => Some(Algorithm::Argon2i),
        b"argon2id" => Some(Algorithm::Argon2id),
        _ => None,
    }
}

/// The options `password_hash()` reads, before any strategy validates them. `None` is "not given".
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HashOptions {
    pub cost: Option<i64>,
    pub memory_cost: Option<i64>,
    pub time_cost: Option<i64>,
    pub threads: Option<i64>,
    /// Whether a `salt` option was given (PHP ignores it, with a warning for bcrypt).
    pub salt: bool,
}

/// The options array `function()` received.
///
/// # Errors
///
/// A `TypeError` when the value is not an array.
pub fn resolve_options(value: &Value, function: &str) -> Result<HashOptions, PasswordError> {
    let value = &*value.read();
    let Value::Array(entries) = value else {
        return Err(PasswordError::type_error(format!(
            "{function}(): Argument #3 ($options) must be of type array, {} given",
            php_type_name(value)
        )));
    };
    let find = |name: &str| {
        entries
            .iter()
            .find(|(key, _)| matches!(key, Key::Str(k) if k == name))
            .map(|(_, v)| php_long(v))
    };
    Ok(HashOptions {
        cost: find("cost"),
        memory_cost: find("memory_cost"),
        time_cost: find("time_cost"),
        threads: find("threads"),
        salt: entries
            .iter()
            .any(|(key, _)| matches!(key, Key::Str(k) if k == "salt")),
    })
}

/// PHP's `zval_get_long()`: integers as they are, floats truncated, numeric string prefixes parsed, `null` and
/// `false` 0, `true` 1, arrays 0 when empty and 1 otherwise.
pub fn php_long(value: &Value) -> i64 {
    match &*value.read() {
        Value::Null => 0,
        Value::Bool(b) => i64::from(*b),
        Value::Int(n) => *n,
        Value::Float(f) => float_to_long(*f),
        Value::Str(s) => string_to_long(s.as_bytes()),
        Value::Bytes(b) => string_to_long(b),
        Value::Array(a) => i64::from(!a.is_empty()),
        Value::Opaque(_) => 1,
    }
}

/// A float as PHP 8 casts it to an integer: truncated; 0 when not finite or out of range.
fn float_to_long(f: f64) -> i64 {
    if f.is_finite() && f >= i64::MIN as f64 && f < i64::MAX as f64 {
        f as i64
    } else {
        0
    }
}

/// The leading number of a string (after whitespace), as `zval_get_long()` reads it; 0 when there is none.
fn string_to_long(text: &[u8]) -> i64 {
    let text = match text
        .iter()
        .position(|b| !matches!(b, b' ' | b'\t' | b'\n' | b'\r' | b'\x0B' | b'\x0C'))
    {
        Some(start) => &text[start..],
        None => return 0,
    };
    let mut end = 0;
    if matches!(text.first(), Some(b'+' | b'-')) {
        end = 1;
    }
    let digits_start = end;
    while text.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    let integral = std::str::from_utf8(&text[..end]).unwrap_or("0");
    let is_float = matches!(text.get(end), Some(b'.' | b'e' | b'E'));
    if !is_float {
        return if end == digits_start {
            0
        } else {
            integral.parse().unwrap_or_else(|_| {
                // Integer overflow becomes a float, then the float rule.
                float_to_long(integral.parse().unwrap_or(0.0))
            })
        };
    }
    let mut fend = end;
    if text.get(fend) == Some(&b'.') {
        fend += 1;
        while text.get(fend).is_some_and(u8::is_ascii_digit) {
            fend += 1;
        }
    }
    if matches!(text.get(fend), Some(b'e' | b'E')) {
        let mut e = fend + 1;
        if matches!(text.get(e), Some(b'+' | b'-')) {
            e += 1;
        }
        if text.get(e).is_some_and(u8::is_ascii_digit) {
            while text.get(e).is_some_and(u8::is_ascii_digit) {
                e += 1;
            }
            fend = e;
        }
    }
    float_to_long(
        std::str::from_utf8(&text[..fend])
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn php_long_reads_strings_like_php() {
        let s = |t: &str| php_long(&Value::Str(t.into()));
        assert_eq!(
            (
                s("5"),
                s(" 12"),
                s("5.7"),
                s("abc"),
                s("7abc"),
                s("-3"),
                s("1e2"),
                s("")
            ),
            (5, 12, 5, 0, 7, -3, 100, 0)
        );
    }
}
