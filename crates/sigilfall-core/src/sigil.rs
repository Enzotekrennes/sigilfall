//! Sigils: traces drawn on the 3 × 3 grid, how they are recognised, the
//! grimoire of known sigils and the rules of casting.
//!
//! Exposes [`Node`], one point of the grid, numbered like a numeric keypad;
//! [`Trace`], the nodes linked so far, which applies the drawing rules of
//! decision D7 (no node twice, a stroke over an unused middle node links it);
//! the errors [`TraceError`] and [`PatternError`]; [`edit_distance`] between
//! two node sequences; [`recognize`], which compares a finished trace to
//! the known sigils and gives a [`Recognition`]: success, unstable or fizzle;
//! [`Grimoire`], the sigils the player has learnt; and the casting rules:
//! essence, reach, [`CastQuality`] and [`Backlash`].
//!
//! The sigil definitions themselves (pattern, effect, power, targeting) are
//! read from the content files by the `content` module. This module applies
//! no effect to the world and spends no energy: the game does, when the
//! player submits a cast (`Action::Cast`).

mod casting;
mod grimoire;
mod recognition;
mod trace;

pub use casting::{
    Backlash, CastQuality, DAZE_TURNS, ESSENCE_COST, ESSENCE_REGEN_TICKS, MAX_ESSENCE,
    MAX_STEADY_CASTS, NEAREST_FOE_REACH, SCORCH_DAMAGE, SPELL_RANGE, draw_backlash,
};
pub use grimoire::Grimoire;
pub use recognition::{
    MIN_PATTERN_DISTANCE, Recognition, UNSTABLE_DISTANCE, edit_distance, recognize,
};
pub use trace::{MIN_PATTERN_NODES, Node, PatternError, Trace, TraceError};
