//! `_wp_scan_utf8`: WordPress's UTF-8 scanner, ported line for line.

/// What one `_wp_scan_utf8` call reports: its return value and its by-reference outputs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scan {
    /// Code points found before stopping (the PHP function's return value).
    pub code_points: usize,
    /// Where scanning stopped (`$at`): the first byte of an invalid sequence, or the first byte not scanned.
    pub at: usize,
    /// Length of the maximal subpart of the invalid sequence at `at`, or 0 (`$invalid_length`).
    pub invalid_length: usize,
    /// Whether a noncharacter was seen (`$has_noncharacters`).
    pub has_noncharacters: bool,
    /// Whether PHP's `$has_noncharacters |= …` ran, which turns the by-reference value from the boolean `false` into
    /// an integer. Only the native function needs this, to hand PHP the same type WordPress does.
    pub noncharacters_tested: bool,
}

#[inline]
fn byte(bytes: &[u8], i: usize) -> u8 {
    // PHP's `$bytes[ $i ] ?? "\xC0"`: past the end of the string (not of the scan window) reads as 0xC0.
    bytes.get(i).copied().unwrap_or(0xC0)
}

#[inline]
fn three_byte_lead_valid(b1: u8, b2: u8) -> bool {
    (b1 == 0xE0 && (0xA0..=0xBF).contains(&b2))
        || ((0xE1..=0xEC).contains(&b1) && (0x80..=0xBF).contains(&b2))
        || (b1 == 0xED && (0x80..=0x9F).contains(&b2))
        || ((0xEE..=0xEF).contains(&b1) && (0x80..=0xBF).contains(&b2))
}

#[inline]
fn four_byte_lead_valid(b1: u8, b2: u8) -> bool {
    (b1 == 0xF0 && (0x90..=0xBF).contains(&b2))
        || ((0xF1..=0xF3).contains(&b1) && (0x80..=0xBF).contains(&b2))
        || (b1 == 0xF4 && (0x80..=0x8F).contains(&b2))
}

/// Scans `bytes` from `at` for valid UTF-8, as WordPress's `_wp_scan_utf8` does.
///
/// Scanning stops at the first invalid sequence, after `max_bytes` bytes (a sequence that starts before that limit
/// is still read to its end, as WordPress does), or once `max_code_points` code points are counted.
pub fn scan_utf8(bytes: &[u8], at: usize, max_bytes: Option<usize>, max_code_points: Option<usize>) -> Scan {
    let byte_length = bytes.len();
    let end = byte_length.min(at.saturating_add(max_bytes.unwrap_or(usize::MAX)));
    let max_count = max_code_points.unwrap_or(usize::MAX);
    let mut out = Scan { code_points: 0, at, invalid_length: 0, has_noncharacters: false, noncharacters_tested: false };
    let mut count = 0usize;
    let mut i = at;

    while i < end && count <= max_count {
        let ascii = bytes[i..end].iter().take_while(|b| **b < 0x80).count();
        if count + ascii >= max_count {
            out.at = i + (max_count - count);
            out.code_points = max_count;
            return out;
        }
        count += ascii;
        i += ascii;
        if i >= end {
            out.at = end;
            out.code_points = count;
            return out;
        }

        let b1 = bytes[i];
        let b2 = byte(bytes, i + 1);
        if (0xC2..=0xDF).contains(&b1) && (0x80..=0xBF).contains(&b2) {
            count += 1;
            i += 2;
            continue;
        }

        let b3 = byte(bytes, i + 2);
        if (0x80..=0xBF).contains(&b3) {
            if three_byte_lead_valid(b1, b2) {
                count += 1;
                i += 3;
                if b1 == 0xEF {
                    out.noncharacters_tested = true;
                    out.has_noncharacters |=
                        (b2 == 0xB7 && (0x90..=0xAF).contains(&b3)) || (b2 == 0xBF && (b3 == 0xBE || b3 == 0xBF));
                }
                continue;
            }
            let b4 = byte(bytes, i + 3);
            if (0x80..=0xBF).contains(&b4) && four_byte_lead_valid(b1, b2) {
                count += 1;
                i += 4;
                out.noncharacters_tested = true;
                out.has_noncharacters |= (b2 & 0x0F) == 0x0F && b3 == 0xBF && (b4 == 0xBE || b4 == 0xBF);
                continue;
            }
        }

        // invalid_utf8:
        out.at = i;
        out.code_points = count;
        out.invalid_length = 1;
        if b1 & 0x80 == 0 || b1 & 0xE0 == 0xC0 {
            return out;
        }
        if b1 & 0xF0 == 0xE0 {
            let b2_valid = three_byte_lead_valid(b1, b2);
            out.invalid_length = (end - i).min(if b2_valid { 2 } else { 1 });
        } else if b1 & 0xF8 == 0xF0 {
            let b2_valid = four_byte_lead_valid(b1, b2);
            let b3_valid = (0x80..=0xBF).contains(&b3);
            out.invalid_length = (end - i).min(if b2_valid { if b3_valid { 3 } else { 2 } } else { 1 });
        }
        return out;
    }

    out.at = i;
    out.code_points = count;
    out
}
