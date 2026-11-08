//! Field of view and fog, core half.
//!
//! They check what is shown, symmetry, monsters out of view, and the rules
//! of decision D15 (an ellipse reaching 16 cells along a row and 8 along a
//! column, light passing a diagonal gap, walls seen as soon as part of their
//! face is lit) against the public API only, and against an oracle written
//! here: whether a straight line between two points crosses the open
//! interior of a wall is decided with exact integer fractions, so the
//! shadowcasting code of the game and this check cannot share a mistake.
//!
//! Hand-made maps go through the save format (a saved game of the fixed
//! room whose tile rows are replaced), because `Map` has no public
//! constructor.

use std::collections::HashSet;

use sigilfall_core::action::Action;
use sigilfall_core::content::{Content, FloorKind};
use sigilfall_core::fov::{SIGHT_RANGE, SightRange, visible_cells};
use sigilfall_core::game::Game;
use sigilfall_core::generation::{FIRST_DEPTH, generate, generate_of_kind, generate_with_monsters};
use sigilfall_core::geometry::{Direction, Position};
use sigilfall_core::map::{Map, Tile};
use sigilfall_core::rng::Seed;
use sigilfall_core::save;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// A seed spread over the whole range from a small number.
fn seed_of(n: u64) -> Seed {
    Seed::from(n.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x3C3C)
}

/// A small deterministic dice (xorshift), so a failure replays identically.
struct Dice(u64);

impl Dice {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn content() -> Content {
    Content::load().expect("the shipped content is valid")
}

fn at(x: i32, y: i32) -> Position {
    Position::new(x, y)
}

/// Every cell of `map` in reading order.
fn cells(map: &Map) -> impl Iterator<Item = Position> + '_ {
    (0..map.height() as i32).flat_map(move |y| (0..map.width() as i32).map(move |x| at(x, y)))
}

fn is_wall(map: &Map, p: Position) -> bool {
    map.tile(p) == Some(Tile::Wall)
}

/// Cells of `map` an actor can stand on.
fn walkable_cells(map: &Map) -> Vec<Position> {
    cells(map).filter(|&p| map.is_walkable(p)).collect()
}

/// A game on the hand-made map `rows` (`#` wall, `.` floor, `>` stairs) with
/// the player at `player`.
fn custom_game(rows: &[&str], player: Position) -> Game {
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
    save::from_str(&text, &content()).expect("hand-made map loads")
}

/// A walled room of `width` × `height` cells, floor inside.
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

/// A creature that takes a turn only once in a hundred ticks, so a test can
/// let a few turns pass without it moving.
fn snail() -> sigilfall_core::content::MonsterDef {
    let content = Content::parse(
        "[Monster(id: \"snail\", name: \"snail\", family: Vermin, elite: false, speed: 1, \
         hp: 900, attack: 0, defense: 0, min_depth: 1)]",
    )
    .expect("test content is valid");
    content.monster("snail").expect("snail").clone()
}

/// Lets one turn pass so the game computes the view from the player's cell:
/// a game loaded from a hand-made save starts with nothing in view.
fn settle(game: &mut Game) {
    game.submit(Action::Wait);
}

fn as_refs(rows: &[String]) -> Vec<&str> {
    rows.iter().map(String::as_str).collect()
}

fn set_of(cells: Vec<Position>) -> HashSet<Position> {
    cells.into_iter().collect()
}

/// Whether a cell `dx` columns and `dy` rows away is inside the ellipse of
/// half-axes `columns + ½` and `rows + ½`. In floating point on purpose: the
/// two sides can never be equal for whole `dx` and `dy`, and every value is a
/// multiple of 1/16, so the comparison is exact.
fn in_ellipse(dx: i32, dy: i32, range: SightRange) -> bool {
    let a = f64::from(range.columns) + 0.5;
    let b = f64::from(range.rows) + 0.5;
    let (dx, dy) = (f64::from(dx), f64::from(dy));
    dx * dx * b * b + dy * dy * a * a < a * a * b * b
}

// -- exact segment / wall oracle ---------------------------------------------

/// A fraction `(numerator, denominator)` with a positive denominator.
type Frac = (i128, i128);

/// Coordinates are kept in sixteenths of a cell; a cell is a square of side
/// `SCALE` centred on `16 · (x, y)`.
const SCALE: i64 = 16;

fn frac(numerator: i64, denominator: i64) -> Frac {
    if denominator < 0 {
        (-i128::from(numerator), -i128::from(denominator))
    } else {
        (i128::from(numerator), i128::from(denominator))
    }
}

fn less(a: Frac, b: Frac) -> bool {
    a.0 * b.1 < b.0 * a.1
}

/// Whether the segment `from` → `to` passes through the open interior of the
/// square of `cell` (touching its edge or a corner does not count).
fn enters_cell(from: (i64, i64), to: (i64, i64), cell: Position) -> bool {
    let mut low: Frac = (-1_000_000, 1);
    let mut high: Frac = (1_000_000, 1);
    for (a, b, c) in [(from.0, to.0, cell.x), (from.1, to.1, cell.y)] {
        let min = i64::from(c) * SCALE - SCALE / 2;
        let max = min + SCALE;
        let delta = b - a;
        if delta == 0 {
            if !(a > min && a < max) {
                return false;
            }
            continue;
        }
        let (t1, t2) = (frac(min - a, delta), frac(max - a, delta));
        let (t1, t2) = if less(t2, t1) { (t2, t1) } else { (t1, t2) };
        if less(low, t1) {
            low = t1;
        }
        if less(t2, high) {
            high = t2;
        }
    }
    less(low, high) && less(low, (1, 1)) && less((0, 1), high)
}

fn centre(p: Position) -> (i64, i64) {
    (i64::from(p.x) * SCALE, i64::from(p.y) * SCALE)
}

/// Whether no wall of `map` is crossed by the segment `from` → `to`.
fn line_is_clear(map: &Map, from: (i64, i64), to: (i64, i64)) -> bool {
    let (x0, x1) = (from.0.min(to.0) / SCALE - 1, from.0.max(to.0) / SCALE + 1);
    let (y0, y1) = (from.1.min(to.1) / SCALE - 1, from.1.max(to.1) / SCALE + 1);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let cell = at(x as i32, y as i32);
            if is_wall(map, cell) && enters_cell(from, to, cell) {
                return false;
            }
        }
    }
    true
}

/// Whether some part of a face of the wall at `wall` (an edge it shares with
/// a cell that is not a wall) can be reached by a straight line from the
/// centre of `origin`. Seven points per face, none of them a corner.
fn some_face_is_lit(map: &Map, origin: Position, wall: Position) -> bool {
    for (dx, dy) in [(0, -1), (0, 1), (-1, 0), (1, 0)] {
        match map.tile(at(wall.x + dx, wall.y + dy)) {
            Some(tile) if tile != Tile::Wall => {}
            _ => continue,
        }
        for offset in [-6, -4, -2, 0, 2, 4, 6] {
            let (px, py) = if dx != 0 {
                (
                    i64::from(wall.x) * SCALE + i64::from(dx) * 8,
                    i64::from(wall.y) * SCALE + offset,
                )
            } else {
                (
                    i64::from(wall.x) * SCALE + offset,
                    i64::from(wall.y) * SCALE + i64::from(dy) * 8,
                )
            };
            if line_is_clear(map, centre(origin), (px, py)) {
                return true;
            }
        }
    }
    // A corner of the wall that touches a cell which is not a wall, even
    // only by the corner (the outer corner of a room), counts as lit when a
    // line reaches that corner.
    for (dx, dy) in [(-1, -1), (1, -1), (-1, 1), (1, 1)] {
        match map.tile(at(wall.x + dx, wall.y + dy)) {
            Some(tile) if tile != Tile::Wall => {}
            _ => continue,
        }
        let corner = (
            i64::from(wall.x) * SCALE + i64::from(dx) * 8,
            i64::from(wall.y) * SCALE + i64::from(dy) * 8,
        );
        if line_is_clear(map, centre(origin), corner) {
            return true;
        }
    }
    false
}

/// Whether the segment `from` -> `to` passes through the open interior of the
/// diamond inscribed in the square of `cell` (its four corners are the middles
/// of the square's sides). This is the shape the shadowcasting algorithm gives
/// a wall: a line may graze a wall's corner region without being blocked.
fn enters_diamond(from: (i64, i64), to: (i64, i64), cell: Position) -> bool {
    let half = SCALE / 2;
    let (cx, cy) = (i64::from(cell.x) * SCALE, i64::from(cell.y) * SCALE);
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    // |x - cx| + |y - cy| is convex along the segment: its minimum is at an
    // end or where x = cx or y = cy.
    let at_end = |x: i64, y: i64| (x - cx).abs() + (y - cy).abs() < half;
    if at_end(from.0, from.1) || at_end(to.0, to.1) {
        return true;
    }
    if dx != 0 {
        // Parameter where x = cx, if inside the segment.
        let t = frac(cx - from.0, dx);
        if !less(t, (0, 1)) && !less((1, 1), t) {
            // |y(t) - cy| = |(from.1 - cy) * dx + (cx - from.0) * dy| / |dx|
            let numerator = ((from.1 - cy) * dx + (cx - from.0) * dy).abs();
            if numerator < half * dx.abs() {
                return true;
            }
        }
    }
    if dy != 0 {
        let t = frac(cy - from.1, dy);
        if !less(t, (0, 1)) && !less((1, 1), t) {
            let numerator = ((from.0 - cx) * dy + (cy - from.1) * dx).abs();
            if numerator < half * dy.abs() {
                return true;
            }
        }
    }
    false
}

/// Like [`line_is_clear`] with walls shaped as diamonds (see
/// [`enters_diamond`]).
fn line_is_clear_of_diamonds(map: &Map, from: (i64, i64), to: (i64, i64)) -> bool {
    let (x0, x1) = (from.0.min(to.0) / SCALE - 1, from.0.max(to.0) / SCALE + 1);
    let (y0, y1) = (from.1.min(to.1) / SCALE - 1, from.1.max(to.1) / SCALE + 1);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let cell = at(x as i32, y as i32);
            if is_wall(map, cell) && enters_diamond(from, to, cell) {
                return false;
            }
        }
    }
    true
}

/// The floors the generator makes: rooms and caves, various depths.
fn generated_floors(count: u64) -> Vec<(String, sigilfall_core::generation::Floor)> {
    let mut floors = Vec::new();
    for n in 0..count {
        let seed = seed_of(n);
        floors.push((
            format!("rooms seed #{n}"),
            generate(seed, 1 + (n % 5) as u32),
        ));
        floors.push((
            format!("caves seed #{n}"),
            generate_of_kind(seed, 3 + (n % 7) as u32, FloorKind::Caves),
        ));
    }
    floors
}

// ---------------------------------------------------------------------------
// Only what is in view is shown clearly, what was seen is kept
// ---------------------------------------------------------------------------

/// What the game reports as visible equals a fresh computation from the
/// player's cell after every move, on rooms and caves; every visible cell is
/// remembered; the remembered cells are exactly the union of everything seen
/// so far and never shrink.
#[test]
fn the_view_is_recomputed_after_every_move_and_what_was_seen_is_kept() {
    for (label, floor) in generated_floors(24) {
        let seed = seed_of(7);
        let mut game = Game::on_floor(seed, floor);
        let mut dice = Dice(0xF3F3_0000 + label.len() as u64);
        let mut ever_seen: HashSet<Position> = HashSet::new();
        let mut moved = 0;
        for step in 0..160 {
            let player = game.player_actor().position();
            let expected = set_of(visible_cells(game.map(), player, SIGHT_RANGE));
            let shown: HashSet<Position> =
                cells(game.map()).filter(|&p| game.is_visible(p)).collect();
            assert_eq!(
                shown, expected,
                "{label}, step {step}: visible set differs from a fresh computation"
            );
            assert!(
                game.is_visible(player),
                "{label}: the player's own cell is always visible"
            );
            ever_seen.extend(expected.iter().copied());
            let remembered: HashSet<Position> = cells(game.map())
                .filter(|&p| game.is_remembered(p))
                .collect();
            assert_eq!(
                remembered, ever_seen,
                "{label}, step {step}: remembered must be exactly what was ever seen"
            );
            // Outside the map nothing is visible or remembered.
            assert!(!game.is_visible(at(-1, 0)) && !game.is_remembered(at(0, -1)));
            let direction = Direction::ALL[dice.below(8)];
            let before = game.player_turns();
            game.submit(Action::Move(direction));
            if game.player_turns() > before {
                moved += 1;
            }
        }
        assert!(
            moved > 20,
            "{label}: the walk should actually move ({moved} moves)"
        );
    }
}

/// A cell the player has not been able to see so far is unknown: not visible
/// and not remembered, however close it is to the walls of the explored area.
#[test]
fn cells_outside_the_ellipse_of_every_position_visited_stay_unknown() {
    let floor = generate(seed_of(3), 1);
    let mut game = Game::on_floor(seed_of(3), floor);
    let start = game.player_actor().position();
    let mut visited = vec![start];
    for _ in 0..12 {
        game.submit(Action::Move(Direction::East));
        visited.push(game.player_actor().position());
    }
    for p in cells(game.map()) {
        let reachable = visited
            .iter()
            .any(|v| in_ellipse(p.x - v.x, p.y - v.y, SIGHT_RANGE));
        if !reachable {
            assert!(
                !game.is_remembered(p),
                "{p:?} was never in range, yet remembered"
            );
            assert!(!game.is_visible(p), "{p:?} is out of range, yet visible");
        }
    }
}

// ---------------------------------------------------------------------------
// Symmetry
// ---------------------------------------------------------------------------

/// If A sees B then B sees A, on real rooms and caves, for the player's
/// range and for several others, with every cell that lets sight through
/// (a wall is lit by sight but sees nothing, so it is outside the property).
#[test]
fn if_a_sees_b_then_b_sees_a_on_generated_floors() {
    let ranges = [
        SIGHT_RANGE,
        SightRange::new(5, 3),
        SightRange::new(3, 9),
        SightRange::new(8, 8),
    ];
    let mut pairs = 0u64;
    for (label, floor) in generated_floors(16) {
        let map = floor.map();
        let open = walkable_cells(map);
        let mut dice = Dice(0x5151 + label.len() as u64);
        for range in ranges {
            for _ in 0..6 {
                let a = open[dice.below(open.len())];
                for b in visible_cells(map, a, range) {
                    if !map.is_walkable(b) {
                        continue;
                    }
                    pairs += 1;
                    assert!(
                        visible_cells(map, b, range).contains(&a),
                        "{label}: {a:?} sees {b:?} with {range:?}, but {b:?} does not see {a:?}"
                    );
                }
            }
        }
    }
    assert!(
        pairs > 50_000,
        "the property should be exercised widely ({pairs} pairs)"
    );
}

/// The same property on every cell of a small open room with pillars, all
/// pairs, for the player's range.
#[test]
fn symmetry_holds_for_every_pair_of_cells_of_a_room_with_pillars() {
    let mut rows = open_room(40, 14);
    for (x, y) in [
        (10, 4),
        (11, 4),
        (20, 7),
        (21, 8),
        (30, 3),
        (30, 10),
        (5, 9),
        (6, 10),
    ] {
        rows[y].replace_range(x..=x, "#");
    }
    let game = custom_game(&as_refs(&rows), at(2, 2));
    let map = game.map();
    let open = walkable_cells(map);
    let views: Vec<HashSet<Position>> = open
        .iter()
        .map(|&a| set_of(visible_cells(map, a, SIGHT_RANGE)))
        .collect();
    for (i, &a) in open.iter().enumerate() {
        for (j, &b) in open.iter().enumerate() {
            assert_eq!(
                views[i].contains(&b),
                views[j].contains(&a),
                "{a:?} and {b:?}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// D15 (revised): range 16 along a row, 8 along a column, an ellipse
// ---------------------------------------------------------------------------

#[test]
fn the_player_sees_16_cells_along_a_row_and_8_along_a_column() {
    assert_eq!((SIGHT_RANGE.columns, SIGHT_RANGE.rows), (16, 8));
    let room = Map::fixed_room();
    // Fixed room: 30 × 12, floor from (1, 1) to (28, 10). The pillar is at
    // (15, 6), off every line used here.
    let from_top_left = set_of(visible_cells(&room, at(1, 1), SIGHT_RANGE));
    assert!(
        from_top_left.contains(&at(17, 1)),
        "16 cells along a row are seen"
    );
    assert!(!from_top_left.contains(&at(18, 1)), "the 17th is not");
    assert!(
        from_top_left.contains(&at(1, 9)),
        "8 cells down a column are seen"
    );
    assert!(!from_top_left.contains(&at(1, 10)), "the 9th is not");
    let from_bottom_right = set_of(visible_cells(&room, at(28, 10), SIGHT_RANGE));
    assert!(
        from_bottom_right.contains(&at(12, 10)),
        "16 cells to the left"
    );
    assert!(!from_bottom_right.contains(&at(11, 10)), "not 17");
    assert!(from_bottom_right.contains(&at(28, 2)), "8 cells up");
    assert!(!from_bottom_right.contains(&at(28, 1)), "not 9");
}

/// In a room too big for the ellipse to touch a wall, the lit floor is
/// exactly the ellipse: nothing beyond 16 columns or 8 rows, nothing missing
/// inside, whatever the direction.
#[test]
fn in_the_open_the_lit_floor_is_exactly_the_ellipse() {
    let rows = open_room(60, 30);
    let game = custom_game(&as_refs(&rows), at(30, 15));
    let map = game.map();
    let origin = at(30, 15);
    let seen = set_of(visible_cells(map, origin, SIGHT_RANGE));
    let mut lit_floor = 0;
    for p in cells(map) {
        if !map.is_walkable(p) {
            continue;
        }
        let expected = in_ellipse(p.x - origin.x, p.y - origin.y, SIGHT_RANGE);
        assert_eq!(seen.contains(&p), expected, "floor {p:?}");
        lit_floor += usize::from(expected);
    }
    // 16 · 2 + 1 = 33 columns wide on the middle row, 17 rows high on the
    // middle column.
    for dx in -16..=16 {
        assert!(seen.contains(&at(30 + dx, 15)));
    }
    assert!(!seen.contains(&at(30 + 17, 15)) && !seen.contains(&at(30 - 17, 15)));
    for dy in -8..=8 {
        assert!(seen.contains(&at(30, 15 + dy)));
    }
    assert!(!seen.contains(&at(30, 15 + 9)) && !seen.contains(&at(30, 15 - 9)));
    assert!(
        lit_floor > 300,
        "an ellipse of that size lights hundreds of cells ({lit_floor})"
    );
    // The diagonal reaches 7 cells, not a square's 8 and not a diamond's 5.
    assert!(seen.contains(&at(37, 22)) && !seen.contains(&at(38, 23)));
    assert!(seen.contains(&at(23, 8)) && !seen.contains(&at(22, 7)));
}

/// On screen a terminal cell is about twice as tall as wide, so the ellipse
/// of cells must look round: its extent in columns is twice its extent in
/// rows.
#[test]
fn the_lit_area_is_as_wide_as_tall_once_cells_are_twice_as_tall_as_wide() {
    let rows = open_room(60, 30);
    let game = custom_game(&as_refs(&rows), at(30, 15));
    let seen = visible_cells(game.map(), at(30, 15), SIGHT_RANGE);
    let floor: Vec<&Position> = seen
        .iter()
        .filter(|p| game.map().is_walkable(**p))
        .collect();
    let width =
        floor.iter().map(|p| p.x).max().unwrap() - floor.iter().map(|p| p.x).min().unwrap() + 1;
    let height =
        floor.iter().map(|p| p.y).max().unwrap() - floor.iter().map(|p| p.y).min().unwrap() + 1;
    assert_eq!((width, height), (33, 17));
    // Roundness on screen: 33 columns against 17 rows · 2 = 34 columns of
    // height, within one column.
    assert!((width - height * 2).abs() <= 1);
    // The same ratio holds on the diagonals: the reach on a 45 degree line
    // (7 cells) is, on screen, 7 columns across and 14 columns down, and the
    // circle through the row and column tips has a screen radius of 16/16.5.
    let on_screen_radius = |p: &Position| {
        let dx = f64::from(p.x - 30);
        let dy = f64::from(p.y - 15) * 2.0;
        (dx * dx + dy * dy).sqrt()
    };
    let farthest = floor
        .iter()
        .map(|p| on_screen_radius(p))
        .fold(0.0, f64::max);
    let nearest_dark = cells(game.map())
        .filter(|p| game.map().is_walkable(*p) && !seen.contains(p))
        .map(|p| on_screen_radius(&p))
        .fold(f64::MAX, f64::min);
    assert!(
        farthest < 17.0,
        "farthest lit cell is {farthest:.2} columns from the player on screen"
    );
    assert!(
        nearest_dark > 15.0,
        "nearest dark cell is {nearest_dark:.2} columns away on screen"
    );
}

/// Nothing is ever seen beyond the ellipse, on the real floors.
#[test]
fn no_cell_is_seen_beyond_the_range_on_generated_floors() {
    for (label, floor) in generated_floors(20) {
        let map = floor.map();
        let open = walkable_cells(map);
        let mut dice = Dice(0x9090 + label.len() as u64);
        for _ in 0..8 {
            let a = open[dice.below(open.len())];
            for b in visible_cells(map, a, SIGHT_RANGE) {
                assert!(
                    in_ellipse(b.x - a.x, b.y - a.y, SIGHT_RANGE),
                    "{label}: {a:?} sees {b:?}, beyond 16 across / 8 down"
                );
                assert!((b.x - a.x).abs() <= 16 && (b.y - a.y).abs() <= 8);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// D15 (revised): the diagonal gap
// ---------------------------------------------------------------------------

/// Two walls touching by a corner leave a gap an actor can walk through; sight
/// goes through it, in both directions, and a solid wall stops it.
#[test]
fn sight_goes_through_a_diagonal_gap_that_an_actor_can_walk_through() {
    // A divider at column 7 with a diagonal step: walls (7,3) and (8,4) touch
    // only by their corners; (7,4) and (8,3) are the two floor cells of the
    // gap.
    let rows = [
        "###############",
        "#......#......#",
        "#......#......#",
        "#......#......#",
        "#.......#.....#",
        "#.......#.....#",
        "#.......#.....#",
        "###############",
    ];
    let game = custom_game(&rows, at(6, 4));
    let map = game.map();
    assert!(is_wall(map, at(7, 3)) && is_wall(map, at(8, 4)));
    assert!(!is_wall(map, at(7, 4)) && !is_wall(map, at(8, 3)));
    // An actor standing in the gap can step diagonally across it, so it is a
    // passage.
    assert!(map.can_step(at(7, 4), Direction::NorthEast));
    assert!(map.can_step(at(8, 3), Direction::SouthWest));

    // Standing on the near side of the gap the player sees the far side.
    let from_gap_west = set_of(visible_cells(map, at(7, 4), SIGHT_RANGE));
    assert!(from_gap_west.contains(&at(8, 3)));
    assert!(
        from_gap_west.contains(&at(10, 1)),
        "a cell further up the diagonal"
    );
    // The line (7,4) -> (10,1) touches the corners of both walls: allowed.
    assert!(line_is_clear(map, centre(at(7, 4)), centre(at(10, 1))));
    // And symmetric.
    let from_far = set_of(visible_cells(map, at(10, 1), SIGHT_RANGE));
    assert!(from_far.contains(&at(7, 4)));
    // The solid part of the divider still blocks: no line of sight from the
    // west room into the east room away from the gap.
    let from_west = set_of(visible_cells(map, at(3, 1), SIGHT_RANGE));
    assert!(
        !from_west.contains(&at(10, 1)),
        "straight through a solid wall"
    );
    assert!(!from_west.contains(&at(12, 5)));
    // For every pair of floor cells on either side, the property of
    // symmetry also holds across the gap.
    for a in walkable_cells(map) {
        let view = set_of(visible_cells(map, a, SIGHT_RANGE));
        for b in view.iter().filter(|b| map.is_walkable(**b)) {
            assert!(
                visible_cells(map, *b, SIGHT_RANGE).contains(&a),
                "{a:?} / {b:?}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// D15 (revised): walls
// ---------------------------------------------------------------------------

/// From inside a room every wall within range is seen, corners included.
#[test]
fn from_inside_a_room_every_wall_within_range_is_seen_corners_included() {
    let rows = open_room(26, 12);
    let game = custom_game(&as_refs(&rows), at(13, 6));
    let map = game.map();
    let seen = set_of(visible_cells(map, at(13, 6), SIGHT_RANGE));
    for wall in cells(map).filter(|p| is_wall(map, *p)) {
        let in_range = in_ellipse(wall.x - 13, wall.y - 6, SIGHT_RANGE);
        assert_eq!(seen.contains(&wall), in_range, "wall {wall:?}");
    }
    // The four corners of this room are within range of its centre.
    for corner in [at(0, 0), at(25, 0), at(0, 11), at(25, 11)] {
        let in_range = in_ellipse(corner.x - 13, corner.y - 6, SIGHT_RANGE);
        assert_eq!(seen.contains(&corner), in_range, "corner {corner:?}");
    }
}

/// A wall is seen as soon as any part of one of its faces is lit: checked on
/// real floors against the exact oracle. Every wall with a lit face within
/// range must be listed (no wall left dark that shows its face), and every
/// wall listed must have a lit face (nothing seen through rock).
#[test]
fn a_wall_is_seen_as_soon_as_part_of_its_face_is_lit() {
    let mut checked = 0u64;
    let mut missing: Vec<String> = Vec::new();
    let mut through_rock: Vec<String> = Vec::new();
    for (label, floor) in generated_floors(12) {
        let map = floor.map();
        let open = walkable_cells(map);
        let mut dice = Dice(0x7777 + label.len() as u64);
        for _ in 0..6 {
            let origin = open[dice.below(open.len())];
            let seen = set_of(visible_cells(map, origin, SIGHT_RANGE));
            for wall in cells(map).filter(|p| is_wall(map, *p)) {
                let in_range = in_ellipse(wall.x - origin.x, wall.y - origin.y, SIGHT_RANGE);
                if !in_range {
                    assert!(
                        !seen.contains(&wall),
                        "{label}: wall {wall:?} seen beyond the range of {origin:?}"
                    );
                    continue;
                }
                let lit = some_face_is_lit(map, origin, wall);
                checked += 1;
                if lit && !seen.contains(&wall) {
                    missing.push(format!(
                        "{label}: from {origin:?}, wall {wall:?} has a lit face but is not seen"
                    ));
                }
                // A wall shown must at least touch open ground: nothing deep
                // inside the rock is ever drawn.
                let touches_open_ground = Direction::ALL.iter().any(|d| {
                    let (dx, dy) = d.delta();
                    map.tile(at(wall.x + dx, wall.y + dy))
                        .is_some_and(|t| t != Tile::Wall)
                });
                if seen.contains(&wall) && !touches_open_ground {
                    through_rock.push(format!(
                        "{label}: from {origin:?}, wall {wall:?} is seen deep in the rock"
                    ));
                }
            }
        }
    }
    assert!(checked > 10_000);
    assert!(
        missing.is_empty(),
        "{} walls dark despite a lit face, e.g. {:?}",
        missing.len(),
        &missing[..missing.len().min(5)]
    );
    assert!(
        through_rock.is_empty(),
        "{} walls seen deep in the rock, e.g. {:?}",
        through_rock.len(),
        &through_rock[..through_rock.len().min(5)]
    );
}

/// A floor cell is seen only when the line to its centre stays clear of every
/// wall (no peeking through rock), on rooms and caves. Walls count as the
/// diamonds the algorithm treats them as: a line may graze the corner region
/// of a square wall.
#[test]
fn a_floor_cell_is_seen_only_along_a_clear_line() {
    let mut checked = 0u64;
    for (label, floor) in generated_floors(12) {
        let map = floor.map();
        let open = walkable_cells(map);
        let mut dice = Dice(0x1234 + label.len() as u64);
        for _ in 0..6 {
            let origin = open[dice.below(open.len())];
            for b in visible_cells(map, origin, SIGHT_RANGE) {
                if map.is_walkable(b) {
                    checked += 1;
                    assert!(
                        line_is_clear_of_diamonds(map, centre(origin), centre(b)),
                        "{label}: {origin:?} sees {b:?} but a wall is in the way"
                    );
                }
            }
        }
    }
    assert!(checked > 10_000);
}

/// No blind spot: every floor cell in range whose centre line is clear is
/// seen, and every neighbour of the player is seen.
#[test]
fn a_floor_cell_in_range_with_a_clear_line_is_never_hidden() {
    let mut hidden: Vec<String> = Vec::new();
    for (label, floor) in generated_floors(12) {
        let map = floor.map();
        let open = walkable_cells(map);
        let mut dice = Dice(0x4242 + label.len() as u64);
        for _ in 0..6 {
            let origin = open[dice.below(open.len())];
            let seen = set_of(visible_cells(map, origin, SIGHT_RANGE));
            for dir in Direction::ALL {
                let (dx, dy) = dir.delta();
                let n = at(origin.x + dx, origin.y + dy);
                if map.is_walkable(n) {
                    assert!(
                        seen.contains(&n),
                        "{label}: neighbour {n:?} of {origin:?} is not seen"
                    );
                }
            }
            for b in &open {
                if in_ellipse(b.x - origin.x, b.y - origin.y, SIGHT_RANGE)
                    && line_is_clear_of_diamonds(map, centre(origin), centre(*b))
                    && !seen.contains(b)
                {
                    hidden.push(format!("{label}: {b:?} from {origin:?}"));
                }
            }
        }
    }
    // The symmetric algorithm is allowed to be a little stricter than a
    // plain line test near grazing lines; it must stay rare.
    assert!(
        hidden.len() < 40,
        "{} clear-line cells hidden, e.g. {:?}",
        hidden.len(),
        &hidden[..hidden.len().min(5)]
    );
}

// ---------------------------------------------------------------------------
// Monsters out of the field of view
// ---------------------------------------------------------------------------

/// `visible_monsters` lists exactly the monsters on a visible cell, on real
/// floors with real monsters, along a long walk.
#[test]
fn a_monster_out_of_view_is_never_listed_and_one_in_view_always_is() {
    let content = content();
    let mut listed_total = 0usize;
    let mut hidden_total = 0usize;
    for n in 0..80 {
        let seed = seed_of(100 + n);
        let mut game = Game::new(seed, &content);
        let mut dice = Dice(0xABCD + n);
        for step in 0..150 {
            let visible_ids: HashSet<_> = game.visible_monsters().map(|m| m.id).collect();
            for (id, actor) in game.actors() {
                if actor.as_monster().is_none() || !actor.is_alive() {
                    continue;
                }
                let in_view = game.is_visible(actor.position());
                assert_eq!(
                    visible_ids.contains(&id),
                    in_view,
                    "seed #{n}, step {step}: monster at {:?} (visible cell: {in_view})",
                    actor.position()
                );
                if in_view {
                    let player = game.player_actor().position();
                    let p = actor.position();
                    assert!(in_ellipse(p.x - player.x, p.y - player.y, SIGHT_RANGE));
                    listed_total += 1;
                } else {
                    hidden_total += 1;
                }
            }
            for listed in game.visible_monsters() {
                assert!(game.is_visible(listed.position));
            }
            if !matches!(game.status(), sigilfall_core::game::Status::Playing) {
                break;
            }
            let direction = Direction::ALL[dice.below(8)];
            game.submit(Action::Move(direction));
        }
    }
    assert!(
        listed_total > 100 && hidden_total > 10_000,
        "listed {listed_total}, hidden {hidden_total}"
    );
}

/// Placed by hand on a long open floor, a monster is listed at 16 cells along
/// the row and not at 17, listed at 8 along the column and not at 9, and
/// never when a wall is between.
#[test]
fn a_monster_is_listed_at_the_edge_of_the_range_and_not_beyond_or_behind_a_wall() {
    let rat = snail();
    let rows = open_room(60, 24);
    let cases = [
        (at(10, 12), at(26, 12), true, "16 along the row"),
        (at(10, 12), at(27, 12), false, "17 along the row"),
        (
            at(40, 12),
            at(24, 12),
            true,
            "16 along the row, to the left",
        ),
        (
            at(40, 12),
            at(23, 12),
            false,
            "17 along the row, to the left",
        ),
        (at(20, 5), at(20, 13), true, "8 down the column"),
        (at(20, 5), at(20, 14), false, "9 down the column"),
        (at(20, 14), at(20, 6), true, "8 up the column"),
        (at(20, 14), at(20, 5), false, "9 up the column"),
        (at(20, 12), at(27, 19), true, "7 across the diagonal"),
        (at(20, 12), at(28, 20), false, "8 across the diagonal"),
    ];
    for (player, monster, expected, what) in cases {
        let mut game = custom_game(&as_refs(&rows), player);
        game.spawn_monster(monster, &rat);
        settle(&mut game);
        let listed = game.visible_monsters().count() == 1;
        assert_eq!(
            listed, expected,
            "{what}: monster {monster:?}, player {player:?}"
        );
        assert_eq!(
            game.is_visible(monster),
            expected,
            "{what}: the cell itself"
        );
    }

    // Behind a wall, in range: not listed. The monster stands right behind a
    // full divider.
    let mut walled = open_room(30, 9);
    for row in walled.iter_mut().take(8).skip(1) {
        row.replace_range(15..=15, "#");
    }
    let mut game = custom_game(&as_refs(&walled), at(10, 4));
    game.spawn_monster(at(17, 4), &rat);
    game.spawn_monster(at(16, 4), &rat);
    settle(&mut game);
    assert_eq!(
        game.visible_monsters().count(),
        0,
        "a divider hides the monsters behind it"
    );
    assert!(
        game.is_visible(at(15, 4)),
        "the near face of the divider is lit"
    );
    assert!(!game.is_visible(at(16, 4)));
}

/// A monster that was in view and walks out of it disappears from the list
/// at once, although its cell stays remembered.
#[test]
fn a_monster_that_leaves_the_view_vanishes_from_the_list_though_its_cell_is_remembered() {
    let rat = snail();
    let rows = open_room(60, 8);
    let mut game = custom_game(&as_refs(&rows), at(10, 4));
    game.spawn_monster(at(26, 4), &rat);
    settle(&mut game);
    assert_eq!(game.visible_monsters().count(), 1);
    // The player steps back: the monster is now 17 away.
    game.submit(Action::Move(Direction::West));
    assert!(
        game.is_remembered(at(26, 4)),
        "the cell was seen a moment ago"
    );
    assert!(!game.is_visible(at(26, 4)));
    assert_eq!(
        game.visible_monsters()
            .filter(|m| m.position == at(26, 4))
            .count(),
        0
    );
}

/// D16: from the start cell of a floor no monster is in view, on every depth
/// the game has, and a new game starts with none listed.
#[test]
fn no_monster_is_in_view_from_the_start_of_a_floor() {
    let content = content();
    let mut floors = 0;
    for n in 0..300u64 {
        let seed = seed_of(1000 + n);
        for depth in 1..=10u32 {
            let floor = generate_with_monsters(seed, depth, &content);
            let in_view = set_of(visible_cells(floor.map(), floor.start(), SIGHT_RANGE));
            for spawn in floor.monsters() {
                assert!(
                    !in_view.contains(&spawn.position()),
                    "seed #{n}, depth {depth}: a monster at {:?} is in view of the start {:?}",
                    spawn.position(),
                    floor.start()
                );
            }
            floors += 1;
        }
        let game = Game::new(seed, &content);
        assert_eq!(
            game.visible_monsters().count(),
            0,
            "seed #{n}: a monster is listed on arrival"
        );
    }
    assert_eq!(floors, 3000);
    assert_eq!(FIRST_DEPTH, 1);
}

// ---------------------------------------------------------------------------
// Sight against the reach of spells (decisions D15 and D19)
// ---------------------------------------------------------------------------

/// Shipped sigils learnt by walking over their glyphs, as the game teaches
/// them, in the fixed room (the player ends on the start cell).
fn learnt(ids: &[&str]) -> Game {
    use sigilfall_core::game::PLAYER_START;
    let content = content();
    let mut game = Game::in_fixed_room(Seed::from(5));
    let mut cell = PLAYER_START;
    for id in ids {
        cell = cell.step(Direction::East);
        game.engrave(cell, content.sigil(id).expect("a shipped sigil"));
    }
    for _ in ids {
        game.submit(Action::Move(Direction::East));
    }
    for _ in ids {
        game.submit(Action::Move(Direction::West));
    }
    assert_eq!(game.grimoire().len(), ids.len(), "every glyph was read");
    game
}

fn cast(
    game: &mut Game,
    digits: &[u8],
    direction: Option<Direction>,
) -> Vec<sigilfall_core::event::Event> {
    use sigilfall_core::sigil::{Node, Trace};
    let mut trace = Trace::new();
    for &digit in digits {
        trace
            .push(Node::new(digit).expect("digit 1-9"))
            .expect("node not linked yet");
    }
    game.submit(Action::Cast { trace, direction })
}

fn wounded(events: &[sigilfall_core::event::Event], id: sigilfall_core::actor::ActorId) -> bool {
    events
        .iter()
        .any(|e| matches!(e, sigilfall_core::event::Event::Wounded { actor, .. } if *actor == id))
}

/// Whether `events` wound or hold the actor `id`: the effect of Ember or Veil
/// reached it.
fn touched(events: &[sigilfall_core::event::Event], id: sigilfall_core::actor::ActorId) -> bool {
    use sigilfall_core::event::Event;
    events.iter().any(
        |e| matches!(e, Event::Wounded { actor, .. } | Event::Held { actor, .. } if *actor == id),
    )
}

/// Whether `events` report that the spell `sigil` found no target.
fn missed(events: &[sigilfall_core::event::Event], sigil: &str) -> bool {
    events.iter().any(|e| {
        matches!(e, sigilfall_core::event::Event::SpellMissed { sigil: missed } if missed == sigil)
    })
}

/// A spell on the nearest foe in sight (Ember, Veil) reaches the foes in
/// sight within the disc of radius 8 it had before the 16 × 8 view
/// (`sigil::NEAREST_FOE_REACH`), so a foe seen 9 to 16 cells away along a row
/// is out of its reach, as it is out of the reach of a spell aimed in a
/// direction (Lance), which still stops at 8.
#[test]
fn a_spell_on_the_nearest_foe_in_sight_and_an_aimed_one_both_stop_at_8_along_a_row() {
    let snail = snail();
    let player = Position::new(5, 5);

    // Ember and Veil: a foe in sight 9 to 16 cells east is not reached.
    for (id, digits) in [("ember", &[7, 5, 3][..]), ("veil", &[4, 7, 8, 9, 6][..])] {
        for gap in [9, 12, 16] {
            let mut game = learnt(&["ember", "veil"]);
            let far = game.spawn_monster(at(player.x + gap, 5), &snail);
            settle(&mut game);
            assert!(game.is_visible(at(player.x + gap, 5)));
            let events = cast(&mut game, digits, None);
            assert!(
                missed(&events, id) && !touched(&events, far),
                "{id} misses a foe {gap} cells away along a row: {events:?}"
            );
        }

        // ... and a foe 8 cells east is.
        let mut game = learnt(&["ember", "veil"]);
        let near = game.spawn_monster(at(player.x + 8, 5), &snail);
        settle(&mut game);
        let events = cast(&mut game, digits, None);
        assert!(
            touched(&events, near),
            "{id} reaches a foe 8 cells away: {events:?}"
        );
    }

    // Lance, aimed east: a foe 8 cells away is hit, one 9 cells away is not.
    let mut game = learnt(&["lance"]);
    let near = game.spawn_monster(at(player.x + 8, 5), &snail);
    settle(&mut game);
    let events = cast(&mut game, &[1, 5, 9, 6], Some(Direction::East));
    assert!(wounded(&events, near), "Lance reaches 8 cells: {events:?}");

    let mut game = learnt(&["lance"]);
    let beyond = game.spawn_monster(at(player.x + 9, 5), &snail);
    settle(&mut game);
    assert!(
        game.is_visible(at(player.x + 9, 5)),
        "the foe is in sight, only out of the aimed reach"
    );
    let events = cast(&mut game, &[1, 5, 9, 6], Some(Direction::East));
    assert!(
        !wounded(&events, beyond),
        "Lance stops at 8 cells: {events:?}"
    );
}
