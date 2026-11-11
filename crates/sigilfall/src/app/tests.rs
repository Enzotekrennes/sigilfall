//! Tests of the whole application: [`App`] driven with simulated keys and
//! rendered into ratatui's `TestBackend`, without any real terminal.
//!
//! This module holds the helpers the test files below it share, and declares
//! them; each file covers one behaviour as the player meets it. The tests
//! written from the player's side, which launch the application as `main` does
//! and read back only what the player reads, share the helpers of
//! [`player_view`].

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

mod combat;
mod daily_run;
mod death_summary;
mod demo_tape;
mod descent;
mod exploration;
mod field_of_view;
mod first_steps;
mod french;
mod game_screen;
mod inventory_screen;
mod key_help;
mod keyboard_layouts;
mod options_screen;
mod overlay_gutter;
mod player_view;
mod readme;
mod release_files;
mod save_files;
mod sigils;
mod title_screen;
mod victory_screen;

/// Seed of every test game: its written form, `7F3A-91C2-0B5E-D4A8`, is the
/// one shown to players as an example.
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

/// The first line of the log of a new run in `language`: the welcome that
/// names Ember, the sigil every run starts with, and the key that traces it.
fn new_run_welcome(language: Language) -> Message {
    let content = shipped_content();
    let ember = content.sigil("ember").expect("a shipped sigil");
    messages::welcome_with_sigil(language, ember)
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
