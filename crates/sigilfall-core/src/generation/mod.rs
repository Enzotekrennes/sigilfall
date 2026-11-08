//! Dungeon floors built from the run's seed: layout, start, stairs,
//! monsters, engraved glyphs and items.
//!
//! Exposes [`generate`] (the layout alone), [`generate_with_monsters`]
//! (with monsters, glyphs and items), [`Floor`], [`MonsterSpawn`],
//! [`Engraving`], [`ItemSpawn`], the size of a floor ([`FLOOR_WIDTH`],
//! [`FLOOR_HEIGHT`]), [`FIRST_DEPTH`], [`MONSTERS_PER_FLOOR`],
//! [`GLYPHS_PER_FLOOR`] and [`ITEMS_PER_FLOOR`]. A floor is
//! a pure function of the seed, the depth and the content: every random
//! choice is drawn from the stream `Seed::stream(depth, Purpose::Generation)`
//! in a fixed order, and nothing reads the clock or iterates over a hash map.
//! The same seed and depth therefore give the same floor on every platform,
//! whatever happened on earlier floors.
//!
//! The layout itself comes from the `bsp` submodule (rooms and corridors).
//! Monsters are drawn after the layout, engraved glyphs after the monsters,
//! and items after the glyphs, all from the same stream, so placing what
//! comes later never changes the layout, the monsters or the glyphs of a
//! seed.

mod bsp;

use rand::RngExt as _;
use rand_chacha::ChaCha8Rng;

use crate::content::{Content, ItemDef, MonsterDef, SigilDef};
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

/// Number of engraved glyphs placed on a floor by
/// [`generate_with_monsters`].
///
/// Two: the first floors offer two sigils each (see `content/sigils.ron`),
/// so a careful player leaves the first floor with both and finds the next
/// ones as they become available deeper. More would hand out the whole
/// grimoire in a few floors; fewer would leave a floor without discovery.
/// Fewer are placed only when fewer sigils are available at that depth.
pub const GLYPHS_PER_FLOOR: usize = 2;

/// Number of items placed on a floor by [`generate_with_monsters`].
///
/// Three: about one room in four holds an item, so exploring pays off on
/// every floor, while a player who drinks every vial still runs short over
/// a few floors of fights. Fewer are placed only when the rooms lack free
/// cells or no item can be found that deep.
pub const ITEMS_PER_FLOOR: usize = 3;

/// A generated dungeon floor: its tiles, where the player arrives, where the
/// stairs down are, which monsters wait on it and what lies on it.
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
    /// Glyphs engraved on the floor, in the order they were drawn; empty for
    /// a floor built by [`generate`].
    pub(crate) glyphs: Vec<Engraving>,
    /// Items lying on the floor, in the order they were drawn; empty for a
    /// floor built by [`generate`].
    pub(crate) items: Vec<ItemSpawn>,
}

/// An item lying on a new floor: where, and which kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemSpawn {
    /// Floor cell the item lies on.
    pub(crate) position: Position,
    /// Definition of the item.
    pub(crate) definition: ItemDef,
}

impl ItemSpawn {
    /// Returns the floor cell the item lies on: a plain floor cell of a room
    /// that the player can walk to from the start.
    pub const fn position(&self) -> Position {
        self.position
    }

    /// Returns the definition of the item, one of those `Content::items_at`
    /// allows on the floor's depth.
    pub const fn definition(&self) -> &ItemDef {
        &self.definition
    }
}

/// A glyph engraved in the floor, which teaches its sigil to the player who
/// reads it (see `Game::engraving_at`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Engraving {
    /// Floor cell the glyph is carved in.
    pub(crate) position: Position,
    /// The sigil it teaches.
    pub(crate) sigil: SigilDef,
}

impl Engraving {
    /// Returns a glyph teaching `sigil`, carved at `position`.
    pub(crate) const fn new(position: Position, sigil: SigilDef) -> Self {
        Self { position, sigil }
    }

    /// Returns the cell the glyph is carved in.
    pub const fn position(&self) -> Position {
        self.position
    }

    /// Returns the sigil the glyph teaches.
    pub const fn sigil(&self) -> &SigilDef {
        &self.sigil
    }
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

    /// Returns the glyphs engraved on the floor, in the order they were
    /// drawn (see [`generate_with_monsters`]); empty for a floor built by
    /// [`generate`].
    pub fn glyphs(&self) -> &[Engraving] {
        &self.glyphs
    }

    /// Returns the items lying on the floor, in the order they were drawn
    /// (see [`generate_with_monsters`]); empty for a floor built by
    /// [`generate`].
    pub fn items(&self) -> &[ItemSpawn] {
        &self.items
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
/// accepted. No monster, glyph nor item is placed: see
/// [`generate_with_monsters`].
pub fn generate(seed: Seed, depth: u32) -> Floor {
    lay_out(&mut seed.stream(depth, Purpose::Generation)).floor
}

/// Builds floor `depth` of the run identified by `seed`, as [`generate`]
/// does, and places [`MONSTERS_PER_FLOOR`] monsters, [`GLYPHS_PER_FLOOR`]
/// engraved glyphs and [`ITEMS_PER_FLOOR`] items on it.
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
/// free cell.
///
/// Then, still from the same stream, each glyph gets in turn a cell drawn
/// uniformly among the plain floor cells of every room (the start room
/// included) that hold neither the start, the stairs, a monster nor
/// another glyph, and a sigil drawn uniformly among the sigils of
/// `content` whose glyph can appear at `depth` (`SigilDef::can_appear_at`)
/// and not yet engraved on this floor. Every glyph thus lies on floor the
/// player can walk to and teaches a different sigil; fewer glyphs are
/// placed when fewer such sigils or cells exist, none when no sigil is
/// available that deep.
///
/// Last, still from the same stream, each item gets in turn a cell drawn
/// uniformly among the plain floor cells of every room (the start room
/// included) that hold neither the start, the stairs, a monster, a glyph
/// nor another item, and a definition drawn uniformly among
/// `content.items_at(depth)`; the same kind may be drawn twice. Every item
/// thus lies on floor the player can walk to, one per cell; fewer are
/// placed when fewer cells exist, none when no item is available that
/// deep. The layout, monsters and glyphs are exactly those of the same
/// floor with no item to place.
///
/// Calling this twice with the same arguments returns equal floors.
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
    let cells = glyph_cells(&floor, &rooms);
    let sigils: Vec<&SigilDef> = content
        .sigils()
        .iter()
        .filter(|sigil| sigil.can_appear_at(depth))
        .collect();
    floor.glyphs = draw_glyphs(cells, sigils, &mut rng);
    let cells = item_cells(&floor, &rooms);
    let table: Vec<&ItemDef> = content.items_at(depth).collect();
    floor.items = draw_items(cells, &table, &mut rng);
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
            glyphs: Vec::new(),
            items: Vec::new(),
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

/// Returns the cells a glyph may be engraved on, room by room in layout
/// order and in reading order within a room: plain floor cells (not the
/// stairs) of every room that hold neither the start nor a monster of
/// `floor`.
fn glyph_cells(floor: &Floor, rooms: &[bsp::Rect]) -> Vec<Position> {
    rooms
        .iter()
        .flat_map(|room| room.cells())
        .filter(|&cell| {
            floor.map.tile(cell) == Some(Tile::Floor)
                && cell != floor.start
                && floor.monsters.iter().all(|spawn| spawn.position != cell)
        })
        .collect()
}

/// Draws up to [`GLYPHS_PER_FLOOR`] glyphs from `rng`: for each, a cell
/// taken out of `cells`, then a sigil taken out of `sigils`, so no cell and
/// no sigil is used twice. Stops early when either list runs out.
fn draw_glyphs(
    mut cells: Vec<Position>,
    mut sigils: Vec<&SigilDef>,
    rng: &mut ChaCha8Rng,
) -> Vec<Engraving> {
    let mut glyphs = Vec::with_capacity(GLYPHS_PER_FLOOR);
    while glyphs.len() < GLYPHS_PER_FLOOR && !cells.is_empty() && !sigils.is_empty() {
        let position = cells.swap_remove(rng.random_range(0..cells.len()));
        let sigil = sigils.swap_remove(rng.random_range(0..sigils.len()));
        glyphs.push(Engraving::new(position, sigil.clone()));
    }
    glyphs
}

/// Returns the cells an item may lie on, room by room in layout order and in
/// reading order within a room: the cells a glyph may be engraved on (see
/// [`glyph_cells`]) that hold no glyph of `floor`.
fn item_cells(floor: &Floor, rooms: &[bsp::Rect]) -> Vec<Position> {
    glyph_cells(floor, rooms)
        .into_iter()
        .filter(|&cell| floor.glyphs.iter().all(|glyph| glyph.position != cell))
        .collect()
}

/// Draws up to [`ITEMS_PER_FLOOR`] items from `rng`: for each, a cell taken
/// out of `cells`, then a definition of `table`, so no cell is used twice.
/// Draws nothing when `table` is empty, and stops early when the cells run
/// out.
fn draw_items(
    mut cells: Vec<Position>,
    table: &[&ItemDef],
    rng: &mut ChaCha8Rng,
) -> Vec<ItemSpawn> {
    let mut items = Vec::with_capacity(ITEMS_PER_FLOOR);
    while items.len() < ITEMS_PER_FLOOR && !cells.is_empty() && !table.is_empty() {
        let position = cells.swap_remove(rng.random_range(0..cells.len()));
        let definition = table[rng.random_range(0..table.len())].clone();
        items.push(ItemSpawn {
            position,
            definition,
        });
    }
    items
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
            assert!(bare.glyphs().is_empty(), "{context}");

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
    fn glyphs_follow_the_placement_rules_on_200_seeds() {
        let content = content();
        for value in 0..SPAWN_SEEDS {
            let seed = Seed::new(value);
            let depth = FIRST_DEPTH + u32::try_from(value).unwrap() % DEEPEST_CHECKED;
            let floor = generate_with_monsters(seed, depth, &content);
            let context = format!("seed {value}, depth {depth}");

            // Deterministic, and drawn after the monsters: those of a floor
            // generated before glyphs existed are untouched.
            let again = generate_with_monsters(seed, depth, &content);
            assert_eq!(floor.glyphs(), again.glyphs(), "{context}");
            let mut rng = seed.stream(depth, Purpose::Generation);
            let layout = lay_out(&mut rng);
            let cells = monster_cells(&layout.floor, &layout.rooms, layout.start_room);
            let table: Vec<&MonsterDef> = content.monsters_at(depth).collect();
            assert_eq!(
                floor.monsters(),
                draw_monsters(cells, &table, &mut rng),
                "{context}"
            );

            let available: Vec<&str> = content
                .sigils()
                .iter()
                .filter(|sigil| sigil.min_depth() <= depth)
                .map(SigilDef::id)
                .collect();
            assert_eq!(
                floor.glyphs().len(),
                GLYPHS_PER_FLOOR.min(available.len()),
                "{context}"
            );
            let distances = DijkstraMap::new(floor.map(), [floor.start()]);
            let mut cells = Vec::new();
            let mut sigils = Vec::new();
            for glyph in floor.glyphs() {
                let cell = glyph.position();
                assert_eq!(floor.map().tile(cell), Some(Tile::Floor), "{context}");
                assert!(
                    distances.distance(cell).is_some_and(|steps| steps > 0),
                    "{context}: {cell:?} not reachable from the start"
                );
                assert!(
                    layout.rooms.iter().any(|&room| inside(room, cell)),
                    "{context}: {cell:?} outside the rooms"
                );
                assert!(
                    floor
                        .monsters()
                        .iter()
                        .all(|spawn| spawn.position() != cell),
                    "{context}: {cell:?} under a monster"
                );
                assert!(
                    available.contains(&glyph.sigil().id()),
                    "{context}: {} too deep",
                    glyph.sigil().id()
                );
                assert!(!cells.contains(&cell), "{context}: {cell:?} twice");
                assert!(!sigils.contains(&glyph.sigil().id()), "{context}");
                cells.push(cell);
                sigils.push(glyph.sigil().id());
            }
        }
    }

    /// The shipped content without any item, to compare floors generated
    /// with and without items to place.
    fn content_without_items() -> Content {
        Content::parse_files(
            include_str!("../../content/monsters.ron"),
            include_str!("../../content/sigils.ron"),
        )
        .expect("the shipped monsters and sigils are valid")
    }

    #[test]
    fn items_follow_the_placement_rules_on_200_seeds() {
        let content = content();
        let without_items = content_without_items();
        for value in 0..SPAWN_SEEDS {
            let seed = Seed::new(value);
            let depth = FIRST_DEPTH + u32::try_from(value).unwrap() % DEEPEST_CHECKED;
            let floor = generate_with_monsters(seed, depth, &content);
            let context = format!("seed {value}, depth {depth}");

            // Deterministic, and drawn last: the layout, the monsters and the
            // glyphs are those of the same floor with no item to place.
            let again = generate_with_monsters(seed, depth, &content);
            assert_eq!(floor.items(), again.items(), "{context}");
            let bare = generate_with_monsters(seed, depth, &without_items);
            assert!(bare.items().is_empty(), "{context}");
            assert_eq!(
                (&bare.map, bare.start, bare.stairs),
                (&floor.map, floor.start, floor.stairs),
                "{context}"
            );
            assert_eq!(bare.monsters(), floor.monsters(), "{context}");
            assert_eq!(bare.glyphs(), floor.glyphs(), "{context}");
            assert!(generate(seed, depth).items().is_empty(), "{context}");

            let layout = lay_out(&mut seed.stream(depth, Purpose::Generation));
            let distances = DijkstraMap::new(floor.map(), [floor.start()]);
            let allowed: Vec<&str> = content.items_at(depth).map(ItemDef::id).collect();
            assert_eq!(floor.items().len(), ITEMS_PER_FLOOR, "{context}");
            let mut cells = Vec::new();
            for spawn in floor.items() {
                let cell = spawn.position();
                assert_eq!(floor.map().tile(cell), Some(Tile::Floor), "{context}");
                assert!(
                    distances.distance(cell).is_some_and(|steps| steps > 0),
                    "{context}: {cell:?} not reachable from the start"
                );
                assert!(
                    layout.rooms.iter().any(|&room| inside(room, cell)),
                    "{context}: {cell:?} outside the rooms"
                );
                assert!(
                    floor.monsters().iter().all(|m| m.position() != cell),
                    "{context}: {cell:?} under a monster"
                );
                assert!(
                    floor.glyphs().iter().all(|g| g.position() != cell),
                    "{context}: {cell:?} on a glyph"
                );
                assert!(
                    allowed.contains(&spawn.definition().id()),
                    "{context}: {} too deep",
                    spawn.definition().id()
                );
                assert!(!cells.contains(&cell), "{context}: {cell:?} twice");
                cells.push(cell);
            }
        }
    }

    #[test]
    fn deeper_floors_hold_deeper_items_and_seeds_vary_them() {
        let content = content();
        let found = |depth| {
            let mut ids: Vec<String> = (0..SPAWN_SEEDS)
                .flat_map(|value| generate_with_monsters(Seed::new(value), depth, &content).items)
                .map(|spawn| spawn.definition.id().to_owned())
                .collect();
            ids.sort_unstable();
            ids.dedup();
            ids
        };

        let mut first: Vec<&str> = content.items_at(FIRST_DEPTH).map(ItemDef::id).collect();
        first.sort_unstable();
        assert_eq!(found(FIRST_DEPTH), first);
        assert_eq!(found(DEEPEST_CHECKED).len(), content.items().len());

        let a = generate_with_monsters(Seed::new(0), FIRST_DEPTH, &content);
        let b = generate_with_monsters(Seed::new(1), FIRST_DEPTH, &content);
        assert_ne!(a.items(), b.items());
    }

    #[test]
    fn items_stop_when_cells_or_definitions_run_out() {
        let content = content();
        let table: Vec<&ItemDef> = content.items().iter().collect();
        let cells = vec![Position::new(3, 3), Position::new(4, 3)];
        let mut rng = Seed::new(0).stream(FIRST_DEPTH, Purpose::Generation);

        let two_cells = draw_items(cells.clone(), &table, &mut rng);
        let mut placed: Vec<Position> = two_cells.iter().map(ItemSpawn::position).collect();
        placed.sort_unstable_by_key(|p| (p.y, p.x));
        assert_eq!(placed, cells);
        assert!(draw_items(cells, &[], &mut rng).is_empty());
        assert!(draw_items(Vec::new(), &table, &mut rng).is_empty());
    }

    #[test]
    fn deeper_floors_engrave_deeper_sigils_and_seeds_vary_the_cells() {
        let content = content();
        let engraved = |depth| {
            let mut ids: Vec<String> = (0..SPAWN_SEEDS)
                .flat_map(|value| generate_with_monsters(Seed::new(value), depth, &content).glyphs)
                .map(|glyph| glyph.sigil.id().to_owned())
                .collect();
            ids.sort_unstable();
            ids.dedup();
            ids
        };

        let first: Vec<&str> = content
            .sigils()
            .iter()
            .filter(|sigil| sigil.min_depth() == FIRST_DEPTH)
            .map(SigilDef::id)
            .collect();
        let mut first_sorted = first.clone();
        first_sorted.sort_unstable();
        assert_eq!(engraved(FIRST_DEPTH), first_sorted);
        assert_eq!(engraved(DEEPEST_CHECKED).len(), content.sigils().len());

        let a = generate_with_monsters(Seed::new(0), FIRST_DEPTH, &content);
        let b = generate_with_monsters(Seed::new(1), FIRST_DEPTH, &content);
        assert_ne!(a.glyphs(), b.glyphs());
    }

    #[test]
    fn glyphs_stop_when_sigils_or_cells_run_out() {
        let content = content();
        let ember = content.sigil("ember").expect("shipped sigil");
        let cells = vec![Position::new(3, 3), Position::new(4, 3)];
        let mut rng = Seed::new(0).stream(FIRST_DEPTH, Purpose::Generation);

        let one_sigil = draw_glyphs(cells.clone(), vec![ember], &mut rng);
        assert_eq!(one_sigil.len(), 1);
        assert_eq!(one_sigil[0].sigil(), ember);

        let all: Vec<&SigilDef> = content.sigils().iter().collect();
        let one_cell = draw_glyphs(vec![Position::new(3, 3)], all.clone(), &mut rng);
        assert_eq!(one_cell.len(), 1);
        assert!(draw_glyphs(cells, Vec::new(), &mut rng).is_empty());
        assert!(draw_glyphs(Vec::new(), all, &mut rng).is_empty());
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
