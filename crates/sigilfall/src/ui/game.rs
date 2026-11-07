//! The main game screen: status line, map with the player, key help.
//!
//! Exposes [`render`]. The layout follows `docs/design.md` in a reduced
//! form: a status line on the first row, the map below it from the left
//! edge, and the key help on the last row. The side panel, the message log
//! and the camera that follows the player on large maps are not drawn yet;
//! the map is drawn from its top-left corner and clipped to the screen.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Position as ScreenPosition, Rect};
use ratatui::style::Style;
use ratatui::text::Line;
use sigilfall_core::game::Game;
use sigilfall_core::geometry::Position;

use super::look;

/// Key help shown on the last row of the screen.
const KEY_HELP: &str = " ←↑→↓/1-9 move   . wait   q quit";

/// Draws the game screen for `game` inside `area`.
///
/// `area` is expected to be at least 80 × 24 (checked by the caller); a
/// smaller area is still drawn without panicking, only clipped.
pub fn render(frame: &mut Frame, area: Rect, game: &Game) {
    let [status_area, map_area, help_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(area);

    frame.render_widget(status_line(game), status_area);
    render_map(frame.buffer_mut(), map_area, game);
    frame.render_widget(Line::raw(KEY_HELP), help_area);
}

/// Builds the status line: the number of turns elapsed so far.
fn status_line(game: &Game) -> Line<'static> {
    Line::raw(format!(" Turn {}", game.ticks()))
}

/// Draws every tile of the map, then the player on top, into `buffer`
/// with the map's top-left corner at the top-left corner of `area`.
///
/// Cells that fall outside `area` are skipped.
fn render_map(buffer: &mut Buffer, area: Rect, game: &Game) {
    let map = game.map();
    for y in 0..map.height() {
        for x in 0..map.width() {
            let (Ok(x), Ok(y)) = (i32::try_from(x), i32::try_from(y)) else {
                continue;
            };
            let position = Position::new(x, y);
            if let Some(tile) = map.tile(position) {
                draw_cell(buffer, area, position, look::tile(tile));
            }
        }
    }
    draw_cell(buffer, area, game.player_actor().position(), look::player());
}

/// Writes one map cell at `position` (map coordinates) with the given
/// character and style, if that cell lies inside `area`.
fn draw_cell(buffer: &mut Buffer, area: Rect, position: Position, (glyph, style): (char, Style)) {
    let Some(screen) = screen_position(area, position) else {
        return;
    };
    if let Some(cell) = buffer.cell_mut(screen) {
        cell.set_char(glyph).set_style(style);
    }
}

/// Converts a map position into a terminal position inside `area`, or
/// `None` when the position is negative or falls outside `area`.
fn screen_position(area: Rect, position: Position) -> Option<ScreenPosition> {
    let x = u16::try_from(position.x).ok()?;
    let y = u16::try_from(position.y).ok()?;
    let screen = ScreenPosition::new(area.x.checked_add(x)?, area.y.checked_add(y)?);
    area.contains(screen).then_some(screen)
}
