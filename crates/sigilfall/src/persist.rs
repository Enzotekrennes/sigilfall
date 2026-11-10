//! Files the game keeps between runs: where they live, the save of the
//! game in progress and the player's options.
//!
//! Exposes [`SaveFile`], the save file of one data folder, which reads,
//! writes and deletes the save; [`Loaded`], what reading it found;
//! [`OptionsFile`], the options file beside the save, and
//! [`LoadedOptions`], what reading it found; and [`DATA_DIR_VAR`], the
//! environment variable that moves the data folder.
//!
//! The data folder is the operating system's folder for application data,
//! found by `directories::ProjectDirs` with no qualifier, no organization
//! and the application name `Sigilfall`:
//!
//! - Windows: `%APPDATA%\Sigilfall\data`, such as
//!   `C:\Users\Name\AppData\Roaming\Sigilfall\data`;
//! - Linux: `$XDG_DATA_HOME/sigilfall`, by default
//!   `~/.local/share/sigilfall`;
//! - macOS: `~/Library/Application Support/Sigilfall`.
//!
//! The save is `save.ron` in that folder. It is written to `save.ron.tmp`
//! first, then renamed over `save.ron`, so a run interrupted while writing
//! leaves either the previous save or the new one, never half of one; a
//! leftover `save.ron.tmp` is never read, and is deleted rather than opened
//! before the next write, so a link put in its place is never followed. A
//! save that cannot be read back (not RON, another format version, content
//! that changed, larger than [`MAX_FILE_BYTES`]…) is renamed `save.ron.bad`
//! rather than deleted, so the run it held can still be looked at.
//!
//! The options are `options.ron` in the same folder, a short RON record
//! with its own format version ([`OPTIONS_FORMAT_VERSION`]), written the
//! same atomic way. An options file that cannot be read back never stops
//! the game: the default options are used, the reason is reported, and the
//! file is replaced the next time an option changes.
//!
//! The text of a save and its checks belong to `sigilfall_core::save`.
//! Why a file could not be read is reported as facts ([`SaveRefusal`],
//! [`OptionsProblem`]), which `messages` words in the player's language;
//! when to save, resume or delete, and when to write the options, is
//! decided by the `app` module.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use sigilfall_core::content::Content;
use sigilfall_core::game::Game;
use sigilfall_core::save::{self, SaveError};

use crate::keyboard::Keyboard;
use crate::language::Language;
use crate::theme::{Appearance, ColorChoice, Defaults, GlyphSet, Options};

/// Environment variable that replaces the operating system's data folder,
/// as a path to a folder, created if missing; an empty value is ignored.
///
/// An environment variable rather than a command-line option: the command
/// line keeps the options a player needs (`--seed`, `--daily`, `--ascii`),
/// a script or a test can move the folder of a game without changing its
/// arguments, and the options live in the same folder, so one variable
/// moves every file of the game.
pub const DATA_DIR_VAR: &str = "SIGILFALL_DATA_DIR";

/// Application name given to `directories::ProjectDirs`, which makes the
/// last part of the data folder's path (lowercased on Linux).
pub(crate) const APPLICATION: &str = "Sigilfall";

/// Largest save or options file read, in bytes: 4 MiB.
///
/// A save of the bottom floor, the largest state the game reaches, takes
/// tens of kilobytes, and an options file less than one; a file a hundred
/// times larger than any save is not one the game wrote, and reading it
/// whole would only spend memory before refusing it.
pub const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;

/// Name of the save file in the data folder.
const SAVE_NAME: &str = "save.ron";

/// Name of the options file in the data folder.
const OPTIONS_NAME: &str = "options.ron";

/// Version of the options file's format, written at its top.
///
/// Raised whenever a field is renamed, removed or given a new meaning, so
/// an options file written by another version of the game is recognised
/// and replaced by the defaults instead of being misread.
pub const OPTIONS_FORMAT_VERSION: u32 = 1;

/// Suffix of the file a save is written to before it replaces the save.
const TEMPORARY_SUFFIX: &str = ".tmp";

/// Suffix given to a save that could not be read back, when it is set
/// aside.
const SET_ASIDE_SUFFIX: &str = ".bad";

/// The save file of one data folder: `save.ron` in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveFile {
    /// Path of `save.ron`.
    path: PathBuf,
}

/// What [`SaveFile::load`] found.
#[derive(Debug)]
pub enum Loaded {
    /// There is no save: the last run ended in death, or none was played.
    Missing,
    /// The save holds this game, ready to go on where it stopped.
    Game(Box<Game>),
    /// The save could not be read back and was renamed so it is never read
    /// again; a new game is to start.
    SetAside {
        /// Why it could not be read (`messages::save_refusal` words it).
        reason: SaveRefusal,
        /// Where the unreadable save now is.
        moved_to: PathBuf,
    },
}

/// Why a save could not be read back, as facts the player is told in
/// their language (`messages::save_refusal`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveRefusal {
    /// The file is not UTF-8 text.
    NotText,
    /// The file is larger than `limit` bytes, and was not read.
    TooLarge {
        /// The largest file read, [`MAX_FILE_BYTES`].
        limit: u64,
    },
    /// The save reader refused the text.
    Refused(SaveError),
}

impl SaveFile {
    /// The save file in `folder`, which need not exist yet: it is created
    /// by the first [`SaveFile::write`].
    pub fn in_folder(folder: impl Into<PathBuf>) -> Self {
        Self {
            path: folder.into().join(SAVE_NAME),
        }
    }

    /// The save file of this player: in the folder named by
    /// [`DATA_DIR_VAR`] if it is set and not empty, otherwise in the
    /// operating system's data folder (see the module documentation).
    ///
    /// Nothing is read or created on disk.
    ///
    /// # Errors
    ///
    /// Returns an error naming [`DATA_DIR_VAR`] when the variable is unset
    /// and the operating system has no data folder for the user, as when no
    /// home folder is known.
    pub fn locate() -> anyhow::Result<Self> {
        let folder = data_folder(std::env::var_os(DATA_DIR_VAR)).with_context(|| {
            format!(
                "cannot find a folder to keep the save in: the system gives no \
                 data folder for this user; set {DATA_DIR_VAR} to a folder path"
            )
        })?;
        Ok(Self::in_folder(folder))
    }

    /// Returns the path of the save, `save.ron`.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Path of the file a save is written to before it replaces the save:
    /// `save.ron.tmp`.
    #[cfg(test)]
    fn temporary_path(&self) -> PathBuf {
        with_suffix(&self.path, TEMPORARY_SUFFIX)
    }

    /// Path an unreadable save is renamed to: `save.ron.bad`.
    fn set_aside_path(&self) -> PathBuf {
        with_suffix(&self.path, SET_ASIDE_SUFFIX)
    }

    /// Reads the save back into a game whose monsters, items and sigils are
    /// found in `content`.
    ///
    /// Only `save.ron` is read, never a leftover `save.ron.tmp`. A save
    /// larger than [`MAX_FILE_BYTES`], not UTF-8 text, or refused by
    /// `save::from_str` is renamed `save.ron.bad`, replacing an older one of
    /// that name, and reported as [`Loaded::SetAside`] with the reason, a
    /// [`SaveRefusal`]; the save itself is otherwise left as it is.
    ///
    /// # Errors
    ///
    /// Returns an error naming the file when it exists but cannot be read
    /// (permissions, a folder of that name…), or when an unreadable save
    /// cannot be renamed: starting a new game then would overwrite it at the
    /// next save.
    pub fn load(&self, content: &Content) -> anyhow::Result<Loaded> {
        let reason = match read_bounded(&self.path) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Err(_) => SaveRefusal::NotText,
                Ok(text) => match save::from_str(&text, content) {
                    Ok(game) => return Ok(Loaded::Game(Box::new(game))),
                    Err(error) => SaveRefusal::Refused(error),
                },
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Loaded::Missing),
            Err(error) if error.kind() == io::ErrorKind::FileTooLarge => SaveRefusal::TooLarge {
                limit: MAX_FILE_BYTES,
            },
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "cannot read the save {}: check that it is a file you can read",
                        self.path.display()
                    )
                });
            }
        };
        let moved_to = self.set_aside_path();
        fs::rename(&self.path, &moved_to).with_context(|| {
            format!(
                "cannot set aside the unreadable save {} ({}): move or \
                 delete it, then start the game again",
                self.path.display(),
                crate::messages::save_refusal(Language::English, &reason)
            )
        })?;
        Ok(Loaded::SetAside { reason, moved_to })
    }

    /// Writes `game` as the save, replacing the previous one, creating the
    /// data folder if needed.
    ///
    /// The text goes to `save.ron.tmp`, is flushed to the disk, and the file
    /// is then renamed over `save.ron`: a crash or a power cut leaves the old
    /// save or the new one whole. A `save.ron.tmp` left by such an
    /// interruption is simply overwritten.
    ///
    /// # Errors
    ///
    /// Returns an error naming the save's path and saying the game was not
    /// saved, when the folder cannot be created, the file cannot be written
    /// or renamed (no permission, disk full…), or the game cannot be written
    /// as text. The previous save, if any, is then left as it was.
    pub fn write(&self, game: &Game) -> anyhow::Result<()> {
        self.try_write(game).with_context(|| {
            format!(
                "the game could not be saved to {}: check that this folder can \
                 be written to and the disk is not full (set {DATA_DIR_VAR} to \
                 save elsewhere)",
                self.path.display()
            )
        })
    }

    /// The steps of [`SaveFile::write`], without the context of its error.
    fn try_write(&self, game: &Game) -> anyhow::Result<()> {
        let text = save::to_string(game)?;
        write_atomically(&self.path, &text)
    }

    /// Deletes the save, so a finished game, lost or won, can never be
    /// resumed. Deleting a save that does not exist succeeds, so this can be
    /// repeated.
    ///
    /// # Errors
    ///
    /// Returns an error naming the save when it exists and cannot be
    /// deleted.
    pub fn delete(&self) -> anyhow::Result<()> {
        match fs::remove_file(&self.path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => {
                Err(error).with_context(|| {
                    format!(
                        "cannot delete the save {} of the finished game: delete \
                         it by hand so it is not resumed",
                        self.path.display()
                    )
                })
            }
            _ => Ok(()),
        }
    }
}

/// The options file of one data folder: `options.ron`, beside the save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionsFile {
    /// Path of `options.ron`.
    path: PathBuf,
}

/// What [`OptionsFile::load`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadedOptions {
    /// There is no options file yet: the defaults apply.
    Missing,
    /// The options the player chose.
    Found(Options),
    /// The file could not be read back: the defaults apply until an
    /// option changes, which rewrites the file.
    Unreadable {
        /// Why (`messages::options_problem` words it).
        reason: OptionsProblem,
    },
}

impl LoadedOptions {
    /// Returns the options to play with: those found, or the defaults with
    /// the language and keyboard layout of `system`, the operating
    /// system's.
    pub fn options(&self, system: Defaults) -> Options {
        match self {
            Self::Found(options) => *options,
            Self::Missing | Self::Unreadable { .. } => Options {
                language: system.language,
                keyboard: system.keyboard,
                ..Options::default()
            },
        }
    }
}

/// Why the options file could not be read back, as facts the player is
/// told in their language (`messages::options_problem`): the system's
/// error, the size, the format version, or what is wrong on which line, in
/// the file's own words. Lines and columns are counted from 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptionsProblem {
    /// The system could not read the file; its error, as it gives it.
    System(String),
    /// The file is larger than `limit` bytes.
    TooLarge {
        /// The largest file read, [`MAX_FILE_BYTES`].
        limit: u64,
    },
    /// The file is not UTF-8 text.
    NotText,
    /// The file is empty or blank.
    Empty,
    /// The file has another format version than
    /// [`OPTIONS_FORMAT_VERSION`].
    OtherVersion {
        /// The version written in the file.
        found: u32,
    },
    /// A value names no value of its setting.
    UnknownValue {
        /// Line of the value.
        line: usize,
        /// The value found.
        found: String,
        /// The setting's key and its accepted values, when known.
        setting: Option<(&'static str, Vec<String>)>,
    },
    /// The format version is missing.
    MissingVersion,
    /// A setting is missing.
    MissingSetting {
        /// Its key in the file.
        key: String,
    },
    /// The format version is written twice.
    RepeatedVersion {
        /// Line of the second one.
        line: usize,
    },
    /// A setting is written twice.
    RepeatedSetting {
        /// Line of the second one.
        line: usize,
        /// Its key in the file.
        key: String,
    },
    /// The format version is not a whole number.
    VersionNotWhole {
        /// Line of the version.
        line: usize,
    },
    /// A value is of the wrong kind, such as a number for a setting.
    WrongKind {
        /// Line of the value.
        line: usize,
        /// Column of the value.
        column: usize,
        /// The key of its setting, when known.
        key: Option<&'static str>,
    },
    /// The settings are not inside parentheses.
    NotInParentheses {
        /// Line where reading stopped.
        line: usize,
    },
    /// The file ends before its closing parenthesis.
    EndsEarly {
        /// Last line read.
        line: usize,
    },
    /// A comma is missing between two settings.
    CommaMissing {
        /// Line where it is missing.
        line: usize,
        /// Column where it is missing.
        column: usize,
    },
    /// A colon is missing after a key.
    ColonMissing {
        /// Line where it is missing.
        line: usize,
        /// Column where it is missing.
        column: usize,
    },
    /// The closing parenthesis is missing.
    ClosingMissing {
        /// Line where it is missing.
        line: usize,
        /// Column where it is missing.
        column: usize,
    },
    /// Text follows the closing parenthesis.
    TextAfterEnd {
        /// Line of that text.
        line: usize,
        /// Column of that text.
        column: usize,
    },
    /// A character has no place where it is.
    UnexpectedCharacter {
        /// Its line.
        line: usize,
        /// Its column.
        column: usize,
        /// The character.
        character: char,
    },
    /// Any other error of the reader: the text is not RON.
    NotRon {
        /// Line where reading stopped.
        line: usize,
        /// Column where reading stopped.
        column: usize,
    },
}

/// The options file as written on disk: the format version first, then
/// one field per option, such as
/// `(format_version: 1, glyphs: Unicode, colors: Auto, theme: Dark,
/// language: French, keyboard: Azerty)`.
///
/// `language` and `keyboard` came after the first files were written: a
/// file without them is still of version 1, and its language and keyboard
/// layout are the system's.
#[derive(Debug, Serialize, Deserialize)]
struct OptionsRecord {
    /// [`OPTIONS_FORMAT_VERSION`] when the file was written.
    format_version: u32,
    /// Characters to draw with.
    glyphs: GlyphSet,
    /// Colours to draw with.
    colors: ColorChoice,
    /// Dark or light theme.
    theme: Appearance,
    /// Language of the game's text; `None` in a file written before the
    /// option existed.
    #[serde(
        default,
        deserialize_with = "read_bare",
        serialize_with = "write_bare",
        skip_serializing_if = "Option::is_none"
    )]
    language: Option<Language>,
    /// Keyboard layout the keys follow; `None` in a file written before
    /// the option existed.
    #[serde(
        default,
        deserialize_with = "read_bare",
        serialize_with = "write_bare",
        skip_serializing_if = "Option::is_none"
    )]
    keyboard: Option<Keyboard>,
}

/// Reads a setting added after the first files, written bare (`French`,
/// `Azerty`) rather than as RON's `Some(French)`; its absence is left to
/// `#[serde(default)]`.
fn read_bare<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

/// Writes a setting bare, as [`read_bare`] reads it; only called with a
/// value, the field being skipped otherwise.
fn write_bare<S: serde::Serializer, T: Serialize>(
    value: &Option<T>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match value {
        Some(value) => value.serialize(serializer),
        None => serializer.serialize_none(),
    }
}

/// The first field of any options file, read alone so that a file of
/// another format version is named as such rather than reported as
/// malformed.
#[derive(Debug, Deserialize)]
struct OptionsVersion {
    /// The format version the file says it was written in.
    format_version: u32,
}

impl OptionsFile {
    /// The options file in `folder`, which need not exist yet: it is
    /// created by the first [`OptionsFile::write`].
    pub fn in_folder(folder: impl Into<PathBuf>) -> Self {
        Self {
            path: folder.into().join(OPTIONS_NAME),
        }
    }

    /// The options file in the same folder as `save`, so
    /// [`DATA_DIR_VAR`] moves both.
    pub fn beside(save: &SaveFile) -> Self {
        Self::in_folder(save.path().parent().unwrap_or_else(|| Path::new("")))
    }

    /// Returns the path of the options file, `options.ron`.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads the player's options back; a file written before the language
    /// or the keyboard option existed gives the language or the layout of
    /// `system`, the operating system's.
    ///
    /// Never fails: a missing file gives [`LoadedOptions::Missing`], and a
    /// file that cannot be read (permissions, larger than
    /// [`MAX_FILE_BYTES`], not text, not RON, another format version, an
    /// unknown value) gives [`LoadedOptions::Unreadable`] with the reason.
    /// The file itself is left as it is.
    pub fn load(&self, system: Defaults) -> LoadedOptions {
        let bytes = match read_bounded(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return LoadedOptions::Missing,
            Err(error) if error.kind() == io::ErrorKind::FileTooLarge => {
                return unreadable(OptionsProblem::TooLarge {
                    limit: MAX_FILE_BYTES,
                });
            }
            Err(error) => return unreadable(OptionsProblem::System(error.to_string())),
        };
        let Ok(text) = String::from_utf8(bytes) else {
            return unreadable(OptionsProblem::NotText);
        };
        match parse_options(&text, system) {
            Ok(options) => LoadedOptions::Found(options),
            Err(reason) => LoadedOptions::Unreadable { reason },
        }
    }

    /// Writes `options` as the options file, replacing the previous one and
    /// creating the data folder if needed, atomically as
    /// [`SaveFile::write`] does.
    ///
    /// # Errors
    ///
    /// Returns an error naming the file when the folder cannot be created
    /// or the file cannot be written or renamed; the previous file, if any,
    /// is then left as it was.
    pub fn write(&self, options: &Options) -> anyhow::Result<()> {
        let record = OptionsRecord {
            format_version: OPTIONS_FORMAT_VERSION,
            glyphs: options.glyphs,
            colors: options.colors,
            theme: options.appearance,
            language: Some(options.language),
            keyboard: Some(options.keyboard),
        };
        let text = ron::ser::to_string_pretty(&record, ron::ser::PrettyConfig::default())
            .context("cannot write the options as text")?;
        write_atomically(&self.path, &text).with_context(|| {
            format!(
                "the options could not be saved to {}: check that this folder \
                 can be written to (set {DATA_DIR_VAR} to keep them elsewhere)",
                self.path.display()
            )
        })
    }
}

/// A [`LoadedOptions::Unreadable`] whose reason is `reason`.
const fn unreadable(reason: OptionsProblem) -> LoadedOptions {
    LoadedOptions::Unreadable { reason }
}

/// Reads the text of an options file, `system` giving the language and the
/// keyboard layout of a file that names none, or returns why it cannot be
/// read: it is empty, it
/// has another format version, or what is wrong and on which line (see
/// [`describe_parse_error`]).
fn parse_options(text: &str, system: Defaults) -> Result<Options, OptionsProblem> {
    if text.trim().is_empty() {
        return Err(OptionsProblem::Empty);
    }
    let version: OptionsVersion =
        ron::from_str(text).map_err(|error| describe_parse_error(text, &error))?;
    if version.format_version != OPTIONS_FORMAT_VERSION {
        return Err(OptionsProblem::OtherVersion {
            found: version.format_version,
        });
    }
    let record: OptionsRecord =
        ron::from_str(text).map_err(|error| describe_parse_error(text, &error))?;
    Ok(Options {
        glyphs: record.glyphs,
        colors: record.colors,
        appearance: record.theme,
        language: record.language.unwrap_or(system.language),
        keyboard: record.keyboard.unwrap_or(system.keyboard),
    })
}

/// The settings of the options file: the name serde gives the type of each
/// value in its errors, and the key that names the setting in the file.
///
/// The parser reports an unknown value with the type's name (`GlyphSet`),
/// which means nothing to a player; the key (`glyphs`) is what they read in
/// the file.
const SETTING_KEYS: [(&str, &str); 5] = [
    ("GlyphSet", "glyphs"),
    ("ColorChoice", "colors"),
    ("Appearance", "theme"),
    ("Language", "language"),
    ("Keyboard", "keyboard"),
];

/// Key of the format version in the options file.
const VERSION_KEY: &str = "format_version";

/// Describes why the options file `text` could not be parsed, as `error`
/// says, as an [`OptionsProblem`] that names the line and, when it can, the
/// setting at fault in the file's own words, such as "line 1: unknown
/// value `Braille` for glyphs (Unicode or Ascii)".
///
/// The parser's own messages are never kept: they name the program's
/// types ("enum `GlyphSet`", "struct `OptionsVersion`"). An error the
/// function does not recognise gives [`OptionsProblem::NotRon`] with its
/// line and column.
fn describe_parse_error(text: &str, error: &ron::error::SpannedError) -> OptionsProblem {
    use ron::error::Error;

    let at = error.span.start;
    let (line, column) = (at.line, at.col);
    match &error.code {
        Error::NoSuchEnumVariant {
            expected,
            found,
            outer,
        } => OptionsProblem::UnknownValue {
            line,
            found: found.clone(),
            setting: outer.as_deref().and_then(setting_key).map(|key| {
                let values = expected.iter().map(|value| (*value).to_owned()).collect();
                (key, values)
            }),
        },
        Error::MissingStructField { field, .. } if *field == VERSION_KEY => {
            OptionsProblem::MissingVersion
        }
        Error::MissingStructField { field, .. } => OptionsProblem::MissingSetting {
            key: (*field).to_owned(),
        },
        Error::DuplicateStructField { field, .. } if *field == VERSION_KEY => {
            OptionsProblem::RepeatedVersion { line }
        }
        Error::DuplicateStructField { field, .. } => OptionsProblem::RepeatedSetting {
            line,
            key: (*field).to_owned(),
        },
        // The format version is the only number of the file.
        Error::ExpectedInteger | Error::IntegerOutOfBounds | Error::InvalidIntegerDigit { .. } => {
            OptionsProblem::VersionNotWhole { line }
        }
        Error::ExpectedIdentifier
        | Error::SuggestRawIdentifier(_)
        | Error::InvalidValueForType { .. } => OptionsProblem::WrongKind {
            line,
            column,
            key: key_before(text, at),
        },
        Error::ExpectedNamedStructLike(_)
        | Error::ExpectedStructLike
        | Error::ExpectedDifferentStructName { .. } => OptionsProblem::NotInParentheses { line },
        Error::Eof => OptionsProblem::EndsEarly { line },
        Error::ExpectedComma => OptionsProblem::CommaMissing { line, column },
        Error::ExpectedMapColon => OptionsProblem::ColonMissing { line, column },
        Error::ExpectedStructLikeEnd => OptionsProblem::ClosingMissing { line, column },
        Error::TrailingCharacters => OptionsProblem::TextAfterEnd { line, column },
        Error::UnexpectedChar(character) => OptionsProblem::UnexpectedCharacter {
            line,
            column,
            character: *character,
        },
        _ => OptionsProblem::NotRon { line, column },
    }
}

/// The key in the file of the setting whose values have the type named
/// `type_name` in the parser's errors, if it is one of [`SETTING_KEYS`].
fn setting_key(type_name: &str) -> Option<&'static str> {
    SETTING_KEYS
        .iter()
        .find(|(name, _)| *name == type_name)
        .map(|(_, key)| *key)
}

/// The key of the setting whose value starts at `at` in `text`, as in
/// `glyphs: 3`, when it is one of [`SETTING_KEYS`].
///
/// The parser does not say which field a value of the wrong kind belongs
/// to, only where that value starts; the key is the word just before the
/// colon that precedes it. `None` when the text before `at` does not end
/// with a known key and a colon.
fn key_before(text: &str, at: ron::error::Position) -> Option<&'static str> {
    let before = text[..byte_offset(text, at)?]
        .trim_end()
        .strip_suffix(':')?
        .trim_end();
    let start = before
        .char_indices()
        .rev()
        .find(|(_, character)| !(character.is_ascii_alphanumeric() || *character == '_'))
        .map_or(0, |(index, character)| index + character.len_utf8());
    let key = &before[start..];
    SETTING_KEYS
        .iter()
        .map(|(_, setting)| *setting)
        .find(|setting| *setting == key)
}

/// The byte offset in `text` of the character at `at`, a 1-based line and
/// column (in characters) as the parser reports them, or `None` when `text`
/// has no such position. The position just after the last character is the
/// length of `text`.
fn byte_offset(text: &str, at: ron::error::Position) -> Option<usize> {
    let (mut line, mut column) = (1, 1);
    for (offset, character) in text.char_indices() {
        if (line, column) == (at.line, at.col) {
            return Some(offset);
        }
        if character == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    ((line, column) == (at.line, at.col)).then_some(text.len())
}

/// Reads the whole file at `path`, at most [`MAX_FILE_BYTES`] of it.
///
/// Only `MAX_FILE_BYTES + 1` bytes are ever read, so a huge file costs no
/// more memory than the largest one accepted.
///
/// # Errors
///
/// Returns the system's error when the file cannot be opened or read (its
/// kind is `NotFound` when there is no file), and an error of kind
/// [`io::ErrorKind::FileTooLarge`] when it holds more than
/// [`MAX_FILE_BYTES`] bytes.
fn read_bounded(path: &Path) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(io::Error::from(io::ErrorKind::FileTooLarge));
    }
    Ok(bytes)
}

/// Writes `text` to `path` so that a crash leaves the old file or the new
/// one whole, never half of one: the text goes to `path` with
/// [`TEMPORARY_SUFFIX`], is flushed to the disk, and that file is renamed
/// over `path`. The folder of `path` is created if missing.
///
/// A temporary file left by an interrupted write is deleted first, and the
/// new one is created only if nothing has that name (`create_new`): when the
/// name is a symbolic link, the link itself is deleted and the file it
/// points to is never opened nor written.
///
/// # Errors
///
/// Returns an error naming the folder or the file when the folder cannot be
/// created, or the temporary file cannot be created (a leftover of that
/// name that cannot be deleted, such as a folder, counts as such), written
/// or renamed.
fn write_atomically(path: &Path, text: &str) -> anyhow::Result<()> {
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)
            .with_context(|| format!("cannot create the folder {}", folder.display()))?;
    }
    let temporary = with_suffix(path, TEMPORARY_SUFFIX);
    match fs::remove_file(&temporary) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => {
            return Err(error).with_context(|| format!("cannot create {}", temporary.display()));
        }
        _ => {}
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .with_context(|| format!("cannot create {}", temporary.display()))?;
    file.write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
        .with_context(|| format!("cannot write {}", temporary.display()))?;
    drop(file);
    fs::rename(&temporary, path).with_context(|| format!("cannot rename {}", temporary.display()))
}

/// The data folder: `override_folder` (the value of [`DATA_DIR_VAR`]) when
/// given and not empty, otherwise the operating system's data folder for
/// [`APPLICATION`], or `None` if the system has none for this user.
fn data_folder(override_folder: Option<OsString>) -> Option<PathBuf> {
    match override_folder {
        Some(folder) if !folder.is_empty() => Some(PathBuf::from(folder)),
        _ => system_data_folder(),
    }
}

/// The operating system's data folder for [`APPLICATION`] (see the module
/// documentation), or `None` if the system has none for this user. Nothing
/// is read or created on disk.
pub(crate) fn system_data_folder() -> Option<PathBuf> {
    ProjectDirs::from("", "", APPLICATION).map(|dirs| dirs.data_dir().to_path_buf())
}

/// Returns `path` with `suffix` added after its whole file name, as
/// `save.ron` becomes `save.ron.tmp`.
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// A fresh, empty folder under the system's temporary folder, deleted with
/// everything in it when dropped, so tests never touch the player's own
/// data folder.
#[cfg(test)]
#[derive(Debug)]
pub struct ScratchFolder {
    /// Path of the folder.
    path: PathBuf,
}

#[cfg(test)]
impl ScratchFolder {
    /// Most names tried before giving up: a run of clashes on 32-bit random
    /// names means something else creates folders of that form on purpose.
    const ATTEMPTS: usize = 16;

    /// Creates a new folder whose name holds `label` and 32 random bits, so
    /// parallel tests and test runs never share one, and another user of the
    /// temporary folder cannot guess it and prepare files or links there
    /// beforehand. 32 bits keep the paths short enough for the screens that
    /// show them whole (a line of 75 characters); a name guessed and taken
    /// first only makes the next attempt draw another.
    ///
    /// The folder is created with `create_dir`, which fails on any existing
    /// name, link included: a name already taken is never reused, another
    /// one is drawn.
    ///
    /// # Panics
    ///
    /// Panics if no folder can be created after [`Self::ATTEMPTS`] names.
    pub fn new(label: &str) -> Self {
        let temporary = std::env::temp_dir();
        for _ in 0..Self::ATTEMPTS {
            let path = temporary.join(format!("sigilfall-{label}-{:08x}", random_bits()));
            match fs::create_dir(&path) {
                Ok(()) => return Self { path },
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("cannot create a scratch folder {}: {error}", path.display()),
            }
        }
        panic!("no free scratch folder name in {}", temporary.display());
    }

    /// Returns the path of the folder.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The save file of this folder.
    pub fn save_file(&self) -> SaveFile {
        SaveFile::in_folder(&self.path)
    }

    /// The options file of this folder.
    pub fn options_file(&self) -> OptionsFile {
        OptionsFile::in_folder(&self.path)
    }
}

/// 32 bits another process cannot predict: the standard library draws the
/// keys of `RandomState` from the operating system's random source once per
/// thread, and changes them for each new one, so hashing the same value
/// with a new one gives a fresh value without a dependency.
#[cfg(test)]
fn random_bits() -> u32 {
    use std::hash::BuildHasher as _;

    let bits = std::collections::hash_map::RandomState::new().hash_one(());
    // The high half of the hash, folded onto the low one: every bit counts.
    (bits ^ (bits >> 32)) as u32
}

#[cfg(test)]
impl Drop for ScratchFolder {
    fn drop(&mut self) {
        // A file still open elsewhere (Windows) can keep the folder; it is
        // in the temporary folder, which the system cleans, so a failure
        // here must not fail the test.
        drop(fs::remove_dir_all(&self.path));
    }
}

#[cfg(test)]
mod tests {
    use sigilfall_core::action::Action;
    use sigilfall_core::geometry::Direction;
    use sigilfall_core::rng::Seed;

    use super::*;
    use crate::messages;

    /// The game's shipped content.
    fn content() -> Content {
        Content::load().expect("the shipped content is valid")
    }

    /// A new game of `seed` on its generated first floor, after a few
    /// actions, so its save holds more than a fresh start.
    fn played_game(seed: u64) -> Game {
        let mut game = Game::new(Seed::new(seed), &content());
        for direction in [Direction::East, Direction::South, Direction::West] {
            game.submit(Action::Move(direction));
        }
        game.submit(Action::Wait);
        game
    }

    /// Asserts that `found` is the game `expected`, by comparing their
    /// saves, which hold the whole state of a game, random stream included.
    fn assert_same_game(found: &Game, expected: &Game) {
        assert_eq!(
            save::to_string(found).unwrap(),
            save::to_string(expected).unwrap()
        );
    }

    #[test]
    fn the_save_is_save_ron_in_the_data_folder() {
        let folder = ScratchFolder::new("path");

        let save = folder.save_file();

        assert_eq!(save.path(), folder.path().join("save.ron"));
        assert_eq!(save.temporary_path(), folder.path().join("save.ron.tmp"));
        assert_eq!(save.set_aside_path(), folder.path().join("save.ron.bad"));
    }

    #[test]
    fn the_variable_replaces_the_system_data_folder_unless_empty() {
        let chosen = data_folder(Some(OsString::from("somewhere/else")));
        let system = data_folder(None);

        assert_eq!(chosen, Some(PathBuf::from("somewhere/else")));
        assert_eq!(
            data_folder(Some(OsString::new())),
            system,
            "empty is ignored"
        );
        let system = system.expect("this machine has a data folder");
        assert!(
            system.ends_with("Sigilfall/data")
                || system.ends_with("sigilfall")
                || system.ends_with("Sigilfall"),
            "{}",
            system.display()
        );
    }

    #[test]
    fn a_written_game_is_read_back_whole() {
        let folder = ScratchFolder::new("round-trip");
        let save = folder.save_file();
        let game = played_game(11);

        save.write(&game).unwrap();
        let Loaded::Game(loaded) = save.load(&content()).unwrap() else {
            panic!("the save is read back");
        };

        assert_same_game(&loaded, &game);
        assert!(save.path().is_file(), "loading keeps the save");
    }

    #[test]
    fn writing_creates_missing_folders_and_leaves_no_temporary_file() {
        let folder = ScratchFolder::new("nested");
        let save = SaveFile::in_folder(folder.path().join("a").join("b"));

        save.write(&played_game(12)).unwrap();

        assert!(save.path().is_file());
        assert!(!save.temporary_path().exists());
    }

    #[test]
    fn writing_again_replaces_the_previous_save() {
        let folder = ScratchFolder::new("replace");
        let save = folder.save_file();
        save.write(&played_game(13)).unwrap();
        let later = played_game(14);

        save.write(&later).unwrap();

        let Loaded::Game(loaded) = save.load(&content()).unwrap() else {
            panic!("the save is read back");
        };
        assert_same_game(&loaded, &later);
    }

    #[test]
    fn no_save_reads_as_missing() {
        let folder = ScratchFolder::new("missing");

        assert!(matches!(
            folder.save_file().load(&content()),
            Ok(Loaded::Missing)
        ));
    }

    #[test]
    fn a_leftover_temporary_file_is_never_read_as_a_save() {
        let folder = ScratchFolder::new("temporary");
        let save = folder.save_file();
        let text = save::to_string(&played_game(15)).unwrap();
        fs::write(save.temporary_path(), &text).unwrap();

        assert!(matches!(save.load(&content()), Ok(Loaded::Missing)));
        assert_eq!(fs::read_to_string(save.temporary_path()).unwrap(), text);
    }

    #[test]
    fn a_half_written_temporary_file_leaves_the_save_as_it_was() {
        let folder = ScratchFolder::new("interrupted");
        let save = folder.save_file();
        let game = played_game(16);
        save.write(&game).unwrap();
        // What an interruption in the middle of the next write leaves.
        let text = save::to_string(&played_game(17)).unwrap();
        fs::write(save.temporary_path(), &text[..text.len() / 2]).unwrap();

        let Loaded::Game(loaded) = save.load(&content()).unwrap() else {
            panic!("the last whole save is read");
        };
        assert_same_game(&loaded, &game);

        // The next write overwrites the leftover and succeeds.
        save.write(&played_game(17)).unwrap();
        assert!(!save.temporary_path().exists());
    }

    #[test]
    fn an_unreadable_save_is_set_aside_with_its_reason() {
        for (label, bytes) in [
            ("not-ron", b"this is not a save".to_vec()),
            ("not-text", vec![0xFF, 0xFE, 0x00, 0x42]),
            ("empty", Vec::new()),
        ] {
            let folder = ScratchFolder::new(label);
            let save = folder.save_file();
            fs::write(save.path(), &bytes).unwrap();

            let Loaded::SetAside { reason, moved_to } = save.load(&content()).unwrap() else {
                panic!("{label}: the save is set aside");
            };
            let reason = messages::save_refusal(Language::English, &reason);

            assert!(!reason.is_empty(), "{label}");
            assert_eq!(moved_to, save.set_aside_path(), "{label}");
            assert_eq!(fs::read(&moved_to).unwrap(), bytes, "{label}: kept whole");
            assert!(!save.path().exists(), "{label}: never read again");
        }
    }

    #[test]
    fn a_save_of_another_version_is_set_aside_and_says_so() {
        let folder = ScratchFolder::new("version");
        let save = folder.save_file();
        let text = save::to_string(&played_game(18)).unwrap();
        let current = format!("format_version: {},", save::FORMAT_VERSION);
        let other = save::FORMAT_VERSION + 1;
        assert!(text.contains(&current), "{text}");
        let text = text.replacen(&current, &format!("format_version: {other},"), 1);
        fs::write(save.path(), &text).unwrap();

        let Loaded::SetAside { reason, moved_to } = save.load(&content()).unwrap() else {
            panic!("the save is set aside");
        };
        let reason = messages::save_refusal(Language::English, &reason);

        assert!(reason.contains(&format!("version {other}")), "{reason}");
        assert_eq!(fs::read_to_string(moved_to).unwrap(), text);
    }

    #[test]
    fn a_save_larger_than_the_limit_is_set_aside_without_being_read_whole() {
        let folder = ScratchFolder::new("too-large");
        let save = folder.save_file();
        let size = MAX_FILE_BYTES + 1;
        File::create(save.path()).unwrap().set_len(size).unwrap();

        let Loaded::SetAside { reason, moved_to } = save.load(&content()).unwrap() else {
            panic!("the save is set aside");
        };
        let reason = messages::save_refusal(Language::English, &reason);

        assert_eq!(reason, "the save is larger than 4 MiB");
        assert_eq!(fs::metadata(moved_to).unwrap().len(), size, "kept whole");
    }

    #[test]
    fn a_save_at_the_limit_is_read_and_refused_for_its_text_only() {
        let folder = ScratchFolder::new("at-limit");
        let save = folder.save_file();
        File::create(save.path())
            .unwrap()
            .set_len(MAX_FILE_BYTES)
            .unwrap();

        let Loaded::SetAside { reason, .. } = save.load(&content()).unwrap() else {
            panic!("the save is set aside");
        };
        let reason = messages::save_refusal(Language::English, &reason);

        assert!(!reason.contains("larger"), "{reason}");
    }

    #[test]
    fn an_escape_sequence_in_a_save_never_reaches_the_reason() {
        let folder = ScratchFolder::new("escape");
        let save = folder.save_file();
        let text = save::to_string(&played_game(23)).unwrap();
        // A terminal title change, a bell and a screen clear, written into
        // the identifier of the first definition the save names.
        let field = "definition: \"";
        assert!(text.contains(field), "{text}");
        let hostile = "\u{1b}]0;owned\u{7}\u{1b}[2J";
        let edited = text.replacen(field, &format!("{field}{hostile}"), 1);
        fs::write(save.path(), &edited).unwrap();

        let Loaded::SetAside { reason, .. } = save.load(&content()).unwrap() else {
            panic!("the save is set aside");
        };
        let reason = messages::save_refusal(Language::English, &reason);

        assert!(!reason.chars().any(char::is_control), "{reason:?}");
        assert!(reason.contains("?]0;owned??[2J"), "{reason:?}");
    }

    #[test]
    fn a_save_set_aside_replaces_an_older_one() {
        let folder = ScratchFolder::new("aside-twice");
        let save = folder.save_file();
        fs::write(save.set_aside_path(), "older").unwrap();
        fs::write(save.path(), "newer").unwrap();

        assert!(matches!(save.load(&content()), Ok(Loaded::SetAside { .. })));
        assert_eq!(fs::read_to_string(save.set_aside_path()).unwrap(), "newer");
    }

    #[test]
    fn a_save_that_cannot_be_read_is_an_error_and_is_kept() {
        let folder = ScratchFolder::new("unreadable");
        let save = folder.save_file();
        fs::create_dir(save.path()).unwrap();

        let error = save.load(&content()).unwrap_err();

        assert!(
            format!("{error:#}").contains(&save.path().display().to_string()),
            "{error:#}"
        );
        assert!(save.path().is_dir(), "nothing is moved");
    }

    #[test]
    fn a_failed_write_names_the_save_and_keeps_the_previous_one() {
        let folder = ScratchFolder::new("write-fails");
        let save = folder.save_file();
        let game = played_game(19);
        save.write(&game).unwrap();
        // A folder in the way of the temporary file makes its creation fail.
        fs::create_dir(save.temporary_path()).unwrap();

        let error = save.write(&played_game(20)).unwrap_err();

        let message = format!("{error:#}");
        assert!(message.contains("could not be saved"), "{message}");
        assert!(
            message.contains(&save.path().display().to_string()),
            "{message}"
        );
        let Loaded::Game(loaded) = save.load(&content()).unwrap() else {
            panic!("the previous save is still there");
        };
        assert_same_game(&loaded, &game);
    }

    #[test]
    fn a_write_into_a_folder_that_cannot_exist_fails() {
        let folder = ScratchFolder::new("no-folder");
        let file = folder.path().join("a-file");
        fs::write(&file, "").unwrap();
        let save = SaveFile::in_folder(file.join("data"));

        let error = save.write(&played_game(21)).unwrap_err();

        assert!(
            format!("{error:#}").contains("could not be saved"),
            "{error:#}"
        );
    }

    #[test]
    fn a_leftover_temporary_file_is_replaced_never_written_through() {
        let folder = ScratchFolder::new("leftover");
        let save = folder.save_file();
        let target = folder.path().join("elsewhere.txt");
        fs::write(&target, "not the game's").unwrap();
        // A link in place of the temporary file must not lead the write to
        // the file it points to. Creating one may need a privilege on
        // Windows; the rule is then checked on the other systems.
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&target, save.temporary_path()).is_ok();
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_file(&target, save.temporary_path()).is_ok();
        if !linked {
            fs::write(save.temporary_path(), "leftover").unwrap();
        }
        let game = played_game(24);

        save.write(&game).unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "not the game's");
        assert!(!save.temporary_path().exists());
        let Loaded::Game(loaded) = save.load(&content()).unwrap() else {
            panic!("the save is read back");
        };
        assert_same_game(&loaded, &game);
        if !linked {
            eprintln!("no symbolic link could be created here: checked with a plain file");
        }
    }

    #[test]
    fn scratch_folders_have_unpredictable_names() {
        let (first, second) = (ScratchFolder::new("names"), ScratchFolder::new("names"));

        assert_ne!(first.path(), second.path());
        for folder in [&first, &second] {
            let name = folder.path().file_name().unwrap().to_string_lossy();
            let suffix = name.strip_prefix("sigilfall-names-").expect("the label");
            assert_eq!(suffix.len(), 8, "{name}");
            assert!(suffix.chars().all(|c| c.is_ascii_hexdigit()), "{name}");
            assert!(
                !name.contains(&std::process::id().to_string()),
                "no process id: {name}"
            );
        }
    }

    #[test]
    fn deleting_removes_the_save_and_can_be_repeated() {
        let folder = ScratchFolder::new("delete");
        let save = folder.save_file();
        save.write(&played_game(22)).unwrap();

        save.delete().unwrap();
        save.delete().unwrap();

        assert!(!save.path().exists());
        assert!(matches!(save.load(&content()), Ok(Loaded::Missing)));
    }

    #[test]
    fn a_save_that_cannot_be_deleted_is_an_error() {
        let folder = ScratchFolder::new("undeletable");
        let save = folder.save_file();
        fs::create_dir(save.path()).unwrap();

        let error = save.delete().unwrap_err();

        assert!(format!("{error:#}").contains("delete"), "{error:#}");
    }

    #[test]
    fn the_options_are_options_ron_beside_the_save() {
        let folder = ScratchFolder::new("options-path");

        let options = OptionsFile::beside(&folder.save_file());

        assert_eq!(options.path(), folder.path().join("options.ron"));
        assert_eq!(options, folder.options_file());
    }

    #[test]
    fn every_combination_of_options_is_read_back() {
        let folder = ScratchFolder::new("options-round-trip");
        let file = folder.options_file();
        for glyphs in [GlyphSet::Unicode, GlyphSet::Ascii] {
            for colors in [
                ColorChoice::Auto,
                ColorChoice::Ansi16,
                ColorChoice::Monochrome,
            ] {
                for (appearance, keyboard) in [
                    (Appearance::Dark, Keyboard::Azerty),
                    (Appearance::Light, Keyboard::Qwerty),
                ] {
                    let options = Options {
                        glyphs,
                        colors,
                        appearance,
                        language: Language::French,
                        keyboard,
                    };

                    file.write(&options).unwrap();

                    assert_eq!(
                        file.load(Defaults::default()),
                        LoadedOptions::Found(options)
                    );
                    assert!(!with_suffix(file.path(), TEMPORARY_SUFFIX).exists());
                }
            }
        }
    }

    #[test]
    fn the_options_file_is_readable_text_with_its_version_first() {
        let folder = ScratchFolder::new("options-text");
        let file = folder.options_file();
        let options = Options {
            glyphs: GlyphSet::Ascii,
            colors: ColorChoice::Ansi16,
            appearance: Appearance::Light,
            language: Language::English,
            keyboard: Keyboard::Qwerty,
        };

        file.write(&options).unwrap();

        let text = fs::read_to_string(file.path()).unwrap();
        let version = text.find("format_version: 1").expect("the version");
        for field in [
            "glyphs: Ascii",
            "colors: Ansi16",
            "theme: Light",
            "language: English",
            "keyboard: Qwerty",
        ] {
            let at = text
                .find(field)
                .unwrap_or_else(|| panic!("{field}:\n{text}"));
            assert!(version < at, "the version comes first:\n{text}");
        }
        // Written by hand, in any order and on one line, it reads the same.
        fs::write(
            file.path(),
            "(theme: Light, colors: Ansi16, glyphs: Ascii, format_version: 1)",
        )
        .unwrap();
        assert_eq!(
            file.load(Defaults::default()),
            LoadedOptions::Found(options)
        );
    }

    #[test]
    fn no_options_file_reads_as_missing_with_the_defaults() {
        let folder = ScratchFolder::new("options-missing");

        let loaded = folder.options_file().load(Defaults::default());

        assert_eq!(loaded, LoadedOptions::Missing);
        assert_eq!(loaded.options(Defaults::default()), Options::default());
        let french_azerty = Defaults {
            language: Language::French,
            keyboard: Keyboard::Azerty,
        };
        assert_eq!(
            loaded.options(french_azerty),
            Options {
                language: Language::French,
                keyboard: Keyboard::Azerty,
                ..Options::default()
            }
        );
    }

    #[test]
    fn a_file_written_before_the_keyboard_option_keeps_its_settings_and_the_system_layout() {
        let folder = ScratchFolder::new("options-before-keyboard");
        let file = folder.options_file();
        fs::write(
            file.path(),
            "(format_version: 1, glyphs: Ascii, colors: Auto, theme: Dark, language: French)",
        )
        .unwrap();
        for keyboard in Keyboard::ALL {
            let system = Defaults {
                language: Language::English,
                keyboard,
            };
            assert_eq!(
                file.load(system),
                LoadedOptions::Found(Options {
                    glyphs: GlyphSet::Ascii,
                    language: Language::French,
                    keyboard,
                    ..Options::default()
                }),
                "{keyboard:?}"
            );
        }
        // Once chosen, the layout is written and wins over the system's.
        let chosen = Options {
            keyboard: Keyboard::Azerty,
            ..Options::default()
        };
        file.write(&chosen).unwrap();
        assert!(
            fs::read_to_string(file.path())
                .unwrap()
                .contains("keyboard: Azerty")
        );
        assert_eq!(file.load(Defaults::default()), LoadedOptions::Found(chosen));
    }

    #[test]
    fn an_unreadable_options_file_gives_the_defaults_and_a_reason() {
        for (label, bytes) in [
            ("not-ron", b"glyphs = ascii".to_vec()),
            ("not-text", vec![0xFF, 0xFE, 0x00]),
            ("empty", Vec::new()),
            ("truncated", b"(format_version: 1, glyphs: Asc".to_vec()),
            (
                "unknown-value",
                b"(format_version: 1, glyphs: Emoji, colors: Auto, theme: Dark)".to_vec(),
            ),
            (
                "missing-field",
                b"(format_version: 1, glyphs: Ascii)".to_vec(),
            ),
        ] {
            let folder = ScratchFolder::new(label);
            let file = folder.options_file();
            fs::write(file.path(), &bytes).unwrap();

            let loaded = file.load(Defaults::default());

            let LoadedOptions::Unreadable { reason } = &loaded else {
                panic!("{label}: {loaded:?}");
            };
            assert!(
                !messages::options_problem(Language::English, reason).is_empty(),
                "{label}"
            );
            assert_eq!(
                loaded.options(Defaults::with_language(Language::French)),
                Options {
                    language: Language::French,
                    ..Options::default()
                },
                "{label}: the defaults, in the system's language"
            );
            assert_eq!(fs::read(file.path()).unwrap(), bytes, "{label}: left as is");
        }
    }

    #[test]
    fn an_options_file_of_another_version_names_both_versions() {
        let folder = ScratchFolder::new("options-other-version");
        let file = folder.options_file();
        fs::write(
            file.path(),
            "(format_version: 2, glyphs: Ascii, colors: Auto, theme: Dark, sound: On)",
        )
        .unwrap();

        let LoadedOptions::Unreadable { reason } = file.load(Defaults::default()) else {
            panic!("a version 2 file is not read");
        };

        assert_eq!(
            messages::options_problem(Language::English, &reason),
            "it has format version 2, but this game reads version 1"
        );
    }

    /// The reason given for an options file whose text is `text`, in
    /// English.
    fn reason_for(text: &str) -> String {
        let problem = parse_options(text, Defaults::default()).expect_err(text);
        messages::options_problem(Language::English, &problem)
    }

    #[test]
    fn an_unknown_value_names_the_setting_and_its_values() {
        assert_eq!(
            reason_for("(format_version: 1, glyphs: Braille, colors: Auto, theme: Dark)"),
            "line 1: unknown value `Braille` for glyphs (Unicode or Ascii)"
        );
        assert_eq!(
            reason_for("(format_version: 1, glyphs: Ascii, colors: Rainbow, theme: Dark)"),
            "line 1: unknown value `Rainbow` for colors (Auto, Ansi16 or Monochrome)"
        );
        assert_eq!(
            reason_for(
                "(\n  format_version: 1,\n  glyphs: Ascii,\n  colors: Auto,\n  theme: Sepia,\n)"
            ),
            "line 5: unknown value `Sepia` for theme (Dark or Light)"
        );
    }

    #[test]
    fn a_value_of_the_wrong_kind_names_the_setting() {
        for (text, expected) in [
            (
                "(format_version: 1, glyphs: 3, colors: Auto, theme: Dark)",
                "line 1: wrong kind of value for glyphs",
            ),
            (
                "(format_version: 1, glyphs: Ascii, colors: \"Auto\", theme: Dark)",
                "line 1: wrong kind of value for colors",
            ),
            (
                "(\n  format_version: 1,\n  glyphs: Ascii,\n  colors: Auto,\n  theme:\n    [1],\n)",
                "line 6: wrong kind of value for theme",
            ),
            (
                "(format_version: 1, glyphs: true, colors: Auto, theme: Dark)",
                "line 1: unknown value `true` for glyphs (Unicode or Ascii)",
            ),
        ] {
            assert_eq!(reason_for(text), expected, "{text}");
        }
    }

    #[test]
    fn a_missing_or_repeated_setting_is_named() {
        assert_eq!(
            reason_for("(format_version: 1, glyphs: Ascii, theme: Dark)"),
            "the `colors` setting is missing"
        );
        assert_eq!(
            reason_for(
                "(format_version: 1, glyphs: Ascii, glyphs: Ascii, colors: Auto, theme: Dark)"
            ),
            "line 1: the `glyphs` setting is given twice"
        );
    }

    #[test]
    fn a_missing_or_malformed_version_is_named_as_the_format_version() {
        for (text, expected) in [
            (
                "(glyphs: Ascii, colors: Auto, theme: Dark)",
                "the format version is missing",
            ),
            (
                "(format_version: \"one\", glyphs: Ascii, colors: Auto, theme: Dark)",
                "line 1: the format version must be a whole number",
            ),
            (
                "(format_version: -1, glyphs: Ascii, colors: Auto, theme: Dark)",
                "line 1: the format version must be a whole number",
            ),
            (
                "(format_version: 99999999999, glyphs: Ascii, colors: Auto, theme: Dark)",
                "line 1: the format version must be a whole number",
            ),
        ] {
            assert_eq!(reason_for(text), expected, "{text}");
        }
    }

    #[test]
    fn invalid_ron_gives_the_line_and_a_short_reason() {
        for (text, expected) in [
            ("", "it is empty"),
            ("  \n ", "it is empty"),
            ("42", "line 1: the settings must be inside ( and )"),
            (
                "this is not ron {{{",
                "line 1: the settings must be inside ( and )",
            ),
            (
                "Options(format_version: 1, glyphs: Ascii, colors: Auto, theme: Dark)",
                "line 1: the settings must be inside ( and )",
            ),
            (
                "(format_version: 1, glyphs: As",
                "line 1, column 29: a comma is missing",
            ),
            (
                "(format_version: 1,\n glyphs:",
                "line 2: the file ends too early",
            ),
            (
                "(format_version: 1 glyphs: Ascii)",
                "line 1, column 20: a comma is missing",
            ),
            (
                "(format_version: 1, glyphs: Ascii, colors: Auto, theme: Dark) x",
                "line 1, column 62: unexpected text after the closing )",
            ),
            ("/* x", "line 1, column 3: this is not valid RON"),
        ] {
            assert_eq!(reason_for(text), expected, "{text}");
        }
    }

    #[test]
    fn no_reason_names_the_program_s_types() {
        let internal = [
            "OptionsVersion",
            "OptionsRecord",
            "GlyphSet",
            "ColorChoice",
            "Appearance",
            "struct",
            "enum",
            "variant",
            "identifier",
            "u32",
            "Expected",
        ];
        for text in [
            "(format_version: 1, glyphs: Braille, colors: Auto, theme: Dark)",
            "(format_version: 1, glyphs: Ascii, colors: Rainbow, theme: Dark)",
            "(format_version: 1, glyphs: Ascii, colors: Auto, theme: Sepia)",
            "(format_version: 1, glyphs: 3, colors: Auto, theme: Dark)",
            "(format_version: 1, glyphs: [1], colors: Auto, theme: Dark)",
            "(format_version: 1, glyphs: Ascii(2), colors: Auto, theme: Dark)",
            "(format_version: \"one\", glyphs: Ascii, colors: Auto, theme: Dark)",
            "(format_version: 1.5, glyphs: Ascii, colors: Auto, theme: Dark)",
            "(glyphs: Ascii, colors: Auto, theme: Dark)",
            "(format_version: 1, glyphs: Ascii)",
            "(format_version: 1, format_version: 1, glyphs: Ascii, colors: Auto, theme: Dark)",
            "OptionsVersion(format_version: 1, glyphs: Ascii, colors: Auto, theme: Dark)",
            "Foo(format_version: 1)",
            "42",
            "[1, 2]",
            "{\"a\": 1}",
            "glyphs = ascii",
            "(format_version 1)",
            "(format_version: 1, glyphs: Ascii, colors: Auto, theme: Dark",
            "(format_version: 1, glyphs: Ascii, colors: Auto, theme: Dark))",
            "(format_version: 1, \"glyphs\": Ascii, colors: Auto, theme: Dark)",
        ] {
            let reason = reason_for(text);
            for word in internal {
                assert!(!reason.contains(word), "{text}: {reason}");
            }
            assert!(
                reason.starts_with("line ") || reason.starts_with("the "),
                "{text}: {reason}"
            );
        }
    }

    #[test]
    fn an_options_file_larger_than_the_limit_is_unreadable() {
        let folder = ScratchFolder::new("options-too-large");
        let file = folder.options_file();
        File::create(file.path())
            .unwrap()
            .set_len(MAX_FILE_BYTES + 1)
            .unwrap();

        assert_eq!(
            file.load(Defaults::default()),
            LoadedOptions::Unreadable {
                reason: OptionsProblem::TooLarge {
                    limit: MAX_FILE_BYTES
                }
            }
        );
    }

    #[test]
    fn a_folder_in_place_of_the_options_file_is_unreadable_not_an_error() {
        let folder = ScratchFolder::new("options-folder");
        let file = folder.options_file();
        fs::create_dir(file.path()).unwrap();

        assert!(matches!(
            file.load(Defaults::default()),
            LoadedOptions::Unreadable { .. }
        ));
    }

    #[test]
    fn a_failed_options_write_names_the_file_and_keeps_the_previous_one() {
        let folder = ScratchFolder::new("options-write-fails");
        let file = folder.options_file();
        file.write(&Options::default()).unwrap();
        fs::create_dir(with_suffix(file.path(), TEMPORARY_SUFFIX)).unwrap();
        let ascii = Options {
            glyphs: GlyphSet::Ascii,
            ..Options::default()
        };

        let error = file.write(&ascii).unwrap_err();

        let message = format!("{error:#}");
        assert!(message.contains("could not be saved"), "{message}");
        assert!(
            message.contains(&file.path().display().to_string()),
            "{message}"
        );
        assert_eq!(
            file.load(Defaults::default()),
            LoadedOptions::Found(Options::default())
        );
    }

    #[test]
    fn writing_the_options_creates_the_data_folder() {
        let folder = ScratchFolder::new("options-nested");
        let file = OptionsFile::in_folder(folder.path().join("a").join("b"));

        file.write(&Options::default()).unwrap();

        assert_eq!(
            file.load(Defaults::default()),
            LoadedOptions::Found(Options::default())
        );
    }
}
