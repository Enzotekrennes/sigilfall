//! Screens of the game, drawn with ratatui from read-only game state.
//!
//! Exposes [`render`], which picks what to draw for the current terminal
//! size, and one module per screen ([`game`] for now). Drawing never changes
//! the game and never touches the real terminal: it fills a [`Frame`], so
//! every screen can be rendered into a test backend.

pub mod game;
mod look;

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::widgets::{Paragraph, Wrap};
use sigilfall_core::game::Game;

/// Smallest terminal width, in columns, the game screen is laid out for.
///
/// The layout of `docs/design.md` targets an 80 × 24 terminal, the classic
/// default size; below it the screen would be cut off.
pub const MIN_WIDTH: u16 = 80;

/// Smallest terminal height, in rows, the game screen is laid out for (see
/// [`MIN_WIDTH`]).
pub const MIN_HEIGHT: u16 = 24;

/// Draws the screen for `game` over the whole frame.
///
/// When the frame is smaller than [`MIN_WIDTH`] × [`MIN_HEIGHT`], a message
/// asking for a larger terminal is drawn instead of a truncated game screen.
/// Any frame size is accepted, down to zero, without panicking; the check is
/// made on every frame, so resizing the terminal switches between the two.
pub fn render(frame: &mut Frame, game: &Game) {
    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        render_too_small(frame, area);
    } else {
        game::render(frame, area, game);
    }
}

/// Draws the "enlarge the terminal" message, with the current size, roughly
/// centred in `area` and wrapped to its width.
fn render_too_small(frame: &mut Frame, area: Rect) {
    let message = format!(
        "Enlarge the terminal to {MIN_WIDTH} × {MIN_HEIGHT} (current: {} × {}).",
        area.width, area.height
    );
    // Start one row above the middle so a message wrapped on two or three
    // lines stays centred.
    let top = area.height.saturating_sub(1) / 2;
    let text_area = Rect {
        y: area.y + top,
        height: area.height - top,
        ..area
    };
    let paragraph = Paragraph::new(message)
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, text_area);
}
