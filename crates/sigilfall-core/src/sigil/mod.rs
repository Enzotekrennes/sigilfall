//! Sigils: traces drawn on the 3 × 3 grid and how they are recognised.
//!
//! Exposes [`Node`], one point of the grid, numbered like a numeric keypad;
//! [`Trace`], the nodes linked so far, which applies the drawing rules of
//! decision D7 (no node twice, a stroke over an unused middle node links it);
//! the errors [`TraceError`] and [`PatternError`]; [`edit_distance`] between
//! two node sequences; and [`recognize`], which compares a finished trace to
//! the known sigils and gives a [`Recognition`]: success, unstable or fizzle.
//!
//! The sigil definitions themselves (pattern, effect, power, targeting) are
//! read from the content files by the `content` module. This module applies
//! no effect and spends no energy: casting is the game's job.

mod recognition;
mod trace;

pub use recognition::{
    MIN_PATTERN_DISTANCE, Recognition, UNSTABLE_DISTANCE, edit_distance, recognize,
};
pub use trace::{MIN_PATTERN_NODES, Node, PatternError, Trace, TraceError};
