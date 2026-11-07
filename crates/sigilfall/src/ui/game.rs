//! The main game screen: status line with the turn and the run's seed, map
//! with the player, key help.
//!
//! Exposes [`render`]. The layout follows the game screen of
//! `docs/design.md`: status line on the first row, map below it on the
//! left, side panel on the right, message log under both, key help on the
//! last row. Only the areas that have something to show are drawn for now:
//! the side panel and the log keep their place but stay empty until the
//! health bar and the log exist. The map is seen through a
//! [`Camera`] that follows the player.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Position as ScreenPosition, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use sigilfall_core::game::Game;
use sigilfall_core::map::Tile;
use sigilfall_core::rng::Seed;

use super::camera::Camera;
use crate::theme::Theme;

/// Width of the side panel, in columns, its left border included.
///
/// `docs/design.md` gives the panel 21 columns of content plus a one-column
/// border, which leaves a 58-column map on an 80-column terminal.
const SIDE_PANEL_WIDTH: u16 = 22;

/// Height of the message log, in rows, its title separator included.
///
/// `docs/design.md` gives the log five lines under a one-row separator,
/// which leaves a 16-row map on a 24-row terminal.
const LOG_HEIGHT: u16 = 6;

/// Empty columns kept at each end of the status line, as in the mock-up of
/// `docs/design.md`, so its text never touches the terminal's edges.
const STATUS_MARGIN: u16 = 1;

/// The areas of the game screen, as laid out by [`GameLayout::new`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GameLayout {
    /// First row: turn counter on the left, seed on the right.
    status: Rect,
    /// The map, which takes every row and column the other areas leave.
    map: Rect,
    /// Last row: key help.
    help: Rect,
}

impl GameLayout {
    /// Splits `area` as on the game screen of `docs/design.md`: status
    /// line, map and side panel side by side, log, key help.
    ///
    /// Beyond 80 × 24 only the map grows; the panel and the log keep their
    /// size. Their areas are left out of the result until something is
    /// drawn in them.
    fn new(area: Rect) -> Self {
        let [status, body, _log, help] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(LOG_HEIGHT),
            Constraint::Length(1),
        ])
        .areas(area);
        let [map, _side_panel] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(SIDE_PANEL_WIDTH)])
                .areas(body);
        Self { status, map, help }
    }
}

/// Returns the map area of a game screen drawn over `screen`, so tests can
/// place a [`Camera`] exactly where [`render`] places it.
#[cfg(test)]
pub fn map_area(screen: Rect) -> Rect {
    GameLayout::new(screen).map
}

/// Draws the game screen for `game` inside `area`, with `theme`.
///
/// `area` is expected to be at least 80 × 24 (checked by the caller); a
/// smaller area is still drawn without panicking, only clipped. The
/// background is expected to be painted already.
pub fn render(frame: &mut Frame, area: Rect, game: &Game, theme: &Theme) {
    let layout = GameLayout::new(area);
    frame.render_widget(status_line(game, theme), layout.status);
    let seed_area = Rect {
        width: layout.status.width.saturating_sub(STATUS_MARGIN),
        ..layout.status
    };
    frame.render_widget(seed_label(game.seed(), theme), seed_area);
    render_map(frame.buffer_mut(), layout.map, game, theme);
    frame.render_widget(key_help(theme), layout.help);
}

/// Builds the left part of the status line: the player's current turn,
/// label in the muted style and value in the text style.
///
/// The number is the turn the player is about to play, counted from 1:
/// [`Game::player_turns`] (actions that took time) plus one, so a new game
/// shows "Turn 1" and each move or wait adds one. Game ticks are not shown:
/// they also count the time other actors and slower actions take.
fn status_line(game: &Game, theme: &Theme) -> Line<'static> {
    let current_turn = game.player_turns() + 1;
    Line::from(vec![
        Span::styled(" Turn ", theme.text_muted),
        Span::styled(current_turn.to_string(), theme.text),
    ])
}

/// Builds the seed shown at the right end of the status line, such as
/// "Seed 7F3A-91C2-0B5E-D4A8": label in the muted style, seed in the title
/// style, as `docs/design.md` asks, right-aligned in the area it is drawn in.
///
/// The seed is written in its shareable form (see `Seed`'s `Display`), which
/// `--seed` accepts back to replay the run.
fn seed_label(seed: Seed, theme: &Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled("Seed ", theme.text_muted),
        Span::styled(seed.to_string(), theme.text_title),
    ])
    .right_aligned()
}

/// Builds the key help: each key in the text style, followed by what it
/// does in the muted style, so keys stand out from their labels.
fn key_help(theme: &Theme) -> Line<'static> {
    let movement = format!("{}/1-9", theme.glyphs.arrows());
    let bindings = [
        (movement, "move"),
        (".".to_owned(), "wait"),
        ("q".to_owned(), "quit"),
    ];
    let mut spans = vec![Span::raw(" ")];
    for (index, (key, label)) in bindings.into_iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("   "));
        }
        spans.push(Span::styled(key, theme.text));
        spans.push(Span::styled(format!(" {label}"), theme.text_muted));
    }
    Line::from(spans)
}

/// Returns the character and style `tile` is drawn with.
const fn tile_look(tile: Tile, theme: &Theme) -> (char, Style) {
    match tile {
        Tile::Floor => (theme.glyphs.floor, theme.floor),
        Tile::Wall => (theme.glyphs.wall, theme.wall_seen),
        Tile::StairsDown => (theme.glyphs.stairs, theme.stairs),
    }
}

/// Draws the part of the map the camera shows in `area`, then the player
/// on top, into `buffer`.
///
/// The camera follows the player (see [`Camera::new`]), so the player is
/// always drawn. Cells of `area` that show no map cell, around a map smaller
/// than the area, keep the background painted beforehand.
fn render_map(buffer: &mut Buffer, area: Rect, game: &Game, theme: &Theme) {
    let map = game.map();
    let player = game.player_actor().position();
    let camera = Camera::new(area, map, player);
    for (screen, position) in camera.cells() {
        if let Some(tile) = map.tile(position) {
            draw_cell(buffer, screen, tile_look(tile, theme));
        }
    }
    if let Some(screen) = camera.screen_position(player) {
        draw_cell(buffer, screen, (theme.glyphs.player, theme.player));
    }
}

/// Writes `glyph` with `style` at the terminal cell `screen`, if that cell
/// exists in `buffer`.
///
/// The style is patched over the cell's current one, so the background
/// painted beforehand is kept.
fn draw_cell(buffer: &mut Buffer, screen: ScreenPosition, (glyph, style): (char, Style)) {
    if let Some(cell) = buffer.cell_mut(screen) {
        cell.set_char(glyph).set_style(style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_80_by_24_the_map_is_58_by_16_as_in_the_design() {
        let layout = GameLayout::new(Rect::new(0, 0, 80, 24));

        assert_eq!(layout.status, Rect::new(0, 0, 80, 1));
        assert_eq!(layout.map, Rect::new(0, 1, 58, 16));
        assert_eq!(layout.help, Rect::new(0, 23, 80, 1));
    }

    #[test]
    fn a_larger_terminal_only_grows_the_map() {
        let layout = GameLayout::new(Rect::new(0, 0, 120, 40));

        assert_eq!(layout.map, Rect::new(0, 1, 120 - SIDE_PANEL_WIDTH, 40 - 8));
        assert_eq!(layout.help, Rect::new(0, 39, 120, 1));
    }
}
