//! Parsing a single `Range: bytes=` request.

use http::HeaderValue;

/// What to send for a `Range` header against a body of a given length.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RangeOutcome {
    /// Ignore the header and send everything: several ranges, or a header that does not parse.
    Full,
    /// Inclusive start and end.
    Partial(u64, u64),
    Unsatisfiable,
}

pub fn parse(value: &HeaderValue, len: u64) -> RangeOutcome {
    let Some(spec) = value
        .to_str()
        .ok()
        .and_then(|v| v.trim().strip_prefix("bytes="))
    else {
        return RangeOutcome::Full;
    };
    if spec.contains(',') {
        return RangeOutcome::Full;
    }
    let Some((start, end)) = spec.trim().split_once('-') else {
        return RangeOutcome::Full;
    };
    let (start, end) = (start.trim(), end.trim());
    let number = |s: &str| s.parse::<u64>().ok();

    if start.is_empty() {
        return match number(end) {
            Some(0) => RangeOutcome::Unsatisfiable,
            Some(_) if len == 0 => RangeOutcome::Unsatisfiable,
            Some(suffix) => RangeOutcome::Partial(len.saturating_sub(suffix), len - 1),
            None => RangeOutcome::Full,
        };
    }
    let Some(first) = number(start) else {
        return RangeOutcome::Full;
    };
    let last = if end.is_empty() {
        None
    } else {
        match number(end) {
            Some(last) if last >= first => Some(last),
            _ => return RangeOutcome::Full,
        }
    };
    if first >= len {
        return RangeOutcome::Unsatisfiable;
    }

    RangeOutcome::Partial(first, last.map_or(len - 1, |last| last.min(len - 1)))
}
