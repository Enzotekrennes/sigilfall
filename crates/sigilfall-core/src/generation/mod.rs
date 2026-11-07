//! Dungeon floors built from the run's seed: layout, start and stairs.
//!
//! Exposes [`generate`], [`Floor`], the size of a floor ([`FLOOR_WIDTH`],
//! [`FLOOR_HEIGHT`]) and [`FIRST_DEPTH`]. A floor is a pure function of the
//! seed and the depth: every random choice is drawn from the stream
//! `Seed::stream(depth, Purpose::Generation)` in a fixed order, and nothing
//! reads the clock or iterates over a hash map. The same seed and depth
//! therefore give the same floor on every platform, whatever happened on
//! earlier floors.
//!
//! The layout itself comes from the `bsp` submodule (rooms and corridors).
//! Monsters, items and engraved glyphs are not placed yet.

mod bsp;

use rand::RngExt as _;
use rand_chacha::ChaCha8Rng;

use crate::geometry::Position;
use crate::map::{Map, Tile};
use crate::path::DijkstraMap;
use crate::rng::{Purpose, Seed};

/// Number of columns of a floor.
///
/// Decision D11 of the plan: floors are larger than the 58 × 16 map area of
/// an 80 × 24 terminal, and a camera follows the player.
pub const FLOOR_WIDTH: usize = 80;

/// Number of rows of a floor (decision D11, see [`FLOOR_WIDTH`]).
pub const FLOOR_HEIGHT: usize = 40;

/// Depth of the first floor of a run; each staircase leads one deeper.
pub const FIRST_DEPTH: u32 = 1;

/// A generated dungeon floor: its tiles, where the player arrives and where
/// the stairs down are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Floor {
    /// The tiles, with [`Tile::StairsDown`] at `stairs`.
    pub(crate) map: Map,
    /// Floor cell, inside a room, where the player arrives.
    pub(crate) start: Position,
    /// Cell of the stairs down.
    pub(crate) stairs: Position,
}

impl Floor {
    /// Returns the tiles of the floor.
    pub const fn map(&self) -> &Map {
        &self.map
    }

    /// Returns the cell where the player arrives: a floor cell of one of the
    /// rooms.
    pub const fn start(&self) -> Position {
        self.start
    }

    /// Returns the cell of the stairs down (see [`generate`] for how it is
    /// chosen).
    pub const fn stairs(&self) -> Position {
        self.stairs
    }
}

/// Builds floor `depth` of the run identified by `seed`.
///
/// The floor is [`FLOOR_WIDTH`] × [`FLOOR_HEIGHT`] cells of rooms joined by
/// corridors, enclosed by walls on every edge, and every walkable cell can
/// be reached from every other one. The player starts on a random cell of a
/// random room.
///
/// The stairs go on the walkable cell farthest from the start in steps (the
/// [`DijkstraMap`] of the start, ties broken in reading order). Placing them
/// at the greatest distance makes every floor a crossing; in particular the
/// stairs are always in the farthest third of the floor, the guarantee the
/// game design asks for.
///
/// Calling this twice with the same arguments returns equal floors. `depth`
/// is the floor number, [`FIRST_DEPTH`] for the first one; any value is
/// accepted.
pub fn generate(seed: Seed, depth: u32) -> Floor {
    let mut rng = seed.stream(depth, Purpose::Generation);
    let (mut map, rooms) = bsp::carve(FLOOR_WIDTH, FLOOR_HEIGHT, &mut rng);
    let start = choose_start(&rooms, &mut rng);
    let stairs = farthest_cell(&map, start);
    map.set_tile(stairs, Tile::StairsDown);
    Floor { map, start, stairs }
}

/// Picks a random room of `rooms`, then a random cell in it.
///
/// # Panics
///
/// Panics if `rooms` is empty; the layout always has at least one room.
fn choose_start(rooms: &[bsp::Rect], rng: &mut ChaCha8Rng) -> Position {
    assert!(!rooms.is_empty(), "a floor layout always has rooms");
    let room = rooms[rng.random_range(0..rooms.len())];
    room.random_cell(rng)
}

/// Returns the walkable cell of `map` farthest from `start` in steps.
///
/// # Panics
///
/// Panics if `start` is not walkable, which would be a bug in the layout.
fn farthest_cell(map: &Map, start: Position) -> Position {
    let (cell, _) = DijkstraMap::new(map, [start])
        .farthest()
        .expect("the start of a floor is walkable");
    cell
}

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;
    use proptest::prelude::*;

    use super::*;
    use crate::path::flood_fill;

    /// Seed of the floor frozen in the snapshot.
    const REFERENCE_SEED: Seed = Seed::new(42);

    /// Number of random seeds the floor properties are checked on.
    const PROPERTY_CASES: u32 = 1000;

    /// Every walkable cell of `map`, in reading order.
    fn walkable_cells(map: &Map) -> Vec<Position> {
        map.positions().filter(|&p| map.is_walkable(p)).collect()
    }

    /// Whether `position` lies on the outer edge of `map`.
    fn on_border(map: &Map, position: Position) -> bool {
        let last_x = i32::try_from(map.width()).unwrap() - 1;
        let last_y = i32::try_from(map.height()).unwrap() - 1;
        position.x == 0 || position.y == 0 || position.x == last_x || position.y == last_y
    }

    /// Draws `floor` as text rows: `#` wall, `.` floor, `>` stairs and `@`
    /// the start.
    fn draw(floor: &Floor) -> String {
        let map = floor.map();
        let mut text = String::new();
        for position in map.positions() {
            let glyph = if position == floor.start() {
                '@'
            } else {
                match map.tile(position) {
                    Some(Tile::Floor) => '.',
                    Some(Tile::StairsDown) => '>',
                    Some(Tile::Wall) | None => '#',
                }
            };
            text.push(glyph);
            if usize::try_from(position.x).unwrap() == map.width() - 1 {
                text.push('\n');
            }
        }
        text
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(PROPERTY_CASES))]

        /// The guarantees of every floor, on random seeds and depths.
        #[test]
        fn every_floor_is_connected_with_far_stairs_and_closed_edges(
            seed in any::<u64>(),
            depth in FIRST_DEPTH..=50,
        ) {
            let floor = generate(Seed::new(seed), depth);
            let map = floor.map();

            prop_assert_eq!(map.width(), FLOOR_WIDTH);
            prop_assert_eq!(map.height(), FLOOR_HEIGHT);
            prop_assert_eq!(map.tile(floor.start()), Some(Tile::Floor));
            prop_assert_eq!(map.tile(floor.stairs()), Some(Tile::StairsDown));

            // Every room and corridor is reachable from the start.
            prop_assert_eq!(flood_fill(map, floor.start()), walkable_cells(map));

            // The stairs are reachable and in the farthest third.
            let distances = DijkstraMap::new(map, [floor.start()]);
            let (_, farthest) = distances.farthest().unwrap();
            let stairs = distances.distance(floor.stairs());
            prop_assert!(stairs.is_some_and(|d| 3 * d >= 2 * farthest && d > 0));

            for position in map.positions().filter(|&p| on_border(map, p)) {
                prop_assert_eq!(map.tile(position), Some(Tile::Wall));
            }
        }

        /// Generation is a pure function of the seed and the depth.
        #[test]
        fn the_same_seed_and_depth_give_the_same_floor(
            seed in any::<u64>(),
            depth in FIRST_DEPTH..=50,
        ) {
            prop_assert_eq!(generate(Seed::new(seed), depth), generate(Seed::new(seed), depth));
        }
    }

    #[test]
    fn the_reference_floor_is_frozen() {
        // If this snapshot changes, every existing seed now gives another
        // dungeon: change the generator only on purpose, and review the new
        // floor before accepting it.
        assert_snapshot!(draw(&generate(REFERENCE_SEED, FIRST_DEPTH)));
    }

    #[test]
    fn the_stairs_are_the_farthest_cell_from_the_start() {
        for seed in 0..50 {
            let floor = generate(Seed::new(seed), FIRST_DEPTH);
            let distances = DijkstraMap::new(floor.map(), [floor.start()]);

            assert_eq!(
                distances.farthest(),
                Some((floor.stairs(), distances.distance(floor.stairs()).unwrap())),
                "seed {seed}"
            );
        }
    }

    #[test]
    fn exactly_one_staircase_is_placed() {
        let floor = generate(REFERENCE_SEED, FIRST_DEPTH);
        let stairs: Vec<Position> = floor
            .map()
            .positions()
            .filter(|&p| floor.map().tile(p) == Some(Tile::StairsDown))
            .collect();

        assert_eq!(stairs, [floor.stairs()]);
    }

    #[test]
    fn other_seeds_and_depths_give_other_floors() {
        let reference = generate(REFERENCE_SEED, FIRST_DEPTH);

        assert_ne!(generate(Seed::new(43), FIRST_DEPTH), reference);
        assert_ne!(generate(REFERENCE_SEED, FIRST_DEPTH + 1), reference);
    }
}
