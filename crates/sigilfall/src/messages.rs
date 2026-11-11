//! The catalogue of every sentence and label the player reads, in English
//! and in French: the message log, the status line, the side panel, the
//! sigil overlay, the inventory, the end screens, the title screen with its
//! seed entry and confirmation, the options screen, the notices about the
//! save and options files, and the resize message.
//!
//! Every text goes through here, for one [`Language`]:
//! - fixed texts are the fields of [`Labels`] ([`labels()`]), whose English
//!   and French values must both be complete to compile;
//! - texts that take values are functions with a `language` parameter,
//!   both wordings side by side in one `match`: [`describe`], which turns
//!   the events of one `Game::submit` into [`Message`]s, the opening
//!   messages ([`welcome`], [`welcome_with_sigil`], [`welcome_back`],
//!   [`opening`]), the hint about Caps Lock ([`caps_lock_hint`]), the notices about
//!   files ([`file_notice`], with [`save_refusal`] and [`options_problem`]),
//!   the save notice [`save_not_deleted`], why automatic exploration stopped
//!   ([`exploration_halted`]), the words of the end screens ([`epitaph`],
//!   [`cause_of_death`], [`cast_breakdown`], [`victory_line`],
//!   [`victory_outcome`], [`replay_hint`]), the names of the content
//!   ([`monster_name`], [`sigil_name`], [`item_name`]), what a sigil or an
//!   item does ([`effect_summary`], [`item_summary`]), the time a cast takes
//!   ([`cast_time`]), the notices of the overlay and the inventory, and the
//!   words of the title and options screens ([`saved_run`],
//!   [`abandon_warning`], [`invalid_seed`], [`auto_colors`]…).
//!
//! A plain function per text, with a `match` on the language, was chosen
//! over a translation framework (Fluent, gettext): two languages, sentences
//! whose grammar differs (French agrees a participle with the gender of a
//! creature, elides its article), and the compiler checking that no
//! language misses a case, without files read at run time.
//!
//! The line to share ([`share_line`]) is the one text kept in English
//! whatever the language: two players compare their lines as text, so the
//! line of a seed must read the same on every screen.
//!
//! The messages about the save and options files quote paths and errors
//! that come from outside the game: what they quote is bounded
//! ([`MAX_REASON_CHARS`], [`MAX_QUOTED_CHARS`], [`ELLIPSIS`]) so that they
//! always fit the rows the screen gives them, and made [`printable`] so that
//! it never acts on the terminal. The system's own error texts and the
//! reader's words quoted from a broken file are shown as the system and the
//! reader give them.
//!
//! French texts follow French typography: a no-break space (`\u{a0}`)
//! before `:`, `;`, `!` and `?` and inside guillemets, so the mark is never
//! carried alone to the next line.
//!
//! The game itself never writes text (decision D4): it reports events, and
//! this module chooses what the player reads about them.

mod grammar;
mod labels;
#[cfg(test)]
mod tests;

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
use sigilfall_core::save::{Reference, SaveError};
use sigilfall_core::schedule::{MOVE_COST, STROKE_COST, cast_cost};
use sigilfall_core::sigil::{CastQuality, Node};
use sigilfall_core::stats::{CastCounts, Stats};

use self::grammar::{Who, capitalized, of_indefinite};
pub use self::labels::{ENGLISH, FRENCH, Labels};
use crate::app::exploring::{Halt, MAX_EXPLORE_STEPS};
use crate::keyboard::Keyboard;
use crate::language::Language;
use crate::message_log::{Message, Tone};
use crate::persist::{OPTIONS_FORMAT_VERSION, OptionsProblem, SaveRefusal};
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

/// Most characters of a file path in a message: one line of the log or of
/// a title notice ([`TEXT_WIDTH`]), less the punctuation that follows the
/// path, so the path is never broken across lines. A longer path loses its
/// start (see [`bounded_path`]).
const MAX_PATH_CHARS: usize = TEXT_WIDTH - 1;

/// No-break space, written before `:`, `;`, `!` and `?` and inside
/// guillemets in French.
const NBSP: char = '\u{a0}';

/// Mark written before a value that warns of a danger, so the warning reads
/// without colour: health below a quarter of the maximum, essence too low
/// to cast, a node the trace refused. The same in every language.
pub const WARNING_MARK: &str = "!";

/// Key that waits, in the key help of the game screen.
pub const WAIT_KEY: &str = ".";

/// Key that opens the sigil overlay, in the key help of the game screen.
pub const TRACE_KEY: &str = "t";

/// Key that picks up the item underfoot, in the key help of the game
/// screen; `,` does the same, as in other roguelikes.
pub const PICK_UP_KEY: &str = "g";

/// Key that opens the inventory, in the key help of the game screen.
pub const INVENTORY_KEY: &str = "i";

/// Key that drops the chosen item, in the key help of the inventory.
pub const DROP_KEY: &str = "d";

/// Name of the game on the title screen, letter-spaced like the end
/// screen's title; a name, the same in every language.
pub const GAME_TITLE: &str = "S I G I L F A L L";

/// Returns the fixed texts of `language`.
pub const fn labels(language: Language) -> &'static Labels {
    match language {
        Language::English => &ENGLISH,
        Language::French => &FRENCH,
    }
}

/// The message shown instead of any screen on a terminal smaller than
/// `min_width` × `min_height`: what is needed and the `width` × `height`
/// of now, `times` being the theme's dimension sign.
pub fn enlarge_terminal(
    language: Language,
    (min_width, min_height): (u16, u16),
    (width, height): (u16, u16),
    times: char,
) -> String {
    match language {
        Language::English => format!(
            "Enlarge the terminal to {min_width} {times} {min_height} (current: {width} {times} {height})."
        ),
        Language::French => format!(
            "Agrandissez le terminal à {min_width} {times} {min_height} (actuel{NBSP}: {width} {times} {height})."
        ),
    }
}

/// What the title screen says of the saved game, beside "Continue": its
/// depth and the turn it stopped on, such as "depth 3 · turn 412", with
/// the theme's `separator`.
pub fn saved_run(language: Language, depth: u32, turn: u64, separator: char) -> String {
    match language {
        Language::English => format!("depth {depth} {separator} turn {turn}"),
        Language::French => format!("profondeur {depth} {separator} tour {turn}"),
    }
}

/// The version of the game, as the title screen shows it in its corner:
/// "v" and the version of this binary, such as "v0.1.0".
pub fn version() -> String {
    format!("v{}", env!("CARGO_PKG_VERSION"))
}

/// What the seed entry says when `typed` is not a seed: the text as typed
/// and what to do.
///
/// With the warning mark before it, the sentence fits 80 columns whole even
/// when `typed` fills the field (`ui::title::SEED_CAPACITY`, 24
/// characters): 71 columns in English, 69 in French, whose guillemets and
/// no-break spaces take two more columns than the English quotes, hence
/// its shorter "see above".
pub fn invalid_seed(language: Language, typed: &str) -> String {
    match language {
        Language::English => format!("\"{typed}\" is not a seed: use one of the forms above."),
        Language::French => {
            format!("«{NBSP}{typed}{NBSP}» n'est pas une graine{NBSP}: voir ci-dessus.")
        }
    }
}

/// The warning under the question of the confirmation: the game in
/// progress, described by `saved` (see [`saved_run`]), will be abandoned
/// and its save deleted. Two lines, each within 60 columns.
pub fn abandon_warning(language: Language, saved: &str) -> [String; 2] {
    match language {
        Language::English => [
            format!("The game in progress ({saved}) will be abandoned"),
            "and its save deleted for good.".to_owned(),
        ],
        Language::French => [
            format!("La partie en cours ({saved})"),
            "sera abandonnée et sa sauvegarde supprimée.".to_owned(),
        ],
    }
}

/// Value of the colour option "Auto", with what the terminal was `detected`
/// to support, so the player knows what Auto gives, such as
/// "Auto · true colour", with the theme's `separator`.
pub fn auto_colors(language: Language, detected: ColorSupport, separator: char) -> String {
    let detected = match (language, detected) {
        (Language::English, ColorSupport::TrueColor) => "true colour",
        (Language::English, ColorSupport::Ansi16) => "16 colours",
        (Language::English, ColorSupport::None) => "none, NO_COLOR is set",
        (Language::French, ColorSupport::TrueColor) => "vraies couleurs",
        (Language::French, ColorSupport::Ansi16) => "16 couleurs",
        (Language::French, ColorSupport::None) => "aucune, NO_COLOR est défini",
    };
    format!("Auto {separator} {detected}")
}

/// Note of the options screen while `--lang` is given: the language
/// option is kept but `forced` is used for this session.
pub fn language_forced(language: Language, forced: Language) -> String {
    let name = forced.endonym();
    match language {
        Language::English => format!("--lang is on: {name} for this session."),
        Language::French => format!("--lang est actif{NBSP}: {name} pour cette session."),
    }
}

/// Note of the options screen while `--keyboard` is given: the keyboard
/// option is kept but `forced` is used for this session.
pub fn keyboard_forced(language: Language, forced: Keyboard) -> String {
    let name = forced.name();
    match language {
        Language::English => format!("--keyboard is on: {name} for this session."),
        Language::French => format!("--keyboard est actif{NBSP}: {name} pour cette session."),
    }
}

/// A problem with the save or options file, kept as facts rather than
/// words so that it is told in the language of the screen that shows it,
/// even after the language changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileNotice {
    /// The save could not be read back at launch, for `reason`, and was
    /// renamed `moved_to`.
    SaveSetAside {
        /// Why it could not be read.
        reason: SaveRefusal,
        /// Where it now is.
        moved_to: String,
    },
    /// The save at `path` could not be deleted to start a new game, for
    /// the system's reason `cause`.
    SaveNotDeleted {
        /// Path of the save.
        path: String,
        /// The system's error.
        cause: String,
    },
    /// The save of a finished game, at `path`, could still not be deleted
    /// on the way back to the title, for the system's reason `cause`.
    FinishedSaveNotDeleted {
        /// Path of the save.
        path: String,
        /// The system's error.
        cause: String,
    },
    /// The options file could not be read back at launch.
    OptionsUnreadable(OptionsProblem),
    /// The options could not be written to `path`, for the system's reason
    /// `cause`.
    OptionsNotSaved {
        /// Path of the options file.
        path: String,
        /// The system's error.
        cause: String,
    },
}

/// The notice of the title or options screen about `notice`, in
/// `language`.
///
/// Every quoted part is bounded: a long reason or cause is cut (see
/// [`MAX_REASON_CHARS`]), and so is the start of a long path (see
/// [`bounded_path`]), so a notice fits the 5 rows the title screen gives
/// its notices; a save's reason is shown whole, its quotes already cut, and
/// the outcome comes first where a path could be cut.
pub fn file_notice(language: Language, notice: &FileNotice) -> String {
    match notice {
        FileNotice::SaveSetAside { reason, moved_to } => {
            let (reason, moved_to) = (save_refusal(language, reason), bounded_path(moved_to));
            match language {
                Language::English => format!(
                    "Your saved game could not be read: {reason}. It was kept as {moved_to}."
                ),
                Language::French => format!(
                    "Votre partie sauvegardée n'a pas pu être lue{NBSP}: {reason}. \
                     Elle a été conservée sous {moved_to}."
                ),
            }
        }
        FileNotice::SaveNotDeleted { path, cause } => {
            let (path, cause) = (bounded_path(path), bounded_reason(cause));
            match language {
                Language::English => format!(
                    "No new game was started: the save {path} could not be deleted ({cause}). \
                     Check that it is a file you can delete."
                ),
                Language::French => format!(
                    "Aucune partie n'a été lancée{NBSP}: la sauvegarde {path} n'a pas pu être \
                     supprimée ({cause}). Vérifiez que vous pouvez supprimer ce fichier."
                ),
            }
        }
        FileNotice::FinishedSaveNotDeleted { path, cause } => {
            let (path, cause) = (bounded_path(path), bounded_reason(cause));
            match language {
                Language::English => format!(
                    "The save {path} of the finished game could not be deleted ({cause}). \
                     Delete it by hand so it is not resumed."
                ),
                Language::French => format!(
                    "La sauvegarde {path} de la partie terminée n'a pas pu être supprimée \
                     ({cause}). Supprimez-la à la main pour qu'elle ne soit pas reprise."
                ),
            }
        }
        FileNotice::OptionsUnreadable(problem) => {
            let reason = bounded_reason(&options_problem(language, problem));
            match language {
                Language::English => format!(
                    "The options file could not be read ({reason}). The default options \
                     are in use; changing one rewrites the file."
                ),
                Language::French => format!(
                    "Le fichier d'options n'a pas pu être lu ({reason}). Les options par \
                     défaut s'appliquent{NBSP}; en changer une réécrit le fichier."
                ),
            }
        }
        FileNotice::OptionsNotSaved { path, cause } => {
            // What happens comes before the path, which may be long and is
            // the first thing a screen too short for the notice would cut.
            let (path, cause) = (bounded_path(path), bounded_reason(cause));
            match language {
                Language::English => format!(
                    "the options could not be saved ({cause}). The options apply until \
                     you quit; check that this folder can be written to: {path}"
                ),
                Language::French => format!(
                    "les options n'ont pas pu être enregistrées ({cause}). Elles \
                     s'appliquent jusqu'à ce que vous quittiez{NBSP}; vérifiez que ce \
                     dossier est accessible en écriture{NBSP}: {path}"
                ),
            }
        }
    }
}

/// The message of the log that opens the next new game about `notice`: a
/// save set aside says that a new game begins and where the old save was
/// kept (see [`save_set_aside`]); any other notice is the title's own
/// sentence, as a danger.
pub fn opening(language: Language, notice: &FileNotice) -> Message {
    match notice {
        FileNotice::SaveSetAside { reason, moved_to } => save_set_aside(language, reason, moved_to),
        other => Message::new(Tone::Danger, file_notice(language, other)),
    }
}

/// Why the save could not be read back, as the player reads it: for a save
/// the reader refused, its error cause first, with only what it quotes from
/// the file (an identifier, the words of the reader) cut at
/// [`MAX_QUOTED_CHARS`], and every control character shown as
/// [`CONTROL_MARK`].
///
/// The sentence around a quote is the game's own and always shown whole,
/// such as "the save references monster `dragon`, absent from the content".
/// The detail of an inconsistent save is the core's own diagnosis, quoted
/// as it gives it, like the reader's words.
pub fn save_refusal(language: Language, reason: &SaveRefusal) -> String {
    match (reason, language) {
        (SaveRefusal::NotText, _) => labels(language).save_not_text.to_owned(),
        (SaveRefusal::TooLarge { limit }, Language::English) => {
            format!("the save is larger than {}", file_size(language, *limit))
        }
        (SaveRefusal::TooLarge { limit }, Language::French) => {
            format!("la sauvegarde dépasse {}", file_size(language, *limit))
        }
        (SaveRefusal::Refused(error), _) => save_error(language, error),
    }
}

/// The sentence of [`save_refusal`] for an `error` of the save reader.
fn save_error(language: Language, error: &SaveError) -> String {
    match (error, language) {
        (
            SaveError::Malformed {
                line,
                column,
                message,
            },
            Language::English,
        ) => format!(
            "the save is unreadable at line {line}, column {column}: {}",
            quoted(message)
        ),
        (
            SaveError::Malformed {
                line,
                column,
                message,
            },
            Language::French,
        ) => format!(
            "la sauvegarde est illisible ligne {line}, colonne {column}{NBSP}: {}",
            quoted(message)
        ),
        (SaveError::UnknownReference { kind, id }, Language::English) => format!(
            "the save references {kind} `{}`, absent from the content",
            quoted(id)
        ),
        (SaveError::UnknownReference { kind, id }, Language::French) => {
            let kind = match kind {
                Reference::Monster => "le monstre",
                Reference::Item => "l'objet",
                Reference::Sigil => "le sigille",
            };
            format!(
                "la sauvegarde mentionne {kind} `{}`, absent du contenu",
                quoted(id)
            )
        }
        (SaveError::UnsupportedVersion { found, expected }, Language::French) => format!(
            "la sauvegarde est au format {found}, mais ce jeu ne lit que le format {expected}"
        ),
        (SaveError::Inconsistent(detail), Language::French) => {
            format!("{} (sauvegarde incohérente)", printable(detail))
        }
        (SaveError::Write(detail), Language::French) => format!(
            "la partie n'a pas pu être écrite en sauvegarde ({})",
            printable(detail)
        ),
        (
            SaveError::UnsupportedVersion { .. } | SaveError::Inconsistent(_) | SaveError::Write(_),
            Language::English,
        ) => printable(&error.to_string()),
    }
}

/// Why the options file could not be read back, in the words of the file
/// rather than of the program, such as "line 1: unknown value `Braille`
/// for glyphs (Unicode or Ascii)".
pub fn options_problem(language: Language, problem: &OptionsProblem) -> String {
    match language {
        Language::English => english_options_problem(problem),
        Language::French => french_options_problem(problem),
    }
}

/// [`options_problem`] in English.
fn english_options_problem(problem: &OptionsProblem) -> String {
    use OptionsProblem as P;
    match problem {
        P::System(error) => error.clone(),
        P::TooLarge { limit } => {
            format!("it is larger than {}", file_size(Language::English, *limit))
        }
        P::NotText => "it is not text".to_owned(),
        P::Empty => "it is empty".to_owned(),
        P::OtherVersion { found } => format!(
            "it has format version {found}, but this game reads version {OPTIONS_FORMAT_VERSION}"
        ),
        P::UnknownValue {
            line,
            found,
            setting: Some((key, expected)),
        } => format!(
            "line {line}: unknown value `{found}` for {key} ({})",
            one_of(expected, "or")
        ),
        P::UnknownValue { line, found, .. } => format!("line {line}: unknown value `{found}`"),
        P::MissingVersion => "the format version is missing".to_owned(),
        P::MissingSetting { key } => format!("the `{key}` setting is missing"),
        P::RepeatedVersion { line } => format!("line {line}: the format version is given twice"),
        P::RepeatedSetting { line, key } => {
            format!("line {line}: the `{key}` setting is given twice")
        }
        P::VersionNotWhole { line } => {
            format!("line {line}: the format version must be a whole number")
        }
        P::WrongKind {
            line,
            key: Some(key),
            ..
        } => format!("line {line}: wrong kind of value for {key}"),
        P::WrongKind { line, column, .. } => {
            format!("line {line}, column {column}: wrong kind of value")
        }
        P::NotInParentheses { line } => format!("line {line}: the settings must be inside ( and )"),
        P::EndsEarly { line } => format!("line {line}: the file ends too early"),
        P::CommaMissing { line, column } => {
            format!("line {line}, column {column}: a comma is missing")
        }
        P::ColonMissing { line, column } => {
            format!("line {line}, column {column}: a colon is missing")
        }
        P::ClosingMissing { line, column } => {
            format!("line {line}, column {column}: a closing ) is missing")
        }
        P::TextAfterEnd { line, column } => {
            format!("line {line}, column {column}: unexpected text after the closing )")
        }
        P::UnexpectedCharacter {
            line,
            column,
            character,
        } => format!("line {line}, column {column}: unexpected character `{character}`"),
        P::NotRon { line, column } => {
            format!("line {line}, column {column}: this is not valid RON")
        }
    }
}

/// [`options_problem`] in French.
fn french_options_problem(problem: &OptionsProblem) -> String {
    use OptionsProblem as P;
    match problem {
        P::System(error) => error.clone(),
        P::TooLarge { limit } => format!("il dépasse {}", file_size(Language::French, *limit)),
        P::NotText => "ce n'est pas un texte".to_owned(),
        P::Empty => "il est vide".to_owned(),
        P::OtherVersion { found } => format!(
            "il est au format {found}, mais ce jeu ne lit que le format {OPTIONS_FORMAT_VERSION}"
        ),
        P::UnknownValue {
            line,
            found,
            setting: Some((key, expected)),
        } => format!(
            "ligne {line}{NBSP}: valeur `{found}` inconnue pour {key} ({})",
            one_of(expected, "ou")
        ),
        P::UnknownValue { line, found, .. } => {
            format!("ligne {line}{NBSP}: valeur `{found}` inconnue")
        }
        P::MissingVersion => "la version du format manque".to_owned(),
        P::MissingSetting { key } => format!("le réglage `{key}` manque"),
        P::RepeatedVersion { line } => {
            format!("ligne {line}{NBSP}: la version du format est donnée deux fois")
        }
        P::RepeatedSetting { line, key } => {
            format!("ligne {line}{NBSP}: le réglage `{key}` est donné deux fois")
        }
        P::VersionNotWhole { line } => {
            format!("ligne {line}{NBSP}: la version du format doit être un nombre entier")
        }
        P::WrongKind {
            line,
            key: Some(key),
            ..
        } => format!("ligne {line}{NBSP}: mauvais type de valeur pour {key}"),
        P::WrongKind { line, column, .. } => {
            format!("ligne {line}, colonne {column}{NBSP}: mauvais type de valeur")
        }
        P::NotInParentheses { line } => {
            format!("ligne {line}{NBSP}: les réglages doivent être entre ( et )")
        }
        P::EndsEarly { line } => format!("ligne {line}{NBSP}: le fichier s'arrête trop tôt"),
        P::CommaMissing { line, column } => {
            format!("ligne {line}, colonne {column}{NBSP}: il manque une virgule")
        }
        P::ColonMissing { line, column } => {
            format!("ligne {line}, colonne {column}{NBSP}: il manque un deux-points")
        }
        P::ClosingMissing { line, column } => {
            format!("ligne {line}, colonne {column}{NBSP}: il manque une ) fermante")
        }
        P::TextAfterEnd { line, column } => {
            format!("ligne {line}, colonne {column}{NBSP}: texte inattendu après la ) fermante")
        }
        P::UnexpectedCharacter {
            line,
            column,
            character,
        } => format!("ligne {line}, colonne {column}{NBSP}: caractère `{character}` inattendu"),
        P::NotRon { line, column } => {
            format!("ligne {line}, colonne {column}{NBSP}: ce n'est pas du RON valide")
        }
    }
}

/// The accepted `values` as a reason lists them, the last after `or`:
/// "Unicode or Ascii", "Auto, Ansi16 or Monochrome".
fn one_of(values: &[String], or: &str) -> String {
    match values {
        [] => String::new(),
        [only] => only.clone(),
        [first @ .., last] => format!("{} {or} {last}", first.join(", ")),
    }
}

/// `bytes` as a reason names a size: in whole mebibytes, such as "4 MiB"
/// ("4 Mio" in French), for the limits of the files the game reads.
pub fn file_size(language: Language, bytes: u64) -> String {
    /// Bytes in a mebibyte.
    const MEBIBYTE: u64 = 1024 * 1024;
    let unit = match language {
        Language::English => "MiB",
        Language::French => "Mio",
    };
    format!("{} {unit}", bytes / MEBIBYTE)
}

/// Why the launch was refused: a game in progress is saved at `path`, and
/// the command line asked for `wanted`, a new game, with `option`
/// (`--seed` or `--daily`). Says how to continue or abandon the saved game.
/// Written on the error output before the terminal is touched.
pub fn launch_refused(
    language: Language,
    path: &str,
    option: &str,
    wanted: &LaunchWanted,
) -> String {
    let path = printable(path);
    match language {
        Language::English => {
            let wanted = match wanted {
                LaunchWanted::Daily(date) => format!("the daily dungeon of {date}"),
                LaunchWanted::Seed(seed) => format!("the seed {seed}"),
            };
            format!(
                "a game in progress is saved in {path}: start the game without {option} to \
                 continue it, or to abandon it with New game, Seed or Daily on the title \
                 screen; you can also delete that file to abandon it and play {wanted}"
            )
        }
        Language::French => {
            let wanted = match wanted {
                LaunchWanted::Daily(date) => format!("le donjon du jour du {date}"),
                LaunchWanted::Seed(seed) => format!("la graine {seed}"),
            };
            format!(
                "une partie en cours est sauvegardée dans {path}{NBSP}: lancez le jeu sans \
                 {option} pour la continuer, ou pour l'abandonner avec Nouvelle partie, Graine \
                 ou Donjon du jour sur l'écran titre{NBSP}; vous pouvez aussi supprimer ce \
                 fichier pour l'abandonner et jouer {wanted}"
            )
        }
    }
}

/// The new game the command line asked for, as [`launch_refused`] names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchWanted {
    /// The daily dungeon of this UTC date (`--daily`).
    Daily(DailyDate),
    /// A game of this seed (`--seed`).
    Seed(Seed),
}

/// The lines the inventory shows while the player carries nothing: that
/// the pack is empty and how to pick an item up. `{glyph}` stands for the
/// map glyph of a vial, the most common item.
pub fn empty_inventory(language: Language) -> [String; 3] {
    match language {
        Language::English => [
            "You carry nothing yet.".to_owned(),
            "Stand on an item, {glyph} on the map, and press".to_owned(),
            format!("{PICK_UP_KEY} to pick it up."),
        ],
        Language::French => [
            "Vous ne portez rien pour l'instant.".to_owned(),
            "Placez-vous sur un objet, {glyph} sur la carte, et".to_owned(),
            format!("appuyez sur {PICK_UP_KEY} pour le ramasser."),
        ],
    }
}

/// The first message of every run, so the log is never empty and tells
/// where the player stands.
pub fn welcome(language: Language) -> Message {
    let text = match language {
        Language::English => "You enter the dungeon.",
        Language::French => "Vous entrez dans le donjon.",
    };
    Message::new(Tone::Info, text)
}

/// The first message of a new run whose player starts knowing `sigil`, in
/// place of [`welcome`]: where the player stands, the sigil ready to cast
/// and the key that opens the trace grid ([`TRACE_KEY`], the same on every
/// layout), so a new player knows at once that a spell is ready and how to
/// cast it.
///
/// "You enter the dungeon knowing the sigil Ember: press t to trace it." /
/// "Vous entrez dans le donjon avec le sigille Braise : t pour le tracer."
/// ([`Tone::Sigil`]). One line of the log with the shipped sigils, so a
/// notice about the save that opens the log (4 lines at most, see
/// [`save_set_aside`]) still shows whole above it in the 5 rows of the log.
pub fn welcome_with_sigil(language: Language, sigil: &SigilDef) -> Message {
    let name = sigil_def_name(language, sigil);
    let text = match language {
        Language::English => {
            format!(
                "You enter the dungeon knowing the sigil {name}: press {TRACE_KEY} to trace it."
            )
        }
        Language::French => format!(
            "Vous entrez dans le donjon avec le sigille {name}{NBSP}: {TRACE_KEY} pour le tracer."
        ),
    };
    Message::new(Tone::Sigil, text)
}

/// The message logged when `letter`, a movement letter typed in upper case,
/// did nothing on the map (`input::capital_movement_letter`): Caps Lock is
/// the likely cause. "Z does nothing: Caps Lock on?" / "Z ne fait rien :
/// Verr Maj activé ?" ([`Tone::Info`]: no time passed).
pub fn caps_lock_hint(language: Language, letter: char) -> Message {
    let text = match language {
        Language::English => format!("{letter} does nothing: Caps Lock on?"),
        Language::French => format!("{letter} ne fait rien{NBSP}: Verr Maj activé{NBSP}?"),
    };
    Message::new(Tone::Info, text)
}

/// The first message of a run resumed from its save, in place of
/// [`welcome`], so the player knows the game went on where it stopped.
pub fn welcome_back(language: Language) -> Message {
    let text = match language {
        Language::English => "Welcome back. The dungeon is as you left it.",
        Language::French => "Bon retour. Le donjon est tel que vous l'avez laissé.",
    };
    Message::new(Tone::Info, text)
}

/// The message that opens a new run when the save could not be read back:
/// `reason` is why, and `moved_to` the path the save was renamed to, where
/// it can still be looked at.
///
/// The path comes last and the start of a path longer than a line is cut
/// (see [`bounded_path`]); the reason is shown whole, its quotes already
/// cut: the message takes at most 4 lines, so it shows whole above the
/// welcome line in the 5 rows of the log of an 80 × 24 screen, with the
/// name of the file kept aside.
pub fn save_set_aside(language: Language, reason: &SaveRefusal, moved_to: &str) -> Message {
    let (reason, moved_to) = (save_refusal(language, reason), bounded_path(moved_to));
    let text = match language {
        Language::English => format!(
            "Your saved game could not be read: {reason}. A new game begins; \
             the old save was kept as {moved_to}."
        ),
        Language::French => format!(
            "Sauvegarde illisible{NBSP}: {reason}. Nouvelle partie{NBSP}; \
             ancienne gardée sous {moved_to}."
        ),
    };
    Message::new(Tone::Danger, text)
}

/// The message logged when the save of a finished game (death or victory)
/// could not be deleted: deleting it is tried again when the player quits,
/// and a failure then is reported on leaving.
pub fn save_not_deleted(language: Language) -> Message {
    let text = match language {
        Language::English => {
            "The save of this character could not be deleted; it will be tried again on quitting."
        }
        Language::French => {
            "La sauvegarde de ce personnage n'a pas pu être supprimée\u{a0}; \
             nouvel essai en quittant."
        }
    };
    Message::new(Tone::Danger, text)
}

/// The message logged when automatic exploration stops, or does not start,
/// for `halt`, naming monsters and items from `content`; `None` when the
/// log already says why ([`Halt::Explored`]: the line of
/// [`Event::NothingToExplore`] from [`describe`]).
///
/// In English:
/// - a monster in view when the key is pressed: "You cannot explore with
///   the ossuary rat in view." ([`Tone::Info`]: nothing happened);
/// - a monster coming into view: "An ossuary rat comes into view."
///   ([`Tone::Danger`]);
/// - an item coming into view: "You spot an essence phial." ([`Tone::Info`]);
/// - an unread glyph coming into view: "You spot a carved glyph you have
///   not read." ([`Tone::Sigil`]);
/// - hit points lost: "You are hurt and stop exploring." ([`Tone::Danger`]);
/// - the way held: "Something stands in your way; you stop exploring.";
/// - the step limit: "You stop exploring after 200 steps."
///   ([`MAX_EXPLORE_STEPS`]);
/// - a key pressed: "You stop exploring." (all three [`Tone::Info`]).
pub fn exploration_halted(language: Language, halt: &Halt, content: &Content) -> Option<Message> {
    let french = language == Language::French;
    let (tone, text) = match halt {
        Halt::Explored => return None,
        Halt::FoeInView(definition) => {
            let foe = Who::monster(content, definition);
            let text = if french {
                format!(
                    "Impossible d'explorer avec {} en vue.",
                    foe.definite(language)
                )
            } else {
                format!(
                    "You cannot explore with {} in view.",
                    foe.definite(language)
                )
            };
            (Tone::Info, text)
        }
        Halt::FoeAppeared(definition) => {
            let foe = capitalized(&Who::monster(content, definition).indefinite(language));
            let text = if french {
                format!("{foe} apparaît.")
            } else {
                format!("{foe} comes into view.")
            };
            (Tone::Danger, text)
        }
        Halt::ItemAppeared(definition) => {
            let item = Who::item(content, definition).indefinite(language);
            let text = if french {
                format!("Vous repérez {item}.")
            } else {
                format!("You spot {item}.")
            };
            (Tone::Info, text)
        }
        Halt::GlyphAppeared => (
            Tone::Sigil,
            if french {
                "Vous repérez un glyphe gravé que vous n'avez pas lu."
            } else {
                "You spot a carved glyph you have not read."
            }
            .to_owned(),
        ),
        Halt::Hurt => (
            Tone::Danger,
            if french {
                "Une blessure interrompt votre exploration."
            } else {
                "You are hurt and stop exploring."
            }
            .to_owned(),
        ),
        Halt::Blocked => (
            Tone::Info,
            if french {
                "Quelque chose vous barre la route\u{a0}; vous cessez d'explorer."
            } else {
                "Something stands in your way; you stop exploring."
            }
            .to_owned(),
        ),
        Halt::StepLimit => (
            Tone::Info,
            if french {
                format!("Vous cessez d'explorer après {MAX_EXPLORE_STEPS}\u{a0}pas.")
            } else {
                format!("You stop exploring after {MAX_EXPLORE_STEPS} steps.")
            },
        ),
        Halt::Interrupted => (
            Tone::Info,
            if french {
                "Vous cessez d'explorer."
            } else {
                "You stop exploring."
            }
            .to_owned(),
        ),
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
/// messages the player reads in `language`, in the same order.
///
/// `game` is the game after the call, so it still knows every actor that
/// survived; one that died during the call is named from its
/// [`Event::Died`], which the events always carry. Monster and sigil names
/// come from `content`.
///
/// What is written, in English (the French sentences say the same, with
/// articles and agreements of their own: "Braise, tracé pur : l'ombre est
/// brûlée (6).", "Vous buvez le philtre de soin et récupérez 8 PV (30/30)."):
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
/// - an item picked up: "You pick up the healing draught." ([`Tone::Success`]);
///   one dropped: "You drop the healing draught." ([`Tone::Info`]);
/// - an item used and its effect, on one [`Tone::Success`] line with the
///   verb of its family (drink a vial, read a page, use a catalyst) and,
///   for a restoration, the value reached out of its maximum, so a capped
///   heal reads as such: "You drink the healing draught and recover 8 HP
///   (30/30).", "You drink the healing draught, but you were not hurt
///   (30/30).", "You drink the essence phial and recover 4 essence (6/6).",
///   "You read the page of the Veil: Veil joins your grimoire.", "You use
///   the steadying ink: your next 2 unstable casts will be cast pure.";
/// - an action on an item that the game refused, which took no time
///   ([`Tone::Info`]): "There is nothing here to pick up.", "Your pack is
///   full (10/10): the healing draught stays on the floor.", and the
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
pub fn describe(
    language: Language,
    events: &[Event],
    game: &Game,
    content: &Content,
) -> Vec<Message> {
    let french = language == Language::French;
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
                attacker.zip(target).map(|(attacker, target)| {
                    blow(language, &attacker, &target, *damage, lethal, content)
                })
            }
            Event::Died { actor, who, .. } if !killed.contains(actor) => {
                Some(death(language, who, content))
            }
            Event::SigilCast {
                sigil: Some(id),
                quality,
            } => {
                cast_lead = Some(format!(
                    "{}, {}",
                    sigil_name(language, content, id),
                    quality_words(language, *quality)
                ));
                None
            }
            // An unknown pattern has a line of its own, which says why.
            Event::SigilCast { sigil: None, .. } if events.contains(&Event::UnknownPattern) => None,
            Event::SigilCast { sigil: None, .. } => Some(Message::new(
                Tone::Sigil,
                if french {
                    "Le tracé ne correspond à aucun sigille connu et se dissipe."
                } else {
                    "The trace matches no sigil you know and fizzles."
                },
            )),
            Event::UnknownPattern => Some(Message::new(
                Tone::Sigil,
                if french {
                    "Ce motif n'est pas dans votre grimoire, et le tracé se dissipe. \
                     Les sigilles s'apprennent en lisant les glyphes gravés dans le sol."
                } else {
                    "That pattern is not in your grimoire, and the trace fizzles. \
                     Sigils are learnt by reading the glyphs carved in the floor."
                },
            )),
            Event::Wounded {
                actor,
                damage,
                hp_left,
            } => {
                if *actor == game.player() {
                    // The game spares the last hit point: a scorch at 1 HP
                    // takes nothing, which "(0)" would misreport.
                    let text = match (*damage > 0, french) {
                        (true, false) => format!("The backlash scorches you ({damage})."),
                        (true, true) => format!("Le contrecoup vous brûle ({damage})."),
                        (false, false) => "The backlash scorches you, but you hold on.".to_owned(),
                        (false, true) => {
                            "Le contrecoup vous brûle, mais vous tenez bon.".to_owned()
                        }
                    };
                    Some(Message::new(Tone::Danger, text))
                } else {
                    let lethal = *hp_left <= 0;
                    if lethal {
                        killed.insert(*actor);
                    }
                    combatant(*actor, events, game).map(|target| {
                        let who = Who::of(&target, content);
                        let effect = match language {
                            Language::English => {
                                let verb = if lethal { "destroyed" } else { "seared" };
                                format!("{} is {verb} ({damage})", who.definite(language))
                            }
                            Language::French => {
                                let verb = if lethal {
                                    who.agreed("détruit", "détruite")
                                } else {
                                    who.agreed("brûlé", "brûlée")
                                };
                                format!("{} est {verb} ({damage})", who.definite(language))
                            }
                        };
                        spell_line(language, cast_lead.take(), &effect)
                    })
                }
            }
            Event::Healed { actor, amount, hp } if use_lead.is_some() => {
                let max = game.actor(*actor).map_or(*hp, |actor| actor.max_hp());
                let lead = use_lead.take().unwrap_or_default();
                Some(Message::new(
                    Tone::Success,
                    restored_line(language, &lead, *amount, Restored::Health, *hp, max),
                ))
            }
            Event::Healed { amount, .. } => {
                let effect = match (*amount > 0, french) {
                    (true, false) => format!("you mend ({amount})"),
                    (true, true) => format!("vous guérissez ({amount})"),
                    (false, false) => "you were not hurt".to_owned(),
                    (false, true) => "aucune blessure à guérir".to_owned(),
                };
                Some(spell_line(language, cast_lead.take(), &effect))
            }
            Event::Held { actor, turns } if *actor == game.player() => {
                let lost = turn_count(language, *turns);
                let text = if french {
                    format!("Le contrecoup vous étourdit{NBSP}: vous perdez {lost}.")
                } else {
                    format!("The backlash dazes you: you lose {lost}.")
                };
                Some(Message::new(Tone::Danger, text))
            }
            Event::Held { actor, turns } => combatant(*actor, events, game).map(|target| {
                let who = Who::of(&target, content);
                let turns = turn_count(language, *turns);
                let effect = match language {
                    Language::English => format!("{} is held ({turns})", who.definite(language)),
                    Language::French => format!(
                        "{} est {} ({turns})",
                        who.definite(language),
                        who.agreed("immobilisé", "immobilisée")
                    ),
                };
                spell_line(language, cast_lead.take(), &effect)
            }),
            Event::Blinked { from, to, .. } => {
                let cells = from.x.abs_diff(to.x).max(from.y.abs_diff(to.y));
                let effect = match (cells, french) {
                    (0, false) => "the way is blocked".to_owned(),
                    (0, true) => "le passage est bloqué".to_owned(),
                    (1, false) => "you blink 1 cell".to_owned(),
                    (1, true) => "vous franchissez 1\u{a0}case".to_owned(),
                    (cells, false) => format!("you blink {cells} cells"),
                    (cells, true) => format!("vous franchissez {cells}{NBSP}cases"),
                };
                Some(spell_line(language, cast_lead.take(), &effect))
            }
            Event::SpellMissed { .. } => {
                let effect = if french {
                    "aucun ennemi à portée"
                } else {
                    "no foe in reach"
                };
                Some(spell_line(language, cast_lead.take(), effect))
            }
            Event::EssenceDrained { lost, .. } if *lost > 0 => Some(Message::new(
                Tone::Danger,
                if french {
                    format!("Le contrecoup draine votre essence ({lost}).")
                } else {
                    format!("The backlash drains your essence ({lost}).")
                },
            )),
            Event::EssenceDrained { .. } => Some(Message::new(
                Tone::Info,
                if french {
                    "Le contrecoup ne trouve aucune essence à drainer."
                } else {
                    "The backlash finds no essence to drain."
                },
            )),
            Event::NotEnoughEssence { essence, cost } => Some(Message::new(
                Tone::Info,
                if french {
                    format!("Pas assez d'essence pour lancer{NBSP}: {essence} sur {cost}.")
                } else {
                    format!("Not enough essence to cast: {essence} of {cost}.")
                },
            )),
            Event::DirectionMissing { sigil } => {
                let name = sigil_name(language, content, sigil);
                Some(Message::new(
                    Tone::Info,
                    if french {
                        format!("{name} demande une direction.")
                    } else {
                        format!("{name} needs a direction.")
                    },
                ))
            }
            Event::GlyphLearned { sigil, .. } => {
                let name = sigil_name(language, content, sigil);
                Some(Message::new(
                    Tone::Success,
                    if french {
                        format!(
                            "Vous déchiffrez un glyphe gravé{NBSP}: {name} rejoint votre grimoire."
                        )
                    } else {
                        format!("You decipher a carved glyph: {name} joins your grimoire.")
                    },
                ))
            }
            Event::CastSteadied { casts_left, .. } => {
                if let Some(lead) = cast_lead.as_mut() {
                    // The game reports a steadied cast as pure: the line
                    // says what made it so instead.
                    let pure = quality_words(language, CastQuality::Pure);
                    let steadied = if french {
                        format!("tracé stabilisé (encore {casts_left})")
                    } else {
                        format!("steadied trace ({casts_left} left)")
                    };
                    *lead = lead.replace(pure, &steadied);
                }
                None
            }
            Event::PickedUp { definition, .. } => {
                let item = Who::item(content, definition).definite(language);
                Some(Message::new(
                    Tone::Success,
                    if french {
                        format!("Vous ramassez {item}.")
                    } else {
                        format!("You pick up {item}.")
                    },
                ))
            }
            Event::InventoryFull { definition, .. } => {
                let item = Who::item(content, definition).definite(language);
                let carried = capacity(INVENTORY_CAPACITY);
                Some(Message::new(
                    Tone::Info,
                    if french {
                        format!("Votre sac est plein ({carried}){NBSP}: {item} reste au sol.")
                    } else {
                        format!("Your pack is full ({carried}): {item} stays on the floor.")
                    },
                ))
            }
            Event::NothingToPickUp { .. } => Some(Message::new(
                Tone::Info,
                if french {
                    "Il n'y a rien à ramasser ici."
                } else {
                    "There is nothing here to pick up."
                },
            )),
            Event::Used { definition, .. } => {
                let family = content.item(definition).map(ItemDef::family);
                let item = Who::item(content, definition).definite(language);
                use_lead = Some(format!("{} {item}", use_verb(language, family)));
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
                let lead = use_lead.take().unwrap_or_else(|| {
                    if french {
                        "Vous vous reposez"
                    } else {
                        "You rest"
                    }
                    .to_owned()
                });
                Some(Message::new(
                    Tone::Success,
                    restored_line(language, &lead, *amount, Restored::Essence, *essence, max),
                ))
            }
            Event::SigilLearned { sigil } => {
                let lead = use_lead.take().unwrap_or_else(|| {
                    if french { "Vous apprenez" } else { "You learn" }.to_owned()
                });
                let name = sigil_name(language, content, sigil);
                Some(Message::new(
                    Tone::Success,
                    if french {
                        format!("{lead}{NBSP}: {name} rejoint votre grimoire.")
                    } else {
                        format!("{lead}: {name} joins your grimoire.")
                    },
                ))
            }
            Event::Steadied { casts } => {
                let lead = use_lead.take().unwrap_or_else(|| {
                    if french {
                        "Vous vous concentrez"
                    } else {
                        "You steady"
                    }
                    .to_owned()
                });
                let text = match (casts, french) {
                    (1, false) => format!("{lead}: your next unstable cast will be cast pure."),
                    (casts, false) => {
                        format!("{lead}: your next {casts} unstable casts will be cast pure.")
                    }
                    (1, true) => {
                        format!("{lead}{NBSP}: votre prochain lancer instable sera pur.")
                    }
                    (casts, true) => format!(
                        "{lead}{NBSP}: vos {casts}{NBSP}prochains lancers instables seront purs."
                    ),
                };
                Some(Message::new(Tone::Success, text))
            }
            Event::Dropped { definition, .. } => {
                let item = Who::item(content, definition).definite(language);
                Some(Message::new(
                    Tone::Info,
                    if french {
                        format!("Vous posez {item}.")
                    } else {
                        format!("You drop {item}.")
                    },
                ))
            }
            Event::ItemRefused { item, reason } => Some(Message::new(
                Tone::Info,
                item_refusal(language, game, *item, *reason),
            )),
            Event::Descended { depth } if generation::is_bottom_floor(*depth) => {
                Some(Message::new(
                    Tone::Info,
                    if french {
                        format!(
                            "Vous descendez à la profondeur {depth}, la dernière. Le sceau repose ici."
                        )
                    } else {
                        format!("You descend to depth {depth}, the last. The seal lies here.")
                    },
                ))
            }
            Event::Descended { depth } => Some(Message::new(
                Tone::Info,
                if french {
                    format!("Vous descendez à la profondeur {depth}.")
                } else {
                    format!("You descend to depth {depth}.")
                },
            )),
            Event::NoStairsHere { .. } if generation::is_bottom_floor(game.depth()) => {
                Some(Message::new(
                    Tone::Info,
                    if french {
                        "Aucun escalier ne mène plus bas\u{a0}: placez-vous sur le sceau \
                         pour le briser."
                    } else {
                        "No stairs lead deeper: stand on the seal to break it."
                    },
                ))
            }
            Event::NoStairsHere { .. } => Some(Message::new(
                Tone::Info,
                if french {
                    "Il n'y a pas d'escalier ici."
                } else {
                    "There are no stairs here."
                },
            )),
            Event::SealBroken { .. } => Some(Message::new(
                Tone::Success,
                if french {
                    "Vous brisez le sceau. Le donjon se tait."
                } else {
                    "You break the seal. The dungeon falls silent."
                },
            )),
            Event::NothingToExplore { .. } => Some(Message::new(
                Tone::Info,
                if french {
                    "Il ne reste rien à explorer à votre portée."
                } else {
                    "There is nothing left to explore within reach."
                },
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
/// that item.", "You already know Veil: the page of the Veil has nothing
/// to teach.", "Something already lies here: drop the healing draught
/// elsewhere."
///
/// The item is named from `game`, which keeps every item not used up; one
/// it no longer knows is called "that item".
pub fn item_refusal(language: Language, game: &Game, item: ItemId, reason: ItemRefusal) -> String {
    let french = language == Language::French;
    let definition = game.item(item).map(|item| item.definition());
    let name = definition.map_or_else(
        || if french { "cet objet" } else { "that item" }.to_owned(),
        |definition| Who::Item(definition).definite(language),
    );
    match reason {
        ItemRefusal::NotCarried if french => "Vous ne portez pas cet objet.".to_owned(),
        ItemRefusal::NotCarried => "You do not carry that item.".to_owned(),
        ItemRefusal::NothingToLearn => {
            let sigil = definition.and_then(|definition| match definition.effect() {
                ItemEffect::Teach(sigil) => Some(sigil),
                _ => None,
            });
            match language {
                Language::English => format!(
                    "You already know {}: {name} has nothing to teach.",
                    sigil.map_or("its sigil", SigilDef::name)
                ),
                Language::French => format!(
                    "Vous connaissez déjà {}{NBSP}: {name} n'a rien à vous apprendre.",
                    sigil.map_or("son sigille", SigilDef::french_name)
                ),
            }
        }
        ItemRefusal::CellTaken if french => {
            format!("Quelque chose repose déjà ici{NBSP}: posez {name} ailleurs.")
        }
        ItemRefusal::CellTaken => format!("Something already lies here: drop {name} elsewhere."),
    }
}

/// Returns the name shown to the player for the item definition `id` of
/// `content`, such as "healing draught" ("philtre de soin" in French,
/// without its article), in lower case as the content writes it, like the
/// names of the creatures; an identifier missing from `content` is shown as
/// is, as [`monster_name`] does.
pub fn item_name<'name>(language: Language, content: &'name Content, id: &'name str) -> &'name str {
    content.item(id).map_or(id, |item| match language {
        Language::English => item.name(),
        Language::French => item.french_name().noun(),
    })
}

/// Returns the name of `item` at the start of a line of the inventory, its
/// first letter in upper case: "Healing draught", "Encre d'aplomb", "Page
/// of the Veil". Inside a sentence the name keeps the lower case of the
/// content ("You drink the healing draught").
pub fn item_entry_name(language: Language, item: &ItemDef) -> String {
    let name = match language {
        Language::English => item.name(),
        Language::French => item.french_name().noun(),
    };
    capitalized(name)
}

/// Returns what `item` does in a few words, for the inventory: "heal 10
/// HP", "restore 4 essence", "teach Veil", "steady 2 casts".
///
/// A page whose sigil the player already knows (`known`) says so after
/// `separator`, "teach Veil · known", since the game would refuse to use
/// it.
pub fn item_summary(language: Language, item: &ItemDef, known: bool, separator: char) -> String {
    match (language, item.effect()) {
        (Language::English, ItemEffect::Heal(amount)) => format!("heal {amount} HP"),
        (Language::French, ItemEffect::Heal(amount)) => format!("soigne {amount}{NBSP}PV"),
        (Language::English, ItemEffect::RestoreEssence(amount)) => {
            format!("restore {amount} essence")
        }
        (Language::French, ItemEffect::RestoreEssence(amount)) => {
            format!("rend {amount} d'essence")
        }
        (Language::English, ItemEffect::Teach(sigil)) if known => {
            format!("teach {} {separator} known", sigil.name())
        }
        (Language::French, ItemEffect::Teach(sigil)) if known => {
            format!("enseigne {} {separator} connu", sigil.french_name())
        }
        (Language::English, ItemEffect::Teach(sigil)) => format!("teach {}", sigil.name()),
        (Language::French, ItemEffect::Teach(sigil)) => {
            format!("enseigne {}", sigil.french_name())
        }
        (Language::English, ItemEffect::Steady(1)) => "steady 1 cast".to_owned(),
        (Language::English, ItemEffect::Steady(casts)) => format!("steady {casts} casts"),
        (Language::French, ItemEffect::Steady(1)) => "stabilise 1\u{a0}lancer".to_owned(),
        (Language::French, ItemEffect::Steady(casts)) => {
            format!("stabilise {casts}{NBSP}lancers")
        }
    }
}

/// Returns how full the inventory is with `carried` items, out of
/// `item::INVENTORY_CAPACITY`: "3/10".
pub fn capacity(carried: usize) -> String {
    format!("{carried}/{INVENTORY_CAPACITY}")
}

/// Returns the sentence of the death summary that says who killed the
/// player, where and when: "Killed by an acolyte at depth 4, on turn 1283."
/// ("Victime d'un acolyte à la profondeur 4, au tour 1283.", which speaks
/// of the player without a gender).
///
/// `depth` is the floor of the death and `turn` the player's turn it
/// happened on, counted from 1 as on the status line. A death by the
/// player's own hand (`Combatant::Player`, such as its own spell, which no
/// rule causes today) says so instead of naming a monster.
pub fn cause_of_death(
    language: Language,
    killed_by: &Combatant,
    depth: u32,
    turn: u64,
    content: &Content,
) -> String {
    match (killed_by, language) {
        (Combatant::Player, Language::English) => {
            format!("Undone by your own magic at depth {depth}, on turn {turn}.")
        }
        (Combatant::Player, Language::French) => {
            format!("Victime de votre propre magie à la profondeur {depth}, au tour {turn}.")
        }
        (Combatant::Monster { definition }, Language::English) => format!(
            "Killed by {} at depth {depth}, on turn {turn}.",
            Who::monster(content, definition).indefinite(language)
        ),
        (Combatant::Monster { definition }, Language::French) => {
            let killer = match content.monster(definition) {
                Some(monster) => of_indefinite(monster.french_name()),
                None => format!("de {definition}"),
            };
            format!("Victime {killer} à la profondeur {depth}, au tour {turn}.")
        }
    }
}

/// Returns the line under the death summary's title for the run of `seed`,
/// one of the `epitaphs` of [`Labels`]: the same seed always gets the same
/// line, at the same position in every language, so a replayed run ends
/// the same way.
pub fn epitaph(language: Language, seed: Seed) -> &'static str {
    line_of_seed(seed, &labels(language).epitaphs)
}

/// Returns the line under the victory screen's title for the run of
/// `seed`, one of the `victory_lines` of [`Labels`], chosen as
/// [`epitaph`] chooses.
pub fn victory_line(language: Language, seed: Seed) -> &'static str {
    line_of_seed(seed, &labels(language).victory_lines)
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
pub fn victory_outcome(language: Language, depth: u32, turn: u64) -> String {
    match language {
        Language::English => format!("You broke the seal at depth {depth}, on turn {turn}."),
        Language::French => {
            format!("Vous avez brisé le sceau à la profondeur {depth}, au tour {turn}.")
        }
    }
}

/// Returns the sigils cast by quality, as the end screens detail them:
/// "pure 22 · unstable 6 · fizzled 3", joined by `separator`.
pub fn cast_breakdown(language: Language, casts: CastCounts, separator: char) -> String {
    let (pure, unstable, fizzled) = match language {
        Language::English => ("pure", "unstable", "fizzled"),
        Language::French => ("purs", "instables", "ratés"),
    };
    format!(
        "{pure} {} {separator} {unstable} {} {separator} {fizzled} {}",
        casts.pure, casts.unstable, casts.fizzled
    )
}

/// Returns the command that replays `seed`, as the end screens suggest it:
/// "Replay this seed: sigilfall --seed 7F3A-91C2-0B5E-D4A8".
pub fn replay_hint(language: Language, seed: Seed) -> String {
    let command = format!("{} --seed {seed}", env!("CARGO_BIN_NAME"));
    match language {
        Language::English => format!("Replay this seed: {command}"),
        Language::French => format!("Rejouer cette graine{NBSP}: {command}"),
    }
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
/// quality. The line is plain ASCII whatever the glyph option, and in
/// English whatever the language, so it pastes intact anywhere and two
/// players' lines compare as text; its longest realistic form (depth 10,
/// five-digit turns, three-digit kills, two-digit casts, a seed) still fits
/// 80 columns.
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

/// Returns the last line of a list of the side panel, the grimoire or the
/// end screens when `hidden` entries do not fit: "and 3 more".
pub fn more_in_view(language: Language, hidden: usize) -> String {
    match language {
        Language::English => format!("and {hidden} more"),
        Language::French => format!("et {hidden} autres"),
    }
}

/// Returns the name shown to the player for the monster definition
/// `definition` of `content`, such as "ossuary rat" ("rat d'ossuaire" in
/// French, without its article).
///
/// A definition missing from `content` (only possible when a game was built
/// from other content than the one given here) is shown by its identifier,
/// so the screen still says something.
pub fn monster_name<'name>(
    language: Language,
    content: &'name Content,
    definition: &'name str,
) -> &'name str {
    content
        .monster(definition)
        .map_or(definition, |monster| match language {
            Language::English => monster.name(),
            Language::French => monster.french_name().noun(),
        })
}

/// Returns the name of `count` creatures of the definition `definition` of
/// `content`, as the end screens count them: the name for one ("ossuary
/// rat"), the plural for any other count ("ossuary rats", "rats
/// d'ossuaire"); an identifier missing from `content` is shown as is.
pub fn monster_count_name<'name>(
    language: Language,
    content: &'name Content,
    definition: &'name str,
    count: u32,
) -> &'name str {
    match content.monster(definition) {
        Some(monster) if count != 1 => match language {
            Language::English => monster.plural(),
            Language::French => monster.french_plural(),
        },
        _ => monster_name(language, content, definition),
    }
}

/// Returns the name shown to the player for the sigil identified by `id`
/// in `content`, such as "Ember" ("Braise"); an identifier missing from
/// `content` is shown as is, as [`monster_name`] does.
pub fn sigil_name<'name>(
    language: Language,
    content: &'name Content,
    id: &'name str,
) -> &'name str {
    content
        .sigil(id)
        .map_or(id, |sigil| sigil_def_name(language, sigil))
}

/// Returns the name of `sigil` in `language`.
pub fn sigil_def_name(language: Language, sigil: &SigilDef) -> &str {
    match language {
        Language::English => sigil.name(),
        Language::French => sigil.french_name(),
    }
}

/// Returns what `sigil` does in a few words, for the grimoire: its effect
/// and power, then whom it reaches, joined by `separator`, such as
/// "harm 6 · nearest" or "blink 4 · aimed" ("dégât 6 · proche").
///
/// The power's unit follows the effect: hit points to harm or heal, turns
/// to hold, cells to blink. "nearest" is the nearest foe in sight,
/// "aimed" a direction asked after the trace, "self" the player. Each
/// fits the 16 columns the grimoire leaves it.
pub fn effect_summary(language: Language, sigil: &SigilDef, separator: char) -> String {
    let french = language == Language::French;
    let verb = match (sigil.effect(), french) {
        (Effect::Damage, false) => "harm",
        (Effect::Heal, false) => "heal",
        (Effect::Blink, false) => "blink",
        (Effect::Hold, false) => "hold",
        (Effect::Damage, true) => "dégât",
        (Effect::Heal, true) => "soin",
        (Effect::Blink, true) => "saut",
        (Effect::Hold, true) => "gel",
    };
    let target = match (sigil.targeting(), french) {
        (Targeting::Caster, false) => "self",
        (Targeting::Direction, false) => "aimed",
        (Targeting::NearestEnemy, false) => "nearest",
        (Targeting::Caster, true) => "soi",
        (Targeting::Direction, true) => "visé",
        (Targeting::NearestEnemy, true) => "proche",
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
/// node adds, joined by `separator`: "1.25 turns · +0.25 a node" ("1,25
/// tour · +0,25 par nœud", with the French decimal comma and "tour" below
/// 2, as French counts).
///
/// The cost is `schedule::cast_cost`, the energy `Game::submit` charges for
/// the cast; at the player's speed, a step's worth of energy is one turn.
pub fn cast_time(language: Language, nodes: usize, separator: char) -> String {
    let cost = cast_cost(nodes);
    let (whole, stroke) = (turns(language, cost), turns(language, STROKE_COST));
    match language {
        Language::English => format!("{whole} turns {separator} +{stroke} a node"),
        Language::French => {
            let unit = if cost >= 2 * MOVE_COST {
                "tours"
            } else {
                "tour"
            };
            format!("{whole} {unit} {separator} +{stroke} par nœud")
        }
    }
}

/// Returns the notice of the sigil overlay when the player presses the key
/// of `node` while the trace already links it: "Node 5 is already linked."
pub fn node_refused(language: Language, node: Node) -> String {
    match language {
        Language::English => format!("Node {node} is already linked."),
        Language::French => format!("Le nœud {node} est déjà relié."),
    }
}

/// Returns the notice of the sigil overlay while the player has less
/// essence than a cast costs: "Not enough essence: 1 of 2."
pub fn essence_too_low(language: Language, essence: i32, cost: i32) -> String {
    match language {
        Language::English => format!("Not enough essence: {essence} of {cost}."),
        Language::French => format!("Pas assez d'essence{NBSP}: {essence} sur {cost}."),
    }
}

/// Returns what a cast costs in essence, after the player's essence under
/// the grid: " · 2 a cast", the theme's `separator` first.
pub fn essence_per_cast(language: Language, cost: i32, separator: char) -> String {
    match language {
        Language::English => format!(" {separator} {cost} a cast"),
        Language::French => format!(" {separator} {cost} par sort"),
    }
}

/// Returns the title of the sigil overlay while a direction is asked for
/// the sigil named `name`: "Aim Passage".
pub fn aim_title(language: Language, name: &str) -> String {
    match language {
        Language::English => format!("Aim {name}"),
        Language::French => format!("Diriger {name}"),
    }
}

/// Returns the notice of the sigil overlay while a direction is asked for
/// the sigil named `name`: "Passage is aimed: choose a direction."
/// ("Passage : choisissez une direction.")
pub fn aim_prompt(language: Language, name: &str) -> String {
    match language {
        Language::English => format!("{name} is aimed: choose a direction."),
        Language::French => format!("{name}{NBSP}: choisissez une direction."),
    }
}

/// `energy` written in turns of the player, with two decimals, the
/// decimal mark of `language`: 125 is "1.25", or "1,25" in French, one
/// step being [`MOVE_COST`].
fn turns(language: Language, energy: i32) -> String {
    let hundredths = energy * 100 / MOVE_COST;
    let mark = match language {
        Language::English => '.',
        Language::French => ',',
    };
    format!("{}{mark}{:02}", hundredths / 100, hundredths % 100)
}

/// How a trace of `quality` is named in the line of its cast.
const fn quality_words(language: Language, quality: CastQuality) -> &'static str {
    match (language, quality) {
        (Language::English, CastQuality::Pure) => "pure trace",
        (Language::English, CastQuality::Unstable) => "unstable trace",
        (Language::English, CastQuality::Fizzled) => "fizzled trace",
        (Language::French, CastQuality::Pure) => "tracé pur",
        (Language::French, CastQuality::Unstable) => "tracé instable",
        (Language::French, CastQuality::Fizzled) => "tracé raté",
    }
}

/// The opening of the line of using an item of `family`, before its name:
/// a vial is drunk, a page read, a catalyst used ("You drink", "Vous
/// buvez"); an unknown item is used.
const fn use_verb(language: Language, family: Option<ItemFamily>) -> &'static str {
    match (language, family) {
        (Language::English, Some(ItemFamily::Vial)) => "You drink",
        (Language::English, Some(ItemFamily::Page)) => "You read",
        (Language::English, Some(ItemFamily::Catalyst) | None) => "You use",
        (Language::French, Some(ItemFamily::Vial)) => "Vous buvez",
        (Language::French, Some(ItemFamily::Page)) => "Vous lisez",
        (Language::French, Some(ItemFamily::Catalyst) | None) => "Vous utilisez",
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
/// ("You drink the healing draught") and closed by the `value` reached out
/// of `max`, so a capped restoration reads as such: "… and recover 8 HP
/// (30/30)." or, when nothing was missing, "…, but you were not hurt
/// (30/30)."
fn restored_line(
    language: Language,
    lead: &str,
    amount: i32,
    what: Restored,
    value: i32,
    max: i32,
) -> String {
    match (language, what, amount > 0) {
        (Language::English, Restored::Health, true) => {
            format!("{lead} and recover {amount} HP ({value}/{max}).")
        }
        (Language::English, Restored::Essence, true) => {
            format!("{lead} and recover {amount} essence ({value}/{max}).")
        }
        (Language::English, Restored::Health, false) => {
            format!("{lead}, but you were not hurt ({value}/{max}).")
        }
        (Language::English, Restored::Essence, false) => {
            format!("{lead}, but your essence was full ({value}/{max}).")
        }
        (Language::French, Restored::Health, true) => {
            format!("{lead} et récupérez {amount}{NBSP}PV ({value}/{max}).")
        }
        (Language::French, Restored::Essence, true) => {
            format!("{lead} et récupérez {amount} d'essence ({value}/{max}).")
        }
        (Language::French, Restored::Health, false) => {
            format!("{lead}, mais vous n'aviez aucune blessure ({value}/{max}).")
        }
        (Language::French, Restored::Essence, false) => {
            format!("{lead}, mais votre essence était pleine ({value}/{max}).")
        }
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

/// "1 turn" or "3 turns" ("1 tour", "3 tours").
fn turn_count(language: Language, turns: u32) -> String {
    match (language, turns) {
        (Language::English, 1) => "1 turn".to_owned(),
        (Language::English, turns) => format!("{turns} turns"),
        (Language::French, 1) => "1\u{a0}tour".to_owned(),
        (Language::French, turns) => format!("{turns}{NBSP}tours"),
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
fn spell_line(language: Language, lead: Option<String>, effect: &str) -> Message {
    let text = match (lead, language) {
        (Some(lead), Language::English) => format!("{lead}: {effect}."),
        (Some(lead), Language::French) => format!("{lead}{NBSP}: {effect}."),
        (None, _) => format!("{}.", capitalized(effect)),
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
/// the tones; in French the player as object comes before the verb ("L'ombre
/// vous frappe").
fn blow(
    language: Language,
    attacker: &Combatant,
    target: &Combatant,
    damage: i32,
    lethal: bool,
    content: &Content,
) -> Message {
    let tone = match (target, lethal) {
        (Combatant::Player, _) => Tone::Danger,
        (Combatant::Monster { .. }, true) => Tone::Success,
        (Combatant::Monster { .. }, false) => Tone::Info,
    };
    let subject = capitalized(&Who::of(attacker, content).definite(language));
    let object = Who::of(target, content).definite(language);
    let player_strikes = matches!(attacker, Combatant::Player);
    let text = match language {
        Language::English => {
            let verb = match (lethal, player_strikes) {
                (false, true) => "hit",
                (false, false) => "hits",
                (true, true) => "kill",
                (true, false) => "kills",
            };
            format!("{subject} {verb} {object} ({damage}).")
        }
        Language::French => {
            let verb = match (lethal, player_strikes) {
                (false, true) => "frappez",
                (false, false) => "frappe",
                (true, true) => "tuez",
                (true, false) => "tue",
            };
            if matches!(target, Combatant::Player) {
                format!("{subject} vous {verb} ({damage}).")
            } else {
                format!("{subject} {verb} {object} ({damage}).")
            }
        }
    };
    Message::new(tone, text)
}

/// The message of the death of `who` when no killing blow announced it.
fn death(language: Language, who: &Combatant, content: &Content) -> Message {
    match (who, language) {
        (Combatant::Player, Language::English) => Message::new(Tone::Danger, "You die."),
        (Combatant::Player, Language::French) => Message::new(Tone::Danger, "Vous mourez."),
        (Combatant::Monster { .. }, _) => {
            let subject = capitalized(&Who::of(who, content).definite(language));
            let verb = match language {
                Language::English => "dies",
                Language::French => "meurt",
            };
            Message::new(Tone::Success, format!("{subject} {verb}."))
        }
    }
}
