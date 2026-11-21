//! The byte scans the diff spends its string time in, 16 bytes at a time.
//!
//! SSE2 on x86_64 and NEON on aarch64 are part of each target's baseline, so neither needs run-time detection.
//! Every scan has a scalar twin in [`scalar`]: the fallback on other targets, and the oracle the vector code is
//! tested against.

/// Whether `b` is a byte `htmlspecialchars()` replaces: `&`, `"`, `'`, `<` or `>`.
#[inline]
pub(crate) fn is_escaped(b: u8) -> bool {
    matches!(b, b'&' | b'"' | b'\'' | b'<' | b'>')
}

/// Whether `b` is an ASCII byte PCRE's `\w` matches: `[0-9A-Za-z_]`.
#[inline]
pub(crate) fn is_ascii_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// The scalar scans.
pub mod scalar {
    use super::{is_ascii_word, is_escaped};

    /// The index of the first byte `htmlspecialchars()` replaces, if any.
    pub fn find_escaped(s: &[u8]) -> Option<usize> {
        s.iter().position(|&b| is_escaped(b))
    }

    /// The length of the run of ASCII word bytes `s` starts with.
    pub fn ascii_word_run(s: &[u8]) -> usize {
        s.iter().position(|&b| !is_ascii_word(b)).unwrap_or(s.len())
    }
}

/// The index of the first byte `htmlspecialchars()` replaces (`&`, `"`, `'`, `<`, `>`), if any.
#[inline]
pub fn find_escaped(s: &[u8]) -> Option<usize> {
    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: NEON is part of the aarch64 baseline.
        unsafe { neon::find_escaped(s) }
    }
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: SSE2 is part of the x86_64 baseline.
        unsafe { sse2::find_escaped(s) }
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        scalar::find_escaped(s)
    }
}

/// The length of the run of ASCII word bytes (`[0-9A-Za-z_]`) `s` starts with.
#[inline]
pub fn ascii_word_run(s: &[u8]) -> usize {
    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: NEON is part of the aarch64 baseline.
        unsafe { neon::ascii_word_run(s) }
    }
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: SSE2 is part of the x86_64 baseline.
        unsafe { sse2::ascii_word_run(s) }
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        scalar::ascii_word_run(s)
    }
}

#[cfg(target_arch = "aarch64")]
mod neon {
    use core::arch::aarch64::*;

    /// A 4-bits-per-byte mask of `v`'s set lanes (the `shrn` trick), as a `u64`.
    #[inline(always)]
    unsafe fn nibble_mask(v: uint8x16_t) -> u64 {
        unsafe {
            let narrowed = vshrn_n_u16::<4>(vreinterpretq_u16_u8(v));
            vget_lane_u64::<0>(vreinterpret_u64_u8(narrowed))
        }
    }

    #[inline(always)]
    unsafe fn escaped_lanes(v: uint8x16_t) -> uint8x16_t {
        unsafe {
            let amp = vceqq_u8(v, vdupq_n_u8(b'&'));
            let dq = vceqq_u8(v, vdupq_n_u8(b'"'));
            let sq = vceqq_u8(v, vdupq_n_u8(b'\''));
            let lt = vceqq_u8(v, vdupq_n_u8(b'<'));
            let gt = vceqq_u8(v, vdupq_n_u8(b'>'));
            vorrq_u8(vorrq_u8(vorrq_u8(amp, dq), vorrq_u8(sq, lt)), gt)
        }
    }

    #[inline(always)]
    unsafe fn non_word_lanes(v: uint8x16_t) -> uint8x16_t {
        unsafe {
            let digit = vcltq_u8(vsubq_u8(v, vdupq_n_u8(b'0')), vdupq_n_u8(10));
            let lower = vorrq_u8(v, vdupq_n_u8(0x20));
            let alpha = vcltq_u8(vsubq_u8(lower, vdupq_n_u8(b'a')), vdupq_n_u8(26));
            let under = vceqq_u8(v, vdupq_n_u8(b'_'));
            vmvnq_u8(vorrq_u8(vorrq_u8(digit, alpha), under))
        }
    }

    pub(super) unsafe fn find_escaped(s: &[u8]) -> Option<usize> {
        let mut i = 0;
        while i + 16 <= s.len() {
            // SAFETY: `i + 16 <= s.len()`.
            let mask = unsafe { nibble_mask(escaped_lanes(vld1q_u8(s.as_ptr().add(i)))) };
            if mask != 0 {
                return Some(i + (mask.trailing_zeros() / 4) as usize);
            }
            i += 16;
        }
        super::scalar::find_escaped(&s[i..]).map(|j| i + j)
    }

    pub(super) unsafe fn ascii_word_run(s: &[u8]) -> usize {
        let mut i = 0;
        while i + 16 <= s.len() {
            // SAFETY: `i + 16 <= s.len()`.
            let mask = unsafe { nibble_mask(non_word_lanes(vld1q_u8(s.as_ptr().add(i)))) };
            if mask != 0 {
                return i + (mask.trailing_zeros() / 4) as usize;
            }
            i += 16;
        }
        i + super::scalar::ascii_word_run(&s[i..])
    }
}

#[cfg(target_arch = "x86_64")]
mod sse2 {
    use core::arch::x86_64::*;

    #[inline(always)]
    unsafe fn escaped_mask(v: __m128i) -> u32 {
        unsafe {
            let amp = _mm_cmpeq_epi8(v, _mm_set1_epi8(b'&' as i8));
            let dq = _mm_cmpeq_epi8(v, _mm_set1_epi8(b'"' as i8));
            let sq = _mm_cmpeq_epi8(v, _mm_set1_epi8(b'\'' as i8));
            let lt = _mm_cmpeq_epi8(v, _mm_set1_epi8(b'<' as i8));
            let gt = _mm_cmpeq_epi8(v, _mm_set1_epi8(b'>' as i8));
            let any = _mm_or_si128(
                _mm_or_si128(_mm_or_si128(amp, dq), _mm_or_si128(sq, lt)),
                gt,
            );
            _mm_movemask_epi8(any) as u32
        }
    }

    /// Lanes where `lo <= v < lo + n`, unsigned: `min(v - lo, n - 1) == v - lo`.
    #[inline(always)]
    unsafe fn in_range(v: __m128i, lo: u8, n: u8) -> __m128i {
        unsafe {
            let shifted = _mm_sub_epi8(v, _mm_set1_epi8(lo as i8));
            _mm_cmpeq_epi8(_mm_min_epu8(shifted, _mm_set1_epi8((n - 1) as i8)), shifted)
        }
    }

    #[inline(always)]
    unsafe fn word_mask(v: __m128i) -> u32 {
        unsafe {
            let digit = in_range(v, b'0', 10);
            let alpha = in_range(_mm_or_si128(v, _mm_set1_epi8(0x20)), b'a', 26);
            let under = _mm_cmpeq_epi8(v, _mm_set1_epi8(b'_' as i8));
            _mm_movemask_epi8(_mm_or_si128(_mm_or_si128(digit, alpha), under)) as u32
        }
    }

    pub(super) unsafe fn find_escaped(s: &[u8]) -> Option<usize> {
        let mut i = 0;
        while i + 16 <= s.len() {
            // SAFETY: `i + 16 <= s.len()`; unaligned load.
            let mask = unsafe { escaped_mask(_mm_loadu_si128(s.as_ptr().add(i).cast())) };
            if mask != 0 {
                return Some(i + mask.trailing_zeros() as usize);
            }
            i += 16;
        }
        super::scalar::find_escaped(&s[i..]).map(|j| i + j)
    }

    pub(super) unsafe fn ascii_word_run(s: &[u8]) -> usize {
        let mut i = 0;
        while i + 16 <= s.len() {
            // SAFETY: `i + 16 <= s.len()`; unaligned load.
            let mask = unsafe { word_mask(_mm_loadu_si128(s.as_ptr().add(i).cast())) } ^ 0xFFFF;
            if mask != 0 {
                return i + mask.trailing_zeros() as usize;
            }
            i += 16;
        }
        i + super::scalar::ascii_word_run(&s[i..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn finds_each_escaped_byte_in_every_lane() {
        for len in 0..50 {
            for at in 0..len {
                for b in *b"&\"'<>" {
                    let mut s = vec![b'a'; len];
                    s[at] = b;
                    assert_eq!(find_escaped(&s), Some(at), "len {len} at {at}");
                }
            }
            assert_eq!(find_escaped(&vec![b'a'; len]), None);
        }
    }

    #[test]
    fn word_runs_stop_at_every_non_word_byte() {
        for b in 0..=255u8 {
            let mut s = vec![b'x'; 40];
            s[21] = b;
            let expected = if is_ascii_word(b) { 40 } else { 21 };
            assert_eq!(ascii_word_run(&s), expected, "byte {b:#x}");
        }
    }

    proptest! {
        #[test]
        fn find_escaped_matches_scalar(s in proptest::collection::vec(any::<u8>(), 0..100), skip in 0usize..16) {
            let s = &s[skip.min(s.len())..];
            prop_assert_eq!(find_escaped(s), scalar::find_escaped(s));
        }

        #[test]
        fn ascii_word_run_matches_scalar(s in proptest::collection::vec(prop_oneof![Just(b'a'), Just(b'_'), Just(b'9'), any::<u8>()], 0..100), skip in 0usize..16) {
            let s = &s[skip.min(s.len())..];
            prop_assert_eq!(ascii_word_run(s), scalar::ascii_word_run(s));
        }
    }
}
