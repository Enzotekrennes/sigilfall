//! Where the application reads the time from.
//!
//! Exposes the [`Clock`] trait and [`SystemClock`], the clock of the real
//! game. The application only needs the time for one thing: the grace
//! period of the end screen (`app::END_SCREEN_GRACE`), during which keys
//! left over from the game are ignored. Reading it through a trait lets
//! tests use `ManualClock`, a clock that only moves when told to, so they
//! never sleep and never depend on how fast the machine runs them.
//!
//! This module does not measure game time: turns and energy belong to
//! `sigilfall_core` and never depend on the wall clock.

use std::fmt::Debug;
use std::time::Instant;

#[cfg(test)]
use std::{cell::Cell, rc::Rc, time::Duration};

/// A source of the current time.
pub trait Clock: Debug {
    /// Returns the current instant. Successive calls never go back in time.
    fn now(&self) -> Instant;
}

/// The clock of the real game: the monotonic clock of the system.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// A clock for tests that stands still until [`ManualClock::advance`] moves
/// it.
///
/// Clones share the same time: a test keeps one clone and hands another to
/// the application, then advances its own to let time pass for both.
#[cfg(test)]
#[derive(Debug, Clone)]
pub struct ManualClock {
    /// The instant every clone reads, shared between them.
    now: Rc<Cell<Instant>>,
}

#[cfg(test)]
impl ManualClock {
    /// A clock stopped at the instant it is created.
    pub fn new() -> Self {
        Self {
            now: Rc::new(Cell::new(Instant::now())),
        }
    }

    /// Moves the time of this clock and of all its clones forward by `by`.
    pub fn advance(&self, by: Duration) {
        self.now.set(self.now.get() + by);
    }
}

#[cfg(test)]
impl Default for ManualClock {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl Clock for ManualClock {
    fn now(&self) -> Instant {
        self.now.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manual_clock_stands_still_until_advanced_and_clones_share_it() {
        let clock = ManualClock::new();
        let shared = clock.clone();
        let start = clock.now();
        assert_eq!(shared.now(), start);

        clock.advance(Duration::from_millis(250));

        assert_eq!(shared.now() - start, Duration::from_millis(250));
        assert_eq!(clock.now(), shared.now());
    }
}
