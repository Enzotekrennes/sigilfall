//! Where the application reads the time from.
//!
//! Exposes the [`Clock`] trait, [`SystemClock`], the clock of the real
//! game, and [`utc_date`], which turns a time of the system clock into its
//! UTC date. The application needs the time for two things: the grace
//! period of the end screen (`app::END_SCREEN_GRACE`), during which keys
//! left over from the game are ignored, and today's UTC date, which names
//! the daily dungeon (`DailyDate`, `Seed::daily`). Reading both through a
//! trait lets tests use `ManualClock`, a clock that only moves when told
//! to, so they never sleep, never depend on how fast the machine runs them,
//! and choose the date, midnight included.
//!
//! This module does not measure game time: turns and energy belong to
//! `sigilfall_core` and never depend on the wall clock. It knows no time
//! zone but UTC: every player gets the same daily dungeon at the same
//! moment, wherever they are.

use std::fmt::Debug;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use sigilfall_core::rng::DailyDate;

#[cfg(test)]
use std::{cell::Cell, rc::Rc, time::Duration};

/// Seconds in a UTC day: Unix time counts every day as exactly this long,
/// leap seconds left out, so dividing by it gives the days since 1970-01-01.
const SECONDS_PER_DAY: u64 = 86_400;

/// Days from 0000-03-01 to 1970-01-01 in the proleptic Gregorian calendar,
/// the offset of the civil-date algorithm of [`utc_date`] (H. Hinnant,
/// "chrono-Compatible Low-Level Date Algorithms", function
/// `civil_from_days`).
const DAYS_FROM_YEAR_ZERO_TO_EPOCH: u64 = 719_468;

/// Days in a 400-year cycle of the Gregorian calendar, after which dates
/// repeat on the same days of the week and leap years return in the same
/// places.
const DAYS_PER_ERA: u64 = 146_097;

/// A source of the current time.
pub trait Clock: Debug {
    /// Returns the current instant. Successive calls never go back in time.
    fn now(&self) -> Instant;

    /// Returns today's date in Coordinated Universal Time, which names the
    /// daily dungeon. It changes at midnight UTC for every player at once.
    fn today(&self) -> DailyDate;
}

/// The clock of the real game: the monotonic clock of the system for
/// instants, and the system's wall clock for the date.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    /// Reads the system's wall clock. A clock set before 1970-01-01 (which
    /// Unix time cannot count forward from) gives 1970-01-01, so the game
    /// still starts a daily dungeon instead of failing.
    fn today(&self) -> DailyDate {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        utc_date(seconds)
    }
}

/// Returns the UTC date of the instant `unix_seconds` seconds after
/// 1970-01-01 00:00:00 UTC (Unix time).
///
/// A day starts at its midnight: 86 399 is still 1970-01-01, 86 400 is
/// 1970-01-02. The conversion is integer arithmetic only (the
/// `civil_from_days` algorithm of H. Hinnant, which counts years from
/// 1 March so that the leap day ends them), exact on every platform. An
/// instant past 9999-12-31, out of reach of a real clock, gives
/// 9999-12-31, the last date `DailyDate` holds.
pub fn utc_date(unix_seconds: u64) -> DailyDate {
    let last = DailyDate::new(DailyDate::MAX_YEAR, 12, 31).expect("9999-12-31 exists");
    let days = unix_seconds / SECONDS_PER_DAY + DAYS_FROM_YEAR_ZERO_TO_EPOCH;
    let era = days / DAYS_PER_ERA;
    let day_of_era = days % DAYS_PER_ERA;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    // Months counted from March (0) to February (11), each 153 days per 5.
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = era * 400 + year_of_era + u64::from(month <= 2);
    let (Ok(year), Ok(month), Ok(day)) =
        (u16::try_from(year), u8::try_from(month), u8::try_from(day))
    else {
        return last;
    };
    DailyDate::new(year, month, day).unwrap_or(last)
}

/// A clock for tests that stands still until [`ManualClock::advance`] moves
/// it.
///
/// Clones share the same time: a test keeps one clone and hands another to
/// the application, then advances its own to let time pass for both. Its
/// wall clock starts at [`ManualClock::START`], so screens that show the
/// date draw the same in every run; [`ManualClock::at_unix_seconds`] sets
/// another.
#[cfg(test)]
#[derive(Debug, Clone)]
pub struct ManualClock {
    /// The instant every clone reads, shared between them.
    now: Rc<Cell<Instant>>,
    /// The wall-clock time every clone reads, as the time elapsed since
    /// 1970-01-01 00:00:00 UTC (Unix time), shared between them.
    unix_time: Rc<Cell<Duration>>,
}

#[cfg(test)]
impl ManualClock {
    /// Unix time a new manual clock reads: 2026-11-08 at 12:00:00 UTC, the
    /// middle of a day, so moving it by hours never changes the date by
    /// surprise.
    pub const START: u64 = 1_794_139_200;

    /// A clock stopped at the instant it is created, on the wall-clock time
    /// [`ManualClock::START`].
    pub fn new() -> Self {
        Self::at_unix_seconds(Self::START)
    }

    /// A clock stopped at the instant it is created, on the wall-clock time
    /// `unix_seconds` (seconds since 1970-01-01 00:00:00 UTC).
    pub fn at_unix_seconds(unix_seconds: u64) -> Self {
        Self {
            now: Rc::new(Cell::new(Instant::now())),
            unix_time: Rc::new(Cell::new(Duration::from_secs(unix_seconds))),
        }
    }

    /// Moves the time of this clock and of all its clones forward by `by`,
    /// the wall clock as much as the instants.
    pub fn advance(&self, by: Duration) {
        self.now.set(self.now.get() + by);
        self.unix_time.set(self.unix_time.get() + by);
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

    fn today(&self) -> DailyDate {
        utc_date(self.unix_time.get().as_secs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The date `year`-`month`-`day`, which the test knows to exist.
    fn date(year: u16, month: u8, day: u8) -> DailyDate {
        DailyDate::new(year, month, day).expect("a real date")
    }

    /// Unix time of midnight UTC opening `year`-`month`-`day`, counted
    /// independently of [`utc_date`]: whole years, then whole months.
    fn midnight(year: u16, month: u8, day: u8) -> u64 {
        let leap =
            |y: u16| y.is_multiple_of(4) && (!y.is_multiple_of(100) || y.is_multiple_of(400));
        let days_in = |y: u16, m: u8| match m {
            2 if leap(y) => 29,
            2 => 28,
            4 | 6 | 9 | 11 => 30,
            _ => 31,
        };
        let years: u64 = (1970..year).map(|y| if leap(y) { 366 } else { 365 }).sum();
        let months: u64 = (1..month).map(|m| days_in(year, m)).sum();
        (years + months + u64::from(day) - 1) * SECONDS_PER_DAY
    }

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

    #[test]
    fn a_manual_clock_starts_on_the_middle_of_a_known_day() {
        assert_eq!(ManualClock::START, midnight(2026, 11, 8) + 12 * 3600);
        assert_eq!(ManualClock::new().today(), date(2026, 11, 8));
    }

    #[test]
    fn the_date_turns_at_midnight_utc_on_the_injected_clock() {
        let clock = ManualClock::at_unix_seconds(midnight(2026, 11, 9) - 1);
        let shared = clock.clone();
        assert_eq!(shared.today(), date(2026, 11, 8), "23:59:59");

        clock.advance(Duration::from_secs(1));
        assert_eq!(shared.today(), date(2026, 11, 9), "00:00:00");

        clock.advance(Duration::from_secs(SECONDS_PER_DAY - 1));
        assert_eq!(shared.today(), date(2026, 11, 9), "23:59:59 the next day");

        clock.advance(Duration::from_millis(999));
        assert_eq!(shared.today(), date(2026, 11, 9), "less than a second");
        clock.advance(Duration::from_millis(1));
        assert_eq!(shared.today(), date(2026, 11, 10), "a second in all");
    }

    #[test]
    fn the_year_and_the_leap_day_turn_at_midnight_too() {
        for (before, after) in [
            ((2026, 12, 31), (2027, 1, 1)),
            ((2028, 2, 28), (2028, 2, 29)),
            ((2028, 2, 29), (2028, 3, 1)),
            ((2100, 2, 28), (2100, 3, 1)),
            ((2000, 2, 28), (2000, 2, 29)),
        ] {
            let (year, month, day) = after;
            let turn = midnight(year, month, day);
            let (year_before, month_before, day_before) = before;

            assert_eq!(
                utc_date(turn - 1),
                date(year_before, month_before, day_before),
                "{before:?}"
            );
            assert_eq!(utc_date(turn), date(year, month, day), "{after:?}");
        }
    }

    #[test]
    fn utc_dates_match_an_independent_count_over_twelve_years() {
        let mut checked = 0;
        for year in 1970..=1971 {
            for month in 1..=12 {
                for day in 1..=31 {
                    let Some(expected) = DailyDate::new(year, month, day) else {
                        continue;
                    };
                    assert_eq!(utc_date(midnight(year, month, day)), expected);
                    checked += 1;
                }
            }
        }
        for year in 2024..=2033 {
            for month in 1..=12 {
                for day in 1..=31 {
                    let Some(expected) = DailyDate::new(year, month, day) else {
                        continue;
                    };
                    let noon = midnight(year, month, day) + SECONDS_PER_DAY / 2;
                    assert_eq!(utc_date(noon), expected);
                    checked += 1;
                }
            }
        }
        assert_eq!(checked, 730 + 3653);
    }

    #[test]
    fn known_instants_have_their_published_dates() {
        assert_eq!(utc_date(0), date(1970, 1, 1));
        // 2000-03-01T00:00:00Z and 2038-01-19T03:14:07Z (2^31 - 1).
        assert_eq!(utc_date(951_868_800), date(2000, 3, 1));
        assert_eq!(utc_date(2_147_483_647), date(2038, 1, 19));
    }

    #[test]
    fn instants_beyond_the_last_date_give_the_last_date() {
        let last = date(9999, 12, 31);

        assert_eq!(utc_date(midnight(9999, 12, 31)), last);
        assert_eq!(utc_date(midnight(9999, 12, 31) + SECONDS_PER_DAY), last);
        assert_eq!(utc_date(u64::MAX), last);
    }

    #[test]
    fn the_system_clock_gives_a_date_of_this_era() {
        let today = SystemClock.today();

        assert!(today.year() >= 2026, "{today}");
    }
}
