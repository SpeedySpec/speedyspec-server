//! Valkey's `INCRBY` rules, for the key-value stores that emulate it: a counter is canonical decimal ASCII, a value
//! that is not is refused rather than reset, and overflow is an error.

use speedyspec_data::DbError;

/// The counter `bytes` holds, when they are canonical decimal: no sign but `-`, no leading zero, no `-0`, no spaces.
pub fn parse_counter(bytes: &[u8]) -> Option<i64> {
    let digits = bytes.strip_prefix(b"-").unwrap_or(bytes);
    let canonical = !digits.is_empty()
        && digits.iter().all(u8::is_ascii_digit)
        && (digits[0] != b'0' || digits.len() == 1)
        && !(bytes.starts_with(b"-") && digits == b"0");
    if !canonical {
        return None;
    }
    std::str::from_utf8(bytes).ok()?.parse().ok()
}

/// `current` (absent counts as 0) plus `by`: the new value and the bytes to store.
pub fn checked_increment(
    engine: &str,
    current: Option<&[u8]>,
    by: i64,
) -> Result<(i64, Vec<u8>), DbError> {
    let error = |code: &str, message: &str| DbError::Engine {
        engine: engine.to_string(),
        code: Some(code.to_string()),
        message: message.to_string(),
    };
    let now = match current {
        None => 0,
        Some(bytes) => parse_counter(bytes)
            .ok_or_else(|| error("not-integer", "value is not an integer or out of range"))?,
    };
    let next = now
        .checked_add(by)
        .ok_or_else(|| error("overflow", "increment or decrement would overflow"))?;
    Ok((next, next.to_string().into_bytes()))
}
