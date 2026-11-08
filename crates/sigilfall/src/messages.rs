//! Every English sentence and label the player reads: the message log, the
//! status line, the side panel, the sigil overlay, the inventory, the end
//! screen, the title screen with its seed entry and confirmation, the
//! options screen and the resize message; and the translation of game
//! events into log messages.
//!
//! Exposes [`describe`], which turns the events of one `Game::submit` into
//! [`Message`]s, the opening messages ([`welcome`], [`welcome_back`],
//! [`save_set_aside`]), why a save was refused ([`save_refusal`]), the save
//! notice [`save_not_deleted`], why automatic exploration stopped
//! ([`exploration_halted`]), the words of the death summary (its
//! [`epitaph`], [`cause_of_death`], the labels of its counts,
//! [`cast_breakdown`] and the [`share_line`]) and of the victory screen
//! ([`victory_line`], [`victory_outcome`]), monster, sigil and item names
//! ([`monster_name`], [`sigil_name`], [`item_name`]), what a sigil or an
//! item does in a few words ([`effect_summary`], [`item_summary`]), the time
//! a cast takes ([`cast_time`]), why the game refused an item
//! ([`item_refusal`]), the notices of the sigil overlay and the inventory,
//! the words of the title and options screens ([`saved_run`],
//! [`abandon_warning`], [`invalid_seed`], [`auto_colors`],
//! [`options_unreadable`], [`options_not_saved`]…), and the fixed labels of
//! every screen and of the key help. The messages about the save and options
//! files quote paths and errors that come from outside the game: what they
//! quote is bounded ([`MAX_REASON_CHARS`], [`MAX_QUOTED_CHARS`],
//! [`ELLIPSIS`]) so that they always fit the rows the screen gives them, and
//! made [`printable`] so that it never acts on the terminal. Keeping the
//! words here, away from the drawing code, is what decision D10 asks: a
//! translation would only touch this file.
//!
//! The game itself never writes text (decision D4): it reports events, and
//! this module chooses what the player reads about them.

use std::collections::HashSet;

use sigilfall_core::actor::ActorId;
use sigilfall_core::combat::Combatant;
use sigilfall_core::content::{
    Content, Effect, ItemDef, ItemEffect, ItemFamily, SigilDef, Targeting,
};
use sigilfall_core::event::Event;
use sigilfall_core::game::{Game, Status};
use sigilfall_core::generation;
use sigilfall_core::item::{INVENTORY_CAPACITY, ItemId, ItemRefusal};
use sigilfall_core::rng::{DailyDate, Seed};
use sigilfall_core::save::SaveError;
use sigilfall_core::schedule::{MOVE_COST, STROKE_COST, cast_cost};
use sigilfall_core::sigil::{CastQuality, Node};
use sigilfall_core::stats::{CastCounts, Stats};

use crate::app::exploring::{Halt, MAX_EXPLORE_STEPS};
use crate::message_log::{Message, Tone};
use crate::theme::ColorSupport;
use crate::ui::log::TEXT_WIDTH;

/// Mark that stands for the part of a text left out: the end of a reason
/// cut at [`MAX_REASON_CHARS`] or of a quote cut at [`MAX_QUOTED_CHARS`],
/// or the start of a path cut to fit a line.
pub const ELLIPSIS: char = '…';

/// Mark shown in place of a control character (escape, bell, carriage
/// return…) of a text that comes from a file or the system, so such a text
/// can never move the cursor, retitle or clear the terminal it is written
/// to.
pub const CONTROL_MARK: char = '?';

/// Most characters of a reason that comes from the system (an error of the
/// file system) or from the options file, in a message about a file; a
/// longer one keeps its first characters and ends with [`ELLIPSIS`].
///
/// Such a reason can be as long as the file it is about, and a message that
/// long either scrolled out of the log at once or hid, on the title screen,
/// what happened to the file. 72 characters keep the usual reasons whole,
/// and keep even a reason of a single word, with the brackets and the full
/// stop around it, on one line of the log ([`TEXT_WIDTH`]): every message
/// about a file then fits the rows it is given, path included.
pub const MAX_REASON_CHARS: usize = 72;

/// Most characters of a piece of a save quoted in the reason it was refused
/// for (an identifier, what the reader found); a longer one keeps its first
/// characters and ends with [`ELLIPSIS`].
///
/// Only the quote is cut, never the sentence around it, so the cause stays
/// whole: 40 characters hold every identifier of the shipped content and the
/// reader's usual words ("Expected colon"), while a corrupt save of 20 kB,
/// which the reader may quote in full, still gives a reason that fits the
/// rows a message about the save is given.
pub const MAX_QUOTED_CHARS: usize = 40;

/// Reason given for a save that is not UTF-8 text.
pub const SAVE_NOT_TEXT: &str = "the save is not text";

/// Most characters of a file path in a message: one line of the log or of
/// a title notice ([`TEXT_WIDTH`]), less the punctuation that follows the
/// path, so the path is never broken across lines. A longer path loses its
/// start (see [`bounded_path`]).
const MAX_PATH_CHARS: usize = TEXT_WIDTH - 1;

/// Title of the side panel's health block.
pub const HP_LABEL: &str = "HP";

/// Mark written before a value that warns of a danger, so the warning reads
/// without colour: health below a quarter of the maximum, essence too low
/// to cast, a node the trace refused.
pub const WARNING_MARK: &str = "!";

/// Label of the player's essence, in the side panel and the sigil overlay.
pub const ESSENCE_LABEL: &str = "Essence";

/// Title of the side panel's list of known sigils.
pub const SIGILS_TITLE: &str = "SIGILS";

/// What the side panel's list of known sigils says while the grimoire is
/// empty.
pub const NO_SIGIL_YET: &str = "none yet";

/// Title of the side panel's list of what the player sees.
pub const IN_VIEW_TITLE: &str = "IN VIEW";

/// Name of the player in the "in view" list, after the `@`.
pub const PLAYER_IN_VIEW: &str = "you";

/// Name of an engraved glyph not read yet, in the "in view" list.
pub const UNREAD_GLYPH_IN_VIEW: &str = "carved glyph";

/// Name of an engraved glyph already read, in the "in view" list.
pub const READ_GLYPH_IN_VIEW: &str = "known glyph";

/// Name of the stairs down in the "in view" list, after their glyph.
pub const STAIRS_IN_VIEW: &str = "stairs down";

/// Name of the seal of the bottom floor in the "in view" list, after its
/// glyph: the word of the log and of the key help ("unseal").
pub const SEAL_IN_VIEW: &str = "seal";

/// Title of the separator above the message log.
pub const LOG_TITLE: &str = "Log";

/// Title of the sigil overlay while the player traces.
pub const TRACE_TITLE: &str = "Trace a sigil";

/// Title of the sigil overlay once an unstable cast has been made.
pub const UNSTABLE_TITLE: &str = "Unstable trace";

/// Title of the sigil overlay once a trace has fizzled.
pub const FAILED_TITLE: &str = "Failed trace";

/// Title of the grimoire, in the sigil overlay.
pub const GRIMOIRE_TITLE: &str = "GRIMOIRE";

/// The lines the grimoire shows while it is empty: how sigils are learnt.
/// `{glyph}` stands for the map glyph of an unread engraving.
pub const EMPTY_GRIMOIRE: [&str; 4] = [
    "No sigil known yet.",
    "Step on a glyph carved",
    "in the floor, {glyph} on the",
    "map, to learn its sigil.",
];

/// Label before the nodes linked so far, in the sigil overlay.
pub const TRACE_LABEL: &str = "Trace";

/// Label before the time the trace takes, in the sigil overlay.
pub const TIME_LABEL: &str = "Time";

/// Label before the pattern a near miss was meant to be, on the verdict of
/// an unstable cast.
pub const EXPECTED_LABEL: &str = "expected";

/// Label before the nodes the player linked, on a verdict.
pub const TRACED_LABEL: &str = "traced";

/// Verdict word of an unstable cast, in capitals so it reads without colour.
pub const UNSTABLE_WORD: &str = "UNSTABLE";

/// Verdict word of a fizzled cast, in capitals so it reads without colour.
pub const FAILED_WORD: &str = "FAILED";

/// How far an unstable trace was from its sigil: decision D7 makes it
/// always one edit.
pub const ONE_NODE_OFF: &str = "one node off";

/// Why a fizzled trace cast nothing, when it was a near miss of a known
/// sigil.
pub const NO_KNOWN_PATTERN: &str = "no known pattern";

/// Why a fizzled trace cast nothing, when its pattern is not in the
/// grimoire at all (`Event::UnknownPattern`).
pub const NOT_IN_GRIMOIRE: &str = "not in your grimoire";

/// The lines under the verdict of a trace whose pattern is not in the
/// grimoire: how sigils are learnt. `{glyph}` stands for the map glyph of
/// an unread engraving. Each fits the 37 columns right of the grid.
pub const LEARN_FROM_GLYPHS: [&str; 3] = [
    "Sigils are learnt from the glyphs",
    "carved in the floor, {glyph} on the",
    "map: step on one to read it.",
];

/// Last line of the verdict of an unstable cast.
pub const UNSTABLE_OUTCOME: &str = "The magic lashes back.";

/// Last line of the verdict of a fizzled cast.
pub const FAILED_OUTCOME: &str = "The spell fizzles.";

/// Notice of the sigil overlay when the player validates an empty trace,
/// which is never cast.
pub const EMPTY_TRACE: &str = "Link at least one node first.";

/// Title of the end screen, letter-spaced as the title of the victory
/// screen is, so it reads as a heading without colour.
pub const DEATH_TITLE: &str = "T H E   D E S C E N T   E N D S";

/// Lines under the death summary's title, one per run, chosen from the
/// seed by [`epitaph`]: each speaks of the trace the player leaves in the
/// dungeon, the theme of the game. At most 60 characters, so a line fits an
/// 80-column screen with room to spare.
pub const EPITAPHS: [&str; 8] = [
    "The walls will keep your trace.",
    "Your last stroke is still drying on the stone.",
    "The ink outlived the hand that traced it.",
    "Somewhere below, a glyph waits for another reader.",
    "The dark learnt your sigils by heart.",
    "Your essence seeps into the old carvings.",
    "The grimoire closes; its margins remember you.",
    "Dust settles over an unfinished pattern.",
];

/// Title of the victory screen, letter-spaced like [`DEATH_TITLE`], which
/// it answers.
pub const VICTORY_TITLE: &str = "T H E   S E A L   B R E A K S";

/// Lines under the victory screen's title, one per run, chosen from the
/// seed by [`victory_line`]: each tells what the broken seal and the
/// player's trace leave behind, in the voice of [`EPITAPHS`]. At most 60
/// characters, like them.
pub const VICTORY_LINES: [&str; 8] = [
    "Your trace was true. The dungeon heard it.",
    "The last sigil was the one that opened the way.",
    "The wax runs, and the dark lets go of you.",
    "Ten floors of stone remember a steady hand.",
    "The carvings fall silent behind you.",
    "The grimoire closes on a finished page.",
    "What the seal held is yours to read now.",
    "Somewhere above, the first glyph still glows.",
];

/// Label inside the frame around the seed on the death summary.
pub const SEED_LABEL: &str = "SEED";

/// Label of the deepest floor reached, in the death summary's counts.
pub const DEPTH_REACHED_LABEL: &str = "Depth reached";

/// Label of the number of turns played, in the death summary's counts.
pub const TURNS_LABEL: &str = "Turns";

/// Label of the number of creatures killed, in the death summary's counts.
pub const KILLS_LABEL: &str = "Creatures slain";

/// Label of the number of glyphs read that taught a sigil, in the death
/// summary's counts.
pub const GLYPHS_LABEL: &str = "Glyphs learnt";

/// Label of the number of sigils cast, in the death summary's counts.
pub const CASTS_LABEL: &str = "Sigils cast";

/// Label of the number of items used, in the death summary's counts.
pub const ITEMS_USED_LABEL: &str = "Items used";

/// Key that leaves the game, as the key help shows it.
pub const QUIT_KEY: &str = "q";

/// What [`QUIT_KEY`] does on the title and end screens, where nothing is
/// left to save.
pub const QUIT_LABEL: &str = "quit";

/// What [`QUIT_KEY`] does on the game screen: the game in progress is
/// saved, then the program ends.
pub const SAVE_AND_QUIT_LABEL: &str = "save & quit";

/// Key that goes back to the title screen from the end screen, as the key
/// help shows it.
pub const TITLE_KEY: &str = "Enter";

/// What [`TITLE_KEY`] does on the end screen.
pub const TITLE_LABEL: &str = "title screen";

/// What the movement keys do, in the key help of the game screen.
pub const MOVE_LABEL: &str = "move";

/// Key that waits, in the key help of the game screen.
pub const WAIT_KEY: &str = ".";

/// What [`WAIT_KEY`] does.
pub const WAIT_LABEL: &str = "wait";

/// Key that opens the sigil overlay, in the key help of the game screen.
pub const TRACE_KEY: &str = "t";

/// What [`TRACE_KEY`] does.
pub const TRACE_KEY_LABEL: &str = "trace";

/// Keys that link a node, in the key help of the sigil overlay.
pub const LINK_KEYS: &str = "1-9";

/// What [`LINK_KEYS`] do.
pub const LINK_LABEL: &str = "link a node";

/// Key that removes the last node, in the key help of the sigil overlay.
pub const UNDO_KEY: &str = "Backspace";

/// What [`UNDO_KEY`] does.
pub const UNDO_LABEL: &str = "undo";

/// Key that validates, in the key help of the sigil overlay.
pub const CAST_KEY: &str = "Enter";

/// What [`CAST_KEY`] does while tracing.
pub const CAST_LABEL: &str = "cast";

/// What [`CAST_KEY`] does on a verdict.
pub const CONTINUE_LABEL: &str = "continue";

/// Key that cancels the trace, or goes back to it from the direction
/// prompt, in the key help of the sigil overlay.
pub const CANCEL_KEY: &str = "Esc";

/// What [`CANCEL_KEY`] does while tracing.
pub const CANCEL_LABEL: &str = "cancel";

/// What [`CANCEL_KEY`] does while a direction is asked.
pub const BACK_LABEL: &str = "back to the trace";

/// What the direction keys do while a direction is asked.
pub const AIM_LABEL: &str = "aim";

/// Key that picks up the item underfoot, in the key help of the game
/// screen; `,` does the same, as in other roguelikes.
pub const PICK_UP_KEY: &str = "g";

/// What [`PICK_UP_KEY`] does: "take" rather than "pick up", so the game
/// screen's help, with "save & quit", fits 80 columns.
pub const PICK_UP_LABEL: &str = "take";

/// Key that opens the inventory, in the key help of the game screen.
pub const INVENTORY_KEY: &str = "i";

/// What [`INVENTORY_KEY`] does: "pack", the word of the log ("Your pack is
/// full"), short enough for the game screen's help to fit 80 columns.
pub const INVENTORY_LABEL: &str = "pack";

/// Key that goes down the stairs, in the key help of the game screen; the
/// alternative `input::DESCEND_ALTERNATIVE_KEY` does the same, but the
/// 80-column help line has no room left to name it.
pub const DESCEND_KEY: &str = ">";

/// What [`DESCEND_KEY`] does.
pub const DESCEND_LABEL: &str = "descend";

/// What [`DESCEND_KEY`] does on the bottom floor, where it breaks the seal
/// instead: one word, shorter than "break seal", so the game screen's help
/// still fits 80 columns.
pub const UNSEAL_LABEL: &str = "unseal";

/// Title of the inventory's frame.
pub const INVENTORY_TITLE: &str = "Inventory";

/// Keys that choose an item after the arrows, in the key help of the
/// inventory: the digits of the slots, `0` being the tenth.
pub const SLOT_KEYS: &str = "1-0";

/// What the arrows and [`SLOT_KEYS`] do in the inventory.
pub const CHOOSE_LABEL: &str = "choose";

/// Keys that use the chosen item, in the key help of the inventory.
pub const USE_KEYS: &str = "Enter/u";

/// What [`USE_KEYS`] do.
pub const USE_LABEL: &str = "use";

/// Key that drops the chosen item, in the key help of the inventory.
pub const DROP_KEY: &str = "d";

/// What [`DROP_KEY`] does.
pub const DROP_LABEL: &str = "drop";

/// What [`CANCEL_KEY`] does in the inventory.
pub const CLOSE_LABEL: &str = "close";

/// Label of the depth on the status line, before its value.
pub const DEPTH_LABEL: &str = "Depth";

/// Label of the turn on the status line, before its value.
pub const TURN_LABEL: &str = "Turn";

/// Label of the seed at the right end of the status line, before it.
pub const STATUS_SEED_LABEL: &str = "Seed";

/// Label of the date at the right end of the status line of a daily run,
/// before it, in place of the seed: "Daily 2026-11-08".
pub const STATUS_DAILY_LABEL: &str = "Daily";

/// The message shown instead of any screen on a terminal smaller than
/// `min_width` × `min_height`: what is needed and the `width` × `height`
/// of now, `times` being the theme's dimension sign.
pub fn enlarge_terminal(
    (min_width, min_height): (u16, u16),
    (width, height): (u16, u16),
    times: char,
) -> String {
    format!(
        "Enlarge the terminal to {min_width} {times} {min_height} (current: {width} {times} {height})."
    )
}

/// Name of the game on the title screen, letter-spaced like the end
/// screen's title.
pub const GAME_TITLE: &str = "S I G I L F A L L";

/// Line under the game's name on the title screen.
pub const TAGLINE: &str = "Trace true. The dungeon listens.";

/// Menu entry that resumes the saved game.
pub const CONTINUE_ITEM: &str = "Continue";

/// Menu entry that starts a new game on a random seed.
pub const NEW_GAME_ITEM: &str = "New game";

/// Menu entry that starts a new game on a seed the player types.
pub const SEED_ITEM: &str = "Seed";

/// Menu entry that starts the daily dungeon of today's UTC date, shown
/// beside it.
pub const DAILY_ITEM: &str = "Daily";

/// Menu entry that opens the options screen.
pub const OPTIONS_ITEM: &str = "Options";

/// Menu entry that leaves the game.
pub const QUIT_ITEM: &str = "Quit";

/// What the title screen says of the saved game, beside "Continue": its
/// depth and the turn it stopped on, such as "depth 3 · turn 412", with
/// the theme's `separator`.
pub fn saved_run(depth: u32, turn: u64, separator: char) -> String {
    format!("depth {depth} {separator} turn {turn}")
}

/// Key that confirms the chosen menu entry, in the key help of the menus.
pub const CONFIRM_KEY: &str = "Enter";

/// What [`CONFIRM_KEY`] does in the menus.
pub const CONFIRM_LABEL: &str = "confirm";

/// The version of the game, as the title screen shows it in its corner:
/// "v" and the version of this binary, such as "v0.1.0".
pub fn version() -> String {
    format!("v{}", env!("CARGO_PKG_VERSION"))
}

/// What the title screen asks while a seed is typed.
pub const SEED_PROMPT: &str = "Type the seed of the dungeon, then press Enter.";

/// Label before the field where the seed is typed.
pub const SEED_FIELD_LABEL: &str = "Seed";

/// The forms a seed is accepted in, under the field.
pub const SEED_FORMS: &str = "A number such as 42, or the shared form 7F3A-91C2-0B5E-D4A8.";

/// What the seed entry says when Enter is pressed on an empty field.
pub const EMPTY_SEED: &str = "Type a seed first.";

/// What the seed entry says when `typed` is not a seed: the text as typed
/// and what to do.
pub fn invalid_seed(typed: &str) -> String {
    format!("\"{typed}\" is not a seed: use one of the forms above.")
}

/// What Enter does in the seed entry.
pub const START_LABEL: &str = "start";

/// What Backspace does in the seed entry.
pub const ERASE_LABEL: &str = "erase";

/// What Escape does on the seed entry and the options screen.
pub const MENU_BACK_LABEL: &str = "back";

/// Question of the confirmation shown before a new game replaces a saved
/// one.
pub const ABANDON_QUESTION: &str = "Start a new game?";

/// The warning under [`ABANDON_QUESTION`]: the game in progress, described
/// by `saved` (see [`saved_run`]), will be abandoned and its save deleted.
/// Two lines, each within 60 columns.
pub fn abandon_warning(saved: &str) -> [String; 2] {
    [
        format!("The game in progress ({saved}) will be abandoned"),
        "and its save deleted for good.".to_owned(),
    ]
}

/// Choice of the confirmation that keeps the saved game; the one chosen
/// at first, so Enter pressed by habit loses nothing.
pub const KEEP_ITEM: &str = "Keep the game in progress";

/// Choice of the confirmation that deletes the saved game and starts the
/// new one.
pub const ABANDON_ITEM: &str = "Abandon it and start anew";

/// What Escape does on the confirmation.
pub const KEEP_LABEL: &str = "keep it";

/// Title of the options screen, letter-spaced like the game's name.
pub const OPTIONS_TITLE: &str = "O P T I O N S";

/// Label of the glyph option.
pub const GLYPHS_OPTION: &str = "Glyphs";

/// Label of the colour option.
pub const COLORS_OPTION: &str = "Colours";

/// Label of the theme option.
pub const THEME_OPTION: &str = "Theme";

/// Last entry of the options screen, back to the title screen.
pub const BACK_ITEM: &str = "Back";

/// Value of the glyph option for Unicode glyphs.
pub const UNICODE_VALUE: &str = "Unicode";

/// Value of the glyph option for ASCII glyphs.
pub const ASCII_VALUE: &str = "ASCII";

/// Value of the colour option for the 16 ANSI colours.
pub const ANSI16_VALUE: &str = "16 colours";

/// Value of the colour option for no colour.
pub const MONOCHROME_VALUE: &str = "Monochrome";

/// Value of the theme option for the dark theme.
pub const DARK_VALUE: &str = "Dark";

/// Value of the theme option for the light theme.
pub const LIGHT_VALUE: &str = "Light";

/// Value of the colour option "Auto", with what the terminal was `detected`
/// to support, so the player knows what Auto gives, such as
/// "Auto · true colour", with the theme's `separator`.
pub fn auto_colors(detected: ColorSupport, separator: char) -> String {
    let detected = match detected {
        ColorSupport::TrueColor => "true colour",
        ColorSupport::Ansi16 => "16 colours",
        ColorSupport::None => "none, NO_COLOR is set",
    };
    format!("Auto {separator} {detected}")
}

/// What each entry of the options screen does, shown under the list for
/// the chosen one: glyphs, colours, theme, back.
pub const OPTION_HELP: [&str; 4] = [
    "Unicode draws stone walls and sigils; ASCII suits any font.",
    "Auto follows the terminal; 16 colours or Monochrome suit plain ones.",
    "Dark ink or light parchment. Light needs true colour.",
    "Options are saved as soon as they change.",
];

/// Label of the sample line of the options screen, which shows a few map
/// glyphs in the current theme.
pub const SAMPLE_LABEL: &str = "Sample";

/// Note of the options screen while `--ascii` is given: the glyph option is
/// kept but not used for this session.
pub const ASCII_FORCED: &str = "--ascii is on: ASCII glyphs for this session.";

/// Note of the options screen when the light theme is chosen on a terminal
/// drawn with 16 colours, where it falls back to monochrome.
pub const LIGHT_IN_MONOCHROME: &str = "Light needs true colour: drawn in monochrome here.";

/// What changes an option on the options screen, in the key help.
pub const CHANGE_LABEL: &str = "change";

/// The notice of the title screen when the options file could not be read
/// back: `reason` (see `persist::LoadedOptions::Unreadable`), and what
/// happens instead.
///
/// A long `reason` is cut (see [`MAX_REASON_CHARS`]).
pub fn options_unreadable(reason: &str) -> String {
    let reason = bounded_reason(reason);
    format!(
        "The options file could not be read ({reason}). The default options \
         are in use; changing one rewrites the file."
    )
}

/// The notice of the options screen when the options could not be written:
/// `error` says which file and what to check.
pub fn options_not_saved(error: &str) -> String {
    format!("{error}. The options apply until you quit.")
}

/// Why the save could not be read back, as the player reads it: the error
/// of the save reader, `error`, cause first, with only what it quotes from
/// the file (an identifier, the words of the reader) cut at
/// [`MAX_QUOTED_CHARS`], and every control character shown as
/// [`CONTROL_MARK`].
///
/// The sentence around a quote is the game's own and always shown whole, such
/// as "floor 10, the bottom floor, has 2 seals (inconsistent save)" or "the
/// save references monster `dragon`, absent from the content".
pub fn save_refusal(error: &SaveError) -> String {
    match error {
        SaveError::Malformed {
            line,
            column,
            message,
        } => format!(
            "the save is unreadable at line {line}, column {column}: {}",
            quoted(message)
        ),
        SaveError::UnknownReference { kind, id } => {
            format!(
                "the save references {kind} `{}`, absent from the content",
                quoted(id)
            )
        }
        SaveError::UnsupportedVersion { .. } | SaveError::Inconsistent(_) | SaveError::Write(_) => {
            printable(&error.to_string())
        }
    }
}

/// Reason given for a save larger than `limit` bytes, which is not read.
pub fn save_too_large(limit: u64) -> String {
    format!("the save is larger than {}", file_size(limit))
}

/// `bytes` as a reason names a size: in whole mebibytes, such as "4 MiB",
/// for the limits of the files the game reads.
pub fn file_size(bytes: u64) -> String {
    /// Bytes in a mebibyte.
    const MEBIBYTE: u64 = 1024 * 1024;
    format!("{} MiB", bytes / MEBIBYTE)
}

/// The notice of the title screen when the save could not be read back at
/// launch: `reason` (a sentence of [`save_refusal`], through
/// `persist::Loaded::SetAside`), and `moved_to`, where the unreadable save
/// was kept.
///
/// The reason is shown whole, its quotes already cut, and the start of a
/// path longer than a line is cut (see [`bounded_path`]): the notice takes 4
/// of the 5 rows the title screen gives its notices, never more.
pub fn save_set_aside_notice(reason: &str, moved_to: &str) -> String {
    let (reason, moved_to) = (printable(reason), bounded_path(moved_to));
    format!("Your saved game could not be read: {reason}. It was kept as {moved_to}.")
}

/// The notice of the title screen when the save at `path` could not be
/// deleted to start a new game, for the reason `cause` (the system's
/// error). The new game is not started, so the save is never overwritten
/// without the player knowing; the outcome comes first, so it is read
/// even when a long path is cut.
///
/// A long `cause` and the start of a long `path` are cut (see
/// [`MAX_REASON_CHARS`] and [`bounded_path`]), so the notice fits the 5 rows
/// the title screen gives its notices.
pub fn save_not_deleted_notice(path: &str, cause: &str) -> String {
    let (path, cause) = (bounded_path(path), bounded_reason(cause));
    format!(
        "No new game was started: the save {path} could not be deleted ({cause}). \
         Check that it is a file you can delete."
    )
}

/// The notice of the title screen when the save of a finished game (the
/// player died, or broke the seal), at `path`, could still not be deleted
/// on the way back to the title, for the reason `cause`.
///
/// A long `cause` and the start of a long `path` are cut, as in
/// [`save_not_deleted_notice`].
pub fn finished_save_not_deleted_notice(path: &str, cause: &str) -> String {
    let (path, cause) = (bounded_path(path), bounded_reason(cause));
    format!(
        "The save {path} of the finished game could not be deleted ({cause}). \
         Delete it by hand so it is not resumed."
    )
}

/// The lines the inventory shows while the player carries nothing: that
/// the pack is empty and how to pick an item up. `{glyph}` stands for the
/// map glyph of a vial, the most common item.
pub fn empty_inventory() -> [String; 3] {
    [
        "You carry nothing yet.".to_owned(),
        "Stand on an item, {glyph} on the map, and press".to_owned(),
        format!("{PICK_UP_KEY} to pick it up."),
    ]
}

/// The first message of every run, so the log is never empty and tells
/// where the player stands.
pub fn welcome() -> Message {
    Message::new(Tone::Info, "You enter the dungeon.")
}

/// The first message of a run resumed from its save, in place of
/// [`welcome`], so the player knows the game went on where it stopped.
pub fn welcome_back() -> Message {
    Message::new(Tone::Info, "Welcome back. The dungeon is as you left it.")
}

/// The message that opens a new run when the save could not be read back:
/// `reason` is why (a sentence of [`save_refusal`], through
/// `persist::Loaded::SetAside`), and `moved_to` the path the save was
/// renamed to, where it can still be looked at.
///
/// The path comes last and the start of a path longer than a line is cut
/// (see [`bounded_path`]); the reason is shown whole, its quotes already
/// cut: the message takes at most 4 lines, so it shows whole above the
/// welcome line in the 5 rows of the log of an 80 × 24 screen, with the
/// name of the file kept aside.
pub fn save_set_aside(reason: &str, moved_to: &str) -> Message {
    let (reason, moved_to) = (printable(reason), bounded_path(moved_to));
    Message::new(
        Tone::Danger,
        format!(
            "Your saved game could not be read: {reason}. A new game begins; \
             the old save was kept as {moved_to}."
        ),
    )
}

/// The message logged when the save of a finished game (death or victory)
/// could not be deleted: deleting it is tried again when the player quits,
/// and a failure then is reported on leaving.
pub fn save_not_deleted() -> Message {
    Message::new(
        Tone::Danger,
        "The save of this character could not be deleted; it will be tried again on quitting.",
    )
}

/// The message logged when automatic exploration stops, or does not start,
/// for `halt`, naming monsters and items from `content`; `None` when the
/// log already says why ([`Halt::Explored`]: the line of
/// [`Event::NothingToExplore`] from [`describe`]).
///
/// - a monster in view when the key is pressed: "You cannot explore with
///   the ossuary rat in view." ([`Tone::Info`]: nothing happened);
/// - a monster coming into view: "An ossuary rat comes into view."
///   ([`Tone::Danger`]);
/// - an item coming into view: "You spot an Essence Phial." ([`Tone::Info`]);
/// - an unread glyph coming into view: "You spot a carved glyph you have
///   not read." ([`Tone::Sigil`]);
/// - hit points lost: "You are hurt and stop exploring." ([`Tone::Danger`]);
/// - the way held: "Something stands in your way; you stop exploring.";
/// - the step limit: "You stop exploring after 200 steps."
///   ([`MAX_EXPLORE_STEPS`]);
/// - a key pressed: "You stop exploring." (all three [`Tone::Info`]).
pub fn exploration_halted(halt: &Halt, content: &Content) -> Option<Message> {
    let (tone, text) = match halt {
        Halt::Explored => return None,
        Halt::FoeInView(definition) => (
            Tone::Info,
            format!(
                "You cannot explore with the {} in view.",
                monster_name(content, definition)
            ),
        ),
        Halt::FoeAppeared(definition) => {
            let name = monster_name(content, definition);
            let article = if indefinite_article(name) == "an" {
                "An"
            } else {
                "A"
            };
            (Tone::Danger, format!("{article} {name} comes into view."))
        }
        Halt::ItemAppeared(definition) => {
            let name = item_name(content, definition);
            let article = indefinite_article(name);
            (Tone::Info, format!("You spot {article} {name}."))
        }
        Halt::GlyphAppeared => (
            Tone::Sigil,
            "You spot a carved glyph you have not read.".to_owned(),
        ),
        Halt::Hurt => (Tone::Danger, "You are hurt and stop exploring.".to_owned()),
        Halt::Blocked => (
            Tone::Info,
            "Something stands in your way; you stop exploring.".to_owned(),
        ),
        Halt::StepLimit => (
            Tone::Info,
            format!("You stop exploring after {MAX_EXPLORE_STEPS} steps."),
        ),
        Halt::Interrupted => (Tone::Info, "You stop exploring.".to_owned()),
    };
    Some(Message::new(tone, text))
}

/// `reason` as a message about a file shows it: [`printable`], then whole
/// when it has at most [`MAX_REASON_CHARS`] characters, otherwise cut as
/// [`bounded`] cuts.
fn bounded_reason(reason: &str) -> String {
    bounded(&printable(reason), MAX_REASON_CHARS)
}

/// `text`, a piece of a file quoted in a reason: [`printable`], then whole
/// when it has at most [`MAX_QUOTED_CHARS`] characters, otherwise cut as
/// [`bounded`] cuts.
fn quoted(text: &str) -> String {
    bounded(&printable(text), MAX_QUOTED_CHARS)
}

/// `text` whole when it has at most `limit` characters, otherwise its first
/// characters, without the spaces they end with, followed by [`ELLIPSIS`],
/// `limit` characters at most in all.
fn bounded(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_owned();
    }
    let kept: String = text.chars().take(limit - 1).collect();
    format!("{}{ELLIPSIS}", kept.trim_end())
}

/// `text` with every control character (`char::is_control`: escape, bell,
/// line breaks, tabulations…) replaced by [`CONTROL_MARK`], so a text read
/// from a file or given by the system can be shown on a screen or written
/// to the error output without acting on the terminal.
pub fn printable(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() {
                CONTROL_MARK
            } else {
                character
            }
        })
        .collect()
}

/// `path` as a message about a file shows it, [`printable`]: whole when it
/// fits on a line with the punctuation after it ([`MAX_PATH_CHARS`]),
/// otherwise [`ELLIPSIS`] followed by its last characters,
/// [`MAX_PATH_CHARS`] in all, so the folder it is in and the name of the
/// file stay readable.
fn bounded_path(path: &str) -> String {
    let path = &printable(path);
    let length = path.chars().count();
    if length <= MAX_PATH_CHARS {
        return path.to_owned();
    }
    let kept: String = path.chars().skip(length - (MAX_PATH_CHARS - 1)).collect();
    format!("{ELLIPSIS}{kept}")
}

/// Turns `events`, the result of one `Game::submit` on `game`, into the
/// messages the player reads, in the same order.
///
/// `game` is the game after the call, so it still knows every actor that
/// survived; one that died during the call is named from its
/// [`Event::Died`], which the events always carry. Monster and sigil names
/// come from `content`.
///
/// What is written:
/// - a blow: who struck whom and the damage, "You hit the shade (3)."
///   ([`Tone::Info`]) or "The shade hits you (2)." ([`Tone::Danger`]);
/// - a killing blow: the same sentence with "kill", which names the victim,
///   "You kill the shade (4)." ([`Tone::Success`]) or "The shade kills you
///   (5)." ([`Tone::Danger`]); its [`Event::Died`] adds nothing more;
/// - a death without a killing blow in the same events: "The shade dies."
///   or "You die.";
/// - a cast: one [`Tone::Sigil`] line naming the sigil, the quality of the
///   trace and what the effect did, "Ember, pure trace: the shade is seared
///   (6)."; a spell that kills says "destroyed" and its [`Event::Died`] adds
///   nothing more;
/// - a fizzle near a known sigil: "The trace matches no sigil you know and
///   fizzles." ([`Tone::Sigil`]); a fizzle whose pattern is not in the
///   grimoire ([`Event::UnknownPattern`]) says so instead, and that sigils
///   are learnt by reading the glyphs carved in the floor;
/// - a backlash, on its own [`Tone::Danger`] line after the cast, with what
///   it cost: "The backlash scorches you (2).", "The backlash drains your
///   essence (4).", "The backlash dazes you: you lose 1 turn."; a backlash
///   that took nothing is told as such, never as a loss of 0: a scorch
///   spared at the last hit point, "The backlash scorches you, but you hold
///   on." (still [`Tone::Danger`]: one hit point is left), and a drain with
///   no essence left, "The backlash finds no essence to drain."
///   ([`Tone::Info`]: nothing was at stake). The drain is still announced,
///   so every unstable cast logs which backlash it drew;
/// - a glyph learnt: "You decipher a carved glyph: Recall joins your
///   grimoire." ([`Tone::Success`]);
/// - a refused cast, which took no time: "Not enough essence to cast: 1 of
///   2." or "Passage needs a direction." ([`Tone::Info`]);
/// - a cast an item steadied: the cast's line says so, with the steadied
///   casts left, "Ember, steadied trace (1 left): the shade is seared (6).";
/// - an item picked up: "You pick up the Healing Draught." ([`Tone::Success`]);
///   one dropped: "You drop the Healing Draught." ([`Tone::Info`]);
/// - an item used and its effect, on one [`Tone::Success`] line with the
///   verb of its family (drink a vial, read a page, use a catalyst) and,
///   for a restoration, the value reached out of its maximum, so a capped
///   heal reads as such: "You drink the Healing Draught and recover 8 HP
///   (30/30).", "You drink the Healing Draught, but you were not hurt
///   (30/30).", "You drink the Essence Phial and recover 4 essence (6/6).",
///   "You read the Page of the Veil: Veil joins your grimoire.", "You use
///   the Steadying Ink: your next 2 unstable casts will be cast pure.";
/// - an action on an item that the game refused, which took no time
///   ([`Tone::Info`]): "There is nothing here to pick up.", "Your pack is
///   full (10/10): the Healing Draught stays on the floor.", and the
///   sentences of [`item_refusal`];
/// - a descent: "You descend to depth 2." ([`Tone::Info`]: the status line
///   and the new floor already show the change, and the line marks in the
///   history where the floor above ends); a descent to the bottom floor
///   (`generation::is_bottom_floor` of the new depth) also tells it is the
///   last one and why: "You descend to depth 10, the last. The seal lies
///   here." ([`Tone::Info`] too); a descent off the stairs, which
///   took no time: "There are no stairs here." ([`Tone::Info`]), or on the
///   bottom floor, which has a seal instead (`generation::is_bottom_floor`
///   of the game's depth): "No stairs lead deeper: stand on the seal to
///   break it." ([`Tone::Info`]);
/// - the seal broken, which wins the run: "You break the seal. The dungeon
///   falls silent." ([`Tone::Success`]); the victory screen follows at
///   once, and the line stays in the log's history;
/// - a step of automatic exploration refused because nothing is left to
///   explore, which took no time: "There is nothing left to explore within
///   reach." ([`Tone::Info`]); a step taken is a move, and says nothing;
/// - nothing for moves and waits, which the map already shows, nor for a
///   move refused by a wall: no time passes and the player sees they did not
///   move, while a line per bump would push the blows out of the five
///   visible lines; nothing either for a turn lost while held, which the
///   daze line has already announced.
pub fn describe(events: &[Event], game: &Game, content: &Content) -> Vec<Message> {
    let mut killed = HashSet::new();
    let mut messages = Vec::new();
    // Opening of the line of the cast in progress, "Ember, pure trace",
    // written once the event of its effect follows.
    let mut cast_lead: Option<String> = None;
    // Opening of the line of the item being used, "You drink the Healing
    // Draught", likewise completed by the event of its effect.
    let mut use_lead: Option<String> = None;
    for event in events {
        if !is_spell_effect(event, game)
            && !matches!(event, Event::CastSteadied { .. })
            && let Some(lead) = cast_lead.take()
        {
            messages.push(Message::new(Tone::Sigil, format!("{lead}.")));
        }
        if !is_item_effect(event)
            && let Some(lead) = use_lead.take()
        {
            messages.push(Message::new(Tone::Success, format!("{lead}.")));
        }
        let message = match event {
            Event::Attacked {
                attacker,
                target,
                damage,
                hp_left,
            } => {
                let lethal = *hp_left <= 0;
                if lethal {
                    killed.insert(*target);
                }
                let attacker = combatant(*attacker, events, game);
                let target = combatant(*target, events, game);
                attacker
                    .zip(target)
                    .map(|(attacker, target)| blow(&attacker, &target, *damage, lethal, content))
            }
            Event::Died { actor, who, .. } if !killed.contains(actor) => Some(death(who, content)),
            Event::SigilCast {
                sigil: Some(id),
                quality,
            } => {
                cast_lead = Some(format!(
                    "{}, {}",
                    sigil_name(content, id),
                    quality_words(*quality)
                ));
                None
            }
            // An unknown pattern has a line of its own, which says why.
            Event::SigilCast { sigil: None, .. } if events.contains(&Event::UnknownPattern) => None,
            Event::SigilCast { sigil: None, .. } => Some(Message::new(
                Tone::Sigil,
                "The trace matches no sigil you know and fizzles.",
            )),
            Event::UnknownPattern => Some(Message::new(
                Tone::Sigil,
                "That pattern is not in your grimoire, and the trace fizzles. \
                 Sigils are learnt by reading the glyphs carved in the floor.",
            )),
            Event::Wounded {
                actor,
                damage,
                hp_left,
            } => {
                if *actor == game.player() {
                    // The game spares the last hit point: a scorch at 1 HP
                    // takes nothing, which "(0)" would misreport.
                    let text = if *damage > 0 {
                        format!("The backlash scorches you ({damage}).")
                    } else {
                        "The backlash scorches you, but you hold on.".to_owned()
                    };
                    Some(Message::new(Tone::Danger, text))
                } else {
                    let lethal = *hp_left <= 0;
                    if lethal {
                        killed.insert(*actor);
                    }
                    let verb = if lethal { "destroyed" } else { "seared" };
                    combatant(*actor, events, game).map(|target| {
                        let effect = format!("{} is {verb} ({damage})", object(&target, content));
                        spell_line(cast_lead.take(), &effect)
                    })
                }
            }
            Event::Healed { actor, amount, hp } if use_lead.is_some() => {
                let max = game.actor(*actor).map_or(*hp, |actor| actor.max_hp());
                let lead = use_lead.take().unwrap_or_default();
                Some(Message::new(
                    Tone::Success,
                    restored_line(&lead, *amount, Restored::Health, *hp, max),
                ))
            }
            Event::Healed { amount, .. } => {
                let effect = if *amount > 0 {
                    format!("you mend ({amount})")
                } else {
                    "you were not hurt".to_owned()
                };
                Some(spell_line(cast_lead.take(), &effect))
            }
            Event::Held { actor, turns } if *actor == game.player() => Some(Message::new(
                Tone::Danger,
                format!("The backlash dazes you: you lose {}.", turn_count(*turns)),
            )),
            Event::Held { actor, turns } => combatant(*actor, events, game).map(|target| {
                let effect = format!(
                    "{} is held ({})",
                    object(&target, content),
                    turn_count(*turns)
                );
                spell_line(cast_lead.take(), &effect)
            }),
            Event::Blinked { from, to, .. } => {
                let cells = from.x.abs_diff(to.x).max(from.y.abs_diff(to.y));
                let effect = match cells {
                    0 => "the way is blocked".to_owned(),
                    1 => "you blink 1 cell".to_owned(),
                    cells => format!("you blink {cells} cells"),
                };
                Some(spell_line(cast_lead.take(), &effect))
            }
            Event::SpellMissed { .. } => Some(spell_line(cast_lead.take(), "no foe in reach")),
            Event::EssenceDrained { lost, .. } if *lost > 0 => Some(Message::new(
                Tone::Danger,
                format!("The backlash drains your essence ({lost})."),
            )),
            Event::EssenceDrained { .. } => Some(Message::new(
                Tone::Info,
                "The backlash finds no essence to drain.",
            )),
            Event::NotEnoughEssence { essence, cost } => Some(Message::new(
                Tone::Info,
                format!("Not enough essence to cast: {essence} of {cost}."),
            )),
            Event::DirectionMissing { sigil } => Some(Message::new(
                Tone::Info,
                format!("{} needs a direction.", sigil_name(content, sigil)),
            )),
            Event::GlyphLearned { sigil, .. } => Some(Message::new(
                Tone::Success,
                format!(
                    "You decipher a carved glyph: {} joins your grimoire.",
                    sigil_name(content, sigil)
                ),
            )),
            Event::CastSteadied { casts_left, .. } => {
                if let Some(lead) = cast_lead.as_mut() {
                    // The game reports a steadied cast as pure: the line
                    // says what made it so instead.
                    let pure = quality_words(CastQuality::Pure);
                    *lead = lead.replace(pure, &format!("steadied trace ({casts_left} left)"));
                }
                None
            }
            Event::PickedUp { definition, .. } => Some(Message::new(
                Tone::Success,
                format!("You pick up the {}.", item_name(content, definition)),
            )),
            Event::InventoryFull { definition, .. } => Some(Message::new(
                Tone::Info,
                format!(
                    "Your pack is full ({}): the {} stays on the floor.",
                    capacity(INVENTORY_CAPACITY),
                    item_name(content, definition)
                ),
            )),
            Event::NothingToPickUp { .. } => Some(Message::new(
                Tone::Info,
                "There is nothing here to pick up.",
            )),
            Event::Used { definition, .. } => {
                let verb = content
                    .item(definition)
                    .map_or("use", |item| use_verb(item.family()));
                use_lead = Some(format!("You {verb} the {}", item_name(content, definition)));
                None
            }
            Event::EssenceRestored {
                actor,
                amount,
                essence,
            } => {
                let max = game
                    .actor(*actor)
                    .map_or(*essence, |actor| actor.max_essence());
                let lead = use_lead.take().unwrap_or_else(|| "You rest".to_owned());
                Some(Message::new(
                    Tone::Success,
                    restored_line(&lead, *amount, Restored::Essence, *essence, max),
                ))
            }
            Event::SigilLearned { sigil } => {
                let lead = use_lead.take().unwrap_or_else(|| "You learn".to_owned());
                Some(Message::new(
                    Tone::Success,
                    format!(
                        "{lead}: {} joins your grimoire.",
                        sigil_name(content, sigil)
                    ),
                ))
            }
            Event::Steadied { casts } => {
                let lead = use_lead.take().unwrap_or_else(|| "You steady".to_owned());
                let next = match casts {
                    1 => "your next unstable cast".to_owned(),
                    casts => format!("your next {casts} unstable casts"),
                };
                Some(Message::new(
                    Tone::Success,
                    format!("{lead}: {next} will be cast pure."),
                ))
            }
            Event::Dropped { definition, .. } => Some(Message::new(
                Tone::Info,
                format!("You drop the {}.", item_name(content, definition)),
            )),
            Event::ItemRefused { item, reason } => {
                Some(Message::new(Tone::Info, item_refusal(game, *item, *reason)))
            }
            Event::Descended { depth } if generation::is_bottom_floor(*depth) => {
                Some(Message::new(
                    Tone::Info,
                    format!("You descend to depth {depth}, the last. The seal lies here."),
                ))
            }
            Event::Descended { depth } => Some(Message::new(
                Tone::Info,
                format!("You descend to depth {depth}."),
            )),
            Event::NoStairsHere { .. } if generation::is_bottom_floor(game.depth()) => {
                Some(Message::new(
                    Tone::Info,
                    "No stairs lead deeper: stand on the seal to break it.",
                ))
            }
            Event::NoStairsHere { .. } => {
                Some(Message::new(Tone::Info, "There are no stairs here."))
            }
            Event::SealBroken { .. } => Some(Message::new(
                Tone::Success,
                "You break the seal. The dungeon falls silent.",
            )),
            Event::NothingToExplore { .. } => Some(Message::new(
                Tone::Info,
                "There is nothing left to explore within reach.",
            )),
            Event::Died { .. }
            | Event::Moved { .. }
            | Event::Waited { .. }
            | Event::MoveBlocked { .. }
            | Event::TurnLost { .. }
            | Event::Backlash { .. } => None,
        };
        messages.extend(message);
    }
    if let Some(lead) = cast_lead {
        messages.push(Message::new(Tone::Sigil, format!("{lead}.")));
    }
    if let Some(lead) = use_lead {
        messages.push(Message::new(Tone::Success, format!("{lead}.")));
    }
    messages
}

/// Returns why the game refused to use or drop the item `item` of `game`
/// for `reason`, as the log and the inventory say it: "You do not carry
/// that item.", "You already know Veil: the Page of the Veil has nothing
/// to teach.", "Something already lies here: drop the Healing Draught
/// elsewhere."
///
/// The item is named from `game`, which keeps every item not used up; one
/// it no longer knows is called "that item".
pub fn item_refusal(game: &Game, item: ItemId, reason: ItemRefusal) -> String {
    let definition = game.item(item).map(|item| item.definition());
    let name = definition.map_or_else(
        || "that item".to_owned(),
        |definition| format!("the {}", definition.name()),
    );
    match reason {
        ItemRefusal::NotCarried => "You do not carry that item.".to_owned(),
        ItemRefusal::NothingToLearn => {
            let sigil = definition.and_then(|definition| match definition.effect() {
                ItemEffect::Teach(sigil) => Some(sigil.name()),
                _ => None,
            });
            format!(
                "You already know {}: {name} has nothing to teach.",
                sigil.unwrap_or("its sigil")
            )
        }
        ItemRefusal::CellTaken => format!("Something already lies here: drop {name} elsewhere."),
    }
}

/// Returns the name shown to the player for the item definition `id` of
/// `content`, such as "Healing Draught"; an identifier missing from
/// `content` is shown as is, as [`monster_name`] does.
pub fn item_name<'name>(content: &'name Content, id: &'name str) -> &'name str {
    content.item(id).map_or(id, ItemDef::name)
}

/// Returns what `item` does in a few words, for the inventory: "heal 10
/// HP", "restore 4 essence", "teach Veil", "steady 2 casts".
///
/// A page whose sigil the player already knows (`known`) says so after
/// `separator`, "teach Veil · known", since the game would refuse to use
/// it.
pub fn item_summary(item: &ItemDef, known: bool, separator: char) -> String {
    match item.effect() {
        ItemEffect::Heal(amount) => format!("heal {amount} HP"),
        ItemEffect::RestoreEssence(amount) => format!("restore {amount} essence"),
        ItemEffect::Teach(sigil) if known => format!("teach {} {separator} known", sigil.name()),
        ItemEffect::Teach(sigil) => format!("teach {}", sigil.name()),
        ItemEffect::Steady(1) => "steady 1 cast".to_owned(),
        ItemEffect::Steady(casts) => format!("steady {casts} casts"),
    }
}

/// Returns how full the inventory is with `carried` items, out of
/// `item::INVENTORY_CAPACITY`: "3/10".
pub fn capacity(carried: usize) -> String {
    format!("{carried}/{INVENTORY_CAPACITY}")
}

/// Returns the sentence of the death summary that says who killed the
/// player, where and when: "Killed by an acolyte at depth 4, on turn 1283."
///
/// `depth` is the floor of the death and `turn` the player's turn it
/// happened on, counted from 1 as on the status line. A death by the
/// player's own hand (`Combatant::Player`, such as its own spell, which no
/// rule causes today) says so instead of naming a monster.
pub fn cause_of_death(killed_by: &Combatant, depth: u32, turn: u64, content: &Content) -> String {
    match killed_by {
        Combatant::Player => {
            format!("Undone by your own magic at depth {depth}, on turn {turn}.")
        }
        Combatant::Monster { definition } => {
            let name = monster_name(content, definition);
            format!(
                "Killed by {} {name} at depth {depth}, on turn {turn}.",
                indefinite_article(name)
            )
        }
    }
}

/// Returns the line under the death summary's title for the run of `seed`,
/// one of [`EPITAPHS`]: the same seed always gets the same line, so a
/// replayed run ends the same way.
pub fn epitaph(seed: Seed) -> &'static str {
    line_of_seed(seed, &EPITAPHS)
}

/// Returns the line under the victory screen's title for the run of
/// `seed`, one of [`VICTORY_LINES`]: the same seed always gets the same
/// line, as with [`epitaph`].
pub fn victory_line(seed: Seed) -> &'static str {
    line_of_seed(seed, &VICTORY_LINES)
}

/// Returns the line of `lines` that `seed` picks, always the same for the
/// same seed; an empty `lines` gives an empty line.
fn line_of_seed(seed: Seed, lines: &[&'static str]) -> &'static str {
    let value = seed.value();
    // Folding the high half in spreads the seeds typed by hand (small
    // numbers) and the random ones alike over the whole list.
    let mixed = value ^ (value >> 32);
    let count = lines.len() as u64;
    if count == 0 {
        return "";
    }
    let index = usize::try_from(mixed % count).unwrap_or_default();
    lines[index]
}

/// Returns the sentence of the victory screen that says where and when the
/// player broke the seal, in the place of the cause of death: "You broke
/// the seal at depth 10, on turn 1283."
///
/// `depth` is the floor of the seal and `turn` the player's turn it was
/// broken on, counted as in [`cause_of_death`].
pub fn victory_outcome(depth: u32, turn: u64) -> String {
    format!("You broke the seal at depth {depth}, on turn {turn}.")
}

/// Returns the sigils cast by quality, as the death summary details them:
/// "pure 22 · unstable 6 · fizzled 3", joined by `separator`.
pub fn cast_breakdown(casts: CastCounts, separator: char) -> String {
    format!(
        "pure {} {separator} unstable {} {separator} fizzled {}",
        casts.pure, casts.unstable, casts.fizzled
    )
}

/// Returns the command that replays `seed`, as the end screen suggests it:
/// "Replay this seed: sigilfall --seed 7F3A-91C2-0B5E-D4A8".
pub fn replay_hint(seed: Seed) -> String {
    format!("Replay this seed: {} --seed {seed}", env!("CARGO_BIN_NAME"))
}

/// Returns the line of the end screens a player copies to share the run:
/// which dungeon it was, then how far it went, such as
/// "Sigilfall daily 2026-11-08 - depth 4, turn 812, 23 slain, 9 sigils"
/// for the daily run of `daily`, or
/// "Sigilfall seed 7F3A-91C2-0B5E-D4A8 - depth 4, turn 812, 23 slain, 9 sigils"
/// for a run of `seed` when `daily` is `None`.
///
/// A run whose `status` is [`Status::Won`] says "victory" where the depth
/// would be ("Sigilfall daily 2026-11-08 - victory, turn 4120, 96 slain,
/// 31 sigils"): the seal lies on the bottom floor only, so the word tells
/// the depth as well, and it is one character shorter than "depth 10".
///
/// The counts come from `stats`: the deepest floor reached, the turns
/// played, the creatures slain and the sigils cast, whatever their
/// quality. The line is plain ASCII whatever the glyph option, so it
/// pastes intact anywhere and two players' lines compare as text; its
/// longest realistic form (depth 10, five-digit turns, three-digit kills,
/// two-digit casts, a seed) still fits 80 columns.
pub fn share_line(daily: Option<DailyDate>, seed: Seed, stats: &Stats, status: &Status) -> String {
    let run = match daily {
        Some(date) => format!("daily {date}"),
        None => format!("seed {seed}"),
    };
    let reached = match status {
        Status::Won => "victory".to_owned(),
        Status::Playing | Status::GameOver { .. } => format!("depth {}", stats.deepest()),
    };
    let casts = stats.casts().total();
    format!(
        "Sigilfall {run} - {reached}, turn {}, {} slain, {casts} {}",
        stats.turns(),
        stats.total_kills(),
        if casts == 1 { "sigil" } else { "sigils" }
    )
}

/// Returns the last line of a list of the side panel or the grimoire when
/// `hidden` entries do not fit: "and 3 more".
pub fn more_in_view(hidden: usize) -> String {
    format!("and {hidden} more")
}

/// Returns the name shown to the player for the monster definition
/// `definition` of `content`, such as "ossuary rat".
///
/// A definition missing from `content` (only possible when a game was built
/// from other content than the one given here) is shown by its identifier,
/// so the screen still says something.
pub fn monster_name<'name>(content: &'name Content, definition: &'name str) -> &'name str {
    content
        .monster(definition)
        .map_or(definition, |monster| monster.name())
}

/// Returns the name shown to the player for the sigil identified by `id`
/// in `content`, such as "Ember"; an identifier missing from `content` is
/// shown as is, as [`monster_name`] does.
pub fn sigil_name<'name>(content: &'name Content, id: &'name str) -> &'name str {
    content.sigil(id).map_or(id, SigilDef::name)
}

/// Returns what `sigil` does in a few words, for the grimoire: its effect
/// and power, then whom it reaches, joined by `separator`, such as
/// "harm 6 · nearest" or "blink 4 · aimed".
///
/// The power's unit follows the effect: hit points to harm or heal, turns
/// to hold, cells to blink. "nearest" is the nearest foe in sight,
/// "aimed" a direction asked after the trace, "self" the player.
pub fn effect_summary(sigil: &SigilDef, separator: char) -> String {
    let verb = match sigil.effect() {
        Effect::Damage => "harm",
        Effect::Heal => "heal",
        Effect::Blink => "blink",
        Effect::Hold => "hold",
    };
    let target = match sigil.targeting() {
        Targeting::Caster => "self",
        Targeting::Direction => "aimed",
        Targeting::NearestEnemy => "nearest",
    };
    format!("{verb} {} {separator} {target}", sigil.power())
}

/// Returns the nodes `nodes` as the player presses them, joined by
/// `separator`: "7·5·3". An empty list gives an empty string.
pub fn pattern_text(nodes: &[Node], separator: char) -> String {
    nodes
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(&separator.to_string())
}

/// Returns the time a cast of a trace of `nodes` nodes takes, in turns of
/// the player (one turn being the time of a step), then what each further
/// node adds, joined by `separator`: "1.25 turns · +0.25 a node".
///
/// The cost is `schedule::cast_cost`, the energy `Game::submit` charges for
/// the cast; at the player's speed, a step's worth of energy is one turn.
pub fn cast_time(nodes: usize, separator: char) -> String {
    format!(
        "{} turns {separator} +{} a node",
        turns(cast_cost(nodes)),
        turns(STROKE_COST)
    )
}

/// Returns the notice of the sigil overlay when the player presses the key
/// of `node` while the trace already links it: "Node 5 is already linked."
pub fn node_refused(node: Node) -> String {
    format!("Node {node} is already linked.")
}

/// Returns the notice of the sigil overlay while the player has less
/// essence than a cast costs: "Not enough essence: 1 of 2."
pub fn essence_too_low(essence: i32, cost: i32) -> String {
    format!("Not enough essence: {essence} of {cost}.")
}

/// Returns the title of the sigil overlay while a direction is asked for
/// the sigil named `name`: "Aim Passage".
pub fn aim_title(name: &str) -> String {
    format!("Aim {name}")
}

/// Returns the notice of the sigil overlay while a direction is asked for
/// the sigil named `name`: "Passage is aimed: choose a direction."
pub fn aim_prompt(name: &str) -> String {
    format!("{name} is aimed: choose a direction.")
}

/// `energy` written in turns of the player, with two decimals: 125 is
/// "1.25", one step being [`MOVE_COST`].
fn turns(energy: i32) -> String {
    let hundredths = energy * 100 / MOVE_COST;
    format!("{}.{:02}", hundredths / 100, hundredths % 100)
}

/// How a trace of `quality` is named in the line of its cast.
const fn quality_words(quality: CastQuality) -> &'static str {
    match quality {
        CastQuality::Pure => "pure trace",
        CastQuality::Unstable => "unstable trace",
        CastQuality::Fizzled => "fizzled trace",
    }
}

/// The verb of using an item of `family`: a vial is drunk, a page read, a
/// catalyst used.
const fn use_verb(family: ItemFamily) -> &'static str {
    match family {
        ItemFamily::Vial => "drink",
        ItemFamily::Page => "read",
        ItemFamily::Catalyst => "use",
    }
}

/// What an item restored, for [`restored_line`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Restored {
    /// Hit points.
    Health,
    /// Essence.
    Essence,
}

/// The line of an item that restored `amount` of `what`, opened by `lead`
/// ("You drink the Healing Draught") and closed by the `value` reached out
/// of `max`, so a capped restoration reads as such: "… and recover 8 HP
/// (30/30)." or, when nothing was missing, "…, but you were not hurt
/// (30/30)."
fn restored_line(lead: &str, amount: i32, what: Restored, value: i32, max: i32) -> String {
    let (unit, already) = match what {
        Restored::Health => ("HP", "you were not hurt"),
        Restored::Essence => ("essence", "your essence was full"),
    };
    if amount > 0 {
        format!("{lead} and recover {amount} {unit} ({value}/{max}).")
    } else {
        format!("{lead}, but {already} ({value}/{max}).")
    }
}

/// Whether `event` is the effect of an item that ends the line of its use
/// (see [`describe`]).
const fn is_item_effect(event: &Event) -> bool {
    matches!(
        event,
        Event::Healed { .. }
            | Event::EssenceRestored { .. }
            | Event::SigilLearned { .. }
            | Event::Steadied { .. }
    )
}

/// "1 turn" or "3 turns".
fn turn_count(turns: u32) -> String {
    if turns == 1 {
        "1 turn".to_owned()
    } else {
        format!("{turns} turns")
    }
}

/// Whether `event` is the effect of a spell that ends the line of its cast
/// (see [`describe`]): a backlash on the player is not.
fn is_spell_effect(event: &Event, game: &Game) -> bool {
    match event {
        Event::Wounded { actor, .. } | Event::Held { actor, .. } => *actor != game.player(),
        Event::Healed { .. } | Event::Blinked { .. } | Event::SpellMissed { .. } => true,
        _ => false,
    }
}

/// The [`Tone::Sigil`] line of a cast: `lead` ("Ember, pure trace"), then
/// `effect` ("the shade is seared (6)"). Without a lead, which the order of
/// the events rules out, the effect alone makes the sentence.
fn spell_line(lead: Option<String>, effect: &str) -> Message {
    let text = match lead {
        Some(lead) => format!("{lead}: {effect}."),
        None => {
            let mut chars = effect.chars();
            let first = chars.next().map(|c| c.to_uppercase().to_string());
            format!("{}{}.", first.unwrap_or_default(), chars.as_str())
        }
    };
    Message::new(Tone::Sigil, text)
}

/// Returns who the actor `id` is: the player, a monster still in `game`, or
/// the one an [`Event::Died`] of `events` describes. `None` only if none of
/// these knows it, which the guarantees of `Game::submit` rule out.
fn combatant(id: ActorId, events: &[Event], game: &Game) -> Option<Combatant> {
    if id == game.player() {
        return Some(Combatant::Player);
    }
    if let Some(monster) = game.actor(id).and_then(|actor| actor.as_monster()) {
        return Some(Combatant::Monster {
            definition: monster.definition().to_owned(),
        });
    }
    events.iter().find_map(|event| match event {
        Event::Died { actor, who, .. } if *actor == id => Some(who.clone()),
        _ => None,
    })
}

/// The message of a blow of `damage` hit points struck by `attacker` on
/// `target`, `lethal` when it killed. See [`describe`] for the wording and
/// the tones.
fn blow(
    attacker: &Combatant,
    target: &Combatant,
    damage: i32,
    lethal: bool,
    content: &Content,
) -> Message {
    let verb = match (lethal, attacker) {
        (false, Combatant::Player) => "hit",
        (false, Combatant::Monster { .. }) => "hits",
        (true, Combatant::Player) => "kill",
        (true, Combatant::Monster { .. }) => "kills",
    };
    let tone = match (target, lethal) {
        (Combatant::Player, _) => Tone::Danger,
        (Combatant::Monster { .. }, true) => Tone::Success,
        (Combatant::Monster { .. }, false) => Tone::Info,
    };
    let text = format!(
        "{} {verb} {} ({damage}).",
        subject(attacker, content),
        object(target, content)
    );
    Message::new(tone, text)
}

/// The message of the death of `who` when no killing blow announced it.
fn death(who: &Combatant, content: &Content) -> Message {
    match who {
        Combatant::Player => Message::new(Tone::Danger, "You die."),
        Combatant::Monster { .. } => {
            Message::new(Tone::Success, format!("{} dies.", subject(who, content)))
        }
    }
}

/// `who` at the start of a sentence: "You" or "The shade".
fn subject(who: &Combatant, content: &Content) -> String {
    match who {
        Combatant::Player => "You".to_owned(),
        Combatant::Monster { definition } => format!("The {}", monster_name(content, definition)),
    }
}

/// `who` after a verb: "you" or "the shade".
fn object(who: &Combatant, content: &Content) -> String {
    match who {
        Combatant::Player => "you".to_owned(),
        Combatant::Monster { definition } => format!("the {}", monster_name(content, definition)),
    }
}

/// "an" before a `name` that starts with a vowel letter, "a" otherwise.
///
/// Spelling decides, not sound: right for every monster of the game
/// ("an acolyte", "a hierophant"), wrong for a name such as "unicorn" or
/// "hour", which the content does not have.
fn indefinite_article(name: &str) -> &'static str {
    let starts_with_vowel = name
        .chars()
        .next()
        .is_some_and(|first| "aeiouAEIOU".contains(first));
    if starts_with_vowel { "an" } else { "a" }
}

#[cfg(test)]
mod tests {
    use sigilfall_core::action::Action;
    use sigilfall_core::game::PLAYER_START;
    use sigilfall_core::geometry::{Direction, Position};
    use sigilfall_core::sigil::{Backlash, Trace};

    use super::*;

    /// Seed of the test games; the fixed room does not depend on it.
    const SEED: Seed = Seed::new(42);

    /// The game's own monsters.
    fn content() -> Content {
        Content::load().expect("the shipped content is valid")
    }

    /// A fixed-room game with a monster of `definition` east of the player.
    fn duel(definition: &str) -> (Game, Content) {
        let content = content();
        let mut game = Game::in_fixed_room(SEED);
        let east = game.player_actor().position().step(Direction::East);
        game.spawn_monster(east, content.monster(definition).unwrap());
        (game, content)
    }

    /// The texts of `messages`, for comparisons.
    fn texts(messages: &[Message]) -> Vec<&str> {
        messages.iter().map(|m| m.text.as_str()).collect()
    }

    #[test]
    fn a_blow_on_a_monster_gives_the_damage_and_a_killing_blow_names_it() {
        // The hierophant (14 hit points) survives a first blow; striking it
        // again and again ends with a kill, bounded by its hit points.
        let (mut game, content) = duel("hierophant");
        let first = game.submit(Action::Move(Direction::East));
        let Some(Event::Attacked { damage, .. }) = first.first() else {
            panic!("the move struck: {first:?}");
        };

        let messages = describe(&first, &game, &content);

        assert_eq!(messages[0].tone, Tone::Info);
        assert_eq!(
            messages[0].text,
            format!("You hit the hierophant ({damage}).")
        );

        for _ in 0..20 {
            let events = game.submit(Action::Move(Direction::East));
            let messages = describe(&events, &game, &content);
            if let Some(Event::Attacked {
                hp_left: 0, damage, ..
            }) = events.first()
            {
                assert_eq!(messages[0].tone, Tone::Success);
                assert_eq!(
                    messages[0].text,
                    format!("You kill the hierophant ({damage}).")
                );
                assert!(
                    texts(&messages).iter().all(|t| !t.contains("dies")),
                    "the death is not told twice: {messages:?}"
                );
                return;
            }
        }
        panic!("twenty blows of at least one point kill 14 hit points");
    }

    #[test]
    fn a_monster_blow_on_the_player_is_a_danger_with_its_damage() {
        let (mut game, content) = duel("ossuary_rat");

        // The rat starts with no energy: waiting lets it strike.
        let events: Vec<_> = (0..3).flat_map(|_| game.submit(Action::Wait)).collect();
        let messages = describe(&events, &game, &content);

        let blows: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                Event::Attacked { damage, .. } => Some(*damage),
                _ => None,
            })
            .collect();
        assert!(!blows.is_empty(), "the rat struck: {events:?}");
        let expected: Vec<_> = blows
            .iter()
            .map(|damage| format!("The ossuary rat hits you ({damage})."))
            .collect();
        assert_eq!(texts(&messages), expected);
        assert!(messages.iter().all(|m| m.tone == Tone::Danger));
    }

    /// A monster that kills the player in one blow: its attack of 99
    /// leaves at least 97 damage against the player's 30 hit points.
    const EXECUTIONER: &str = r#"[
        Monster(id: "executioner", name: "executioner", family: Acolyte, elite: true,
            speed: 100, hp: 999, attack: 99, defense: 0, min_depth: 1),
    ]"#;

    #[test]
    fn the_killing_blow_on_the_player_names_the_killer_once() {
        let content = Content::parse(EXECUTIONER).unwrap();
        let mut game = Game::in_fixed_room(SEED);
        let east = game.player_actor().position().step(Direction::East);
        game.spawn_monster(east, content.monster("executioner").unwrap());

        // The monster starts with no energy; a few waits are enough.
        let mut events = Vec::new();
        for _ in 0..3 {
            events.extend(game.submit(Action::Wait));
        }
        assert!(game.is_over(), "{events:?}");
        let Some(Event::Attacked { damage, .. }) = events
            .iter()
            .find(|event| matches!(event, Event::Attacked { .. }))
        else {
            panic!("the executioner struck: {events:?}");
        };

        let messages = describe(&events, &game, &content);

        assert_eq!(
            texts(&messages),
            [format!("The executioner kills you ({damage}).")]
        );
        assert_eq!(messages[0].tone, Tone::Danger);
    }

    #[test]
    fn moves_waits_and_bumps_into_walls_write_nothing() {
        let mut game = Game::in_fixed_room(SEED);
        let content = content();
        let mut events = game.submit(Action::Move(Direction::East));
        events.extend(game.submit(Action::Wait));
        for _ in 0..10 {
            events.extend(game.submit(Action::Move(Direction::North)));
        }
        assert!(
            events
                .iter()
                .any(|event| matches!(event, Event::MoveBlocked { .. })),
            "the walk ended against the wall: {events:?}"
        );

        assert_eq!(describe(&events, &game, &content), []);
    }

    #[test]
    fn the_cause_of_death_names_the_killer_with_its_article() {
        let content = content();
        let by = |definition: &str| Combatant::Monster {
            definition: definition.to_owned(),
        };

        assert_eq!(
            cause_of_death(&by("acolyte"), 4, 1283, &content),
            "Killed by an acolyte at depth 4, on turn 1283."
        );
        assert_eq!(
            cause_of_death(&by("ossuary_rat"), 1, 7, &content),
            "Killed by an ossuary rat at depth 1, on turn 7."
        );
        assert_eq!(
            cause_of_death(&by("hierophant"), 10, 12, &content),
            "Killed by a hierophant at depth 10, on turn 12."
        );
    }

    #[test]
    fn a_death_by_the_player_own_hand_names_no_monster() {
        assert_eq!(
            cause_of_death(&Combatant::Player, 3, 40, &content()),
            "Undone by your own magic at depth 3, on turn 40."
        );
    }

    #[test]
    fn the_epitaph_follows_the_seed_and_every_one_can_be_drawn() {
        let seed = Seed::new(0x7F3A_91C2_0B5E_D4A8);
        assert_eq!(epitaph(seed), epitaph(seed), "deterministic");

        let drawn: HashSet<&str> = (0..64).map(|value| epitaph(Seed::new(value))).collect();
        assert_eq!(drawn.len(), EPITAPHS.len(), "small seeds reach them all");
        for line in EPITAPHS {
            assert!(line.is_ascii() && line.len() <= 60, "{line}");
        }
    }

    #[test]
    fn the_cast_breakdown_names_each_quality_in_order() {
        let casts = CastCounts {
            pure: 22,
            unstable: 6,
            fizzled: 3,
        };

        assert_eq!(
            cast_breakdown(casts, '·'),
            "pure 22 · unstable 6 · fizzled 3"
        );
        assert_eq!(
            cast_breakdown(casts, '-'),
            "pure 22 - unstable 6 - fizzled 3"
        );
    }

    #[test]
    fn an_unknown_definition_is_shown_by_its_identifier() {
        assert_eq!(monster_name(&content(), "mystery_beast"), "mystery_beast");
        assert_eq!(monster_name(&content(), "wax_beetle"), "wax beetle");
    }

    #[test]
    fn the_replay_hint_gives_the_command_with_the_shareable_seed() {
        assert_eq!(
            replay_hint(Seed::new(0x7F3A_91C2_0B5E_D4A8)),
            "Replay this seed: sigilfall --seed 7F3A-91C2-0B5E-D4A8"
        );
    }

    #[test]
    fn the_share_line_names_the_daily_date_or_the_seed_then_the_counts() {
        let date = DailyDate::new(2026, 11, 8).expect("a real date");
        let fresh = Stats::default();

        assert_eq!(
            share_line(Some(date), Seed::daily(date), &fresh, &Status::Playing),
            "Sigilfall daily 2026-11-08 - depth 1, turn 0, 0 slain, 0 sigils"
        );
        assert_eq!(
            share_line(
                None,
                Seed::new(0x7F3A_91C2_0B5E_D4A8),
                &fresh,
                &Status::Playing
            ),
            "Sigilfall seed 7F3A-91C2-0B5E-D4A8 - depth 1, turn 0, 0 slain, 0 sigils"
        );
    }

    #[test]
    fn the_share_line_reads_every_count_from_the_stats() {
        let content = content();
        let mut game = Game::in_fixed_room(SEED);
        let rat = content.monster("ossuary_rat").expect("a shipped monster");
        let east = PLAYER_START.step(Direction::East);
        game.spawn_monster(east, rat);
        while game.stats().total_kills() == 0 {
            game.submit(Action::Move(Direction::East));
        }
        game.submit(Action::Cast {
            trace: Trace::from_pattern(&[1, 2, 3, 6, 9, 8, 7, 4]).expect("a trace"),
            direction: None,
        });
        let stats = game.stats();
        assert_eq!(stats.casts().total(), 1, "one cast, fizzled");

        assert_eq!(
            share_line(None, SEED, stats, game.status()),
            format!(
                "Sigilfall seed {SEED} - depth 1, turn {}, 1 slain, 1 sigil",
                stats.turns()
            )
        );
        assert!(stats.turns() > 1);
    }

    #[test]
    fn the_longest_share_line_fits_80_columns_in_plain_ascii() {
        // Depth 10, the deepest floor, a seed (longer than a date), and
        // counts far beyond the runs of the balance tests (deaths at depth
        // 2 to 7, a few hundred turns).
        // A fresh run writes one digit for each count: the longest line
        // has 1 + 4 + 2 + 1 more.
        let fresh = share_line(
            None,
            Seed::new(u64::MAX),
            &Stats::default(),
            &Status::Playing,
        );
        let longest = fresh.len() + 1 + 4 + 2 + 1;

        assert!(fresh.is_ascii(), "{fresh}");
        assert!(longest <= 80, "{longest} columns: {fresh}");
        // A victory writes "victory" in the place of "depth 1", which
        // leaves room for "depth 10": its longest line is shorter still.
        let won = share_line(None, Seed::new(u64::MAX), &Stats::default(), &Status::Won);
        assert!(won.is_ascii(), "{won}");
        assert!(won.len() + 4 + 2 + 1 < longest, "{won}");
    }

    #[test]
    fn the_share_line_of_a_victory_says_so_in_place_of_the_depth() {
        let date = DailyDate::new(2026, 11, 8).expect("a real date");
        let fresh = Stats::default();

        assert_eq!(
            share_line(Some(date), Seed::daily(date), &fresh, &Status::Won),
            "Sigilfall daily 2026-11-08 - victory, turn 0, 0 slain, 0 sigils"
        );
        assert_eq!(
            share_line(None, Seed::new(0x7F3A_91C2_0B5E_D4A8), &fresh, &Status::Won),
            "Sigilfall seed 7F3A-91C2-0B5E-D4A8 - victory, turn 0, 0 slain, 0 sigils"
        );
    }

    #[test]
    fn the_victory_line_follows_the_seed_and_every_one_can_be_drawn() {
        let seed = Seed::new(0x7F3A_91C2_0B5E_D4A8);
        assert_eq!(victory_line(seed), victory_line(seed), "deterministic");

        let drawn: HashSet<&str> = (0..64)
            .map(|value| victory_line(Seed::new(value)))
            .collect();
        assert_eq!(
            drawn.len(),
            VICTORY_LINES.len(),
            "small seeds reach them all"
        );
        for line in VICTORY_LINES {
            assert!(line.is_ascii() && line.len() <= 60, "{line}");
            assert!(!EPITAPHS.contains(&line), "{line}");
        }
        assert_eq!(
            victory_outcome(10, 1283),
            "You broke the seal at depth 10, on turn 1283."
        );
    }

    #[test]
    fn a_monster_dying_without_a_blow_is_still_told() {
        let events = [Event::Died {
            actor: ActorId::default(),
            who: Combatant::Monster {
                definition: "shade".to_owned(),
            },
            killed_by: Combatant::Player,
        }];
        let game = Game::in_fixed_room(SEED);

        let messages = describe(&events, &game, &content());

        assert_eq!(texts(&messages), ["The shade dies."]);
        assert_eq!(messages[0].tone, Tone::Success);
    }

    /// A fixed-room game with an ossuary rat three cells east of the
    /// player, its key, and the shipped content.
    fn spell_target() -> (Game, ActorId, Content) {
        let content = content();
        let mut game = Game::in_fixed_room(SEED);
        let cell = Position::new(8, 5);
        let rat = game.spawn_monster(cell, content.monster("ossuary_rat").unwrap());
        (game, rat, content)
    }

    /// The event of a cast of the sigil `id` with `quality`.
    fn cast(id: &str, quality: CastQuality) -> Event {
        Event::SigilCast {
            sigil: Some(id.to_owned()),
            quality,
        }
    }

    /// The tone and text of each of `messages`.
    fn lines(messages: &[Message]) -> Vec<(Tone, &str)> {
        messages.iter().map(|m| (m.tone, m.text.as_str())).collect()
    }

    #[test]
    fn a_cast_and_its_effect_make_one_sigil_line() {
        let (game, rat, content) = spell_target();
        let player = game.player();
        let cases = [
            (
                vec![
                    cast("ember", CastQuality::Pure),
                    Event::Wounded {
                        actor: rat,
                        damage: 6,
                        hp_left: 2,
                    },
                ],
                "Ember, pure trace: the ossuary rat is seared (6).",
            ),
            (
                vec![
                    cast("recall", CastQuality::Pure),
                    Event::Healed {
                        actor: player,
                        amount: 8,
                        hp: 30,
                    },
                ],
                "Recall, pure trace: you mend (8).",
            ),
            (
                vec![
                    cast("recall", CastQuality::Pure),
                    Event::Healed {
                        actor: player,
                        amount: 0,
                        hp: 30,
                    },
                ],
                "Recall, pure trace: you were not hurt.",
            ),
            (
                vec![
                    cast("veil", CastQuality::Pure),
                    Event::Held {
                        actor: rat,
                        turns: 2,
                    },
                ],
                "Veil, pure trace: the ossuary rat is held (2 turns).",
            ),
            (
                vec![
                    cast("passage", CastQuality::Pure),
                    Event::Blinked {
                        actor: player,
                        from: Position::new(5, 5),
                        to: Position::new(9, 5),
                    },
                ],
                "Passage, pure trace: you blink 4 cells.",
            ),
            (
                vec![
                    cast("passage", CastQuality::Pure),
                    Event::Blinked {
                        actor: player,
                        from: Position::new(1, 5),
                        to: Position::new(1, 5),
                    },
                ],
                "Passage, pure trace: the way is blocked.",
            ),
            (
                vec![
                    cast("lance", CastQuality::Pure),
                    Event::SpellMissed {
                        sigil: "lance".to_owned(),
                    },
                ],
                "Lance, pure trace: no foe in reach.",
            ),
        ];

        for (events, expected) in cases {
            let messages = describe(&events, &game, &content);
            assert_eq!(lines(&messages), [(Tone::Sigil, expected)], "{events:?}");
        }
    }

    #[test]
    fn a_spell_that_kills_says_so_once() {
        let (game, rat, content) = spell_target();
        let events = [
            cast("ember", CastQuality::Pure),
            Event::Wounded {
                actor: rat,
                damage: 6,
                hp_left: 0,
            },
            Event::Died {
                actor: rat,
                who: Combatant::Monster {
                    definition: "ossuary_rat".to_owned(),
                },
                killed_by: Combatant::Player,
            },
        ];

        let messages = describe(&events, &game, &content);

        assert_eq!(
            texts(&messages),
            ["Ember, pure trace: the ossuary rat is destroyed (6)."]
        );
    }

    #[test]
    fn an_unstable_cast_is_followed_by_its_backlash_as_a_danger() {
        let (game, rat, content) = spell_target();
        let player = game.player();
        let backlashes = [
            (
                Backlash::Scorch,
                Event::Wounded {
                    actor: player,
                    damage: 2,
                    hp_left: 28,
                },
                "The backlash scorches you (2).",
            ),
            (
                Backlash::Drain,
                Event::EssenceDrained {
                    actor: player,
                    lost: 4,
                },
                "The backlash drains your essence (4).",
            ),
            (
                Backlash::Daze,
                Event::Held {
                    actor: player,
                    turns: 1,
                },
                "The backlash dazes you: you lose 1 turn.",
            ),
        ];

        for (backlash, consequence, expected) in backlashes {
            let events = [
                cast("ember", CastQuality::Unstable),
                Event::Wounded {
                    actor: rat,
                    damage: 6,
                    hp_left: 2,
                },
                Event::Backlash {
                    caster: player,
                    backlash,
                },
                consequence,
            ];
            let messages = describe(&events, &game, &content);
            assert_eq!(
                lines(&messages),
                [
                    (
                        Tone::Sigil,
                        "Ember, unstable trace: the ossuary rat is seared (6)."
                    ),
                    (Tone::Danger, expected),
                ],
                "{backlash:?}"
            );
        }
    }

    #[test]
    fn a_backlash_that_takes_nothing_is_never_told_as_a_loss_of_zero() {
        let (game, _, content) = spell_target();
        let player = game.player();
        let cases = [
            (
                Backlash::Scorch,
                Event::Wounded {
                    actor: player,
                    damage: 0,
                    hp_left: 1,
                },
                (Tone::Danger, "The backlash scorches you, but you hold on."),
            ),
            (
                Backlash::Scorch,
                Event::Wounded {
                    actor: player,
                    damage: 1,
                    hp_left: 1,
                },
                (Tone::Danger, "The backlash scorches you (1)."),
            ),
            (
                Backlash::Drain,
                Event::EssenceDrained {
                    actor: player,
                    lost: 0,
                },
                (Tone::Info, "The backlash finds no essence to drain."),
            ),
        ];

        for (backlash, consequence, expected) in cases {
            let events = [
                Event::Backlash {
                    caster: player,
                    backlash,
                },
                consequence,
            ];
            let messages = describe(&events, &game, &content);
            assert_eq!(lines(&messages), [expected], "{backlash:?}");
            assert!(!messages[0].text.contains("(0)"));
        }
    }

    #[test]
    fn fizzles_refusals_and_glyphs_have_their_own_lines() {
        let (game, _, content) = spell_target();
        let events = [
            Event::SigilCast {
                sigil: None,
                quality: CastQuality::Fizzled,
            },
            Event::NotEnoughEssence {
                essence: 1,
                cost: 2,
            },
            Event::DirectionMissing {
                sigil: "passage".to_owned(),
            },
            Event::GlyphLearned {
                sigil: "recall".to_owned(),
                position: Position::new(6, 5),
            },
        ];

        let messages = describe(&events, &game, &content);

        assert_eq!(
            lines(&messages),
            [
                (
                    Tone::Sigil,
                    "The trace matches no sigil you know and fizzles."
                ),
                (Tone::Info, "Not enough essence to cast: 1 of 2."),
                (Tone::Info, "Passage needs a direction."),
                (
                    Tone::Success,
                    "You decipher a carved glyph: Recall joins your grimoire."
                ),
            ]
        );
    }

    #[test]
    fn an_unknown_pattern_has_one_line_that_points_to_the_glyphs() {
        let (game, _, content) = spell_target();
        let events = [
            Event::SigilCast {
                sigil: None,
                quality: CastQuality::Fizzled,
            },
            Event::UnknownPattern,
        ];

        let messages = describe(&events, &game, &content);

        assert_eq!(
            lines(&messages),
            [(
                Tone::Sigil,
                "That pattern is not in your grimoire, and the trace fizzles. \
                 Sigils are learnt by reading the glyphs carved in the floor."
            )]
        );
    }

    #[test]
    fn a_cast_without_an_effect_event_is_still_told() {
        let (game, _, content) = spell_target();

        let messages = describe(&[cast("ember", CastQuality::Pure)], &game, &content);

        assert_eq!(lines(&messages), [(Tone::Sigil, "Ember, pure trace.")]);
    }

    #[test]
    fn the_overlay_texts_follow_the_glyph_set() {
        let ember = content().sigil("ember").unwrap().clone();

        assert_eq!(
            pattern_text(ember.pattern().nodes(), '\u{b7}'),
            "7\u{b7}5\u{b7}3"
        );
        assert_eq!(pattern_text(ember.pattern().nodes(), '-'), "7-5-3");
        assert_eq!(pattern_text(&[], '-'), "");
        assert_eq!(effect_summary(&ember, '-'), "harm 6 - nearest");
        assert_eq!(cast_time(3, '-'), "1.25 turns - +0.25 a node");
        assert_eq!(cast_time(0, '-'), "0.50 turns - +0.25 a node");
        assert_eq!(cast_time(9, '-'), "2.75 turns - +0.25 a node");
    }

    #[test]
    fn every_shipped_sigil_has_a_short_effect_summary() {
        let summaries: Vec<String> = content()
            .sigils()
            .iter()
            .map(|sigil| effect_summary(sigil, '-'))
            .collect();

        assert_eq!(
            summaries,
            [
                "harm 6 - nearest",
                "heal 8 - self",
                "hold 2 - nearest",
                "blink 4 - aimed",
                "harm 9 - aimed",
            ]
        );
    }

    /// The event of the item of definition `id` used up.
    fn used(id: &str) -> Event {
        Event::Used {
            item: ItemId::default(),
            definition: id.to_owned(),
        }
    }

    #[test]
    fn an_item_used_and_its_effect_make_one_line_with_the_capped_value() {
        let (game, _, content) = spell_target();
        let player = game.player();
        let cases = [
            (
                vec![
                    used("healing_draught"),
                    Event::Healed {
                        actor: player,
                        amount: 8,
                        hp: 30,
                    },
                ],
                "You drink the Healing Draught and recover 8 HP (30/30).",
            ),
            (
                vec![
                    used("healing_draught"),
                    Event::Healed {
                        actor: player,
                        amount: 0,
                        hp: 30,
                    },
                ],
                "You drink the Healing Draught, but you were not hurt (30/30).",
            ),
            (
                vec![
                    used("essence_phial"),
                    Event::EssenceRestored {
                        actor: player,
                        amount: 4,
                        essence: 5,
                    },
                ],
                "You drink the Essence Phial and recover 4 essence (5/6).",
            ),
            (
                vec![
                    used("essence_phial"),
                    Event::EssenceRestored {
                        actor: player,
                        amount: 0,
                        essence: 6,
                    },
                ],
                "You drink the Essence Phial, but your essence was full (6/6).",
            ),
            (
                vec![
                    used("veil_page"),
                    Event::SigilLearned {
                        sigil: "veil".to_owned(),
                    },
                ],
                "You read the Page of the Veil: Veil joins your grimoire.",
            ),
            (
                vec![used("steadying_ink"), Event::Steadied { casts: 2 }],
                "You use the Steadying Ink: your next 2 unstable casts will be cast pure.",
            ),
            (
                vec![used("steadying_ink"), Event::Steadied { casts: 1 }],
                "You use the Steadying Ink: your next unstable cast will be cast pure.",
            ),
            (
                vec![used("healing_draught")],
                "You drink the Healing Draught.",
            ),
        ];

        for (events, expected) in cases {
            let messages = describe(&events, &game, &content);
            assert_eq!(lines(&messages), [(Tone::Success, expected)], "{events:?}");
        }
    }

    #[test]
    fn a_healing_spell_and_a_healing_draught_read_apart() {
        let (game, _, content) = spell_target();
        let healed = Event::Healed {
            actor: game.player(),
            amount: 8,
            hp: 30,
        };

        let spell = describe(
            &[cast("recall", CastQuality::Pure), healed.clone()],
            &game,
            &content,
        );
        let draught = describe(&[used("healing_draught"), healed], &game, &content);

        assert_eq!(texts(&spell), ["Recall, pure trace: you mend (8)."]);
        assert_eq!(
            texts(&draught),
            ["You drink the Healing Draught and recover 8 HP (30/30)."]
        );
    }

    #[test]
    fn picking_up_dropping_and_refusals_have_their_own_lines() {
        let (game, _, content) = spell_target();
        let item = ItemId::default();
        let events = [
            Event::PickedUp {
                item,
                definition: "healing_draught".to_owned(),
            },
            Event::NothingToPickUp {
                position: Position::new(5, 5),
            },
            Event::InventoryFull {
                item,
                definition: "steadying_ink".to_owned(),
            },
            Event::Dropped {
                item,
                definition: "essence_phial".to_owned(),
                position: Position::new(5, 5),
            },
            Event::ItemRefused {
                item,
                reason: ItemRefusal::NotCarried,
            },
        ];

        let messages = describe(&events, &game, &content);

        assert_eq!(
            lines(&messages),
            [
                (Tone::Success, "You pick up the Healing Draught."),
                (Tone::Info, "There is nothing here to pick up."),
                (
                    Tone::Info,
                    "Your pack is full (10/10): the Steadying Ink stays on the floor."
                ),
                (Tone::Info, "You drop the Essence Phial."),
                (Tone::Info, "You do not carry that item."),
            ]
        );
    }

    #[test]
    fn a_descent_names_the_new_depth_and_a_refused_one_says_why() {
        let (game, _, content) = spell_target();
        let events = [
            Event::NoStairsHere {
                position: Position::new(5, 5),
            },
            Event::Descended { depth: 2 },
            Event::Descended { depth: 9 },
        ];

        let messages = describe(&events, &game, &content);

        assert_eq!(
            lines(&messages),
            [
                (Tone::Info, "There are no stairs here."),
                (Tone::Info, "You descend to depth 2."),
                (Tone::Info, "You descend to depth 9."),
            ]
        );
    }

    #[test]
    fn the_descent_to_the_bottom_floor_says_it_is_the_last_and_holds_the_seal() {
        let (game, _, content) = spell_target();
        let line = "You descend to depth 10, the last. The seal lies here.";
        assert!(generation::is_bottom_floor(10), "10 is the bottom floor");
        let events = [Event::Descended { depth: 10 }];

        let messages = describe(&events, &game, &content);

        assert_eq!(lines(&messages), [(Tone::Info, line)]);
        assert!(
            line.chars().count() <= TEXT_WIDTH,
            "the line fits the log without wrapping"
        );
    }

    #[test]
    fn nothing_left_to_explore_is_said_once() {
        let (game, _, content) = spell_target();
        let events = [Event::NothingToExplore {
            position: Position::new(5, 5),
        }];

        let messages = describe(&events, &game, &content);

        assert_eq!(
            lines(&messages),
            [(Tone::Info, "There is nothing left to explore within reach.")]
        );
        assert_eq!(exploration_halted(&Halt::Explored, &content), None);
    }

    #[test]
    fn every_other_halt_of_exploration_says_why() {
        let content = content();
        let halts = [
            (
                Halt::FoeInView("ossuary_rat".to_owned()),
                Tone::Info,
                "You cannot explore with the ossuary rat in view.",
            ),
            (
                Halt::FoeAppeared("ossuary_rat".to_owned()),
                Tone::Danger,
                "An ossuary rat comes into view.",
            ),
            (
                Halt::FoeAppeared("shade".to_owned()),
                Tone::Danger,
                "A shade comes into view.",
            ),
            (
                Halt::ItemAppeared("essence_phial".to_owned()),
                Tone::Info,
                "You spot an Essence Phial.",
            ),
            (
                Halt::ItemAppeared("healing_draught".to_owned()),
                Tone::Info,
                "You spot a Healing Draught.",
            ),
            (
                Halt::GlyphAppeared,
                Tone::Sigil,
                "You spot a carved glyph you have not read.",
            ),
            (Halt::Hurt, Tone::Danger, "You are hurt and stop exploring."),
            (
                Halt::Blocked,
                Tone::Info,
                "Something stands in your way; you stop exploring.",
            ),
            (
                Halt::StepLimit,
                Tone::Info,
                "You stop exploring after 200 steps.",
            ),
            (Halt::Interrupted, Tone::Info, "You stop exploring."),
        ];

        for (halt, tone, text) in halts {
            let message = exploration_halted(&halt, &content);

            assert_eq!(message, Some(Message::new(tone, text)), "{halt:?}");
            assert!(text.chars().count() <= TEXT_WIDTH, "{text}");
        }
    }

    #[test]
    fn a_refusal_names_the_item_and_why() {
        let content = content();
        let mut game = Game::in_fixed_room(SEED);
        let page = game.place_item(Position::new(6, 5), content.item("veil_page").unwrap());
        let draught = game.place_item(
            Position::new(7, 5),
            content.item("healing_draught").unwrap(),
        );

        assert_eq!(
            item_refusal(&game, page, ItemRefusal::NothingToLearn),
            "You already know Veil: the Page of the Veil has nothing to teach."
        );
        assert_eq!(
            item_refusal(&game, draught, ItemRefusal::CellTaken),
            "Something already lies here: drop the Healing Draught elsewhere."
        );
        assert_eq!(
            item_refusal(&game, ItemId::default(), ItemRefusal::CellTaken),
            "Something already lies here: drop that item elsewhere."
        );
    }

    #[test]
    fn a_steadied_cast_says_so_in_its_line() {
        let (game, rat, content) = spell_target();
        let events = [
            cast("ember", CastQuality::Pure),
            Event::CastSteadied {
                sigil: "ember".to_owned(),
                casts_left: 1,
            },
            Event::Wounded {
                actor: rat,
                damage: 6,
                hp_left: 2,
            },
        ];

        let messages = describe(&events, &game, &content);

        assert_eq!(
            lines(&messages),
            [(
                Tone::Sigil,
                "Ember, steadied trace (1 left): the ossuary rat is seared (6)."
            )]
        );
    }

    #[test]
    fn every_shipped_item_has_a_short_summary() {
        let summaries: Vec<String> = content()
            .items()
            .iter()
            .map(|item| item_summary(item, false, '-'))
            .collect();

        assert_eq!(
            summaries,
            [
                "heal 10 HP",
                "restore 4 essence",
                "steady 2 casts",
                "teach Veil"
            ]
        );
        let page = content().item("veil_page").unwrap().clone();
        assert_eq!(item_summary(&page, true, '-'), "teach Veil - known");
        assert_eq!(capacity(3), "3/10");
    }

    #[test]
    fn a_refused_save_gives_its_cause_whole_and_cuts_only_what_it_quotes() {
        use sigilfall_core::save::Reference;

        // The sentences of the game are never cut, however long.
        let cause =
            "the hit points of the actor in slot 4294967295 is -2147483648, expected 0 to 999";
        assert_eq!(
            save_refusal(&SaveError::Inconsistent(cause.to_owned())),
            format!("{cause} (inconsistent save)")
        );
        assert_eq!(
            save_refusal(&SaveError::Inconsistent(
                "floor 10, the bottom floor, has 2 seals".to_owned()
            )),
            "floor 10, the bottom floor, has 2 seals (inconsistent save)"
        );
        // What the reader quotes from the file is cut, the sentence kept.
        let found = format!("found `{}`", "x".repeat(20_000));
        let malformed = save_refusal(&SaveError::Malformed {
            line: 1,
            column: 20_002,
            message: found,
        });
        assert!(
            malformed.starts_with("the save is unreadable at line 1, column 20002: found `x"),
            "{malformed}"
        );
        assert!(malformed.ends_with(ELLIPSIS), "{malformed}");
        let quote = malformed.split(": ").nth(1).unwrap();
        assert_eq!(quote.chars().count(), MAX_QUOTED_CHARS, "{malformed}");
        let unknown = save_refusal(&SaveError::UnknownReference {
            kind: Reference::Monster,
            id: "d".repeat(500),
        });
        assert!(
            unknown.starts_with("the save references monster `d"),
            "{unknown}"
        );
        assert!(
            unknown.ends_with(&format!("{ELLIPSIS}`, absent from the content")),
            "{unknown}"
        );
        // A short quote is kept whole.
        assert_eq!(
            save_refusal(&SaveError::Malformed {
                line: 3,
                column: 7,
                message: "Expected colon".to_owned()
            }),
            "the save is unreadable at line 3, column 7: Expected colon"
        );
    }

    #[test]
    fn no_control_character_of_a_file_reaches_the_screen_or_the_error_output() {
        use sigilfall_core::save::Reference;

        let hostile = "dra\u{1b}[2Jgon\u{7}\r\n\u{9b}31m";
        let refusal = save_refusal(&SaveError::UnknownReference {
            kind: Reference::Monster,
            id: hostile.to_owned(),
        });
        assert_eq!(
            refusal,
            "the save references monster `dra?[2Jgon????31m`, absent from the content"
        );
        assert_eq!(
            save_refusal(&SaveError::Malformed {
                line: 1,
                column: 2,
                message: hostile.to_owned()
            }),
            "the save is unreadable at line 1, column 2: dra?[2Jgon????31m"
        );
        for text in [
            save_set_aside(hostile, hostile).text,
            save_set_aside_notice(hostile, hostile),
            save_not_deleted_notice(hostile, hostile),
            finished_save_not_deleted_notice(hostile, hostile),
            options_unreadable(hostile),
        ] {
            assert!(!text.chars().any(char::is_control), "{text:?}");
        }
        assert_eq!(printable("plain text"), "plain text");
    }

    #[test]
    fn a_long_reason_is_cut_with_an_ellipsis_and_a_short_one_kept_whole() {
        let short = "the save is unreadable at line 1, column 20002: Expected colon";
        assert_eq!(bounded_reason(short), short);
        let limit = "r".repeat(MAX_REASON_CHARS);
        assert_eq!(bounded_reason(&limit), limit);

        let long = format!("the save is unreadable: found `{}`", "x".repeat(20_000));
        let cut = bounded_reason(&long);
        assert_eq!(cut.chars().count(), MAX_REASON_CHARS);
        assert!(cut.ends_with(ELLIPSIS), "{cut}");
        assert!(cut.starts_with("the save is unreadable: found `x"), "{cut}");
        // No space is left before the ellipsis.
        let spaced = format!("{} {}", "w".repeat(MAX_REASON_CHARS - 2), "tail");
        assert_eq!(
            bounded_reason(&spaced),
            format!("{}{ELLIPSIS}", "w".repeat(MAX_REASON_CHARS - 2))
        );
    }

    #[test]
    fn a_path_longer_than_a_line_loses_its_start_and_keeps_the_file_name() {
        let short = r"C:\Users\player\AppData\Roaming\sigilfall\data\save.ron.bad";
        assert_eq!(bounded_path(short), short);

        let folder = "d".repeat(120);
        let long = format!(r"C:\{folder}\save.ron.bad");
        let cut = bounded_path(&long);
        assert_eq!(cut.chars().count(), MAX_PATH_CHARS);
        assert!(cut.starts_with(ELLIPSIS), "{cut}");
        assert!(long.ends_with(cut.trim_start_matches(ELLIPSIS)), "{cut}");
        assert!(cut.ends_with(r"\save.ron.bad"), "{cut}");
        // Characters beyond ASCII count as one each, and are not split.
        let accented = format!("{}\\é.bad", "é".repeat(100));
        assert_eq!(bounded_path(&accented).chars().count(), MAX_PATH_CHARS);
    }

    #[test]
    fn every_message_about_a_file_keeps_its_path_whole_or_its_end() {
        let reason = "x".repeat(20_000);
        let refusal = save_refusal(&SaveError::Malformed {
            line: 1,
            column: 1,
            message: reason.clone(),
        });
        let path = format!(r"C:\{}\save.ron.bad", "p".repeat(104));
        assert_eq!(path.chars().count(), 120);
        let shown = bounded_path(&path);
        for text in [
            save_set_aside(&refusal, &path).text,
            save_set_aside_notice(&refusal, &path),
            save_not_deleted_notice(&path, &reason),
            finished_save_not_deleted_notice(&path, &reason),
        ] {
            assert!(text.contains(&shown), "{text}");
            assert!(text.contains(r"\save.ron.bad"), "{text}");
            assert!(text.chars().count() < 400, "{text}");
        }
        assert!(options_unreadable(&reason).chars().count() < 200);
    }

    #[test]
    fn the_readme_names_the_colour_values_as_the_options_screen_shows_them() {
        /// The README, from the root of the repository.
        const README: &str = include_str!("../../../README.md");
        let options = README
            .split("### Options")
            .nth(1)
            .and_then(|section| {
                section
                    .split(
                        "
## ",
                    )
                    .next()
            })
            .expect("the README has an Options section");
        let options = options.split_whitespace().collect::<Vec<_>>().join(" ");

        let listed = format!(
            "**Colours** ({}, {ANSI16_VALUE} or {MONOCHROME_VALUE})",
            auto_colors(ColorSupport::None, '=')
                .split_whitespace()
                .next()
                .unwrap()
        );
        assert!(options.contains(&listed), "{listed} in {options}");
    }

    #[test]
    fn the_colour_help_names_the_values_as_the_options_screen_shows_them() {
        let help = OPTION_HELP[1];
        assert!(help.contains(ANSI16_VALUE), "{help}");
        assert!(help.contains(MONOCHROME_VALUE), "{help}");
        assert!(!help.contains("none"), "{help}");
    }
}
