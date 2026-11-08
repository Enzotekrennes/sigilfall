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
//! daily dungeon of today's UTC date (`Game::daily`), "Options" and "Quit".
//! A new game that would replace a saved one asks first
//! ([`State::Confirm`]), and deletes the save once the player agrees. The
//! options screen ([`State::Options`], `ui::options`) changes the glyphs,
//! the colours and the theme: each change is applied to the screen at
//! once and written to the options file (`persist::OptionsFile`); `--ascii` keeps ASCII glyphs
//! for the session whatever the option says.
//!
//! The application keeps the message log of the game in progress: the
//! events of each action are turned into English lines by the `messages`
//! module and appended to it. Once the game is over, its end screen
//! replaces it: the death summary (`ui::death`) once the player has died,
//! the victory screen (`ui::victory`) once it has broken the seal of the
//! bottom floor; both are [`State::GameOver`], and Enter goes back to the
//! title screen. For [`END_SCREEN_GRACE`] after the end, every key but
//! Ctrl+C is thrown away, so a key the player was already pressing (the
//! Enter of a deadly cast, a held movement key, the `>` that broke the
//! seal) never closes a summary nobody has seen. The time, and today's UTC date, are read from a [`Clock`], which
//! tests replace.
//!
//! The quit keys are `q`, on the game, end and title screens, and Ctrl+C
//! everywhere; quitting a game saves it. Escape never quits: it closes the
//! overlays, verdicts and sub-screens of the title, and does nothing on the
//! game, end and title screens.
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
//! Going down the stairs (`>`, or `D`) is a game action like a move: the
//! game replaces the floor and its field of view, the screen draws the new
//! one around the player with the depth on the status line, and the log
//! keeps the floors above. Off the stairs the game refuses it at no cost,
//! and the log says so. On the bottom floor the same key, on the seal,
//! breaks it and wins the run; anywhere else there it is refused the same
//! way.
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
//! screen is drawn, and
//! [`App::save_on_exit`] writes the game in progress when the event loop
//! ends. The rules of the launch are given at [`App::launch`].

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
use crate::message_log::{Message, MessageLog, Tone};
use crate::messages;
use crate::persist::{Loaded, LoadedOptions, OptionsFile, SaveFile};
#[cfg(test)]
use crate::theme::Options;
use crate::theme::{ColorSupport, Settings, Theme};
use crate::ui::inventory::InventoryScreen;
use crate::ui::options::{self as options_screen, OptionRow, OptionsMenu};
use crate::ui::title::{Confirmation, NewRun, SeedEntry, TitleItem, TitleMenu, TitleStep};
use crate::ui::{self, MenuScreen, Overlay};

/// How long the end screen ignores every key but Ctrl+C once the game is
/// over, by death or by victory (fix B1 of the F9 test).
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
    /// Messages that open the next new game, such as why the save was set
    /// aside at launch; emptied by that game.
    opening: Vec<Message>,
    /// Why the options file could not be read at launch, shown on the title
    /// screen until the options are written again.
    options_unreadable: Option<String>,
    /// A problem with the save to show on the title screen: set aside at
    /// launch, or not deleted. Cleared once a game starts.
    save_notice: Option<String>,
    /// Why the options could not be written, shown on the options screen
    /// until they are written again.
    options_not_saved: Option<String>,
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
    /// much colour, with the monsters, items and sigils of `content`.
    ///
    /// The theme comes from the saved options, with ASCII glyphs whatever
    /// they say when `cli` has `--ascii`. An options file that cannot be
    /// read gives the default options and a notice, never an error.
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
    /// title, unless a game in progress is saved: the launch is then
    /// refused, so a seed typed out of habit never wipes a run (decision of
    /// T24, kept by T26: the refusal comes before the terminal changes,
    /// works the same from a script, and says how to continue or abandon
    /// the game on the title screen). `--daily` follows the same rules with
    /// the daily dungeon of today's UTC date, read from the system's clock.
    /// A save set aside or finished is handled as above, the options notice
    /// going to the log.
    ///
    /// # Errors
    ///
    /// Returns an error, before anything is drawn, when the launch is
    /// refused as above, or when the save cannot be read, set aside or
    /// deleted (see `SaveFile::load` and `SaveFile::delete`). A saved game
    /// is never overwritten by a launch.
    pub fn launch(
        detected: ColorSupport,
        cli: &Cli,
        content: &Content,
        save: SaveFile,
    ) -> anyhow::Result<Self> {
        Self::launch_with_clock(detected, cli, content, save, Box::new(SystemClock))
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
        cli: &Cli,
        content: &Content,
        save: SaveFile,
        clock: Box<dyn Clock>,
    ) -> anyhow::Result<Self> {
        let options_file = OptionsFile::beside(&save);
        let loaded_options = options_file.load();
        let settings = Settings {
            options: loaded_options.options(),
            detected,
            forced_ascii: cli.ascii,
        };
        let mut app = Self::empty(settings.theme(), settings, content.clone(), clock);
        let requested = if cli.daily {
            Some(NewRun::Daily(app.clock.today()))
        } else {
            cli.seed.map(NewRun::Seed)
        };
        if let LoadedOptions::Unreadable { reason } = loaded_options {
            app.options_unreadable = Some(messages::options_unreadable(&reason));
        }
        match save.load(content)? {
            Loaded::Game(game) if game.is_over() => save.delete()?,
            Loaded::Game(game) => {
                if let Some(run) = requested {
                    let (option, wanted) = match run {
                        NewRun::Daily(date) => ("--daily", format!("the daily dungeon of {date}")),
                        NewRun::Seed(seed) => ("--seed", format!("the seed {seed}")),
                        // The command line never asks for a random seed;
                        // kept so the match stays whole.
                        NewRun::Random => ("an option", "a new game".to_owned()),
                    };
                    anyhow::bail!(
                        "a game in progress is saved in {}: start the game without \
                         {option} to continue it, or to abandon it with New game, \
                         Seed or Daily on the title screen; you can also delete \
                         that file to abandon it and play {wanted}",
                        save.path().display()
                    );
                }
                app.saved = Some(*game);
            }
            Loaded::Missing => {}
            Loaded::SetAside { reason, moved_to } => {
                let moved_to = moved_to.display().to_string();
                app.opening
                    .push(messages::save_set_aside(&reason, &moved_to));
                app.save_notice = Some(messages::save_set_aside_notice(&reason, &moved_to));
            }
        }
        app.save = Some(save);
        app.options_file = Some(options_file);
        match requested {
            Some(run) => {
                if let Some(problem) = app.options_unreadable.clone() {
                    app.opening.insert(0, Message::new(Tone::Danger, problem));
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
            options: Options::default(),
            detected: ColorSupport::TrueColor,
            forced_ascii: false,
        };
        let mut app = Self::empty(theme, settings, content, Box::new(ManualClock::new()));
        app.begin(game, vec![messages::welcome()]);
        app
    }

    /// Makes the application read the time from `clock`, so tests decide
    /// when the end screen's grace period ([`END_SCREEN_GRACE`]) is over.
    ///
    /// Give it before the player dies: the grace period starts at the
    /// instant `clock` reads at the end of the game. On an application already on
    /// the end screen, the period restarts from `clock`'s current time.
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

    /// Starts the new game `run`, its log opening with the messages
    /// waiting in `opening`, then the welcome message: a game on a seed
    /// drawn now (`cli::random_seed`), on the seed given, or the daily
    /// dungeon of the date given (`Game::daily`).
    fn start_new_game(&mut self, run: NewRun) {
        let mut opening = std::mem::take(&mut self.opening);
        opening.push(messages::welcome());
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
        let menu = match input::title_command_for(key) {
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
                    self.begin(game, vec![messages::welcome_back()]);
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
        let confirmation = match input::confirm_command_for(key) {
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
            self.save_notice = Some(messages::save_not_deleted_notice(
                &save.path().display().to_string(),
                &error.root_cause().to_string(),
            ));
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
        let menu = match input::options_command_for(key) {
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
                self.options_not_saved = Some(messages::options_not_saved(&format!("{error:#}")));
            }
        }
    }

    /// Reacts to `key` on the game screen (see [`App::handle_key`]); while
    /// the player explores, see [`App::handle_exploring_key`] instead.
    fn handle_game_key(&mut self, key: KeyEvent) {
        if self.exploration.is_some() {
            return self.handle_exploring_key(key);
        }
        match input::command_for(key) {
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
            run.log
                .extend(messages::exploration_halted(halt, &self.content));
        }
    }

    /// Reacts to `key` on the end screen (`input::end_command_for`): Enter
    /// goes back to the title screen (see [`App::back_to_title`]), `q` and
    /// Ctrl+C quit.
    ///
    /// Until [`END_SCREEN_GRACE`] has passed since the end of the game,
    /// only Ctrl+C works: Enter and `q` are thrown away, and do not wait for the end of
    /// the period to act.
    fn handle_end_key(&mut self, key: KeyEvent) {
        match input::end_command_for(key) {
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
    /// victory); if that failed, it is tried again here, and a second failure is shown on
    /// the title screen. Nothing is left to continue.
    fn back_to_title(&mut self) {
        if let Some(save) = &self.save
            && let Err(error) = save.delete()
        {
            self.save_notice = Some(messages::finished_save_not_deleted_notice(
                &save.path().display().to_string(),
                &error.root_cause().to_string(),
            ));
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
        let screen = match input::inventory_command_for(key) {
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
            Casting::Drawing(mut drawing) => match input::trace_command_for(key) {
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
            Casting::Aiming { trace, .. } => match input::aim_command_for(key) {
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
        run.log
            .extend(messages::describe(&events, &run.game, &self.content));
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
            run.log.push(messages::save_not_deleted());
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
                let screen = MenuScreen::Options {
                    menu,
                    settings: &self.settings,
                    notice: self.options_not_saved.as_deref(),
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
    /// and the notices about the files.
    fn render_title(&self, frame: &mut Frame, step: TitleStep<'_>) {
        let separator = self.theme.glyphs.separator;
        let saved = self
            .saved
            .as_ref()
            .map(|game| messages::saved_run(game.depth(), game.player_turns() + 1, separator));
        let notices: Vec<String> = [&self.save_notice, &self.options_unreadable]
            .into_iter()
            .flatten()
            .cloned()
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
mod tests {
    //! Test harness: drives [`App`] with simulated keys and renders it into
    //! a `TestBackend`, without any real terminal.

    use insta::assert_snapshot;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
    use ratatui::layout::Rect;
    use sigilfall_core::game::PLAYER_START;
    use sigilfall_core::generation::{self, FIRST_DEPTH};
    use sigilfall_core::geometry::{Direction, Position};
    use sigilfall_core::path::DijkstraMap;
    use sigilfall_core::rng::Seed;

    use super::*;
    use crate::theme::{ColorMode, GlyphSet};
    use crate::ui::camera::Camera;

    mod daily_run;
    mod death_summary;
    mod demo_tape;
    mod exploration;
    mod inventory_screen;
    mod options_screen;
    mod save_files;
    mod title_screen;
    mod victory_screen;

    /// Seed of every test game: its written form, `7F3A-91C2-0B5E-D4A8`,
    /// is the one of the game screen mock-up in `docs/design.md`.
    const SEED: Seed = Seed::new(0x7F3A_91C2_0B5E_D4A8);

    /// A new application on [`SEED`] in the fixed room, drawn in the
    /// default mode of Windows Terminal: dark 24-bit colours and Unicode
    /// glyphs.
    ///
    /// The tests play in the fixed room, whose walls and start never change,
    /// rather than on the generated floor of the seed.
    fn new_app() -> App {
        fixed_room_app(
            Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode),
            SEED,
        )
    }

    /// A new application on `seed` in the fixed room, drawn with `theme`.
    fn fixed_room_app(theme: Theme, seed: Seed) -> App {
        App::with_game(theme, Game::in_fixed_room(seed), shipped_content())
    }

    /// The game's own monsters, for their names.
    fn shipped_content() -> Content {
        Content::load().expect("the shipped content is valid")
    }

    /// A key press of `code`, as crossterm reports it.
    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// The release of `code`, which crossterm also reports on Windows.
    fn release(code: KeyCode) -> KeyEvent {
        KeyEvent::new_with_kind(code, KeyModifiers::NONE, KeyEventKind::Release)
    }

    /// Sends a press of each key in `codes` to `app`, in order.
    fn press_all(app: &mut App, codes: &[KeyCode]) {
        for &code in codes {
            app.handle_key(press(code));
        }
    }

    /// The player's current cell.
    fn player_position(app: &App) -> Position {
        app.game().player_actor().position()
    }

    /// The camera of `app`'s game screen on a `width` × `height` terminal,
    /// placed as the screen places it.
    fn camera(app: &App, width: u16, height: u16) -> Camera {
        let map_area = ui::game::map_area(Rect::new(0, 0, width, height));
        Camera::new(map_area, app.game().map(), player_position(app))
    }

    /// Renders `app` once into a new `width` × `height` test terminal.
    fn render(app: &App, width: u16, height: u16) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
        terminal
    }

    #[test]
    fn new_game_shows_the_room_and_the_player() {
        let app = new_app();

        assert_eq!(app.state(), State::Playing);
        assert_snapshot!(render(&app, 80, 24).backend());
    }

    #[test]
    fn ascii_glyphs_replace_every_unicode_glyph() {
        let app = fixed_room_app(Theme::new(ColorMode::TrueColorDark, GlyphSet::Ascii), SEED);

        let text = render(&app, 80, 24).backend().to_string();

        assert!(text.is_ascii(), "{text}");
        assert_snapshot!(text);
    }

    #[test]
    fn movement_keys_move_the_player_one_cell_each() {
        let mut app = new_app();
        let ticks = app.game().ticks();

        // Arrow, keypad digit, vi key, then a keypad diagonal without Num Lock.
        press_all(
            &mut app,
            &[
                KeyCode::Right,
                KeyCode::Char('2'),
                KeyCode::Char('l'),
                KeyCode::PageDown,
            ],
        );

        let expected = PLAYER_START
            .step(Direction::East)
            .step(Direction::South)
            .step(Direction::East)
            .step(Direction::SouthEast);
        assert_eq!(player_position(&app), expected);
        assert_eq!(app.game().ticks(), ticks + 4, "each move took one turn");
        assert_snapshot!(render(&app, 80, 24).backend());
    }

    #[test]
    fn waiting_lets_time_pass_without_moving() {
        let mut app = new_app();
        let ticks = app.game().ticks();

        press_all(&mut app, &[KeyCode::Char('.'), KeyCode::Char('5')]);

        assert_eq!(player_position(&app), PLAYER_START);
        assert_eq!(app.game().ticks(), ticks + 2);
    }

    #[test]
    fn wall_stops_the_player_and_no_time_passes() {
        let mut app = new_app();
        // The start is four cells right of the floor next to the left wall.
        press_all(&mut app, &[KeyCode::Char('h'); 4]);
        let at_wall = Position::new(1, PLAYER_START.y);
        assert_eq!(player_position(&app), at_wall);
        let ticks = app.game().ticks();

        press_all(&mut app, &[KeyCode::Left, KeyCode::Home, KeyCode::End]);

        assert_eq!(player_position(&app), at_wall);
        assert_eq!(app.game().ticks(), ticks, "blocked moves take no time");
        assert_eq!(app.state(), State::Playing);
    }

    #[test]
    fn press_then_release_counts_as_one_move() {
        let mut app = new_app();
        let ticks = app.game().ticks();

        app.handle_key(press(KeyCode::Right));
        app.handle_key(release(KeyCode::Right));

        assert_eq!(player_position(&app), PLAYER_START.step(Direction::East));
        assert_eq!(app.game().ticks(), ticks + 1);
    }

    #[test]
    fn q_quits() {
        let mut app = new_app();

        app.handle_key(press(KeyCode::Char('q')));

        assert_eq!(app.state(), State::Quit);
    }

    #[test]
    fn q_then_later_keys_are_ignored() {
        let mut app = new_app();

        press_all(&mut app, &[KeyCode::Char('q'), KeyCode::Right]);

        assert_eq!(app.state(), State::Quit);
        assert_eq!(player_position(&app), PLAYER_START);
    }

    #[test]
    fn escape_on_the_game_screen_does_nothing_and_passes_no_time() {
        let mut app = new_app();
        let ticks = app.game().ticks();
        let turns = app.game().player_turns();
        let messages = app.log().len();
        let before = render(&app, 80, 24).backend().buffer().clone();

        // Twice, as a player closing an overlay then pressing once more.
        press_all(&mut app, &[KeyCode::Esc, KeyCode::Esc]);

        assert_eq!(app.state(), State::Playing);
        assert_eq!(app.game().ticks(), ticks, "no time passes");
        assert_eq!(app.game().player_turns(), turns);
        assert_eq!(player_position(&app), PLAYER_START);
        assert_eq!(app.log().len(), messages, "nothing is logged");
        assert_eq!(render(&app, 80, 24).backend().buffer(), &before);
        // The game still answers.
        app.handle_key(press(KeyCode::Right));
        assert_eq!(player_position(&app), PLAYER_START.step(Direction::East));
    }

    #[test]
    fn releasing_q_does_not_quit() {
        let mut app = new_app();

        app.handle_key(release(KeyCode::Char('q')));

        assert_eq!(app.state(), State::Playing);
    }

    #[test]
    fn terminal_smaller_than_80_by_24_shows_a_resize_message() {
        let app = new_app();

        assert_snapshot!(render(&app, 60, 20).backend());
        for (width, height) in [(79, 24), (80, 23)] {
            let text = render(&app, width, height).backend().to_string();
            assert!(
                text.contains("Enlarge the terminal"),
                "{width} × {height}:\n{text}"
            );
            assert!(!text.contains('@'), "{width} × {height}:\n{text}");
        }
    }

    /// The text of row `y` of `buffer`, one symbol per cell.
    fn row_text(buffer: &Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect()
    }

    /// A new application on the generated first floor of [`SEED`], in the
    /// default mode of Windows Terminal (see [`new_app`]), without its
    /// monsters: these tests walk the floor, and a monster in the way would
    /// refuse the player's moves.
    fn generated_floor_app() -> App {
        App::with_game(
            Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode),
            Game::on_floor(SEED, generation::generate(SEED, FIRST_DEPTH)),
            shipped_content(),
        )
    }

    /// The vi key that moves the player one cell in `direction`.
    const fn vi_key(direction: Direction) -> KeyCode {
        KeyCode::Char(match direction {
            Direction::North => 'k',
            Direction::NorthEast => 'u',
            Direction::East => 'l',
            Direction::SouthEast => 'n',
            Direction::South => 'j',
            Direction::SouthWest => 'b',
            Direction::West => 'h',
            Direction::NorthWest => 'y',
        })
    }

    #[test]
    fn a_generated_floor_is_shown_around_the_player_at_the_start() {
        let app = generated_floor_app();
        let floor = generation::generate(SEED, FIRST_DEPTH);
        assert_eq!(player_position(&app), floor.start());

        let terminal = render(&app, 80, 24);

        let player = camera(&app, 80, 24).screen_position(floor.start());
        assert_eq!(
            player.map(|cell| terminal.backend().buffer()[cell].symbol()),
            Some("@")
        );
        assert_snapshot!(terminal.backend());
    }

    #[test]
    fn walking_across_the_floor_scrolls_the_view_and_keeps_the_player_in_it() {
        // The stairs are the cell farthest from the start, so walking to
        // them crosses the floor. Stop two steps short so they stay visible.
        let mut app = generated_floor_app();
        let floor = generation::generate(SEED, FIRST_DEPTH);
        let to_stairs = DijkstraMap::new(floor.map(), [floor.stairs()]);
        let start_origin = camera(&app, 80, 24).origin();
        let mut origins = vec![start_origin];

        while to_stairs.distance(player_position(&app)) > Some(2) {
            let before = to_stairs.distance(player_position(&app));
            let direction = to_stairs.downhill(player_position(&app)).unwrap();
            app.handle_key(press(vi_key(direction)));
            // Each step must bring the player closer, which also bounds the
            // loop: a refused move fails here instead of looping forever.
            assert!(
                to_stairs.distance(player_position(&app)) < before,
                "the move {direction:?} was refused"
            );
            let buffer = render(&app, 80, 24).backend().buffer().clone();
            let camera = camera(&app, 80, 24);
            let player = camera.screen_position(player_position(&app));
            assert_eq!(
                player.map(|cell| buffer[cell].symbol()),
                Some("@"),
                "{camera:?}"
            );
            origins.push(camera.origin());
        }

        origins.dedup();
        assert!(origins.len() > 1, "the view scrolled: {origins:?}");
        assert_ne!(camera(&app, 80, 24).origin(), start_origin);
        assert_snapshot!(render(&app, 80, 24).backend());
    }

    #[test]
    fn a_120_by_40_terminal_centres_the_floor_across_and_crops_it_down_the_rows() {
        // The 98 × 32 map area is wider than the 80 × 40 floor but shorter.
        let app = generated_floor_app();

        let terminal = render(&app, 120, 40);

        let origin = camera(&app, 120, 40).origin();
        assert_eq!(origin.x, -9, "9 spare columns on each side");
        assert!((0..=40 - 32).contains(&origin.y), "{origin:?}");
        let first_line = row_text(terminal.backend().buffer(), 0);
        assert!(
            first_line.ends_with(" Seed 7F3A-91C2-0B5E-D4A8 "),
            "{first_line:?}"
        );
        assert_snapshot!(terminal.backend());
    }

    #[test]
    fn resize_message_uses_the_ascii_dimension_sign() {
        let app = fixed_room_app(Theme::new(ColorMode::Ansi16, GlyphSet::Ascii), SEED);

        let text = render(&app, 60, 20).backend().to_string();

        assert!(text.contains("80 x 24 (current: 60 x 20)"), "{text}");
    }

    #[test]
    fn resizing_switches_between_message_and_game_without_panicking() {
        let app = new_app();
        let mut terminal = render(&app, 80, 24);

        for (width, height) in [(40, 10), (1, 1), (79, 30), (80, 24), (120, 40)] {
            terminal.backend_mut().resize(width, height);
            terminal.draw(|frame| app.render(frame)).unwrap();
            let text = terminal.backend().to_string();
            let fits = width >= ui::MIN_WIDTH && height >= ui::MIN_HEIGHT;
            assert_eq!(text.contains('@'), fits, "{width} × {height}:\n{text}");
        }
    }

    /// Going down the stairs (T22b), driven by keys like a player on the
    /// bare floors of [`SEED`] (see [`generated_floor_app`]), where no
    /// monster stands in the way, and read back from the rendered screen.
    mod descent {
        use sigilfall_core::map::Tile;

        use super::*;

        /// The cell of the stairs down of the floor `app`'s player is on.
        fn stairs(app: &App) -> Position {
            let map = app.game().map();
            let width = i32::try_from(map.width()).unwrap();
            let height = i32::try_from(map.height()).unwrap();
            (0..height)
                .flat_map(|y| (0..width).map(move |x| Position::new(x, y)))
                .find(|&cell| map.tile(cell) == Some(Tile::StairsDown))
                .expect("every generated floor has stairs")
        }

        /// Walks `app`'s player with the vi keys along a shortest path until
        /// it is `distance` steps from the stairs of its floor, checking
        /// each step brings it closer (which also bounds the loop).
        fn walk_near_stairs(app: &mut App, distance: u32) {
            // The distances borrow a map: a copy leaves `app` free to move.
            let map = app.game().map().clone();
            let to_stairs = DijkstraMap::new(&map, [stairs(app)]);
            while to_stairs.distance(player_position(app)) > Some(distance) {
                let before = to_stairs.distance(player_position(app));
                let direction = to_stairs.downhill(player_position(app)).unwrap();
                app.handle_key(press(vi_key(direction)));
                assert!(
                    to_stairs.distance(player_position(app)) < before,
                    "the move {direction:?} was refused"
                );
            }
        }

        /// The first row of `app`'s screen, trimmed: "Depth 2 · Turn 40 …".
        fn status(app: &App) -> String {
            row_text(render(app, 80, 24).backend().buffer(), 0)
                .trim()
                .to_owned()
        }

        /// The texts of `app`'s log, oldest first.
        fn log_texts(app: &App) -> Vec<String> {
            let mut texts: Vec<_> = app
                .log()
                .newest_first()
                .map(|message| message.text.clone())
                .collect();
            texts.reverse();
            texts
        }

        /// What `app` measures of the game: depth, ticks, player turns and
        /// the player's cell.
        fn clock(app: &App) -> (u32, u64, u64, Position) {
            let game = app.game();
            (
                game.depth(),
                game.ticks(),
                game.player_turns(),
                player_position(app),
            )
        }

        #[test]
        fn the_status_line_starts_with_the_depth_then_the_turn() {
            let app = generated_floor_app();

            let line = status(&app);

            assert!(line.starts_with("Depth 1 · Turn 1 "), "{line:?}");
            assert!(line.ends_with("Seed 7F3A-91C2-0B5E-D4A8"), "{line:?}");
        }

        #[test]
        fn greater_than_on_the_stairs_shows_the_next_floor_and_keeps_the_log() {
            let mut app = generated_floor_app();
            walk_near_stairs(&mut app, 1);
            // One step short, the stairs are in view and named in the panel.
            let before = render(&app, 80, 24).backend().to_string();
            assert!(before.contains("▼ stairs down"), "{before}");
            walk_near_stairs(&mut app, 0);
            let turns = app.game().player_turns();
            let history = log_texts(&app);

            app.handle_key(KeyEvent::new(KeyCode::Char('>'), KeyModifiers::SHIFT));

            assert_eq!(app.state(), State::Playing);
            assert_eq!(app.game().depth(), 2);
            assert_eq!(app.game().player_turns(), turns + 1, "a descent is a turn");
            let next = generation::generate(SEED, 2);
            assert_eq!(player_position(&app), next.start());
            let line = status(&app);
            assert!(
                line.starts_with(&format!("Depth 2 · Turn {} ", turns + 2)),
                "{line:?}"
            );
            assert!(line.ends_with("Seed 7F3A-91C2-0B5E-D4A8"), "{line:?}");
            // The log keeps the floor above and adds one line.
            let after = log_texts(&app);
            assert_eq!(after[..history.len()], history[..]);
            assert_eq!(after[history.len()..], ["You descend to depth 2."]);
            // The screen shows the new floor around the player, and only
            // what the player sees from its start: the floor above is
            // forgotten with its map.
            let terminal = render(&app, 80, 24);
            let buffer = terminal.backend().buffer();
            let camera = camera(&app, 80, 24);
            let player = camera.screen_position(next.start()).expect("in view");
            assert_eq!(buffer[player].symbol(), "@");
            for (screen, cell) in camera.cells() {
                if buffer[screen].symbol() != " " {
                    assert!(app.game().is_visible(cell), "{cell:?} drawn unseen");
                    assert_eq!(app.game().map().tile(cell), next.map().tile(cell));
                }
            }
            assert_snapshot!(terminal.backend());
        }

        #[test]
        fn a_descent_off_the_stairs_is_refused_at_no_cost_and_logged() {
            let mut app = generated_floor_app();
            walk_near_stairs(&mut app, 1);
            let before = clock(&app);
            let history = log_texts(&app);

            for key in [
                press(KeyCode::Char('>')),
                KeyEvent::new(KeyCode::Char('D'), KeyModifiers::SHIFT),
            ] {
                app.handle_key(key);
            }

            assert_eq!(clock(&app), before, "no depth, time or turn spent");
            assert_eq!(app.state(), State::Playing);
            assert!(status(&app).starts_with("Depth 1 · "), "{}", status(&app));
            let after = log_texts(&app);
            assert_eq!(after[..history.len()], history[..]);
            assert_eq!(
                after[history.len()..],
                ["There are no stairs here.", "There are no stairs here."]
            );
        }

        #[test]
        fn lowercase_d_and_less_than_never_descend() {
            let mut app = generated_floor_app();
            walk_near_stairs(&mut app, 0);
            let before = clock(&app);
            let messages = app.log().len();

            press_all(&mut app, &[KeyCode::Char('d'), KeyCode::Char('<')]);

            assert_eq!(clock(&app), before);
            assert_eq!(app.log().len(), messages, "unbound keys log nothing");
        }

        #[test]
        fn descending_again_and_again_counts_each_floor() {
            let mut app = generated_floor_app();
            let mut turn = 1;

            for depth in 2..=4 {
                walk_near_stairs(&mut app, 0);
                // Both keys go down: `>` then the alternative, in turn.
                let key = if depth % 2 == 0 {
                    KeyEvent::new(KeyCode::Char('>'), KeyModifiers::SHIFT)
                } else {
                    KeyEvent::new(KeyCode::Char('D'), KeyModifiers::SHIFT)
                };
                app.handle_key(key);

                assert_eq!(app.game().depth(), depth);
                assert_eq!(
                    player_position(&app),
                    generation::generate(SEED, depth).start()
                );
                assert!(app.game().player_turns() + 1 > turn, "time went on");
                turn = app.game().player_turns() + 1;
                let line = status(&app);
                assert!(
                    line.starts_with(&format!("Depth {depth} · Turn {turn} ")),
                    "{line:?}"
                );
                let newest = app.log().newest_first().next().unwrap();
                assert_eq!(newest.text, format!("You descend to depth {depth}."));
            }
            let descents = log_texts(&app)
                .into_iter()
                .filter(|text| text.starts_with("You descend"))
                .count();
            assert_eq!(descents, 3, "the log keeps every floor");
        }

        #[test]
        fn the_ascii_status_line_and_help_stay_plain_ascii() {
            let app = App::with_game(
                Theme::new(ColorMode::Ansi16, GlyphSet::Ascii),
                Game::on_floor(SEED, generation::generate(SEED, FIRST_DEPTH)),
                shipped_content(),
            );
            let buffer = render(&app, 80, 24).backend().buffer().clone();

            let status = row_text(&buffer, 0);
            let help = row_text(&buffer, 23);

            assert!(status.starts_with(" Depth 1 - Turn 1 "), "{status:?}");
            // Seven help entries fit the 80 columns, one to spare: nothing cut.
            assert_eq!(
                help,
                " <^>v/1-9 move   . wait   t trace   g take   i pack   > descend   q save & quit "
            );
        }
    }

    /// The overlays leave no sliver of the map against their frame (bug B1
    /// of the F7 tests): on 80 × 24, the map column between the 57-column
    /// frame and the side panel, and the map row under it, are blank.
    mod overlay_gutter {
        use super::*;

        /// The map cells of an 80 × 24 screen around the overlays' frame:
        /// column 57 of the map rows, then row 16 left of the side panel.
        fn gutter() -> Vec<(u16, u16)> {
            let column = (1..=16).map(|y| (57, y));
            let row = (0..57).map(|x| (x, 16));
            column.chain(row).collect()
        }

        /// The gutter cells of `buffer` that show something.
        fn drawn(buffer: &Buffer) -> Vec<(u16, u16)> {
            gutter()
                .into_iter()
                .filter(|&cell| buffer[cell].symbol() != " ")
                .collect()
        }

        #[test]
        fn the_inventory_and_the_trace_blank_the_map_around_their_frame() {
            let theme = Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode);
            let mut app = generated_floor_app();
            // Walk towards the stairs until map glyphs fill the gutter, so
            // the test shows they are hidden rather than never drawn.
            let floor = generation::generate(SEED, FIRST_DEPTH);
            let to_stairs = DijkstraMap::new(floor.map(), [floor.stairs()]);
            while drawn(render(&app, 80, 24).backend().buffer()).is_empty() {
                let direction = to_stairs
                    .downhill(player_position(&app))
                    .expect("map glyphs reach the gutter before the stairs");
                app.handle_key(press(vi_key(direction)));
            }

            for (open, close) in [
                (KeyCode::Char('i'), KeyCode::Esc),
                (KeyCode::Char('t'), KeyCode::Esc),
            ] {
                app.handle_key(press(open));
                let buffer = render(&app, 80, 24).backend().buffer().clone();

                assert_eq!(drawn(&buffer), [], "{open:?}: the gutter is blank");
                for cell in gutter() {
                    assert_eq!(Some(buffer[cell].bg), theme.bg.bg, "{open:?} {cell:?}");
                }
                assert_eq!(buffer[(56, 1)].symbol(), "╮", "{open:?}: frame corner");
                assert_eq!(buffer[(58, 1)].symbol(), "│", "{open:?}: panel border");
                app.handle_key(press(close));
            }
            assert!(!drawn(render(&app, 80, 24).backend().buffer()).is_empty());
        }
    }

    /// The side panel, the message log and the end screen (T16), driven by
    /// keys like a player and read back from the rendered screen.
    mod combat_screens {
        use ratatui::style::{Color, Style};
        use sigilfall_core::actor::ActorId;

        use super::*;
        use crate::ui::hud::{HP_BAR_WIDTH, filled_cells};

        /// Test monsters whose blows are known in advance:
        /// - the cave gnat (attack 0) always deals exactly 1 damage to the
        ///   player (defense 1), and its 999 hit points outlast any fight;
        /// - the wax moth dies from any blow of the player;
        /// - the executioner (attack 99) kills the player's 30 hit points in
        ///   one blow.
        const TEST_MONSTERS: &str = r#"[
            Monster(id: "gnat", name: "cave gnat", family: Vermin, elite: false,
                speed: 100, hp: 999, attack: 0, defense: 0, min_depth: 1),
            Monster(id: "moth", name: "wax moth", family: Vermin, elite: false,
                speed: 100, hp: 1, attack: 0, defense: 0, min_depth: 1),
            Monster(id: "executioner", name: "executioner", family: Acolyte, elite: true,
                speed: 100, hp: 999, attack: 99, defense: 0, min_depth: 1),
        ]"#;

        /// Column of the first character of the side panel's content.
        const PANEL_LEFT: u16 = 60;
        /// Last column of the side panel's content on an 80-column screen.
        const PANEL_RIGHT: u16 = 78;
        /// Row of "HP" and its value.
        const HP_ROW: u16 = 1;
        /// Row of the health bar.
        const BAR_ROW: u16 = 2;
        /// Row of the "IN VIEW" title, under the essence and the sigils
        /// block of an empty grimoire ("SIGILS" and "none yet").
        const IN_VIEW_ROW: u16 = 8;
        /// Row of the log's separator.
        const LOG_SEPARATOR_ROW: u16 = 17;
        /// First and last rows of the log's messages.
        const LOG_ROWS: std::ops::RangeInclusive<u16> = 18..=22;

        /// Picks the health token a level is drawn with out of a theme.
        type HealthToken = fn(&Theme) -> Style;

        /// The four rendering modes of `docs/design.md`.
        const MODES: [ColorMode; 4] = [
            ColorMode::TrueColorDark,
            ColorMode::TrueColorLight,
            ColorMode::Ansi16,
            ColorMode::Monochrome,
        ];

        /// An application in the fixed room of [`SEED`], drawn with
        /// `theme`, with a monster of each `(definition, cell)` of
        /// [`TEST_MONSTERS`]. Returns the keys of the monsters, in order.
        fn arena(theme: Theme, monsters: &[(&str, Position)]) -> (App, Vec<ActorId>) {
            let content = Content::parse(TEST_MONSTERS).expect("valid test monsters");
            let mut game = Game::in_fixed_room(SEED);
            let ids = monsters
                .iter()
                .map(|&(id, cell)| game.spawn_monster(cell, content.monster(id).unwrap()))
                .collect();
            (App::with_game(theme, game, content), ids)
        }

        /// The cell east of the player's start, where the duels happen.
        fn east() -> Position {
            PLAYER_START.step(Direction::East)
        }

        /// The default theme of Windows Terminal (see [`new_app`]).
        fn dark() -> Theme {
            Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode)
        }

        /// The player's hit points.
        fn player_hp(app: &App) -> i32 {
            app.game().player_actor().hp()
        }

        /// Waits until the cave gnat, one blow of 1 per turn, has brought
        /// the player down to `hp`; bounded so a gnat that never strikes
        /// fails the test instead of looping.
        fn wait_until_hp(app: &mut App, hp: i32) {
            for _ in 0..100 {
                if player_hp(app) <= hp {
                    break;
                }
                app.handle_key(press(KeyCode::Char('.')));
            }
            assert_eq!(player_hp(app), hp, "the gnat deals exactly 1 per blow");
        }

        /// Rows `rows` of `buffer`, as text.
        fn rows(buffer: &Buffer, rows: std::ops::RangeInclusive<u16>) -> Vec<String> {
            rows.map(|y| row_text(buffer, y)).collect()
        }

        /// The text of the side panel's content on row `y`, trimmed.
        fn panel_text(buffer: &Buffer, y: u16) -> String {
            (PANEL_LEFT..=PANEL_RIGHT)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim()
                .to_owned()
        }

        /// Asserts that the cell `(x, y)` of `buffer` has exactly the
        /// colour and emphasis of `style`.
        fn assert_style(buffer: &Buffer, (x, y): (u16, u16), style: Style, what: &str) {
            let cell = &buffer[(x, y)];
            assert_eq!(cell.fg, style.fg.unwrap_or(Color::Reset), "{what}");
            assert_eq!(cell.modifier, style.add_modifier, "{what}");
        }

        #[test]
        fn a_new_game_shows_full_health_and_the_welcome_message() {
            let app = new_app();
            let screen = render(&app, 80, 24).backend().buffer().clone();

            assert_eq!(
                panel_text(&screen, HP_ROW),
                format!("HP{}30/30", " ".repeat(12))
            );
            assert_eq!(panel_text(&screen, BAR_ROW), "█".repeat(18));
            assert_eq!(panel_text(&screen, IN_VIEW_ROW), "IN VIEW");
            assert_eq!(panel_text(&screen, IN_VIEW_ROW + 1), "@ you");
            assert_eq!(
                row_text(&screen, 18).trim_end(),
                "   You enter the dungeon."
            );
            let separator = row_text(&screen, LOG_SEPARATOR_ROW);
            assert!(separator.starts_with("─ Log ──"), "{separator:?}");
            assert_eq!(
                screen[(58, LOG_SEPARATOR_ROW)].symbol(),
                "┴",
                "the panel's border meets the separator"
            );
            assert_eq!(screen[(58, 1)].symbol(), "│");
        }

        #[test]
        fn each_blow_is_logged_with_its_damage_and_the_health_bar_drops() {
            let (mut app, ids) = arena(dark(), &[("gnat", east())]);
            let gnat_hp = |app: &App| app.game().actor(ids[0]).unwrap().hp();

            let before = gnat_hp(&app);
            app.handle_key(press(KeyCode::Char('l')));
            let dealt = before - gnat_hp(&app);
            for _ in 0..3 {
                app.handle_key(press(KeyCode::Char('.')));
            }

            let screen = render(&app, 80, 24).backend().buffer().clone();
            let log = rows(&screen, LOG_ROWS);
            assert!(
                log.iter()
                    .any(|line| line.trim_end() == format!("   You hit the cave gnat ({dealt}).")),
                "{log:#?}"
            );
            let taken = 30 - player_hp(&app);
            assert!(taken >= 1, "the gnat struck back");
            let danger = log
                .iter()
                .filter(|line| line.trim_end() == " ! The cave gnat hits you (1).")
                .count();
            assert_eq!(danger, usize::try_from(taken).unwrap(), "{log:#?}");
            let hp = player_hp(&app);
            assert_eq!(
                panel_text(&screen, HP_ROW),
                format!("HP{}{hp}/30", " ".repeat(19 - 2 - format!("{hp}/30").len()))
            );
            let filled = usize::from(filled_cells(hp, 30, HP_BAR_WIDTH));
            assert_eq!(
                panel_text(&screen, BAR_ROW),
                format!("{}{}", "█".repeat(filled), "░".repeat(18 - filled))
            );
            // D25: the gnat's line ends with its own health.
            assert_eq!(
                panel_text(&screen, IN_VIEW_ROW + 2),
                format!("v cave gnat {}/999", gnat_hp(&app))
            );
        }

        #[test]
        fn danger_lines_carry_the_danger_colour_and_the_bang_prefix_in_every_mode() {
            for mode in MODES {
                let theme = Theme::new(mode, GlyphSet::Unicode);
                let (mut app, _) = arena(theme.clone(), &[("gnat", east())]);
                wait_until_hp(&mut app, 29);

                let screen = render(&app, 80, 24).backend().buffer().clone();
                let row = LOG_ROWS
                    .clone()
                    .find(|&y| row_text(&screen, y).contains("hits you"))
                    .unwrap_or_else(|| panic!("{mode:?}: the blow is logged"));
                assert_eq!(screen[(1, row)].symbol(), "!", "{mode:?}");
                assert_style(
                    &screen,
                    (1, row),
                    theme.log_danger,
                    &format!("{mode:?}: prefix"),
                );
                assert_style(
                    &screen,
                    (3, row),
                    theme.log_danger,
                    &format!("{mode:?}: text"),
                );
                assert_style(
                    &screen,
                    (3, 18),
                    theme.log_info,
                    &format!("{mode:?}: welcome"),
                );
            }
        }

        #[test]
        fn a_killing_blow_names_the_victim_and_removes_it_from_the_panel() {
            let (mut app, ids) = arena(dark(), &[("moth", east())]);
            let before = render(&app, 80, 24).backend().buffer().clone();
            assert_eq!(panel_text(&before, IN_VIEW_ROW + 2), "v wax moth      1/1");

            app.handle_key(press(KeyCode::Char('l')));

            assert!(app.game().actor(ids[0]).is_none(), "the moth died");
            let screen = render(&app, 80, 24).backend().buffer().clone();
            let log = rows(&screen, LOG_ROWS);
            let kill = log
                .iter()
                .find(|line| line.starts_with(" + You kill the wax moth ("))
                .unwrap_or_else(|| panic!("{log:#?}"));
            assert!(kill.trim_end().ends_with(")."), "{kill:?}");
            assert!(!log.iter().any(|line| line.contains("dies")), "{log:#?}");
            assert_style(&screen, (1, 19), dark().log_success, "kill line");
            assert_eq!(panel_text(&screen, IN_VIEW_ROW + 2), "");
        }

        #[test]
        fn the_log_scrolls_to_keep_the_five_newest_messages() {
            // A wall bump writes nothing, so alternate blows and waits:
            // each turn adds the player's blow and the gnat's reply.
            let (mut app, _) = arena(dark(), &[("gnat", east())]);
            for _ in 0..4 {
                app.handle_key(press(KeyCode::Char('l')));
            }
            assert!(app.log().len() > 5, "more messages than rows");

            let screen = render(&app, 80, 24).backend().buffer().clone();
            let shown: Vec<_> = rows(&screen, LOG_ROWS)
                .into_iter()
                .map(|line| line[3..].trim_end().to_owned())
                .collect();
            let mut newest: Vec<_> = app
                .log()
                .newest_first()
                .take(5)
                .map(|message| message.text.clone())
                .collect();
            newest.reverse();
            assert_eq!(shown, newest, "oldest at the top, newest at the bottom");
            assert!(!shown.iter().any(|line| line.contains("enter the dungeon")));
        }

        #[test]
        fn walking_into_a_wall_writes_nothing_in_the_log() {
            let mut app = new_app();
            press_all(&mut app, &[KeyCode::Char('h'); 6]);

            assert_eq!(app.log().len(), 1, "only the welcome message");
        }

        #[test]
        fn the_health_value_and_bar_follow_the_thresholds_in_every_mode() {
            // 16/30 is above half, 15/30 exactly half (mid), 8/30 the last
            // mid value, 7/30 under a quarter (low, marked "!").
            let levels: [(i32, &str, HealthToken); 4] = [
                (16, "16/30", |t| t.hp_high),
                (15, "15/30", |t| t.hp_mid),
                (8, "8/30", |t| t.hp_mid),
                (7, "! 7/30", |t| t.hp_low),
            ];
            for mode in MODES {
                for glyph_set in [GlyphSet::Unicode, GlyphSet::Ascii] {
                    let theme = Theme::new(mode, glyph_set);
                    let (mut app, _) = arena(theme.clone(), &[("gnat", east())]);
                    for (hp, value, token) in levels {
                        wait_until_hp(&mut app, hp);
                        let case = format!("{mode:?}, {glyph_set:?}, {hp} HP");
                        let screen = render(&app, 80, 24).backend().buffer().clone();

                        let figures = panel_text(&screen, HP_ROW);
                        assert!(figures.starts_with("HP "), "{case}: {figures:?}");
                        assert!(figures.ends_with(value), "{case}: {figures:?}");
                        assert_eq!(
                            row_text(&screen, HP_ROW).chars().nth(78),
                            value.chars().last(),
                            "{case}: right-aligned"
                        );
                        assert_style(&screen, (PANEL_RIGHT, HP_ROW), token(&theme), &case);
                        assert_style(&screen, (PANEL_LEFT, HP_ROW), theme.text_muted, &case);

                        let filled = filled_cells(hp, 30, HP_BAR_WIDTH);
                        for offset in 0..HP_BAR_WIDTH {
                            let cell = (PANEL_LEFT + offset, BAR_ROW);
                            let (glyph, style) = if offset < filled {
                                (theme.glyphs.bar_full, token(&theme))
                            } else {
                                (theme.glyphs.bar_empty, theme.border)
                            };
                            assert_eq!(
                                screen[cell].symbol(),
                                glyph.to_string(),
                                "{case}: bar cell {offset}"
                            );
                            assert_style(
                                &screen,
                                cell,
                                style,
                                &format!("{case}: bar cell {offset}"),
                            );
                        }
                    }
                }
            }
        }

        #[test]
        fn monochrome_tells_low_health_by_the_mark_alone() {
            let theme = Theme::new(ColorMode::Monochrome, GlyphSet::Ascii);
            let (mut app, _) = arena(theme, &[("gnat", east())]);
            wait_until_hp(&mut app, 8);
            let mid = render(&app, 80, 24).backend().buffer().clone();
            wait_until_hp(&mut app, 7);
            let low = render(&app, 80, 24).backend().buffer().clone();

            assert!(!panel_text(&mid, HP_ROW).contains('!'));
            assert!(panel_text(&low, HP_ROW).contains("! 7/30"));
            assert_eq!(
                panel_text(&low, BAR_ROW),
                format!("{}{}", "=".repeat(5), "-".repeat(13))
            );
        }

        /// An application whose player was just killed by the executioner,
        /// with the turn of the death.
        fn killed_app(theme: Theme) -> App {
            let (mut app, _) = arena(theme, &[("executioner", east())]);
            for _ in 0..10 {
                if app.state() == State::GameOver {
                    break;
                }
                app.handle_key(press(KeyCode::Char('.')));
            }
            assert_eq!(app.state(), State::GameOver);
            assert!(app.game().is_over());
            app
        }

        #[test]
        fn the_player_death_is_logged_and_the_end_screen_names_the_killer() {
            let app = killed_app(dark());
            let turn = app.game().player_turns();

            let last = app.log().newest_first().next().unwrap();
            assert!(
                last.text.starts_with("The executioner kills you (") && last.text.ends_with(")."),
                "{last:?}"
            );
            let screen = render(&app, 80, 24).backend().buffer().clone();
            let text = rows(&screen, 0..=23).join(
                "
",
            );
            assert!(
                text.contains(&format!(
                    "Killed by an executioner at depth 1, on turn {turn}."
                )),
                "{text}"
            );
            assert!(text.contains("SEED   7F3A-91C2-0B5E-D4A8"), "{text}");
            assert!(
                text.contains("Replay this seed: sigilfall --seed 7F3A-91C2-0B5E-D4A8"),
                "{text}"
            );
            assert!(
                row_text(&screen, 23).starts_with(" Enter title screen   q quit"),
                "{text}"
            );
            assert!(!text.contains("Turn "), "no game screen:\n{text}");
            assert_snapshot!(render(&app, 80, 24).backend());
        }

        #[test]
        fn no_game_action_is_played_after_death_but_quitting_works() {
            let clock = ManualClock::new();
            let mut app = killed_app(dark()).with_clock(clock.clone());
            clock.advance(END_SCREEN_GRACE);
            let ticks = app.game().ticks();
            let turns = app.game().player_turns();
            let messages = app.log().len();
            let before = render(&app, 80, 24).backend().buffer().clone();

            press_all(
                &mut app,
                &[
                    KeyCode::Char('l'),
                    KeyCode::Right,
                    KeyCode::Char('.'),
                    KeyCode::Char('5'),
                    KeyCode::Esc,
                ],
            );

            assert_eq!(app.state(), State::GameOver);
            assert_eq!(app.game().ticks(), ticks, "no time passes");
            assert_eq!(app.game().player_turns(), turns);
            assert_eq!(app.log().len(), messages, "nothing new is logged");
            assert_eq!(
                render(&app, 80, 24).backend().buffer(),
                &before,
                "the end screen stays as it is"
            );

            // Escape never quits, not even here: only `q` and Ctrl+C do.
            app.handle_key(press(KeyCode::Esc));
            assert_eq!(app.state(), State::GameOver);
            app.handle_key(press(KeyCode::Char('q')));
            assert_eq!(app.state(), State::Quit);
        }

        #[test]
        fn the_end_screen_also_asks_for_room_below_80_by_24() {
            let app = killed_app(dark());

            let text = render(&app, 79, 24).backend().to_string();

            assert!(text.contains("Enlarge the terminal"), "{text}");
            assert!(!text.contains("Killed by"), "{text}");
        }

        #[test]
        fn a_fight_on_screen_shows_the_panel_and_the_log_together() {
            let (mut app, _) = arena(dark(), &[("gnat", east()), ("moth", Position::new(6, 7))]);
            press_all(
                &mut app,
                &[KeyCode::Char('l'), KeyCode::Char('l'), KeyCode::Char('.')],
            );

            assert_snapshot!(render(&app, 80, 24).backend());
        }

        #[test]
        fn the_combat_screens_are_plain_ascii_with_ascii_glyphs() {
            let theme = Theme::new(ColorMode::Monochrome, GlyphSet::Ascii);
            let (mut app, _) = arena(theme.clone(), &[("gnat", east())]);
            wait_until_hp(&mut app, 5);
            let playing = render(&app, 80, 24).backend().to_string();
            assert!(playing.is_ascii(), "{playing}");

            let dead = killed_app(theme);
            let end = render(&dead, 80, 24).backend().to_string();
            assert!(end.is_ascii(), "{end}");
            assert_snapshot!(playing);
        }
    }

    /// Acceptance tests of "move around the dungeon" (F1), written from the
    /// player's side: only keys go in and only the rendered screen and the
    /// game clock are read back.
    mod acceptance {
        use std::path::PathBuf;
        use std::process::{Command, Stdio};

        use ratatui::crossterm::event::KeyEventState;
        use sigilfall_core::map::FIXED_ROOM_PILLAR;

        use super::*;

        /// Environment variable that turns the child-process tests below
        /// into the program that panics; absent in a normal test run.
        const CHILD_MODE: &str = "SIGILFALL_TEST_PANIC_CHILD";

        /// Value of [`CHILD_MODE`] for the child that installs the
        /// production panic hook before panicking.
        const WITH_HOOK: &str = "with-hook";

        /// Value of [`CHILD_MODE`] for the control child that panics without
        /// the hook, to show the checks below can fail.
        const WITHOUT_HOOK: &str = "without-hook";

        /// Terminal sequence that leaves the alternate screen.
        const LEAVE_ALTERNATE_SCREEN: &str = "\x1b[?1049l";

        /// Terminal sequence that shows the cursor again.
        const SHOW_CURSOR: &str = "\x1b[?25h";

        /// The screen cell `(column, row)` where the map's cell `position`
        /// is drawn on the 80 × 24 screen of `app`, through the camera.
        fn screen_cell_of(app: &App, position: Position) -> (u16, u16) {
            let cell = camera(app, 80, 24)
                .screen_position(position)
                .unwrap_or_else(|| panic!("{position:?} is off screen"));
            (cell.x, cell.y)
        }

        /// Every cell of the map area of an 80 × 24 `buffer` that shows the
        /// player glyph. The side panel's "in view" list also shows `@`, as
        /// a legend, so it is left out.
        fn player_cells(buffer: &Buffer) -> Vec<(u16, u16)> {
            let area = ui::game::map_area(buffer.area);
            (area.top()..area.bottom())
                .flat_map(|y| (area.left()..area.right()).map(move |x| (x, y)))
                .filter(|&(x, y)| buffer[(x, y)].symbol() == "@")
                .collect()
        }

        /// The game screen of `app` at 80 × 24.
        fn screen(app: &App) -> Buffer {
            render(app, 80, 24).backend().buffer().clone()
        }

        /// The whole screen of `buffer` as text, rows joined by newlines;
        /// unlike the backend's own display it accepts a zero-sized screen.
        fn screen_text(buffer: &Buffer) -> String {
            (0..buffer.area.height)
                .map(|y| row_text(buffer, y))
                .collect::<Vec<_>>()
                .join(
                    "
",
                )
        }

        /// The number shown after "Turn" on the status line.
        fn turn_shown(app: &App) -> u64 {
            let first_line = row_text(&screen(app), 0);
            first_line
                .trim()
                .split_once("Turn ")
                .and_then(|(_, rest)| rest.split_whitespace().next())
                .and_then(|value| value.parse().ok())
                .unwrap_or_else(|| panic!("no turn counter on: {first_line:?}"))
        }

        /// Presses `code` `times` times and checks the player ended where
        /// `expected` says, so a route to a test position is verified.
        fn walk(app: &mut App, steps: &[(KeyCode, usize)], expected: Position) {
            for &(code, times) in steps {
                press_all(app, &vec![code; times]);
            }
            assert_eq!(player_position(app), expected, "route {steps:?}");
        }

        /// The key press as a keypad digit with Num Lock on: the character
        /// is a digit and the key state says it comes from the keypad.
        fn keypad(digit: char) -> KeyEvent {
            KeyEvent::new_with_kind_and_state(
                KeyCode::Char(digit),
                KeyModifiers::NONE,
                KeyEventKind::Press,
                KeyEventState::KEYPAD,
            )
        }

        #[test]
        fn every_movement_key_moves_one_cell_in_its_direction() {
            // Direction, arrow or navigation key, digit row / keypad digit
            // and vi key, as listed in the keymap documentation.
            let bindings = [
                (Direction::North, KeyCode::Up, '8', 'k'),
                (Direction::NorthEast, KeyCode::PageUp, '9', 'u'),
                (Direction::East, KeyCode::Right, '6', 'l'),
                (Direction::SouthEast, KeyCode::PageDown, '3', 'n'),
                (Direction::South, KeyCode::Down, '2', 'j'),
                (Direction::SouthWest, KeyCode::End, '1', 'b'),
                (Direction::West, KeyCode::Left, '4', 'h'),
                (Direction::NorthWest, KeyCode::Home, '7', 'y'),
            ];

            for (direction, navigation, digit, vi) in bindings {
                let events = [
                    press(navigation),
                    press(KeyCode::Char(digit)),
                    keypad(digit),
                    press(KeyCode::Char(vi)),
                ];
                for key in events {
                    let mut app = new_app();
                    let ticks = app.game().ticks();

                    app.handle_key(key);

                    let target = PLAYER_START.step(direction);
                    assert_eq!(player_position(&app), target, "{direction:?} by {key:?}");
                    assert_eq!(app.game().ticks(), ticks + 1, "{direction:?} by {key:?}");
                    let buffer = screen(&app);
                    assert_eq!(
                        player_cells(&buffer),
                        [screen_cell_of(&app, target)],
                        "{direction:?} by {key:?}: one @, one cell from the start"
                    );
                    assert_eq!(turn_shown(&app), 2, "{direction:?} by {key:?}");
                }
            }
        }

        #[test]
        fn moving_leaves_floor_behind_and_draws_a_single_player() {
            let mut app = new_app();
            let start_cell = screen_cell_of(&app, PLAYER_START);

            app.handle_key(press(KeyCode::Char('l')));

            let buffer = screen(&app);
            assert_eq!(
                buffer[start_cell].symbol(),
                "·",
                "the old cell is floor again"
            );
            assert_eq!(player_cells(&buffer).len(), 1);
        }

        #[test]
        fn a_long_walk_follows_every_diagonal_and_returns_to_the_start() {
            let mut app = new_app();
            let there_and_back = [
                KeyCode::Char('u'),
                KeyCode::Char('u'),
                KeyCode::Char('n'),
                KeyCode::Char('n'),
                KeyCode::Char('b'),
                KeyCode::Char('b'),
                KeyCode::Char('y'),
                KeyCode::Char('y'),
            ];

            press_all(&mut app, &there_and_back);

            assert_eq!(player_position(&app), PLAYER_START);
            assert_eq!(turn_shown(&app), 1 + 8);
        }

        #[test]
        fn the_pillar_blocks_a_move_from_each_of_its_eight_sides() {
            // Start (5, 5); the pillar is at (15, 6). Each route reaches a
            // neighbour of the pillar by moves that are themselves legal.
            let right = KeyCode::Char('l');
            let down = KeyCode::Char('j');
            let cases = [
                (
                    vec![(right, 9)],
                    Position::new(14, 5),
                    'n',
                    "north-west side",
                ),
                (vec![(right, 10)], Position::new(15, 5), 'j', "north side"),
                (
                    vec![(right, 11)],
                    Position::new(16, 5),
                    'b',
                    "north-east side",
                ),
                (
                    vec![(right, 9), (down, 1)],
                    Position::new(14, 6),
                    'l',
                    "west side",
                ),
                (
                    vec![(right, 11), (down, 1)],
                    Position::new(16, 6),
                    'h',
                    "east side",
                ),
                (
                    vec![(right, 9), (down, 2)],
                    Position::new(14, 7),
                    'u',
                    "south-west side",
                ),
                (
                    vec![(right, 9), (down, 2), (right, 1)],
                    Position::new(15, 7),
                    'k',
                    "south side",
                ),
                (
                    vec![(right, 11), (down, 2)],
                    Position::new(16, 7),
                    'y',
                    "south-east side",
                ),
            ];
            assert_eq!(FIXED_ROOM_PILLAR, Position::new(15, 6));

            for (route, from, towards_pillar, side) in cases {
                let mut app = new_app();
                walk(&mut app, &route, from);
                let ticks = app.game().ticks();
                let before = screen(&app);

                app.handle_key(press(KeyCode::Char(towards_pillar)));

                assert_eq!(player_position(&app), from, "{side}");
                assert_eq!(app.game().ticks(), ticks, "{side}: no time passes");
                assert_eq!(screen(&app), before, "{side}: the screen is unchanged");
                assert_eq!(turn_shown(&app), app.game().player_turns() + 1, "{side}");
            }
        }

        #[test]
        fn the_outer_walls_block_every_move_that_would_enter_them() {
            // (1, 1) is the top-left floor cell, (28, 10) the bottom-right one.
            let corners = [
                (
                    [(KeyCode::Char('y'), 4)],
                    Position::new(1, 1),
                    "kyuhb",
                    "top-left corner",
                ),
                (
                    [(KeyCode::Char('n'), 5)],
                    Position::new(10, 10),
                    "jnb",
                    "bottom wall",
                ),
            ];

            for (route, from, blocked_keys, place) in corners {
                let mut app = new_app();
                walk(&mut app, &route, from);
                let ticks = app.game().ticks();
                let before = screen(&app);

                for key in blocked_keys.chars() {
                    app.handle_key(press(KeyCode::Char(key)));
                }

                assert_eq!(player_position(&app), from, "{place}");
                assert_eq!(app.game().ticks(), ticks, "{place}: no time passes");
                assert_eq!(screen(&app), before, "{place}: the screen is unchanged");
            }
        }

        #[test]
        fn the_bottom_right_corner_is_enclosed_too() {
            let mut app = new_app();
            walk(
                &mut app,
                &[(KeyCode::Char('n'), 5), (KeyCode::Char('l'), 18)],
                Position::new(28, 10),
            );
            let ticks = app.game().ticks();

            for key in ['j', 'l', 'n', 'u', 'b'] {
                app.handle_key(press(KeyCode::Char(key)));
            }
            // The arrows, keypad digits and navigation keys are refused alike.
            for key in [
                KeyCode::Down,
                KeyCode::Right,
                KeyCode::PageDown,
                KeyCode::End,
            ] {
                app.handle_key(press(key));
            }
            app.handle_key(keypad('3'));

            assert_eq!(player_position(&app), Position::new(28, 10));
            assert_eq!(app.game().ticks(), ticks);
        }

        #[test]
        fn a_refused_move_does_not_prevent_the_next_legal_one() {
            let mut app = new_app();
            walk(&mut app, &[(KeyCode::Char('y'), 4)], Position::new(1, 1));
            let ticks = app.game().ticks();

            press_all(&mut app, &[KeyCode::Char('k'), KeyCode::Char('l')]);

            assert_eq!(player_position(&app), Position::new(2, 1));
            assert_eq!(app.game().ticks(), ticks + 1, "only the legal move counts");
        }

        #[test]
        fn a_game_started_in_a_too_small_terminal_asks_for_more_room() {
            let app = new_app();
            let sizes = [
                (0, 0),
                (1, 1),
                (30, 1),
                (79, 24),
                (80, 23),
                (79, 23),
                (60, 20),
                (300, 10),
                (10, 300),
                (79, 200),
                (200, 23),
            ];

            for (width, height) in sizes {
                let text = screen_text(render(&app, width, height).backend().buffer());

                assert!(!text.contains('@'), "{width} × {height}:\n{text}");
                assert!(!text.contains("Turn"), "{width} × {height}:\n{text}");
                if width >= 30 && height >= 2 {
                    assert!(text.contains("Enlarge"), "{width} × {height}:\n{text}");
                }
            }
        }

        #[test]
        fn the_resize_message_gives_the_target_and_the_current_size() {
            let app = new_app();

            let text = render(&app, 79, 24).backend().to_string();

            assert!(
                text.contains("Enlarge the terminal to 80 × 24 (current: 79 × 24)."),
                "{text}"
            );
        }

        #[test]
        fn shrinking_and_enlarging_keeps_the_game_exactly_as_it_was() {
            let mut app = new_app();
            press_all(
                &mut app,
                &[KeyCode::Char('l'), KeyCode::Char('n'), KeyCode::Char('.')],
            );
            let before = screen(&app);
            let position = player_position(&app);
            let ticks = app.game().ticks();
            let mut terminal = render(&app, 80, 24);

            for (width, height) in [(40, 10), (1, 1), (0, 0), (79, 24), (80, 24)] {
                terminal.backend_mut().resize(width, height);
                terminal.draw(|frame| app.render(frame)).unwrap();
            }

            assert_eq!(terminal.backend().buffer(), &before);
            assert_eq!(player_position(&app), position);
            assert_eq!(app.game().ticks(), ticks);
        }

        #[test]
        fn key_releases_and_repeats_never_make_time_pass() {
            let mut codes = vec![
                KeyCode::Up,
                KeyCode::Down,
                KeyCode::Left,
                KeyCode::Right,
                KeyCode::Home,
                KeyCode::End,
                KeyCode::PageUp,
                KeyCode::PageDown,
                KeyCode::KeypadBegin,
                KeyCode::Enter,
                KeyCode::Tab,
                KeyCode::Esc,
                KeyCode::Backspace,
                KeyCode::Delete,
                KeyCode::Insert,
            ];
            codes.extend((b' '..=b'~').map(|byte| KeyCode::Char(char::from(byte))));
            codes.extend((1..=12).map(KeyCode::F));
            let modifiers = [
                KeyModifiers::NONE,
                KeyModifiers::SHIFT,
                KeyModifiers::CONTROL,
                KeyModifiers::ALT,
            ];
            let mut app = new_app();
            let ticks = app.game().ticks();
            let before = screen(&app);

            for kind in [KeyEventKind::Release, KeyEventKind::Repeat] {
                for &code in &codes {
                    for modifiers in modifiers {
                        app.handle_key(KeyEvent::new_with_kind(code, modifiers, kind));
                    }
                }
            }

            assert_eq!(app.state(), State::Playing, "no release or repeat quits");
            assert_eq!(app.game().ticks(), ticks);
            assert_eq!(player_position(&app), PLAYER_START);
            assert_eq!(screen(&app), before);
        }

        #[test]
        fn drawing_the_screen_again_and_again_never_makes_time_pass() {
            let app = new_app();
            let ticks = app.game().ticks();

            for round in 0..200_u16 {
                let (width, height) = if round % 3 == 0 {
                    (80, 24)
                } else {
                    (60 + round % 70, 20)
                };
                drop(render(&app, width, height));
            }

            assert_eq!(app.game().ticks(), ticks);
            assert_eq!(turn_shown(&app), 1);
        }

        #[test]
        fn a_new_game_starts_with_the_clock_at_one_and_counts_player_actions() {
            // The status line shows the turn the player is about to play:
            // "Turn 1" before any action, one more per move or wait, and
            // nothing more for a refused move (B1).
            let mut app = new_app();
            assert_eq!(app.game().player_turns(), 0);
            assert_eq!(turn_shown(&app), 1);

            press_all(&mut app, &[KeyCode::Char('l'), KeyCode::Char('.')]);
            assert_eq!(turn_shown(&app), 3);

            walk(&mut app, &[(KeyCode::Char('y'), 4)], Position::new(2, 1));
            assert_eq!(turn_shown(&app), 7);
            press_all(&mut app, &[KeyCode::Char('k'), KeyCode::Char('y')]);
            assert_eq!(turn_shown(&app), 7, "refused moves are not turns");
        }

        #[test]
        fn the_status_line_shows_the_seed_of_the_run_in_its_shareable_form() {
            let mut app = new_app();
            let first_line = row_text(&screen(&app), 0);

            assert!(
                first_line.ends_with(" Seed 7F3A-91C2-0B5E-D4A8 "),
                "{first_line:?}"
            );
            let shown = first_line.trim_end().rsplit(' ').next().unwrap();
            assert_eq!(shown.parse(), Ok(app.game().seed()), "--seed reads it back");

            press_all(&mut app, &[KeyCode::Char('l'), KeyCode::Char('.')]);
            let first_line = row_text(&screen(&app), 0);
            assert!(
                first_line.starts_with(" Depth 1 · Turn 3 "),
                "{first_line:?}"
            );
            assert!(
                first_line.ends_with(" Seed 7F3A-91C2-0B5E-D4A8 "),
                "the seed does not change while playing: {first_line:?}"
            );
        }

        #[test]
        fn the_seed_stays_at_the_right_end_of_a_wider_terminal() {
            let app = fixed_room_app(
                Theme::new(ColorMode::TrueColorDark, GlyphSet::Ascii),
                Seed::new(42),
            );
            let first_line = row_text(render(&app, 120, 30).backend().buffer(), 0);

            assert_eq!(first_line.chars().count(), 120);
            assert!(
                first_line.ends_with(" Seed 0000-0000-0000-002A "),
                "{first_line:?}"
            );
            assert!(
                first_line.starts_with(" Depth 1 - Turn 1 "),
                "{first_line:?}"
            );
        }

        #[test]
        fn game_keys_do_nothing_below_80_by_24_but_quitting_works() {
            // B2: the resize message hides the game, so nothing is played
            // blind; only the quit keys still act.
            for (width, height) in [(79, 24), (80, 23), (40, 10), (0, 0)] {
                let mut app = new_app();
                drop(render(&app, width, height));
                let turns = app.game().player_turns();
                let ticks = app.game().ticks();

                press_all(
                    &mut app,
                    &[KeyCode::Char('l'), KeyCode::Right, KeyCode::Char('.')],
                );

                let size = format!("{width} × {height}");
                assert_eq!(player_position(&app), PLAYER_START, "{size}");
                assert_eq!(app.game().player_turns(), turns, "{size}");
                assert_eq!(app.game().ticks(), ticks, "{size}: no time passes");
                assert_eq!(app.state(), State::Playing, "{size}");

                app.handle_key(press(KeyCode::Char('q')));

                assert_eq!(app.state(), State::Quit, "{size}");
            }
        }

        #[test]
        fn game_keys_work_again_once_the_terminal_is_enlarged() {
            let mut app = new_app();
            let mut terminal = render(&app, 79, 24);
            app.handle_key(press(KeyCode::Char('l')));
            assert_eq!(player_position(&app), PLAYER_START);

            terminal.backend_mut().resize(80, 24);
            terminal.draw(|frame| app.render(frame)).unwrap();
            app.handle_key(press(KeyCode::Char('l')));

            assert_eq!(player_position(&app), PLAYER_START.step(Direction::East));
            assert_eq!(turn_shown(&app), 2);
        }

        /// Path of the file that receives the output of a child process,
        /// unique per test run and per child mode.
        fn child_output_path(mode: &str) -> PathBuf {
            std::env::temp_dir().join(format!("sigilfall-panic-{mode}-{}.log", std::process::id()))
        }

        /// Runs this test executable again, restricted to the child test, in
        /// `mode`, with standard output and error written to one file so the
        /// order of what was written is kept. Returns the exit code and the
        /// captured output.
        fn run_panicking_child(mode: &str) -> (Option<i32>, String) {
            let path = child_output_path(mode);
            let log = std::fs::File::create(&path).expect("create the child's log file");
            let status = Command::new(std::env::current_exe().expect("path of the test binary"))
                .args([
                    "--exact",
                    "app::tests::acceptance::panicking_child",
                    "--nocapture",
                ])
                .env(CHILD_MODE, mode)
                .env("TERM", "xterm-256color")
                .env("RUST_BACKTRACE", "0")
                .stdin(Stdio::null())
                .stdout(log.try_clone().expect("duplicate the log handle"))
                .stderr(log)
                .status()
                .expect("start the child test process");
            let bytes = std::fs::read(&path).expect("read the child's log file");
            drop(std::fs::remove_file(&path));
            (status.code(), String::from_utf8_lossy(&bytes).into_owned())
        }

        /// The program the two tests below start: panics, with or without
        /// the production panic hook installed first, depending on
        /// [`CHILD_MODE`]. It does nothing in a normal test run.
        #[test]
        fn panicking_child() {
            match std::env::var(CHILD_MODE).as_deref() {
                Ok(WITH_HOOK) => {
                    crate::install_panic_hook();
                    panic!("deliberate panic of the hook test");
                }
                Ok(WITHOUT_HOOK) => panic!("deliberate panic of the control test"),
                _ => {}
            }
        }

        #[test]
        fn a_panic_restores_the_terminal_before_its_message_is_printed() {
            let (code, output) = run_panicking_child(WITH_HOOK);

            assert_eq!(
                code,
                Some(101),
                "the panic still ends the process:\n{output:?}"
            );
            let message = output
                .find("deliberate panic of the hook test")
                .unwrap_or_else(|| panic!("the panic message is printed:\n{output:?}"));
            let leave = output
                .find(LEAVE_ALTERNATE_SCREEN)
                .unwrap_or_else(|| panic!("the alternate screen is left:\n{output:?}"));
            let cursor = output
                .find(SHOW_CURSOR)
                .unwrap_or_else(|| panic!("the cursor is shown again:\n{output:?}"));
            assert!(
                leave < message,
                "the screen is left before the message:\n{output:?}"
            );
            assert!(
                cursor < message,
                "the cursor is shown before the message:\n{output:?}"
            );
        }

        #[test]
        fn without_the_hook_a_panic_leaves_the_terminal_as_it_was() {
            // Control: the same child without the hook emits no restore
            // sequence, so the test above does detect the hook's work.
            let (code, output) = run_panicking_child(WITHOUT_HOOK);

            assert_eq!(code, Some(101), "{output:?}");
            assert!(
                output.contains("deliberate panic of the control test"),
                "{output:?}"
            );
            assert!(!output.contains(LEAVE_ALTERNATE_SCREEN), "{output:?}");
            assert!(!output.contains(SHOW_CURSOR), "{output:?}");
        }
    }

    /// Acceptance tests of F5 (combat, life and journal), written from the
    /// player's side: keys go in, and the rendered 80 × 24 screen is read
    /// back. The expected log lines are worked out by a twin of the game
    /// that is fed the same actions, with the wording of `docs/design.md`
    /// written out again here rather than taken from `messages`.
    mod f5_acceptance {
        use std::collections::HashMap;

        use sigilfall_core::actor::ActorId;
        use sigilfall_core::event::Event as GameEvent;

        use super::*;
        use crate::message_log::Tone;

        /// Creatures whose blows are known in advance.
        /// - the gnat deals exactly 1 to the player and outlasts any fight;
        /// - the sloth has 999 hit points and acts once per hundred actions
        ///   of the player (speed 1);
        /// - the moth dies from any blow of the player;
        /// - the executioner kills the player in one blow;
        /// - the hammer deals 3 to 5 to the player, so the player dies in
        ///   6 to 10 blows, the exact number depending on the seed;
        /// - the brute has 40 hit points and 2 defense.
        const MONSTERS: &str = r#"[
            Monster(id: "gnat", name: "cave gnat", family: Vermin, elite: false,
                speed: 100, hp: 999, attack: 0, defense: 0, min_depth: 1),
            Monster(id: "sloth", name: "sloth", family: Vermin, elite: false,
                speed: 1, hp: 999, attack: 0, defense: 0, min_depth: 1),
            Monster(id: "moth", name: "wax moth", family: Vermin, elite: false,
                speed: 100, hp: 1, attack: 0, defense: 0, min_depth: 1),
            Monster(id: "executioner", name: "executioner", family: Acolyte, elite: true,
                speed: 100, hp: 999, attack: 99, defense: 0, min_depth: 1),
            Monster(id: "hammer", name: "iron hammer", family: Homunculus, elite: false,
                speed: 100, hp: 999, attack: 5, defense: 0, min_depth: 1),
            Monster(id: "brute", name: "brute", family: Acolyte, elite: false,
                speed: 100, hp: 40, attack: 0, defense: 2, min_depth: 1),
        ]"#;

        /// Column of the first character of the side panel's content, and
        /// its last on an 80-column screen.
        const PANEL_LEFT: u16 = 60;
        const PANEL_RIGHT: u16 = 78;
        /// Rows of the health value and of the bar.
        const HP_ROW: u16 = 1;
        const BAR_ROW: u16 = 2;
        /// Rows of the log's messages.
        const LOG_ROWS: std::ops::RangeInclusive<u16> = 18..=22;
        /// Messages the log keeps (T16: "100 entries"), written out here so
        /// that a change of the limit is noticed.
        const HISTORY: usize = 100;

        /// The rows [`LOG_ROWS`] of the log of `app` as the game screen
        /// draws it at 80 × 24 (separator on row 17), for the fatal blow:
        /// the death summary that replaces the game screen (T25) does not
        /// show the log.
        fn drawn_log(app: &App) -> Vec<String> {
            let mut buffer = Buffer::empty(Rect::new(0, 0, 80, 24));
            let area = Rect::new(0, *LOG_ROWS.start() - 1, 80, 6);
            ui::log::render(&mut buffer, area, None, app.log(), &app.theme);
            rows(&buffer, LOG_ROWS)
        }
        /// Cells of the health bar.
        const BAR_CELLS: usize = 18;

        /// The seeds named by the brief.
        fn seeds() -> [Seed; 3] {
            [
                Seed::new(0),
                Seed::new(42),
                "7F3A-91C2-0B5E-D4A8".parse().expect("valid grouped seed"),
            ]
        }

        /// The default theme of Windows Terminal.
        fn dark() -> Theme {
            Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode)
        }

        /// A fight: the application, its twin game, and the name of each
        /// monster by key.
        struct Fight {
            app: App,
            twin: Game,
            names: HashMap<ActorId, String>,
            ids: Vec<ActorId>,
            /// The messages the log must hold, oldest first, as
            /// `(prefix, text)`.
            expected: Vec<(char, String)>,
        }

        impl Fight {
            /// A fixed-room game of `seed` with one monster per
            /// `(definition, cell)`, in the application and in its twin.
            fn new(seed: Seed, theme: Theme, monsters: &[(&str, Position)]) -> Self {
                Self::with_content(seed, theme, MONSTERS, monsters)
            }

            /// As [`Fight::new`], with the creatures of `file`.
            fn with_content(
                seed: Seed,
                theme: Theme,
                file: &str,
                monsters: &[(&str, Position)],
            ) -> Self {
                let content = Content::parse(file).expect("valid test monsters");
                let mut game = Game::in_fixed_room(seed);
                let mut twin = Game::in_fixed_room(seed);
                let mut names = HashMap::new();
                let mut ids = Vec::new();
                for &(definition, cell) in monsters {
                    let def = content.monster(definition).expect("known monster");
                    let id = game.spawn_monster(cell, def);
                    assert_eq!(twin.spawn_monster(cell, def), id, "twin keys agree");
                    names.insert(id, def.name().to_owned());
                    ids.push(id);
                }
                Self {
                    app: App::with_game(theme, game, content),
                    twin,
                    names,
                    ids,
                    expected: vec![(' ', "You enter the dungeon.".to_owned())],
                }
            }

            /// Presses `code` in the application and plays `action` in the
            /// twin, which tells what the log must gain. Returns the twin's
            /// events.
            fn play(&mut self, code: KeyCode, action: Action) -> Vec<GameEvent> {
                self.app.handle_key(press(code));
                let events = self.twin.submit(action);
                for event in &events {
                    if let GameEvent::Attacked {
                        attacker,
                        target,
                        damage,
                        hp_left,
                    } = event
                    {
                        let line = self.blow_line(*attacker, *target, *damage, *hp_left);
                        self.expected.push(line);
                    }
                }
                events
            }

            /// Presses `l` (east) in the application and the twin.
            fn hit_east(&mut self) -> Vec<GameEvent> {
                self.play(KeyCode::Char('l'), Action::Move(Direction::East))
            }

            /// Presses `.` (wait) in the application and the twin.
            fn wait(&mut self) -> Vec<GameEvent> {
                self.play(KeyCode::Char('.'), Action::Wait)
            }

            /// The line a blow must produce: `You hit the sloth (4).`,
            /// `You kill the wax moth (5).`, `The cave gnat hits you (1).`
            /// or `The executioner kills you (99).`, with the prefix of its
            /// tone.
            fn blow_line(
                &self,
                attacker: ActorId,
                target: ActorId,
                damage: i32,
                hp_left: i32,
            ) -> (char, String) {
                let lethal = hp_left == 0;
                if attacker == self.twin.player() {
                    let name = &self.names[&target];
                    if lethal {
                        ('+', format!("You kill the {name} ({damage})."))
                    } else {
                        (' ', format!("You hit the {name} ({damage})."))
                    }
                } else {
                    let name = &self.names[&attacker];
                    let verb = if lethal { "kills" } else { "hits" };
                    ('!', format!("The {name} {verb} you ({damage})."))
                }
            }

            /// The screen at 80 × 24.
            fn screen(&self) -> Buffer {
                render(&self.app, 80, 24).backend().buffer().clone()
            }

            /// What the log should hold, oldest first, newest last, capped
            /// as the history is.
            fn expected_history(&self) -> &[(char, String)] {
                let skip = self.expected.len().saturating_sub(HISTORY);
                &self.expected[skip..]
            }

            /// Checks the whole log and, while the game screen shows it,
            /// the visible rows against the twin's account. The death
            /// summary does not show the log.
            fn assert_log_is_exact(&self, context: &str) {
                let expected = self.expected_history();
                let mut kept: Vec<(char, String)> = self
                    .app
                    .log()
                    .newest_first()
                    .map(|m| (prefix_of(m.tone), m.text.clone()))
                    .collect();
                kept.reverse();
                assert_eq!(kept, expected, "{context}: log content");
                if self.app.state() == State::GameOver {
                    return;
                }

                let shown: Vec<String> = rows(&self.screen(), LOG_ROWS)
                    .into_iter()
                    .map(|line| line.trim_end().to_owned())
                    .filter(|line| !line.is_empty())
                    .collect();
                let wanted: Vec<String> = expected
                    .iter()
                    .rev()
                    .take(5)
                    .rev()
                    .map(|(prefix, text)| format!(" {prefix} {text}"))
                    .collect();
                assert_eq!(shown, wanted, "{context}: visible rows");
            }
        }

        /// The log prefix of a tone, as `docs/design.md` writes it.
        const fn prefix_of(tone: Tone) -> char {
            match tone {
                Tone::Info => ' ',
                Tone::Success => '+',
                Tone::Danger => '!',
                Tone::Sigil => '~',
            }
        }

        /// Rows `range` of `buffer`, as text.
        fn rows(buffer: &Buffer, range: std::ops::RangeInclusive<u16>) -> Vec<String> {
            range.map(|y| row_text(buffer, y)).collect()
        }

        /// The side panel's content on row `y`, trimmed.
        fn panel_text(buffer: &Buffer, y: u16) -> String {
            (PANEL_LEFT..=PANEL_RIGHT)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim()
                .to_owned()
        }

        /// The map area of an 80 × 24 `buffer`, as text rows.
        fn map_text(buffer: &Buffer) -> Vec<String> {
            let area = ui::game::map_area(buffer.area);
            (area.top()..area.bottom())
                .map(|y| {
                    (area.left()..area.right())
                        .map(|x| buffer[(x, y)].symbol())
                        .collect()
                })
                .collect()
        }

        /// The glyph drawn where the map cell `cell` is, on `app`'s 80 × 24
        /// screen.
        fn glyph_at(app: &App, cell: Position) -> String {
            let at = camera(app, 80, 24)
                .screen_position(cell)
                .unwrap_or_else(|| panic!("{cell:?} is off screen"));
            render(app, 80, 24).backend().buffer()[(at.x, at.y)]
                .symbol()
                .to_owned()
        }

        /// The cell east of the player's start.
        fn east() -> Position {
            PLAYER_START.step(Direction::East)
        }

        /// Health read back from the panel: `(hp, max, filled cells, empty
        /// cells)`.
        fn panel_health(buffer: &Buffer) -> (i32, i32, usize, usize) {
            let value = panel_text(buffer, HP_ROW);
            let figures = value
                .trim_start_matches("HP")
                .trim()
                .trim_start_matches('!')
                .trim();
            let (hp, max) = figures
                .split_once('/')
                .unwrap_or_else(|| panic!("no hp/max on {value:?}"));
            let bar = panel_text(buffer, BAR_ROW);
            let full = bar.chars().filter(|&c| c == '█').count();
            let empty = bar.chars().filter(|&c| c == '░').count();
            (hp.parse().unwrap(), max.parse().unwrap(), full, empty)
        }

        /// The number after "Turn" on the status line.
        fn turn_shown(app: &App) -> u64 {
            let first = row_text(render(app, 80, 24).backend().buffer(), 0);
            first
                .trim()
                .split_once("Turn ")
                .and_then(|(_, rest)| rest.split_whitespace().next())
                .and_then(|value| value.parse().ok())
                .unwrap_or_else(|| panic!("no turn counter on {first:?}"))
        }

        // --- Scenario 1 -------------------------------------------------

        #[test]
        fn s1_moving_into_a_monster_attacks_it_and_the_damage_is_in_the_log() {
            for seed in seeds() {
                let mut fight = Fight::new(seed, dark(), &[("brute", east())]);
                let start = player_position(&fight.app);

                let events = fight.hit_east();

                let GameEvent::Attacked { damage, .. } = events[0] else {
                    panic!("seed {seed}: no blow: {events:?}");
                };
                assert!((1..=3).contains(&damage), "4 - 2 +/- 1, got {damage}");
                assert_eq!(player_position(&fight.app), start, "the player stays");
                assert_eq!(
                    fight.app.game().actor(fight.ids[0]).unwrap().position(),
                    east(),
                    "the monster stays"
                );
                fight.assert_log_is_exact(&format!("seed {seed}"));
                let log = rows(&fight.screen(), LOG_ROWS);
                assert!(
                    log.iter()
                        .any(|l| l.trim_end() == format!("   You hit the brute ({damage}).")),
                    "{log:#?}"
                );
                assert_eq!(
                    glyph_at(&fight.app, east()),
                    "a",
                    "the brute is still drawn"
                );
            }
        }

        #[test]
        fn s1_the_player_bar_and_value_follow_the_core_after_every_blow() {
            for seed in seeds() {
                let mut fight = Fight::new(seed, dark(), &[("gnat", east())]);
                let mut last_cells = BAR_CELLS;
                let mut stages = Vec::new();
                for turn in 0..40 {
                    fight.hit_east();
                    if fight.app.state() != State::Playing {
                        break;
                    }
                    let hp = fight.app.game().player_actor().hp();
                    let (shown, max, full, empty) = panel_health(&fight.screen());
                    let context = format!("seed {seed} turn {turn}");
                    assert_eq!((shown, max), (hp, 30), "{context}: value");
                    assert_eq!(full + empty, BAR_CELLS, "{context}: bar width");
                    assert!(full >= 1, "{context}: a living player has a cell");
                    assert_eq!(
                        full == BAR_CELLS,
                        hp == 30,
                        "{context}: only full health fills"
                    );
                    assert!(full <= last_cells, "{context}: the bar never grows");
                    // Within one cell of the true share, rounded up.
                    let share = f64::from(hp) * 18.0 / 30.0;
                    assert!(
                        (full as f64 - share).abs() < 1.0 + 1e-9,
                        "{context}: {full} cells for {hp}/30"
                    );
                    last_cells = full;
                    stages.push((hp, full));
                }
                assert!(
                    stages.len() > 20,
                    "seed {seed}: the fight was too short to prove anything"
                );
                assert!(
                    stages.last().unwrap().1 < BAR_CELLS / 2,
                    "seed {seed}: {stages:?}"
                );
            }
        }

        #[test]
        fn s1_the_panel_gives_each_monster_its_current_and_maximum_health() {
            // D25 (2026-11-08): the "in view" line of a monster ends with
            // its hit points, current out of maximum, as figures only.
            let mut fight = Fight::new(Seed::new(42), dark(), &[("brute", east())]);
            fight.hit_east();
            let hp = fight.app.game().actor(fight.ids[0]).unwrap().hp();
            assert!(hp < 40, "the blow wounded the brute");
            let screen = fight.screen();
            // The "in view" list, from its title down: the essence and the
            // sigils count above it are figures of the player.
            let panel: Vec<String> = (3..=16)
                .map(|y| panel_text(&screen, y))
                .skip_while(|line| line != "IN VIEW")
                .collect();
            assert!(!panel.is_empty(), "the panel lists what is in view");

            let line = panel
                .iter()
                .find(|l| l.starts_with("a brute"))
                .unwrap_or_else(|| panic!("{panel:#?}"));
            assert!(line.ends_with(&format!(" {hp}/40")), "{panel:#?}");
            assert!(!panel.iter().any(|l| l.contains('█')), "{panel:#?}");
        }

        // --- Scenario 2 -------------------------------------------------

        #[test]
        fn s2_a_dead_monster_disappears_from_the_map_and_the_log_announces_it() {
            for seed in seeds() {
                let mut fight = Fight::new(seed, dark(), &[("moth", east())]);
                assert_eq!(glyph_at(&fight.app, east()), "v", "seed {seed}");

                let events = fight.hit_east();

                assert!(
                    events.iter().any(|e| matches!(e, GameEvent::Died { .. })),
                    "seed {seed}"
                );
                assert!(fight.app.game().actor(fight.ids[0]).is_none());
                let screen = fight.screen();
                assert!(
                    !map_text(&screen).iter().any(|row| row.contains('v')),
                    "seed {seed}: the corpse is still drawn"
                );
                assert_eq!(
                    glyph_at(&fight.app, east()),
                    "·",
                    "the cell shows floor again"
                );
                fight.assert_log_is_exact(&format!("seed {seed}"));
                let log = rows(&screen, LOG_ROWS);
                let kills = log.iter().filter(|l| l.contains("wax moth")).count();
                assert_eq!(kills, 1, "announced exactly once: {log:#?}");
                assert!(
                    log.iter()
                        .any(|l| l.starts_with(" + You kill the wax moth (")),
                    "{log:#?}"
                );
                // The freed cell can be entered.
                fight.hit_east();
                assert_eq!(player_position(&fight.app), east(), "seed {seed}");
            }
        }

        #[test]
        fn s2_one_of_two_monsters_dies_and_only_that_one_goes() {
            let west = PLAYER_START.step(Direction::West);
            let mut fight = Fight::new(Seed::new(42), dark(), &[("moth", east()), ("gnat", west)]);
            fight.hit_east();

            let screen = fight.screen();
            assert!(fight.app.game().actor(fight.ids[0]).is_none());
            assert!(fight.app.game().actor(fight.ids[1]).is_some());
            assert_eq!(glyph_at(&fight.app, east()), "·");
            assert_eq!(glyph_at(&fight.app, west), "v", "the gnat is still there");
            fight.assert_log_is_exact("two monsters");
            let panel: Vec<String> = (3..=10).map(|y| panel_text(&screen, y)).collect();
            let gnat_hp = fight.app.game().actor(fight.ids[1]).unwrap().hp();
            let gnat = format!("v cave gnat {gnat_hp}/999");
            assert!(panel.contains(&gnat), "{panel:#?}");
            assert!(!panel.iter().any(|l| l.contains("moth")), "{panel:#?}");
        }

        #[test]
        fn a_killing_blow_is_named_whatever_the_overkill() {
            // The moth has 1 hit point; the blow deals 3 to 5, and the log
            // gives the blow's damage, not the hit points that were left.
            for seed in seeds() {
                let mut fight = Fight::new(seed, dark(), &[("moth", east())]);
                let events = fight.hit_east();
                let GameEvent::Attacked {
                    damage, hp_left, ..
                } = events[0]
                else {
                    panic!("{events:?}")
                };
                assert_eq!(hp_left, 0);
                assert!(damage >= 3, "the formula gives 3 to 5, got {damage}");
                fight.assert_log_is_exact(&format!("seed {seed}"));
                assert_eq!(
                    fight.expected.last().unwrap(),
                    &('+', format!("You kill the wax moth ({damage})."))
                );
            }
        }

        // --- Scenario 3 -------------------------------------------------

        /// A fight with the hammer, played until the player dies (bounded).
        /// Returns it with the turn shown on the status line when the
        /// fatal action was played.
        fn fight_to_death(seed: Seed) -> (Fight, u64) {
            let mut fight = Fight::new(seed, dark(), &[("hammer", east())]);
            let mut turn_of_death = 0;
            for _ in 0..60 {
                if fight.app.state() == State::GameOver {
                    break;
                }
                turn_of_death = turn_shown(&fight.app);
                fight.wait();
            }
            assert_eq!(fight.app.state(), State::GameOver, "seed {seed}");
            (fight, turn_of_death)
        }

        #[test]
        fn s3_the_death_of_the_player_stops_the_game_and_opens_the_end_screen() {
            for seed in seeds() {
                let (fight, turn) = fight_to_death(seed);

                assert!(fight.app.game().is_over());
                assert_eq!(fight.app.game().player_actor().hp(), 0);
                assert_eq!(fight.app.game().player_turns(), turn);
                fight.assert_log_is_exact(&format!("seed {seed}"));
                let text = rows(&fight.screen(), 0..=23).join("\n");
                assert!(text.contains("T H E   D E S C E N T   E N D S"), "{text}");
                assert!(
                    text.contains(&format!(
                        "Killed by an iron hammer at depth 1, on turn {turn}."
                    )),
                    "cause and turn, seed {seed}:\n{text}"
                );
                assert!(text.contains(&format!("SEED   {seed}")), "{text}");
                assert!(!text.contains("Turn "), "no game screen:\n{text}");
                assert!(!text.contains("IN VIEW"), "no side panel:\n{text}");
                let last = fight.expected.last().unwrap();
                assert!(
                    last.1.starts_with("The iron hammer kills you ("),
                    "{last:?}"
                );
            }
        }

        #[test]
        fn s3_the_seed_on_the_end_screen_is_reusable_as_it_is_written() {
            for seed in seeds() {
                let (fight, _) = fight_to_death(seed);
                let text = rows(&fight.screen(), 0..=23).join("\n");

                // The seed is written in its frame after "SEED" and in the
                // replay command.
                let shown = text
                    .lines()
                    .find_map(|l| l.split_once("SEED"))
                    .map(|(_, rest)| rest.trim().trim_end_matches('│').trim())
                    .expect("a seed line");
                let hint = text
                    .lines()
                    .find_map(|l| l.trim().strip_prefix("Replay this seed: sigilfall --seed "))
                    .expect("a replay line")
                    .trim();
                assert_eq!(shown, hint);
                let parsed: Seed = shown.parse().expect("the shown seed parses");
                assert_eq!(parsed, seed);

                // Replaying it with the same keys gives the same death.
                let (replay, _) = fight_to_death(parsed);
                assert_eq!(
                    rows(&replay.screen(), 0..=23),
                    rows(&fight.screen(), 0..=23),
                    "seed {seed}: the replay differs"
                );
            }
            // And the seed does drive the blows.
            let ends: Vec<_> = seeds()
                .into_iter()
                .map(|seed| fight_to_death(seed).0.expected)
                .collect();
            assert!(
                ends[0] != ends[1] || ends[1] != ends[2],
                "the seed drives nothing"
            );
        }

        /// Every key but the quit ones and Enter, which goes back to the
        /// title screen since T26, with modifiers that matter.
        fn all_non_quit_keys() -> Vec<KeyEvent> {
            let mut keys = Vec::new();
            for c in (' '..='~').filter(|&c| c != 'q') {
                keys.push(press(KeyCode::Char(c)));
                keys.push(KeyEvent::new(KeyCode::Char(c), KeyModifiers::SHIFT));
                keys.push(KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT));
                if c != 'c' {
                    keys.push(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
                }
            }
            keys.push(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL));
            keys.push(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::ALT));
            for code in [
                KeyCode::Esc,
                KeyCode::Tab,
                KeyCode::BackTab,
                KeyCode::Backspace,
                KeyCode::Delete,
                KeyCode::Insert,
                KeyCode::Up,
                KeyCode::Down,
                KeyCode::Left,
                KeyCode::Right,
                KeyCode::Home,
                KeyCode::End,
                KeyCode::PageUp,
                KeyCode::PageDown,
                KeyCode::KeypadBegin,
            ] {
                keys.push(press(code));
            }
            keys.extend((1..=12).map(|n| press(KeyCode::F(n))));
            // A release or a repeat of a quit key is not a press.
            keys.push(release(KeyCode::Char('q')));
            keys.push(KeyEvent::new_with_kind(
                KeyCode::Char('q'),
                KeyModifiers::NONE,
                KeyEventKind::Repeat,
            ));
            keys
        }

        #[test]
        fn s3_after_death_no_key_but_quit_has_any_effect() {
            for seed in seeds() {
                let (mut fight, _) = fight_to_death(seed);
                let before = fight.screen();
                let game = fight.app.game().clone();
                let log = fight.app.log().clone();

                let keys = all_non_quit_keys();
                assert!(keys.len() > 300, "the key set is broad");
                for key in keys {
                    fight.app.handle_key(key);
                    assert_eq!(
                        fight.app.state(),
                        State::GameOver,
                        "{key:?} left the end screen"
                    );
                }

                assert_eq!(fight.app.game().ticks(), game.ticks());
                assert_eq!(fight.app.game().player_turns(), game.player_turns());
                assert_eq!(fight.app.game().player_actor().hp(), 0);
                assert_eq!(
                    fight.app.game().player_actor().position(),
                    game.player_actor().position()
                );
                assert_eq!(fight.app.game().status(), game.status());
                assert_eq!(*fight.app.log(), log, "the log is untouched");
                assert_eq!(fight.screen(), before, "the screen is untouched");
            }
        }

        #[test]
        fn s3_every_quit_key_still_works_after_death() {
            let quits = [
                press(KeyCode::Char('q')),
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            ];
            for key in quits {
                let (fight, _) = fight_to_death(Seed::new(42));
                let clock = ManualClock::new();
                let mut app = fight.app.with_clock(clock.clone());
                clock.advance(END_SCREEN_GRACE);
                app.handle_key(key);
                assert_eq!(app.state(), State::Quit, "{key:?}");
            }
        }

        #[test]
        fn s3_right_after_death_only_ctrl_c_quits() {
            let (fight, _) = fight_to_death(Seed::new(42));
            let mut app = fight.app;
            app.handle_key(press(KeyCode::Char('q')));
            app.handle_key(press(KeyCode::Enter));
            assert_eq!(app.state(), State::GameOver, "held back by the grace");
            app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
            assert_eq!(app.state(), State::Quit);
        }

        #[test]
        fn s3_the_end_screen_survives_a_resize_and_fits_larger_terminals() {
            let (mut fight, _) = fight_to_death(Seed::new(42));
            let big = rows(&fight.screen(), 0..=23);

            render(&fight.app, 60, 20);
            fight.app.handle_key(press(KeyCode::Char('l')));
            assert_eq!(fight.app.state(), State::GameOver);

            assert_eq!(rows(&fight.screen(), 0..=23), big);
            for (width, height) in [(120, 40), (200, 60)] {
                let text = render(&fight.app, width, height).backend().to_string();
                assert!(text.contains("T H E   D E S C E N T   E N D S"), "{text}");
                assert!(text.contains("SEED   0000-0000-0000-002A"), "{text}");
                assert!(
                    text.contains("Killed by an iron hammer at depth 1"),
                    "{text}"
                );
                assert!(text.contains("q quit"), "{text}");
            }
        }

        // --- Journal ----------------------------------------------------

        #[test]
        fn a_long_exchange_scrolls_and_the_log_matches_the_core_blow_for_blow() {
            for seed in seeds() {
                let mut fight = Fight::new(seed, dark(), &[("gnat", east())]);
                let mut turns = 0;
                while fight.app.state() == State::Playing && turns < 60 {
                    let before = fight.app.log().len();
                    let events = fight.hit_east();
                    let blows = events
                        .iter()
                        .filter(|e| matches!(e, GameEvent::Attacked { .. }))
                        .count();
                    assert_eq!(
                        fight.app.log().len(),
                        (before + blows).min(HISTORY),
                        "seed {seed} turn {turns}: one line per blow, no more"
                    );
                    fight.assert_log_is_exact(&format!("seed {seed} turn {turns}"));
                    turns += 1;
                }
                assert!(turns >= 25, "seed {seed}: only {turns} turns");
                assert_eq!(
                    fight.app.state(),
                    State::GameOver,
                    "the gnat wins in the end"
                );
                // The fatal blow names the killer, whole, on the last row.
                let log = drawn_log(&fight.app);
                assert_eq!(
                    log.last().unwrap().trim_end(),
                    " ! The cave gnat kills you (1)."
                );
            }
        }

        #[test]
        fn the_history_keeps_a_hundred_lines_over_a_very_long_fight() {
            let mut fight = Fight::new(Seed::new(42), dark(), &[("sloth", east())]);
            for turn in 0..130 {
                fight.hit_east();
                if turn % 13 == 0 || turn > 120 {
                    fight.assert_log_is_exact(&format!("turn {turn}"));
                }
            }

            assert_eq!(fight.app.log().len(), HISTORY);
            let all: Vec<String> = fight
                .app
                .log()
                .newest_first()
                .map(|m| m.text.clone())
                .collect();
            assert!(
                !all.iter().any(|t| t.contains("enter the dungeon")),
                "welcome is gone"
            );
            assert!(
                all.iter().all(|t| t.starts_with("You hit the sloth (")
                    || t.starts_with("The sloth hits you (")),
                "{all:?}"
            );
            assert!(
                fight.app.game().actor(fight.ids[0]).is_some(),
                "still alive"
            );
        }

        /// A name longer than the content files accept
        /// (`content::MAX_NAME_LENGTH`).
        const LONG_NAME: &str = "extraordinarily patient executioner of the seventh hollow crypt";

        /// A name of exactly `content::MAX_NAME_LENGTH` characters, the
        /// longest a creature can have.
        const LONGEST_NAME: &str = "eldest crypt executioner";

        /// A monster file with one creature named `name` that kills in one
        /// blow.
        fn long_named_file(name: &str) -> String {
            format!(
                r#"[Monster(id: "long", name: "{name}", family: Acolyte, elite: true,
                    speed: 100, hp: 999, attack: 99, defense: 0, min_depth: 1)]"#
            )
        }

        /// A fight lost against the creature named [`LONGEST_NAME`].
        fn lost_to_the_long_name() -> Fight {
            let file = long_named_file(LONGEST_NAME);
            let mut fight = Fight::with_content(Seed::new(42), dark(), &file, &[("long", east())]);
            for _ in 0..5 {
                if fight.app.state() == State::GameOver {
                    break;
                }
                fight.wait();
            }
            assert_eq!(fight.app.state(), State::GameOver);
            fight
        }

        #[test]
        fn the_longest_monster_name_keeps_the_fatal_blow_whole_in_the_log() {
            let fight = lost_to_the_long_name();

            let log: Vec<String> = drawn_log(&fight.app)
                .into_iter()
                .map(|l| l.trim_end().to_owned())
                .collect();
            let fatal = &fight.expected.last().unwrap().1;
            assert!(fatal.contains(LONGEST_NAME), "{fatal}");
            let joined = log
                .iter()
                .skip_while(|l| !l.starts_with(" ! "))
                .filter(|l| !l.is_empty())
                .map(|l| l[3..].to_owned())
                .collect::<Vec<_>>()
                .join(" ");
            assert_eq!(&joined, fatal, "{log:#?}");
            for line in &log {
                assert!(line.chars().count() <= 79, "{line:?}");
            }
        }

        #[test]
        fn a_long_monster_name_is_not_cut_off_in_the_cause_of_death() {
            // A name too long to show whole is refused when loading...
            let error = Content::parse(&long_named_file(LONG_NAME))
                .expect_err("a name longer than the limit is refused");
            let message = error.to_string();
            assert!(
                message.contains("monster `long` (entry 1), field `name`"),
                "{message}"
            );
            assert_eq!(LONGEST_NAME.chars().count(), 24);

            // ...and the longest accepted one is shown whole.
            let fight = lost_to_the_long_name();
            let text = rows(&fight.screen(), 0..=23).join("\n");

            assert!(
                text.contains(&format!("Killed by an {LONGEST_NAME} at depth 1")),
                "the cause of death is cut off on the end screen:\n{text}"
            );
        }

        // --- Input robustness during a fight ----------------------------

        #[test]
        fn a_press_with_its_release_and_repeats_strikes_exactly_once() {
            let mut fight = Fight::new(Seed::new(42), dark(), &[("sloth", east())]);

            fight.hit_east();
            fight.app.handle_key(KeyEvent::new_with_kind(
                KeyCode::Char('l'),
                KeyModifiers::NONE,
                KeyEventKind::Repeat,
            ));
            fight.app.handle_key(release(KeyCode::Char('l')));

            assert_eq!(fight.app.game().player_turns(), 1);
            fight.assert_log_is_exact("press, repeat, release");
        }

        #[test]
        fn a_too_small_terminal_blocks_blows_and_the_fight_resumes_when_enlarged() {
            let mut fight = Fight::new(Seed::new(42), dark(), &[("sloth", east())]);
            render(&fight.app, 79, 23);
            fight.app.handle_key(press(KeyCode::Char('l')));
            assert_eq!(fight.app.game().player_turns(), 0, "no blow in the dark");
            assert_eq!(fight.app.log().len(), 1);

            render(&fight.app, 80, 24);
            fight.hit_east();
            fight.assert_log_is_exact("after enlarging");
            assert_eq!(fight.app.game().player_turns(), 1);
        }

        #[test]
        fn drawing_again_and_again_during_a_fight_changes_nothing() {
            let mut fight = Fight::new(Seed::new(42), dark(), &[("gnat", east())]);
            fight.hit_east();
            let first = fight.screen();
            let log = fight.app.log().clone();
            let turns = fight.app.game().player_turns();
            for _ in 0..50 {
                render(&fight.app, 80, 24);
            }
            assert_eq!(fight.screen(), first);
            assert_eq!(*fight.app.log(), log);
            assert_eq!(fight.app.game().player_turns(), turns);
        }
    }

    mod sigil_overlay {
        //! The sigil overlay driven by simulated keys (T19): opening,
        //! linking, undoing, cancelling, casting with and without a
        //! direction, verdicts, essence, glyphs on the map, and the styles
        //! of all of it in the four rendering modes.

        use ratatui::style::{Color, Modifier, Style};
        use sigilfall_core::sigil::{CastQuality, ESSENCE_COST, Node};

        use super::*;
        use crate::message_log::Tone;

        /// A monster that never threatens the tests: the cave gnat has
        /// 999 hit points and deals 1 damage per blow.
        const GNAT: &str = r#"[
            Monster(id: "gnat", name: "cave gnat", family: Vermin, elite: false,
                speed: 100, hp: 999, attack: 0, defense: 0, min_depth: 1),
        ]"#;

        /// Where the gnat stands when a test wants a target in sight, four
        /// cells east and two up from the start: in view, not adjacent.
        const GNAT_CELL: Position = Position::new(9, 3);

        /// The four rendering modes of `docs/design.md`.
        const MODES: [ColorMode; 4] = [
            ColorMode::TrueColorDark,
            ColorMode::TrueColorLight,
            ColorMode::Ansi16,
            ColorMode::Monochrome,
        ];

        /// Screen column and row of the top-left node (7) on an 80 × 24
        /// screen: the overlay fills the map's 58 columns from row 1, its
        /// frame takes one cell, and the grid starts 2 columns and 1 row
        /// inside it.
        const GRID_ORIGIN: (u16, u16) = (3, 3);

        /// The test content: the gnat and the shipped sigils.
        fn content() -> Content {
            Content::parse(GNAT).expect("valid test monsters")
        }

        /// A fixed-room game of [`SEED`] where the player has learnt the
        /// shipped sigils `ids` by stepping on their glyphs, engraved in a
        /// row east of the start, and walked back to the start; with the
        /// gnat at [`GNAT_CELL`] if `gnat` is set.
        fn game_knowing(ids: &[&str], gnat: bool) -> Game {
            let content = content();
            let mut game = Game::in_fixed_room(SEED);
            let mut cell = PLAYER_START;
            for id in ids {
                cell = cell.step(Direction::East);
                game.engrave(cell, content.sigil(id).expect("a shipped sigil"));
            }
            for _ in ids {
                game.submit(Action::Move(Direction::East));
            }
            for _ in ids {
                game.submit(Action::Move(Direction::West));
            }
            assert_eq!(game.grimoire().len(), ids.len(), "every glyph was read");
            assert_eq!(game.player_actor().position(), PLAYER_START);
            if gnat {
                game.spawn_monster(GNAT_CELL, content.monster("gnat").unwrap());
            }
            game
        }

        /// An application on [`game_knowing`] drawn with `theme`.
        fn app_knowing(theme: Theme, ids: &[&str], gnat: bool) -> App {
            App::with_game(theme, game_knowing(ids, gnat), content())
        }

        /// The default theme of Windows Terminal (see [`new_app`]).
        fn dark() -> Theme {
            Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode)
        }

        /// The key of a digit.
        const fn digit(d: char) -> KeyCode {
            KeyCode::Char(d)
        }

        /// Opens the overlay and presses each digit of `digits`.
        fn trace(app: &mut App, digits: &str) {
            app.handle_key(press(KeyCode::Char('t')));
            for d in digits.chars() {
                app.handle_key(press(digit(d)));
            }
        }

        /// The digits of the trace being drawn, or `None` when the overlay
        /// is not on the drawing step.
        fn drawn(app: &App) -> Option<Vec<u8>> {
            match app.state() {
                State::Casting(Casting::Drawing(drawing)) => {
                    Some(drawing.trace().nodes().iter().map(|n| n.digit()).collect())
                }
                _ => None,
            }
        }

        /// The whole screen of `app` at 80 × 24.
        fn screen(app: &App) -> Buffer {
            render(app, 80, 24).backend().buffer().clone()
        }

        /// The screen of `app` at 80 × 24, as lines of text.
        fn text(app: &App) -> String {
            render(app, 80, 24).backend().to_string()
        }

        /// The texts of the log, oldest first.
        fn log_texts(app: &App) -> Vec<String> {
            let mut texts: Vec<String> = app.log().newest_first().map(|m| m.text.clone()).collect();
            texts.reverse();
            texts
        }

        /// What the game shows of the player and the clock, to check that
        /// nothing happened.
        fn clock(app: &App) -> (u64, u64, i32, Position) {
            let game = app.game();
            (
                game.ticks(),
                game.player_turns(),
                game.player_actor().essence(),
                game.player_actor().position(),
            )
        }

        /// Screen cell of the grid node `digit` on an 80 × 24 screen.
        fn node_cell(digit: u8) -> (u16, u16) {
            let node = Node::new(digit).unwrap();
            let column = u16::from(node.column());
            let row_from_top = 2 - u16::from(node.row());
            (GRID_ORIGIN.0 + column * 6, GRID_ORIGIN.1 + row_from_top * 3)
        }

        /// Asserts that the cell `(x, y)` of `buffer` shows `glyph` with
        /// exactly the colour and emphasis of `style`.
        fn assert_cell(buffer: &Buffer, (x, y): (u16, u16), glyph: char, style: Style, what: &str) {
            let cell = &buffer[(x, y)];
            assert_eq!(cell.symbol(), glyph.to_string(), "{what}");
            assert_eq!(cell.fg, style.fg.unwrap_or(Color::Reset), "{what}");
            assert_eq!(cell.modifier, style.add_modifier, "{what}");
        }

        /// The gnat's hit points.
        fn gnat_hp(app: &App) -> i32 {
            app.game()
                .actors()
                .find(|(_, actor)| !actor.is_player())
                .map(|(_, actor)| actor.hp())
                .expect("the gnat is alive")
        }

        #[test]
        fn t_opens_an_empty_overlay_that_explains_how_to_learn_sigils() {
            let mut app = new_app();

            app.handle_key(press(KeyCode::Char('t')));

            assert_eq!(drawn(&app), Some(vec![]));
            assert_snapshot!(text(&app));
        }

        #[test]
        fn the_trace_is_drawn_stroke_by_stroke_beside_the_grimoire() {
            let mut app = app_knowing(
                dark(),
                &["ember", "recall", "veil", "passage", "lance"],
                false,
            );

            trace(&mut app, "785");

            assert_eq!(drawn(&app), Some(vec![7, 8, 5]));
            let text = text(&app);
            assert!(text.contains("Trace    7·8·5"), "{text}");
            assert!(
                text.contains("Time     1.25 turns · +0.25 a node"),
                "{text}"
            );
            assert_snapshot!(text);
        }

        #[test]
        fn the_time_shown_grows_by_a_quarter_turn_per_node() {
            let mut app = new_app();
            app.handle_key(press(KeyCode::Char('t')));
            let mut shown = vec![];
            for d in ['1', '2', '3', '6'] {
                app.handle_key(press(digit(d)));
                let line = text(&app)
                    .lines()
                    .find(|line| line.contains("Time "))
                    .map(str::to_owned)
                    .unwrap();
                shown.push(line.split_whitespace().nth(2).unwrap().to_owned());
            }

            assert_eq!(shown, ["0.75", "1.00", "1.25", "1.50"]);
        }

        #[test]
        fn an_exact_trace_casts_the_spell_and_time_passes() {
            let mut app = app_knowing(dark(), &["ember"], true);
            let (ticks, turns, essence, _) = clock(&app);
            let hp = gnat_hp(&app);

            trace(&mut app, "753");
            assert_eq!(clock(&app).0, ticks, "no time passes while tracing");
            app.handle_key(press(KeyCode::Enter));

            assert_eq!(
                app.state(),
                State::Playing,
                "a pure cast closes the overlay"
            );
            let (after_ticks, after_turns, after_essence, _) = clock(&app);
            // 3 strokes and the validation cost 125 energy: at speed 100,
            // two ticks pass before the player's next turn, against one for
            // a step.
            assert_eq!(after_ticks, ticks + 2);
            assert_eq!(after_turns, turns + 1);
            assert_eq!(after_essence, essence - ESSENCE_COST);
            assert_eq!(gnat_hp(&app), hp - 6, "Ember harms for 6");
            let cast = app
                .log()
                .newest_first()
                .find(|m| m.tone == Tone::Sigil)
                .expect("the cast is logged");
            assert_eq!(cast.text, "Ember, pure trace: the cave gnat is seared (6).");
        }

        #[test]
        fn escape_closes_the_overlay_at_no_cost() {
            let mut app = app_knowing(dark(), &["ember"], true);
            let before = clock(&app);
            let log = app.log().clone();

            trace(&mut app, "75");
            app.handle_key(press(KeyCode::Esc));

            assert_eq!(app.state(), State::Playing);
            assert_eq!(clock(&app), before, "no time, essence or move spent");
            assert_eq!(*app.log(), log);
            app.handle_key(press(KeyCode::Char('t')));
            assert_eq!(drawn(&app), Some(vec![]), "a new trace starts empty");
        }

        #[test]
        fn backspace_removes_the_last_node_only() {
            let mut app = new_app();

            trace(&mut app, "73");
            assert_eq!(drawn(&app), Some(vec![7, 5, 3]), "7 to 3 links 5");
            app.handle_key(press(KeyCode::Backspace));
            assert_eq!(drawn(&app), Some(vec![7, 5]));
            assert!(text(&app).contains("Trace    7·5 "));
            for _ in 0..3 {
                app.handle_key(press(KeyCode::Backspace));
            }
            assert_eq!(drawn(&app), Some(vec![]), "undo stops at an empty trace");
            app.handle_key(press(digit('9')));
            assert_eq!(drawn(&app), Some(vec![9]));
        }

        #[test]
        fn a_node_already_linked_is_refused_and_the_overlay_says_so() {
            let mut app = new_app();
            let before = clock(&app);

            trace(&mut app, "755");

            assert_eq!(drawn(&app), Some(vec![7, 5]), "the trace is unchanged");
            assert!(text(&app).contains("! Node 5 is already linked."));
            app.handle_key(press(digit('3')));
            assert_eq!(drawn(&app), Some(vec![7, 5, 3]));
            assert!(!text(&app).contains("already linked"), "the notice goes");
            assert_eq!(clock(&app), before);
        }

        #[test]
        fn an_empty_trace_is_never_submitted() {
            let mut app = new_app();
            let before = clock(&app);
            let log = app.log().clone();

            app.handle_key(press(KeyCode::Char('t')));
            app.handle_key(press(KeyCode::Enter));
            app.handle_key(press(KeyCode::Enter));

            assert_eq!(drawn(&app), Some(vec![]), "the overlay stays open");
            assert!(text(&app).contains("! Link at least one node first."));
            assert_eq!(clock(&app), before);
            assert_eq!(*app.log(), log, "nothing reached the game");
        }

        #[test]
        fn an_aimed_sigil_asks_for_a_direction_before_it_is_cast() {
            let mut app = app_knowing(dark(), &["passage"], false);
            let before = clock(&app);

            trace(&mut app, "74123");
            app.handle_key(press(KeyCode::Enter));

            assert!(
                matches!(app.state(), State::Casting(Casting::Aiming { .. })),
                "{:?}",
                app.state()
            );
            assert_eq!(clock(&app), before, "nothing is cast yet");
            let text = text(&app);
            assert!(text.contains("Aim Passage"), "{text}");
            assert!(
                text.contains("Passage is aimed: choose a direction."),
                "{text}"
            );
            assert!(
                text.contains("←↑→↓/1-9 aim   Esc back to the trace"),
                "{text}"
            );

            // Escape goes back to the trace, kept; Enter asks again.
            app.handle_key(press(KeyCode::Esc));
            assert_eq!(drawn(&app), Some(vec![7, 4, 1, 2, 3]));
            app.handle_key(press(KeyCode::Enter));
            app.handle_key(press(KeyCode::Char('5')));
            assert!(matches!(
                app.state(),
                State::Casting(Casting::Aiming { .. })
            ));

            app.handle_key(press(KeyCode::Right));

            assert_eq!(app.state(), State::Playing);
            let (_, turns, essence, position) = clock(&app);
            assert_eq!(turns, before.1 + 1);
            assert_eq!(essence, before.2 - ESSENCE_COST);
            assert_eq!(position, Position::new(PLAYER_START.x + 4, PLAYER_START.y));
            assert_eq!(
                log_texts(&app).last().unwrap(),
                "Passage, pure trace: you blink 4 cells."
            );
        }

        #[test]
        fn an_unstable_trace_shows_its_verdict_until_the_next_key() {
            let mut app = app_knowing(dark(), &["ember"], true);
            let hp = gnat_hp(&app);

            trace(&mut app, "752");
            app.handle_key(press(KeyCode::Enter));

            let State::Casting(Casting::Verdict(verdict)) = app.state() else {
                panic!("{:?}", app.state());
            };
            assert_eq!(verdict.quality, CastQuality::Unstable);
            assert_eq!(gnat_hp(&app), hp - 6, "the effect still applies");
            let log = log_texts(&app);
            assert!(
                log.iter()
                    .any(|t| t == "Ember, unstable trace: the cave gnat is seared (6)."),
                "{log:#?}"
            );
            assert!(
                log.iter().any(|t| t.starts_with("The backlash")),
                "{log:#?}"
            );
            assert_snapshot!(text(&app));

            let after = clock(&app);
            app.handle_key(press(KeyCode::Right));
            assert_eq!(app.state(), State::Playing);
            assert_eq!(clock(&app), after, "the key only closed the verdict");
        }

        #[test]
        fn an_unknown_trace_fizzles_spends_essence_and_shows_its_verdict() {
            let mut app = new_app();
            let before = clock(&app);

            trace(&mut app, "486");
            app.handle_key(press(KeyCode::Enter));

            let State::Casting(Casting::Verdict(verdict)) = app.state() else {
                panic!("{:?}", app.state());
            };
            assert_eq!(verdict.quality, CastQuality::Fizzled);
            let (ticks, turns, essence, _) = clock(&app);
            assert!(ticks > before.0, "the strokes took time");
            assert_eq!(turns, before.1 + 1);
            assert_eq!(essence, before.2 - ESSENCE_COST, "the essence is lost");
            assert_eq!(
                log_texts(&app).last().unwrap(),
                "The trace matches no sigil you know and fizzles."
            );
            let text = text(&app);
            assert!(text.contains(" Enter continue"), "{text}");
            assert_snapshot!(text);

            app.handle_key(press(KeyCode::Enter));
            assert_eq!(app.state(), State::Playing);
        }

        /// Fizzles until the player has less essence than a cast costs,
        /// closing each verdict; bounded by the casts a full reserve pays.
        fn drain_essence(app: &mut App) {
            for _ in 0..sigilfall_core::sigil::MAX_ESSENCE {
                if app.game().player_actor().essence() < ESSENCE_COST {
                    return;
                }
                trace(app, "123");
                app.handle_key(press(KeyCode::Enter));
                app.handle_key(press(KeyCode::Enter));
            }
            panic!("the essence never ran out");
        }

        #[test]
        fn without_essence_a_cast_is_refused_and_takes_no_time() {
            let mut app = app_knowing(dark(), &["ember", "passage"], true);
            drain_essence(&mut app);
            let before = clock(&app);
            let essence = before.2;

            // The overlay warns before the player even validates.
            trace(&mut app, "753");
            let warning = format!("! Not enough essence: {essence} of {ESSENCE_COST}.");
            assert!(text(&app).contains(&warning), "{}", text(&app));
            app.handle_key(press(KeyCode::Enter));

            assert_eq!(app.state(), State::Playing);
            assert_eq!(clock(&app), before, "no time and no essence spent");
            assert_eq!(
                log_texts(&app).last().unwrap(),
                &format!("Not enough essence to cast: {essence} of {ESSENCE_COST}.")
            );
            let panel = text(&app);
            assert!(
                panel.contains(&format!("Essence       ! {essence}/6")),
                "{panel}"
            );

            // An aimed sigil does not ask for a direction it cannot use.
            trace(&mut app, "74123");
            app.handle_key(press(KeyCode::Enter));
            assert_eq!(app.state(), State::Playing);
            assert_eq!(clock(&app), before);
        }

        #[test]
        fn a_glyph_is_drawn_unread_then_read_once_stepped_on() {
            let content = content();
            let mut game = Game::in_fixed_room(SEED);
            let glyph = PLAYER_START.step(Direction::East);
            game.engrave(glyph, content.sigil("recall").unwrap());
            let mut app = App::with_game(dark(), game, content);
            let theme = dark();
            let cell = |app: &App| {
                let at = camera(app, 80, 24).screen_position(glyph).unwrap();
                (at.x, at.y)
            };

            let before = screen(&app);
            assert_cell(&before, cell(&app), '§', theme.engraving_unread, "unread");
            assert!(text(&app).contains("§ carved glyph"));

            press_all(&mut app, &[KeyCode::Right, KeyCode::Left]);

            let after = screen(&app);
            assert_cell(&after, cell(&app), '≡', theme.engraving_read, "read");
            let log = log_texts(&app);
            assert!(
                log.contains(
                    &"You decipher a carved glyph: Recall joins your grimoire.".to_owned()
                ),
                "{log:#?}"
            );
            let text = text(&app);
            assert!(text.contains("≡ known glyph"), "{text}");
            assert!(text.contains("SIGILS          1/5"), "{text}");
            assert!(text.contains("│ Recall"), "{text}");
        }

        /// A game where the player knows Ember, has seen the glyph of Veil
        /// without reading it, and stands where both glyphs, and the one of
        /// Lance further east, are in view.
        fn glyph_room() -> Game {
            let content = content();
            let mut game = Game::in_fixed_room(SEED);
            game.engrave(Position::new(6, 5), content.sigil("ember").unwrap());
            game.engrave(Position::new(8, 3), content.sigil("veil").unwrap());
            game.engrave(Position::new(12, 8), content.sigil("lance").unwrap());
            // Two steps: the first reads Ember, the second leaves its glyph
            // in view beside the player.
            game.submit(Action::Move(Direction::East));
            game.submit(Action::Move(Direction::East));
            game
        }

        #[test]
        fn read_and_unread_glyphs_on_the_map() {
            let app = App::with_game(dark(), glyph_room(), content());

            assert_snapshot!(text(&app));
            let ascii = App::with_game(
                Theme::new(ColorMode::TrueColorDark, GlyphSet::Ascii),
                glyph_room(),
                content(),
            );
            let ascii = text(&ascii);
            assert!(ascii.is_ascii(), "{ascii}");
            assert!(ascii.contains("& carved glyph x2") && ascii.contains("= known glyph"));
        }

        #[test]
        fn glyphs_keep_their_glyph_and_dim_out_of_view_in_every_mode() {
            for mode in MODES {
                for set in [GlyphSet::Unicode, GlyphSet::Ascii] {
                    let theme = Theme::new(mode, set);
                    let glyphs = theme.glyphs;
                    let mut game = glyph_room();
                    let read = Position::new(6, 5);
                    let unread = Position::new(8, 3);
                    let mut app = App::with_game(Theme::new(mode, set), game.clone(), content());
                    let at = |app: &App, cell| {
                        let at = camera(app, 80, 24).screen_position(cell).unwrap();
                        (at.x, at.y)
                    };
                    let what = format!("{mode:?} {set:?}");

                    let shown = screen(&app);
                    assert_cell(
                        &shown,
                        at(&app, read),
                        glyphs.engraving_read,
                        theme.engraving_read,
                        &what,
                    );
                    assert_cell(
                        &shown,
                        at(&app, unread),
                        glyphs.engraving_unread,
                        theme.engraving_unread,
                        &what,
                    );

                    // Walk far east along the clear row 5, to (25, 5): both
                    // glyphs drop out of the 16 cells sight reaches along a
                    // row.
                    for _ in 0..18 {
                        game.submit(Action::Move(Direction::East));
                    }
                    assert!(!game.is_visible(read) && game.is_remembered(read));
                    assert!(!game.is_visible(unread) && game.is_remembered(unread));
                    app = App::with_game(Theme::new(mode, set), game, content());

                    let remembered = screen(&app);
                    assert_cell(
                        &remembered,
                        at(&app, read),
                        glyphs.engraving_read,
                        theme.engraving_read_remembered,
                        &what,
                    );
                    assert_cell(
                        &remembered,
                        at(&app, unread),
                        glyphs.engraving_unread,
                        theme.engraving_unread_remembered,
                        &what,
                    );
                }
            }
        }

        #[test]
        fn the_overlay_draws_with_its_tokens_in_every_mode() {
            for mode in MODES {
                for set in [GlyphSet::Unicode, GlyphSet::Ascii] {
                    let theme = Theme::new(mode, set);
                    let glyphs = theme.glyphs;
                    let mut app =
                        App::with_game(Theme::new(mode, set), Game::in_fixed_room(SEED), content());
                    let what = format!("{mode:?} {set:?}");

                    trace(&mut app, "785");
                    let drawing = screen(&app);
                    assert_cell(
                        &drawing,
                        node_cell(7),
                        glyphs.sigil_linked,
                        theme.sigil_stroke,
                        &what,
                    );
                    assert_cell(
                        &drawing,
                        node_cell(5),
                        glyphs.sigil_pen,
                        theme.sigil_cursor,
                        &what,
                    );
                    assert_cell(
                        &drawing,
                        node_cell(9),
                        glyphs.sigil_node,
                        theme.sigil_node,
                        &what,
                    );
                    let (x, y) = node_cell(7);
                    assert_cell(
                        &drawing,
                        (x + 3, y),
                        glyphs.stroke_horizontal,
                        theme.sigil_stroke,
                        &what,
                    );
                    let (x, y) = node_cell(8);
                    assert_cell(
                        &drawing,
                        (x, y + 1),
                        glyphs.stroke_vertical,
                        theme.sigil_stroke,
                        &what,
                    );
                    assert_eq!(Some(drawing[(x, y + 1)].bg), theme.bg_panel.bg, "{what}");
                    assert_cell(&drawing, (0, 1), glyphs.frame_top_left, theme.border, &what);
                    assert!(
                        theme.sigil_cursor.add_modifier.contains(Modifier::BOLD),
                        "{what}"
                    );

                    app.handle_key(press(KeyCode::Enter));
                    let fizzled = screen(&app);
                    assert_cell(
                        &fizzled,
                        node_cell(5),
                        glyphs.sigil_fault,
                        theme.sigil_failed,
                        &what,
                    );
                    assert_cell(
                        &fizzled,
                        node_cell(7),
                        glyphs.sigil_linked,
                        theme.sigil_failed,
                        &what,
                    );
                    let (x, y) = node_cell(7);
                    assert_cell(
                        &fizzled,
                        (x + 3, y),
                        glyphs.stroke_horizontal,
                        theme.sigil_failed,
                        &what,
                    );
                    app.handle_key(press(KeyCode::Enter));

                    let mut app = App::with_game(
                        Theme::new(mode, set),
                        game_knowing(&["ember"], true),
                        content(),
                    );
                    trace(&mut app, "752");
                    app.handle_key(press(KeyCode::Enter));
                    let unstable = screen(&app);
                    assert_cell(
                        &unstable,
                        node_cell(2),
                        glyphs.sigil_fault,
                        theme.sigil_unstable,
                        &what,
                    );
                    assert_cell(
                        &unstable,
                        node_cell(5),
                        glyphs.sigil_linked,
                        theme.sigil_unstable,
                        &what,
                    );
                    assert_cell(
                        &unstable,
                        node_cell(9),
                        glyphs.sigil_node,
                        theme.sigil_node,
                        &what,
                    );
                }
            }
        }

        #[test]
        fn the_overlay_is_plain_ascii_with_ascii_glyphs() {
            let ascii = || Theme::new(ColorMode::TrueColorDark, GlyphSet::Ascii);
            let mut app =
                App::with_game(ascii(), game_knowing(&["ember", "recall"], true), content());

            trace(&mut app, "7859");
            let drawing = text(&app);
            app.handle_key(press(KeyCode::Enter));
            let verdict = text(&app);

            for screen in [drawing, verdict] {
                assert!(screen.is_ascii(), "{screen}");
            }
        }

        #[test]
        fn the_overlay_keys_do_nothing_on_a_too_small_screen_but_ctrl_c_quits() {
            let mut app = new_app();
            app.handle_key(press(KeyCode::Char('t')));
            render(&app, 79, 24);

            for code in [KeyCode::Char('7'), KeyCode::Enter, KeyCode::Esc] {
                app.handle_key(press(code));
            }
            assert_eq!(drawn(&app), Some(vec![]));

            app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
            assert_eq!(app.state(), State::Quit);
        }

        #[test]
        fn q_does_not_quit_while_tracing() {
            let mut app = new_app();

            trace(&mut app, "7");
            app.handle_key(press(KeyCode::Char('q')));

            assert_eq!(drawn(&app), Some(vec![7]));
        }

        #[test]
        fn the_side_panel_shows_the_essence_and_the_sigils_known() {
            let app = app_knowing(
                dark(),
                &["ember", "recall", "veil", "passage", "lance"],
                false,
            );
            let screen = screen(&app);
            let panel: Vec<String> = (1..=12)
                .map(|y| {
                    (60..=78)
                        .map(|x| screen[(x, y)].symbol())
                        .collect::<String>()
                        .trim_end()
                        .to_owned()
                })
                .collect();

            assert_eq!(
                panel,
                [
                    "HP            30/30",
                    "██████████████████",
                    "Essence         6/6",
                    "",
                    "SIGILS          5/5",
                    "Ember",
                    "Recall",
                    "Veil",
                    "Passage",
                    "Lance",
                    "",
                    "IN VIEW",
                ]
            );
            assert_eq!(screen[(76, 3)].fg, dark().sigil_stroke.fg.unwrap());
        }
    }
}
