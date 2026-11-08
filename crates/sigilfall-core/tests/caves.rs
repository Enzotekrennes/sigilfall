//! Caves: a floor of the cave kind is entirely joined, and the properties
//! checked for floors of rooms (`dungeon.rs`) hold on 1 000 seeds.
//!
//! Written against the public API of the core crate only, in the manner of
//! `dungeon.rs`: connectivity and distances are recomputed here with a plain
//! breadth-first search, never with the generator's own helpers, so a bug
//! shared by the generator and its checks cannot hide. The properties of a
//! floor of rooms that make sense for a cave are checked: the floor is 80 x
//! 40 and walled on every edge, every walkable cell is joined to every other
//! (no sealed pocket, no stray piece), the player starts on walkable ground,
//! there is exactly one staircase, it is reachable and it lies in the
//! farthest third of the floor from the start, both by distance and by rank.
//! A room-based check ("the start is inside a room") has no meaning in a cave
//! and is replaced by "the start is on open floor". On top of them: the size
//! promised by the design (900 open cells or more), the same seed giving the
//! same cave, and the cave being a different layout from the rooms of the
//! same seed.

use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

use sigilfall_core::content::FloorKind;
use sigilfall_core::generation::{FLOOR_HEIGHT, FLOOR_WIDTH, generate, generate_of_kind};
use sigilfall_core::geometry::Position;
use sigilfall_core::map::{Map, Tile};
use sigilfall_core::rng::Seed;

/// The reference seeds of the tests: both ends of the range, 42 and the
/// seed shown to players as an example.
const NAMED_SEEDS: [u64; 4] = [0, 42, 0x7F3A_91C2_0B5E_D4A8, u64::MAX];

/// Number of seeds of each of the two random batches.
const RANDOM_SEEDS: usize = 1000;

/// Fewest open cells the design promises a cave (decision D22).
const MIN_OPEN_CELLS: usize = 900;

/// The eight moves of the game.
const ALL_MOVES: [(i32, i32); 8] = [
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
];

/// The four orthogonal moves.
const ORTHOGONAL: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

/// A small xorshift64* generator, different from the game's own streams.
struct SeedSource(u64);

impl SeedSource {
    /// Returns the next pseudo-random 64-bit value.
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

/// `RANDOM_SEEDS` seeds from a fixed generator (reproducible) followed by
/// `RANDOM_SEEDS` seeds from the clock; a failure names its seed and depth.
fn random_seeds() -> Vec<u64> {
    let clock = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after 1970")
        .as_nanos();
    let clock = u64::try_from(clock & u128::from(u64::MAX)).unwrap() | 1;
    let mut fixed = SeedSource(0x9E37_79B9_7F4A_7C15);
    let mut clocked = SeedSource(clock);
    (0..RANDOM_SEEDS)
        .map(|_| fixed.next())
        .chain((0..RANDOM_SEEDS).map(|_| clocked.next()))
        .collect()
}

/// Whether `position` is open ground (floor, stairs, or the seal of the
/// bottom floor).
fn open(map: &Map, position: Position) -> bool {
    matches!(
        map.tile(position),
        Some(Tile::Floor | Tile::StairsDown | Tile::Seal)
    )
}

/// The tile of the way down of a floor at `depth`: the stairs, or from the
/// tenth floor, the bottom of the dungeon, the seal (decision D23).
fn way_down(depth: u32) -> Tile {
    if depth >= 10 {
        Tile::Seal
    } else {
        Tile::StairsDown
    }
}

/// Width and height of `map` as `i32`.
fn size(map: &Map) -> (i32, i32) {
    (
        i32::try_from(map.width()).unwrap(),
        i32::try_from(map.height()).unwrap(),
    )
}

/// Every position of `map` in reading order.
fn positions(map: &Map) -> Vec<Position> {
    let (width, height) = size(map);
    (0..height)
        .flat_map(|y| (0..width).map(move |x| Position::new(x, y)))
        .collect()
}

/// Breadth-first distances from `from` over open cells, in steps of
/// `moves`; unreachable cells are `None`.
fn distances(map: &Map, from: Position, moves: &[(i32, i32)]) -> Vec<Option<u32>> {
    let (width, height) = size(map);
    let index = |p: Position| usize::try_from(p.y * width + p.x).unwrap();
    let mut seen = vec![None; usize::try_from(width * height).unwrap()];
    let mut queue = VecDeque::new();
    seen[index(from)] = Some(0);
    queue.push_back(from);
    while let Some(here) = queue.pop_front() {
        let d = seen[index(here)].unwrap();
        for &(dx, dy) in moves {
            let next = Position::new(here.x + dx, here.y + dy);
            if open(map, next) && seen[index(next)].is_none() {
                seen[index(next)] = Some(d + 1);
                queue.push_back(next);
            }
        }
    }
    seen
}

/// Distance of `position` in a table built by [`distances`].
fn at(map: &Map, table: &[Option<u32>], position: Position) -> Option<u32> {
    let (width, _) = size(map);
    table[usize::try_from(position.y * width + position.x).unwrap()]
}

/// What was measured on one cave, to report afterwards.
struct Measure {
    /// Open cells of the cave.
    open_cells: usize,
    /// Steps from the start to the stairs.
    crossing: u32,
}

/// Every property of a joined cave, checked on the cave of `seed` at `depth`,
/// as an error message on failure.
fn check_cave(seed: u64, depth: u32) -> Result<Measure, String> {
    let shown = Seed::new(seed);
    let fail = |what: &str| Err(format!("seed {seed} ({shown}) depth {depth}: {what}"));
    let floor = generate_of_kind(shown, depth, FloorKind::Caves);
    let map = floor.map();

    if (map.width(), map.height()) != (FLOOR_WIDTH, FLOOR_HEIGHT) {
        return fail("cave is not 80 x 40");
    }
    let (width, height) = size(map);
    // Walled on every edge.
    for p in positions(map) {
        let on_edge = p.x == 0 || p.y == 0 || p.x == width - 1 || p.y == height - 1;
        if on_edge && map.tile(p) != Some(Tile::Wall) {
            return fail(&format!("edge cell {p:?} is not a wall"));
        }
    }
    let cells: Vec<Position> = positions(map)
        .into_iter()
        .filter(|&p| open(map, p))
        .collect();
    if cells.len() < MIN_OPEN_CELLS {
        return fail(&format!("only {} open cells", cells.len()));
    }

    // Start: open floor (not the stairs).
    let start = floor.start();
    if map.tile(start) != Some(Tile::Floor) {
        return fail(&format!("start {start:?} is not plain floor"));
    }

    // Entirely joined: every open cell reached from the start with the
    // eight moves, and the game's own step rule agrees with it.
    let from_start = distances(map, start, &ALL_MOVES);
    if let Some(&orphan) = cells.iter().find(|&&c| at(map, &from_start, c).is_none()) {
        return fail(&format!("open cell {orphan:?} is cut off from the start"));
    }
    // Moves are symmetric, but check the other direction anyway from the
    // stairs and from the last cell in reading order.
    for from in [floor.stairs(), *cells.last().unwrap()] {
        let table = distances(map, from, &ALL_MOVES);
        if let Some(&orphan) = cells.iter().find(|&&c| at(map, &table, c).is_none()) {
            return fail(&format!("{orphan:?} cannot be reached from {from:?}"));
        }
    }
    // The rule the game really applies (`Map::can_step`) joins them too.
    let reached = game_reach(map, start);
    if reached != cells.len() {
        return fail(&format!(
            "the game's step rule reaches {reached} of {} open cells",
            cells.len()
        ));
    }

    // Stairs (the seal on the bottom floor, which has no stairs): exactly
    // one, reachable, not on the start, in the farthest third by distance
    // and by rank, and at the greatest distance.
    let staircases: Vec<Position> = positions(map)
        .into_iter()
        .filter(|&p| matches!(map.tile(p), Some(Tile::StairsDown | Tile::Seal)))
        .collect();
    if map.tile(floor.stairs()) != Some(way_down(depth)) {
        return fail("the way down is not the stairs above floor 10, the seal on it");
    }
    if staircases != [floor.stairs()] {
        return fail(&format!("staircases found: {staircases:?}"));
    }
    let stairs = floor.stairs();
    if stairs == start {
        return fail("stairs are on the start cell");
    }
    let Some(crossing) = at(map, &from_start, stairs) else {
        return fail("stairs are unreachable from the start");
    };
    let reachable: Vec<u32> = from_start.iter().flatten().copied().collect();
    let farthest = *reachable.iter().max().unwrap();
    if 3 * crossing < 2 * farthest {
        return fail(&format!(
            "stairs at {crossing} steps, farthest cell at {farthest}: not in the farthest third by distance"
        ));
    }
    let farther = reachable.iter().filter(|&&d| d > crossing).count();
    if 3 * farther > reachable.len() {
        return fail(&format!(
            "{farther} of {} cells are farther than the stairs: not in the farthest third by rank",
            reachable.len()
        ));
    }
    if crossing != farthest {
        return fail(&format!(
            "stairs at {crossing} steps but the farthest cell is at {farthest}"
        ));
    }

    // The same seed, depth and kind give the same cave.
    if generate_of_kind(shown, depth, FloorKind::Caves) != floor {
        return fail("two generations differ");
    }
    Ok(Measure {
        open_cells: cells.len(),
        crossing,
    })
}

/// Number of open cells reachable from `start` with the game's own step
/// rule (`Map::can_step`).
fn game_reach(map: &Map, start: Position) -> usize {
    use sigilfall_core::geometry::Direction;
    let (width, height) = size(map);
    let index = |p: Position| usize::try_from(p.y * width + p.x).unwrap();
    let mut seen = vec![false; usize::try_from(width * height).unwrap()];
    let mut queue = VecDeque::from([start]);
    seen[index(start)] = true;
    let mut count = 0;
    while let Some(here) = queue.pop_front() {
        count += 1;
        for direction in Direction::ALL {
            if map.can_step(here, direction) {
                let next = here.step(direction);
                if !seen[index(next)] {
                    seen[index(next)] = true;
                    queue.push_back(next);
                }
            }
        }
    }
    count
}

/// Median of a sorted-in-place list.
fn median(values: &mut [u32]) -> u32 {
    values.sort_unstable();
    values[values.len() / 2]
}

/// The example seeds, at several depths.
#[test]
fn the_example_seeds_give_caves_entirely_joined_with_far_stairs() {
    for value in NAMED_SEEDS {
        for depth in [1, 3, 10, 11, 99] {
            check_cave(value, depth).unwrap();
        }
    }
}

/// 1 000 fixed seeds and 1 000 clock-drawn seeds, every property of a floor
/// of rooms that has a meaning for a cave, with the depth varied from 3 to 10
/// so the depth in the stream is exercised too.
#[test]
fn caves_are_entirely_joined_with_far_stairs_on_2000_seeds() {
    let mut open_cells = Vec::new();
    let mut crossings = Vec::new();
    let mut failures = Vec::new();
    for (i, seed) in random_seeds().into_iter().enumerate() {
        let depth = 3 + u32::try_from(i % 8).unwrap();
        match check_cave(seed, depth) {
            Ok(measure) => {
                open_cells.push(u32::try_from(measure.open_cells).unwrap());
                crossings.push(measure.crossing);
            }
            Err(message) => failures.push(message),
        }
    }
    println!(
        "caves over {} seeds: open cells min {} median {} max {}; crossing min {} median {} max {}",
        open_cells.len(),
        open_cells.iter().min().unwrap(),
        median(&mut open_cells.clone()),
        open_cells.iter().max().unwrap(),
        crossings.iter().min().unwrap(),
        median(&mut crossings.clone()),
        crossings.iter().max().unwrap(),
    );
    assert!(
        failures.is_empty(),
        "{} of {} caves failed, first ones:\n{}",
        failures.len(),
        2 * RANDOM_SEEDS,
        failures
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// The cave is not the rooms of the same seed under another name, and the
/// seeds do not all collapse to one cave.
#[test]
fn a_cave_is_not_the_rooms_of_its_seed_and_seeds_give_other_caves() {
    let mut caves = Vec::new();
    for value in NAMED_SEEDS {
        let seed = Seed::new(value);
        let cave = generate_of_kind(seed, 5, FloorKind::Caves);
        let rooms = generate_of_kind(seed, 5, FloorKind::Rooms);
        assert_ne!(cave.map(), rooms.map(), "seed {seed}");
        assert_eq!(rooms, generate(seed, 5), "`generate` stays rooms");
        caves.push(cave);
    }
    for (i, a) in caves.iter().enumerate() {
        for b in &caves[i + 1..] {
            assert_ne!(a.map(), b.map(), "two named seeds gave the same cave");
        }
    }
    // A floor does not depend on the floors generated before it.
    let direct = generate_of_kind(Seed::new(42), 7, FloorKind::Caves);
    let _ = generate_of_kind(Seed::new(42), 3, FloorKind::Caves);
    assert_eq!(
        generate_of_kind(Seed::new(42), 7, FloorKind::Caves),
        direct,
        "depth 7 depends on earlier floors"
    );
}

/// A cave is a natural layout: it has no room in the sense of `dungeon.rs` (a
/// block of 4 x 4 open cells is rare in a field of pillars) in the same
/// numbers as a floor of rooms, and it is much more open than a floor of
/// rooms. This guards the finder used for the other tests: the cave kind must
/// really differ in structure from the room kind.
#[test]
fn caves_are_structurally_different_from_rooms() {
    let mut cave_blocks = 0_usize;
    let mut room_blocks = 0_usize;
    let mut cave_orthogonal_orphans = 0_usize;
    for value in random_seeds().into_iter().take(100) {
        let seed = Seed::new(value);
        let cave = generate_of_kind(seed, 5, FloorKind::Caves);
        let rooms = generate_of_kind(seed, 5, FloorKind::Rooms);
        cave_blocks += open_blocks(cave.map());
        room_blocks += open_blocks(rooms.map());
        // Diagonal-only joins: cells that need a corner cut to be reached.
        let table = distances(cave.map(), cave.start(), &ORTHOGONAL);
        cave_orthogonal_orphans += positions(cave.map())
            .into_iter()
            .filter(|&p| open(cave.map(), p) && at(cave.map(), &table, p).is_none())
            .count();
    }
    println!(
        "4x4 open blocks over 100 seeds: caves {cave_blocks}, rooms {room_blocks}; \
         cave cells reached only by cutting a corner: {cave_orthogonal_orphans}"
    );
    assert!(cave_blocks != room_blocks);
}

/// Number of 4 x 4 blocks of open cells on `map`.
fn open_blocks(map: &Map) -> usize {
    let (width, height) = size(map);
    let mut count = 0;
    for y in 0..=(height - 4) {
        for x in 0..=(width - 4) {
            if (0..4).all(|dy| (0..4).all(|dx| open(map, Position::new(x + dx, y + dy)))) {
                count += 1;
            }
        }
    }
    count
}
