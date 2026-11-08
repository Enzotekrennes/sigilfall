//! Errors raised while loading the game's content files.
//!
//! Exposes [`ContentError`], returned by `Content::load` and the
//! `Content::parse*` functions, and [`FieldProblem`], what is wrong with one
//! field of one entry. Every message names the file and, when the fault lies in an
//! entry, that entry and the field, so whoever edits the data knows where to
//! look.

use thiserror::Error;

use super::sigil::{Effect, Targeting};
use crate::sigil::PatternError;

/// Why the content files could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ContentError {
    /// The file is not valid RON, or does not have the expected shape (an
    /// unknown or repeated field, a value of the wrong type, an unknown
    /// family or effect…).
    #[error("{file}, line {line}, column {column}, {place}: {message}")]
    Syntax {
        /// Path of the faulty file, relative to the crate.
        file: &'static str,
        /// Line of the fault, counted from 1.
        line: usize,
        /// Column of the fault, counted from 1.
        column: usize,
        /// The entry and field the parser was reading, such as
        /// "monster `shade` (entry 6), field `speed`", or where it was
        /// outside the entries ("before the first monster", "after the last
        /// sigil").
        place: String,
        /// What the RON parser expected or found.
        message: String,
    },
    /// One field of one entry is missing or holds a value the game refuses.
    #[error("{file}: {entry}, field `{field}`: {problem}")]
    InvalidField {
        /// Path of the faulty file, relative to the crate.
        file: &'static str,
        /// The faulty entry: its identifier when it has a usable one, and its
        /// position in the file (such as "monster `acolyte` (entry 3)").
        entry: String,
        /// Name of the faulty field, as written in the file.
        field: &'static str,
        /// What is wrong with the field.
        problem: FieldProblem,
    },
    /// The file defines no entry at all.
    #[error("{file}: the file defines no monster, at least one is needed")]
    NoMonster {
        /// Path of the faulty file, relative to the crate.
        file: &'static str,
    },
    /// No sigil can be found on the first floor, so a new run would have
    /// no glyph to learn from.
    #[error(
        "{file}: no sigil can be found on floor {first_depth}: the lowest field \
         `min_depth` is {min_depth}, on {entry}"
    )]
    NoSigilAtFirstDepth {
        /// Path of the faulty file, relative to the crate.
        file: &'static str,
        /// Number of the first floor.
        first_depth: u32,
        /// The entry with the lowest `min_depth`, the one to lower first.
        entry: String,
        /// That entry's `min_depth`.
        min_depth: u32,
    },
    /// The sigil file defines no entry at all.
    #[error("{file}: the file defines no sigil, at least one is needed")]
    NoSigil {
        /// Path of the faulty file, relative to the crate.
        file: &'static str,
    },
    /// No spawn rule places a monster on this floor, which would leave it
    /// empty.
    #[error(
        "{file}: no rule places a monster on floor {depth}; every floor from \
         {first_depth} to {max_depth} needs one"
    )]
    NoMonsterAtDepth {
        /// Path of the spawn file, relative to the crate.
        file: &'static str,
        /// The floor left without monster.
        depth: u32,
        /// Number of the first floor.
        first_depth: u32,
        /// Deepest floor the content is written for (`content::MAX_DEPTH`).
        max_depth: u32,
    },
    /// No rule of the floor file covers this floor, so the generation would
    /// not know how to lay it out.
    #[error(
        "{file}: no rule gives a kind to floor {depth}; every floor from \
         {first_depth} to {max_depth} needs one"
    )]
    NoFloorKindAtDepth {
        /// Path of the floor file, relative to the crate.
        file: &'static str,
        /// The floor left without kind.
        depth: u32,
        /// Number of the first floor.
        first_depth: u32,
        /// Deepest floor the content is written for (`content::MAX_DEPTH`).
        max_depth: u32,
    },
    /// A monster or an item has no spawn rule, so it would never appear:
    /// most likely a rule was forgotten when the entry was added.
    #[error("{file}: no rule places {entry}, so it would never appear")]
    NoSpawnRule {
        /// Path of the spawn file, relative to the crate.
        file: &'static str,
        /// The entry without rule, such as "monster `acolyte` (entry 3)".
        entry: String,
    },
    /// No monster can appear on the first floor, which would leave it empty.
    #[error(
        "{file}: no monster can appear on floor {first_depth}: the lowest field \
         `min_depth` is {min_depth}, on {entry}"
    )]
    NoMonsterAtFirstDepth {
        /// Path of the faulty file, relative to the crate.
        file: &'static str,
        /// Number of the first floor.
        first_depth: u32,
        /// The entry with the lowest `min_depth`, the one to lower first.
        entry: String,
        /// That entry's `min_depth`.
        min_depth: u32,
    },
}

/// What is wrong with one field of one entry.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FieldProblem {
    /// The field is absent (or written `None`).
    #[error("missing")]
    Missing,
    /// The value lies outside the accepted range.
    #[error("{value} is out of range, expected {min} to {max}")]
    OutOfRange {
        /// The value found in the file.
        value: i64,
        /// Lowest accepted value.
        min: i64,
        /// Highest accepted value.
        max: i64,
    },
    /// The value is below the lowest accepted one; there is no upper bound.
    #[error("{value} is too low, expected at least {min}")]
    TooLow {
        /// The value found in the file.
        value: i64,
        /// Lowest accepted value.
        min: i64,
    },
    /// The value is above the highest accepted one; there is no lower bound
    /// beyond the one the field's type gives.
    #[error("{value} is too high, expected at most {max}")]
    TooHigh {
        /// The value found in the file.
        value: i64,
        /// Highest accepted value.
        max: i64,
    },
    /// The floor is shallower than the `min_depth` of the entry the value
    /// refers to, such as a page found before the glyph of the sigil it
    /// teaches could be, or a spawn rule placing a creature above its own
    /// `min_depth`.
    #[error(
        "floor {value} is shallower than {reference}, whose `min_depth` is \
         {min_depth}: expected at least {min_depth}"
    )]
    ShallowerThanReference {
        /// The floor found in the file.
        value: u32,
        /// The entry referred to, such as "sigil `veil`".
        reference: String,
        /// That entry's `min_depth`, the shallowest floor accepted.
        min_depth: u32,
    },
    /// Two spawn rules of the same creature or item cover a common floor,
    /// so its weight there would be ambiguous.
    #[error("floors {from_depth} to {to_depth} overlap those of {other}")]
    Overlap {
        /// First floor of the faulty rule.
        from_depth: u32,
        /// Last floor of the faulty rule.
        to_depth: u32,
        /// The earlier rule covering a common floor, such as
        /// "spawn `acolyte` (entry 4)".
        other: String,
    },
    /// Another entry already uses this identifier.
    #[error("`{id}` is already the identifier of entry {first_entry}")]
    Duplicate {
        /// The repeated identifier.
        id: String,
        /// Position of the first entry using it, counted from 1.
        first_entry: usize,
    },
    /// The identifier does not have the stable form saves rely on.
    #[error(
        "`{id}` is not a valid identifier, expected lower-case letters, digits \
         and underscores, starting with a letter"
    )]
    BadIdentifier {
        /// The identifier found in the file.
        id: String,
    },
    /// The text is empty or only blank.
    #[error("must not be empty")]
    Empty,
    /// The text is longer than the screens can show whole (see
    /// `content::MAX_NAME_LENGTH`).
    #[error("{length} characters, expected at most {max}")]
    TooLong {
        /// Number of characters found.
        length: usize,
        /// Most characters accepted.
        max: usize,
    },
    /// The list of nodes is not a pattern the player can draw exactly as
    /// written.
    #[error("{0}")]
    BadPattern(PatternError),
    /// The pattern is so close to the pattern of an earlier entry that a
    /// near miss of one could be read as the other (see
    /// `sigil::MIN_PATTERN_DISTANCE`).
    #[error("{distance} edit(s) away from the pattern of {other}, expected at least {min}")]
    TooClose {
        /// The earlier entry, such as "sigil `ember` (entry 1)".
        other: String,
        /// Edit distance between the two patterns (`sigil::edit_distance`).
        distance: usize,
        /// Smallest distance accepted.
        min: usize,
    },
    /// The value names an entry of another file that does not exist, such as
    /// an item teaching a sigil the sigil file does not define.
    #[error("`{id}` is not the identifier of any {noun} of {file}")]
    UnknownReference {
        /// Noun of the kind of entry referred to, such as "sigil".
        noun: &'static str,
        /// The identifier found in the file.
        id: String,
        /// Path of the file that should define it, relative to the crate.
        file: &'static str,
    },
    /// The targeting cannot aim the sigil's effect, such as a blink that is
    /// not aimed in a direction or a heal aimed at an enemy (see
    /// `Effect::targetings`).
    #[error(
        "{targeting:?} cannot aim the effect {effect:?}, expected {}",
        or_list(allowed)
    )]
    UnsuitedTargeting {
        /// The sigil's effect.
        effect: Effect,
        /// The targeting found in the file.
        targeting: Targeting,
        /// Every targeting the effect accepts.
        allowed: &'static [Targeting],
    },
}

/// Writes `targetings` as an English alternative, such as "Direction" or
/// "NearestEnemy or Direction", for error messages.
fn or_list(targetings: &[Targeting]) -> String {
    let names: Vec<String> = targetings.iter().map(|t| format!("{t:?}")).collect();
    names.join(" or ")
}
