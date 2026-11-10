//! Helpers of the tests written from the player's side: the application
//! launched as `main` launches it, on scratch folders and an injected clock,
//! keys pressed and typed, and the screen read back as rows of text, cells,
//! colours and emphasis, as the player reads it.
//!
//! These tests are written independently of the unit tests of `app`, `ui` and
//! `persist`: screens are read from rendered buffers, colours from the cells,
//! and expected seeds and key bindings are written out instead of being read
//! from the production code. The only place that depends on how `main` builds
//! the application is [`launch_with_terminal`].

use crate::app::{App, State};
use crate::cli::Cli;
use crate::clock::ManualClock;
use crate::language::Language;
use crate::persist::ScratchFolder;
use crate::theme::{ColorSupport, Defaults};
use crate::ui::options::OptionRow;
use crate::ui::title::TitleItem;
use clap::Parser as _;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Color, Modifier};
use sigilfall_core::action::Action;
use sigilfall_core::content::Content;
use sigilfall_core::game::Game;
use sigilfall_core::rng::Seed;
use std::fs;

/// The two sizes screens are compared at: the smallest the game supports
/// and a roomy one.
pub(super) const SIZES: [(u16, u16); 2] = [(80, 24), (120, 40)];

/// The game's own content.
pub(super) fn shipped() -> Content {
    Content::load().expect("the shipped content is valid")
}

/// The command line `sigilfall` followed by `arguments`.
pub(super) fn command_line(arguments: &[&str]) -> Cli {
    Cli::try_parse_from(std::iter::once("sigilfall").chain(arguments.iter().copied()))
        .expect("valid arguments")
}

/// Launches the application as `main` does, on the save file of `folder`,
/// for a terminal detected as `support`, with `arguments` on the command
/// line, reading the date from `clock`. Stops on the title screen when there
/// is one.
pub(super) fn launch_with_terminal(
    folder: &ScratchFolder,
    arguments: &[&str],
    support: ColorSupport,
    clock: &ManualClock,
) -> anyhow::Result<App> {
    App::launch_with_clock(
        support,
        Defaults::with_language(Language::English),
        &command_line(arguments),
        &shipped(),
        folder.save_file(),
        Box::new(clock.clone()),
    )
}

/// [`launch_with_terminal`] on a true-colour terminal and the default clock.
pub(super) fn launch_with_arguments(
    folder: &ScratchFolder,
    arguments: &[&str],
) -> anyhow::Result<App> {
    launch_with_terminal(
        folder,
        arguments,
        ColorSupport::TrueColor,
        &ManualClock::new(),
    )
}

/// A key press of `code`.
pub(super) fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

/// Ctrl+C.
pub(super) fn ctrl_c() -> KeyEvent {
    KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
}

/// Plays each character of `keys` as a key press.
pub(super) fn type_text(app: &mut App, keys: &str) {
    for key in keys.chars() {
        app.handle_key(press(KeyCode::Char(key)));
    }
}

/// The buffer of `app` drawn on a `width` × `height` screen.
pub(super) fn buffer(app: &App, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
    terminal.backend().buffer().clone()
}

/// The text of `app` drawn on a `width` × `height` screen, one string per
/// row, trailing blanks cut.
pub(super) fn rows(app: &App, width: u16, height: u16) -> Vec<String> {
    let buffer = buffer(app, width, height);
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect()
}

/// The whole text of the 80 × 24 screen.
pub(super) fn text(app: &App) -> String {
    rows(app, 80, 24).join("\n")
}

/// The index of the first row of `rows` that contains `needle`.
pub(super) fn row_of(rows: &[String], needle: &str) -> Option<usize> {
    rows.iter().position(|row| row.contains(needle))
}

/// The entry of the title menu that is chosen.
pub(super) fn chosen(app: &App) -> TitleItem {
    let State::Title(menu) = app.state() else {
        panic!("on the title menu, not {:?}", app.state());
    };
    menu.chosen(app.has_saved_game())
}

/// Moves the title menu onto `item`.
pub(super) fn go_to(app: &mut App, item: TitleItem) {
    for _ in 0..8 {
        if chosen(app) == item {
            return;
        }
        app.handle_key(press(KeyCode::Down));
    }
    panic!("the title menu has no {item:?}");
}

/// Opens the options screen from the title menu.
pub(super) fn enter_options(app: &mut App) {
    go_to(app, TitleItem::Options);
    app.handle_key(press(KeyCode::Enter));
    assert!(
        matches!(app.state(), State::Options(_)),
        "{:?}",
        app.state()
    );
}

/// Moves the options screen onto `row`.
pub(super) fn go_to_row(app: &mut App, row: OptionRow) {
    for _ in 0..6 {
        let State::Options(menu) = app.state() else {
            panic!("on the options screen, not {:?}", app.state());
        };
        if menu.chosen() == row {
            return;
        }
        app.handle_key(press(KeyCode::Down));
    }
    panic!("the options screen has no {row:?}");
}

/// Changes the option of `row` once, forward, from the title menu, and goes
/// back to the title.
pub(super) fn change_option(app: &mut App, row: OptionRow) {
    enter_options(app);
    go_to_row(app, row);
    app.handle_key(press(KeyCode::Enter));
    app.handle_key(press(KeyCode::Esc));
}

/// What the cells of a screen are painted with.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Paint {
    /// Some cell has a 24-bit colour.
    pub(super) rgb: bool,
    /// Some cell has a colour of the 256-colour palette.
    pub(super) indexed: bool,
    /// Some cell has one of the 16 named colours.
    pub(super) named: bool,
    /// Some cell is bold, reversed or dim.
    pub(super) emphasis: bool,
}

/// Reads how `buffer` is painted.
pub(super) fn paint(buffer: &Buffer) -> Paint {
    let mut paint = Paint::default();
    for cell in buffer.content() {
        for color in [cell.fg, cell.bg] {
            match color {
                Color::Rgb(..) => paint.rgb = true,
                Color::Indexed(_) => paint.indexed = true,
                Color::Reset => {}
                _ => paint.named = true,
            }
        }
        if cell
            .modifier
            .intersects(Modifier::BOLD | Modifier::REVERSED | Modifier::DIM)
        {
            paint.emphasis = true;
        }
    }
    paint
}

/// The brightness (0 to 255) of the background of the top-left cell, or
/// `None` when it is not a 24-bit colour.
pub(super) fn background_brightness(buffer: &Buffer) -> Option<u32> {
    match buffer[(0, 0)].bg {
        Color::Rgb(r, g, b) => {
            Some((u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114) / 1000)
        }
        _ => None,
    }
}

/// A save in `folder` holding a fresh game of `seed` played for `waits`
/// turns, written as the application writes it.
pub(super) fn save_game(folder: &ScratchFolder, seed: u64, waits: usize) {
    let mut game = Game::new(Seed::from(seed), &shipped());
    for _ in 0..waits {
        game.submit(Action::Wait);
    }
    folder
        .save_file()
        .write(&game)
        .expect("the save is written");
}

/// The names of the files in `folder`, sorted.
pub(super) fn files(folder: &ScratchFolder) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(folder.path())
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// The options file of `folder`, as text, or an empty string.
pub(super) fn options_text(folder: &ScratchFolder) -> String {
    fs::read_to_string(folder.options_file().path()).unwrap_or_default()
}
