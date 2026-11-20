//! PHP's `(int)` cast.

use speedyspec_wp_hooks::HookValue;

/// The longest leading number in `s` (after leading whitespace), as PHP reads a numeric prefix.
fn leading_number(s: &str) -> Option<f64> {
    let t = s.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let b = t.as_bytes();
    let mut end = 0;
    if matches!(b.first(), Some(b'+' | b'-')) {
        end = 1;
    }
    let digits_from = end;
    while b.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    let mut seen_digit = end > digits_from;
    if b.get(end) == Some(&b'.') {
        let frac_from = end + 1;
        let mut e = frac_from;
        while b.get(e).is_some_and(u8::is_ascii_digit) {
            e += 1;
        }
        if seen_digit || e > frac_from {
            seen_digit = true;
            end = e;
        }
    }
    if !seen_digit {
        return None;
    }
    if matches!(b.get(end), Some(b'e' | b'E')) {
        let mut e = end + 1;
        if matches!(b.get(e), Some(b'+' | b'-')) {
            e += 1;
        }
        let exp_from = e;
        while b.get(e).is_some_and(u8::is_ascii_digit) {
            e += 1;
        }
        if e > exp_from {
            end = e;
        }
    }
    t[..end].parse().ok()
}

fn float_to_int(f: f64) -> i64 {
    if f.is_finite() { f as i64 } else { 0 }
}

/// `(int) $value`: null 0, bools 0 or 1, floats truncated, strings by their leading numeric prefix, else 0.
pub fn php_int(value: &HookValue) -> i64 {
    match value {
        HookValue::Null => 0,
        HookValue::Bool(b) => i64::from(*b),
        HookValue::Int(n) => *n,
        HookValue::Float(f) => float_to_int(*f),
        HookValue::Str(s) => string_int(s),
        HookValue::Bytes(b) => std::str::from_utf8(b).map(string_int).unwrap_or(0),
        // A value a language bridge carries unconverted (a PHP filter's string) is cast as it reads.
        HookValue::Opaque(_) => match value.read() {
            std::borrow::Cow::Owned(read) if !matches!(read, HookValue::Opaque(_)) => {
                php_int(&read)
            }
            std::borrow::Cow::Owned(_) => 0,
            std::borrow::Cow::Borrowed(_) => 0,
        },
        _ => 0,
    }
}

fn string_int(s: &str) -> i64 {
    let t = s.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let integer_end = t
        .char_indices()
        .skip(usize::from(t.starts_with(['+', '-'])))
        .find(|(_, c)| !c.is_ascii_digit());
    let integer = &t[..integer_end.map_or(t.len(), |(i, _)| i)];
    match leading_number(s) {
        // A plain integer prefix is read exactly; a fraction or exponent through the float.
        Some(f)
            if integer.len() == t.len()
                || !matches!(t.as_bytes().get(integer.len()), Some(b'.' | b'e' | b'E')) =>
        {
            integer.parse::<i64>().unwrap_or_else(|_| float_to_int(f))
        }
        Some(f) => float_to_int(f),
        None => 0,
    }
}

/// PHP 8's `$value < $now` for a stored timeout: numeric values compare as numbers, other strings as text.
pub(crate) fn less_than(value: &HookValue, now: i64) -> bool {
    match value {
        HookValue::Int(n) => *n < now,
        HookValue::Float(f) => *f < now as f64,
        // A bool or null against a number compares as bools: only `false < true` holds.
        HookValue::Bool(b) => !b && now != 0,
        HookValue::Null => now != 0,
        HookValue::Str(s) => {
            numeric(s).map_or_else(|| s.as_str() < now.to_string().as_str(), |f| f < now as f64)
        }
        HookValue::Bytes(b) => {
            let s = String::from_utf8_lossy(b);
            numeric(&s).map_or_else(|| *s < *now.to_string(), |f| f < now as f64)
        }
        HookValue::Opaque(_) => match value.read() {
            std::borrow::Cow::Owned(read) if !matches!(read, HookValue::Opaque(_)) => {
                less_than(&read, now)
            }
            std::borrow::Cow::Owned(_) => false,
            std::borrow::Cow::Borrowed(_) => false,
        },
        _ => false,
    }
}

/// A numeric string's value (surrounding whitespace allowed), as `is_numeric` accepts it.
fn numeric(s: &str) -> Option<f64> {
    let t = s.trim_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let n = leading_number(t)?;
    let len = t.trim_start_matches(['+', '-']).len() + usize::from(t.starts_with(['+', '-']));
    (len == t.len() && t.parse::<f64>().is_ok()).then_some(n)
}
