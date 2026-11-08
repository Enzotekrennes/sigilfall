//! The run's seed and the random number streams derived from it.
//!
//! Exposes [`Seed`], [`Purpose`], [`ParseSeedError`] and [`DailyDate`], the
//! UTC date of a daily dungeon. Each (seed, depth,
//! purpose) triple gets its own [`ChaCha8Rng`] stream, so a floor's layout
//! never depends on what the player did on earlier floors, and fights never
//! shift the dungeon. The derivation only uses fixed integer arithmetic and
//! little-endian bytes: the same seed gives the same streams on every
//! platform and every version of the game, which shared seeds, the daily
//! seed and the tests rely on.
//!
//! The daily seed ([`Seed::daily`]) is a pure function of a UTC date: every
//! player who starts the dungeon of the same date gets the same seed, with
//! no server involved.
//!
//! This module never draws from the operating system and never reads the
//! clock: picking a seed when the player gives none, and knowing today's
//! date, are the binary's job.

use std::fmt;
use std::str::FromStr;

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

/// Increment of the SplitMix64 generator: 2^64 divided by the golden ratio,
/// rounded to an odd number (Steele, Lea and Flood, "Fast splittable
/// pseudorandom number generators", 2014).
const SPLITMIX_GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

/// Number of 64-bit words in a [`ChaCha8Rng`] seed (a 256-bit key).
const KEY_WORDS: usize = 4;

/// Number of hexadecimal digits in each group of a seed's written form.
const GROUP_DIGITS: usize = 4;

/// Number of groups in a seed's written form: 4 groups of 4 hexadecimal
/// digits hold the 64 bits of a seed.
const GROUPS: usize = 4;

/// Tag mixed into every daily seed: the bytes of the ASCII text
/// `SIGILDAY`, read as a big-endian number.
///
/// It sets the daily seeds apart from the plain output of the mixing
/// function on small numbers. Like [`Purpose::tag`], it is part of the seed
/// format: changing it gives every date another dungeon.
const DAILY_TAG: u64 = 0x5349_4749_4C44_4159;

/// The seed of a run: every random choice of the game follows from it.
///
/// A seed is written as 16 upper-case hexadecimal digits in four groups,
/// such as `7F3A-91C2-0B5E-D4A8` (see the [`fmt::Display`] implementation),
/// and parsed from that form or from a decimal number (see [`Seed::from_str`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Seed(u64);

impl Seed {
    /// Wraps `value`; every 64-bit value is a valid seed.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the seed as a number.
    pub const fn value(self) -> u64 {
        self.0
    }

    /// Returns the random number stream for `purpose` on floor `depth`.
    ///
    /// The same seed, depth and purpose always give the same sequence of
    /// values. Two different depths or purposes give two independent
    /// streams: their 256-bit keys are always different for a given seed.
    /// `depth` is the floor number (1 for the first floor); any value is
    /// accepted.
    pub fn stream(self, depth: u32, purpose: Purpose) -> ChaCha8Rng {
        ChaCha8Rng::from_seed(self.stream_key(depth, purpose))
    }

    /// Derives the 256-bit key of the stream for `depth` and `purpose`.
    ///
    /// The depth (high 32 bits) and the purpose's tag (low bits) are packed
    /// into one 64-bit word without overlap. The seed and that word are each
    /// passed through the SplitMix64 mixing function, which is a bijection,
    /// and combined with an exclusive or: for a fixed seed, two different
    /// (depth, purpose) pairs give two different starting states. That state
    /// seeds a SplitMix64 generator whose next four outputs form the key, in
    /// little-endian order so the key does not depend on the platform.
    fn stream_key(self, depth: u32, purpose: Purpose) -> [u8; 32] {
        let context = (u64::from(depth) << 32) | u64::from(purpose.tag());
        let mut state = mix(self.0.wrapping_add(SPLITMIX_GAMMA)) ^ mix(context);
        let mut key = [0; KEY_WORDS * 8];
        let (words, _) = key.as_chunks_mut::<8>();
        for word in words {
            state = state.wrapping_add(SPLITMIX_GAMMA);
            *word = mix(state).to_le_bytes();
        }
        key
    }

    /// Returns the seed of the daily dungeon of `date`, the same for every
    /// player and on every platform.
    ///
    /// The date is packed into one number without overlap, the year in the
    /// bits 16 and up, the month in the bits 8 to 15 and the day in the
    /// bits 0 to 7 (2026-11-08 gives `0x07EA_0B08`). That number is combined
    /// with a fixed tag (the ASCII bytes of `SIGILDAY`) by an exclusive or,
    /// then passed through the SplitMix64 mixing function. Both steps are
    /// bijections, so two different dates always give two different seeds,
    /// and the mixing spreads neighbouring dates over the whole range of
    /// seeds. Only fixed integer arithmetic is used: the value never
    /// depends on the platform.
    pub const fn daily(date: DailyDate) -> Self {
        Self(mix(date.packed() ^ DAILY_TAG))
    }
}

impl From<u64> for Seed {
    /// Same as [`Seed::new`].
    fn from(value: u64) -> Self {
        Self::new(value)
    }
}

impl fmt::Display for Seed {
    /// Writes the seed as 16 upper-case hexadecimal digits in four groups
    /// joined by hyphens, most significant first, such as
    /// `7F3A-91C2-0B5E-D4A8`: the form shown to players and accepted back by
    /// [`Seed::from_str`].
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [a, b, c, d] = [48, 32, 16, 0].map(|shift| (self.0 >> shift) & 0xFFFF);
        write!(formatter, "{a:04X}-{b:04X}-{c:04X}-{d:04X}")
    }
}

impl FromStr for Seed {
    type Err = ParseSeedError;

    /// Reads a seed written either as a decimal number from 0 to
    /// 18446744073709551615 (such as `42`), or as the four groups of four
    /// hexadecimal digits shown by the game (such as `7F3A-91C2-0B5E-D4A8`,
    /// in upper or lower case).
    ///
    /// # Errors
    ///
    /// Returns [`ParseSeedError`] for anything else: letters, a negative or
    /// too large number, surrounding spaces, or groups of the wrong length.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if let Ok(value) = text.parse::<u64>() {
            return Ok(Self(value));
        }
        parse_groups(text).map(Self).ok_or(ParseSeedError)
    }
}

/// Reads the grouped hexadecimal form of a seed (see [`Seed::from_str`]),
/// or returns `None` if `text` is not exactly in that form.
fn parse_groups(text: &str) -> Option<u64> {
    let groups: Vec<&str> = text.split('-').collect();
    if groups.len() != GROUPS {
        return None;
    }
    groups.into_iter().try_fold(0_u64, |value, group| {
        let well_formed =
            group.len() == GROUP_DIGITS && group.bytes().all(|byte| byte.is_ascii_hexdigit());
        let group = u16::from_str_radix(group, 16)
            .ok()
            .filter(|_| well_formed)?;
        Some((value << 16) | u64::from(group))
    })
}

/// The SplitMix64 mixing function (finaliser "Mix13" of D. Stafford, used
/// by SplitMix64): spreads every input bit over every output bit, and maps
/// distinct inputs to distinct outputs.
const fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// What a random number stream is used for; each purpose has its own
/// stream on each floor (see [`Seed::stream`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Purpose {
    /// Building a floor: layout, stairs, monsters and items placed on it.
    Generation,
    /// Everything decided while playing: combat rolls and monster choices.
    Gameplay,
    /// Choosing how a floor is laid out (rooms or caves), before
    /// [`Purpose::Generation`] lays it out.
    ///
    /// The choice has its own stream so that adding it left every layout
    /// drawn from the generation stream as it was: a floor of rooms is the
    /// same whether or not caves could have been drawn instead.
    FloorKind,
}

impl Purpose {
    /// Returns the number that identifies this purpose in the stream
    /// derivation.
    ///
    /// The values are part of the seed format: changing one changes every
    /// stream of that purpose, so existing seeds would give other dungeons.
    const fn tag(self) -> u32 {
        match self {
            Self::Generation => 1,
            Self::Gameplay => 2,
            Self::FloorKind => 3,
        }
    }
}

/// The text given as a seed is neither a decimal number that fits in 64
/// bits nor four groups of four hexadecimal digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseSeedError;

impl fmt::Display for ParseSeedError {
    /// Explains both accepted forms, with an example of each, so the player
    /// can correct the value.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(
            "expected a whole number from 0 to 18446744073709551615 (such as 42) \
             or 16 hexadecimal digits in four groups (such as 7F3A-91C2-0B5E-D4A8)",
        )
    }
}

impl std::error::Error for ParseSeedError {}

/// A calendar date in Coordinated Universal Time (UTC), which names a daily
/// dungeon (see [`Seed::daily`]).
///
/// Always a real date of the proleptic Gregorian calendar between the years
/// [`DailyDate::MIN_YEAR`] and [`DailyDate::MAX_YEAR`]: 29 February only in
/// leap years. Written as `2026-11-08` (see the [`fmt::Display`]
/// implementation). Dates compare in calendar order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DailyDate {
    /// The year, from [`DailyDate::MIN_YEAR`] to [`DailyDate::MAX_YEAR`].
    year: u16,
    /// The month, from 1 (January) to 12 (December).
    month: u8,
    /// The day of the month, from 1 to the length of the month.
    day: u8,
}

impl DailyDate {
    /// First year a date may have.
    pub const MIN_YEAR: u16 = 1;

    /// Last year a date may have: four digits, so the written form always
    /// takes 10 characters.
    pub const MAX_YEAR: u16 = 9999;

    /// Returns the date `year`-`month`-`day`, or `None` if it does not
    /// exist: a year outside [`DailyDate::MIN_YEAR`] to
    /// [`DailyDate::MAX_YEAR`], a month outside 1 to 12, or a day outside
    /// the month (such as 31 April, or 29 February of a common year).
    pub const fn new(year: u16, month: u8, day: u8) -> Option<Self> {
        if year < Self::MIN_YEAR || year > Self::MAX_YEAR || month < 1 || month > 12 {
            return None;
        }
        if day < 1 || day > days_in_month(year, month) {
            return None;
        }
        Some(Self { year, month, day })
    }

    /// Returns the year.
    pub const fn year(self) -> u16 {
        self.year
    }

    /// Returns the month, from 1 (January) to 12 (December).
    pub const fn month(self) -> u8 {
        self.month
    }

    /// Returns the day of the month, from 1.
    pub const fn day(self) -> u8 {
        self.day
    }

    /// Packs the date into one number, year, month and day in separate
    /// bits (see [`Seed::daily`]): two dates give the same number only if
    /// they are equal.
    const fn packed(self) -> u64 {
        // `u64::from` cannot be called in a const function; these widenings
        // never lose a bit.
        ((self.year as u64) << 16) | ((self.month as u64) << 8) | self.day as u64
    }
}

impl fmt::Display for DailyDate {
    /// Writes the date in the ISO 8601 form, year first, such as
    /// `2026-11-08`.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:04}-{:02}-{:02}",
            self.year, self.month, self.day
        )
    }
}

/// Returns the number of days of `month` (1 to 12) in `year`: February has
/// 29 days in the leap years of the Gregorian calendar (divisible by 4, but
/// not by 100 unless by 400), 28 otherwise.
const fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

#[cfg(test)]
mod tests {
    use rand::Rng as _;

    use super::*;

    /// Seed of the frozen reference values below.
    const REFERENCE_SEED: Seed = Seed::new(42);

    /// Draws the first `count` values of the stream for `depth` and `purpose`.
    fn first_values(seed: Seed, depth: u32, purpose: Purpose, count: usize) -> Vec<u64> {
        let mut rng = seed.stream(depth, purpose);
        (0..count).map(|_| rng.next_u64()).collect()
    }

    #[test]
    fn the_same_seed_depth_and_purpose_give_the_same_values() {
        for seed in [0, 1, 42, u64::MAX].map(Seed::new) {
            for depth in [0, 1, 7, u32::MAX] {
                for purpose in [Purpose::Generation, Purpose::Gameplay, Purpose::FloorKind] {
                    assert_eq!(
                        first_values(seed, depth, purpose, 64),
                        first_values(seed, depth, purpose, 64),
                        "{seed} depth {depth} {purpose:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn purposes_have_different_streams() {
        for depth in 0..20 {
            assert_ne!(
                first_values(REFERENCE_SEED, depth, Purpose::Generation, 4),
                first_values(REFERENCE_SEED, depth, Purpose::Gameplay, 4),
                "depth {depth}"
            );
            assert_ne!(
                first_values(REFERENCE_SEED, depth, Purpose::Generation, 4),
                first_values(REFERENCE_SEED, depth, Purpose::FloorKind, 4),
                "depth {depth}"
            );
            assert_ne!(
                first_values(REFERENCE_SEED, depth, Purpose::Gameplay, 4),
                first_values(REFERENCE_SEED, depth, Purpose::FloorKind, 4),
                "depth {depth}"
            );
        }
    }

    #[test]
    fn depths_have_different_streams() {
        let mut streams: Vec<Vec<u64>> = (0..100)
            .flat_map(|depth| {
                [Purpose::Generation, Purpose::Gameplay, Purpose::FloorKind]
                    .map(|purpose| first_values(REFERENCE_SEED, depth, purpose, 4))
            })
            .collect();
        let count = streams.len();
        streams.sort_unstable();
        streams.dedup();

        assert_eq!(streams.len(), count, "two (depth, purpose) pairs collide");
    }

    #[test]
    fn neighbouring_seeds_have_different_streams() {
        assert_ne!(
            first_values(Seed::new(1), 1, Purpose::Generation, 4),
            first_values(Seed::new(2), 1, Purpose::Generation, 4),
        );
        assert_ne!(
            first_values(Seed::new(0), 1, Purpose::Generation, 4),
            first_values(Seed::new(0), 2, Purpose::Generation, 4),
        );
    }

    #[test]
    fn drawing_from_one_stream_does_not_move_another() {
        let mut gameplay = REFERENCE_SEED.stream(1, Purpose::Gameplay);
        for _ in 0..1000 {
            gameplay.next_u64();
        }

        assert_eq!(
            first_values(REFERENCE_SEED, 2, Purpose::Generation, 8),
            first_values(REFERENCE_SEED, 2, Purpose::Generation, 8),
        );
    }

    #[test]
    fn reference_values_are_frozen() {
        // These values pin the derivation and the ChaCha8 generator. If this
        // test fails, every existing seed now gives another dungeon: change
        // the derivation only on purpose, and update the values with it.
        assert_eq!(
            first_values(REFERENCE_SEED, 1, Purpose::Generation, 4),
            [
                0xF5BD_1C27_9780_BDB8,
                0x7F71_3B4F_F3ED_C699,
                0x4199_8ACC_6448_915C,
                0x6742_21AA_7D79_035F
            ]
        );
        assert_eq!(
            first_values(REFERENCE_SEED, 1, Purpose::Gameplay, 4),
            [
                0x4656_B3B7_F738_9335,
                0xEFD3_2B4F_A980_9F87,
                0xAB99_4456_468C_CB6A,
                0x36A0_87B3_13B2_4704
            ]
        );
        assert_eq!(
            first_values(REFERENCE_SEED, 2, Purpose::Generation, 4),
            [
                0x23BD_A242_891A_ED01,
                0xB87E_C0B7_A4EE_B8A2,
                0x9542_FE22_21E7_AA6C,
                0x32A4_1683_228A_5EA2
            ]
        );
    }

    #[test]
    fn mix_matches_the_published_splitmix64_outputs() {
        // A SplitMix64 generator started at 0 yields mix(gamma), mix(2 gamma)…
        // The expected values are the reference outputs of SplitMix64 seeded
        // with 0.
        let outputs: Vec<u64> = (1..=3_u64)
            .map(|step| mix(SPLITMIX_GAMMA.wrapping_mul(step)))
            .collect();

        assert_eq!(
            outputs,
            [
                0xE220_A839_7B1D_CDAF,
                0x6E78_9E6A_A1B9_65F4,
                0x06C4_5D18_8009_454F
            ]
        );
    }

    #[test]
    fn seeds_are_written_in_four_hexadecimal_groups() {
        assert_eq!(
            Seed::new(0x7F3A_91C2_0B5E_D4A8).to_string(),
            "7F3A-91C2-0B5E-D4A8"
        );
        assert_eq!(Seed::new(42).to_string(), "0000-0000-0000-002A");
        assert_eq!(Seed::new(u64::MAX).to_string(), "FFFF-FFFF-FFFF-FFFF");
    }

    #[test]
    fn seeds_are_read_as_decimal_or_grouped_hexadecimal() {
        assert_eq!("42".parse(), Ok(Seed::new(42)));
        assert_eq!("0".parse(), Ok(Seed::new(0)));
        assert_eq!("18446744073709551615".parse(), Ok(Seed::new(u64::MAX)));
        assert_eq!(
            "7F3A-91C2-0B5E-D4A8".parse(),
            Ok(Seed::new(0x7F3A_91C2_0B5E_D4A8))
        );
        assert_eq!(
            "7f3a-91c2-0b5e-d4a8".parse(),
            Ok(Seed::new(0x7F3A_91C2_0B5E_D4A8))
        );
    }

    #[test]
    fn a_written_seed_reads_back_as_the_same_seed() {
        for value in [0, 1, 42, 0x7F3A_91C2_0B5E_D4A8, u64::MAX] {
            let seed = Seed::new(value);
            assert_eq!(seed.to_string().parse(), Ok(seed));
        }
    }

    #[test]
    fn anything_else_is_not_a_seed() {
        for text in [
            "",
            "abc",
            "-1",
            "4.2",
            " 42",
            "42 ",
            "18446744073709551616",
            "7F3A-91C2-0B5E",
            "7F3A-91C2-0B5E-D4A8-0000",
            "7F3A91C20B5ED4A8",
            "7F3A-91C2-0B5E-D4A",
            "7F3A-91C2-0B5E-D4A80",
            "7F3A-91C2-0B5E-+4A8",
            "7G3A-91C2-0B5E-D4A8",
            "7F3A--91C2-0B5E",
        ] {
            assert_eq!(text.parse::<Seed>(), Err(ParseSeedError), "{text:?}");
        }
    }

    /// Every date from `first` on, `count` days in a row, in calendar
    /// order.
    fn consecutive_dates(first: DailyDate, count: usize) -> Vec<DailyDate> {
        std::iter::successors(Some(first), |date| next_day(*date))
            .take(count)
            .collect()
    }

    /// The day after `date`, or `None` after 9999-12-31.
    fn next_day(date: DailyDate) -> Option<DailyDate> {
        let (year, month, day) = (date.year(), date.month(), date.day());
        DailyDate::new(year, month, day + 1)
            .or_else(|| DailyDate::new(year, month + 1, 1))
            .or_else(|| DailyDate::new(year + 1, 1, 1))
    }

    /// The date `year`-`month`-`day`, which the test knows to exist.
    fn date(year: u16, month: u8, day: u8) -> DailyDate {
        DailyDate::new(year, month, day).expect("a real date")
    }

    #[test]
    fn the_same_date_gives_the_same_daily_seed() {
        for (year, month, day) in [(2026, 11, 8), (2000, 2, 29), (1970, 1, 1)] {
            assert_eq!(
                Seed::daily(date(year, month, day)),
                Seed::daily(date(year, month, day)),
            );
        }
    }

    #[test]
    fn ten_years_of_dates_give_ten_years_of_different_daily_seeds() {
        let dates = consecutive_dates(date(2026, 1, 1), 3652);
        assert_eq!(dates.last(), Some(&date(2035, 12, 31)), "10 years");

        let mut seeds: Vec<u64> = dates.iter().map(|&d| Seed::daily(d).value()).collect();
        seeds.sort_unstable();
        seeds.dedup();

        assert_eq!(seeds.len(), dates.len(), "two dates share a daily seed");
    }

    #[test]
    fn neighbouring_dates_give_unrelated_daily_seeds() {
        // The mixing spreads one day of difference over the whole seed:
        // about half the 64 bits differ, never only a few.
        for pair in consecutive_dates(date(2026, 1, 1), 400).windows(2) {
            let flipped =
                (Seed::daily(pair[0]).value() ^ Seed::daily(pair[1]).value()).count_ones();
            assert!((12..=52).contains(&flipped), "{pair:?}: {flipped} bits");
        }
    }

    #[test]
    fn daily_seeds_are_frozen() {
        // These values pin the date packing, the tag and the mixing. If this
        // test fails, every player now gets another daily dungeon than the
        // one shared for that date: change the derivation only on purpose.
        assert_eq!(
            Seed::daily(date(2026, 11, 8)),
            Seed::new(0x4A8B_CAEC_21D9_3697)
        );
        assert_eq!(
            Seed::daily(date(2000, 2, 29)),
            Seed::new(0x29B9_A9C5_780A_F19E)
        );
        assert_eq!(
            Seed::daily(date(2035, 12, 31)),
            Seed::new(0xBBE6_0F7F_D840_72B3)
        );
    }

    #[test]
    fn a_date_is_packed_year_month_day_without_overlap() {
        assert_eq!(date(2026, 11, 8).packed(), 0x07EA_0B08);
        assert_eq!(date(9999, 12, 31).packed(), 0x270F_0C1F);
        assert_eq!(
            Seed::daily(date(2026, 11, 8)).value(),
            mix(0x07EA_0B08 ^ DAILY_TAG)
        );
        assert_eq!(DAILY_TAG.to_be_bytes(), *b"SIGILDAY");
    }

    #[test]
    fn only_real_dates_exist() {
        assert!(DailyDate::new(2024, 2, 29).is_some(), "leap year");
        assert!(DailyDate::new(2000, 2, 29).is_some(), "divisible by 400");
        assert!(DailyDate::new(2026, 2, 29).is_none(), "common year");
        assert!(DailyDate::new(2100, 2, 29).is_none(), "divisible by 100");
        assert!(DailyDate::new(2026, 4, 30).is_some());
        assert!(DailyDate::new(2026, 4, 31).is_none());
        assert!(DailyDate::new(2026, 12, 31).is_some());
        for (year, month, day) in [
            (2026, 0, 1),
            (2026, 13, 1),
            (2026, 1, 0),
            (2026, 1, 32),
            (0, 1, 1),
            (10_000, 1, 1),
        ] {
            assert!(
                DailyDate::new(year, month, day).is_none(),
                "{year}-{month}-{day}"
            );
        }
        assert_eq!(
            consecutive_dates(date(2024, 1, 1), 367).last(),
            Some(&date(2025, 1, 1)),
            "a leap year has 366 days"
        );
        assert_eq!(next_day(date(9999, 12, 31)), None);
    }

    #[test]
    fn dates_are_written_year_first_with_padded_fields() {
        assert_eq!(date(2026, 11, 8).to_string(), "2026-11-08");
        assert_eq!(date(1, 1, 1).to_string(), "0001-01-01");
        assert_eq!(date(9999, 12, 31).to_string(), "9999-12-31");
    }

    #[test]
    fn dates_compare_in_calendar_order() {
        let dates = consecutive_dates(date(2023, 12, 25), 800);
        assert!(dates.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn the_parse_error_gives_both_accepted_forms() {
        let message = ParseSeedError.to_string();

        assert!(message.contains("18446744073709551615"), "{message}");
        assert!(message.contains("7F3A-91C2-0B5E-D4A8"), "{message}");
    }
}
