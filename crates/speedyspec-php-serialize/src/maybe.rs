//! WordPress's `is_serialized`, `maybe_serialize` and `maybe_unserialize` (`functions.php`), exactly.

use crate::{PhpValue, serialize, unserialize};

fn trim(data: &[u8]) -> &[u8] {
    let ws = |b: &u8| b" \t\n\r\0\x0B".contains(b);
    let start = data.iter().position(|b| !ws(b)).unwrap_or(data.len());
    let end = data.iter().rposition(|b| !ws(b)).map_or(start, |p| p + 1);
    &data[start..end.max(start)]
}

/// `^<token>:[0-9]+:`.
fn counted(data: &[u8]) -> bool {
    let digits = data[2..].iter().take_while(|b| b.is_ascii_digit()).count();
    digits > 0 && data.get(2 + digits) == Some(&b':')
}

/// `^<token>:[0-9.E+-]+;` and, when strict, nothing after.
fn scalar(data: &[u8], strict: bool) -> bool {
    let run = data[2..]
        .iter()
        .take_while(|b| b.is_ascii_digit() || b".E+-".contains(b))
        .count();
    if run == 0 || data.get(2 + run) != Some(&b';') {
        return false;
    }
    !strict || 2 + run + 1 == data.len()
}

/// `is_serialized()`.
pub fn is_serialized(data: &[u8], strict: bool) -> bool {
    let data = trim(data);
    if data == b"N;" {
        return true;
    }
    if data.len() < 4 || data[1] != b':' {
        return false;
    }
    if strict {
        if !matches!(data.last(), Some(b';' | b'}')) {
            return false;
        }
    } else {
        let semicolon = data.iter().position(|&b| b == b';');
        let brace = data.iter().position(|&b| b == b'}');
        if semicolon.is_none() && brace.is_none() {
            return false;
        }
        if semicolon.is_some_and(|p| p < 3) || brace.is_some_and(|p| p < 4) {
            return false;
        }
    }
    match data[0] {
        b's' => {
            let quoted = if strict {
                data[data.len() - 2] == b'"'
            } else {
                data.contains(&b'"')
            };
            quoted && counted(data)
        }
        b'a' | b'O' | b'E' => counted(data),
        b'b' | b'i' | b'd' => scalar(data, strict),
        _ => false,
    }
}

/// `maybe_serialize()`: arrays and objects serialized, and strings that look serialized serialized again.
pub fn maybe_serialize(value: &PhpValue) -> PhpValue {
    match value {
        PhpValue::Array(_)
        | PhpValue::Object { .. }
        | PhpValue::Custom { .. }
        | PhpValue::Enum { .. } => PhpValue::Str(serialize(value)),
        PhpValue::Str(s) if is_serialized(s, false) => PhpValue::Str(serialize(value)),
        other => other.clone(),
    }
}

/// `maybe_unserialize()`: a serialized string's value, `false` when it does not unserialize (`@unserialize`).
pub fn maybe_unserialize(value: &PhpValue) -> PhpValue {
    match value {
        PhpValue::Str(s) if is_serialized(s, true) => {
            unserialize(trim(s)).unwrap_or(PhpValue::Bool(false))
        }
        other => other.clone(),
    }
}
