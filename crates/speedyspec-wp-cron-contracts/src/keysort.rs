//! PHP 8's `ksort()` key comparison (`SORT_REGULAR`): WordPress 7.2's `wp_schedule_event()` and
//! `wp_schedule_single_event()` order the cron array with `ksort( $crons )`.
//!
//! Two integer keys compare as integers; two string keys as `zendi_smart_strcmp()` does (numerically when both are
//! numeric strings, else byte for byte); an integer against a string as `compare_longs_to_string()` does
//! (numerically when the string is numeric, else the integer's decimal text against the string, byte for byte).

use std::cmp::Ordering;

use speedyspec_hooks::Key;

/// A numeric string's value, as `is_numeric_string_ex()` reads it.
#[derive(Clone, Copy)]
enum Numeric {
    Long(i64),
    /// `overflow` is the sign of an integer string too large for an integer, else 0.
    Double {
        value: f64,
        overflow: i8,
    },
}

impl Numeric {
    fn as_f64(self) -> f64 {
        match self {
            Numeric::Long(l) => l as f64,
            Numeric::Double { value, .. } => value,
        }
    }
}

/// PHP 8's `is_numeric_string_ex()` without errors allowed: surrounding whitespace, a sign, digits with an optional
/// fraction and exponent. No hex, octal or binary.
fn numeric(text: &[u8]) -> Option<Numeric> {
    let ws = |b: &u8| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c);
    let start = text.iter().position(|b| !ws(b))?;
    let end = text.iter().rposition(|b| !ws(b))? + 1;
    let s = &text[start..end];
    let mut i = usize::from(matches!(s.first(), Some(b'+' | b'-')));
    let int_digits = s[i..].iter().take_while(|b| b.is_ascii_digit()).count();
    i += int_digits;
    let mut frac_digits = 0;
    let mut is_double = false;
    if s.get(i) == Some(&b'.') {
        is_double = true;
        i += 1;
        frac_digits = s[i..].iter().take_while(|b| b.is_ascii_digit()).count();
        i += frac_digits;
    }
    if int_digits + frac_digits == 0 {
        return None;
    }
    if matches!(s.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1;
        if matches!(s.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        let exp_digits = s[j..].iter().take_while(|b| b.is_ascii_digit()).count();
        if exp_digits > 0 {
            is_double = true;
            i = j + exp_digits;
        }
    }
    if i != s.len() {
        return None;
    }
    let text = std::str::from_utf8(s).ok()?;
    if !is_double {
        if let Ok(long) = text.parse::<i64>() {
            return Some(Numeric::Long(long));
        }
        let value: f64 = text.parse().ok()?;
        let overflow = if value < 0.0 { -1 } else { 1 };
        return Some(Numeric::Double { value, overflow });
    }
    Some(Numeric::Double {
        value: text.parse().ok()?,
        overflow: 0,
    })
}

/// `ZEND_NORMALIZE_BOOL( $a - $b )` over doubles: NaN is equal.
fn sign(difference: f64) -> Ordering {
    difference.partial_cmp(&0.0).unwrap_or(Ordering::Equal)
}

/// `zendi_smart_strcmp()`.
fn smart_strcmp(a: &[u8], b: &[u8]) -> Ordering {
    let (Some(x), Some(y)) = (numeric(a), numeric(b)) else {
        return a.cmp(b);
    };
    let (o1, o2) = (
        match x {
            Numeric::Double { overflow, .. } => overflow,
            Numeric::Long(_) => 0,
        },
        match y {
            Numeric::Double { overflow, .. } => overflow,
            Numeric::Long(_) => 0,
        },
    );
    // Both integers overflowed to the same side: their text decides.
    if o1 != 0 && o1 == o2 && x.as_f64() - y.as_f64() == 0.0 {
        return a.cmp(b);
    }
    match (x, y) {
        (Numeric::Long(l1), Numeric::Long(l2)) => l1.cmp(&l2),
        (Numeric::Long(_), Numeric::Double { .. }) if o2 != 0 => {
            if o2 > 0 {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        }
        (Numeric::Double { .. }, Numeric::Long(_)) if o1 != 0 => {
            if o1 > 0 {
                Ordering::Greater
            } else {
                Ordering::Less
            }
        }
        (Numeric::Double { value: d1, .. }, Numeric::Double { value: d2, .. })
            if d1 == d2 && !d1.is_finite() =>
        {
            a.cmp(b)
        }
        _ => sign(x.as_f64() - y.as_f64()),
    }
}

/// `compare_longs_to_string( $long, $text )`.
fn compare_long_to_string(long: i64, text: &[u8]) -> Ordering {
    match numeric(text) {
        Some(Numeric::Long(other)) => long.cmp(&other),
        Some(Numeric::Double { value, .. }) => {
            (long as f64).partial_cmp(&value).unwrap_or(Ordering::Equal)
        }
        None => long.to_string().as_bytes().cmp(text),
    }
}

/// How PHP 8's `ksort()` with `SORT_REGULAR` orders two array keys.
pub fn ksort_compare(a: &Key, b: &Key) -> Ordering {
    match (a, b) {
        (Key::Int(x), Key::Int(y)) => x.cmp(y),
        (Key::Str(x), Key::Str(y)) => smart_strcmp(x.as_bytes(), y.as_bytes()),
        (Key::Int(x), Key::Str(y)) => compare_long_to_string(*x, y.as_bytes()),
        (Key::Str(x), Key::Int(y)) => compare_long_to_string(*y, x.as_bytes()).reverse(),
    }
}

/// PHP's `zend_sort()` (`Zend/zend_sort.c`): insertion sort up to 16 elements, else libc++'s quicksort, comparing
/// with `greater( a, b )` (`cmp( a, b ) > 0`). Ported step for step, so keys whose comparisons are not a total order
/// (a plugin's non-numeric key beside timestamps) land where PHP puts them, and sorting never panics.
pub(crate) fn zend_sort<T>(items: &mut [T], greater: impl Fn(&T, &T) -> bool) {
    let len = items.len();
    Sorter { items, greater }.sort(0, len);
}

struct Sorter<'a, T, F> {
    items: &'a mut [T],
    greater: F,
}

impl<T, F: Fn(&T, &T) -> bool> Sorter<'_, T, F> {
    fn gt(&self, a: usize, b: usize) -> bool {
        (self.greater)(&self.items[a], &self.items[b])
    }

    fn swap(&mut self, a: usize, b: usize) {
        self.items.swap(a, b);
    }

    fn sort2(&mut self, a: usize, b: usize) {
        if self.gt(a, b) {
            self.swap(a, b);
        }
    }

    fn sort3(&mut self, a: usize, b: usize, c: usize) {
        if !self.gt(a, b) {
            if !self.gt(b, c) {
                return;
            }
            self.swap(b, c);
            if self.gt(a, b) {
                self.swap(a, b);
            }
            return;
        }
        if !self.gt(c, b) {
            self.swap(a, c);
            return;
        }
        self.swap(a, b);
        if self.gt(b, c) {
            self.swap(b, c);
        }
    }

    fn sort4(&mut self, a: usize, b: usize, c: usize, d: usize) {
        self.sort3(a, b, c);
        if self.gt(c, d) {
            self.swap(c, d);
            if self.gt(b, c) {
                self.swap(b, c);
                if self.gt(a, b) {
                    self.swap(a, b);
                }
            }
        }
    }

    fn sort5(&mut self, a: usize, b: usize, c: usize, d: usize, e: usize) {
        self.sort4(a, b, c, d);
        if self.gt(d, e) {
            self.swap(d, e);
            if self.gt(c, d) {
                self.swap(c, d);
                if self.gt(b, c) {
                    self.swap(b, c);
                    if self.gt(a, b) {
                        self.swap(a, b);
                    }
                }
            }
        }
    }

    /// Moves the element at `i` down to `j`.
    fn rotate_into(&mut self, i: usize, j: usize) {
        let mut k = i;
        while k > j {
            self.swap(k, k - 1);
            k -= 1;
        }
    }

    /// `zend_insert_sort()` over `len` elements from `start`.
    fn insert_sort(&mut self, start: usize, len: usize) {
        match len {
            0 | 1 => {}
            2 => self.sort2(start, start + 1),
            3 => self.sort3(start, start + 1, start + 2),
            4 => self.sort4(start, start + 1, start + 2, start + 3),
            5 => self.sort5(start, start + 1, start + 2, start + 3, start + 4),
            _ => {
                let end = start + len;
                let sentry = start + 6;
                for i in start + 1..sentry {
                    let mut j = i - 1;
                    if !self.gt(j, i) {
                        continue;
                    }
                    while j != start {
                        j -= 1;
                        if !self.gt(j, i) {
                            j += 1;
                            break;
                        }
                    }
                    self.rotate_into(i, j);
                }
                for i in sentry..end {
                    let mut j = i - 1;
                    if !self.gt(j, i) {
                        continue;
                    }
                    loop {
                        j -= 2;
                        if !self.gt(j, i) {
                            j += 1;
                            if !self.gt(j, i) {
                                j += 1;
                            }
                            break;
                        }
                        if j == start {
                            break;
                        }
                        if j == start + 1 {
                            j -= 1;
                            if self.gt(i, j) {
                                j += 1;
                            }
                            break;
                        }
                    }
                    self.rotate_into(i, j);
                }
            }
        }
    }

    /// `zend_sort()` over `len` elements from `start`.
    fn sort(&mut self, mut start: usize, mut len: usize) {
        loop {
            if len <= 16 {
                self.insert_sort(start, len);
                return;
            }
            let end = start + len;
            let offset = len >> 1;
            let pivot = start + offset;
            if len >> 10 != 0 {
                let delta = offset >> 1;
                self.sort5(start, start + delta, pivot, pivot + delta, end - 1);
            } else {
                self.sort3(start, pivot, end - 1);
            }
            self.swap(start + 1, pivot);
            let pivot = start + 1;
            let mut i = pivot + 1;
            let mut j = end - 1;
            'partition: loop {
                while self.gt(pivot, i) {
                    i += 1;
                    if i == j {
                        break 'partition;
                    }
                }
                j -= 1;
                if j == i {
                    break 'partition;
                }
                while self.gt(j, pivot) {
                    j -= 1;
                    if j == i {
                        break 'partition;
                    }
                }
                self.swap(i, j);
                i += 1;
                if i == j {
                    break 'partition;
                }
            }
            self.swap(pivot, i - 1);
            if (i - 1) - start < end - i {
                self.sort(start, (i - start) - 1);
                start = i;
                len = end - i;
            } else {
                self.sort(i, end - i);
                len = (i - start) - 1;
            }
        }
    }
}
