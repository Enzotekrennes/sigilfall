//! Screens of the game, drawn with ratatui from read-only game state.
//!
//! Exposes [`render`], which picks what to draw for the current terminal
//! size, and one module per screen ([`game`] for now). Drawing never changes
//! the game and never touches the real terminal: it fills a [`Frame`], so
//! every screen can be rendered into a test backend. Every colour and glyph
//! comes from the [`Theme`] passed in.

pub mod game;

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect, Size};
use ratatui::widgets::{Paragraph, Wrap};
use sigilfall_core::game::Game;

use crate::theme::Theme;

/// Smallest terminal width, in columns, the game screen is laid out for.
///
/// The layout of `docs/design.md` targets an 80 × 24 terminal, the classic
/// default size; below it the screen would be cut off.
pub const MIN_WIDTH: u16 = 80;

/// Smallest terminal height, in rows, the game screen is laid out for (see
/// [`MIN_WIDTH`]).
pub const MIN_HEIGHT: u16 = 24;

/// Returns whether a terminal of `size` is large enough for the game
/// screen, that is at least [`MIN_WIDTH`] × [`MIN_HEIGHT`].
///
/// Below that size [`render`] shows a message instead of the game, and the
/// application ignores game keys, so nothing is played blind.
pub const fn fits(size: Size) -> bool {
    size.width >= MIN_WIDTH && size.height >= MIN_HEIGHT
}

/// Draws the screen for `game` over the whole frame, with `theme`.
///
/// The theme's background is painted on every cell first. When the frame
/// is smaller than [`MIN_WIDTH`] × [`MIN_HEIGHT`], a message asking for a
/// larger terminal is drawn instead of a truncated game screen.
/// Any frame size is accepted, down to zero, without panicking; the check is
/// made on every frame, so resizing the terminal switches between the two.
pub fn render(frame: &mut Frame, game: &Game, theme: &Theme) {
    let area = frame.area();
    frame.buffer_mut().set_style(area, theme.bg);
    if fits(area.as_size()) {
        game::render(frame, area, game, theme);
    } else {
        render_too_small(frame, area, theme);
    }
}

/// Draws the "enlarge the terminal" message, with the current size, roughly
/// centred in `area` and wrapped to its width, in the theme's text style.
fn render_too_small(frame: &mut Frame, area: Rect, theme: &Theme) {
    let times = theme.glyphs.dimension;
    let message = format!(
        "Enlarge the terminal to {MIN_WIDTH} {times} {MIN_HEIGHT} (current: {} {times} {}).",
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
        .style(theme.text)
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, text_area);
}

#[cfg(test)]
mod tests {
    //! Colours of the rendered screen, checked cell by cell: the text
    //! snapshots of the application tests do not carry styles.

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::{Buffer, Cell};
    use ratatui::style::{Color, Modifier};
    use sigilfall_core::game::PLAYER_START;
    use sigilfall_core::rng::Seed;

    use super::*;
    use crate::theme::{ColorMode, GlyphSet};

    /// Screen cell of the player at the start: the map starts on row 1.
    const PLAYER_CELL: (u16, u16) = (5, 6);
    /// Screen cell of the map's top-left wall.
    const WALL_CELL: (u16, u16) = (0, 1);
    /// Screen cell of a floor tile next to that wall.
    const FLOOR_CELL: (u16, u16) = (1, 2);
    /// Seed of the test games; the fixed room does not depend on it.
    const SEED: Seed = Seed::new(42);
    /// Screen cell outside the map, in the empty side panel.
    const EMPTY_CELL: (u16, u16) = (70, 10);
    /// Screen cell of the "S" of the seed label: the seed of [`SEED`] is
    /// written `0000-0000-0000-002A`, right-aligned one column before the
    /// right edge.
    const SEED_LABEL_CELL: (u16, u16) = (55, 0);
    /// Screen cell of the first digit of the seed.
    const SEED_VALUE_CELL: (u16, u16) = (60, 0);
    /// Screen cell of the first key of the key help (the left arrow).
    const HELP_KEY_CELL: (u16, u16) = (1, 23);
    /// Screen cell of the first letter of the help label "move".
    const HELP_LABEL_CELL: (u16, u16) = (10, 23);

    /// Renders a new game at 80 × 24 in `mode`, with Unicode glyphs, and
    /// returns the screen with the theme it was drawn with.
    fn screen(mode: ColorMode) -> (Buffer, Theme) {
        let theme = Theme::new(mode, GlyphSet::Unicode);
        let game = Game::in_fixed_room(SEED);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| render(frame, &game, &theme)).unwrap();
        (terminal.backend().buffer().clone(), theme)
    }

    /// The cell at screen column `x`, row `y`.
    fn cell(buffer: &Buffer, (x, y): (u16, u16)) -> &Cell {
        &buffer[(x, y)]
    }

    #[test]
    fn the_player_cell_is_where_the_game_puts_the_player() {
        let start = (
            u16::try_from(PLAYER_START.x).unwrap(),
            u16::try_from(PLAYER_START.y).unwrap() + 1,
        );
        assert_eq!(start, PLAYER_CELL);
    }

    #[test]
    fn dark_true_colour_paints_the_tokens_of_the_design() {
        let (screen, _) = screen(ColorMode::TrueColorDark);
        let ink = Color::Rgb(0x0F, 0x0D, 0x15);

        let player = cell(&screen, PLAYER_CELL);
        assert_eq!(player.symbol(), "@");
        assert_eq!(player.fg, Color::Rgb(0xF5, 0xC4, 0x51));
        assert!(player.modifier.contains(Modifier::BOLD));
        assert_eq!(player.bg, ink, "the background survives the glyph");

        let wall = cell(&screen, WALL_CELL);
        assert_eq!(
            (wall.symbol(), wall.fg),
            ("▒", Color::Rgb(0xB9, 0xAF, 0x9C))
        );
        let floor = cell(&screen, FLOOR_CELL);
        assert_eq!(
            (floor.symbol(), floor.fg),
            ("·", Color::Rgb(0x6A, 0x63, 0x79))
        );
        assert_eq!(cell(&screen, EMPTY_CELL).bg, ink, "every cell is painted");

        assert_eq!(
            cell(&screen, HELP_KEY_CELL).fg,
            Color::Rgb(0xE6, 0xDE, 0xCB)
        );
        assert_eq!(
            cell(&screen, HELP_LABEL_CELL).fg,
            Color::Rgb(0xA4, 0x9C, 0xB2)
        );
    }

    #[test]
    fn the_seed_is_drawn_in_the_title_colour_after_a_muted_label() {
        let (screen, theme) = screen(ColorMode::TrueColorDark);

        let label = cell(&screen, SEED_LABEL_CELL);
        assert_eq!((label.symbol(), Some(label.fg)), ("S", theme.text_muted.fg));
        let value = cell(&screen, SEED_VALUE_CELL);
        assert_eq!(value.symbol(), "0");
        assert_eq!(value.fg, Color::Rgb(0xE3, 0xB2, 0x4F));
        assert!(value.modifier.contains(Modifier::BOLD));
        assert_eq!(cell(&screen, (78, 0)).symbol(), "A", "last digit");
        assert_eq!(cell(&screen, (79, 0)).symbol(), " ", "right margin");
    }

    #[test]
    fn light_true_colour_paints_the_parchment() {
        let (screen, theme) = screen(ColorMode::TrueColorLight);

        assert_eq!(cell(&screen, EMPTY_CELL).bg, Color::Rgb(0xF2, 0xEB, 0xDC));
        assert_eq!(cell(&screen, PLAYER_CELL).fg, Color::Rgb(0x7A, 0x4E, 0x00));
        assert_eq!(Some(cell(&screen, WALL_CELL).fg), theme.wall_seen.fg);
    }

    #[test]
    fn ansi16_falls_back_to_named_colours_on_black() {
        let (screen, _) = screen(ColorMode::Ansi16);

        let player = cell(&screen, PLAYER_CELL);
        assert_eq!(player.fg, Color::LightYellow);
        assert!(player.modifier.contains(Modifier::BOLD));
        assert_eq!(cell(&screen, WALL_CELL).fg, Color::Gray);
        assert_eq!(cell(&screen, FLOOR_CELL).fg, Color::DarkGray);
        assert_eq!(cell(&screen, HELP_KEY_CELL).fg, Color::White);
        assert_eq!(cell(&screen, HELP_LABEL_CELL).fg, Color::Gray);
        assert_eq!(cell(&screen, EMPTY_CELL).bg, Color::Black);
    }

    #[test]
    fn monochrome_keeps_the_terminal_colours_and_uses_emphasis() {
        let (screen, _) = screen(ColorMode::Monochrome);

        for cell in screen.content() {
            assert_eq!((cell.fg, cell.bg), (Color::Reset, Color::Reset), "{cell:?}");
        }
        assert!(cell(&screen, PLAYER_CELL).modifier.contains(Modifier::BOLD));
        assert!(cell(&screen, FLOOR_CELL).modifier.contains(Modifier::DIM));
        assert!(!cell(&screen, WALL_CELL).modifier.contains(Modifier::DIM));
        assert!(
            !cell(&screen, HELP_LABEL_CELL)
                .modifier
                .contains(Modifier::DIM)
        );
    }

    #[test]
    fn resize_message_is_drawn_in_the_text_colour_on_the_background() {
        let theme = Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode);
        let game = Game::in_fixed_room(SEED);
        let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
        terminal.draw(|frame| render(frame, &game, &theme)).unwrap();
        let screen = terminal.backend().buffer();

        let first_letter = screen
            .content()
            .iter()
            .find(|cell| cell.symbol() == "E")
            .expect("the message is drawn");
        assert_eq!(Some(first_letter.fg), theme.text.fg);
        assert_eq!(Some(first_letter.bg), theme.bg.bg);
        assert_eq!(Some(screen[(0, 0)].bg), theme.bg.bg);
    }
}
