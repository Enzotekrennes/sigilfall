//! The main game screen: status line with the turn and the run's seed, map
//! with the player, key help.
//!
//! Exposes [`render`]. The layout follows the game screen of
//! `docs/design.md`: status line on the first row, map below it on the
//! left, side panel on the right, message log under both, key help on the
//! last row. Only the areas that have something to show are drawn for now:
//! the side panel and the log keep their place but stay empty until the
//! health bar and the log exist. The map is seen through a
//! [`Camera`] that follows the player, and each map cell is drawn according
//! to what the player knows of it ([`Sight`]): in full when in view, only
//! its landmarks when explored but out of view, not at all when never seen.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Position as ScreenPosition, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use sigilfall_core::game::Game;
use sigilfall_core::geometry::Position;
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

/// What the player knows of a map cell, which decides how it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sight {
    /// In the player's field of view right now.
    Visible,
    /// Seen before on this floor, out of view now.
    Remembered,
    /// Never seen, or outside the map.
    Unknown,
}

impl Sight {
    /// Returns what the player of `game` knows of the map cell `position`.
    fn of(game: &Game, position: Position) -> Self {
        if game.is_visible(position) {
            Self::Visible
        } else if game.is_remembered(position) {
            Self::Remembered
        } else {
            Self::Unknown
        }
    }
}

/// Returns the character and style a cell holding `tile` is drawn with,
/// given what the player knows of it, or `None` when the cell is left as
/// the background.
///
/// The rule of `docs/design.md`: a cell in view is drawn in full with the
/// "seen" tokens; an explored cell out of view shows only its landmarks
/// (walls and stairs today; arches and engraved glyphs when they exist), in
/// the `*_remembered` tokens, and never its floor; a cell never seen shows
/// nothing. The three states therefore differ by glyph as well as by colour:
/// out of view, the floor around the landmarks disappears, which tells them
/// apart without colour or Unicode.
const fn cell_look(tile: Tile, sight: Sight, theme: &Theme) -> Option<(char, Style)> {
    let glyphs = &theme.glyphs;
    match (sight, tile) {
        (Sight::Unknown, _) | (Sight::Remembered, Tile::Floor) => None,
        (Sight::Visible, Tile::Floor) => Some((glyphs.floor, theme.floor)),
        (Sight::Visible, Tile::Wall) => Some((glyphs.wall, theme.wall_seen)),
        (Sight::Remembered, Tile::Wall) => Some((glyphs.wall, theme.wall_remembered)),
        (Sight::Visible, Tile::StairsDown) => Some((glyphs.stairs, theme.stairs)),
        (Sight::Remembered, Tile::StairsDown) => Some((glyphs.stairs, theme.stairs_remembered)),
    }
}

/// Draws the part of the map the camera shows in `area`, then the player
/// on top, into `buffer`.
///
/// Each cell is drawn by [`cell_look`] from what the player knows of it.
/// The camera follows the player (see [`Camera::new`]) and the player's own
/// cell is always in view, so the player is always drawn. Cells of `area`
/// that show nothing (never seen, remembered floor, or outside a map smaller
/// than the area) keep the background painted beforehand.
fn render_map(buffer: &mut Buffer, area: Rect, game: &Game, theme: &Theme) {
    let map = game.map();
    let player = game.player_actor().position();
    let camera = Camera::new(area, map, player);
    for (screen, position) in camera.cells() {
        let look = map
            .tile(position)
            .and_then(|tile| cell_look(tile, Sight::of(game, position), theme));
        if let Some(look) = look {
            draw_cell(buffer, screen, look);
        }
    }
    if let Some(screen) = camera.screen_position(player) {
        draw_cell(buffer, screen, (theme.glyphs.player, theme.player));
    }
}

/// Writes `glyph` with `style` at the terminal cell `screen`, if that cell
/// exists in `buffer`.
///
/// The glyph replaces whatever was drawn there, foreground and emphasis
/// included: the player drawn over a dim floor must not stay dim in
/// monochrome. Only the background painted beforehand is kept.
fn draw_cell(buffer: &mut Buffer, screen: ScreenPosition, (glyph, style): (char, Style)) {
    if let Some(cell) = buffer.cell_mut(screen) {
        cell.fg = Color::Reset;
        cell.modifier = Modifier::empty();
        cell.set_char(glyph).set_style(style);
    }
}

#[cfg(test)]
mod tests {
    //! Layout of the game screen, and the three states of a map cell
    //! (visible, remembered, unknown) checked on the styles of the rendered
    //! buffer in every rendering mode: the text snapshots of the application
    //! tests do not carry styles.

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Cell;
    use sigilfall_core::action::Action;
    use sigilfall_core::game::PLAYER_START;
    use sigilfall_core::geometry::Direction;
    use sigilfall_core::map::FIXED_ROOM_PILLAR;

    use super::*;
    use crate::theme::{ColorMode, GlyphSet};

    /// The four rendering modes of `docs/design.md`.
    const MODES: [ColorMode; 4] = [
        ColorMode::TrueColorDark,
        ColorMode::TrueColorLight,
        ColorMode::Ansi16,
        ColorMode::Monochrome,
    ];

    /// Both glyph sets, so every state is also checked in ASCII.
    const GLYPH_SETS: [GlyphSet; 2] = [GlyphSet::Unicode, GlyphSet::Ascii];

    /// Seed of the fixed-room games; the room does not depend on it.
    const FIXED_ROOM_SEED: Seed = Seed::new(42);

    /// Steps the player takes east from [`PLAYER_START`] in
    /// [`explored_fixed_room`], ending at (19, 5).
    ///
    /// From there the left wall, seen at the start, is 19 cells away, far
    /// out of sight, while the right wall, 10 cells away, has never been
    /// within the 8-cell sight radius.
    const STEPS_EAST: i32 = 14;

    /// Left wall on the player's row: seen at the start, out of view after
    /// the walk.
    const REMEMBERED_WALL: Position = Position::new(0, 5);
    /// Floor next to it: seen at the start, out of view after the walk.
    const REMEMBERED_FLOOR: Position = Position::new(1, 5);
    /// The pillar, four cells behind the player after the walk.
    const VISIBLE_WALL: Position = FIXED_ROOM_PILLAR;
    /// Floor one cell east of the player after the walk.
    const VISIBLE_FLOOR: Position = Position::new(20, 5);
    /// Right wall on the player's row: never within sight.
    const UNKNOWN_WALL: Position = Position::new(29, 5);
    /// Floor next to it, nine cells from the player: never within sight.
    const UNKNOWN_FLOOR: Position = Position::new(28, 5);

    /// A game in the fixed room after the player walked [`STEPS_EAST`]
    /// cells east, which leaves every cell state of the constants above.
    fn explored_fixed_room() -> Game {
        let mut game = Game::in_fixed_room(FIXED_ROOM_SEED);
        for _ in 0..STEPS_EAST {
            game.submit(Action::Move(Direction::East));
        }
        game
    }

    /// Renders `game` on a whole 80 × 24 screen with `theme`.
    fn draw(game: &Game, theme: &Theme) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| crate::ui::render(frame, game, theme))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    /// The screen cell of `buffer` that shows the map cell `position` of
    /// `game` on an 80 × 24 screen.
    fn cell_at<'buffer>(buffer: &'buffer Buffer, game: &Game, position: Position) -> &'buffer Cell {
        let area = map_area(Rect::new(0, 0, 80, 24));
        let camera = Camera::new(area, game.map(), game.player_actor().position());
        let screen = camera
            .screen_position(position)
            .unwrap_or_else(|| panic!("{position:?} is out of the view"));
        &buffer[screen]
    }

    /// Asserts that `cell` shows `glyph` in exactly the colour and emphasis
    /// of the token `style`. A token without a colour leaves the terminal's
    /// own colour (`Color::Reset`, as painted with the background).
    fn assert_drawn(cell: &Cell, glyph: char, style: Style, what: &str) {
        assert_eq!(cell.symbol(), glyph.to_string(), "{what}");
        assert_eq!(cell.fg, style.fg.unwrap_or(Color::Reset), "{what}");
        assert_eq!(cell.modifier, style.add_modifier, "{what}");
    }

    /// Asserts that `cell` shows nothing but the theme's background.
    fn assert_blank(cell: &Cell, theme: &Theme, what: &str) {
        assert_eq!(cell.symbol(), " ", "{what}");
        assert_eq!(Some(cell.bg), theme.bg.bg, "{what}");
        assert_eq!(cell.modifier, Modifier::empty(), "{what}");
    }

    /// Foreground colour and emphasis of `cell`: what tells two cells with
    /// the same glyph apart.
    const fn look_of(cell: &Cell) -> (Color, Modifier) {
        (cell.fg, cell.modifier)
    }

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

    #[test]
    fn the_explored_room_holds_each_state_the_tests_rely_on() {
        let game = explored_fixed_room();

        assert_eq!(
            game.player_actor().position(),
            Position::new(PLAYER_START.x + STEPS_EAST, PLAYER_START.y)
        );
        for position in [VISIBLE_WALL, VISIBLE_FLOOR] {
            assert!(game.is_visible(position), "{position:?}");
        }
        for position in [REMEMBERED_WALL, REMEMBERED_FLOOR] {
            assert!(!game.is_visible(position), "{position:?}");
            assert!(game.is_remembered(position), "{position:?}");
        }
        for position in [UNKNOWN_WALL, UNKNOWN_FLOOR] {
            assert!(!game.is_remembered(position), "{position:?}");
        }
    }

    #[test]
    fn visible_remembered_and_unknown_cells_are_drawn_apart_in_every_mode() {
        let game = explored_fixed_room();
        for mode in MODES {
            for glyph_set in GLYPH_SETS {
                let theme = Theme::new(mode, glyph_set);
                let glyphs = &theme.glyphs;
                let screen = draw(&game, &theme);
                let cell = |position| cell_at(&screen, &game, position);
                let case = |what: &str| format!("{mode:?}, {glyph_set:?}: {what}");

                assert_drawn(
                    cell(VISIBLE_FLOOR),
                    glyphs.floor,
                    theme.floor,
                    &case("visible floor"),
                );
                assert_drawn(
                    cell(VISIBLE_WALL),
                    glyphs.wall,
                    theme.wall_seen,
                    &case("visible wall"),
                );
                assert_drawn(
                    cell(REMEMBERED_WALL),
                    glyphs.wall,
                    theme.wall_remembered,
                    &case("remembered wall"),
                );
                assert_blank(cell(REMEMBERED_FLOOR), &theme, &case("remembered floor"));
                assert_blank(cell(UNKNOWN_WALL), &theme, &case("unknown wall"));
                assert_blank(cell(UNKNOWN_FLOOR), &theme, &case("unknown floor"));
                assert_ne!(
                    look_of(cell(VISIBLE_WALL)),
                    look_of(cell(REMEMBERED_WALL)),
                    "{}",
                    case("a wall in view and a wall out of view look alike")
                );
                let player = cell(game.player_actor().position());
                assert_drawn(player, glyphs.player, theme.player, &case("player"));
            }
        }
    }

    #[test]
    fn remembered_walls_take_the_values_of_the_design_in_every_mode() {
        // "wall_seen" and "wall_remembered" rows of docs/design.md.
        let expected = [
            (
                ColorMode::TrueColorDark,
                (Color::Rgb(0xB9, 0xAF, 0x9C), Modifier::empty()),
                (Color::Rgb(0x6E, 0x68, 0x78), Modifier::empty()),
            ),
            (
                ColorMode::TrueColorLight,
                (Color::Rgb(0x4A, 0x40, 0x38), Modifier::empty()),
                (Color::Rgb(0x85, 0x79, 0x6C), Modifier::empty()),
            ),
            (
                ColorMode::Ansi16,
                (Color::Gray, Modifier::empty()),
                (Color::DarkGray, Modifier::empty()),
            ),
            (
                ColorMode::Monochrome,
                (Color::Reset, Modifier::empty()),
                (Color::Reset, Modifier::DIM),
            ),
        ];
        let game = explored_fixed_room();
        for (mode, seen, remembered) in expected {
            let screen = draw(&game, &Theme::new(mode, GlyphSet::Unicode));

            let visible_wall = cell_at(&screen, &game, VISIBLE_WALL);
            assert_eq!(look_of(visible_wall), seen, "{mode:?}");
            let remembered_wall = cell_at(&screen, &game, REMEMBERED_WALL);
            assert_eq!(look_of(remembered_wall), remembered, "{mode:?}");
        }
    }

    #[test]
    fn cell_look_hides_unknown_cells_and_remembered_floor_in_every_mode() {
        let tiles = [Tile::Floor, Tile::Wall, Tile::StairsDown];
        for mode in MODES {
            for glyph_set in GLYPH_SETS {
                let theme = Theme::new(mode, glyph_set);
                for tile in tiles {
                    assert_eq!(cell_look(tile, Sight::Unknown, &theme), None, "{tile:?}");
                    let visible = cell_look(tile, Sight::Visible, &theme)
                        .unwrap_or_else(|| panic!("{tile:?} in view is drawn"));
                    let Some(remembered) = cell_look(tile, Sight::Remembered, &theme) else {
                        assert_eq!(tile, Tile::Floor, "only the floor is left out");
                        continue;
                    };
                    assert_eq!(visible.0, remembered.0, "{mode:?}: glyph of {tile:?}");
                    assert_ne!(visible.1, remembered.1, "{mode:?}: style of {tile:?}");
                }
            }
        }
    }

    #[test]
    fn remembered_stairs_stay_drawn_but_lose_their_emphasis_in_every_mode() {
        for mode in MODES {
            for glyph_set in GLYPH_SETS {
                let theme = Theme::new(mode, glyph_set);
                let stairs = theme.glyphs.stairs;

                let seen = cell_look(Tile::StairsDown, Sight::Visible, &theme);
                let remembered = cell_look(Tile::StairsDown, Sight::Remembered, &theme);

                assert_eq!(seen, Some((stairs, theme.stairs)), "{mode:?}");
                assert_eq!(
                    remembered,
                    Some((stairs, theme.stairs_remembered)),
                    "{mode:?}"
                );
                assert!(
                    theme.stairs.add_modifier.contains(Modifier::BOLD),
                    "{mode:?}"
                );
                let faded = theme.stairs_remembered.add_modifier;
                assert!(!faded.contains(Modifier::BOLD), "{mode:?}");
                assert!(
                    !faded.contains(Modifier::DIM),
                    "{mode:?}: the way down stays legible"
                );
            }
        }
    }
}
