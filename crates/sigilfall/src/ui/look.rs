//! Provisional glyphs and colours of the map.
//!
//! Exposes [`tile`] and [`player`], each returning the character and style
//! a map cell is drawn with. This is the only place where the map's look is
//! decided: the theme module built from `docs/design.md` (tokens, colour
//! modes, ASCII fallback) replaces this file without touching the screens.

use ratatui::style::{Color, Modifier, Style};
use sigilfall_core::map::Tile;

/// Returns the character and style used to draw `tile`.
pub fn tile(tile: Tile) -> (char, Style) {
    match tile {
        Tile::Floor => ('.', Style::new().fg(Color::DarkGray)),
        Tile::Wall => ('#', Style::new().fg(Color::Gray)),
    }
}

/// Returns the character and style used to draw the player.
pub fn player() -> (char, Style) {
    (
        '@',
        Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    )
}
