//! PHP's `is_numeric()` and the arithmetic of `incr`/`decr` (`+=` with an `(int)` offset).

use speedyspec_hooks::Value;

const WS: [char; 6] = [' ', '\t', '\n', '\r', '\x0b', '\x0c'];

/// A numeric string as PHP reads it: an integer when it has no point or exponent and fits, else a float.
fn numeric_text(text: &str) -> Option<Value> {
    let t = text.trim_start_matches(WS).trim_end_matches(WS);
    let body = t.strip_prefix(['+', '-']).unwrap_or(t);
    let (mantissa, exponent) = match body.find(['e', 'E']) {
        Some(at) => (&body[..at], Some(&body[at + 1..])),
        None => (body, None),
    };
    let (int_part, frac) = match mantissa.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (mantissa, None),
    };
    let digits = |s: &str| s.bytes().all(|c| c.is_ascii_digit());
    if !digits(int_part)
        || !frac.is_none_or(digits)
        || (int_part.is_empty() && frac.is_none_or(str::is_empty))
    {
        return None;
    }
    if let Some(e) = exponent {
        let e = e.strip_prefix(['+', '-']).unwrap_or(e);
        if e.is_empty() || !digits(e) {
            return None;
        }
    }
    if frac.is_none() && exponent.is_none()
        && let Ok(i) = t.parse::<i64>() {
            return Some(Value::Int(i));
        }
    t.parse::<f64>().ok().map(Value::Float)
}

/// `is_numeric()` and the number it reads.
pub(crate) fn numeric(value: &Value) -> Option<Value> {
    match value {
        Value::Int(_) | Value::Float(_) => Some(value.clone()),
        Value::Str(s) => numeric_text(s),
        _ => None,
    }
}

/// `$value += $offset` (or `-=`), then a result below 0 becomes 0.
pub(crate) fn add(value: &Value, offset: i64) -> Value {
    let base = numeric(value).unwrap_or(Value::Int(0));
    let sum = match base {
        Value::Int(i) => i
            .checked_add(offset)
            .map_or_else(|| Value::Float(i as f64 + offset as f64), Value::Int),
        Value::Float(f) => Value::Float(f + offset as f64),
        _ => Value::Int(offset),
    };
    match sum {
        Value::Int(i) if i < 0 => Value::Int(0),
        Value::Float(f) if f < 0.0 => Value::Int(0),
        other => other,
    }
}
