//! The main game screen: status line with the depth, the turn and the run's
//! seed (or the date of a daily run), map
//! with the monsters in view and the player, side panel, message log, key
//! help.
//!
//! Exposes [`render`]. The layout follows the game screen of
//! `docs/design.md`: status line on the first row, map below it on the
//! left, side panel on the right (drawn by `ui::hud`), message log under
//! both (drawn by `ui::log`), key help on the last row. The map is seen
//! through a
//! [`Camera`] that follows the player, and each map cell is drawn according
//! to what the player knows of it ([`Sight`]): in full when in view, only
//! its landmarks when explored but out of view, not at all when never seen.
//! Engraved glyphs are landmarks: `§` until the player knows their sigil,
//! `≡` after, dimmer once out of view. So are the stairs down, `▼`, and on
//! the bottom floor the seal that takes their place, `◎`, whose key help
//! then says "unseal" instead of "descend". Items and monsters are drawn only on
//! cells in view, items with their family's glyph in `item`, monsters with
//! their family's letter and colour, monsters over items, and the player is
//! drawn over everything else. While the player casts or looks at the
//! inventory, the sigil overlay (`ui::sigil`) or the inventory
//! (`ui::inventory`) covers the map, and the key help names the keys of the
//! overlay instead of those of the game.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Position as ScreenPosition, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use sigilfall_core::content::Content;
use sigilfall_core::game::{Game, VisibleItem, VisibleMonster};
use sigilfall_core::generation::{self, Engraving};
use sigilfall_core::geometry::Position;
use sigilfall_core::map::Tile;

use super::camera::Camera;
use super::{Overlay, hud, inventory, log, sigil};
use crate::casting::Casting;
use crate::message_log::MessageLog;
use crate::messages;
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
    /// First row: depth and turn counter on the left, seed on the right.
    status: Rect,
    /// The map, which takes every row and column the other areas leave.
    map: Rect,
    /// Right of the map: health and what is in view, border included.
    side_panel: Rect,
    /// Under the map and the panel: separator and messages.
    log: Rect,
    /// Last row: key help.
    help: Rect,
}

impl GameLayout {
    /// Splits `area` as on the game screen of `docs/design.md`: status
    /// line, map and side panel side by side, log, key help.
    ///
    /// Beyond 80 × 24 only the map grows; the panel keeps its width and the
    /// log its height.
    fn new(area: Rect) -> Self {
        let [status, body, log, help] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(LOG_HEIGHT),
            Constraint::Length(1),
        ])
        .areas(area);
        let [map, side_panel] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(SIDE_PANEL_WIDTH)])
                .areas(body);
        Self {
            status,
            map,
            side_panel,
            log,
            help,
        }
    }
}

/// Returns the map area of a game screen drawn over `screen`, so tests can
/// place a [`Camera`] exactly where [`render`] places it.
#[cfg(test)]
pub fn map_area(screen: Rect) -> Rect {
    GameLayout::new(screen).map
}

/// Draws the game screen for `game` inside `area`, with the messages of
/// `log`, monster names from `content` and `theme`; while the player casts
/// or looks at the inventory, `overlay` covers the map and the key help is
/// the overlay's.
///
/// `area` is expected to be at least 80 × 24 (checked by the caller); a
/// smaller area is still drawn without panicking, only clipped. The
/// background is expected to be painted already.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    game: &Game,
    log: &MessageLog,
    content: &Content,
    overlay: Option<Overlay<'_>>,
    theme: &Theme,
) {
    let layout = GameLayout::new(area);
    frame.render_widget(status_line(game, theme), layout.status);
    let seed_area = Rect {
        width: layout.status.width.saturating_sub(STATUS_MARGIN),
        ..layout.status
    };
    frame.render_widget(run_label(game, theme), seed_area);
    render_map(frame.buffer_mut(), layout.map, game, theme);
    hud::render(frame.buffer_mut(), layout.side_panel, game, content, theme);
    log::render(
        frame.buffer_mut(),
        layout.log,
        Some(layout.side_panel.x),
        log,
        theme,
    );
    match overlay {
        Some(Overlay::Casting(casting)) => {
            sigil::render(frame.buffer_mut(), layout.map, casting, game, theme);
        }
        Some(Overlay::Inventory(screen)) => {
            inventory::render(frame.buffer_mut(), layout.map, screen, game, theme);
        }
        None => {}
    }
    let carries_items = game.inventory().next().is_some();
    let bottom_floor = generation::is_bottom_floor(game.depth());
    frame.render_widget(
        key_help(overlay, carries_items, bottom_floor, theme),
        layout.help,
    );
}

/// Builds the left part of the status line: the depth of the floor, then
/// the player's current turn, "Depth 2 · Turn 412", labels and separator in
/// the muted style, values in the text style. The separator is the theme's
/// (`-` in ASCII).
///
/// The depth is [`Game::depth`], 1 on the first floor. The turn is the one
/// the player is about to play, counted from 1: [`Game::player_turns`]
/// (actions that took time) plus one, so a new game shows "Turn 1" and each
/// move, wait or descent adds one; it runs on across floors. Game ticks are
/// not shown: they also count the time other actors and slower actions
/// take.
fn status_line(game: &Game, theme: &Theme) -> Line<'static> {
    let current_turn = game.player_turns() + 1;
    Line::from(vec![
        Span::styled(format!(" {} ", messages::DEPTH_LABEL), theme.text_muted),
        Span::styled(game.depth().to_string(), theme.text),
        Span::styled(
            format!(" {} {} ", theme.glyphs.separator, messages::TURN_LABEL),
            theme.text_muted,
        ),
        Span::styled(current_turn.to_string(), theme.text),
    ])
}

/// Builds what names the run at the right end of the status line: the
/// seed, such as "Seed 7F3A-91C2-0B5E-D4A8", or for a daily run its date,
/// such as "Daily 2026-11-08". Label in the muted style, value in the title
/// style, as `docs/design.md` asks, right-aligned in the area it is drawn
/// in.
///
/// The seed is written in its shareable form (see `Seed`'s `Display`), which
/// `--seed` accepts back to replay the run. A daily run shows its date
/// instead, which is what players compare; its seed stays on the death
/// summary.
fn run_label(game: &Game, theme: &Theme) -> Line<'static> {
    let (label, value) = match game.daily_date() {
        Some(date) => (messages::STATUS_DAILY_LABEL, date.to_string()),
        None => (messages::STATUS_SEED_LABEL, game.seed().to_string()),
    };
    Line::from(vec![
        Span::styled(format!("{label} "), theme.text_muted),
        Span::styled(value, theme.text_title),
    ])
    .right_aligned()
}

/// Builds the key help: each key in the text style, followed by what it
/// does in the muted style, so keys stand out from their labels.
///
/// The keys are those of the screen in front of the player: the game's
/// (move, wait, trace, take, pack, descend, save & quit), which fit the
/// 80 columns with one to spare, so the explore key (`input::EXPLORE_KEY`)
/// is left to the README; on the bottom floor (`bottom_floor` true), where
/// the same key breaks the seal instead, "descend" becomes "unseal",
/// which fits as well; under the sigil overlay,
/// the overlay's (link a node, undo, cast, cancel), the direction prompt's
/// (aim, back) or the verdict's (continue); in the inventory, its own
/// (choose, use, drop, close), or only close while the player carries
/// nothing (`carries_items` false).
fn key_help(
    overlay: Option<Overlay<'_>>,
    carries_items: bool,
    bottom_floor: bool,
    theme: &Theme,
) -> Line<'static> {
    let glyphs = &theme.glyphs;
    let directions = format!("{}/1-9", glyphs.arrows());
    let descend_label = if bottom_floor {
        messages::UNSEAL_LABEL
    } else {
        messages::DESCEND_LABEL
    };
    let bindings: Vec<(String, &str)> = match overlay {
        None => vec![
            (directions, messages::MOVE_LABEL),
            (messages::WAIT_KEY.to_owned(), messages::WAIT_LABEL),
            (messages::TRACE_KEY.to_owned(), messages::TRACE_KEY_LABEL),
            (messages::PICK_UP_KEY.to_owned(), messages::PICK_UP_LABEL),
            (
                messages::INVENTORY_KEY.to_owned(),
                messages::INVENTORY_LABEL,
            ),
            (messages::DESCEND_KEY.to_owned(), descend_label),
            (messages::QUIT_KEY.to_owned(), messages::SAVE_AND_QUIT_LABEL),
        ],
        Some(Overlay::Inventory(_)) if carries_items => vec![
            (
                format!(
                    "{}{}/{}",
                    glyphs.arrow_up,
                    glyphs.arrow_down,
                    messages::SLOT_KEYS
                ),
                messages::CHOOSE_LABEL,
            ),
            (messages::USE_KEYS.to_owned(), messages::USE_LABEL),
            (messages::DROP_KEY.to_owned(), messages::DROP_LABEL),
            (messages::CANCEL_KEY.to_owned(), messages::CLOSE_LABEL),
        ],
        Some(Overlay::Inventory(_)) => {
            vec![(messages::CANCEL_KEY.to_owned(), messages::CLOSE_LABEL)]
        }
        Some(Overlay::Casting(casting)) => casting_bindings(casting, directions),
    };
    super::key_help(bindings, theme)
}

/// The keys of the sigil overlay at the step `casting`, with their labels,
/// for [`key_help`]; `directions` names the movement keys.
fn casting_bindings(casting: &Casting, directions: String) -> Vec<(String, &'static str)> {
    match casting {
        Casting::Drawing(_) => vec![
            (messages::LINK_KEYS.to_owned(), messages::LINK_LABEL),
            (messages::UNDO_KEY.to_owned(), messages::UNDO_LABEL),
            (messages::CAST_KEY.to_owned(), messages::CAST_LABEL),
            (messages::CANCEL_KEY.to_owned(), messages::CANCEL_LABEL),
        ],
        Casting::Aiming { .. } => vec![
            (directions, messages::AIM_LABEL),
            (messages::CANCEL_KEY.to_owned(), messages::BACK_LABEL),
        ],
        Casting::Verdict(_) => {
            vec![(messages::CAST_KEY.to_owned(), messages::CONTINUE_LABEL)]
        }
    }
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
/// (walls, stairs and the seal today; arches when they exist), in
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
        (Sight::Visible, Tile::Seal) => Some((glyphs.seal, theme.seal)),
        (Sight::Remembered, Tile::Seal) => Some((glyphs.seal, theme.seal_remembered)),
    }
}

/// Returns the character and style `monster` is drawn with: its family's
/// letter, upper case for an elite, in its family's colour (see
/// [`Glyphs::creature`](crate::theme::Glyphs::creature) and
/// [`Theme::creature`]).
///
/// The letter alone names the family and the case alone tells the elite
/// apart, so the monster stays legible without colour, in every mode and in
/// the ASCII fallback, whose letters are the same.
const fn monster_look(monster: &VisibleMonster<'_>, theme: &Theme) -> (char, Style) {
    let glyph = theme.glyphs.creature(monster.family, monster.elite);
    (glyph, theme.creature(monster.family))
}

/// Returns the character and style `item` is drawn with: its family's
/// glyph (see [`Glyphs::item`](crate::theme::Glyphs::item)) in `item`.
///
/// The glyph alone names the family, in both glyph sets, so the item stays
/// legible without colour.
const fn item_look(item: &VisibleItem<'_>, theme: &Theme) -> (char, Style) {
    (theme.glyphs.item(item.family), theme.item)
}

/// Returns the character and style an engraved glyph is drawn with, given
/// whether the player of `game` knows its sigil and what the player knows
/// of its cell, or `None` on a cell never seen.
///
/// The rule of T12 for landmarks: a glyph in view is drawn in full (`§` in
/// `engraving_unread`, `≡` in `engraving_read`); an explored glyph out of
/// view keeps its glyph in the dimmer `*_remembered` token, and its floor
/// disappears around it. `§` and `≡` tell read from unread without colour,
/// in both glyph sets (`&` and `=` in ASCII).
fn engraving_look(
    engraving: &Engraving,
    game: &Game,
    sight: Sight,
    theme: &Theme,
) -> Option<(char, Style)> {
    let glyphs = &theme.glyphs;
    let read = game.grimoire().knows(engraving.sigil().id());
    match (sight, read) {
        (Sight::Unknown, _) => None,
        (Sight::Visible, false) => Some((glyphs.engraving_unread, theme.engraving_unread)),
        (Sight::Visible, true) => Some((glyphs.engraving_read, theme.engraving_read)),
        (Sight::Remembered, false) => {
            Some((glyphs.engraving_unread, theme.engraving_unread_remembered))
        }
        (Sight::Remembered, true) => Some((glyphs.engraving_read, theme.engraving_read_remembered)),
    }
}

/// Draws the part of the map the camera shows in `area`, then the engraved
/// glyphs, then the items in view, then the monsters in view, then the
/// player on top of everything, into `buffer`.
///
/// Each cell is drawn by [`cell_look`] from what the player knows of it,
/// and each engraved glyph by [`engraving_look`]. Items come from
/// [`Game::visible_items`], which only lists those lying on a cell in view,
/// like monsters: an item out of view is never drawn ([`item_look`]).
/// Monsters come from [`Game::visible_monsters`], which only lists those on
/// a cell in view: a monster out of view is never drawn, not even on a
/// remembered cell, where only the landmarks show. Each one replaces the
/// floor of its cell ([`monster_look`]); one outside the camera's view is
/// skipped. The camera follows the player (see [`Camera::new`]) and the
/// player's own cell is always in view, so the player is always drawn. Cells
/// of `area` that show nothing (never seen, remembered floor, or outside a
/// map smaller than the area) keep the background painted beforehand.
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
    for engraving in game.engravings() {
        let position = engraving.position();
        let look = engraving_look(engraving, game, Sight::of(game, position), theme);
        if let Some((screen, look)) = camera.screen_position(position).zip(look) {
            draw_cell(buffer, screen, look);
        }
    }
    for item in game.visible_items() {
        if let Some(screen) = camera.screen_position(item.position) {
            draw_cell(buffer, screen, item_look(&item, theme));
        }
    }
    for monster in game.visible_monsters() {
        if let Some(screen) = camera.screen_position(monster.position) {
            draw_cell(buffer, screen, monster_look(&monster, theme));
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
    //! Layout of the game screen, the three states of a map cell (visible,
    //! remembered, unknown), the monsters and the items in view, checked on
    //! the styles of the rendered buffer in every rendering mode: text
    //! snapshots do not carry styles. Two snapshots show monsters on a
    //! generated floor and every creature form side by side.

    use insta::assert_snapshot;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Cell;
    use sigilfall_core::action::Action;
    use sigilfall_core::content::{Content, MonsterDef};
    use sigilfall_core::game::PLAYER_START;
    use sigilfall_core::generation::{FIRST_DEPTH, generate};
    use sigilfall_core::geometry::Direction;
    use sigilfall_core::map::FIXED_ROOM_PILLAR;
    use sigilfall_core::rng::Seed;

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

    /// Steps the player first takes north from [`PLAYER_START`] in
    /// [`explored_fixed_room`], up to the top row of the room, (5, 1).
    const STEPS_NORTH: i32 = 4;

    /// Steps the player then takes east along the top row in
    /// [`explored_fixed_room`], ending at (19, 1).
    ///
    /// From there the left wall, seen at the start, is 19 columns away, out
    /// of the 16 cells sight reaches along a row. Sight reaches only 8 rows
    /// up and down, so the bottom right corner of the room, 9 rows below the
    /// top row and more than 16 columns from the start, has never been in
    /// sight.
    const STEPS_EAST: i32 = 14;

    /// Left wall on the start row: seen at the start, out of view after the
    /// walk.
    const REMEMBERED_WALL: Position = Position::new(0, 5);
    /// Floor next to it: seen at the start, out of view after the walk.
    const REMEMBERED_FLOOR: Position = Position::new(1, 5);
    /// The pillar, below and to the left of the player after the walk.
    const VISIBLE_WALL: Position = FIXED_ROOM_PILLAR;
    /// Floor four rows below and one column east of the player after the
    /// walk.
    const VISIBLE_FLOOR: Position = Position::new(20, 5);
    /// Right wall on the bottom floor row: never within sight.
    const UNKNOWN_WALL: Position = Position::new(29, 10);
    /// Floor next to it, nine rows below the player: never within sight.
    const UNKNOWN_FLOOR: Position = Position::new(28, 10);

    /// A game in the fixed room after the player walked [`STEPS_NORTH`]
    /// cells north then [`STEPS_EAST`] cells east, which leaves every cell
    /// state of the constants above.
    fn explored_fixed_room() -> Game {
        let mut game = Game::in_fixed_room(FIXED_ROOM_SEED);
        for _ in 0..STEPS_NORTH {
            game.submit(Action::Move(Direction::North));
        }
        for _ in 0..STEPS_EAST {
            game.submit(Action::Move(Direction::East));
        }
        game
    }

    /// Renders `game` on a whole 80 × 24 screen with `theme`, an empty log
    /// and monster names from the shipped content.
    fn draw(game: &Game, theme: &Theme) -> Buffer {
        let content = Content::load().expect("the shipped content is valid");
        draw_with(game, theme, &content)
    }

    /// Renders `game` like [`draw`], with monster names from `content`.
    fn draw_with(game: &Game, theme: &Theme, content: &Content) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| {
                crate::ui::render(frame, game, &MessageLog::default(), content, None, theme)
            })
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
        assert_eq!(layout.side_panel, Rect::new(58, 1, 22, 16));
        assert_eq!(layout.log, Rect::new(0, 17, 80, 6));
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
            Position::new(PLAYER_START.x + STEPS_EAST, PLAYER_START.y - STEPS_NORTH)
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
        let tiles = [Tile::Floor, Tile::Wall, Tile::StairsDown, Tile::Seal];
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

    #[test]
    fn the_seal_has_its_own_glyph_and_fades_like_the_stairs_in_every_mode() {
        for mode in MODES {
            for glyph_set in GLYPH_SETS {
                let theme = Theme::new(mode, glyph_set);
                let seal = theme.glyphs.seal;
                assert_ne!(seal, theme.glyphs.stairs, "{glyph_set:?}");

                let seen = cell_look(Tile::Seal, Sight::Visible, &theme);
                let remembered = cell_look(Tile::Seal, Sight::Remembered, &theme);

                assert_eq!(seen, Some((seal, theme.seal)), "{mode:?}");
                assert_eq!(remembered, Some((seal, theme.seal_remembered)), "{mode:?}");
                assert!(theme.seal.add_modifier.contains(Modifier::BOLD), "{mode:?}");
                let faded = theme.seal_remembered.add_modifier;
                assert!(!faded.contains(Modifier::BOLD), "{mode:?}");
                assert!(
                    !faded.contains(Modifier::DIM),
                    "{mode:?}: the end of the run stays legible"
                );
            }
        }
    }

    #[test]
    fn the_key_help_says_unseal_on_the_bottom_floor_and_still_fits() {
        for glyph_set in GLYPH_SETS {
            let theme = Theme::new(ColorMode::TrueColorDark, glyph_set);
            let above = key_help(None, false, false, &theme).to_string();
            let bottom = key_help(None, false, true, &theme).to_string();

            assert!(above.contains("> descend"), "{above}");
            assert!(bottom.contains("> unseal"), "{bottom}");
            assert!(!bottom.contains("descend"), "{bottom}");
            assert!(bottom.chars().count() < 80, "{bottom}");
        }
    }

    /// Monster file with the common and the elite form of every family, so
    /// the tests can draw forms the shipped content does not have yet (no
    /// elite vermin, eye or spectre).
    const EVERY_FORM: &str = r#"[
        Monster(id: "vermin", name: "vermin", family: Vermin, elite: false,
            speed: 100, hp: 1, attack: 0, defense: 0, min_depth: 1),
        Monster(id: "vermin_elite", name: "elite vermin", family: Vermin, elite: true,
            speed: 100, hp: 1, attack: 0, defense: 0, min_depth: 1),
        Monster(id: "acolyte", name: "acolyte", family: Acolyte, elite: false,
            speed: 100, hp: 1, attack: 0, defense: 0, min_depth: 1),
        Monster(id: "acolyte_elite", name: "elite acolyte", family: Acolyte, elite: true,
            speed: 100, hp: 1, attack: 0, defense: 0, min_depth: 1),
        Monster(id: "homunculus", name: "homunculus", family: Homunculus, elite: false,
            speed: 100, hp: 1, attack: 0, defense: 0, min_depth: 1),
        Monster(id: "homunculus_elite", name: "elite homunculus", family: Homunculus,
            elite: true, speed: 100, hp: 1, attack: 0, defense: 0, min_depth: 1),
        Monster(id: "eye", name: "eye", family: Eye, elite: false,
            speed: 100, hp: 1, attack: 0, defense: 0, min_depth: 1),
        Monster(id: "eye_elite", name: "elite eye", family: Eye, elite: true,
            speed: 100, hp: 1, attack: 0, defense: 0, min_depth: 1),
        Monster(id: "spectre", name: "spectre", family: Spectre, elite: false,
            speed: 100, hp: 1, attack: 0, defense: 0, min_depth: 1),
        Monster(id: "spectre_elite", name: "elite spectre", family: Spectre, elite: true,
            speed: 100, hp: 1, attack: 0, defense: 0, min_depth: 1),
    ]"#;

    /// Which colour token a family is drawn with, as `docs/design.md`
    /// assigns it: written out here rather than taken from
    /// [`Theme::creature`], so a wrong mapping there fails the tests.
    #[derive(Debug, Clone, Copy)]
    enum Token {
        /// `monster_hostile`: creatures of flesh.
        Hostile,
        /// `monster_arcane`: creatures of spirit.
        Arcane,
    }

    impl Token {
        /// The style of this token in `theme`.
        const fn style(self, theme: &Theme) -> Style {
            match self {
                Self::Hostile => theme.monster_hostile,
                Self::Arcane => theme.monster_arcane,
            }
        }
    }

    /// Every form of [`EVERY_FORM`], with the letter and the token
    /// `docs/design.md` gives it ("Créatures") and the cell
    /// [`room_with_every_form`] places it on.
    ///
    /// The cells form two rows of five, two and six rows below the player
    /// of [`explored_fixed_room`] at (19, 1), east of the pillar so nothing
    /// hides them: commons on the upper row, elites under them.
    const FORMS: [(&str, char, Token, Position); 10] = [
        ("vermin", 'v', Token::Hostile, Position::new(17, 3)),
        ("acolyte", 'a', Token::Hostile, Position::new(18, 3)),
        ("homunculus", 'h', Token::Hostile, Position::new(19, 3)),
        ("eye", 'e', Token::Arcane, Position::new(20, 3)),
        ("spectre", 's', Token::Arcane, Position::new(21, 3)),
        ("vermin_elite", 'V', Token::Hostile, Position::new(17, 7)),
        ("acolyte_elite", 'A', Token::Hostile, Position::new(18, 7)),
        (
            "homunculus_elite",
            'H',
            Token::Hostile,
            Position::new(19, 7),
        ),
        ("eye_elite", 'E', Token::Arcane, Position::new(20, 7)),
        ("spectre_elite", 'S', Token::Arcane, Position::new(21, 7)),
    ];

    /// The parsed [`EVERY_FORM`] file.
    fn every_form() -> Content {
        Content::parse(EVERY_FORM).expect("the test monster file is valid")
    }

    /// The definition `id` of `content`.
    fn definition<'content>(content: &'content Content, id: &str) -> &'content MonsterDef {
        content
            .monster(id)
            .unwrap_or_else(|| panic!("no monster {id:?} in the content"))
    }

    /// [`explored_fixed_room`] with every form of [`FORMS`] standing on its
    /// cell, all in view.
    fn room_with_every_form() -> Game {
        let content = every_form();
        let mut game = explored_fixed_room();
        for (id, _, _, position) in FORMS {
            game.spawn_monster(position, definition(&content, id));
        }
        game
    }

    /// [`explored_fixed_room`] with a monster on the remembered floor and
    /// another on the floor never seen: neither is in view.
    fn room_with_hidden_monsters() -> Game {
        let content = every_form();
        let mut game = explored_fixed_room();
        game.spawn_monster(REMEMBERED_FLOOR, definition(&content, "acolyte"));
        game.spawn_monster(UNKNOWN_FLOOR, definition(&content, "spectre_elite"));
        game
    }

    #[test]
    fn every_form_stands_in_view_and_hidden_monsters_do_not() {
        let game = room_with_every_form();
        for (id, _, _, position) in FORMS {
            assert!(game.is_visible(position), "{id} at {position:?}");
        }
        assert_eq!(game.visible_monsters().count(), FORMS.len());

        let game = room_with_hidden_monsters();
        assert_eq!(game.visible_monsters().count(), 0);
    }

    #[test]
    fn monsters_in_view_take_their_family_letter_and_colour_in_every_mode() {
        let game = room_with_every_form();
        for mode in MODES {
            for glyph_set in GLYPH_SETS {
                let theme = Theme::new(mode, glyph_set);
                let screen = draw(&game, &theme);
                for (id, letter, token, position) in FORMS {
                    let cell = cell_at(&screen, &game, position);
                    let what = format!("{mode:?}, {glyph_set:?}: {id}");
                    assert_drawn(cell, letter, token.style(&theme), &what);
                }
                let player = cell_at(&screen, &game, game.player_actor().position());
                assert_drawn(player, '@', theme.player, &format!("{mode:?}: player"));
            }
        }
    }

    #[test]
    fn an_elite_differs_from_its_common_form_by_case_only() {
        let game = room_with_every_form();
        let (commons, elites) = FORMS.split_at(FORMS.len() / 2);
        for mode in MODES {
            let screen = draw(&game, &Theme::new(mode, GlyphSet::Unicode));
            for (common, elite) in commons.iter().zip(elites) {
                let common_cell = cell_at(&screen, &game, common.3);
                let elite_cell = cell_at(&screen, &game, elite.3);
                let what = format!("{mode:?}: {} and {}", common.0, elite.0);

                assert_eq!(
                    elite_cell.symbol(),
                    common_cell.symbol().to_uppercase(),
                    "{what}"
                );
                assert!(
                    common_cell.symbol().chars().all(|c| c.is_ascii_lowercase()),
                    "{what}"
                );
                assert_eq!(look_of(elite_cell), look_of(common_cell), "{what}");
            }
        }
    }

    #[test]
    fn monster_colours_take_the_values_of_the_design_in_every_mode() {
        // "monster_hostile" and "monster_arcane" rows of docs/design.md.
        let expected = [
            (
                ColorMode::TrueColorDark,
                Color::Rgb(0xEC, 0x6A, 0x50),
                Color::Rgb(0xB7, 0x94, 0xF0),
            ),
            (
                ColorMode::TrueColorLight,
                Color::Rgb(0xA8, 0x2C, 0x1A),
                Color::Rgb(0x6A, 0x3F, 0xB0),
            ),
            (ColorMode::Ansi16, Color::LightRed, Color::LightRed),
            (ColorMode::Monochrome, Color::Reset, Color::Reset),
        ];
        let game = room_with_every_form();
        for (mode, hostile, arcane) in expected {
            let screen = draw(&game, &Theme::new(mode, GlyphSet::Unicode));
            for (id, _, token, position) in FORMS {
                let colour = match token {
                    Token::Hostile => hostile,
                    Token::Arcane => arcane,
                };
                let cell = cell_at(&screen, &game, position);
                assert_eq!(look_of(cell), (colour, Modifier::empty()), "{mode:?}: {id}");
            }
        }
    }

    #[test]
    fn monsters_out_of_view_are_never_drawn_in_every_mode() {
        let game = room_with_hidden_monsters();
        for mode in MODES {
            for glyph_set in GLYPH_SETS {
                let theme = Theme::new(mode, glyph_set);
                let screen = draw(&game, &theme);
                let cell = |position| cell_at(&screen, &game, position);
                let case = |what: &str| format!("{mode:?}, {glyph_set:?}: {what}");

                assert_blank(
                    cell(REMEMBERED_FLOOR),
                    &theme,
                    &case("monster on a remembered cell"),
                );
                assert_blank(
                    cell(UNKNOWN_FLOOR),
                    &theme,
                    &case("monster on a cell never seen"),
                );
                assert_drawn(
                    cell(REMEMBERED_WALL),
                    theme.glyphs.wall,
                    theme.wall_remembered,
                    &case("landmark next to the hidden monster"),
                );
            }
        }
    }

    /// Seed of the generated floor of the snapshot: the one of the game
    /// screen mock-up in `docs/design.md`, as in the application tests.
    const GENERATED_SEED: Seed = Seed::new(0x7F3A_91C2_0B5E_D4A8);

    /// Offsets from the start cell of [`GENERATED_SEED`]'s first floor to
    /// the two monsters placed in view: floor cells of the start room (see
    /// the snapshot of the application tests).
    const RAT_OFFSET: (i32, i32) = (-4, -2);
    /// See [`RAT_OFFSET`].
    const HIEROPHANT_OFFSET: (i32, i32) = (3, -1);

    /// The first floor of [`GENERATED_SEED`] with, from the shipped
    /// content, a common ossuary rat and an elite hierophant in view near
    /// the player, and a shade on the first floor cell out of view.
    fn generated_floor_with_monsters() -> Game {
        let content = Content::load().expect("the shipped content is valid");
        let floor = generate(GENERATED_SEED, FIRST_DEPTH);
        let start = floor.start();
        let mut game = Game::on_floor(GENERATED_SEED, floor);
        let near = |(dx, dy): (i32, i32)| Position::new(start.x + dx, start.y + dy);
        game.spawn_monster(near(RAT_OFFSET), definition(&content, "ossuary_rat"));
        game.spawn_monster(near(HIEROPHANT_OFFSET), definition(&content, "hierophant"));
        let hidden = first_floor_cell_out_of_view(&game);
        game.spawn_monster(hidden, definition(&content, "shade"));
        game
    }

    /// The first walkable cell of `game`'s map, row by row, that the player
    /// does not see.
    fn first_floor_cell_out_of_view(game: &Game) -> Position {
        let map = game.map();
        let width = i32::try_from(map.width()).unwrap();
        let height = i32::try_from(map.height()).unwrap();
        (0..height)
            .flat_map(|y| (0..width).map(move |x| Position::new(x, y)))
            .find(|&position| map.is_walkable(position) && !game.is_visible(position))
            .expect("a generated floor is larger than the field of view")
    }

    /// The text of `buffer`, one quoted line per row, as the snapshots of
    /// the application tests show it.
    fn screen_text(buffer: &Buffer) -> String {
        (0..buffer.area.height)
            .map(|y| {
                let row: String = (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect();
                format!("\"{row}\"\n")
            })
            .collect()
    }

    #[test]
    fn a_generated_floor_shows_the_monsters_in_view_only() {
        let game = generated_floor_with_monsters();
        let shown: Vec<_> = game.visible_monsters().map(|m| m.definition).collect();
        assert_eq!(shown, ["ossuary_rat", "hierophant"]);

        let theme = Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode);
        let screen = draw(&game, &theme);
        let hidden = first_floor_cell_out_of_view(&game);
        let camera = Camera::new(
            map_area(Rect::new(0, 0, 80, 24)),
            game.map(),
            game.player_actor().position(),
        );
        if let Some(cell) = camera.screen_position(hidden) {
            assert_blank(&screen[cell], &theme, "shade out of view");
        }

        assert_snapshot!(screen_text(&screen));
    }

    /// Items of [`room_with_items`], with the glyph of their family in
    /// Unicode and in ASCII (`docs/design.md`, "Objets") and their cell: in
    /// view, in a row two cells below the player of [`explored_fixed_room`].
    const ITEMS: [(&str, char, char, Position); 3] = [
        ("healing_draught", '¡', '!', Position::new(18, 3)),
        ("veil_page", '¶', '?', Position::new(19, 3)),
        ("steadying_ink", '♦', '*', Position::new(20, 3)),
    ];

    /// Cell of the draught a monster stands on in [`room_with_items`].
    const ITEM_UNDER_MONSTER: Position = Position::new(21, 6);

    /// [`explored_fixed_room`] with the items of [`ITEMS`] in view, one more
    /// under a monster and one under the player, and one on the remembered
    /// floor and one on the floor never seen, out of view.
    fn room_with_items() -> Game {
        let content = Content::load().expect("the shipped content is valid");
        let item = |id| content.item(id).expect("a shipped item");
        let mut game = explored_fixed_room();
        for (id, _, _, position) in ITEMS {
            game.place_item(position, item(id));
        }
        for position in [
            ITEM_UNDER_MONSTER,
            game.player_actor().position(),
            REMEMBERED_FLOOR,
            UNKNOWN_FLOOR,
        ] {
            game.place_item(position, item("essence_phial"));
        }
        let monsters = every_form();
        game.spawn_monster(ITEM_UNDER_MONSTER, definition(&monsters, "eye"));
        game
    }

    #[test]
    fn items_in_view_take_their_family_glyph_in_the_item_token_in_every_mode() {
        let game = room_with_items();
        for mode in MODES {
            for glyph_set in GLYPH_SETS {
                let theme = Theme::new(mode, glyph_set);
                let screen = draw(&game, &theme);
                for (id, unicode, ascii, position) in ITEMS {
                    let glyph = match glyph_set {
                        GlyphSet::Unicode => unicode,
                        GlyphSet::Ascii => ascii,
                    };
                    let what = format!("{mode:?}, {glyph_set:?}: {id}");
                    assert_drawn(cell_at(&screen, &game, position), glyph, theme.item, &what);
                }
            }
        }
    }

    #[test]
    fn item_colours_take_the_values_of_the_design_in_every_mode() {
        // "item" row of docs/design.md.
        let expected = [
            (ColorMode::TrueColorDark, Color::Rgb(0x8D, 0xBD, 0xE6)),
            (ColorMode::TrueColorLight, Color::Rgb(0x21, 0x56, 0x85)),
            (ColorMode::Ansi16, Color::LightBlue),
            (ColorMode::Monochrome, Color::Reset),
        ];
        let game = room_with_items();
        for (mode, colour) in expected {
            let screen = draw(&game, &Theme::new(mode, GlyphSet::Unicode));
            for (id, _, _, position) in ITEMS {
                let cell = cell_at(&screen, &game, position);
                assert_eq!(look_of(cell), (colour, Modifier::empty()), "{mode:?}: {id}");
            }
        }
    }

    #[test]
    fn items_out_of_view_are_never_drawn_and_creatures_stand_over_items() {
        let game = room_with_items();
        assert_eq!(
            game.visible_items().count(),
            ITEMS.len() + 2,
            "the three in a row, the one under the eye and the one underfoot"
        );
        for mode in MODES {
            for glyph_set in GLYPH_SETS {
                let theme = Theme::new(mode, glyph_set);
                let screen = draw(&game, &theme);
                let cell = |position| cell_at(&screen, &game, position);
                let case = |what: &str| format!("{mode:?}, {glyph_set:?}: {what}");

                assert_blank(cell(REMEMBERED_FLOOR), &theme, &case("remembered item"));
                assert_blank(cell(UNKNOWN_FLOOR), &theme, &case("item never seen"));
                assert_drawn(
                    cell(ITEM_UNDER_MONSTER),
                    'e',
                    theme.monster_arcane,
                    &case("the eye over its item"),
                );
                assert_drawn(
                    cell(game.player_actor().position()),
                    '@',
                    theme.player,
                    &case("the player over the item underfoot"),
                );
            }
        }
    }

    #[test]
    fn every_form_in_view_in_ascii() {
        let game = room_with_every_form();
        let theme = Theme::new(ColorMode::Monochrome, GlyphSet::Ascii);
        let screen = draw_with(&game, &theme, &every_form());

        assert_snapshot!(screen_text(&screen));
    }
}
