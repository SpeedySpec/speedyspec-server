//! `count_chars()` and the L1 distance WordPress pairs changed lines by.

/// Byte counts of `s`, as `count_chars( $s )` (mode 0).
///
/// Four interleaved tables, summed at the end, so runs of one byte don't stall on the same counter.
pub fn byte_counts(s: &[u8]) -> [u32; 256] {
    let mut tables = [[0u32; 256]; 4];
    let (chunks, remainder) = s.as_chunks::<4>();
    for c in chunks {
        tables[0][c[0] as usize] += 1;
        tables[1][c[1] as usize] += 1;
        tables[2][c[2] as usize] += 1;
        tables[3][c[3] as usize] += 1;
    }
    for &b in remainder {
        tables[0][b as usize] += 1;
    }
    let mut out = [0u32; 256];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = tables[0][i] + tables[1][i] + tables[2][i] + tables[3][i];
    }
    out
}

/// The sum of absolute differences of two byte counts; written so the compiler vectorizes it.
pub fn l1_distance(a: &[u32; 256], b: &[u32; 256]) -> u64 {
    let mut lanes = [0u64; 8];
    for (ca, cb) in a.as_chunks::<8>().0.iter().zip(b.as_chunks::<8>().0) {
        for k in 0..8 {
            lanes[k] += ca[k].abs_diff(cb[k]) as u64;
        }
    }
    lanes.iter().sum()
}

/// `WP_Text_Diff_Renderer_Table::compute_string_distance()`: the L1 distance per byte of the first string, or the
/// whole distance when the first string is falsy (`''` or `'0'`).
pub fn string_distance(s1: &[u8], counts1: &[u32; 256], counts2: &[u32; 256]) -> f64 {
    let difference = l1_distance(counts1, counts2) as f64;
    if s1.is_empty() || s1 == b"0" {
        return difference;
    }
    difference / s1.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_bytes() {
        let c = byte_counts(b"abracadabra");
        assert_eq!(c[b'a' as usize], 5);
        assert_eq!(c[b'b' as usize], 2);
        assert_eq!(c.iter().sum::<u32>(), 11);
    }

    #[test]
    fn distance_per_byte_of_the_first_string() {
        let (a, b) = (b"abc".as_slice(), b"abd".as_slice());
        assert_eq!(
            string_distance(a, &byte_counts(a), &byte_counts(b)),
            2.0 / 3.0
        );
        assert_eq!(
            string_distance(b"0", &byte_counts(b"0"), &byte_counts(b"")),
            1.0
        );
    }
}
