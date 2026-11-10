//! The application: which screen is active, what keys do, and the event loop.
//!
//! Exposes [`App`] and [`State`]. Input handling ([`App::handle_key`]) and
//! drawing ([`App::render`]) never touch the real terminal, so tests drive
//! the whole application with simulated keys and render it into ratatui's
//! `TestBackend`. Only [`App::run`] reads the terminal; preparing and
//! restoring the terminal is left to `main`. Rendering only records the size
//! of the frame it drew, so that keys are not played on a screen too small
//! to show the game.
//!
//! The application started by `main` ([`App::launch`]) opens on the title
//! screen (`ui::title`), unless `--seed` or `--daily` asks for a game at
//! once. The title menu lists "Continue" while a game is saved, "New game",
//! "Seed", where the player types the seed of the new game, "Daily", the
//! daily dungeon of today's UTC date (`Game::daily`), "Options" and "Quit". A
//! new game that would replace a saved one asks first ([`State::Confirm`]),
//! and deletes the save once the player agrees. The options screen
//! ([`State::Options`], `ui::options`) changes the glyphs, the colours, the
//! theme, the language and the keyboard layout: each change is applied to
//! the screen at once and written to the options file
//! (`persist::OptionsFile`); `--ascii` keeps ASCII glyphs, `--lang` a
//! language and `--keyboard` a layout for the session whatever the options
//! say. Until the player chooses them, the language and the layout are the
//! system's ([`Defaults`]). Every key is read through the keymaps of the
//! layout in use (`input`), which the theme carries.
//!
//! The application keeps the message log of the game in progress: the
//! events of each action are turned into lines of the language in use by
//! the `messages` module and appended to it. The problems met with the
//! save and options files are kept as facts ([`FileNotice`]) and worded
//! when a screen shows them, so they follow a change of language. Once the game is over, its end screen
//! replaces it: the death summary (`ui::death`) once the player has died,
//! the victory screen (`ui::victory`) once it has broken the seal of the
//! bottom floor; both are [`State::GameOver`], and Enter goes back to the
//! title screen. For [`END_SCREEN_GRACE`] after the end, every key but
//! Ctrl+C is thrown away, so a key the player was already pressing (the
//! Enter of a deadly cast, a held movement key, the `>` that broke the
//! seal) never closes a summary nobody has seen. The time, and today's UTC
//! date, are read from a [`Clock`], which tests replace.
//!
//! The quit keys are `q` (`p` on AZERTY, where `q` moves), on the game, end
//! and title screens, and Ctrl+C everywhere; quitting a game saves it.
//! Escape never quits: it closes the overlays, verdicts and sub-screens of
//! the title, and does nothing on the game, end and title screens.
//!
//! Casting a sigil goes through the overlay ([`State::Casting`]): `t` opens
//! it, the digits link nodes, Backspace removes the last one, Escape closes
//! it at no cost and Enter validates. An empty trace is never submitted.
//! When the trace casts a sigil aimed in a direction, the direction is
//! asked next. Only then is the whole trace submitted to the game as one
//! `Action::Cast` (decision D19), so time passes once, for every stroke at
//! once. A pure cast closes the overlay; an unstable or fizzled one shows
//! its verdict until the next key.
//!
//! Items are picked up from the game screen (`g` or `,`) and handled in the
//! inventory ([`State::Inventory`]): `i` opens it at no cost, the arrows and
//! digits choose an item, Enter or `u` uses it, `d` drops it, Escape closes
//! it. Using or dropping submits one action for the chosen item and goes
//! back to the map, so the player sees at once what the monsters did in the
//! meantime; a refusal of the game took no time and keeps the inventory
//! open, with the reason under the list.
//!
//! Going down the stairs (`>`, or `E`) is a game action like
//! a move: the game replaces the floor and its field of view, the screen
//! draws the new one around the player with the depth on the status line,
//! and the log keeps the floors above. Off the stairs the game refuses it at
//! no cost, and the log says so. On the bottom floor the same key, on the
//! seal, breaks it and wins the run; anywhere else there it is refused the
//! same way.
//!
//! `x` explores the floor automatically: the application submits
//! `Action::Explore` again and again, drawing the screen after every step,
//! until a monster comes into view, an item or an unread glyph appears, the
//! player is hurt, nothing is left to explore, a key is pressed or
//! `exploring::MAX_EXPLORE_STEPS` steps are taken; the log says why it
//! stopped. The rules of the walk live in the `exploring` submodule.
//!
//! The launched application is tied to the player's save file
//! (`persist::SaveFile`): "Continue" resumes the saved game, the save is
//! deleted the moment the game is over (death or victory), before the end
//! screen is drawn, and [`App::save_on_exit`] writes the game in progress
//! when the event loop ends. The rules of the launch are given at
//! [`App::launch`].

pub(crate) mod exploring;

use std::cell::Cell;
use std::time::{Duration, Instant};

use anyhow::Context as _;
use ratatui::crossterm::event::{self, Event, KeyEvent};
use ratatui::layout::Size;
use ratatui::{DefaultTerminal, Frame};
use sigilfall_core::action::Action;
use sigilfall_core::content::{Content, SigilDef, Targeting};
use sigilfall_core::event::Event as GameEvent;
use sigilfall_core::game::Game;
use sigilfall_core::geometry::Direction;
use sigilfall_core::item::ItemId;
#[cfg(test)]
use sigilfall_core::rng::Seed;
use sigilfall_core::sigil::{CastQuality, ESSENCE_COST, Recognition, Trace, recognize};

use self::exploring::{EXPLORE_STEP_DELAY, Exploration, Halt, Sightings};
use crate::casting::{Casting, Drawing, Verdict};
use crate::cli::{self, Cli};
#[cfg(test)]
use crate::clock::ManualClock;
use crate::clock::{Clock, SystemClock};
use crate::input::{
    self, AimCommand, Command, EndCommand, ExploringCommand, InventoryCommand, MenuCommand,
    OptionsCommand, SeedCommand, TraceCommand, VerdictCommand,
};
use crate::keyboard::Keyboard;
use crate::language::Language;
use crate::message_log::{Message, MessageLog};
use crate::messages::{self, FileNotice, LaunchWanted};
use crate::persist::{Loaded, LoadedOptions, OptionsFile, SaveFile};
#[cfg(test)]
use crate::theme::Options;
use crate::theme::{ColorSupport, Defaults, Settings, Theme};
use crate::ui::inventory::InventoryScreen;
use crate::ui::options::{self as options_screen, OptionRow, OptionsMenu};
use crate::ui::title::{Confirmation, NewRun, SeedEntry, TitleItem, TitleMenu, TitleStep};
use crate::ui::{self, MenuScreen, Overlay};

/// How long the end screen ignores every key but Ctrl+C once the game is
/// over, by death or by victory.
///
/// The death comes from a key the player was in the middle of typing: the
/// Enter that validates a deadly trace, a movement key held down to walk
/// or to fight. Without this delay, a second Enter, or the next key typed
/// ahead, closes the summary before it was drawn long enough to be seen,
/// and the save is already gone. One second covers a reflex press (a
/// quarter of a second or so) and the auto-repeat delay of a held key
/// (about half a second by default on common systems) with margin, so a
/// finger still down when the player dies is lifted before keys count
/// again, while staying shorter than the time it takes to read the title
/// of the summary, so a player who means to leave never waits on it. Keys
/// pressed during the delay are thrown away, not kept for later.
pub const END_SCREEN_GRACE: Duration = Duration::from_secs(1);

/// The screen the application is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// The title menu, in this state: no game is being played.
    Title(TitleMenu),
    /// The title screen asks for the seed of a new game.
    SeedEntry(SeedEntry),
    /// The title screen asks whether to abandon the saved game for a new
    /// one.
    Confirm(Confirmation),
    /// The options screen, in this state.
    Options(OptionsMenu),
    /// A game is in progress and keys control the player.
    Playing,
    /// The sigil overlay is open over the game: the player traces a sigil,
    /// chooses its direction or reads the verdict of a cast.
    Casting(Casting),
    /// The inventory is open over the game: the player chooses an item to
    /// use or drop.
    Inventory(InventoryScreen),
    /// The game is over, the player having died or broken the seal: the
    /// end screen of its outcome is shown (`ui::render` sorts them on
    /// `Game::status`), the game takes no more actions, Enter goes back to
    /// the title and the quit keys work, once [`END_SCREEN_GRACE`] has
    /// passed (Ctrl+C works at once).
    GameOver,
    /// The player asked to leave; the event loop stops.
    Quit,
}

/// A game being played, with the log of what happened in it.
#[derive(Debug)]
struct Run {
    /// The game.
    game: Game,
    /// What happened so far, in words, newest last.
    log: MessageLog,
}

/// The running application: the screen shown, the game in progress if
/// any, the files kept between runs, and the theme everything is drawn
/// with.
///
/// A game is in progress (`run` is set) on [`State::Playing`],
/// [`State::Casting`], [`State::Inventory`] and [`State::GameOver`], and
/// never on the screens of the title.
#[derive(Debug)]
pub struct App {
    /// The game in progress and its log, or `None` on the title screens.
    run: Option<Run>,
    /// The game data, for new games and for the names the log and the side
    /// panel show.
    content: Content,
    /// The active screen.
    state: State,
    /// The options and what decides the theme with them.
    settings: Settings,
    /// Colours and glyphs of every screen: the theme of `settings`, or the
    /// one a test gave.
    theme: Theme,
    /// Size of the last frame drawn by [`App::render`], or `None` before the
    /// first one.
    ///
    /// A `Cell` so that rendering, which only reads the game, can still
    /// note the size it drew at.
    screen_size: Cell<Option<Size>>,
    /// The save file the game is kept in between runs, or `None` for an
    /// application that saves nothing, such as most test ones.
    save: Option<SaveFile>,
    /// The file the options are kept in, or `None` for an application that
    /// keeps none, such as most test ones.
    options_file: Option<OptionsFile>,
    /// The saved game "Continue" resumes, read at launch; `None` when no
    /// game is saved, or once it has been resumed or abandoned.
    saved: Option<Game>,
    /// Problems that open the log of the next new game, such as why the
    /// save was set aside at launch; emptied by that game.
    opening: Vec<FileNotice>,
    /// Why the options file could not be read at launch
    /// ([`FileNotice::OptionsUnreadable`]), shown on the title screen until
    /// the options are written again.
    options_unreadable: Option<FileNotice>,
    /// A problem with the save to show on the title screen: set aside at
    /// launch, or not deleted. Cleared once a game starts.
    save_notice: Option<FileNotice>,
    /// Why the options could not be written
    /// ([`FileNotice::OptionsNotSaved`]), shown on the options screen until
    /// they are written again.
    options_not_saved: Option<FileNotice>,
    /// Where the time of the end screen's grace period and today's date
    /// for the daily dungeon are read: the system's clock, or a test's.
    clock: Box<dyn Clock>,
    /// When the application reached [`State::GameOver`], read from
    /// `clock`; `None` while no game is over.
    ended_at: Option<Instant>,
    /// The walk of automatic exploration in progress on the game screen,
    /// or `None` when the player is not exploring.
    exploration: Option<Exploration>,
}

impl App {
    /// Starts the application of a run of the program, tied to the player's
    /// `save` file and to the options file beside it
    /// (`OptionsFile::beside`), for a terminal `detected` to support that
    /// much colour and a system whose language and usual keyboard layout
    /// are `system`, with the monsters, items and sigils of `content`.
    ///
    /// The theme comes from the saved options, with ASCII glyphs whatever
    /// they say when `cli` has `--ascii`, the language of `--lang` and the
    /// layout of `--keyboard` when given. Without a language or a keyboard
    /// option (no options file, or one written before the option existed),
    /// the game follows `system`. An options file that cannot be read gives
    /// the default options and a notice, never an error.
    ///
    /// Without `--seed`, the title screen opens, its menu on its first
    /// entry:
    /// - a save of a game in progress: "Continue" is listed and chosen,
    ///   and resumes the game exactly where it stopped, random stream
    ///   included, its log opening with "Welcome back"
    ///   (`messages::welcome_back`);
    /// - no save: "New game" is chosen;
    /// - a save that could not be read back (corrupt, another format
    ///   version, content changed): `SaveFile::load` set it aside, the
    ///   title says why and where it went, and the next new game's log
    ///   opens with it too (`messages::save_set_aside`);
    /// - a save of a finished game, left by a deletion that failed: it is
    ///   deleted, and "Continue" is not listed.
    ///
    /// With `--seed`, a new game of that seed starts at once, without the
    /// title, unless a game in progress is saved: the launch is then refused,
    /// so a seed typed out of habit never wipes a run (the refusal comes
    /// before the terminal changes, works the same from a script, and says
    /// how to continue or abandon the game on the title screen). `--daily`
    /// follows the same rules with the daily dungeon of today's UTC date,
    /// read from the system's clock. A save set aside or finished is handled
    /// as above, the options notice going to the log.
    ///
    /// # Errors
    ///
    /// Returns an error, before anything is drawn, when the launch is
    /// refused as above, or when the save cannot be read, set aside or
    /// deleted (see `SaveFile::load` and `SaveFile::delete`). A saved game
    /// is never overwritten by a launch.
    pub fn launch(
        detected: ColorSupport,
        system: Defaults,
        cli: &Cli,
        content: &Content,
        save: SaveFile,
    ) -> anyhow::Result<Self> {
        Self::launch_with_clock(detected, system, cli, content, save, Box::new(SystemClock))
    }

    /// [`App::launch`], reading the time and today's UTC date from `clock`
    /// instead of the system's clock, so tests choose the date of the daily
    /// dungeon and the one the title screen shows.
    ///
    /// # Errors
    ///
    /// The errors of [`App::launch`].
    pub fn launch_with_clock(
        detected: ColorSupport,
        system: Defaults,
        cli: &Cli,
        content: &Content,
        save: SaveFile,
        clock: Box<dyn Clock>,
    ) -> anyhow::Result<Self> {
        let options_file = OptionsFile::beside(&save);
        let loaded_options = options_file.load(system);
        let settings = Settings {
            options: loaded_options.options(system),
            detected,
            forced_ascii: cli.ascii,
            forced_language: cli.lang,
            forced_keyboard: cli.keyboard,
        };
        let mut app = Self::empty(settings.theme(), settings, content.clone(), clock);
        let requested = if cli.daily {
            Some(NewRun::Daily(app.clock.today()))
        } else {
            cli.seed.map(NewRun::Seed)
        };
        if let LoadedOptions::Unreadable { reason } = loaded_options {
            app.options_unreadable = Some(FileNotice::OptionsUnreadable(reason));
        }
        match save.load(content)? {
            Loaded::Game(game) if game.is_over() => save.delete()?,
            Loaded::Game(game) => {
                // The command line never asks for a random seed: only
                // `--daily` and `--seed` start a game at once.
                let refused = match requested {
                    Some(NewRun::Daily(date)) => Some(("--daily", LaunchWanted::Daily(date))),
                    Some(NewRun::Seed(seed)) => Some(("--seed", LaunchWanted::Seed(seed))),
                    Some(NewRun::Random) | None => None,
                };
                if let Some((option, wanted)) = refused {
                    anyhow::bail!(messages::launch_refused(
                        app.language(),
                        &save.path().display().to_string(),
                        option,
                        &wanted,
                    ));
                }
                app.saved = Some(*game);
            }
            Loaded::Missing => {}
            Loaded::SetAside { reason, moved_to } => {
                let notice = FileNotice::SaveSetAside {
                    reason,
                    moved_to: moved_to.display().to_string(),
                };
                app.opening.push(notice.clone());
                app.save_notice = Some(notice);
            }
        }
        app.save = Some(save);
        app.options_file = Some(options_file);
        match requested {
            Some(run) => {
                if let Some(problem) = app.options_unreadable.clone() {
                    app.opening.insert(0, problem);
                }
                app.start_new_game(run);
            }
            None => app.state = State::Title(TitleMenu::open()),
        }
        Ok(app)
    }

    /// Starts the application on a new game of the run identified by
    /// `seed`, peopled from `content`, in [`State::Playing`], drawn with
    /// `theme`, with no save file. The log opens with the welcome message.
    #[cfg(test)]
    pub fn new(theme: Theme, seed: Seed, content: &Content) -> Self {
        Self::with_game(theme, Game::new(seed, content), content.clone())
    }

    /// Starts the application on `game` instead of a new game of a seed,
    /// so tests can play in a map they know, such as the fixed room of
    /// `Game::in_fixed_room`; `content` names its monsters. No save or
    /// options file is used, and the options screen, if reached, starts
    /// from the default options on a 24-bit terminal.
    ///
    /// The clock stands still (`ManualClock`): once the player dies, keys
    /// other than Ctrl+C stay ignored until a test gives the application a
    /// clock it advances ([`App::with_clock`]).
    #[cfg(test)]
    pub fn with_game(theme: Theme, game: Game, content: Content) -> Self {
        let settings = Settings {
            options: Options {
                language: theme.language,
                keyboard: theme.keyboard,
                ..Options::default()
            },
            detected: ColorSupport::TrueColor,
            forced_ascii: false,
            forced_language: None,
            forced_keyboard: None,
        };
        let welcome = messages::welcome(theme.language);
        let mut app = Self::empty(theme, settings, content, Box::new(ManualClock::new()));
        app.begin(game, vec![welcome]);
        app
    }

    /// Makes the application read the time from `clock`, so tests decide
    /// when the end screen's grace period ([`END_SCREEN_GRACE`]) is over.
    ///
    /// Give it before the player dies: the grace period starts at the instant
    /// `clock` reads at the end of the game. On an application already on the
    /// end screen, the period restarts from `clock`'s current time.
    #[cfg(test)]
    pub fn with_clock(mut self, clock: ManualClock) -> Self {
        if self.ended_at.is_some() {
            self.ended_at = Some(clock.now());
        }
        self.clock = Box::new(clock);
        self
    }

    /// Ties the application to `save`, as [`App::launch`] does, so tests
    /// can check when an application built by [`App::with_game`] saves and
    /// deletes.
    #[cfg(test)]
    pub fn saving_to(mut self, save: SaveFile) -> Self {
        self.save = Some(save);
        self
    }

    /// An application on the title menu with no game, no file and nothing
    /// to report, drawn with `theme` for `settings`, reading the time from
    /// `clock`.
    fn empty(theme: Theme, settings: Settings, content: Content, clock: Box<dyn Clock>) -> Self {
        Self {
            run: None,
            content,
            state: State::Title(TitleMenu::open()),
            settings,
            theme,
            screen_size: Cell::new(None),
            save: None,
            options_file: None,
            saved: None,
            opening: Vec::new(),
            options_unreadable: None,
            save_notice: None,
            options_not_saved: None,
            clock,
            ended_at: None,
            exploration: None,
        }
    }

    /// Starts playing `game`, its log holding the `opening` messages: the
    /// screen is [`State::GameOver`] if the game is already over,
    /// [`State::Playing`] otherwise. The notices about the save are
    /// dropped: the player has seen them.
    fn begin(&mut self, game: Game, opening: Vec<Message>) {
        let mut log = MessageLog::default();
        log.extend(opening);
        if game.is_over() {
            self.show_end_screen();
        } else {
            self.state = State::Playing;
        }
        self.run = Some(Run { game, log });
        self.save_notice = None;
    }

    /// Moves to the end screen ([`State::GameOver`]) and starts its grace
    /// period ([`END_SCREEN_GRACE`]) at the current time of the clock.
    fn show_end_screen(&mut self) {
        self.state = State::GameOver;
        self.ended_at = Some(self.clock.now());
    }

    /// Returns whether the end screen still ignores every key but Ctrl+C:
    /// less than [`END_SCREEN_GRACE`] has passed since the game ended. `false`
    /// when no game is over.
    fn end_screen_is_settling(&self) -> bool {
        self.ended_at
            .is_some_and(|ended_at| self.clock.now().duration_since(ended_at) < END_SCREEN_GRACE)
    }

    /// Returns the language every text is written in, the theme's.
    const fn language(&self) -> Language {
        self.theme.language
    }

    /// Returns the keyboard layout every key is read for, the theme's.
    const fn keyboard(&self) -> Keyboard {
        self.theme.keyboard
    }

    /// Starts the new game `run`, its log opening with the problems
    /// waiting in `opening`, then the welcome message, in the language in
    /// use: a game on a seed drawn now (`cli::random_seed`), on the seed
    /// given, or the daily dungeon of the date given (`Game::daily`).
    fn start_new_game(&mut self, run: NewRun) {
        let language = self.language();
        let mut opening: Vec<Message> = std::mem::take(&mut self.opening)
            .iter()
            .map(|notice| messages::opening(language, notice))
            .collect();
        opening.push(messages::welcome(language));
        let game = match run {
            NewRun::Random => Game::new(cli::random_seed(), &self.content),
            NewRun::Seed(seed) => Game::new(seed, &self.content),
            NewRun::Daily(date) => Game::daily(date, &self.content),
        };
        self.begin(game, opening);
    }

    /// Returns the active screen, so tests can check it.
    #[cfg(test)]
    pub const fn state(&self) -> State {
        self.state
    }

    /// Returns the game in progress, so tests can check its state.
    ///
    /// # Panics
    ///
    /// Panics on the title screens, where no game is in progress.
    #[cfg(test)]
    pub fn game(&self) -> &Game {
        &self.run.as_ref().expect("a game is in progress").game
    }

    /// Returns the message log of the game in progress, so tests can check
    /// what it says.
    ///
    /// # Panics
    ///
    /// Panics on the title screens, where no game is in progress.
    #[cfg(test)]
    pub fn log(&self) -> &MessageLog {
        &self.run.as_ref().expect("a game is in progress").log
    }

    /// Returns whether a saved game can be resumed with "Continue", so
    /// tests can check it.
    #[cfg(test)]
    pub const fn has_saved_game(&self) -> bool {
        self.saved.is_some()
    }

    /// Returns the options in use, so tests can check them.
    #[cfg(test)]
    pub const fn options(&self) -> Options {
        self.settings.options
    }

    /// Returns the theme every screen is drawn with, so tests can check it.
    #[cfg(test)]
    pub const fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Reacts to one key event, with the keymap of the active screen
    /// (`input`): releases, repeats and unbound keys do nothing.
    ///
    /// - Title screens: see [`App::handle_title_key`],
    ///   [`App::handle_seed_key`], [`App::handle_confirm_key`] and
    ///   [`App::handle_options_key`].
    /// - Game screen (`input::command_for`): a game action is submitted to
    ///   the game, which may refuse it (a move into a wall) without any
    ///   time passing, and what happened is added to the log
    ///   (`messages::describe`). `t` opens the sigil overlay
    ///   ([`State::Casting`], see [`App::handle_casting_key`]); `i` opens
    ///   the inventory ([`State::Inventory`], see
    ///   [`App::handle_inventory_key`]); `x` starts automatic exploration
    ///   (see [`App::explore_further`]), during which any key press only
    ///   stops it ([`App::handle_exploring_key`]). If the player died or
    ///   broke the seal, the application moves to [`State::GameOver`].
    /// - End screen (`input::end_command_for`): Enter goes back to the
    ///   title screen, `q` quits, once [`END_SCREEN_GRACE`] has passed
    ///   since the game ended; Ctrl+C quits at once (see
    ///   [`App::handle_end_key`]).
    ///
    /// Once the application has quit, keys are ignored. While the last
    /// frame drawn was smaller than 80 × 24 (see `ui::fits`), the screen
    /// shows a resize message instead, so only the quit keys work. Before
    /// the first frame the size is unknown and keys are accepted: the event
    /// loop always draws before it reads a key.
    pub fn handle_key(&mut self, key: KeyEvent) {
        match self.state {
            State::Quit => {}
            State::Title(menu) => self.handle_title_key(menu, key),
            State::SeedEntry(entry) => self.handle_seed_key(entry, key),
            State::Confirm(confirmation) => self.handle_confirm_key(confirmation, key),
            State::Options(menu) => self.handle_options_key(menu, key),
            State::Casting(casting) => self.handle_casting_key(casting, key),
            State::Inventory(screen) => self.handle_inventory_key(screen, key),
            State::Playing => self.handle_game_key(key),
            State::GameOver => self.handle_end_key(key),
        }
    }

    /// Reacts to `key` on the title menu in the state `menu`
    /// (`input::title_command_for`): the arrows choose an entry, wrapping
    /// around; Enter does what it says (see [`App::choose`]); `q` and
    /// Ctrl+C quit.
    fn handle_title_key(&mut self, menu: TitleMenu, key: KeyEvent) {
        let has_save = self.saved.is_some();
        let menu = match input::title_command_for(self.keyboard(), key) {
            None => return,
            Some(MenuCommand::Quit) => {
                self.state = State::Quit;
                return;
            }
            Some(_) if !self.screen_is_visible() => return,
            Some(MenuCommand::Previous) => menu.previous(has_save),
            Some(MenuCommand::Next) => menu.next(has_save),
            Some(MenuCommand::First) => TitleMenu::open(),
            Some(MenuCommand::Last) => TitleMenu::last(has_save),
            Some(MenuCommand::Confirm) => return self.choose(menu.chosen(has_save)),
            Some(MenuCommand::Back) => menu,
        };
        self.state = State::Title(menu);
    }

    /// Does what the title menu's entry `item` says: "Continue" resumes the
    /// saved game; "New game" starts a game on a random seed and "Daily"
    /// the daily dungeon of today's UTC date (read from the clock now),
    /// both asking first when a game is saved; "Seed" opens the seed entry;
    /// "Options" opens the options screen; "Quit" quits.
    fn choose(&mut self, item: TitleItem) {
        match item {
            TitleItem::Continue => {
                if let Some(game) = self.saved.take() {
                    self.begin(game, vec![messages::welcome_back(self.language())]);
                }
            }
            TitleItem::NewGame => self.ask_new_game(NewRun::Random),
            TitleItem::Seed => self.state = State::SeedEntry(SeedEntry::open()),
            TitleItem::Daily => self.ask_new_game(NewRun::Daily(self.clock.today())),
            TitleItem::Options => self.state = State::Options(OptionsMenu::open()),
            TitleItem::Quit => self.state = State::Quit,
        }
    }

    /// Starts the new game `run`, or asks first ([`State::Confirm`]) when
    /// a saved game would be abandoned.
    fn ask_new_game(&mut self, run: NewRun) {
        if self.saved.is_some() {
            self.state = State::Confirm(Confirmation::new(run));
        } else {
            self.start_new_game(run);
        }
    }

    /// Reacts to `key` while the seed `entry` is typed
    /// (`input::seed_command_for`): characters are typed, Backspace erases
    /// the last one, Escape goes back to the menu on "Seed", Ctrl+C quits.
    /// Enter starts a game on the seed typed (asking first when a game is
    /// saved), or keeps the entry open with the reason it is not a seed.
    fn handle_seed_key(&mut self, entry: SeedEntry, key: KeyEvent) {
        let entry = match input::seed_command_for(key) {
            None => return,
            Some(SeedCommand::Quit) => {
                self.state = State::Quit;
                return;
            }
            Some(_) if !self.screen_is_visible() => return,
            Some(SeedCommand::Type(typed)) => entry.push(typed),
            Some(SeedCommand::Erase) => entry.erase(),
            Some(SeedCommand::Back) => {
                self.back_to_menu(TitleItem::Seed);
                return;
            }
            Some(SeedCommand::Submit) => match entry.submit() {
                Ok(seed) => return self.ask_new_game(NewRun::Seed(seed)),
                Err(refused) => refused,
            },
        };
        self.state = State::SeedEntry(entry);
    }

    /// Reacts to `key` on the confirmation `confirmation`
    /// (`input::confirm_command_for`): the arrows switch between "Keep"
    /// and "Abandon"; Escape, or Enter on "Keep", goes back to the menu
    /// with the saved game untouched; Enter on "Abandon" deletes the save
    /// and starts the new game (see [`App::abandon_saved_game`]); Ctrl+C
    /// quits.
    fn handle_confirm_key(&mut self, confirmation: Confirmation, key: KeyEvent) {
        let confirmation = match input::confirm_command_for(self.keyboard(), key) {
            None => return,
            Some(MenuCommand::Quit) => {
                self.state = State::Quit;
                return;
            }
            Some(_) if !self.screen_is_visible() => return,
            Some(MenuCommand::Previous | MenuCommand::Next) => confirmation.switch(),
            Some(MenuCommand::First) => confirmation.choosing(false),
            Some(MenuCommand::Last) => confirmation.choosing(true),
            Some(MenuCommand::Confirm) if confirmation.abandons() => {
                return self.abandon_saved_game(confirmation);
            }
            Some(MenuCommand::Confirm | MenuCommand::Back) => {
                self.back_to_menu(confirmation.asked_from());
                return;
            }
        };
        self.state = State::Confirm(confirmation);
    }

    /// Deletes the save and starts the new game `confirmation` asked for.
    ///
    /// When the save cannot be deleted, no game starts: the title menu
    /// comes back with the reason, so the save is never overwritten without
    /// the player knowing.
    fn abandon_saved_game(&mut self, confirmation: Confirmation) {
        if let Some(save) = &self.save
            && let Err(error) = save.delete()
        {
            self.save_notice = Some(FileNotice::SaveNotDeleted {
                path: save.path().display().to_string(),
                cause: error.root_cause().to_string(),
            });
            self.back_to_menu(confirmation.asked_from());
            return;
        }
        self.saved = None;
        self.start_new_game(confirmation.run());
    }

    /// Shows the title menu with `item` chosen.
    fn back_to_menu(&mut self, item: TitleItem) {
        self.state = State::Title(TitleMenu::on(item, self.saved.is_some()));
    }

    /// Reacts to `key` on the options screen in the state `menu`
    /// (`input::options_command_for`): the arrows choose an entry; Left
    /// and Right, Enter and Space change the chosen option (see
    /// [`App::change_option`]); Escape, or Enter on "Back", goes back to
    /// the menu on "Options"; Ctrl+C quits.
    fn handle_options_key(&mut self, menu: OptionsMenu, key: KeyEvent) {
        let menu = match input::options_command_for(self.keyboard(), key) {
            None => return,
            Some(OptionsCommand::Quit) => {
                self.state = State::Quit;
                return;
            }
            Some(_) if !self.screen_is_visible() => return,
            Some(OptionsCommand::Previous) => menu.previous(),
            Some(OptionsCommand::Next) => menu.next(),
            Some(OptionsCommand::First) => OptionsMenu::open(),
            Some(OptionsCommand::Last) => OptionsMenu::last(),
            Some(OptionsCommand::Back) => {
                self.back_to_menu(TitleItem::Options);
                return;
            }
            Some(OptionsCommand::Confirm) if menu.chosen() == OptionRow::Back => {
                self.back_to_menu(TitleItem::Options);
                return;
            }
            Some(OptionsCommand::PreviousValue) => {
                self.change_option(menu.chosen(), false);
                menu
            }
            Some(OptionsCommand::NextValue | OptionsCommand::Confirm) => {
                self.change_option(menu.chosen(), true);
                menu
            }
        };
        self.state = State::Options(menu);
    }

    /// Gives the option of `row` its next value (`forward`) or its previous
    /// one, applies it to the theme at once, and writes the options to the
    /// options file, if the application has one.
    ///
    /// A failed write does not undo the change: the options apply for this
    /// session, and the options screen says why they were not kept. A
    /// successful write clears the notices about the options file, which
    /// it has just replaced.
    fn change_option(&mut self, row: OptionRow, forward: bool) {
        let options = options_screen::change(self.settings.options, row, forward);
        if options == self.settings.options {
            return;
        }
        self.settings.options = options;
        self.theme = self.settings.theme();
        let Some(file) = &self.options_file else {
            return;
        };
        match file.write(&options) {
            Ok(()) => {
                self.options_not_saved = None;
                self.options_unreadable = None;
            }
            Err(error) => {
                self.options_not_saved = Some(FileNotice::OptionsNotSaved {
                    path: file.path().display().to_string(),
                    cause: error.root_cause().to_string(),
                });
            }
        }
    }

    /// Reacts to `key` on the game screen (see [`App::handle_key`]); while
    /// the player explores, see [`App::handle_exploring_key`] instead.
    fn handle_game_key(&mut self, key: KeyEvent) {
        if self.exploration.is_some() {
            return self.handle_exploring_key(key);
        }
        match input::command_for(self.keyboard(), key) {
            None => {}
            Some(Command::Quit) => self.state = State::Quit,
            Some(_) if !self.screen_is_visible() => {}
            Some(Command::Act(action)) => {
                self.play(action);
            }
            Some(Command::Trace) => self.state = State::Casting(Casting::start()),
            Some(Command::Inventory) => self.state = State::Inventory(InventoryScreen::open()),
            Some(Command::Explore) => self.start_exploring(),
        }
    }

    /// Reacts to `key` while the player explores automatically
    /// (`input::exploring_command_for`): any key press stops the walk, with
    /// a line in the log, and does nothing else; Ctrl+C quits. Releases,
    /// such as the one of the explore key itself, change nothing.
    fn handle_exploring_key(&mut self, key: KeyEvent) {
        match input::exploring_command_for(key) {
            None => {}
            Some(ExploringCommand::Stop) => self.halt_exploration(&Halt::Interrupted),
            Some(ExploringCommand::Quit) => {
                self.exploration = None;
                self.state = State::Quit;
            }
        }
    }

    /// Starts a walk of automatic exploration and takes its first step at
    /// once (see [`App::explore_further`]), so the key always answers.
    ///
    /// With a monster in view, the walk does not start: the log says so and
    /// no time passes, since stepping blindly next to a foe is what the
    /// walk exists to avoid.
    fn start_exploring(&mut self) {
        let Some(run) = &self.run else {
            return;
        };
        if let Some(foe) = exploring::foe_in_view(&run.game) {
            return self.halt_exploration(&Halt::FoeInView(foe));
        }
        self.exploration = Some(Exploration::default());
        self.explore_further();
    }

    /// Returns whether a walk of automatic exploration is in progress: the
    /// event loop then takes its next step ([`App::explore_further`])
    /// unless a key comes first.
    pub const fn is_exploring(&self) -> bool {
        self.exploration.is_some()
    }

    /// Takes the next step of the walk of automatic exploration in
    /// progress, if any: submits `Action::Explore` (see [`App::play`]) and
    /// stops the walk when `exploring::halt_after_step` says so, logging
    /// why (`messages::exploration_halted`).
    ///
    /// The walk also stops, before stepping, when the last frame drawn was
    /// smaller than 80 × 24, as any key would do nothing then; and it ends
    /// without a word when the player dies, the end screen saying enough.
    pub fn explore_further(&mut self) {
        let Some(walk) = self.exploration.take() else {
            return;
        };
        if !self.screen_is_visible() {
            return self.halt_exploration(&Halt::Interrupted);
        }
        let Some(run) = &self.run else {
            return;
        };
        let before = Sightings::of(&run.game);
        let hp_before = run.game.player_actor().hp();
        let events = self.play(Action::Explore);
        let Some(run) = self.run.as_ref().filter(|run| !run.game.is_over()) else {
            return;
        };
        let walk = walk.stepped();
        match exploring::halt_after_step(&events, &run.game, &before, hp_before, walk) {
            Some(halt) => self.halt_exploration(&halt),
            None => self.exploration = Some(walk),
        }
    }

    /// Ends the walk of automatic exploration, if any, and logs why it
    /// stopped, or did not start, for `halt`.
    fn halt_exploration(&mut self, halt: &Halt) {
        self.exploration = None;
        if let Some(run) = &mut self.run {
            run.log.extend(messages::exploration_halted(
                self.theme.language,
                halt,
                &self.content,
            ));
        }
    }

    /// Reacts to `key` on the end screen (`input::end_command_for`): Enter
    /// goes back to the title screen (see [`App::back_to_title`]), `q` and
    /// Ctrl+C quit.
    ///
    /// Until [`END_SCREEN_GRACE`] has passed since the end of the game, only
    /// Ctrl+C works: Enter and `q` are thrown away, and do not wait for the
    /// end of the period to act.
    fn handle_end_key(&mut self, key: KeyEvent) {
        match input::end_command_for(self.keyboard(), key) {
            None => {}
            Some(EndCommand::Interrupt) => self.state = State::Quit,
            Some(_) if self.end_screen_is_settling() => {}
            Some(EndCommand::Quit) => self.state = State::Quit,
            Some(_) if !self.screen_is_visible() => {}
            Some(EndCommand::Title) => self.back_to_title(),
        }
    }

    /// Leaves the finished game for the title menu.
    ///
    /// The save of the finished game was deleted when it ended (death or
    /// victory); if that failed, it is tried again here, and a second failure
    /// is shown on the title screen. Nothing is left to continue.
    fn back_to_title(&mut self) {
        if let Some(save) = &self.save
            && let Err(error) = save.delete()
        {
            self.save_notice = Some(FileNotice::FinishedSaveNotDeleted {
                path: save.path().display().to_string(),
                cause: error.root_cause().to_string(),
            });
        }
        self.run = None;
        self.saved = None;
        self.ended_at = None;
        self.state = State::Title(TitleMenu::open());
    }

    /// Reacts to `key` while the inventory shows `screen`
    /// (`input::inventory_command_for`).
    ///
    /// The arrows, Home, End and the digits choose an item, which takes no
    /// time; Escape closes the inventory, and no time passes either. Use
    /// and Drop submit `Action::Use` or `Action::Drop` for the chosen item
    /// (see [`App::act_on_item`]); with nothing carried they do nothing.
    /// Ctrl+C quits. Below 80 × 24 nothing else works.
    fn handle_inventory_key(&mut self, screen: InventoryScreen, key: KeyEvent) {
        let carried = self.carried();
        let screen = match input::inventory_command_for(self.keyboard(), key) {
            None => return,
            Some(InventoryCommand::Quit) => {
                self.state = State::Quit;
                return;
            }
            Some(_) if !self.screen_is_visible() => return,
            Some(InventoryCommand::Close) => {
                self.state = State::Playing;
                return;
            }
            Some(InventoryCommand::Previous) => screen.previous(carried),
            Some(InventoryCommand::Next) => screen.next(carried),
            Some(InventoryCommand::First) => InventoryScreen::open(),
            Some(InventoryCommand::Last) => InventoryScreen::last(carried),
            Some(InventoryCommand::Slot(slot)) => screen.slot(slot, carried),
            Some(InventoryCommand::Use) => return self.act_on_item(screen, Action::Use),
            Some(InventoryCommand::Drop) => return self.act_on_item(screen, Action::Drop),
        };
        self.state = State::Inventory(screen);
    }

    /// Returns how many items the player of the game in progress carries,
    /// or 0 without a game.
    fn carried(&self) -> usize {
        self.run
            .as_ref()
            .map_or(0, |run| run.game.inventory().count())
    }

    /// Submits `action` (`Action::Use` or `Action::Drop`) for the item
    /// `screen` has chosen, and logs what happened.
    ///
    /// When the action took time, the inventory closes: the player is back
    /// on the map and sees what the monsters did meanwhile, and the end
    /// screen follows if the player died. Dropping again at once could only
    /// be refused, since a cell holds one item. When the game refused the
    /// action ([`GameEvent::ItemRefused`]), no time passed and the
    /// inventory stays open on the same item with the reason under the
    /// list. With nothing carried, nothing is submitted.
    fn act_on_item(&mut self, screen: InventoryScreen, action: fn(ItemId) -> Action) {
        let carried = self.carried();
        let chosen = self.run.as_ref().and_then(|run| {
            run.game
                .inventory()
                .nth(screen.selected(carried))
                .map(|(id, _)| id)
        });
        let Some(item) = chosen else {
            return;
        };
        let events = self.play(action(item));
        if self.state == State::GameOver {
            return;
        }
        let refusal = events.iter().find_map(|event| match event {
            GameEvent::ItemRefused { item, reason } => Some((*item, *reason)),
            _ => None,
        });
        self.state = match refusal {
            Some((item, reason)) => State::Inventory(screen.refused(item, reason)),
            None => State::Playing,
        };
    }

    /// Reacts to `key` while the sigil overlay shows `casting`.
    ///
    /// - Drawing (`input::trace_command_for`): a digit links its node, or is
    ///   refused with a notice if the node is already linked; Backspace
    ///   removes the last node; Escape closes the overlay, and no time
    ///   passes; Enter validates (see [`App::validate`]).
    /// - Direction prompt (`input::aim_command_for`): a movement key casts
    ///   in its direction; Escape goes back to the trace, kept as it was.
    /// - Verdict (`input::verdict_command_for`): any key closes it and does
    ///   nothing else.
    ///
    /// Ctrl+C quits from every step. Below 80 × 24 nothing else works.
    fn handle_casting_key(&mut self, casting: Casting, key: KeyEvent) {
        let visible = self.screen_is_visible();
        match casting {
            Casting::Drawing(mut drawing) => match input::trace_command_for(self.keyboard(), key) {
                None => {}
                Some(TraceCommand::Quit) => self.state = State::Quit,
                Some(_) if !visible => {}
                Some(TraceCommand::Cancel) => self.state = State::Playing,
                Some(TraceCommand::Link(node)) => {
                    drawing.link(node);
                    self.state = State::Casting(Casting::Drawing(drawing));
                }
                Some(TraceCommand::Undo) => {
                    drawing.undo();
                    self.state = State::Casting(Casting::Drawing(drawing));
                }
                Some(TraceCommand::Cast) => self.validate(drawing),
            },
            Casting::Aiming { trace, .. } => match input::aim_command_for(self.keyboard(), key) {
                None => {}
                Some(AimCommand::Quit) => self.state = State::Quit,
                Some(_) if !visible => {}
                Some(AimCommand::Back) => {
                    self.state = State::Casting(Casting::Drawing(Drawing::of(trace)));
                }
                Some(AimCommand::Aim(direction)) => self.cast(trace, Some(direction)),
            },
            Casting::Verdict(_) => match input::verdict_command_for(key) {
                None => {}
                Some(VerdictCommand::Quit) => self.state = State::Quit,
                Some(_) if !visible => {}
                Some(VerdictCommand::Continue) => self.state = State::Playing,
            },
        }
    }

    /// Validates the trace of `drawing`, as Enter does on the overlay.
    ///
    /// An empty trace is never submitted: the overlay stays open and says
    /// so. A trace that casts a sigil of the grimoire aimed in a direction
    /// (exactly or unstably, see `sigil::recognize`) asks for the direction
    /// first, unless the player lacks the essence of a cast: then the game
    /// refuses the cast whatever the direction, so it is submitted at once
    /// and the refusal is logged. Any other trace is cast right away.
    fn validate(&mut self, mut drawing: Drawing) {
        let trace = drawing.trace();
        if trace.is_empty() {
            drawing.refuse_empty();
            self.state = State::Casting(Casting::Drawing(drawing));
            return;
        }
        let can_pay = self
            .run
            .as_ref()
            .is_some_and(|run| run.game.player_actor().essence() >= ESSENCE_COST);
        match self.aimed_sigil(&trace) {
            Some(sigil) if can_pay => self.state = State::Casting(Casting::Aiming { trace, sigil }),
            _ => self.cast(trace, None),
        }
    }

    /// Returns the position in the sigil file (`SigilDef::index`) of the
    /// sigil `trace` casts if that sigil is aimed in a direction, or `None`
    /// when it casts no sigil, one that needs no direction, or there is no
    /// game.
    ///
    /// The trace is recognised as the game will recognise it: against the
    /// player's grimoire, with `sigil::recognize`.
    fn aimed_sigil(&self, trace: &Trace) -> Option<usize> {
        let run = self.run.as_ref()?;
        match recognize(trace, run.game.grimoire().sigils()) {
            Recognition::Success(sigil) | Recognition::Unstable(sigil)
                if sigil.targeting() == Targeting::Direction =>
            {
                Some(sigil.index())
            }
            _ => None,
        }
    }

    /// Submits `trace` as `Action::Cast`, aimed in `direction` if given,
    /// logs what happened and leaves the overlay: for the game screen after
    /// a pure cast or a cast the game refused (no time passed), for the
    /// verdict after an unstable or fizzled one, for the end screen if the
    /// player died while the monsters acted.
    fn cast(&mut self, trace: Trace, direction: Option<Direction>) {
        let events = self.play(Action::Cast { trace, direction });
        if self.state == State::GameOver {
            return;
        }
        self.state = State::Playing;
        let cast = events.iter().find_map(|event| match event {
            GameEvent::SigilCast { sigil, quality } => Some((sigil.as_deref(), *quality)),
            _ => None,
        });
        let Some(run) = &self.run else {
            return;
        };
        if let Some((sigil, quality @ (CastQuality::Unstable | CastQuality::Fizzled))) = cast {
            let known = run.game.grimoire().sigils();
            let sigil = sigil.and_then(|id| known.iter().find(|known| known.id() == id));
            let sigil = sigil.map(SigilDef::index);
            self.state = State::Casting(Casting::Verdict(Verdict {
                trace,
                quality,
                sigil,
                unknown: events.contains(&GameEvent::UnknownPattern),
            }));
        }
    }

    /// Submits the player's `action` to the game in progress, adds what
    /// happened to the log, moves to [`State::GameOver`] if the game is
    /// over (the player died or broke the seal), and returns the events;
    /// without a game, does nothing.
    ///
    /// The end of the game deletes the save at once, before the end screen
    /// is drawn, so a finished run cannot be resumed even if the game is
    /// killed rather than quit.
    fn play(&mut self, action: Action) -> Vec<GameEvent> {
        let Some(run) = &mut self.run else {
            return Vec::new();
        };
        let events = run.game.submit(action);
        run.log.extend(messages::describe(
            self.theme.language,
            &events,
            &run.game,
            &self.content,
        ));
        if run.game.is_over() {
            self.show_end_screen();
            self.delete_save();
        }
        events
    }

    /// Deletes the save of the finished game (the player died or broke the
    /// seal), if the application has one.
    ///
    /// A failure is not lost: the log says so, and [`App::save_on_exit`]
    /// (or going back to the title) tries again and reports a second
    /// failure.
    fn delete_save(&mut self) {
        let Some(save) = &self.save else {
            return;
        };
        if save.delete().is_err()
            && let Some(run) = &mut self.run
        {
            run.log
                .push(messages::save_not_deleted(self.theme.language));
        }
    }

    /// Keeps the run between launches, once the event loop has ended: the
    /// game in progress is written to the save file (`SaveFile::write`),
    /// and the save of a finished game is deleted if it is still there, so
    /// quitting from the end screen never brings a save back. Does nothing
    /// for an application without a save file, nor on the title screens,
    /// where the saved game, if any, is left as it is.
    ///
    /// Meant to be called once, after [`App::run`] returned, whatever it
    /// returned: a game whose screen failed is still saved.
    ///
    /// # Errors
    ///
    /// Returns the error of `SaveFile::write` or `SaveFile::delete`, which
    /// says which file failed and what to check; the game in progress is then
    /// not saved, and the caller must tell the player.
    pub fn save_on_exit(&self) -> anyhow::Result<()> {
        let (Some(save), Some(run)) = (&self.save, &self.run) else {
            return Ok(());
        };
        if run.game.is_over() {
            save.delete()
        } else {
            save.write(&run.game)
        }
    }

    /// Draws the active screen over the whole frame, with the application's
    /// theme: the title screen, its seed entry or confirmation, or the
    /// options screen before a game; the game screen with its log, under
    /// the sigil overlay while the player casts or under the inventory
    /// while it is open; or the end screen once the game is over.
    ///
    /// Any frame size is accepted: below 80 × 24 a message asks for a larger
    /// terminal instead (see `ui::render`). The frame's size is recorded, and
    /// decides whether [`App::handle_key`] acts on keys.
    pub fn render(&self, frame: &mut Frame) {
        self.screen_size.set(Some(frame.area().as_size()));
        let step = match &self.state {
            State::Title(menu) => Some(TitleStep::Menu(menu)),
            State::SeedEntry(entry) => Some(TitleStep::Seed(entry)),
            State::Confirm(confirmation) => Some(TitleStep::Confirm(confirmation)),
            State::Options(menu) => {
                let notice = self
                    .options_not_saved
                    .as_ref()
                    .map(|notice| messages::file_notice(self.language(), notice));
                let screen = MenuScreen::Options {
                    menu,
                    settings: &self.settings,
                    notice: notice.as_deref(),
                };
                return ui::render_menu(frame, screen, &self.theme);
            }
            State::Playing
            | State::Casting(_)
            | State::Inventory(_)
            | State::GameOver
            | State::Quit => None,
        };
        let (Some(run), None) = (&self.run, step) else {
            let menu = TitleMenu::open();
            return self.render_title(frame, step.unwrap_or(TitleStep::Menu(&menu)));
        };
        let overlay = match &self.state {
            State::Casting(casting) => Some(Overlay::Casting(casting)),
            State::Inventory(screen) => Some(Overlay::Inventory(screen)),
            _ => None,
        };
        ui::render(
            frame,
            &run.game,
            &run.log,
            &self.content,
            overlay,
            &self.theme,
        );
    }

    /// Draws the title screen at `step`, with the saved game beside
    /// "Continue", today's UTC date (read from the clock) beside "Daily",
    /// and the notices about the files, in the language in use.
    fn render_title(&self, frame: &mut Frame, step: TitleStep<'_>) {
        let (separator, language) = (self.theme.glyphs.separator, self.language());
        let saved = self.saved.as_ref().map(|game| {
            messages::saved_run(language, game.depth(), game.player_turns() + 1, separator)
        });
        let notices: Vec<String> = [&self.save_notice, &self.options_unreadable]
            .into_iter()
            .flatten()
            .map(|notice| messages::file_notice(language, notice))
            .collect();
        let screen = MenuScreen::Title {
            step,
            saved: saved.as_deref(),
            today: self.clock.today(),
            notices: &notices,
        };
        ui::render_menu(frame, screen, &self.theme);
    }

    /// Returns whether the screen is shown: `true` when the last frame
    /// drawn was at least 80 × 24, or when no frame has been drawn yet.
    fn screen_is_visible(&self) -> bool {
        self.screen_size.get().is_none_or(ui::fits)
    }

    /// Runs the event loop on `terminal` until the player quits, from any
    /// screen: draw, wait for the next terminal event, handle it, repeat.
    ///
    /// While the player explores automatically, the loop waits
    /// `exploring::EXPLORE_STEP_DELAY` at most: the next step is taken
    /// ([`App::explore_further`]) and drawn if no event came, so every step
    /// is seen and any key press stops the walk.
    ///
    /// A resize event needs no handling of its own: the next draw adapts to
    /// the new size and records it before any further key is read.
    ///
    /// # Errors
    ///
    /// Returns an error if drawing or reading terminal input fails. The
    /// terminal is left as it is; restoring it is the caller's job.
    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> anyhow::Result<()> {
        while self.state != State::Quit {
            terminal
                .draw(|frame| self.render(frame))
                .context("cannot draw the game screen on the terminal")?;
            // While exploring, the next step comes after a short wait
            // unless an event comes first; otherwise the loop waits for one.
            let event_ready = !self.is_exploring()
                || event::poll(EXPLORE_STEP_DELAY).context("cannot read terminal input")?;
            if !event_ready {
                self.explore_further();
                continue;
            }
            if let Event::Key(key) = event::read().context("cannot read terminal input")? {
                self.handle_key(key);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
