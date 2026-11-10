//! Field of view and fog, binary half.
//!
//! They drive the application as the event loop does (key presses through
//! `App::handle_key`) and read what the player reads: the cells of the
//! rendered screen, their glyph, colour and emphasis, and the "IN VIEW" block
//! of the side panel. The core half (rules of sight) lives in
//! `sigilfall-core/tests/field_of_view.rs`; here the questions are what
//! reaches the screen:
//!
//! - visible cells are drawn in full, explored cells out of view keep only
//!   their landmarks in a dimmer token, unknown cells stay blank, in the four
//!   colour modes, in Unicode and in ASCII, at 80 × 24 and 120 × 40;
//! - a monster out of the field of view is never drawn;
//! - the health shown for each monster in "IN VIEW" is the real one and
//!   follows the blows, every blow;
//! - the lit area looks round on screen.
//!
//! The expected look of every cell is worked out here from the game's own
//! answers (`is_visible`, `is_remembered`, `tile`) and the tokens of the
//! theme, not from the drawing code.

use super::player_view::{buffer, press, rows, shipped};
use crate::app::App;
use crate::theme::{ColorMode, GlyphSet, Theme};
use crate::ui::camera::Camera;
use crate::ui::game::map_area;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use sigilfall_core::game::{Game, Status};
use sigilfall_core::generation::generate_with_monsters;
use sigilfall_core::geometry::Position;
use sigilfall_core::map::Tile;
use sigilfall_core::rng::Seed;
use sigilfall_core::save;
use std::collections::HashSet;

/// Width of the side panel, as the design gives it.
const PANEL_WIDTH: u16 = 22;

/// The four colour modes of the theme.
const MODES: [ColorMode; 4] = [
    ColorMode::TrueColorDark,
    ColorMode::TrueColorLight,
    ColorMode::Ansi16,
    ColorMode::Monochrome,
];

/// The two glyph sets.
const GLYPH_SETS: [GlyphSet; 2] = [GlyphSet::Unicode, GlyphSet::Ascii];

/// The two screen sizes of the design.
const SIZES: [(u16, u16); 2] = [(80, 24), (120, 40)];

/// Letters the creatures are drawn with, lower and upper case (elites).
const CREATURE_LETTERS: &str = "vVaAhHeEsS";

fn seed_of(n: u64) -> Seed {
    Seed::from(n.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xF3F3)
}

/// A small deterministic dice, so a failure replays identically.
struct Dice(u64);

impl Dice {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}

/// The movement keys of the README, one per direction.
const MOVE_KEYS: [char; 8] = ['h', 'j', 'k', 'l', 'y', 'u', 'b', 'n'];

/// A game on the hand-made map `rows` (`#` wall, `.` floor) with the player
/// at `player`, built through the save format.
fn custom_game(rows: &[String], player: Position) -> Game {
    let base = save::to_string(&Game::in_fixed_room(Seed::from(1))).expect("fixed room saves");
    let map_start = base.find("    map: (").expect("map field");
    let player_start = base.find("    player: (").expect("player field");
    let mut text = base[..map_start].to_owned();
    text.push_str("    map: (\n        tiles: [\n");
    for row in rows {
        text.push_str(&format!("            \"{row}\",\n"));
    }
    text.push_str("        ],\n        sight: [\n");
    for row in rows {
        text.push_str(&format!("            \"{}\",\n", " ".repeat(row.len())));
    }
    text.push_str("        ],\n    ),\n");
    text.push_str(&base[player_start..]);
    let text = text.replace(
        "position: (x: 5, y: 5)",
        &format!("position: (x: {}, y: {})", player.x, player.y),
    );
    save::from_str(&text, &shipped()).expect("hand-made map loads")
}

/// A walled room of `width` × `height` cells.
fn open_room(width: usize, height: usize) -> Vec<String> {
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| {
                    if x == 0 || y == 0 || x == width - 1 || y == height - 1 {
                        '#'
                    } else {
                        '.'
                    }
                })
                .collect()
        })
        .collect()
}

fn app_for(game: Game, mode: ColorMode, glyphs: GlyphSet) -> App {
    App::with_game(Theme::new(mode, glyphs), game, shipped())
}

/// The cell of `buffer` where the map cell `position` is drawn on a screen of
/// `size`, or `None` when the camera does not show it.
fn screen_cell<'a>(
    buffer: &'a Buffer,
    game: &Game,
    size: (u16, u16),
    position: Position,
) -> Option<&'a ratatui::buffer::Cell> {
    let area = map_area(Rect::new(0, 0, size.0, size.1));
    let camera = Camera::new(area, game.map(), game.player_actor().position());
    camera
        .screen_position(position)
        .map(|screen| &buffer[screen])
}

/// What a cell of the map must look like, from the game's own answers.
enum Look {
    /// A tile kind this file makes no claim about (the seal of the bottom
    /// floor is checked by `victory_screen.rs`).
    Skip,
    /// Blank background.
    Blank,
    /// This glyph with this token.
    Drawn(char, Style),
}

/// The look the design gives a plain map cell (no creature, item, glyph or
/// player on it): visible cells in full, explored cells out of view only when
/// they are walls or stairs, unknown cells nothing.
fn expected_look(game: &Game, theme: &Theme, position: Position) -> Look {
    let tile = game.map().tile(position);
    let glyphs = &theme.glyphs;
    if let Some(other) = tile
        && !matches!(other, Tile::Floor | Tile::Wall | Tile::StairsDown)
    {
        return Look::Skip;
    }
    match (
        game.is_visible(position),
        game.is_remembered(position),
        tile,
    ) {
        (true, _, Some(Tile::Floor)) => Look::Drawn(glyphs.floor, theme.floor),
        (true, _, Some(Tile::Wall)) => Look::Drawn(glyphs.wall, theme.wall_seen),
        (true, _, Some(Tile::StairsDown)) => Look::Drawn(glyphs.stairs, theme.stairs),
        (false, true, Some(Tile::Wall)) => Look::Drawn(glyphs.wall, theme.wall_remembered),
        (false, true, Some(Tile::StairsDown)) => {
            Look::Drawn(glyphs.stairs, theme.stairs_remembered)
        }
        _ => Look::Blank,
    }
}

/// Cells where something other than the terrain is drawn: creatures in view,
/// items in view, engraved glyphs and the player.
fn occupied(game: &Game) -> HashSet<Position> {
    let mut cells: HashSet<Position> = game.visible_monsters().map(|m| m.position).collect();
    cells.extend(game.visible_items().map(|i| i.position));
    cells.extend(game.engravings().iter().map(|e| e.position()));
    cells.insert(game.player_actor().position());
    cells
}

/// Counts of what one check of a screen saw, so a test can insist it was not
/// empty.
#[derive(Default)]
struct Seen {
    visible_floor: u32,
    visible_wall: u32,
    remembered_wall: u32,
    remembered_floor_left_blank: u32,
    unknown_blank: u32,
}

/// Checks every terrain cell of the map area of `buffer` against its
/// expected look.
fn check_terrain(
    buffer: &Buffer,
    game: &Game,
    theme: &Theme,
    size: (u16, u16),
    label: &str,
    seen: &mut Seen,
) {
    let area = map_area(Rect::new(0, 0, size.0, size.1));
    let camera = Camera::new(area, game.map(), game.player_actor().position());
    let busy = occupied(game);
    for (screen, position) in camera.cells() {
        if busy.contains(&position) {
            continue;
        }
        let cell = &buffer[screen];
        match expected_look(game, theme, position) {
            Look::Skip => {}
            Look::Drawn(glyph, style) => {
                assert_eq!(
                    cell.symbol(),
                    glyph.to_string(),
                    "{label}: glyph at {position:?}"
                );
                assert_eq!(
                    cell.fg,
                    style.fg.unwrap_or(Color::Reset),
                    "{label}: colour at {position:?}"
                );
                assert_eq!(
                    cell.modifier, style.add_modifier,
                    "{label}: emphasis at {position:?}"
                );
                if game.is_visible(position) {
                    match game.map().tile(position) {
                        Some(Tile::Floor) => seen.visible_floor += 1,
                        Some(Tile::Wall) => seen.visible_wall += 1,
                        _ => {}
                    }
                } else if game.map().tile(position) == Some(Tile::Wall) {
                    seen.remembered_wall += 1;
                }
            }
            Look::Blank => {
                assert_eq!(cell.symbol(), " ", "{label}: {position:?} must be blank");
                assert_eq!(
                    Some(cell.bg),
                    theme.bg.bg,
                    "{label}: background at {position:?}"
                );
                assert_eq!(cell.modifier, Modifier::empty(), "{label}: {position:?}");
                if game.is_remembered(position) && !game.is_visible(position) {
                    seen.remembered_floor_left_blank += 1;
                } else if !game.is_remembered(position) {
                    seen.unknown_blank += 1;
                }
            }
        }
        // Without a player on it, an in-range cell never shows a creature
        // letter.
        assert!(
            !CREATURE_LETTERS.contains(cell.symbol()) || cell.symbol().is_empty(),
            "{label}: a creature letter on a terrain cell {position:?}"
        );
    }
}

/// The (colour, emphasis) pair that tells two tokens apart on screen.
fn look_pair(style: Style) -> (Option<Color>, Modifier) {
    (style.fg, style.add_modifier)
}

// ---------------------------------------------------------------------------
// Only what is in view is drawn clearly, what was seen stays dim
// ---------------------------------------------------------------------------

/// In every mode and both glyph sets, a visible wall and an explored wall out
/// of view are drawn with different tokens, and the explored floor is not
/// drawn at all: the three states differ even without colour.
#[test]
fn the_three_states_of_a_cell_differ_in_every_mode_and_glyph_set() {
    for mode in MODES {
        for glyphs in GLYPH_SETS {
            let theme = Theme::new(mode, glyphs);
            assert_ne!(
                look_pair(theme.wall_seen),
                look_pair(theme.wall_remembered),
                "{mode:?}/{glyphs:?}: visible and remembered walls must differ"
            );
            assert_ne!(
                look_pair(theme.stairs),
                look_pair(theme.stairs_remembered),
                "{mode:?}/{glyphs:?}"
            );
            // Monochrome has no colour: emphasis must carry the difference.
            if mode == ColorMode::Monochrome {
                assert!(theme.wall_remembered.add_modifier.contains(Modifier::DIM));
                assert!(!theme.wall_seen.add_modifier.contains(Modifier::DIM));
            }
        }
    }
}

/// A walk on generated floors: after every step, on both screen sizes, in the
/// 4 colour modes and 2 glyph sets, each cell of the screen is exactly what
/// the game says it is.
#[test]
fn walking_on_real_floors_draws_visible_remembered_and_unknown_cells_correctly() {
    let content = shipped();
    let mut totals = Seen::default();
    for n in 0..4u64 {
        for mode in MODES {
            for glyphs in GLYPH_SETS {
                let theme = Theme::new(mode, glyphs);
                let mut dice = Dice(0xF3A0 + n * 31 + 1);
                let mut app =
                    App::with_game(theme.clone(), Game::new(seed_of(n), &content), shipped());
                for step in 0..45 {
                    for size in SIZES {
                        let label = format!(
                            "seed #{n} {mode:?}/{glyphs:?} {}x{} step {step}",
                            size.0, size.1
                        );
                        let screen = buffer(&app, size.0, size.1);
                        check_terrain(&screen, app.game(), &theme, size, &label, &mut totals);
                    }
                    if !matches!(app.game().status(), Status::Playing) {
                        break;
                    }
                    app.handle_key(press(KeyCode::Char(MOVE_KEYS[dice.below(8)])));
                }
            }
        }
    }
    assert!(totals.visible_floor > 5_000, "{}", totals.visible_floor);
    assert!(totals.visible_wall > 500, "{}", totals.visible_wall);
    assert!(
        totals.remembered_wall > 500,
        "walls left behind must be seen dimmed ({})",
        totals.remembered_wall
    );
    assert!(
        totals.remembered_floor_left_blank > 500,
        "{}",
        totals.remembered_floor_left_blank
    );
    assert!(totals.unknown_blank > 5_000, "{}", totals.unknown_blank);
}

/// In ASCII the whole map area holds ASCII characters only.
#[test]
fn the_ascii_map_holds_nothing_but_ascii() {
    let content = shipped();
    for n in 0..6u64 {
        let mut app = app_for(
            Game::new(seed_of(n), &content),
            ColorMode::Ansi16,
            GlyphSet::Ascii,
        );
        let mut dice = Dice(0xA5C1 + n);
        for _ in 0..30 {
            for size in SIZES {
                let screen = buffer(&app, size.0, size.1);
                let area = map_area(Rect::new(0, 0, size.0, size.1));
                for y in area.top()..area.bottom() {
                    for x in area.left()..area.right() {
                        assert!(
                            screen[(x, y)].symbol().is_ascii(),
                            "seed #{n}: {:?} at ({x}, {y})",
                            screen[(x, y)].symbol()
                        );
                    }
                }
            }
            app.handle_key(press(KeyCode::Char(MOVE_KEYS[dice.below(8)])));
        }
    }
}

/// The player walking away from a lit room keeps its walls on screen, dim, and
/// the floor vanishes: shown on a plain two-room map, with the exact glyphs.
#[test]
fn leaving_a_room_leaves_its_walls_dim_and_its_floor_gone() {
    let mut rows = open_room(40, 9);
    // A wall splits the map into two rooms joined by a doorway at (20, 4).
    for (y, row) in rows.iter_mut().enumerate().take(8).skip(1) {
        if y != 4 {
            row.replace_range(20..=20, "#");
        }
    }
    let game = custom_game(&rows, Position::new(5, 4));
    let theme = Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode);
    let mut app = App::with_game(theme.clone(), game, shipped());
    app.handle_key(press(KeyCode::Char('.')));
    // At the start the far east wall is 34 cells away: unknown and blank.
    let east_wall = Position::new(39, 4);
    assert!(!app.game().is_remembered(east_wall));
    let first = buffer(&app, 80, 24);
    assert_eq!(
        screen_cell(&first, app.game(), (80, 24), east_wall)
            .expect("on screen")
            .symbol(),
        " "
    );
    // Walk through the doorway and well into the second room.
    for _ in 0..25 {
        app.handle_key(press(KeyCode::Char('l')));
    }
    let game = app.game();
    assert!(game.player_actor().position().x >= 29);
    let screen = buffer(&app, 80, 24);
    let cell = |x: i32, y: i32| {
        screen_cell(&screen, game, (80, 24), Position::new(x, y)).expect("on screen")
    };
    // The first room: its far wall (x = 0) was seen from the start and is now
    // out of view (more than 16 cells away), drawn dim; its floor is not drawn.
    assert!(game.is_remembered(Position::new(0, 4)) && !game.is_visible(Position::new(0, 4)));
    assert_eq!(cell(0, 4).symbol(), "\u{2592}");
    assert_eq!(cell(0, 4).fg, theme.wall_remembered.fg.unwrap());
    assert!(game.is_remembered(Position::new(8, 4)) && !game.is_visible(Position::new(8, 4)));
    assert_eq!(cell(8, 4).symbol(), " ", "remembered floor is not drawn");
    // The top wall of the first room, left behind, is dim too.
    assert!(game.is_remembered(Position::new(3, 0)) && !game.is_visible(Position::new(3, 0)));
    assert_eq!(cell(3, 0).fg, theme.wall_remembered.fg.unwrap());
    // Close to the player: visible floor, in full.
    let me = game.player_actor().position();
    assert_eq!(cell(me.x + 3, me.y).symbol(), "\u{b7}");
    assert_eq!(cell(me.x + 3, me.y).fg, theme.floor.fg.unwrap());
    // And the east wall, now in view, is drawn in full.
    assert!(game.is_visible(east_wall));
    assert_eq!(cell(39, 4).fg, theme.wall_seen.fg.unwrap());
}

// ---------------------------------------------------------------------------
// A monster out of view does not appear
// ---------------------------------------------------------------------------

/// On a real floor with its real monsters, every creature letter on the map
/// area belongs to a monster in view, every monster in view is drawn with its
/// letter, and a monster out of view is never drawn, in 4 modes and 2 glyph
/// sets, at both sizes.
#[test]
fn only_monsters_in_view_are_drawn_in_every_mode_and_glyph_set() {
    let content = shipped();
    let (mut drawn_total, mut hidden_total) = (0u32, 0u32);
    for n in 0..30u64 {
        let mode = MODES[(n % 4) as usize];
        let glyphs = GLYPH_SETS[((n / 4) % 2) as usize];
        let theme = Theme::new(mode, glyphs);
        let mut app = App::with_game(
            theme.clone(),
            Game::new(seed_of(200 + n), &content),
            shipped(),
        );
        let mut dice = Dice(0xBEE5 + n);
        for step in 0..120 {
            for size in SIZES {
                let screen = buffer(&app, size.0, size.1);
                let game = app.game();
                let area = map_area(Rect::new(0, 0, size.0, size.1));
                // Every creature letter on the map belongs to a monster in
                // view.
                let camera = Camera::new(area, game.map(), game.player_actor().position());
                let in_view: Vec<(Position, char)> = game
                    .visible_monsters()
                    .map(|m| (m.position, theme.glyphs.creature(m.family, m.elite)))
                    .collect();
                let mut letters = 0;
                for (screen_position, position) in camera.cells() {
                    let symbol = screen[screen_position].symbol();
                    if symbol.chars().count() == 1 && CREATURE_LETTERS.contains(symbol) {
                        letters += 1;
                        assert!(
                            in_view
                                .iter()
                                .any(|(p, g)| *p == position && g.to_string() == symbol),
                            "seed #{n} step {step}: letter {symbol} at {position:?} is not a monster in view"
                        );
                    }
                }
                let on_screen = in_view
                    .iter()
                    .filter(|(p, _)| camera.screen_position(*p).is_some())
                    .count();
                assert_eq!(
                    letters, on_screen,
                    "seed #{n} step {step}: letters on screen vs monsters in view"
                );
                drawn_total += on_screen as u32;
                // A monster out of view: its cell shows no letter.
                for (_, actor) in game.actors() {
                    if actor.as_monster().is_none() || game.is_visible(actor.position()) {
                        continue;
                    }
                    if let Some(cell) = screen_cell(&screen, game, size, actor.position()) {
                        hidden_total += 1;
                        assert!(
                            !CREATURE_LETTERS.contains(cell.symbol()) || cell.symbol() == " ",
                            "seed #{n} step {step}: a hidden monster at {:?} is drawn as {}",
                            actor.position(),
                            cell.symbol()
                        );
                    }
                }
            }
            if !matches!(app.game().status(), Status::Playing) {
                break;
            }
            app.handle_key(press(KeyCode::Char(MOVE_KEYS[dice.below(8)])));
        }
    }
    assert!(
        drawn_total > 40,
        "monsters should have been in view now and then ({drawn_total})"
    );
    assert!(
        hidden_total > 500,
        "hidden monsters on screen cells ({hidden_total})"
    );
}

/// D16 at the screen: the first screen of a floor shows no monster, whatever
/// the seed or the floor's depth, in both sizes.
#[test]
fn no_monster_letter_is_on_the_first_screen_of_a_floor() {
    let content = shipped();
    for n in 0..150u64 {
        for depth in [1u32, 4, 7, 10] {
            let floor = generate_with_monsters(seed_of(500 + n), depth, &content);
            let game = Game::on_floor(seed_of(500 + n), floor);
            let app = app_for(game, ColorMode::TrueColorDark, GlyphSet::Unicode);
            for size in SIZES {
                let screen = buffer(&app, size.0, size.1);
                let area = map_area(Rect::new(0, 0, size.0, size.1));
                for y in area.top()..area.bottom() {
                    for x in area.left()..area.right() {
                        let symbol = screen[(x, y)].symbol();
                        assert!(
                            !(symbol.chars().count() == 1 && CREATURE_LETTERS.contains(symbol)),
                            "seed #{n} depth {depth}: {symbol} at ({x}, {y}) on arrival"
                        );
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Monster health in IN VIEW follows every blow
// ---------------------------------------------------------------------------

/// The text of the "IN VIEW" block of an `width` × `height` screen, one entry
/// per line, panel margin removed. The second row of an entry that takes two
/// (a long name, then its count and health right-aligned) starts with a
/// blank: it is joined to the first, one blank between them.
fn in_view_lines(app: &App, width: u16, height: u16) -> Vec<String> {
    let rows = rows(app, width, height);
    let start = rows
        .iter()
        .position(|row| row.contains("IN VIEW"))
        .expect("the panel has an IN VIEW title");
    let skip = usize::from(width - PANEL_WIDTH) + 2;
    let mut lines: Vec<String> = Vec::new();
    for row in rows[start + 1..]
        .iter()
        .take_while(|row| !row.contains("Log"))
    {
        let line = row.chars().skip(skip).collect::<String>();
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        match lines.last_mut() {
            Some(entry) if line.starts_with(' ') => {
                entry.push(' ');
                entry.push_str(line.trim_start());
            }
            _ => lines.push(line.to_owned()),
        }
    }
    lines
}

/// Health a monster line must show: the most wounded of the monsters of that
/// definition in view, as "hp/max".
fn expected_health(game: &Game, definition: &str) -> Option<String> {
    let mut best: Option<(i32, i32)> = None;
    for monster in game
        .visible_monsters()
        .filter(|m| m.definition == definition)
    {
        let actor = game.actor(monster.id).expect("listed monster exists");
        let (hp, max) = (actor.hp(), actor.max_hp());
        // Smaller share of the maximum is more wounded; a tie keeps the first.
        let better = best
            .is_none_or(|(bh, bm)| i64::from(hp) * i64::from(bm) < i64::from(bh) * i64::from(max));
        if better {
            best = Some((hp, max));
        }
    }
    best.map(|(hp, max)| format!("{hp}/{max}"))
}

/// The monsters in view show their real health in IN VIEW and it follows the
/// blows, down to death, at both sizes and in ASCII; a monster out of view is
/// not counted.
#[test]
fn in_view_shows_the_real_health_of_monsters_and_follows_the_blows() {
    let content = shipped();
    let def = content.monster("ossuary_rat").expect("shipped rat").clone();
    for glyphs in GLYPH_SETS {
        for size in SIZES {
            let theme = Theme::new(ColorMode::TrueColorDark, glyphs);
            let letter = theme.glyphs.creature(def.family(), false);
            let mut game = Game::in_fixed_room(seed_of(9));
            // The farther one is first in the list of monsters, the one the
            // player strikes first is second: the figures shown must be the
            // wounded one's, not the first one's.
            game.spawn_monster(Position::new(11, 9), &def);
            game.spawn_monster(Position::new(8, 5), &def);
            // 23 columns away: out of the range of 16, same kind as the others.
            let far = game.spawn_monster(Position::new(28, 10), &def);
            let mut app = App::with_game(theme.clone(), game, shipped());
            app.handle_key(press(KeyCode::Char('.')));
            assert!(
                !app.game()
                    .is_visible(app.game().actor(far).unwrap().position())
            );
            let (mut blows, mut saw_wounded, mut saw_group) = (0, false, false);
            for round in 0..80 {
                let game = app.game();
                let lines = in_view_lines(&app, size.0, size.1);
                let prefix = format!("{letter} ");
                let monster_lines: Vec<&String> =
                    lines.iter().filter(|l| l.starts_with(&prefix)).collect();
                let in_view = game
                    .visible_monsters()
                    .filter(|m| m.definition == def.id())
                    .count();
                match expected_health(game, def.id()) {
                    Some(health) => {
                        assert_eq!(
                            monster_lines.len(),
                            1,
                            "{glyphs:?} {size:?} round {round}: one line per kind: {lines:?}"
                        );
                        assert!(
                            monster_lines[0].ends_with(&health),
                            "{glyphs:?} {size:?} round {round}: line {:?} must end with the real health {health}",
                            monster_lines[0]
                        );
                        let count = format!("{}{in_view}", theme.glyphs.dimension);
                        assert_eq!(
                            monster_lines[0].contains(&count),
                            in_view >= 2,
                            "{glyphs:?} {size:?} round {round}: group count {count} in {:?} ({in_view} in view)",
                            monster_lines[0]
                        );
                        saw_group |= in_view >= 2;
                        saw_wounded |= game
                            .visible_monsters()
                            .any(|m| game.actor(m.id).is_some_and(|a| a.hp() < a.max_hp()));
                    }
                    None => assert!(
                        monster_lines.is_empty(),
                        "round {round}: no monster in view, yet {lines:?}"
                    ),
                }
                if !matches!(game.status(), Status::Playing) {
                    break;
                }
                // Strike or approach the nearest monster in view.
                let me = game.player_actor().position();
                let Some(target) = game
                    .visible_monsters()
                    .map(|m| m.position)
                    .min_by_key(|p| (p.x - me.x).abs().max((p.y - me.y).abs()))
                else {
                    break;
                };
                if (target.x - me.x).abs().max((target.y - me.y).abs()) == 1 {
                    blows += 1;
                }
                app.handle_key(press(KeyCode::Char(direction_key(me, target))));
            }
            assert!(
                blows >= 2 && saw_wounded && saw_group,
                "{glyphs:?} {size:?}: blows {blows}, wounded {saw_wounded}, group {saw_group}"
            );
            // Everything near is dead by now or the far one is out of view.
            assert!(
                !in_view_lines(&app, size.0, size.1)
                    .iter()
                    .any(|l| l.starts_with(&format!("{letter} ")))
                    || app.game().visible_monsters().count() > 0
            );
        }
    }
}

/// The movement key that steps from `from` towards `to`.
fn direction_key(from: Position, to: Position) -> char {
    match ((to.x - from.x).signum(), (to.y - from.y).signum()) {
        (-1, 0) => 'h',
        (1, 0) => 'l',
        (0, -1) => 'k',
        (0, 1) => 'j',
        (-1, -1) => 'y',
        (1, -1) => 'u',
        (-1, 1) => 'b',
        (1, 1) => 'n',
        _ => '.',
    }
}

// ---------------------------------------------------------------------------
// Round on screen
// ---------------------------------------------------------------------------

/// Extent, in screen columns and rows, of the visible floor glyphs drawn on
/// the map area.
fn lit_extent(buffer: &Buffer, area: Rect, floor: char) -> (u16, u16) {
    let (mut xs, mut ys) = (HashSet::new(), HashSet::new());
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if buffer[(x, y)].symbol() == floor.to_string() {
                xs.insert(x);
                ys.insert(y);
            }
        }
    }
    let span = |set: &HashSet<u16>| set.iter().max().unwrap() - set.iter().min().unwrap() + 1;
    (span(&xs), span(&ys))
}

/// In the middle of a wide open space the lit area is 33 columns by 17 rows,
/// which is as wide as tall once a row is counted as two columns; at 80 × 24
/// the map area is 16 rows high, so one row of the lit area is cut off.
#[test]
fn the_lit_area_looks_round_on_screen_at_120_by_40_and_is_clipped_by_one_row_at_80_by_24() {
    let rows = open_room(80, 40);
    let theme = Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode);
    let game = custom_game(&rows, Position::new(40, 20));
    let mut app = App::with_game(theme.clone(), game, shipped());
    app.handle_key(press(KeyCode::Char('.')));

    let big = buffer(&app, 120, 40);
    let (width, height) = lit_extent(&big, map_area(Rect::new(0, 0, 120, 40)), theme.glyphs.floor);
    assert_eq!(
        (width, height),
        (33, 17),
        "120 x 40: the whole ellipse fits"
    );
    assert!(
        (i32::from(width) - 2 * i32::from(height)).abs() <= 1,
        "33 columns against 17 rows (two columns each) must look round"
    );

    let small = buffer(&app, 80, 24);
    let area = map_area(Rect::new(0, 0, 80, 24));
    assert_eq!(area.height, 16, "the map area is 16 rows high at 80 x 24");
    let (width, height) = lit_extent(&small, area, theme.glyphs.floor);
    assert_eq!(width, 33, "80 x 24: the full width fits");
    assert_eq!(
        height, 16,
        "80 x 24: 17 rows of light, 16 rows of room: one row is cut"
    );
}

/// A group of two shows the figures of the more wounded member even when it
/// is not the first of the list, and the count of the group.
#[test]
fn a_group_in_view_shows_the_health_of_its_most_wounded_member() {
    let content = sigilfall_core::content::Content::parse(
        "[Monster(id: \"brute\", name: \"brute\", name_fr: Masculine(\"le monstre\"), plural: \"brutes\", plural_fr: \"monstres\", family: Vermin, elite: false, speed: 1, \
         hp: 900, attack: 0, defense: 0, min_depth: 1)]",
    )
    .expect("test content is valid");
    let def = content.monster("brute").expect("brute").clone();
    let mut game = Game::in_fixed_room(seed_of(3));
    // First of the list, far and untouched; second of the list, adjacent.
    game.spawn_monster(Position::new(12, 5), &def);
    let adjacent = game.spawn_monster(Position::new(6, 5), &def);
    let theme = Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode);
    let mut app = App::with_game(theme, game, content);
    app.handle_key(press(KeyCode::Char('.')));
    let lines = in_view_lines(&app, 80, 24);
    let line = lines
        .iter()
        .find(|l| l.starts_with("v "))
        .expect("a vermin line");
    assert!(
        line.ends_with("900/900") && line.contains("\u{d7}2"),
        "{line:?}"
    );

    app.handle_key(press(KeyCode::Char('l')));
    let game = app.game();
    let wounded = game.actor(adjacent).expect("still alive");
    assert!(wounded.hp() < wounded.max_hp(), "the blow landed");
    let lines = in_view_lines(&app, 80, 24);
    let line = lines
        .iter()
        .find(|l| l.starts_with("v "))
        .expect("a vermin line");
    let expected = format!("{}/{}", wounded.hp(), wounded.max_hp());
    assert!(
        line.ends_with(&expected) && line.contains("\u{d7}2"),
        "{line:?} must show {expected}"
    );
}
