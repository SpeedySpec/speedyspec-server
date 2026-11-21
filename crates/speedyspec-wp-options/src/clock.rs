//! `time()` as a port, so expirations can be tested without waiting.

use std::time::{SystemTime, UNIX_EPOCH};

/// The current Unix time in seconds, as PHP's `time()`.
pub trait Clock {
    fn now(&self) -> i64;
}

/// The system clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }
}
