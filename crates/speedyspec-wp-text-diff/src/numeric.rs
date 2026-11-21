//! PHP 8's `==` between two strings (`zendi_smart_str_equals()`), which Horde's engine uses in two places.

/// A numeric string's value, as `is_numeric_string_ex()` reads it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Numeric {
    /// An integer that fits in an `int`.
    Int(i64),
    /// A float; `overflow` is -1 or 1 for an integer-looking string beyond `int`, 0 otherwise.
    Float { value: f64, overflow: i8 },
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C)
}

/// Reads `s` as PHP 8 reads a numeric string: leading and trailing whitespace allowed, no hex, no other trailing
/// characters. `None` when it is not numeric.
pub fn parse_numeric(s: &[u8]) -> Option<Numeric> {
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    let mut start = 0;
    while start < s.len() && is_space(s[start]) {
        start += 1;
    }
    let str_ = start;
    let mut ptr = start;
    let negative = at(ptr) == b'-';
    if at(ptr) == b'-' || at(ptr) == b'+' {
        ptr += 1;
    }
    let mut is_double = false;
    let mut overflow = 0i8;
    let mut int_value: u64 = 0;
    let mut digits = 0usize;
    let end;
    if at(ptr).is_ascii_digit() {
        while at(ptr) == b'0' {
            ptr += 1;
        }
        loop {
            let c = at(ptr);
            if digits >= 20 {
                overflow = if at(str_) == b'-' { -1 } else { 1 };
                is_double = true;
                break;
            }
            if c.is_ascii_digit() {
                int_value = int_value.wrapping_mul(10).wrapping_add((c - b'0') as u64);
                digits += 1;
                ptr += 1;
                continue;
            } else if c == b'.' {
                is_double = true;
                break;
            } else if c == b'e' || c == b'E' {
                let mut e = ptr + 1;
                if at(e) == b'-' || at(e) == b'+' {
                    ptr = e;
                    e += 1;
                }
                if at(e).is_ascii_digit() {
                    is_double = true;
                }
                break;
            }
            break;
        }
        end = if is_double { strtod_end(s, str_) } else { ptr };
    } else if at(ptr) == b'.' && at(ptr + 1).is_ascii_digit() {
        is_double = true;
        end = strtod_end(s, str_);
    } else {
        return None;
    }
    let mut tail = end;
    while tail < s.len() && is_space(s[tail]) {
        tail += 1;
    }
    if tail != s.len() {
        return None;
    }
    if !is_double {
        if digits == 19 {
            let first = end - digits;
            let digit_text = &s[first..end];
            let cmp = digit_text.cmp(b"9223372036854775808".as_slice());
            if !(cmp.is_lt() || (cmp.is_eq() && at(str_) == b'-')) {
                return Some(Numeric::Float {
                    value: strtod(&s[str_..end]),
                    overflow: if at(str_) == b'-' { -1 } else { 1 },
                });
            }
        }
        let value = if negative {
            (int_value as i64).wrapping_neg()
        } else {
            int_value as i64
        };
        return Some(Numeric::Int(value));
    }
    Some(Numeric::Float {
        value: strtod(&s[str_..end]),
        overflow,
    })
}

/// Where `zend_strtod()` stops: `[+-]?digits[.digits][(e|E)[+-]?digits]`, from `from`.
fn strtod_end(s: &[u8], from: usize) -> usize {
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    let mut p = from;
    if at(p) == b'-' || at(p) == b'+' {
        p += 1;
    }
    while at(p).is_ascii_digit() {
        p += 1;
    }
    if at(p) == b'.' {
        p += 1;
        while at(p).is_ascii_digit() {
            p += 1;
        }
    }
    if at(p) == b'e' || at(p) == b'E' {
        let mut q = p + 1;
        if at(q) == b'-' || at(q) == b'+' {
            q += 1;
        }
        if at(q).is_ascii_digit() {
            while at(q).is_ascii_digit() {
                q += 1;
            }
            p = q;
        }
    }
    p
}

fn strtod(text: &[u8]) -> f64 {
    // The text is ASCII digits, signs, '.' and exponent: always a valid Rust float literal, correctly rounded as
    // zend_strtod() is.
    std::str::from_utf8(text)
        .ok()
        .and_then(|t| t.parse().ok())
        .unwrap_or(0.0)
}

/// PHP 8's `$a == $b` for two strings whose numeric readings are `na` and `nb`, when the strings differ.
///
/// Equal strings are equal; callers check that first.
pub fn numeric_equal(na: Option<Numeric>, nb: Option<Numeric>) -> bool {
    let (Some(a), Some(b)) = (na, nb) else {
        return false;
    };
    match (a, b) {
        (Numeric::Int(x), Numeric::Int(y)) => x == y,
        (
            Numeric::Float {
                value: x,
                overflow: ox,
            },
            Numeric::Float {
                value: y,
                overflow: oy,
            },
        ) => {
            if ox != 0 && ox == oy && x - y == 0.0 {
                return false; // both overflowed to the same side: compared as strings, which differ
            }
            if x == y && !x.is_finite() {
                return false; // compared as strings
            }
            x == y
        }
        (
            Numeric::Int(x),
            Numeric::Float {
                value: y,
                overflow: oy,
            },
        )
        | (
            Numeric::Float {
                value: y,
                overflow: oy,
            },
            Numeric::Int(x),
        ) => {
            if oy != 0 {
                return false;
            }
            x as f64 == y
        }
    }
}

/// PHP 8's `$a == $b` for two strings.
pub fn php_loose_eq(a: &[u8], b: &[u8]) -> bool {
    a == b || numeric_equal(parse_numeric(a), parse_numeric(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_strings_compare_as_numbers() {
        assert!(php_loose_eq(b"1e3", b"1000"));
        assert!(php_loose_eq(b"1.0", b"1"));
        assert!(php_loose_eq(b" 1", b"1 "));
        assert!(!php_loose_eq(b"abc", b"ABC"));
        assert!(!php_loose_eq(b"0x1A", b"26"));
        assert!(!php_loose_eq(b"1e500", b"1e400"));
    }
}
