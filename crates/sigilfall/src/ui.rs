//! Screens of the game, drawn with ratatui from read-only game state.
//!
//! Exposes [`render`], which picks what to draw for the current terminal
//! size and the state of the game, and [`render_menu`], its counterpart for
//! the screens before a game ([`MenuScreen`]); the [`Overlay`] drawn over
//! the map; one module per screen ([`title`], the title screen with its
//! seed entry and confirmation, [`options`], [`game`] while playing,
//! [`death`], the death summary, once the player has died, and
//! [`victory`], once the player has broken the seal); the parts of
//! the game screen ([`hud`], the side panel, [`log`], the message log,
//! [`sigil`], the overlay where sigils are traced, with its [`grimoire`],
//! and [`inventory`], the overlay of what the player carries); and the
//! [`camera`] that decides which part of the map is seen. Drawing never
//! changes the game and never touches the real terminal: it fills a
//! [`Frame`], so every screen can be rendered into a test backend. Every
//! colour and glyph comes from the [`Theme`] passed in, and every word from
//! `messages`.

pub mod camera;
pub mod death;
pub mod game;
pub mod grimoire;
pub mod hud;
pub mod inventory;
pub mod log;
pub mod options;
pub mod sigil;
pub mod title;
pub mod victory;

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect, Size};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget as _, Wrap};
use sigilfall_core::content::Content;
use sigilfall_core::game::{Game, Status};
use sigilfall_core::rng::DailyDate;

use self::inventory::InventoryScreen;
use self::options::OptionsMenu;
use self::title::TitleStep;
use crate::casting::Casting;
use crate::message_log::MessageLog;
use crate::messages;
use crate::theme::{Settings, Theme};

/// A screen shown before a game, drawn by [`render_menu`].
#[derive(Debug, Clone, Copy)]
pub enum MenuScreen<'state> {
    /// The title screen at this step, with the saved game described for
    /// "Continue" (`None` when no game is saved) and the notices to show
    /// under the menu.
    Title {
        /// The menu, the seed entry or the confirmation.
        step: TitleStep<'state>,
        /// The saved game, as `messages::saved_run` describes it.
        saved: Option<&'state str>,
        /// Today's UTC date, written beside "Daily".
        today: DailyDate,
        /// Problems met at launch, such as an unreadable options file.
        notices: &'state [String],
    },
    /// The options screen, in this state, for these settings, with the
    /// notice of a failed write if any.
    Options {
        /// Which entry is chosen.
        menu: &'state OptionsMenu,
        /// The options and what decides the theme with them.
        settings: &'state Settings,
        /// Why the options could not be written, if they could not.
        notice: Option<&'state str>,
    },
}

/// What covers the map of the game screen, if anything: the side panel
/// and the log stay in view under both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay<'state> {
    /// The sigil overlay, at this step of the cast.
    Casting(&'state Casting),
    /// The inventory, in this state.
    Inventory(&'state InventoryScreen),
}

/// Smallest terminal width, in columns, the game screen is laid out for.
///
/// The layout targets an 80 × 24 terminal, the classic default size; below
/// it the screen would be cut off.
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

/// Draws the screen for `game` over the whole frame, with `theme`: the game
/// screen with the messages of `log` while the game is played, with
/// `overlay` over its map while the player casts or looks at the
/// inventory, and once the game is over the end screen its status calls
/// for: the death summary after a death ([`Status::GameOver`]), the victory
/// screen after the seal was broken ([`Status::Won`]). Monster and sigil
/// names come from `content`.
///
/// The theme's background is painted on every cell first. When the frame
/// is smaller than [`MIN_WIDTH`] × [`MIN_HEIGHT`], a message asking for a
/// larger terminal is drawn instead of either screen.
/// Any frame size is accepted, down to zero, without panicking; the check is
/// made on every frame, so resizing the terminal switches between the two.
pub fn render(
    frame: &mut Frame,
    game: &Game,
    log: &MessageLog,
    content: &Content,
    overlay: Option<Overlay<'_>>,
    theme: &Theme,
) {
    let area = frame.area();
    frame.buffer_mut().set_style(area, theme.bg);
    if !fits(area.as_size()) {
        render_too_small(frame, area, theme);
    } else {
        match game.status() {
            Status::Playing => game::render(frame, area, game, log, content, overlay, theme),
            Status::GameOver { .. } => death::render(frame, area, game, content, theme),
            Status::Won => victory::render(frame, area, game, content, theme),
        }
    }
}

/// Draws `screen`, a screen shown before a game, over the whole frame with
/// `theme`, as [`render`] draws the game: the background painted first,
/// and the "enlarge the terminal" message instead below [`MIN_WIDTH`] ×
/// [`MIN_HEIGHT`].
pub fn render_menu(frame: &mut Frame, screen: MenuScreen<'_>, theme: &Theme) {
    let area = frame.area();
    frame.buffer_mut().set_style(area, theme.bg);
    if !fits(area.as_size()) {
        render_too_small(frame, area, theme);
        return;
    }
    let buffer = frame.buffer_mut();
    match screen {
        MenuScreen::Title {
            step,
            saved,
            today,
            notices,
        } => title::render(buffer, area, step, saved, today, notices, theme),
        MenuScreen::Options {
            menu,
            settings,
            notice,
        } => options::render(buffer, area, menu, settings, notice, theme),
    }
}

/// Builds a line of key help from `bindings`, pairs of a key and what it
/// does: each key in `text`, its label in `text_muted`, three blank columns
/// between two pairs, one before the first, the same on every screen.
pub(crate) fn key_help(bindings: Vec<(String, &str)>, theme: &Theme) -> Line<'static> {
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

/// The up and down arrows as one key name, `↑↓` (`^v` in ASCII), for the
/// key help of the lists.
pub(crate) fn up_down(theme: &Theme) -> String {
    format!("{}{}", theme.glyphs.arrow_up, theme.glyphs.arrow_down)
}

/// Draws `rows` as a block centred in `area`: the block centred down the
/// rows as if it had at least `reserved` rows, so a screen whose lower part
/// changes length does not move its upper part, yet raised as needed for
/// every row to fit; each line centred across on its own width (pad the
/// lines of a list to one width to keep it aligned); and each line marked
/// `true` on `selection` over its width. Rows beyond the area are dropped;
/// nothing panics on a small area.
pub(crate) fn draw_centred_rows(
    buffer: &mut Buffer,
    area: Rect,
    rows: Vec<(Line<'static>, bool)>,
    reserved: usize,
    selection: Style,
) {
    let count = u16::try_from(rows.len()).unwrap_or(u16::MAX);
    let centred_on = count.max(u16::try_from(reserved).unwrap_or(u16::MAX));
    let centred = area.height.saturating_sub(centred_on) / 2;
    let top = area.y + centred.min(area.height.saturating_sub(count));
    for (offset, (line, chosen)) in (0..area.height).zip(rows) {
        let y = top + offset;
        if y >= area.bottom() {
            break;
        }
        let width = u16::try_from(line.width())
            .unwrap_or(u16::MAX)
            .min(area.width);
        let row = Rect {
            x: area.x + (area.width - width) / 2,
            y,
            width,
            height: 1,
        };
        if chosen {
            buffer.set_style(row, selection);
        }
        line.render(row, buffer);
    }
}

/// Draws the "enlarge the terminal" message, with the current size, roughly
/// centred in `area` and wrapped to its width, in the theme's text style.
fn render_too_small(frame: &mut Frame, area: Rect, theme: &Theme) {
    let message = messages::enlarge_terminal(
        theme.language,
        (MIN_WIDTH, MIN_HEIGHT),
        (area.width, area.height),
        theme.glyphs.dimension,
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
    use sigilfall_core::geometry::Position;
    use sigilfall_core::rng::Seed;

    use super::*;
    use crate::theme::{ColorMode, GlyphSet};
    use crate::ui::camera::Camera;

    /// Screen cell of the player at the start. The fixed room (30 × 12) is
    /// centred in the 58 × 16 map area that starts on row 1: 14 columns
    /// left of it and 2 rows above it.
    const PLAYER_CELL: (u16, u16) = (5 + 14, 5 + 1 + 2);
    /// Screen cell of the room's top-left wall, map cell (0, 0).
    const WALL_CELL: (u16, u16) = (14, 3);
    /// Screen cell of a floor tile next to that wall, map cell (1, 1).
    const FLOOR_CELL: (u16, u16) = (15, 4);
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

    /// The game's own monsters, for their names.
    fn content() -> Content {
        Content::load().expect("the shipped content is valid")
    }

    /// Renders a new game at 80 × 24 in `mode`, with Unicode glyphs, and
    /// returns the screen with the theme it was drawn with.
    fn screen(mode: ColorMode) -> (Buffer, Theme) {
        let theme = Theme::new(mode, GlyphSet::Unicode);
        let game = Game::in_fixed_room(SEED);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    &game,
                    &MessageLog::default(),
                    &content(),
                    None,
                    &theme,
                )
            })
            .unwrap();
        (terminal.backend().buffer().clone(), theme)
    }

    /// The cell at screen column `x`, row `y`.
    fn cell(buffer: &Buffer, (x, y): (u16, u16)) -> &Cell {
        &buffer[(x, y)]
    }

    #[test]
    fn the_player_cell_is_where_the_camera_puts_the_player() {
        let game = Game::in_fixed_room(SEED);
        let map_area = game::map_area(Rect::new(0, 0, 80, 24));
        let camera = Camera::new(map_area, game.map(), PLAYER_START);

        let start = camera.screen_position(PLAYER_START).unwrap();

        assert_eq!((start.x, start.y), PLAYER_CELL);
        let wall = camera.screen_position(Position::new(0, 0));
        assert_eq!(wall.map(|cell| (cell.x, cell.y)), Some(WALL_CELL));
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
        terminal
            .draw(|frame| {
                render(
                    frame,
                    &game,
                    &MessageLog::default(),
                    &content(),
                    None,
                    &theme,
                )
            })
            .unwrap();
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
