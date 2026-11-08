//! Acceptance tests of F13, "Grottes selon l'étage": which kind of floor each
//! depth gets (`content/floors.ron`), how the file is validated, and how
//! monsters, glyphs and items are placed in a cave.
//!
//! Written against the public API of the core crate only. The floor file is
//! read here with a small parser of its own, so the expected weights do not
//! come from the code under test; reachability and distances are recomputed
//! with a plain breadth-first search. The rules are written out (6 monsters
//! on the first floor then one more per floor up to 12, 2 glyphs, 3 items,
//! sight 16 cells along a row and 8 along a column) instead of read from the
//! production constants, except where a test says so.

use std::collections::{HashSet, VecDeque};

use sigilfall_core::content::{Content, ContentError, FloorKind};
use sigilfall_core::game::Game;
use sigilfall_core::generation::{
    FLOOR_HEIGHT, FLOOR_WIDTH, Floor, floor_kind, generate_of_kind, generate_with_monsters,
};
use sigilfall_core::geometry::Position;
use sigilfall_core::map::{Map, Tile};
use sigilfall_core::rng::Seed;

/// The floor file shipped with the game, as text.
const FLOORS: &str = include_str!("../content/floors.ron");
/// The other content files, for `Content::parse_with_floors`.
const MONSTERS: &str = include_str!("../content/monsters.ron");
/// Sigil file.
const SIGILS: &str = include_str!("../content/sigils.ron");
/// Item file.
const ITEMS: &str = include_str!("../content/items.ron");
/// Spawn file.
const SPAWNS: &str = include_str!("../content/spawns.ron");

/// Deepest floor the content is written for; deeper floors use its weights.
const MAX_DEPTH: u32 = 10;

/// Seeds drawn per depth to measure the share of each kind.
const DRAWS: u64 = 6000;

/// One rule of the floor file: the kind, the floors it covers, its weight.
#[derive(Debug, Clone, Copy)]
struct Rule {
    /// Name of the kind (`Rooms` or `Caves`).
    kind: &'static str,
    /// First floor.
    from: u32,
    /// Last floor, included.
    to: u32,
    /// Relative chance.
    weight: u32,
}

/// Reads the rules of `FLOORS`: every non-comment line that holds
/// `Floor(kind: X, from_depth: a, to_depth: b, weight: w)`.
fn shipped_rules() -> Vec<Rule> {
    let number = |line: &str, field: &str| -> u32 {
        let after = line.split(&format!("{field}:")).nth(1).expect(field);
        after
            .trim_start()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .expect(field)
    };
    FLOORS
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("Floor("))
        .map(|line| {
            let kind = if line.contains("kind: Rooms") {
                "Rooms"
            } else if line.contains("kind: Caves") {
                "Caves"
            } else {
                panic!("unknown kind in {line}")
            };
            Rule {
                kind,
                from: number(line, "from_depth"),
                to: number(line, "to_depth"),
                weight: number(line, "weight"),
            }
        })
        .collect()
}

/// The weight of `kind` on `depth` in `rules` (deeper than the last floor
/// counts as the last).
fn weight_of(rules: &[Rule], kind: &str, depth: u32) -> u32 {
    let depth = depth.clamp(1, MAX_DEPTH);
    rules
        .iter()
        .filter(|r| r.kind == kind && r.from <= depth && depth <= r.to)
        .map(|r| r.weight)
        .sum()
}

fn content() -> Content {
    Content::load().expect("the shipped content is valid")
}

/// The name of a kind as written in the file.
const fn name(kind: FloorKind) -> &'static str {
    match kind {
        FloorKind::Rooms => "Rooms",
        FloorKind::Caves => "Caves",
    }
}

// --- Kinds by depth -------------------------------------------------------

#[test]
fn the_shipped_file_has_the_rules_the_design_describes() {
    let rules = shipped_rules();
    assert_eq!(rules.len(), 6, "{rules:?}");
    for depth in 1..=2 {
        assert_eq!(weight_of(&rules, "Caves", depth), 0, "floor {depth}");
        assert!(weight_of(&rules, "Rooms", depth) > 0, "floor {depth}");
    }
    assert_eq!(weight_of(&rules, "Rooms", 10), 0);
    assert!(weight_of(&rules, "Caves", 10) > 0);
}

#[test]
fn floors_one_and_two_are_rooms_and_ten_and_deeper_are_caves_for_every_seed() {
    let content = content();
    for value in 0..2000_u64 {
        let seed = Seed::new(value.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        for depth in [1, 2] {
            assert_eq!(floor_kind(seed, depth, &content), FloorKind::Rooms);
        }
        for depth in [10, 11, 12, 99, u32::MAX] {
            assert_eq!(floor_kind(seed, depth, &content), FloorKind::Caves);
        }
    }
}

#[test]
fn the_content_reports_exactly_the_weights_of_the_file_floor_by_floor() {
    let rules = shipped_rules();
    let content = content();
    for depth in 1..=14 {
        let reported: Vec<(FloorKind, u32)> = content.floor_kind_weights(depth).collect();
        for kind in [FloorKind::Rooms, FloorKind::Caves] {
            let expected = weight_of(&rules, name(kind), depth);
            let found: u32 = reported
                .iter()
                .filter(|(k, _)| *k == kind)
                .map(|(_, w)| *w)
                .sum();
            assert_eq!(found, expected, "floor {depth}, {kind:?}");
        }
    }
}

/// The share of caves drawn on each floor follows the weights of the file:
/// within five standard deviations of a binomial draw.
#[test]
fn the_share_of_caves_on_each_depth_follows_the_weights_of_the_file() {
    let rules = shipped_rules();
    let content = content();
    for depth in 1..=12 {
        let rooms = f64::from(weight_of(&rules, "Rooms", depth));
        let caves = f64::from(weight_of(&rules, "Caves", depth));
        let expected = caves / (rooms + caves);
        let mut seen = 0_u64;
        for value in 0..DRAWS {
            // Spread the seeds over the whole range, not 0..DRAWS.
            let seed = Seed::new(value.wrapping_mul(0xD1B5_4A32_D192_ED03).wrapping_add(7));
            if floor_kind(seed, depth, &content) == FloorKind::Caves {
                seen += 1;
            }
        }
        #[allow(clippy::cast_precision_loss)]
        let share = seen as f64 / DRAWS as f64;
        #[allow(clippy::cast_precision_loss)]
        let sigma = (expected * (1.0 - expected) / DRAWS as f64).sqrt();
        println!("floor {depth}: caves {share:.3} (expected {expected:.3})");
        assert!(
            (share - expected).abs() <= 5.0 * sigma + 1e-9,
            "floor {depth}: {share:.3} caves, expected {expected:.3}"
        );
    }
}

/// The kind of a floor does not depend on the floors before it, and the
/// game really lays the floor out as the kind says.
#[test]
fn the_kind_is_a_pure_function_and_the_floor_matches_it() {
    let content = content();
    let mut caves = 0;
    let mut rooms = 0;
    for value in 0..300_u64 {
        let seed = Seed::new(value.wrapping_mul(0xA24B_AED4_963E_E407));
        for depth in [3, 5, 7, 9, 10] {
            let kind = floor_kind(seed, depth, &content);
            assert_eq!(floor_kind(seed, depth, &content), kind);
            let floor = generate_with_monsters(seed, depth, &content);
            let bare = generate_of_kind(seed, depth, kind);
            assert_eq!(floor.map(), bare.map(), "seed {seed} depth {depth}");
            assert_eq!(floor.start(), bare.start());
            assert_eq!(floor.stairs(), bare.stairs());
            match kind {
                FloorKind::Caves => caves += 1,
                FloorKind::Rooms => rooms += 1,
            }
        }
    }
    assert!(caves > 0 && rooms > 0, "caves {caves}, rooms {rooms}");
}

/// A content whose floor file says "caves everywhere" starts the game in a
/// cave: the file drives the game, not a constant.
#[test]
fn a_floor_file_that_asks_for_caves_everywhere_gives_a_game_in_a_cave() {
    let all_caves = "[Floor(kind: Caves, from_depth: 1, to_depth: 10, weight: 1)]";
    let content = Content::parse_with_floors(MONSTERS, SIGILS, ITEMS, SPAWNS, all_caves)
        .expect("a valid floor file");
    for depth in 1..=12 {
        assert_eq!(floor_kind(Seed::new(5), depth, &content), FloorKind::Caves);
    }
    for value in [0_u64, 42, 9940, u64::MAX] {
        let seed = Seed::new(value);
        let game = Game::new(seed, &content);
        let cave = generate_of_kind(seed, 1, FloorKind::Caves);
        let rooms = generate_of_kind(seed, 1, FloorKind::Rooms);
        assert_eq!(tiles(game.map()), tiles(cave.map()), "seed {seed}");
        assert_ne!(tiles(game.map()), tiles(rooms.map()), "seed {seed}");
        assert_eq!(game.player_actor().position(), cave.start());
    }
    // And the reverse: rooms everywhere.
    let all_rooms = "[Floor(kind: Rooms, from_depth: 1, to_depth: 10, weight: 1)]";
    let content = Content::parse_with_floors(MONSTERS, SIGILS, ITEMS, SPAWNS, all_rooms).unwrap();
    for depth in [1, 5, 10, 30] {
        assert_eq!(floor_kind(Seed::new(5), depth, &content), FloorKind::Rooms);
    }
}

/// Weights are weights: 1 against 3 gives about three caves in four.
#[test]
fn a_custom_weight_changes_the_share_drawn() {
    let floors = "[Floor(kind: Rooms, from_depth: 1, to_depth: 10, weight: 1),\n\
                   Floor(kind: Caves, from_depth: 1, to_depth: 10, weight: 3)]";
    let content = Content::parse_with_floors(MONSTERS, SIGILS, ITEMS, SPAWNS, floors).unwrap();
    let caves = (0..4000_u64)
        .filter(|&v| {
            floor_kind(
                Seed::new(v.wrapping_mul(0x9E37_79B9_7F4A_7C15)),
                4,
                &content,
            ) == FloorKind::Caves
        })
        .count();
    assert!((2800..=3200).contains(&caves), "{caves} caves of 4000");
}

fn tiles(map: &Map) -> Vec<Option<Tile>> {
    let (w, h) = (
        i32::try_from(map.width()).unwrap(),
        i32::try_from(map.height()).unwrap(),
    );
    (0..h)
        .flat_map(|y| (0..w).map(move |x| map.tile(Position::new(x, y))))
        .collect()
}

// --- Validation of floors.ron ----------------------------------------------

/// Parses `floors` with the shipped content around it.
fn parse(floors: &str) -> Result<Content, ContentError> {
    Content::parse_with_floors(MONSTERS, SIGILS, ITEMS, SPAWNS, floors)
}

/// `floors` must be refused with a message that names the file and every
/// word of `names`; returns the message.
fn refused(floors: &str, names: &[&str]) -> String {
    let error = match parse(floors) {
        Ok(_) => panic!("accepted: {floors}"),
        Err(error) => error,
    };
    let message = error.to_string();
    println!("{message}");
    assert!(message.contains("floors.ron"), "{message}");
    for word in names {
        assert!(message.contains(word), "`{word}` missing from: {message}");
    }
    assert!(!message.contains("Some("), "{message}");
    assert!(!message.contains("Err("), "{message}");
    message
}

#[test]
fn the_shipped_file_is_accepted() {
    assert!(parse(FLOORS).is_ok());
}

#[test]
fn a_missing_field_names_the_entry_and_the_field() {
    for (field, rule) in [
        ("weight", "Floor(kind: Caves, from_depth: 3, to_depth: 4)"),
        ("from_depth", "Floor(kind: Caves, to_depth: 4, weight: 1)"),
        ("to_depth", "Floor(kind: Caves, from_depth: 3, weight: 1)"),
        ("kind", "Floor(from_depth: 3, to_depth: 4, weight: 1)"),
    ] {
        let floors =
            format!("[Floor(kind: Rooms, from_depth: 1, to_depth: 10, weight: 1),\n{rule}]");
        let message = refused(&floors, &[field]);
        assert!(
            message.contains("entry 2") || message.contains("2"),
            "{message}"
        );
        if field != "kind" {
            assert!(message.contains("Caves"), "{message}");
        }
    }
}

#[test]
fn values_out_of_bounds_name_the_entry_and_the_field() {
    let rooms = "Floor(kind: Rooms, from_depth: 1, to_depth: 10, weight: 1)";
    for (rule, field) in [
        (
            "Floor(kind: Caves, from_depth: 0, to_depth: 4, weight: 1)",
            "from_depth",
        ),
        (
            "Floor(kind: Caves, from_depth: 11, to_depth: 11, weight: 1)",
            "from_depth",
        ),
        (
            "Floor(kind: Caves, from_depth: 3, to_depth: 11, weight: 1)",
            "to_depth",
        ),
        (
            "Floor(kind: Caves, from_depth: 6, to_depth: 5, weight: 1)",
            "to_depth",
        ),
        (
            "Floor(kind: Caves, from_depth: 3, to_depth: 4, weight: 0)",
            "weight",
        ),
        (
            "Floor(kind: Caves, from_depth: 3, to_depth: 4, weight: 100)",
            "weight",
        ),
        (
            "Floor(kind: Caves, from_depth: 3, to_depth: 4, weight: -1)",
            "weight",
        ),
    ] {
        let message = refused(&format!("[{rooms},\n{rule}]"), &[field, "Caves"]);
        assert!(message.contains("entry 2"), "{message}");
    }
}

#[test]
fn an_overlap_of_two_rules_of_one_kind_names_both() {
    let floors = "[Floor(kind: Rooms, from_depth: 1, to_depth: 5, weight: 1),\n\
                   Floor(kind: Rooms, from_depth: 5, to_depth: 6, weight: 1),\n\
                   Floor(kind: Caves, from_depth: 1, to_depth: 10, weight: 1)]";
    let message = refused(floors, &["Rooms", "from_depth", "overlap"]);
    assert!(
        message.contains("entry 2") && message.contains("entry 1"),
        "{message}"
    );
}

#[test]
fn a_floor_without_any_kind_is_named() {
    let message = refused(
        "[Floor(kind: Rooms, from_depth: 1, to_depth: 3, weight: 1),\n\
          Floor(kind: Caves, from_depth: 5, to_depth: 10, weight: 1)]",
        &["floor 4"],
    );
    assert!(message.contains("needs one"), "{message}");
    refused("[]", &["floor 1"]);
    refused(
        "[Floor(kind: Rooms, from_depth: 1, to_depth: 9, weight: 1)]",
        &["floor 10"],
    );
}

#[test]
fn syntax_faults_are_refused_with_the_file_and_a_place() {
    for text in [
        "",
        "Floor(kind: Rooms, from_depth: 1, to_depth: 10, weight: 1)",
        "[Floor(kind: Rooms, from_depth: 1, to_depth: 10, weight: 1)",
        "[Floor(kind: Mazes, from_depth: 1, to_depth: 10, weight: 1)]",
        "[Floor(kind: Rooms, from_depth: one, to_depth: 10, weight: 1)]",
        "[Floor(kind: Rooms, from_depth: 1, to_depth: 10, weight: 1.5)]",
        "[Floor(kind: Rooms, from_depth: 1, to_depth: 10, weight: 1, colour: 3)]",
        "[Floor(kind: Rooms, kind: Caves, from_depth: 1, to_depth: 10, weight: 1)]",
        "[Floor(kind: Rooms, from_depth: 1, to_depth: 10, weight: 99999999999999999999)]",
        "[Floor(kind: \"Rooms\", from_depth: 1, to_depth: 10, weight: 1)]",
        "{}",
        "\u{0}\u{1}\u{2}",
        "[Cave(kind: Caves, from_depth: 1, to_depth: 10, weight: 1)]",
    ] {
        let message = refused(text, &[]);
        assert!(!message.is_empty());
    }
}

#[test]
fn an_unknown_kind_names_the_kind_field() {
    refused(
        "[Floor(kind: Mazes, from_depth: 1, to_depth: 10, weight: 1)]",
        &["kind"],
    );
}

// --- Placement in a cave ---------------------------------------------------

/// Sight range of the player along a row, written out from the design.
const SIGHT_COLUMNS: f64 = 16.0;
/// Sight range of the player along a column, written out from the design.
const SIGHT_ROWS: f64 = 8.0;

/// Breadth-first reach from `from` over open cells with the eight moves.
fn reach(map: &Map, from: Position) -> HashSet<Position> {
    let mut seen = HashSet::from([from]);
    let mut queue = VecDeque::from([from]);
    while let Some(here) = queue.pop_front() {
        for dx in -1..=1 {
            for dy in -1..=1 {
                let next = Position::new(here.x + dx, here.y + dy);
                if matches!(map.tile(next), Some(Tile::Floor | Tile::StairsDown))
                    && seen.insert(next)
                {
                    queue.push_back(next);
                }
            }
        }
    }
    seen
}

/// Whether `b` is within the player's sight range of `a`, walls ignored:
/// its centre lies inside the ellipse whose half-axes are half a cell longer
/// than the range (no cell lies on its edge, so rounding cannot matter).
fn within_sight(a: Position, b: Position) -> bool {
    let dx = f64::from(a.x - b.x) / (SIGHT_COLUMNS + 0.5);
    let dy = f64::from(a.y - b.y) / (SIGHT_ROWS + 0.5);
    dx * dx + dy * dy < 1.0
}

/// Everything the placement of a cave must respect, for one floor.
fn check_placement(seed: Seed, depth: u32, floor: &Floor) -> Result<(), String> {
    let fail = |what: String| Err(format!("seed {seed} depth {depth}: {what}"));
    let map = floor.map();
    let reachable = reach(map, floor.start());
    let expected_monsters = (5 + usize::try_from(depth).unwrap()).min(12);
    if floor.monsters().len() != expected_monsters {
        return fail(format!(
            "{} monsters, expected {expected_monsters}",
            floor.monsters().len()
        ));
    }
    let mut used: HashSet<Position> = HashSet::from([floor.start(), floor.stairs()]);
    for monster in floor.monsters() {
        let p = monster.position();
        if map.tile(p) != Some(Tile::Floor) || !reachable.contains(&p) {
            return fail(format!("monster at {p:?} is not on reachable floor"));
        }
        if within_sight(p, floor.start()) {
            return fail(format!(
                "monster at {p:?} is within sight range of the start {:?}",
                floor.start()
            ));
        }
        if !used.insert(p) {
            return fail(format!("monster at {p:?} shares a cell"));
        }
    }
    if floor.glyphs().len() != 2 {
        return fail(format!("{} glyphs", floor.glyphs().len()));
    }
    let mut sigils = HashSet::new();
    for glyph in floor.glyphs() {
        let p = glyph.position();
        if map.tile(p) != Some(Tile::Floor) || !reachable.contains(&p) {
            return fail(format!("glyph at {p:?} is not on reachable floor"));
        }
        if !used.insert(p) {
            return fail(format!("glyph at {p:?} shares a cell"));
        }
        if !sigils.insert(glyph.sigil().id().to_owned()) {
            return fail("two glyphs teach the same sigil".to_owned());
        }
    }
    if floor.items().len() != 3 {
        return fail(format!("{} items", floor.items().len()));
    }
    for item in floor.items() {
        let p = item.position();
        if map.tile(p) != Some(Tile::Floor) || !reachable.contains(&p) {
            return fail(format!("item at {p:?} is not on reachable floor"));
        }
        if !used.insert(p) {
            return fail(format!("item at {p:?} shares a cell"));
        }
    }
    // The player sees no monster on arrival.
    let game = Game::on_floor(seed, floor.clone());
    if game.visible_monsters().count() != 0 {
        return fail("a monster is in view on arrival".to_owned());
    }
    if (map.width(), map.height()) != (FLOOR_WIDTH, FLOOR_HEIGHT) {
        return fail("not 80 x 40".to_owned());
    }
    Ok(())
}

#[test]
fn cave_floors_place_monsters_out_of_sight_and_items_and_glyphs_within_reach() {
    let content = content();
    let mut checked = 0_u32;
    let mut failures = Vec::new();
    for value in 0..400_u64 {
        let seed = Seed::new(value.wrapping_mul(0x2545_F491_4F6C_DD1D).wrapping_add(1));
        for depth in [3, 5, 7, 8, 9, 10, 12] {
            if floor_kind(seed, depth, &content) != FloorKind::Caves {
                continue;
            }
            let floor = generate_with_monsters(seed, depth, &content);
            checked += 1;
            if let Err(message) = check_placement(seed, depth, &floor) {
                failures.push(message);
            }
        }
    }
    println!("{checked} cave floors checked");
    assert!(checked > 1000, "only {checked} cave floors reached");
    assert!(
        failures.is_empty(),
        "{} failures, first:\n{}",
        failures.len(),
        failures
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// The same placement on the whole game: the game built from caves
/// everywhere shows no monster on the first screen, whatever the seed.
#[test]
fn a_game_that_starts_in_a_cave_shows_no_monster_on_arrival() {
    let floors = "[Floor(kind: Caves, from_depth: 1, to_depth: 10, weight: 1)]";
    let content = Content::parse_with_floors(MONSTERS, SIGILS, ITEMS, SPAWNS, floors).unwrap();
    for value in 0..300_u64 {
        let seed = Seed::new(value.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x5555);
        let game = Game::new(seed, &content);
        assert_eq!(game.visible_monsters().count(), 0, "seed {seed}");
        assert_eq!(game.actors().count(), 7, "player and six monsters");
    }
}
