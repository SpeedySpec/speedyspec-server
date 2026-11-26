//! PHP's value semantics the cron functions rely on.

use speedyspec_wp_cron_contracts::key_for;
use speedyspec_wp_hooks_contracts::{HookValue, Key};

/// PHP's conversion to `bool`.
pub fn truthy(value: &HookValue) -> bool {
    match &*value.read() {
        HookValue::Null => false,
        HookValue::Bool(b) => *b,
        HookValue::Int(n) => *n != 0,
        HookValue::Float(f) => *f != 0.0,
        HookValue::Str(s) => !(s.is_empty() || s == "0"),
        HookValue::Bytes(b) => !(b.is_empty() || b == b"0"),
        HookValue::Array(items) => !items.is_empty(),
        _ => true,
    }
}

/// PHP 8's `is_numeric()` for a string: optional surrounding whitespace, a sign, digits with an optional fraction,
/// an optional exponent.
pub fn numeric_str(s: &str) -> Option<f64> {
    let t = s.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let t = t.trim_end_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let b = t.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let digits_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let mut digits = i - digits_start;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let frac = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        digits += i - frac;
    }
    if digits == 0 {
        return None;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        let exp = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > exp {
            i = j;
        }
    }
    (i == b.len()).then(|| t.parse::<f64>().ok()).flatten()
}

/// `(float) $string`: the leading numeric part, 0 when there is none.
pub fn numeric_prefix(s: &str) -> f64 {
    let t = s.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let mut end = 0;
    for i in (1..=t.len()).rev() {
        if t.is_char_boundary(i) && numeric_str(&t[..i]).is_some() {
            end = i;
            break;
        }
    }
    t[..end].parse::<f64>().unwrap_or(0.0)
}

/// `is_numeric( $value )`.
pub fn is_numeric(value: &HookValue) -> bool {
    match &*value.read() {
        HookValue::Int(_) | HookValue::Float(_) => true,
        HookValue::Str(s) => numeric_str(s).is_some(),
        _ => false,
    }
}

/// The key PHP uses for `$array[ $value ]`; `None` for a value that cannot be a key (an array or object).
pub fn array_key(value: &HookValue) -> Option<Key> {
    match &*value.read() {
        HookValue::Str(s) => Some(key_for(s)),
        HookValue::Bytes(b) => Some(Key::Str(String::from_utf8_lossy(b).into_owned())),
        HookValue::Int(n) => Some(Key::Int(*n)),
        HookValue::Bool(b) => Some(Key::Int(i64::from(*b))),
        HookValue::Null => Some(Key::Str(String::new())),
        HookValue::Float(f) => Some(Key::Int(f.trunc() as i64)),
        _ => None,
    }
}

/// A key as the PHP value `foreach` gives it.
pub fn key_value(key: &Key) -> HookValue {
    match key {
        Key::Int(n) => HookValue::Int(*n),
        Key::Str(s) => HookValue::Str(s.clone()),
    }
}

/// `$key > $float` for a key of the cron array, as PHP 8 compares them: numerically when the key is numeric,
/// otherwise as strings against the float's decimal text.
pub fn key_after(key: &Key, float: f64) -> bool {
    match key {
        Key::Int(n) => (*n as f64) > float,
        Key::Str(s) => match numeric_str(s) {
            Some(n) => n > float,
            None => s.as_bytes() > format!("{float}").as_bytes(),
        },
    }
}

/// `$key < $float`, as [`key_after`] compares.
pub fn key_before(key: &Key, float: f64) -> bool {
    match key {
        Key::Int(n) => (*n as f64) < float,
        Key::Str(s) => match numeric_str(s) {
            Some(n) => n < float,
            None => s.as_bytes() < format!("{float}").as_bytes(),
        },
    }
}

/// A numeric value as a float (`is_numeric()` values only).
pub fn number(value: &HookValue) -> Option<f64> {
    match &*value.read() {
        HookValue::Int(n) => Some(*n as f64),
        HookValue::Float(f) => Some(*f),
        HookValue::Str(s) => numeric_str(s),
        _ => None,
    }
}

/// PHP's `empty()` of an array entry.
pub fn empty(value: Option<&HookValue>) -> bool {
    match value {
        None => true,
        Some(v) => !truthy(v),
    }
}

/// `unset( $array[ $a ][ $b ][ $c ] )`, then `$array[ $a ][ $b ]` and `$array[ $a ]` unset when `empty()`, as
/// `wp_unschedule_event()` does.
pub fn unset3(items: &mut Vec<(Key, HookValue)>, a: &Key, b: &Key, c: &Key) {
    if let Some((_, HookValue::Array(hooks))) = items.iter_mut().find(|(k, _)| k == a) {
        if let Some((_, HookValue::Array(keyed))) = hooks.iter_mut().find(|(k, _)| k == b) {
            keyed.retain(|(k, _)| k != c);
        }
        let hook_empty = empty(hooks.iter().find(|(k, _)| k == b).map(|(_, v)| v));
        if hook_empty {
            hooks.retain(|(k, _)| k != b);
        }
    }
    let ts_empty = empty(items.iter().find(|(k, _)| k == a).map(|(_, v)| v));
    if ts_empty {
        items.retain(|(k, _)| k != a);
    }
}

/// `$array[ $key ]`, when `array` is an array holding it.
pub fn get<'a>(array: &'a HookValue, key: &Key) -> Option<&'a HookValue> {
    match array {
        HookValue::Array(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

/// `isset( $array[ $key ] )`.
pub fn isset(array: &HookValue, key: &Key) -> bool {
    get(array, key).is_some_and(|v| !matches!(v, HookValue::Null))
}

/// `(array) $value`.
pub fn array_cast(value: &HookValue) -> Vec<(Key, HookValue)> {
    match value {
        HookValue::Array(items) => items.clone(),
        HookValue::Null => Vec::new(),
        other => vec![(Key::Int(0), other.clone())],
    }
}

/// `$array[ $a ][ $b ][ $c ] = $value`, creating levels as PHP does.
pub fn set3(items: &mut Vec<(Key, HookValue)>, a: Key, b: Key, c: Key, value: HookValue) {
    fn level(items: &mut Vec<(Key, HookValue)>, key: Key) -> &mut Vec<(Key, HookValue)> {
        let i = match items.iter().position(|(k, _)| *k == key) {
            Some(i) => i,
            None => {
                items.push((key, HookValue::Array(Vec::new())));
                items.len() - 1
            }
        };
        if !matches!(items[i].1, HookValue::Array(_)) {
            items[i].1 = HookValue::Array(Vec::new());
        }
        match &mut items[i].1 {
            HookValue::Array(inner) => inner,
            _ => unreachable!(),
        }
    }
    let inner = level(level(items, a), b);
    match inner.iter_mut().find(|(k, _)| *k == c) {
        Some((_, v)) => *v = value,
        None => inner.push((c, value)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_strings_match_php() {
        for yes in ["1", " 1", "1 ", "-1.5", ".5", "1e3", "+2"] {
            assert!(numeric_str(yes).is_some(), "{yes}");
        }
        for no in ["", "abc", "1x", "0x1A", ".", "e3", "1e"] {
            assert!(numeric_str(no).is_none(), "{no}");
        }
    }
}
