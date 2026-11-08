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
//! leftover `save.ron.tmp` is never read. A save that cannot be read back
//! (not RON, another format version, content that changed…) is renamed
//! `save.ron.bad` rather than deleted, so the run it held can still be
//! looked at.
//!
//! The options are `options.ron` in the same folder, a short RON record
//! with its own format version ([`OPTIONS_FORMAT_VERSION`]), written the
//! same atomic way. An options file that cannot be read back never stops
//! the game: the default options are used, the reason is reported, and the
//! file is replaced the next time an option changes.
//!
//! The text of a save and its checks belong to `sigilfall_core::save`; when
//! to save, resume or delete, and when to write the options, is decided by
//! the `app` module.

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use sigilfall_core::content::Content;
use sigilfall_core::game::Game;
use sigilfall_core::save;

use crate::theme::{Appearance, ColorChoice, GlyphSet, Options};

/// Environment variable that replaces the operating system's data folder,
/// as a path to a folder, created if missing; an empty value is ignored.
///
/// An environment variable rather than a command-line option: the command
/// line stays the one the plan documents (`--seed`, `--ascii`), a test or a
/// tester can move the folder of a game started from a script without
/// changing its arguments, and the options live in the same folder, so one
/// variable moves every file of the game.
pub const DATA_DIR_VAR: &str = "SIGILFALL_DATA_DIR";

/// Application name given to `directories::ProjectDirs`, which makes the
/// last part of the data folder's path (lowercased on Linux).
const APPLICATION: &str = "Sigilfall";

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
        /// Why it could not be read, in a sentence a player can act on.
        reason: String,
        /// Where the unreadable save now is.
        moved_to: PathBuf,
    },
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
    /// Only `save.ron` is read, never a leftover `save.ron.tmp`. A save that
    /// is not UTF-8 text or that `save::from_str` refuses is renamed
    /// `save.ron.bad`, replacing an older one of that name, and reported as
    /// [`Loaded::SetAside`]; the save itself is otherwise left as it is.
    ///
    /// # Errors
    ///
    /// Returns an error naming the file when it exists but cannot be read
    /// (permissions, a folder of that name…), or when an unreadable save
    /// cannot be renamed: starting a new game then would overwrite it at the
    /// next save.
    pub fn load(&self, content: &Content) -> anyhow::Result<Loaded> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Loaded::Missing),
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "cannot read the save {}: check that it is a file you can read",
                        self.path.display()
                    )
                });
            }
        };
        let reason = match String::from_utf8(bytes) {
            Err(_) => "the save is not text".to_owned(),
            Ok(text) => match save::from_str(&text, content) {
                Ok(game) => return Ok(Loaded::Game(Box::new(game))),
                Err(error) => error.to_string(),
            },
        };
        let moved_to = self.set_aside_path();
        fs::rename(&self.path, &moved_to).with_context(|| {
            format!(
                "cannot set aside the unreadable save {} ({reason}): move or \
                 delete it, then start the game again",
                self.path.display()
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

    /// Deletes the save, so a dead character can never be resumed. Deleting
    /// a save that does not exist succeeds, so this can be repeated.
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
                        "cannot delete the save {} of the dead character: delete \
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
        /// Why, in a short sentence a player can act on.
        reason: String,
    },
}

impl LoadedOptions {
    /// Returns the options to play with: those found, or the defaults.
    pub fn options(&self) -> Options {
        match self {
            Self::Found(options) => *options,
            Self::Missing | Self::Unreadable { .. } => Options::default(),
        }
    }
}

/// The options file as written on disk: the format version first, then
/// one field per option, such as
/// `(format_version: 1, glyphs: Unicode, colors: Auto, theme: Dark)`.
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
    #[cfg(test)]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads the player's options back.
    ///
    /// Never fails: a missing file gives [`LoadedOptions::Missing`], and a
    /// file that cannot be read (permissions, not text, not RON, another
    /// format version, an unknown value) gives
    /// [`LoadedOptions::Unreadable`] with the reason. The file itself is
    /// left as it is.
    pub fn load(&self) -> LoadedOptions {
        let text = match fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return LoadedOptions::Missing,
            Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                return unreadable("it is not text");
            }
            Err(error) => return unreadable(&error.to_string()),
        };
        match parse_options(&text) {
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
fn unreadable(reason: &str) -> LoadedOptions {
    LoadedOptions::Unreadable {
        reason: reason.to_owned(),
    }
}

/// Reads the text of an options file, or returns why it cannot be, in
/// the words of the file rather than of the program: it is empty, it has
/// another format version, or what is wrong and on which line (see
/// [`describe_parse_error`]).
fn parse_options(text: &str) -> Result<Options, String> {
    if text.trim().is_empty() {
        return Err("it is empty".to_owned());
    }
    let version: OptionsVersion =
        ron::from_str(text).map_err(|error| describe_parse_error(text, &error))?;
    if version.format_version != OPTIONS_FORMAT_VERSION {
        return Err(format!(
            "it has format version {}, but this game reads version {OPTIONS_FORMAT_VERSION}",
            version.format_version
        ));
    }
    let record: OptionsRecord =
        ron::from_str(text).map_err(|error| describe_parse_error(text, &error))?;
    Ok(Options {
        glyphs: record.glyphs,
        colors: record.colors,
        appearance: record.theme,
    })
}

/// The settings of the options file: the name serde gives the type of each
/// value in its errors, and the key that names the setting in the file.
///
/// The parser reports an unknown value with the type's name (`GlyphSet`),
/// which means nothing to a player; the key (`glyphs`) is what they read in
/// the file.
const SETTING_KEYS: [(&str, &str); 3] = [
    ("GlyphSet", "glyphs"),
    ("ColorChoice", "colors"),
    ("Appearance", "theme"),
];

/// Key of the format version in the options file.
const VERSION_KEY: &str = "format_version";

/// Describes why the options file `text` could not be parsed, as `error`
/// says, in a short reason that names the line and, when it can, the
/// setting at fault in the file's own words, such as "line 1: unknown
/// value `Braille` for glyphs (Unicode or Ascii)".
///
/// The parser's own messages are never shown: they name the program's
/// types ("enum `GlyphSet`", "struct `OptionsVersion`"). An error the
/// function does not recognise gives "this is not valid RON" with its line
/// and column.
fn describe_parse_error(text: &str, error: &ron::error::SpannedError) -> String {
    use ron::error::Error;

    let at = error.span.start;
    let line = format!("line {}", at.line);
    let place = format!("line {}, column {}", at.line, at.col);
    match &error.code {
        Error::NoSuchEnumVariant {
            expected,
            found,
            outer,
        } => {
            let key = outer.as_deref().and_then(setting_key);
            match key {
                Some(key) => format!(
                    "{line}: unknown value `{found}` for {key} ({})",
                    one_of(expected)
                ),
                None => format!("{line}: unknown value `{found}`"),
            }
        }
        Error::MissingStructField { field, .. } => {
            format!("{} is missing", setting_name(field))
        }
        Error::DuplicateStructField { field, .. } => {
            format!("{line}: {} is given twice", setting_name(field))
        }
        // The format version is the only number of the file.
        Error::ExpectedInteger | Error::IntegerOutOfBounds | Error::InvalidIntegerDigit { .. } => {
            format!("{line}: the format version must be a whole number")
        }
        Error::ExpectedIdentifier
        | Error::SuggestRawIdentifier(_)
        | Error::InvalidValueForType { .. } => match key_before(text, at) {
            Some(key) => format!("{line}: wrong kind of value for {key}"),
            None => format!("{place}: wrong kind of value"),
        },
        Error::ExpectedNamedStructLike(_)
        | Error::ExpectedStructLike
        | Error::ExpectedDifferentStructName { .. } => {
            format!("{line}: the settings must be inside ( and )")
        }
        Error::Eof => format!("{line}: the file ends too early"),
        Error::ExpectedComma => format!("{place}: a comma is missing"),
        Error::ExpectedMapColon => format!("{place}: a colon is missing"),
        Error::ExpectedStructLikeEnd => format!("{place}: a closing ) is missing"),
        Error::TrailingCharacters => format!("{place}: unexpected text after the closing )"),
        Error::UnexpectedChar(character) => {
            format!("{place}: unexpected character `{character}`")
        }
        _ => format!("{place}: this is not valid RON"),
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

/// How a reason names the field `field` of the file: "the format version"
/// for [`VERSION_KEY`], "the `glyphs` setting" for a setting.
fn setting_name(field: &str) -> String {
    if field == VERSION_KEY {
        "the format version".to_owned()
    } else {
        format!("the `{field}` setting")
    }
}

/// The accepted values `values` as a reason lists them: "Unicode or Ascii",
/// "Auto, Ansi16 or Monochrome".
fn one_of(values: &[&str]) -> String {
    match values {
        [] => String::new(),
        [only] => (*only).to_owned(),
        [first @ .., last] => format!("{} or {last}", first.join(", ")),
    }
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

/// Writes `text` to `path` so that a crash leaves the old file or the new
/// one whole, never half of one: the text goes to `path` with
/// [`TEMPORARY_SUFFIX`], is flushed to the disk, and that file is renamed
/// over `path`. The folder of `path` is created if missing, and a temporary
/// file left by an interrupted write is overwritten.
///
/// # Errors
///
/// Returns an error naming the folder or the file when the folder cannot
/// be created, or the temporary file cannot be created, written or renamed.
fn write_atomically(path: &Path, text: &str) -> anyhow::Result<()> {
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)
            .with_context(|| format!("cannot create the folder {}", folder.display()))?;
    }
    let temporary = with_suffix(path, TEMPORARY_SUFFIX);
    let mut file = File::create(&temporary)
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
        _ => ProjectDirs::from("", "", APPLICATION).map(|dirs| dirs.data_dir().to_path_buf()),
    }
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
    /// Creates a new folder whose name holds `label`, the process id and a
    /// counter, so parallel tests and test runs never share one.
    ///
    /// # Panics
    ///
    /// Panics if the folder cannot be created.
    pub fn new(label: &str) -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};

        /// Number of folders created so far by this process.
        static CREATED: AtomicUsize = AtomicUsize::new(0);
        let number = CREATED.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("sigilfall-{label}-{}-{number}", std::process::id()));
        // A folder left by an earlier run with the same process id would
        // hold its files; start empty.
        drop(fs::remove_dir_all(&path));
        fs::create_dir_all(&path).expect("create a scratch folder");
        Self { path }
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

        assert!(reason.contains(&format!("version {other}")), "{reason}");
        assert_eq!(fs::read_to_string(moved_to).unwrap(), text);
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
                for appearance in [Appearance::Dark, Appearance::Light] {
                    let options = Options {
                        glyphs,
                        colors,
                        appearance,
                    };

                    file.write(&options).unwrap();

                    assert_eq!(file.load(), LoadedOptions::Found(options));
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
        };

        file.write(&options).unwrap();

        let text = fs::read_to_string(file.path()).unwrap();
        let version = text.find("format_version: 1").expect("the version");
        for field in ["glyphs: Ascii", "colors: Ansi16", "theme: Light"] {
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
        assert_eq!(file.load(), LoadedOptions::Found(options));
    }

    #[test]
    fn no_options_file_reads_as_missing_with_the_defaults() {
        let folder = ScratchFolder::new("options-missing");

        let loaded = folder.options_file().load();

        assert_eq!(loaded, LoadedOptions::Missing);
        assert_eq!(loaded.options(), Options::default());
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

            let loaded = file.load();

            let LoadedOptions::Unreadable { reason } = &loaded else {
                panic!("{label}: {loaded:?}");
            };
            assert!(!reason.is_empty(), "{label}");
            assert_eq!(loaded.options(), Options::default(), "{label}");
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

        let LoadedOptions::Unreadable { reason } = file.load() else {
            panic!("a version 2 file is not read");
        };

        assert_eq!(
            reason,
            "it has format version 2, but this game reads version 1"
        );
    }

    /// The reason given for an options file whose text is `text`.
    fn reason_for(text: &str) -> String {
        parse_options(text).expect_err(text)
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
    fn a_folder_in_place_of_the_options_file_is_unreadable_not_an_error() {
        let folder = ScratchFolder::new("options-folder");
        let file = folder.options_file();
        fs::create_dir(file.path()).unwrap();

        assert!(matches!(file.load(), LoadedOptions::Unreadable { .. }));
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
        assert_eq!(file.load(), LoadedOptions::Found(Options::default()));
    }

    #[test]
    fn writing_the_options_creates_the_data_folder() {
        let folder = ScratchFolder::new("options-nested");
        let file = OptionsFile::in_folder(folder.path().join("a").join("b"));

        file.write(&Options::default()).unwrap();

        assert_eq!(file.load(), LoadedOptions::Found(Options::default()));
    }
}
