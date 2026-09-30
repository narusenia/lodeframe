// SPDX-License-Identifier: Apache-2.0 OR MIT
//! The time source of the tick loop, replaceable so tests need not wait.

use std::time::Instant;

/// What the tick loop needs from a clock.
pub trait Clock {
    /// The current time.
    fn now(&self) -> Instant;
    /// Blocks until `t` (returns at once if it has passed).
    fn sleep_until(&self, t: Instant);
}

/// The real clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn sleep_until(&self, t: Instant) {
        // ponytail: thread::sleep wakes up to a few ms late on some systems; spin the last ms if jitter shows
        std::thread::sleep(t.saturating_duration_since(Instant::now()));
    }
}
