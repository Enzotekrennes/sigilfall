//! The rules of casting that are not about drawing: essence, reach, quality
//! and backlash.
//!
//! Exposes the essence constants ([`MAX_ESSENCE`], [`ESSENCE_COST`],
//! [`ESSENCE_REGEN_TICKS`]), the reach of aimed spells ([`SPELL_RANGE`]),
//! [`CastQuality`], the closed list of [`Backlash`]es with their strength
//! ([`SCORCH_DAMAGE`], [`DAZE_TURNS`]) and [`draw_backlash`]. The time a cast
//! takes is set by the `schedule` module (`STROKE_COST`, `CAST_COST`), and
//! the game applies effects and backlashes to the world.

use rand::RngExt as _;
use rand_chacha::ChaCha8Rng;

use super::Recognition;
use crate::fov::SIGHT_RADIUS;

/// Essence of the player at the start of a run, and the most it can hold.
///
/// Provisional until spells are balanced: with [`ESSENCE_COST`], three casts
/// in a row from a full reserve, enough to open a fight with a spell or two
/// and keep one to heal, but not to fight with spells alone.
pub const MAX_ESSENCE: i32 = 6;

/// Essence spent by every cast, pure, unstable or fizzled alike.
///
/// The same for every sigil: the price of a strong sigil is the length of
/// its pattern, paid in time (`schedule::STROKE_COST`), not in essence. A
/// fizzle costs it too, so tracing at random is never free.
pub const ESSENCE_COST: i32 = 2;

/// Ticks for the player to recover one point of essence while below
/// [`MAX_ESSENCE`].
///
/// At normal speed a tick is one action, so a spell's worth of essence
/// ([`ESSENCE_COST`]) comes back in 24 actions and a full reserve in 72:
/// slow enough that essence runs out in a long fight, fast enough that
/// exploring a few rooms refills it.
pub const ESSENCE_REGEN_TICKS: u64 = 12;

/// Farthest a spell aimed in a direction reaches, in cells.
///
/// The radius of sight: a spell never reaches a creature the player could
/// not see along that line.
pub const SPELL_RANGE: u32 = SIGHT_RADIUS;

/// Hit points a [`Backlash::Scorch`] takes from the caster.
///
/// About one blow of a first-floor creature: felt, never decisive. The
/// scorch also never kills (see [`Backlash::Scorch`]).
pub const SCORCH_DAMAGE: i32 = 2;

/// Turns a [`Backlash::Daze`] makes the caster lose.
///
/// One: the monsters get a free round, which hurts in a fight without
/// turning a slip of the hand into a death sentence.
pub const DAZE_TURNS: u32 = 1;

/// How well a trace matched the sigil it casts, as `Event::SigilCast`
/// reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CastQuality {
    /// The trace was exactly the pattern: the effect applies, nothing else.
    Pure,
    /// The trace was one edit away: the effect applies, then a [`Backlash`].
    Unstable,
    /// The trace matched no known sigil: nothing happens but the cost.
    Fizzled,
}

impl CastQuality {
    /// Returns the quality of a cast that `recognition` describes.
    pub const fn of(recognition: &Recognition<'_>) -> Self {
        match recognition {
            Recognition::Success(_) => Self::Pure,
            Recognition::Unstable(_) => Self::Unstable,
            Recognition::Fizzle => Self::Fizzled,
        }
    }
}

/// What goes wrong after an unstable cast, on top of its effect: a closed
/// list, each drawn with the same chance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Backlash {
    /// The magic burns the caster for [`SCORCH_DAMAGE`] hit points. It never
    /// kills: the caster keeps at least 1 hit point, so a mistake costs a
    /// fight, not the run.
    Scorch,
    /// The caster's essence is emptied, whatever was left after the cast.
    Drain,
    /// The caster loses its next [`DAZE_TURNS`] turns, as if held.
    Daze,
}

impl Backlash {
    /// Every backlash, in the order [`draw_backlash`] numbers them.
    pub const ALL: [Self; 3] = [Self::Scorch, Self::Drain, Self::Daze];
}

/// Draws the backlash of one unstable cast from `rng`, uniformly among
/// [`Backlash::ALL`].
///
/// It takes exactly one draw, so a given gameplay stream (`Seed::stream`
/// with `Purpose::Gameplay`) always gives the same backlashes in the same
/// order.
pub fn draw_backlash(rng: &mut ChaCha8Rng) -> Backlash {
    Backlash::ALL[rng.random_range(0..Backlash::ALL.len())]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::{Purpose, Seed};

    #[test]
    fn every_backlash_is_drawn_and_a_seed_gives_the_same_ones() {
        let draws = |seed| {
            let mut rng = Seed::new(seed).stream(1, Purpose::Gameplay);
            (0..60).map(|_| draw_backlash(&mut rng)).collect::<Vec<_>>()
        };

        let first = draws(7);

        assert_eq!(first, draws(7));
        assert_ne!(first, draws(8));
        for backlash in Backlash::ALL {
            assert!(first.contains(&backlash), "{backlash:?} never drawn");
        }
    }

    #[test]
    fn quality_follows_the_recognition() {
        assert_eq!(CastQuality::of(&Recognition::Fizzle), CastQuality::Fizzled);
        let sigil = crate::content::SigilDef::unchecked("ember", &[7, 5, 3]);
        assert_eq!(
            CastQuality::of(&Recognition::Success(&sigil)),
            CastQuality::Pure
        );
        assert_eq!(
            CastQuality::of(&Recognition::Unstable(&sigil)),
            CastQuality::Unstable
        );
    }
}
