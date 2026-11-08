//! Errors raised while writing or reading a save.
//!
//! Exposes [`SaveError`], returned by `save::to_string` and
//! `save::from_str`, and [`Reference`], the kind of definition a save
//! names. Every message says what is wrong and where to look, so a player
//! can be told why a save was refused and a developer can find the fault in
//! the text.

use std::fmt;

use thiserror::Error;

/// Why a game could not be written as a save, or a save read back.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SaveError {
    /// The save was written in another version of the format, by an older or
    /// newer game: it is refused rather than read wrongly.
    #[error("the save uses format version {found}, but this game reads version {expected} only")]
    UnsupportedVersion {
        /// Version written in the save.
        found: u32,
        /// Version this game reads, `save::FORMAT_VERSION`.
        expected: u32,
    },
    /// The text is not RON, or not shaped like a save: a truncated or
    /// edited file, a field missing, unknown, repeated or of the wrong type.
    #[error("the save is unreadable at line {line}, column {column}: {message}")]
    Malformed {
        /// Line where reading stopped, counted from 1.
        line: usize,
        /// Column where reading stopped, counted from 1.
        column: usize,
        /// What the RON reader expected or found.
        message: String,
    },
    /// The save names a monster, an item or a sigil the content does not
    /// define, such as a save of a game whose content has changed since.
    #[error("the save references {kind} `{id}`, absent from the content")]
    UnknownReference {
        /// Kind of definition named.
        kind: Reference,
        /// Identifier written in the save.
        id: String,
    },
    /// The save is well formed but its values could not be played: an actor
    /// outside the map, no player, two actors on one cell, a key naming
    /// nothing, a value out of its bounds…
    ///
    /// The message names the cause first, in a short sentence, so the cause
    /// is still read when a screen has little room for it.
    #[error("{0} (inconsistent save)")]
    Inconsistent(String),
    /// The RON writer failed to write the game.
    #[error("the game could not be written as a save: {0}")]
    Write(String),
}

impl SaveError {
    /// Turns an error of the RON reader into [`SaveError::Malformed`], with
    /// the line and column where it stopped.
    pub(super) fn malformed(error: ron::error::SpannedError) -> Self {
        Self::Malformed {
            line: error.span.start.line,
            column: error.span.start.col,
            message: error.code.to_string(),
        }
    }
}

/// A kind of definition of the content that a save names by identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reference {
    /// A creature (`content::MonsterDef`).
    Monster,
    /// A kind of item (`content::ItemDef`).
    Item,
    /// A sigil (`content::SigilDef`).
    Sigil,
}

impl fmt::Display for Reference {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Monster => "monster",
            Self::Item => "item",
            Self::Sigil => "sigil",
        })
    }
}
