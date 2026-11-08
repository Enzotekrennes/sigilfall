//! Dungeon floors built from the run's seed: layout, start, stairs and
//! monsters.
//!
//! Exposes [`generate`] (the layout alone), [`generate_with_monsters`],
//! [`Floor`], [`MonsterSpawn`], the size of a floor ([`FLOOR_WIDTH`],
//! [`FLOOR_HEIGHT`]), [`FIRST_DEPTH`] and [`MONSTERS_PER_FLOOR`]. A floor is
//! a pure function of the seed, the depth and the content: every random
//! choice is drawn from the stream `Seed::stream(depth, Purpose::Generation)`
//! in a fixed order, and nothing reads the clock or iterates over a hash map.
//! The same seed and depth therefore give the same floor on every platform,
//! whatever happened on earlier floors.
//!
//! The layout itself comes from the `bsp` submodule (rooms and corridors).
//! Monsters are drawn after the layout, from the same stream, so placing
//! them never changes the layout of a seed. Items and engraved glyphs are not
//! placed yet.

mod bsp;

use rand::RngExt as _;
use rand_chacha::ChaCha8Rng;

use crate::content::{Content, MonsterDef};
use crate::fov::{self, SIGHT_RADIUS};
use crate::geometry::Position;
use crate::map::{Map, Tile};
use crate::path::DijkstraMap;
use crate::rng::{Purpose, Seed};
use crate::schedule::ACTION_THRESHOLD;

/// Number of columns of a floor.
///
/// Decision D11 of the plan: floors are larger than the 58 × 16 map area of
/// an 80 × 24 terminal, and a camera follows the player.
pub const FLOOR_WIDTH: usize = 80;

/// Number of rows of a floor (decision D11, see [`FLOOR_WIDTH`]).
pub const FLOOR_HEIGHT: usize = 40;

/// Depth of the first floor of a run; each staircase leads one deeper.
pub const FIRST_DEPTH: u32 = 1;

/// Number of monsters placed on a floor by [`generate_with_monsters`].
///
/// A floor has 8 to 16 rooms, about 12 on average, and the start room stays
/// empty: six monsters leave roughly one room in two occupied, so the player
/// meets creatures regularly while crossing the floor and still finds quiet
/// rooms to recover in. Fewer are placed only if the rooms lack free cells,
/// which the shipped layouts never do.
pub const MONSTERS_PER_FLOOR: usize = 6;

/// A generated dungeon floor: its tiles, where the player arrives, where the
/// stairs down are and which monsters wait on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Floor {
    /// The tiles, with [`Tile::StairsDown`] at `stairs`.
    pub(crate) map: Map,
    /// Floor cell, inside a room, where the player arrives.
    pub(crate) start: Position,
    /// Cell of the stairs down.
    pub(crate) stairs: Position,
    /// Monsters to create on the floor, in the order they were drawn; empty
    /// for a floor built by [`generate`].
    pub(crate) monsters: Vec<MonsterSpawn>,
}

/// A monster to create when a floor is entered: where, which kind, and how
/// much energy it starts with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonsterSpawn {
    /// Floor cell the monster stands on.
    pub(crate) position: Position,
    /// Definition of the creature, from the floor's spawn table.
    pub(crate) definition: MonsterDef,
    /// Energy stored before the floor's first tick.
    pub(crate) energy: i32,
}

impl MonsterSpawn {
    /// Returns the floor cell the monster stands on: inside a room other
    /// than the start room, out of the player's sight range from the start.
    pub const fn position(&self) -> Position {
        self.position
    }

    /// Returns the definition of the creature, one of those
    /// `Content::monsters_at` allows on the floor's depth.
    pub const fn definition(&self) -> &MonsterDef {
        &self.definition
    }

    /// Returns the energy the monster starts with, from `0` to
    /// `ACTION_THRESHOLD - 1`, drawn uniformly from the generation stream.
    ///
    /// Starting every monster at zero would keep monsters of the same speed
    /// in step for the whole floor, all acting on the same ticks; a random
    /// head start spreads the turns of slow and fast creatures across ticks.
    /// Staying below the threshold means no monster is ready before time
    /// starts, and since the player is first in turn order, the player
    /// always acts first on a new floor.
    pub const fn energy(&self) -> i32 {
        self.energy
    }
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

    /// Returns the monsters to create on the floor, in the order they were
    /// drawn (see [`generate_with_monsters`]); empty for a floor built by
    /// [`generate`].
    pub fn monsters(&self) -> &[MonsterSpawn] {
        &self.monsters
    }
}

/// Builds floor `depth` of the run identified by `seed`.
///
/// The floor is [`FLOOR_WIDTH`] × [`FLOOR_HEIGHT`] cells of rooms joined by
/// corridors, enclosed by walls on every edge, and every walkable cell can
/// be reached from every other one. The number of rooms, from 8 to 16,
/// varies from floor to floor. The player starts on a random cell of a
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
/// accepted. No monster is placed: see [`generate_with_monsters`].
pub fn generate(seed: Seed, depth: u32) -> Floor {
    lay_out(&mut seed.stream(depth, Purpose::Generation)).floor
}

/// Builds floor `depth` of the run identified by `seed`, as [`generate`]
/// does, and places [`MONSTERS_PER_FLOOR`] monsters on it.
///
/// The map, start and stairs are exactly those of [`generate`] with the same
/// seed and depth. Then, from the same generation stream, each monster gets
/// in turn:
///
/// - a cell drawn uniformly among the free floor cells of the rooms, the
///   start room excluded, that lie out of [`SIGHT_RADIUS`] of the start
///   (whatever the walls), so the player sees no monster on arrival;
/// - a definition drawn uniformly among `content.monsters_at(depth)`;
/// - a starting energy (see [`MonsterSpawn::energy`]).
///
/// Two monsters never share a cell, and none stands on the stairs. If fewer
/// free cells than [`MONSTERS_PER_FLOOR`] exist, one monster is placed per
/// free cell. Calling this twice with the same arguments returns equal
/// floors.
pub fn generate_with_monsters(seed: Seed, depth: u32, content: &Content) -> Floor {
    let mut rng = seed.stream(depth, Purpose::Generation);
    let Layout {
        mut floor,
        rooms,
        start_room,
    } = lay_out(&mut rng);
    let cells = monster_cells(&floor, &rooms, start_room);
    let table: Vec<&MonsterDef> = content.monsters_at(depth).collect();
    floor.monsters = draw_monsters(cells, &table, &mut rng);
    floor
}

/// A floor without monsters, with the rooms it was carved from.
struct Layout {
    /// The floor: map, start and stairs, no monster.
    floor: Floor,
    /// Every room of the floor, in the order the layout produced them.
    rooms: Vec<bsp::Rect>,
    /// Index in `rooms` of the room holding the start.
    start_room: usize,
}

/// Carves the rooms and corridors, then picks the start and the stairs,
/// drawing from `rng`.
///
/// The stream is left right after the last draw of the layout, ready for
/// whatever is placed next.
fn lay_out(rng: &mut ChaCha8Rng) -> Layout {
    let (mut map, rooms) = bsp::carve(FLOOR_WIDTH, FLOOR_HEIGHT, rng);
    let (start_room, start) = choose_start(&rooms, rng);
    let stairs = farthest_cell(&map, start);
    map.set_tile(stairs, Tile::StairsDown);
    Layout {
        floor: Floor {
            map,
            start,
            stairs,
            monsters: Vec::new(),
        },
        rooms,
        start_room,
    }
}

/// Picks a random room of `rooms`, then a random cell in it, and returns the
/// index of the room with the cell.
///
/// # Panics
///
/// Panics if `rooms` is empty; the layout always has at least one room.
fn choose_start(rooms: &[bsp::Rect], rng: &mut ChaCha8Rng) -> (usize, Position) {
    assert!(!rooms.is_empty(), "a floor layout always has rooms");
    let index = rng.random_range(0..rooms.len());
    (index, rooms[index].random_cell(rng))
}

/// Returns the cells a monster may be placed on, room by room in layout
/// order and in reading order within a room: plain floor cells (not the
/// stairs) of every room but `start_room`, out of sight range of the start.
fn monster_cells(floor: &Floor, rooms: &[bsp::Rect], start_room: usize) -> Vec<Position> {
    rooms
        .iter()
        .enumerate()
        .filter(|&(index, _)| index != start_room)
        .flat_map(|(_, room)| room.cells())
        .filter(|&cell| {
            floor.map.tile(cell) == Some(Tile::Floor)
                && !fov::in_sight_range(floor.start, cell, SIGHT_RADIUS)
        })
        .collect()
}

/// Draws up to [`MONSTERS_PER_FLOOR`] monsters from `rng`: for each, a cell
/// taken out of `cells`, a definition of `table`, then a starting energy.
///
/// # Panics
///
/// Panics if `table` is empty while cells remain; `Content` guarantees at
/// least one monster for every depth.
fn draw_monsters(
    mut cells: Vec<Position>,
    table: &[&MonsterDef],
    rng: &mut ChaCha8Rng,
) -> Vec<MonsterSpawn> {
    let mut monsters = Vec::with_capacity(MONSTERS_PER_FLOOR);
    while monsters.len() < MONSTERS_PER_FLOOR && !cells.is_empty() {
        assert!(!table.is_empty(), "every depth has at least one monster");
        let position = cells.swap_remove(rng.random_range(0..cells.len()));
        let definition = table[rng.random_range(0..table.len())].clone();
        let energy = rng.random_range(0..ACTION_THRESHOLD);
        monsters.push(MonsterSpawn {
            position,
            definition,
            energy,
        });
    }
    monsters
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

    /// Number of seeds the monster placement rules are checked on.
    const SPAWN_SEEDS: u64 = 200;

    /// Deepest floor the monster placement rules are checked on; deep
    /// enough for every shipped monster to be allowed.
    const DEEPEST_CHECKED: u32 = 8;

    /// The content shipped with the game.
    fn content() -> Content {
        Content::load().expect("the shipped content is valid")
    }

    /// Whether `position` lies inside `room`.
    const fn inside(room: bsp::Rect, position: Position) -> bool {
        position.x >= room.x
            && position.x < room.x + room.width
            && position.y >= room.y
            && position.y < room.y + room.height
    }

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
    fn monsters_follow_the_placement_rules_on_200_seeds() {
        let content = content();
        for value in 0..SPAWN_SEEDS {
            let seed = Seed::new(value);
            let depth = FIRST_DEPTH + u32::try_from(value).unwrap() % DEEPEST_CHECKED;
            let floor = generate_with_monsters(seed, depth, &content);
            let context = format!("seed {value}, depth {depth}");

            // Deterministic, and the layout is that of `generate`.
            assert_eq!(
                floor,
                generate_with_monsters(seed, depth, &content),
                "{context}"
            );
            let bare = generate(seed, depth);
            assert_eq!(
                (&bare.map, bare.start, bare.stairs),
                (&floor.map, floor.start, floor.stairs),
                "{context}"
            );
            assert!(bare.monsters().is_empty(), "{context}");

            let layout = lay_out(&mut seed.stream(depth, Purpose::Generation));
            let start_room = layout.rooms[layout.start_room];
            assert!(inside(start_room, floor.start()), "{context}");
            let seen_at_start = fov::visible_cells(floor.map(), floor.start(), SIGHT_RADIUS);
            let allowed: Vec<&str> = content.monsters_at(depth).map(MonsterDef::id).collect();

            assert_eq!(floor.monsters().len(), MONSTERS_PER_FLOOR, "{context}");
            let mut cells = Vec::new();
            for spawn in floor.monsters() {
                let cell = spawn.position();
                assert_eq!(floor.map().tile(cell), Some(Tile::Floor), "{context}");
                assert!(
                    !inside(start_room, cell),
                    "{context}: {cell:?} in start room"
                );
                assert!(
                    layout.rooms.iter().any(|&room| inside(room, cell)),
                    "{context}: {cell:?} outside the rooms"
                );
                assert!(
                    !fov::in_sight_range(floor.start(), cell, SIGHT_RADIUS),
                    "{context}: {cell:?} within sight range"
                );
                assert!(!seen_at_start.contains(&cell), "{context}: {cell:?} seen");
                assert!(
                    allowed.contains(&spawn.definition().id()),
                    "{context}: {} not allowed",
                    spawn.definition().id()
                );
                assert!(spawn.definition().can_appear_at(depth), "{context}");
                assert!(
                    (0..ACTION_THRESHOLD).contains(&spawn.energy()),
                    "{context}: energy {}",
                    spawn.energy()
                );
                assert!(!cells.contains(&cell), "{context}: {cell:?} twice");
                cells.push(cell);
            }
        }
    }

    #[test]
    fn the_first_floor_only_holds_first_floor_monsters() {
        let content = content();
        for value in 0..SPAWN_SEEDS {
            let floor = generate_with_monsters(Seed::new(value), FIRST_DEPTH, &content);

            for spawn in floor.monsters() {
                assert_eq!(spawn.definition().min_depth(), FIRST_DEPTH, "seed {value}");
            }
        }
    }

    #[test]
    fn monsters_and_head_starts_vary_between_seeds() {
        let content = content();
        let depth = DEEPEST_CHECKED;
        let spawns: Vec<MonsterSpawn> = (0..SPAWN_SEEDS)
            .flat_map(|value| generate_with_monsters(Seed::new(value), depth, &content).monsters)
            .collect();

        let mut kinds: Vec<&str> = spawns.iter().map(|s| s.definition().id()).collect();
        kinds.sort_unstable();
        kinds.dedup();
        assert_eq!(
            kinds.len(),
            content.monsters_at(depth).count(),
            "every kind appears"
        );

        let mut energies: Vec<i32> = spawns.iter().map(MonsterSpawn::energy).collect();
        energies.sort_unstable();
        energies.dedup();
        assert!(energies.len() > 50, "head starts are spread: {energies:?}");

        let first = generate_with_monsters(Seed::new(0), depth, &content);
        let second = generate_with_monsters(Seed::new(1), depth, &content);
        assert_ne!(first.monsters(), second.monsters());
    }

    #[test]
    fn rooms_short_of_free_cells_get_fewer_monsters() {
        let content = content();
        let table: Vec<&MonsterDef> = content.monsters_at(FIRST_DEPTH).collect();
        let cells = vec![Position::new(3, 3), Position::new(4, 3)];
        let mut rng = Seed::new(0).stream(FIRST_DEPTH, Purpose::Generation);

        let monsters = draw_monsters(cells.clone(), &table, &mut rng);

        let mut placed: Vec<Position> = monsters.iter().map(MonsterSpawn::position).collect();
        placed.sort_unstable_by_key(|p| (p.y, p.x));
        assert_eq!(placed, cells);
    }

    #[test]
    fn other_seeds_and_depths_give_other_floors() {
        let reference = generate(REFERENCE_SEED, FIRST_DEPTH);

        assert_ne!(generate(Seed::new(43), FIRST_DEPTH), reference);
        assert_ne!(generate(REFERENCE_SEED, FIRST_DEPTH + 1), reference);
    }
}
