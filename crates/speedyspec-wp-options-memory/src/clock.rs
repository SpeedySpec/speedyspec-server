//! `FixedClock`: a `Clock` that tells the time it is set to.

use std::cell::Cell;

use speedyspec_wp_options::Clock;

pub struct FixedClock(Cell<i64>);

impl FixedClock {
    pub fn new(now: i64) -> FixedClock {
        FixedClock(Cell::new(now))
    }

    pub fn set(&self, now: i64) {
        self.0.set(now);
    }
}

impl Clock for FixedClock {
    fn now(&self) -> i64 {
        self.0.get()
    }
}
