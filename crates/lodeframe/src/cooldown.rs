// SPDX-License-Identifier: Apache-2.0 OR MIT
//! How long until something can be used again.
//!
//! A [`Cooldown`] is a small value that remembers when it ends. Keep it where the thing it
//! limits belongs: in a player's [`Data`](crate::data::Data) under a [`Key`](crate::data::Key),
//! for example, so that it goes with the player. It is the server's own bookkeeping; the client
//! is not told, so it shows nothing on an item.
//!
//! ```
//! use lodeframe::{cooldown::Cooldown, data::{Data, Key}, schedule::{Delay, Tick}};
//!
//! const FIRE: Key<Cooldown> = Key::new("mygame:fire");
//!
//! let mut data = Data::default();
//! let now = Tick::default();
//! let cooldown = data.get_or_insert_with(&FIRE, Cooldown::default);
//! assert!(cooldown.try_use(now, Delay::secs(3)));
//! assert!(!cooldown.try_use(now, Delay::secs(3)));
//! ```

use crate::schedule::{Delay, Tick};

/// When something can be used again. A new one (or the [`Default`]) can be used at once.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cooldown {
    until: Tick,
}

impl Cooldown {
    /// Whether it can be used at `now`.
    pub fn ready(&self, now: Tick) -> bool {
        now >= self.until
    }

    /// Starts it over at `now`, whether or not it was ready: it can be used again `delay` ticks
    /// from `now`. A delay of zero means it can always be used.
    pub fn start(&mut self, now: Tick, delay: Delay) {
        self.until = now + delay;
    }

    /// Starts it and returns `true` if it was ready. Otherwise returns `false` and leaves it as
    /// it was: trying again does not make the wait longer.
    pub fn try_use(&mut self, now: Tick, delay: Delay) -> bool {
        let ready = self.ready(now);
        if ready {
            self.start(now, delay);
        }
        ready
    }

    /// How long until it is ready at `now`; [`Delay::ZERO`] if it is.
    pub fn remaining(&self, now: Tick) -> Delay {
        Delay::ticks(self.until.as_ticks().saturating_sub(now.as_ticks()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(n: u64) -> Tick {
        Tick::default() + Delay::ticks(n)
    }

    #[test]
    fn a_new_cooldown_is_ready_and_a_used_one_waits_for_its_delay() {
        let mut cd = Cooldown::default();
        assert!(cd.ready(at(0)));
        assert!(cd.try_use(at(10), Delay::ticks(5)));

        assert!(!cd.ready(at(10)));
        assert!(!cd.ready(at(14)));
        assert!(cd.ready(at(15)));
        assert!(cd.try_use(at(15), Delay::ticks(5)));
    }

    #[test]
    fn a_failed_try_does_not_extend_the_wait() {
        let mut cd = Cooldown::default();
        cd.try_use(at(0), Delay::ticks(5));

        assert!(!cd.try_use(at(3), Delay::ticks(100)));
        assert!(cd.ready(at(5)));
    }

    #[test]
    fn start_begins_again_even_when_it_was_not_ready() {
        let mut cd = Cooldown::default();
        cd.try_use(at(0), Delay::ticks(5));
        cd.start(at(3), Delay::ticks(10));

        assert!(!cd.ready(at(12)));
        assert!(cd.ready(at(13)));
    }

    #[test]
    fn remaining_counts_down_to_zero() {
        let mut cd = Cooldown::default();
        cd.start(at(0), Delay::ticks(5));

        assert_eq!(cd.remaining(at(0)), Delay::ticks(5));
        assert_eq!(cd.remaining(at(3)), Delay::ticks(2));
        assert_eq!(cd.remaining(at(5)), Delay::ZERO);
        assert_eq!(cd.remaining(at(50)), Delay::ZERO);
    }

    #[test]
    fn a_zero_delay_can_always_be_used_and_a_huge_one_does_not_overflow() {
        let mut cd = Cooldown::default();
        assert!(cd.try_use(at(1), Delay::ZERO));
        assert!(cd.try_use(at(1), Delay::ZERO));

        cd.start(at(1), Delay::ticks(u64::MAX));
        assert!(!cd.ready(at(1_000_000)));
    }
}
