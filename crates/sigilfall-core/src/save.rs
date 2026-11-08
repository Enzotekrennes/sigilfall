//! Saves: a game in progress written as RON text, and read back.
//!
//! Exposes [`to_string`], [`from_str`], [`FORMAT_VERSION`], the version of
//! the format written at the head of every save, and the errors
//! [`SaveError`] and [`Reference`].
//!
//! A save holds the state of a game and nothing of the content: monsters,
//! items and sigils are named by their identifiers (`MonsterDef::id`,
//! `ItemDef::id`, `SigilDef::id`), and loading takes the [`Content`] to find
//! them in. Everything else is kept as it is: the seed, the date of a daily
//! run, the depth, the
//! map with the cells seen and remembered, every actor with its energy and
//! its memory of the player, the turn order, the items on the floor and in
//! the inventory, the engraved glyphs, the grimoire, the counters of time,
//! the status, the run's statistics (`Game::stats`) and the exact position
//! of the gameplay random stream. A game
//! read back therefore plays out exactly as the one saved: the same actions
//! give the same events, with the same actor and item keys.
//!
//! Loading never panics on a faulty text: a save of another format version,
//! text that is not RON or not of the expected shape, a reference to a
//! definition the content lacks and values the game could not run with
//! (an actor outside the map, no player…) are each reported as a
//! [`SaveError`].
//!
//! This module does not read or write files: choosing where saves live and
//! deleting them at death is the binary's job.

mod error;
mod record;
mod slots;

pub use error::{Reference, SaveError};
pub(crate) use record::{
    ActorKindRecord, ActorRecord, DailyRecord, EngravingRecord, GameRecord, ItemRecord, MapRecord,
};
pub(crate) use slots::{SlotKey, capture, rebuild};

use ron::ser::PrettyConfig;
use serde::Deserialize;

use crate::content::Content;
use crate::game::Game;

/// Version of the save format, written first in every save as
/// `format_version`.
///
/// It goes up by one whenever a change of the game makes older saves read
/// differently: a field added, removed or renamed, or a rule whose state is
/// kept differently. [`from_str`] refuses any other version rather than
/// guessing.
///
/// Version 2 (T25) moved the player's turn count into the run's statistics
/// (`stats`), written last. Version 3 (T28) added the date of a daily run
/// (`daily`), after the seed. Version 4 (T33) added the seal of the bottom
/// floor to the map's tiles (`*`) and the won game to the status (`Won`).
pub const FORMAT_VERSION: u32 = 4;

/// Depth of nesting from which the pretty printer writes a value on one
/// line.
///
/// At 3, each field of the game, each actor, item and glyph, and each
/// row of the map gets a line of its own, while the small values inside
/// them (positions, keys, the bytes of the random stream) stay on that line:
/// the save reads like a list, as short as it can be while staying
/// readable.
const PRETTY_DEPTH: usize = 3;

/// The head of a save: only its format version, read before the rest so a
/// save of another version is reported as such whatever else it holds.
#[derive(Deserialize)]
struct Header {
    /// Version of the format the save was written in.
    format_version: u32,
}

/// Writes `game` as a save: RON text starting with
/// `format_version: FORMAT_VERSION`, which [`from_str`] reads back.
///
/// The game may be in progress or over. The same game always gives the same
/// text, with `\n` line endings on every platform, and reading a save then
/// writing it again gives the text read.
///
/// # Errors
///
/// Returns [`SaveError::Write`] if the RON writer fails, which no value of a
/// game is known to cause.
pub fn to_string(game: &Game) -> Result<String, SaveError> {
    // The pretty printer ends lines with "\r\n" on Windows by default; one
    // line ending everywhere makes a save the same text on every platform.
    let config = PrettyConfig::new().depth_limit(PRETTY_DEPTH).new_line("\n");
    ron::ser::to_string_pretty(&game.to_record(), config)
        .map_err(|error| SaveError::Write(error.to_string()))
}

/// Reads the save `text` back into a game, finding the monsters, items and
/// sigils it names in `content`.
///
/// `content` is normally `Content::load()`, the content the game was saved
/// with. A game started with content keeps a copy of it to generate the
/// floors below; a game started without ([`Game::on_floor`],
/// [`Game::in_fixed_room`]) stays without. It is the player's turn in the
/// game returned, unless it is over.
///
/// # Errors
///
/// - [`SaveError::UnsupportedVersion`] if the save was written in another
///   format version than [`FORMAT_VERSION`];
/// - [`SaveError::Malformed`] if the text is not RON or not shaped like a
///   save (a field missing, unknown or of the wrong type), with the line
///   and column where reading stopped: a truncated save ends up here;
/// - [`SaveError::UnknownReference`] if it names a monster, an item or a
///   sigil `content` does not define;
/// - [`SaveError::Inconsistent`] if its values could not be played: an
///   actor or an item outside the map or on a wall, two actors on one cell,
///   no player, a key naming nothing, a value out of its bounds, a bottom
///   floor without its seal or with stairs, a seal or a victory above the
///   bottom floor…
pub fn from_str(text: &str, content: &Content) -> Result<Game, SaveError> {
    let header: Header = ron::from_str(text).map_err(SaveError::malformed)?;
    if header.format_version != FORMAT_VERSION {
        return Err(SaveError::UnsupportedVersion {
            found: header.format_version,
            expected: FORMAT_VERSION,
        });
    }
    let record: GameRecord = ron::from_str(text).map_err(SaveError::malformed)?;
    Game::from_record(record, content)
}
