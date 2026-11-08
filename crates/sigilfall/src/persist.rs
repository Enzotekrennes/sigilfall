//! Files the game keeps between runs: where they live, and the save of the
//! game in progress.
//!
//! Exposes [`SaveFile`], the save file of one data folder, which reads,
//! writes and deletes the save; [`Loaded`], what reading it found; and
//! [`DATA_DIR_VAR`], the environment variable that moves the data folder.
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
//! The text of a save and its checks belong to `sigilfall_core::save`; when
//! to save, resume or delete is decided by the `app` module.

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use directories::ProjectDirs;
use sigilfall_core::content::Content;
use sigilfall_core::game::Game;
use sigilfall_core::save;

/// Environment variable that replaces the operating system's data folder,
/// as a path to a folder, created if missing; an empty value is ignored.
///
/// An environment variable rather than a command-line option: the command
/// line stays the one the plan documents (`--seed`, `--ascii`), a test or a
/// tester can move the folder of a game started from a script without
/// changing its arguments, and the options of T26 will live in the same
/// folder, so one variable moves every file of the game.
pub const DATA_DIR_VAR: &str = "SIGILFALL_DATA_DIR";

/// Application name given to `directories::ProjectDirs`, which makes the
/// last part of the data folder's path (lowercased on Linux).
const APPLICATION: &str = "Sigilfall";

/// Name of the save file in the data folder.
const SAVE_NAME: &str = "save.ron";

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
        if let Some(folder) = self.path.parent() {
            fs::create_dir_all(folder)
                .with_context(|| format!("cannot create the folder {}", folder.display()))?;
        }
        let temporary = self.temporary_path();
        let mut file = File::create(&temporary)
            .with_context(|| format!("cannot create {}", temporary.display()))?;
        file.write_all(text.as_bytes())
            .and_then(|()| file.sync_all())
            .with_context(|| format!("cannot write {}", temporary.display()))?;
        drop(file);
        fs::rename(&temporary, &self.path)
            .with_context(|| format!("cannot rename {}", temporary.display()))
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
}
