//! The indexer's time limit reads a clock it is handed, so tests can count checks instead
//! of waiting on a machine.

use std::time::Instant;

pub trait Clock {
    /// Seconds on a monotonic scale; only differences matter.
    fn now(&self) -> f64;
}

/// The real clock: seconds since it was made.
pub struct MonotonicClock(Instant);

impl MonotonicClock {
    pub fn new() -> Self {
        Self(Instant::now())
    }
}

impl Default for MonotonicClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for MonotonicClock {
    fn now(&self) -> f64 {
        self.0.elapsed().as_secs_f64()
    }
}
