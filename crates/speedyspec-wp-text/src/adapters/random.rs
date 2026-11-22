//! PHP's `rand()`: MT19937 and the range reduction of PHP 8.5's `ext/random`.

use std::cell::RefCell;

use speedyspec_wp_text_contracts::{RandomSource, TextError};

const N: usize = 624;
const M: usize = 397;

/// PHP 8.5's Mt19937 engine in `MT_RAND_MT19937` mode, with `rand( $min, $max )`'s range reduction, so a seeded
/// run makes PHP's choices.
pub struct PhpMtRand {
    state: RefCell<([u32; N], usize)>,
}

fn twist(m: u32, u: u32, v: u32) -> u32 {
    let mix = (u & 0x8000_0000) | (v & 0x7FFF_FFFF);
    m ^ (mix >> 1) ^ ((v & 1).wrapping_neg() & 0x9908_B0DF)
}

fn reload(state: &mut [u32; N]) {
    for i in 0..N - M {
        state[i] = twist(state[i + M], state[i], state[i + 1]);
    }
    for i in N - M..N - 1 {
        state[i] = twist(state[i + M - N], state[i], state[i + 1]);
    }
    state[N - 1] = twist(state[M - 1], state[N - 1], state[0]);
}

impl PhpMtRand {
    /// The engine after `mt_srand( $seed )`.
    ///
    /// @param seed
    ///     The seed.
    /// @return
    ///     The engine.
    pub fn seeded(seed: u32) -> PhpMtRand {
        let mut state = [0u32; N];
        state[0] = seed;
        for i in 1..N {
            let prev = state[i - 1];
            state[i] = 1_812_433_253u32.wrapping_mul(prev ^ (prev >> 30)).wrapping_add(i as u32);
        }
        reload(&mut state);
        PhpMtRand { state: RefCell::new((state, 0)) }
    }

    /// An engine seeded from the clock and the process, for callers that need no reproducibility.
    ///
    /// @return
    ///     The engine.
    pub fn from_entropy() -> PhpMtRand {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
        PhpMtRand::seeded(nanos ^ std::process::id().rotate_left(16))
    }

    /// One 32-bit output.
    fn next(&self) -> u32 {
        let mut guard = self.state.borrow_mut();
        let (state, count) = &mut *guard;
        if *count >= N {
            reload(state);
            *count = 0;
        }
        let mut s1 = state[*count];
        *count += 1;
        s1 ^= s1 >> 11;
        s1 ^= (s1 << 7) & 0x9D2C_5680;
        s1 ^= (s1 << 15) & 0xEFC6_0000;
        s1 ^ (s1 >> 18)
    }

    /// `php_random_range64()`: two outputs make 64 bits.
    fn range64(&self, umax: u64) -> u64 {
        let draw = || u64::from(self.next()) | (u64::from(self.next()) << 32);
        let mut result = draw();
        if umax == u64::MAX {
            return result;
        }
        let umax = umax + 1;
        if umax & (umax - 1) == 0 {
            return result & (umax - 1);
        }
        let limit = u64::MAX - (u64::MAX % umax) - 1;
        while result > limit {
            result = draw();
        }
        result % umax
    }

    /// `php_random_range32()`.
    fn range32(&self, umax: u32) -> u32 {
        let mut result = self.next();
        if umax == u32::MAX {
            return result;
        }
        let umax = umax + 1;
        if umax & (umax - 1) == 0 {
            return result & (umax - 1);
        }
        let limit = u32::MAX - (u32::MAX % umax) - 1;
        while result > limit {
            result = self.next();
        }
        result % umax
    }
}

impl RandomSource for PhpMtRand {
    fn rand(&self, min: i64, max: i64) -> Result<i64, TextError> {
        let (min, max) = if max < min { (max, min) } else { (min, max) };
        let umax = (max as u64).wrapping_sub(min as u64);
        Ok(if umax > u64::from(u32::MAX) {
            (self.range64(umax) as i64).wrapping_add(min)
        } else {
            i64::from(self.range32(umax as u32)).wrapping_add(min)
        })
    }
}
