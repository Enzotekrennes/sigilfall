//! Acceptance tests of F2, "Donjon généré par graine", written against the
//! public API of the core crate only.
//!
//! They deliberately do not reuse the generator's own helpers (the BSP room
//! list, `flood_fill`, `DijkstraMap`): the rooms are rediscovered from the
//! tiles alone, and connectivity and distances are recomputed here with a
//! plain breadth-first search. A bug shared by the generator and its own
//! checking code therefore cannot hide.
//!
//! A "room" is found as a cluster of 4 x 4 blocks of floor, since the
//! generator promises rooms of at least 4 x 4 cells and corridors one cell
//! wide.

use std::collections::{HashSet, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

use sigilfall_core::content::Content;
use sigilfall_core::game::Game;
use sigilfall_core::generation::{FIRST_DEPTH, FLOOR_HEIGHT, FLOOR_WIDTH, generate};
use sigilfall_core::geometry::Position;
use sigilfall_core::map::{Map, Tile};
use sigilfall_core::rng::Seed;

/// Seeds named by the acceptance brief: both ends of the range, the seed of
/// scenario 1 and the seed shown as an example to players.
const NAMED_SEEDS: [u64; 4] = [0, 42, 0x7F3A_91C2_0B5E_D4A8, u64::MAX];

/// Number of random seeds scenario 2 is checked on.
const RANDOM_SEEDS: usize = 1000;

/// Side of the block of floor cells that marks a room.
const ROOM_SIDE: i32 = 4;

/// Steps of a walk: the eight moves of the game, or only the four
/// orthogonal ones.
const ORTHOGONAL: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

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

/// A small xorshift64* generator, different from the game's own streams, so
/// the seeds under test are not produced by the code under test.
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
/// `RANDOM_SEEDS` seeds from the clock (a new draw on every run). A failure
/// message always names the seed, so a clock-drawn failure can be replayed
/// with `--seed`.
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

/// Whether `position` is a floor-like cell (floor or stairs).
fn open(map: &Map, position: Position) -> bool {
    matches!(map.tile(position), Some(Tile::Floor | Tile::StairsDown))
}

/// Every position of `map` in reading order; the map's own iterator is not
/// public.
fn positions(map: &Map) -> impl Iterator<Item = Position> {
    let (width, height) = size(map);
    (0..height).flat_map(move |y| (0..width).map(move |x| Position::new(x, y)))
}

/// Every tile of `map`, in reading order.
fn tiles(map: &Map) -> Vec<Option<Tile>> {
    positions(map).map(|p| map.tile(p)).collect()
}

/// Every open cell of `map`.
fn open_cells(map: &Map) -> Vec<Position> {
    positions(map).filter(|&p| open(map, p)).collect()
}

/// Width and height of `map` as `i32`.
fn size(map: &Map) -> (i32, i32) {
    (
        i32::try_from(map.width()).unwrap(),
        i32::try_from(map.height()).unwrap(),
    )
}

/// Breadth-first distances from `from`, in steps of `moves`, to every open
/// cell it can reach. Unreachable cells are absent.
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

/// Whether the 4 x 4 block whose top-left cell is `corner` is all open.
fn block_is_open(map: &Map, corner: Position) -> bool {
    (0..ROOM_SIDE)
        .all(|dy| (0..ROOM_SIDE).all(|dx| open(map, Position::new(corner.x + dx, corner.y + dy))))
}

/// The rooms of `map`, rediscovered from its tiles: each room is the set of
/// cells covered by overlapping 4 x 4 blocks of open cells. Rooms are
/// returned in reading order of their first cell.
fn rooms(map: &Map) -> Vec<Vec<Position>> {
    let (width, height) = size(map);
    let mut covered: HashSet<Position> = HashSet::new();
    for y in 0..=(height - ROOM_SIDE) {
        for x in 0..=(width - ROOM_SIDE) {
            let corner = Position::new(x, y);
            if block_is_open(map, corner) {
                for dy in 0..ROOM_SIDE {
                    for dx in 0..ROOM_SIDE {
                        covered.insert(Position::new(x + dx, y + dy));
                    }
                }
            }
        }
    }
    let mut left: Vec<Position> = positions(map).filter(|p| covered.contains(p)).collect();
    left.reverse();
    let mut rooms = Vec::new();
    let mut taken: HashSet<Position> = HashSet::new();
    while let Some(first) = left.pop() {
        if taken.contains(&first) {
            continue;
        }
        let mut room = vec![first];
        let mut queue = VecDeque::from([first]);
        taken.insert(first);
        while let Some(here) = queue.pop_front() {
            for &(dx, dy) in &ORTHOGONAL {
                let next = Position::new(here.x + dx, here.y + dy);
                if covered.contains(&next) && taken.insert(next) {
                    room.push(next);
                    queue.push_back(next);
                }
            }
        }
        rooms.push(room);
    }
    rooms
}

/// Every check of scenario 2 on one floor, as an error message on failure.
fn check_floor(seed: u64) -> Result<(), String> {
    let floor = generate(Seed::new(seed), FIRST_DEPTH);
    let map = floor.map();
    let fail = |what: &str| Err(format!("seed {seed} ({}): {what}", Seed::new(seed)));

    if (map.width(), map.height()) != (FLOOR_WIDTH, FLOOR_HEIGHT) {
        return fail("floor is not 80 x 40");
    }
    let found = rooms(map);
    if found.len() < 2 {
        return fail(&format!("only {} room(s) found", found.len()));
    }

    // Rooms: every room is joined to every other one, walking orthogonally
    // (the strictest reading: no corner cutting needed).
    let from_first = distances(map, found[0][0], &ORTHOGONAL);
    for (number, room) in found.iter().enumerate() {
        if room
            .iter()
            .any(|&cell| at(map, &from_first, cell).is_none())
        {
            return fail(&format!(
                "room {number} is not joined to room 0 by orthogonal steps"
            ));
        }
    }
    // Pairwise, in both directions, with the game's eight moves.
    for (a, room_a) in found.iter().enumerate() {
        let table = distances(map, room_a[0], &ALL_MOVES);
        for (b, room_b) in found.iter().enumerate() {
            if room_b.iter().any(|&cell| at(map, &table, cell).is_none()) {
                return fail(&format!("room {b} cannot be reached from room {a}"));
            }
        }
    }
    // No stray corridor piece: all open cells are in the one component.
    if let Some(&orphan) = open_cells(map)
        .iter()
        .find(|&&cell| at(map, &from_first, cell).is_none())
    {
        return fail(&format!("open cell {orphan:?} is cut off from the rooms"));
    }

    // Start: an open cell inside a room.
    let start = floor.start();
    if !found.iter().any(|room| room.contains(&start)) {
        return fail(&format!("start {start:?} is not inside a room"));
    }

    // Stairs: exactly one, reachable, in the farthest third.
    let staircases: Vec<Position> = positions(map)
        .filter(|&p| map.tile(p) == Some(Tile::StairsDown))
        .collect();
    if staircases != [floor.stairs()] {
        return fail(&format!("staircases found: {staircases:?}"));
    }
    let stairs = floor.stairs();
    let from_start = distances(map, start, &ALL_MOVES);
    let Some(stairs_distance) = at(map, &from_start, stairs) else {
        return fail("stairs are unreachable from the start");
    };
    if stairs == start {
        return fail("stairs are on the start cell");
    }
    let reachable: Vec<u32> = from_start.iter().flatten().copied().collect();
    let farthest = *reachable.iter().max().unwrap();
    // Reading 1: distance in the top third of the range of distances.
    if 3 * stairs_distance < 2 * farthest {
        return fail(&format!(
            "stairs at {stairs_distance} steps, farthest cell at {farthest}: not in the farthest third by distance"
        ));
    }
    // Reading 2: at most a third of the reachable cells are farther away.
    let farther = reachable.iter().filter(|&&d| d > stairs_distance).count();
    if 3 * farther > reachable.len() {
        return fail(&format!(
            "{farther} of {} cells are farther than the stairs: not in the farthest third by rank",
            reachable.len()
        ));
    }
    Ok(())
}

/// Scenario 1: the same seed gives the same dungeon, in the map, the start
/// and the game built on it, for the named seeds and for the brief's seed 42
/// started twice.
#[test]
fn scenario_1_the_same_seed_gives_identical_dungeons() {
    for value in NAMED_SEEDS {
        let seed = Seed::new(value);
        let (first, second) = (generate(seed, FIRST_DEPTH), generate(seed, FIRST_DEPTH));
        assert_eq!(first, second, "seed {seed}");

        let content = Content::load().expect("the shipped content is valid");
        let (game_a, game_b) = (Game::new(seed, &content), Game::new(seed, &content));
        assert_eq!(game_a.map(), game_b.map(), "seed {seed}");
        // The game's map also holds what the player has seen, so only the
        // tiles are compared with the generated floor.
        assert_eq!(tiles(game_a.map()), tiles(first.map()), "seed {seed}");
        assert_eq!(
            game_a.player_actor().position(),
            game_b.player_actor().position(),
            "seed {seed}"
        );
        assert_eq!(
            game_a.player_actor().position(),
            first.start(),
            "seed {seed}"
        );
    }
}

/// Scenario 1, other side: different seeds and depths do not all collapse
/// to one dungeon, and a floor does not depend on the floors generated
/// before it.
#[test]
fn scenario_1_other_seeds_give_other_dungeons_and_order_does_not_matter() {
    let floors: Vec<_> = NAMED_SEEDS
        .iter()
        .map(|&v| generate(Seed::new(v), FIRST_DEPTH))
        .collect();
    for (i, a) in floors.iter().enumerate() {
        for b in &floors[i + 1..] {
            assert_ne!(a.map(), b.map(), "two named seeds gave the same floor");
        }
    }
    let direct = generate(Seed::new(42), 3);
    let _ = generate(Seed::new(42), 1);
    let _ = generate(Seed::new(42), 2);
    assert_eq!(
        generate(Seed::new(42), 3),
        direct,
        "depth 3 depends on earlier floors"
    );
}

/// Scenario 2 on the named seeds.
#[test]
fn scenario_2_named_seeds() {
    for value in NAMED_SEEDS {
        check_floor(value).unwrap();
    }
}

/// Scenario 2: 1 000 fixed seeds and 1 000 clock-drawn seeds, checked in
/// terms of rooms.
#[test]
fn scenario_2_rooms_are_joined_and_stairs_are_far_on_random_seeds() {
    let failures: Vec<String> = random_seeds()
        .into_iter()
        .filter_map(|seed| check_floor(seed).err())
        .collect();
    assert!(
        failures.is_empty(),
        "{} of {} floors failed, first ones:\n{}",
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

/// Seeds survive the trip "shown in game, typed back after `--seed`": the
/// displayed form parses to the same seed and gives the same floor, for the
/// named seeds and 1 000 random ones. Decimal and upper/lower-case hex of
/// the same value are the same seed.
#[test]
fn the_displayed_seed_can_be_typed_back_and_gives_the_same_dungeon() {
    let mut values: Vec<u64> = NAMED_SEEDS.to_vec();
    values.extend(random_seeds().into_iter().take(RANDOM_SEEDS));
    for value in values {
        let seed = Seed::new(value);
        let shown = seed.to_string();
        assert_eq!(shown.len(), 19, "{shown}");
        let parsed: Seed = shown
            .parse()
            .unwrap_or_else(|_| panic!("{shown} not accepted"));
        assert_eq!(parsed, seed, "{shown}");
        assert_eq!(shown.to_lowercase().parse::<Seed>(), Ok(seed));
        assert_eq!(value.to_string().parse::<Seed>(), Ok(seed));
    }
    let typed: Seed = "7F3A-91C2-0B5E-D4A8".parse().unwrap();
    assert_eq!(typed.value(), 0x7F3A_91C2_0B5E_D4A8);
    assert_eq!(
        generate(typed, FIRST_DEPTH),
        generate(Seed::new(0x7F3A_91C2_0B5E_D4A8), FIRST_DEPTH)
    );
    assert_eq!(Seed::new(42).to_string(), "0000-0000-0000-002A");
    assert_eq!(Seed::new(u64::MAX).to_string(), "FFFF-FFFF-FFFF-FFFF");
}

/// Scenario 3, at the parser level: text that is not a seed is refused
/// with a message that names both accepted forms.
#[test]
fn scenario_3_bad_seed_texts_are_refused_with_the_expected_format() {
    for text in [
        "abc",
        "",
        " 42",
        "42 ",
        "-1",
        "4.2",
        "0x2A",
        "18446744073709551616",
        "7F3A-91C2-0B5E",
        "7F3A-91C2-0B5E-D4A8-0000",
        "7F3A-91C2-0B5E-D4A",
        "7F3A-91C2-0B5E-D4AG",
        "7F3A91C20B5ED4A8",
        "٤٢",
    ] {
        let error = text.parse::<Seed>().expect_err(text);
        let message = error.to_string();
        assert!(message.contains("such as 42"), "{text:?}: {message}");
        assert!(
            message.contains("7F3A-91C2-0B5E-D4A8"),
            "{text:?}: {message}"
        );
    }
}

/// The room finder sees what the design promises: between 8 and 16 rooms
/// per floor, each at least 4 x 4. If the finder were blind (no rooms, or
/// everything merged), the connectivity checks above would prove nothing.
#[test]
fn the_room_finder_sees_eight_to_sixteen_rooms_per_floor() {
    let mut counts = std::collections::BTreeMap::new();
    for seed in random_seeds().into_iter().take(RANDOM_SEEDS) {
        let floor = generate(Seed::new(seed), FIRST_DEPTH);
        let found = rooms(floor.map());
        *counts.entry(found.len()).or_insert(0_u32) += 1;
        assert!(
            found.iter().all(|room| room.len() >= 16),
            "seed {seed}: a room smaller than 4 x 4"
        );
    }
    println!("rooms found per floor over {RANDOM_SEEDS} seeds: {counts:?}");
    let (&fewest, _) = counts.iter().next().unwrap();
    let (&most, _) = counts.iter().next_back().unwrap();
    assert!((8..=16).contains(&fewest) && most <= 16, "{counts:?}");
}
