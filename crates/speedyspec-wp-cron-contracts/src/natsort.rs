//! PHP's `strnatcasecmp()` (`strnatcmp_ex` with case folding), byte for byte: `wp_schedule_event()` orders the cron
//! array with `uksort( $crons, 'strnatcasecmp' )`.

use std::cmp::Ordering;

fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

fn digit(s: &[u8], i: usize) -> bool {
    i < s.len() && s[i].is_ascii_digit()
}

/// C's `isspace` in the C locale.
fn space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// Two right-aligned numbers: the longer run of digits wins, then the first differing digit.
fn compare_right(a: &[u8], ai: &mut usize, b: &[u8], bi: &mut usize) -> Ordering {
    let mut bias = Ordering::Equal;
    loop {
        match (digit(a, *ai), digit(b, *bi)) {
            (false, false) => return bias,
            (false, true) => return Ordering::Less,
            (true, false) => return Ordering::Greater,
            (true, true) => {
                if bias == Ordering::Equal {
                    bias = a[*ai].cmp(&b[*bi]);
                }
            }
        }
        *ai += 1;
        *bi += 1;
    }
}

/// Two left-aligned numbers (a fractional part): the first differing digit wins.
fn compare_left(a: &[u8], ai: &mut usize, b: &[u8], bi: &mut usize) -> Ordering {
    loop {
        match (digit(a, *ai), digit(b, *bi)) {
            (false, false) => return Ordering::Equal,
            (false, true) => return Ordering::Less,
            (true, false) => return Ordering::Greater,
            (true, true) => match a[*ai].cmp(&b[*bi]) {
                Ordering::Equal => {}
                other => return other,
            },
        }
        *ai += 1;
        *bi += 1;
    }
}

/// `strnatcasecmp( $a, $b )`'s sign.
pub fn strnatcasecmp(a: &[u8], b: &[u8]) -> Ordering {
    if a.is_empty() || b.is_empty() {
        return a.len().cmp(&b.len());
    }
    let (mut ai, mut bi) = (0usize, 0usize);
    let mut leading = true;
    loop {
        let mut ca = at(a, ai);
        let mut cb = at(b, bi);
        while leading && ca == b'0' && ai + 1 < a.len() && a[ai + 1].is_ascii_digit() {
            ai += 1;
            ca = at(a, ai);
        }
        while leading && cb == b'0' && bi + 1 < b.len() && b[bi + 1].is_ascii_digit() {
            bi += 1;
            cb = at(b, bi);
        }
        leading = false;
        while space(ca) {
            ai += 1;
            ca = at(a, ai);
        }
        while space(cb) {
            bi += 1;
            cb = at(b, bi);
        }
        if ca.is_ascii_digit() && cb.is_ascii_digit() {
            let result = if ca == b'0' || cb == b'0' {
                compare_left(a, &mut ai, b, &mut bi)
            } else {
                compare_right(a, &mut ai, b, &mut bi)
            };
            if result != Ordering::Equal {
                return result;
            }
            match (ai >= a.len(), bi >= b.len()) {
                (true, true) => return Ordering::Equal,
                (true, false) => return Ordering::Less,
                (false, true) => return Ordering::Greater,
                _ => {
                    ca = at(a, ai);
                    cb = at(b, bi);
                }
            }
        }
        match ca.to_ascii_uppercase().cmp(&cb.to_ascii_uppercase()) {
            Ordering::Equal => {}
            other => return other,
        }
        ai += 1;
        bi += 1;
        match (ai >= a.len(), bi >= b.len()) {
            (true, true) => return Ordering::Equal,
            (true, false) => return Ordering::Less,
            (false, true) => return Ordering::Greater,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_php() {
        // Signs from PHP 8.5.12's strnatcasecmp.
        assert_eq!(strnatcasecmp(b"img12", b"img10"), Ordering::Greater);
        assert_eq!(strnatcasecmp(b"A", b"a"), Ordering::Equal);
        assert_eq!(strnatcasecmp(b"x01", b"x1"), Ordering::Less);
        assert_eq!(strnatcasecmp(b"", b"a"), Ordering::Less);
    }
}
