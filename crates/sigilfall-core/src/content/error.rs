//! Errors raised while loading the game's content files.
//!
//! Exposes [`ContentError`], returned by `Content::load` and
//! `Content::parse`, and [`FieldProblem`], what is wrong with one field of
//! one entry. Every message names the file and, when the fault lies in an
//! entry, that entry and the field, so whoever edits the data knows where to
//! look.

use thiserror::Error;

/// Why the content files could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ContentError {
    /// The file is not valid RON, or does not have the expected shape (an
    /// unknown or repeated field, a value of the wrong type, an unknown
    /// family…).
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
        /// outside the entries ("before the first monster").
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
}
