//! Dungeon floors built from the run's seed: layout, start, stairs (or the
//! seal of the bottom floor), monsters, engraved glyphs and items.
//!
//! Exposes [`generate`] (a layout of rooms alone), [`generate_of_kind`] (a
//! layout of the given kind alone), [`floor_kind`] (the kind of a floor),
//! [`is_bottom_floor`] (whether a floor has the seal instead of stairs),
//! [`generate_with_monsters`] (the floor of its kind, with monsters,
//! glyphs and items), [`Floor`], [`MonsterSpawn`],
//! [`Engraving`], [`ItemSpawn`], the size of a floor ([`FLOOR_WIDTH`],
//! [`FLOOR_HEIGHT`]), [`FIRST_DEPTH`], the rule of difficulty
//! ([`monsters_per_floor`] with [`MONSTERS_PER_FLOOR`],
//! [`EXTRA_MONSTERS_PER_DEPTH`] and [`MAX_MONSTERS_PER_FLOOR`]),
//! [`GLYPHS_PER_FLOOR`] and [`ITEMS_PER_FLOOR`]. A floor is
//! a pure function of the seed, the depth and the content: every random
//! choice is drawn from the stream `Seed::stream(depth, Purpose::Generation)`
//! in a fixed order, and nothing reads the clock or iterates over a hash map.
//! The same seed and depth therefore give the same floor on every platform,
//! whatever happened on earlier floors.
//!
//! The layout itself comes from the `bsp` submodule (rooms and corridors)
//! or the `caves` submodule (one cave grown by a cellular automaton),
//! depending on the kind of the floor. The kind is drawn first, from its own
//! stream `Seed::stream(depth, Purpose::FloorKind)` and the weights of the
//! content's floor file, so choosing it never shifts the draws of a layout.
//! Monsters are drawn after the layout, engraved glyphs after the monsters,
//! and items after the glyphs, all from the generation stream, so placing
//! what comes later never changes the layout, the monsters or the glyphs of
//! a seed. Both kinds share every step after the layout: only the cells
//! open to placement differ (the rooms of a floor of rooms, the whole of a
//! cave).
//!
//! The bottom floor, `content::MAX_DEPTH`, has no stairs: the seal whose
//! breaking wins the run lies where its stairs would be (decision D23).
//! Swapping one tile for the other draws nothing, so every floor above the
//! bottom, and the layout of the bottom floor itself, are exactly what they
//! were before the seal existed.
//!
//! Difficulty grows with depth in two ways: the spawn tables of the content
//! favour stronger creatures deeper down (`Content::monster_weights`), and
//! deeper floors hold more of them ([`monsters_per_floor`]).

mod bsp;
mod caves;

use rand::RngExt as _;
use rand_chacha::ChaCha8Rng;

use crate::content::{Content, FloorKind, ItemDef, MAX_DEPTH, MonsterDef, SigilDef};
use crate::fov::{self, SIGHT_RANGE};
use crate::geometry::Position;
use crate::map::{Map, Tile};
use crate::path::DijkstraMap;
use crate::rng::{Purpose, Seed};
use crate::schedule::ACTION_THRESHOLD;

/// Number of columns of a floor.
///
/// Decision D11: floors are larger than the 58 × 16 map area of an 80 × 24
/// terminal, and a camera follows the player.
pub const FLOOR_WIDTH: usize = 80;

/// Number of rows of a floor (decision D11, see [`FLOOR_WIDTH`]).
pub const FLOOR_HEIGHT: usize = 40;

/// Depth of the first floor of a run; each staircase leads one deeper.
pub const FIRST_DEPTH: u32 = 1;

/// Number of monsters placed on the first floor by
/// [`generate_with_monsters`]; deeper floors hold more (see
/// [`monsters_per_floor`]).
///
/// A floor has 8 to 16 rooms, about 12 on average, and the start room stays
/// empty: six monsters leave roughly one room in two occupied, so the player
/// meets creatures regularly while crossing the floor and still finds quiet
/// rooms to recover in. Fewer are placed only if the rooms lack free cells,
/// which the shipped layouts never do.
pub const MONSTERS_PER_FLOOR: usize = 6;

/// Monsters added to a floor for each floor below the first (see
/// [`monsters_per_floor`]).
///
/// One per floor: on top of the stronger creatures the spawn tables bring
/// deeper down, the count grows steadily enough that each staircase is felt,
/// without the jump from one floor to the next ever doubling a fight.
pub const EXTRA_MONSTERS_PER_DEPTH: usize = 1;

/// Most monsters placed on a floor, however deep (see
/// [`monsters_per_floor`]).
///
/// Twelve: about one per room on an average floor of 12 rooms, the start
/// room still empty. More would leave no quiet room to recover in, and the
/// floor would be a single running fight.
pub const MAX_MONSTERS_PER_FLOOR: usize = 12;

/// Number of engraved glyphs placed on a floor by
/// [`generate_with_monsters`].
///
/// Two: the second floor offers two new sigils (see `content/sigils.ron`),
/// so a careful player finds the next ones as they become available deeper.
/// More would hand out the whole grimoire in a few floors; fewer would leave
/// a floor without discovery. Fewer are placed only when fewer sigils are
/// available at that depth: on the first floor, one, since Ember is known
/// from the start and only Recall is left to find there.
pub const GLYPHS_PER_FLOOR: usize = 2;

/// Number of items placed on a floor by [`generate_with_monsters`].
///
/// Three: about one room in four holds an item, so exploring pays off on
/// every floor, while a player who drinks every vial still runs short over
/// a few floors of fights. Fewer are placed only when the rooms lack free
/// cells or no item can be found that deep.
pub const ITEMS_PER_FLOOR: usize = 3;

/// Returns how many monsters [`generate_with_monsters`] places on floor
/// `depth`: the rule of difficulty of the dungeon.
///
/// [`MONSTERS_PER_FLOOR`] on the first floor, then
/// [`EXTRA_MONSTERS_PER_DEPTH`] more for each floor below it, up to
/// [`MAX_MONSTERS_PER_FLOOR`] (reached on floor 7). A `depth` shallower than
/// the first floor counts as the first. Fewer are placed only when the rooms
/// lack free cells.
pub fn monsters_per_floor(depth: u32) -> usize {
    let below_first = usize::try_from(depth.saturating_sub(FIRST_DEPTH)).unwrap_or(usize::MAX);
    below_first
        .saturating_mul(EXTRA_MONSTERS_PER_DEPTH)
        .saturating_add(MONSTERS_PER_FLOOR)
        .min(MAX_MONSTERS_PER_FLOOR)
}

/// Returns whether floor `depth` is the bottom of the dungeon, laid out
/// with [`Tile::Seal`] where the stairs would be (decision D23).
///
/// The bottom is `content::MAX_DEPTH`, the deepest floor the content is
/// written for. A game never goes below it, since it has no stairs; deeper
/// numbers, which only a direct call to the generator can ask for, count as
/// the bottom too, as they count as it for the content's tables.
pub const fn is_bottom_floor(depth: u32) -> bool {
    depth >= MAX_DEPTH
}

/// A generated dungeon floor: its tiles, where the player arrives, where the
/// stairs down (or the seal) are, which monsters wait on it and what lies on
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Floor {
    /// The tiles, with [`Tile::StairsDown`] at `stairs`, or [`Tile::Seal`]
    /// on the bottom floor.
    pub(crate) map: Map,
    /// Floor cell, inside a room or the cave, where the player arrives.
    pub(crate) start: Position,
    /// Cell of the way down: the stairs, or the seal on the bottom floor.
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
    /// Returns the floor cell the item lies on: a plain floor cell of a room,
    /// or of the cave, that the player can walk to from the start.
    pub const fn position(&self) -> Position {
        self.position
    }

    /// Returns the definition of the item, one of those `Content::items_at`
    /// allows on the floor's depth, drawn according to its weight there.
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
    /// than the start room, or anywhere in the cave, out of the player's
    /// sight range from the start.
    pub const fn position(&self) -> Position {
        self.position
    }

    /// Returns the definition of the creature, one of those
    /// `Content::monsters_at` allows on the floor's depth, drawn according to
    /// its weight there.
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
    /// rooms, or of the cave.
    pub const fn start(&self) -> Position {
        self.start
    }

    /// Returns the cell of the stairs down or, on the bottom floor (see
    /// [`is_bottom_floor`]), of the seal that lies there instead; see
    /// [`generate_of_kind`] for how it is chosen.
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

/// Builds floor `depth` of the run identified by `seed` as a floor of rooms:
/// [`generate_of_kind`] with [`FloorKind::Rooms`], whatever kind the
/// content gives that floor.
///
/// A game without content lays out every floor this way. Tests that walk a
/// generated floor use it too: the first floor of a seed, always rooms in
/// the shipped content, is exactly this layout.
pub fn generate(seed: Seed, depth: u32) -> Floor {
    generate_of_kind(seed, depth, FloorKind::Rooms)
}

/// Builds floor `depth` of the run identified by `seed`, laid out as `kind`.
///
/// The floor is [`FLOOR_WIDTH`] × [`FLOOR_HEIGHT`] cells enclosed by walls
/// on every edge, and every walkable cell can be reached from every other
/// one:
///
/// - [`FloorKind::Rooms`]: rooms joined by corridors, from 8 to 16 rooms
///   depending on the floor. The player starts on a random cell of a random
///   room.
/// - [`FloorKind::Caves`]: one cave grown by a cellular automaton, almost
///   always of 900 open cells or more. The player starts at one end of it:
///   the walkable cell farthest from a random cell of the cave, so the
///   crossing spans the whole cave rather than half of it from a start
///   drawn in its middle.
///
/// The stairs go on the walkable cell farthest from the start in steps (the
/// [`DijkstraMap`] of the start, ties broken in reading order). Placing them
/// at the greatest distance makes every floor a crossing; in particular the
/// stairs are always in the farthest third of the floor, the guarantee the
/// game design asks for. On the bottom floor ([`is_bottom_floor`]) that
/// cell holds [`Tile::Seal`] instead, and the floor has no stairs.
///
/// Every draw comes from `Seed::stream(depth, Purpose::Generation)`, so
/// calling this twice with the same arguments returns equal floors. `depth`
/// is the floor number, [`FIRST_DEPTH`] for the first one; any value is
/// accepted. No monster, glyph nor item is placed: see
/// [`generate_with_monsters`].
pub fn generate_of_kind(seed: Seed, depth: u32, kind: FloorKind) -> Floor {
    let mut floor = lay_out(kind, &mut seed.stream(depth, Purpose::Generation)).floor;
    seal_the_bottom(&mut floor, depth);
    floor
}

/// Returns how floor `depth` of the run identified by `seed` is laid out:
/// a kind drawn among `content.floor_kind_weights(depth)`, each with a
/// chance proportional to its weight.
///
/// The draw is the first of `Seed::stream(depth, Purpose::FloorKind)`, a
/// stream used for nothing else, so the kind is a pure function of the
/// seed, the depth and the content, and drawing it never changes the
/// layout of a floor of that kind. A floor with a single kind always gets
/// it.
pub fn floor_kind(seed: Seed, depth: u32, content: &Content) -> FloorKind {
    let kinds: Vec<(FloorKind, u32)> = content.floor_kind_weights(depth).collect();
    let table: Vec<(&FloorKind, u32)> =
        kinds.iter().map(|(kind, weight)| (kind, *weight)).collect();
    *draw_weighted(&table, &mut seed.stream(depth, Purpose::FloorKind))
}

/// Builds floor `depth` of the run identified by `seed`, laid out as the
/// kind [`floor_kind`] draws for it, and places [`monsters_per_floor`]
/// monsters, [`GLYPHS_PER_FLOOR`] engraved glyphs and [`ITEMS_PER_FLOOR`]
/// items on it.
///
/// The map, start and stairs (or seal) are exactly those of
/// [`generate_of_kind`] with the same seed, depth and kind. Below, the open
/// cells are the cells of the rooms (room by room in layout order, reading
/// order within a room; the corridors are left out) on a floor of rooms, and
/// every walkable cell in reading order in a cave. Then, from the same
/// generation stream, each monster gets in turn:
///
/// - a cell drawn uniformly among the free floor cells of the open cells,
///   the start room excluded on a floor of rooms, that lie out of
///   [`SIGHT_RANGE`] of the start (whatever the walls), so the player sees
///   no monster on arrival;
/// - a definition drawn among `content.monster_weights(depth)`, each with a
///   chance proportional to its weight, in one draw of the stream;
/// - a starting energy (see [`MonsterSpawn::energy`]).
///
/// Two monsters never share a cell, and none stands on the stairs or the
/// seal. If fewer free cells than [`monsters_per_floor`] exist, one monster
/// is placed per free cell.
///
/// Then, still from the same stream, each glyph gets in turn a cell drawn
/// uniformly among the plain floor cells of the open cells (the start room
/// included) that hold neither the start, the stairs, a monster nor
/// another glyph (the seal of the bottom floor is no plain floor either),
/// and a sigil drawn uniformly among the sigils of `content` whose glyph
/// can appear at `depth` (`SigilDef::can_appear_at`) and not yet engraved
/// on this floor. Every glyph thus lies on floor the
/// player can walk to and teaches a different sigil; fewer glyphs are
/// placed when fewer such sigils or cells exist, none when no sigil is
/// available that deep.
///
/// Last, still from the same stream, each item gets in turn a cell drawn
/// uniformly among the plain floor cells of the open cells (the start room
/// included) that hold neither the start, the stairs, a monster, a glyph
/// nor another item, and a definition drawn among
/// `content.item_weights(depth)` according to the weights, as for monsters;
/// the same kind may be drawn twice. Every item
/// thus lies on floor the player can walk to, one per cell; fewer are
/// placed when fewer cells exist, none when no item is available that
/// deep. The layout, monsters and glyphs are exactly those of the same
/// floor with no item to place.
///
/// Calling this twice with the same arguments returns equal floors.
pub fn generate_with_monsters(seed: Seed, depth: u32, content: &Content) -> Floor {
    let kind = floor_kind(seed, depth, content);
    let mut rng = seed.stream(depth, Purpose::Generation);
    let Layout {
        mut floor,
        open_cells,
        start_room,
    } = lay_out(kind, &mut rng);
    seal_the_bottom(&mut floor, depth);
    let cells = monster_cells(&floor, &open_cells, start_room);
    let table: Vec<(&MonsterDef, u32)> = content.monster_weights(depth).collect();
    floor.monsters = draw_monsters(cells, &table, monsters_per_floor(depth), &mut rng);
    let cells = glyph_cells(&floor, &open_cells);
    let sigils: Vec<&SigilDef> = content
        .sigils()
        .iter()
        .filter(|sigil| sigil.can_appear_at(depth))
        .collect();
    floor.glyphs = draw_glyphs(cells, sigils, &mut rng);
    let cells = item_cells(&floor, &open_cells);
    let table: Vec<(&ItemDef, u32)> = content.item_weights(depth).collect();
    floor.items = draw_items(cells, &table, &mut rng);
    floor
}

/// A floor without monsters, with the cells things may be placed on.
struct Layout {
    /// The floor: map, start and stairs, no monster.
    floor: Floor,
    /// The cells monsters, glyphs and items may be drawn among, in the order
    /// the draws index them: room by room in layout order and in reading
    /// order within a room on a floor of rooms (corridors left out), every
    /// walkable cell in reading order in a cave. May include the start and
    /// the stairs, which the placement rules leave out.
    open_cells: Vec<Position>,
    /// The room holding the start on a floor of rooms, where no monster
    /// waits; `None` in a cave, whose start is kept clear by the sight range
    /// rule alone.
    start_room: Option<bsp::Rect>,
}

/// Lays out a floor of `kind`, then picks the start and the stairs, drawing
/// from `rng`.
///
/// The stream is left right after the last draw of the layout, ready for
/// whatever is placed next.
fn lay_out(kind: FloorKind, rng: &mut ChaCha8Rng) -> Layout {
    match kind {
        FloorKind::Rooms => lay_out_rooms(rng),
        FloorKind::Caves => lay_out_cave(rng),
    }
}

/// Carves the rooms and corridors, then picks the start in a random room
/// and the stairs at the farthest cell, drawing from `rng`.
fn lay_out_rooms(rng: &mut ChaCha8Rng) -> Layout {
    let (map, rooms) = bsp::carve(FLOOR_WIDTH, FLOOR_HEIGHT, rng);
    let (start_room, start) = choose_start(&rooms, rng);
    Layout {
        floor: with_stairs(map, start),
        open_cells: rooms.iter().flat_map(|room| room.cells()).collect(),
        start_room: Some(rooms[start_room]),
    }
}

/// Grows a cave, then picks the start at one end of it and the stairs at
/// the farthest cell, drawing from `rng`.
fn lay_out_cave(rng: &mut ChaCha8Rng) -> Layout {
    let map = caves::carve(FLOOR_WIDTH, FLOOR_HEIGHT, rng);
    let open_cells: Vec<Position> = map.positions().filter(|&p| map.is_walkable(p)).collect();
    let start = choose_cave_start(&map, &open_cells, rng);
    Layout {
        floor: with_stairs(map, start),
        open_cells,
        start_room: None,
    }
}

/// Returns the floor of `map` with the player arriving at `start` and the
/// stairs down on the walkable cell farthest from it (see
/// [`farthest_cell`]), with neither monster, glyph nor item.
fn with_stairs(mut map: Map, start: Position) -> Floor {
    let stairs = farthest_cell(&map, start);
    map.set_tile(stairs, Tile::StairsDown);
    Floor {
        map,
        start,
        stairs,
        monsters: Vec::new(),
        glyphs: Vec::new(),
        items: Vec::new(),
    }
}

/// Puts the seal on the cell of the stairs of `floor` when `depth` is the
/// bottom floor ([`is_bottom_floor`]), which then has no stairs; leaves any
/// other floor as it is.
///
/// Draws nothing, so the layout, start and placements of the floor are
/// those it would have with stairs.
fn seal_the_bottom(floor: &mut Floor, depth: u32) {
    if is_bottom_floor(depth) {
        floor.map.set_tile(floor.stairs, Tile::Seal);
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

/// Picks the start of a cave: the walkable cell of `map` farthest from a
/// cell drawn uniformly among `open_cells`, one end of the cave.
///
/// # Panics
///
/// Panics if `open_cells` is empty; a cave always has open cells.
fn choose_cave_start(map: &Map, open_cells: &[Position], rng: &mut ChaCha8Rng) -> Position {
    assert!(!open_cells.is_empty(), "a cave always has open cells");
    farthest_cell(map, open_cells[rng.random_range(0..open_cells.len())])
}

/// Returns whether `position` lies inside `room`.
const fn inside(room: bsp::Rect, position: Position) -> bool {
    position.x >= room.x
        && position.x < room.x + room.width
        && position.y >= room.y
        && position.y < room.y + room.height
}

/// Returns the cells a monster may be placed on, in the order of
/// `open_cells`: plain floor cells (not the stairs nor the seal) outside
/// `start_room`, out of sight range of the start.
fn monster_cells(
    floor: &Floor,
    open_cells: &[Position],
    start_room: Option<bsp::Rect>,
) -> Vec<Position> {
    open_cells
        .iter()
        .copied()
        .filter(|&cell| {
            start_room.is_none_or(|room| !inside(room, cell))
                && floor.map.tile(cell) == Some(Tile::Floor)
                && !fov::in_sight_range(floor.start, cell, SIGHT_RANGE)
        })
        .collect()
}

/// Draws up to `count` monsters from `rng`: for each, a cell taken out of
/// `cells`, a definition of `table` according to its weight (see
/// [`draw_weighted`]), then a starting energy.
///
/// # Panics
///
/// Panics if `table` is empty while cells remain; `Content` guarantees at
/// least one monster for every depth.
fn draw_monsters(
    mut cells: Vec<Position>,
    table: &[(&MonsterDef, u32)],
    count: usize,
    rng: &mut ChaCha8Rng,
) -> Vec<MonsterSpawn> {
    let mut monsters = Vec::with_capacity(count);
    while monsters.len() < count && !cells.is_empty() {
        assert!(!table.is_empty(), "every depth has at least one monster");
        let position = cells.swap_remove(rng.random_range(0..cells.len()));
        let definition = draw_weighted(table, rng).clone();
        let energy = rng.random_range(0..ACTION_THRESHOLD);
        monsters.push(MonsterSpawn {
            position,
            definition,
            energy,
        });
    }
    monsters
}

/// Returns the cells a glyph may be engraved on, in the order of
/// `open_cells`: plain floor cells (not the stairs nor the seal) that hold
/// neither the start nor a monster of `floor`.
fn glyph_cells(floor: &Floor, open_cells: &[Position]) -> Vec<Position> {
    open_cells
        .iter()
        .copied()
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

/// Returns the cells an item may lie on, in the order of `open_cells`: the
/// cells a glyph may be engraved on (see [`glyph_cells`]) that hold no glyph
/// of `floor`.
fn item_cells(floor: &Floor, open_cells: &[Position]) -> Vec<Position> {
    glyph_cells(floor, open_cells)
        .into_iter()
        .filter(|&cell| floor.glyphs.iter().all(|glyph| glyph.position != cell))
        .collect()
}

/// Draws up to [`ITEMS_PER_FLOOR`] items from `rng`: for each, a cell taken
/// out of `cells`, then a definition of `table` according to its weight (see
/// [`draw_weighted`]), so no cell is used twice. Draws nothing when `table`
/// is empty, and stops early when the cells run out.
fn draw_items(
    mut cells: Vec<Position>,
    table: &[(&ItemDef, u32)],
    rng: &mut ChaCha8Rng,
) -> Vec<ItemSpawn> {
    let mut items = Vec::with_capacity(ITEMS_PER_FLOOR);
    while items.len() < ITEMS_PER_FLOOR && !cells.is_empty() && !table.is_empty() {
        let position = cells.swap_remove(rng.random_range(0..cells.len()));
        let definition = draw_weighted(table, rng).clone();
        items.push(ItemSpawn {
            position,
            definition,
        });
    }
    items
}

/// Draws one entry of `table` from `rng`, each with a chance proportional to
/// its weight, in one draw.
///
/// The draw is a number below the sum of the weights, and the entries are
/// walked in order until it falls within one: when every weight is 1, this
/// is exactly a uniform draw of an index, so floors whose table has equal
/// weights are the same as with a uniform choice.
///
/// # Panics
///
/// Panics if `table` is empty or its weights sum to 0; `Content` lists
/// positive weights only, and the callers never pass an empty table.
fn draw_weighted<'a, T>(table: &[(&'a T, u32)], rng: &mut ChaCha8Rng) -> &'a T {
    let weight = |entry: &(&T, u32)| usize::try_from(entry.1).unwrap_or(usize::MAX);
    let total: usize = table.iter().map(weight).sum();
    assert!(total > 0, "a weighted draw needs a positive weight");
    let mut roll = rng.random_range(0..total);
    for entry in table {
        if roll < weight(entry) {
            return entry.0;
        }
        roll -= weight(entry);
    }
    unreachable!("the roll is below the sum of the weights")
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
    use crate::content::MAX_DEPTH;
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

    /// Draws `floor` as text rows: `#` wall, `.` floor, `>` stairs, `*` the
    /// seal and `@` the start.
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
                    Some(Tile::Seal) => '*',
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

    /// Checks the guarantees every floor of any kind gives at `depth`: its
    /// size, the start on a floor cell, a single way down (the stairs, or the
    /// seal on the bottom floor, never both) reachable and in the farthest
    /// third from it, every walkable cell reachable from the start, and walls
    /// all along the edges.
    fn check_floor_guarantees(floor: &Floor, depth: u32) -> Result<(), TestCaseError> {
        let map = floor.map();

        prop_assert_eq!(map.width(), FLOOR_WIDTH);
        prop_assert_eq!(map.height(), FLOOR_HEIGHT);
        prop_assert_eq!(map.tile(floor.start()), Some(Tile::Floor));
        let exit = if depth >= MAX_DEPTH {
            Tile::Seal
        } else {
            Tile::StairsDown
        };
        let exits: Vec<Position> = map
            .positions()
            .filter(|&p| matches!(map.tile(p), Some(Tile::StairsDown | Tile::Seal)))
            .collect();
        prop_assert_eq!(exits, vec![floor.stairs()]);
        prop_assert_eq!(map.tile(floor.stairs()), Some(exit));

        // Every room and corridor, or every part of the cave, is reachable
        // from the start.
        prop_assert_eq!(flood_fill(map, floor.start()), walkable_cells(map));

        // The stairs are reachable and in the farthest third.
        let distances = DijkstraMap::new(map, [floor.start()]);
        let (_, farthest) = distances.farthest().unwrap();
        let stairs = distances.distance(floor.stairs());
        prop_assert!(stairs.is_some_and(|d| 3 * d >= 2 * farthest && d > 0));

        for position in map.positions().filter(|&p| on_border(map, p)) {
            prop_assert_eq!(map.tile(position), Some(Tile::Wall));
        }
        Ok(())
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(PROPERTY_CASES))]

        /// The guarantees of every floor of rooms, on random seeds and depths.
        #[test]
        fn every_floor_is_connected_with_far_stairs_and_closed_edges(
            seed in any::<u64>(),
            depth in FIRST_DEPTH..=50,
        ) {
            check_floor_guarantees(&generate(Seed::new(seed), depth), depth)?;
        }

        /// The same guarantees for every cave, which is also large enough
        /// and the same for the same seed and depth.
        #[test]
        fn every_cave_is_connected_with_far_stairs_and_closed_edges(
            seed in any::<u64>(),
            depth in FIRST_DEPTH..=50,
        ) {
            let cave = generate_of_kind(Seed::new(seed), depth, FloorKind::Caves);

            check_floor_guarantees(&cave, depth)?;
            prop_assert!(walkable_cells(cave.map()).len() >= caves::MIN_CAVE_CELLS);
            prop_assert_eq!(
                &cave,
                &generate_of_kind(Seed::new(seed), depth, FloorKind::Caves)
            );
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
    fn the_reference_cave_is_frozen() {
        // Same as the reference floor of rooms: if this snapshot changes,
        // every cave of every seed changes too.
        assert_snapshot!(draw(&generate_of_kind(
            REFERENCE_SEED,
            MAX_DEPTH,
            FloorKind::Caves
        )));
    }

    #[test]
    fn caves_and_rooms_of_the_same_floor_differ_and_keep_their_stairs_far() {
        for seed in 0..50 {
            let seed = Seed::new(seed);
            let cave = generate_of_kind(seed, MAX_DEPTH, FloorKind::Caves);
            let distances = DijkstraMap::new(cave.map(), [cave.start()]);

            assert_ne!(cave.map(), generate(seed, MAX_DEPTH).map(), "{seed}");
            assert_eq!(
                distances.farthest(),
                Some((cave.stairs(), distances.distance(cave.stairs()).unwrap())),
                "{seed}"
            );
            // The start is one end of the cave: no cell lies farther from
            // the stairs than the start does.
            let back = DijkstraMap::new(cave.map(), [cave.stairs()]);
            assert_eq!(
                back.farthest().map(|(_, steps)| steps),
                back.distance(cave.start()),
                "{seed}"
            );
        }
    }

    #[test]
    fn the_first_floors_are_rooms_and_the_last_one_a_cave() {
        let content = content();
        for value in 0..SPAWN_SEEDS {
            let seed = Seed::new(value);
            assert_eq!(floor_kind(seed, FIRST_DEPTH, &content), FloorKind::Rooms);
            assert_eq!(floor_kind(seed, 2, &content), FloorKind::Rooms);
            assert_eq!(floor_kind(seed, MAX_DEPTH, &content), FloorKind::Caves);
            assert_eq!(floor_kind(seed, MAX_DEPTH + 5, &content), FloorKind::Caves);
        }
        // The first floor of the reference seed is the floor of rooms frozen
        // in its snapshot, monsters or not.
        let first = generate_with_monsters(REFERENCE_SEED, FIRST_DEPTH, &content);
        assert_eq!(first.map(), generate(REFERENCE_SEED, FIRST_DEPTH).map());
    }

    /// Number of seeds the floor kinds are checked on, at every depth: one
    /// draw per floor, so 1 000 floors give more draws than
    /// [`SHARE_TOLERANCE`] asks for.
    const KIND_SEEDS: u64 = 1000;

    #[test]
    fn floor_kinds_follow_their_weights_on_1000_seeds() {
        let content = content();
        for depth in FIRST_DEPTH..=MAX_DEPTH + 2 {
            let drawn: Vec<&str> = (0..KIND_SEEDS)
                .map(|value| floor_kind(Seed::new(value), depth, &content).name())
                .collect();
            let table: Vec<(&str, u32)> = content
                .floor_kind_weights(depth)
                .map(|(kind, weight)| (kind.name(), weight))
                .collect();

            assert_follows(&drawn, &table, depth, "floor kind");
        }
        // Mixed floors do get both kinds.
        let kinds: Vec<FloorKind> = (0..KIND_SEEDS)
            .map(|value| floor_kind(Seed::new(value), 5, &content))
            .collect();
        assert!(kinds.contains(&FloorKind::Rooms) && kinds.contains(&FloorKind::Caves));
    }

    #[test]
    fn a_floor_is_laid_out_as_its_kind() {
        let content = content();
        for value in 0..SPAWN_SEEDS {
            let seed = Seed::new(value);
            let depth = FIRST_DEPTH + u32::try_from(value).unwrap() % MAX_DEPTH;
            let kind = floor_kind(seed, depth, &content);
            let floor = generate_with_monsters(seed, depth, &content);

            assert_eq!(kind, floor_kind(seed, depth, &content), "{value}");
            let bare = generate_of_kind(seed, depth, kind);
            assert_eq!(
                (&bare.map, bare.start, bare.stairs),
                (&floor.map, floor.start, floor.stairs),
                "seed {value}, depth {depth}"
            );
        }
    }

    #[test]
    fn caves_are_peopled_by_the_same_rules_as_rooms() {
        let content = content();
        for value in 0..SPAWN_SEEDS / 2 {
            let seed = Seed::new(value);
            let floor = generate_with_monsters(seed, MAX_DEPTH, &content);
            let context = format!("seed {value}");
            assert_eq!(
                floor.map(),
                generate_of_kind(seed, MAX_DEPTH, FloorKind::Caves).map(),
                "{context}"
            );

            assert_eq!(
                floor.monsters().len(),
                monsters_per_floor(MAX_DEPTH),
                "{context}"
            );
            assert_eq!(floor.glyphs().len(), GLYPHS_PER_FLOOR, "{context}");
            assert_eq!(floor.items().len(), ITEMS_PER_FLOOR, "{context}");
            let distances = DijkstraMap::new(floor.map(), [floor.start()]);
            let seen_at_start = fov::visible_cells(floor.map(), floor.start(), SIGHT_RANGE);
            let mut cells: Vec<Position> = Vec::new();
            let monsters = floor.monsters().iter().map(MonsterSpawn::position);
            let glyphs = floor.glyphs().iter().map(Engraving::position);
            let items = floor.items().iter().map(ItemSpawn::position);
            for cell in monsters.clone().chain(glyphs).chain(items) {
                assert_eq!(floor.map().tile(cell), Some(Tile::Floor), "{context}");
                assert!(
                    distances.distance(cell).is_some_and(|steps| steps > 0),
                    "{context}: {cell:?} not reachable from the start"
                );
                assert!(!cells.contains(&cell), "{context}: {cell:?} twice");
                cells.push(cell);
            }
            for cell in monsters {
                assert!(
                    !fov::in_sight_range(floor.start(), cell, SIGHT_RANGE),
                    "{context}: {cell:?} within sight range"
                );
                assert!(!seen_at_start.contains(&cell), "{context}: {cell:?} seen");
            }
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

            // Deterministic, and the layout is that of `generate_of_kind`.
            assert_eq!(
                floor,
                generate_with_monsters(seed, depth, &content),
                "{context}"
            );
            let kind = floor_kind(seed, depth, &content);
            let bare = generate_of_kind(seed, depth, kind);
            assert_eq!(
                (&bare.map, bare.start, bare.stairs),
                (&floor.map, floor.start, floor.stairs),
                "{context}"
            );
            assert!(bare.monsters().is_empty(), "{context}");
            assert!(bare.glyphs().is_empty(), "{context}");

            let layout = lay_out(kind, &mut seed.stream(depth, Purpose::Generation));
            assert_eq!(layout.start_room.is_some(), kind == FloorKind::Rooms);
            if let Some(start_room) = layout.start_room {
                assert!(inside(start_room, floor.start()), "{context}");
            }
            let seen_at_start = fov::visible_cells(floor.map(), floor.start(), SIGHT_RANGE);
            let allowed: Vec<&str> = content.monsters_at(depth).map(MonsterDef::id).collect();

            assert_eq!(
                floor.monsters().len(),
                monsters_per_floor(depth),
                "{context}"
            );
            let mut cells = Vec::new();
            for spawn in floor.monsters() {
                let cell = spawn.position();
                assert_eq!(floor.map().tile(cell), Some(Tile::Floor), "{context}");
                assert!(
                    layout.start_room.is_none_or(|room| !inside(room, cell)),
                    "{context}: {cell:?} in start room"
                );
                assert!(
                    layout.open_cells.contains(&cell),
                    "{context}: {cell:?} outside the rooms"
                );
                assert!(
                    !fov::in_sight_range(floor.start(), cell, SIGHT_RANGE),
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
            let layout = lay_out(floor_kind(seed, depth, &content), &mut rng);
            let cells = monster_cells(&layout.floor, &layout.open_cells, layout.start_room);
            let table: Vec<(&MonsterDef, u32)> = content.monster_weights(depth).collect();
            assert_eq!(
                floor.monsters(),
                draw_monsters(cells, &table, monsters_per_floor(depth), &mut rng),
                "{context}"
            );

            let available: Vec<&str> = content
                .sigils()
                .iter()
                .filter(|sigil| sigil.can_appear_at(depth))
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
                    layout.open_cells.contains(&cell),
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
    /// with and without items to place: the shipped files and spawn rules
    /// of the monsters (one rule per line in the spawn file).
    fn content_without_items() -> Content {
        let monster_rules: String = include_str!("../content/spawns.ron")
            .lines()
            .filter(|line| !line.contains("of: Item("))
            .map(|line| {
                format!(
                    "{line}
"
                )
            })
            .collect();
        Content::parse_with_spawns(
            include_str!("../content/monsters.ron"),
            include_str!("../content/sigils.ron"),
            "[]",
            &monster_rules,
        )
        .expect("the shipped monsters, sigils and monster spawns are valid")
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

            let kind = floor_kind(seed, depth, &content);
            let layout = lay_out(kind, &mut seed.stream(depth, Purpose::Generation));
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
                    layout.open_cells.contains(&cell),
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
        let table: Vec<(&ItemDef, u32)> = content.items().iter().map(|item| (item, 1)).collect();
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
            .filter(|sigil| sigil.can_appear_at(FIRST_DEPTH))
            .map(SigilDef::id)
            .collect();
        let mut first_sorted = first.clone();
        first_sorted.sort_unstable();
        assert_eq!(engraved(FIRST_DEPTH), first_sorted);
        // Every sigil but those known from the start is found somewhere.
        let to_learn = content.sigils().iter().filter(|s| !s.known_at_start());
        assert_eq!(engraved(DEEPEST_CHECKED).len(), to_learn.count());
        assert!(!engraved(DEEPEST_CHECKED).contains(&"ember".to_owned()));

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
        let table: Vec<(&MonsterDef, u32)> = content.monster_weights(FIRST_DEPTH).collect();
        let cells = vec![Position::new(3, 3), Position::new(4, 3)];
        let mut rng = Seed::new(0).stream(FIRST_DEPTH, Purpose::Generation);

        let monsters = draw_monsters(cells.clone(), &table, MONSTERS_PER_FLOOR, &mut rng);

        let mut placed: Vec<Position> = monsters.iter().map(MonsterSpawn::position).collect();
        placed.sort_unstable_by_key(|p| (p.y, p.x));
        assert_eq!(placed, cells);
    }

    /// Number of seeds the spawn tables are checked on, at every depth.
    const TABLE_SEEDS: u64 = 200;

    /// Largest gap accepted between the share of draws an entry gets over
    /// [`TABLE_SEEDS`] floors and the share its weight promises.
    ///
    /// A floor draws at least 3 items and 6 monsters, so 200 floors give at
    /// least 600 draws: the standard deviation of a share is then at most
    /// 2 points, and 6 points leave three of them. The seeds are fixed, so
    /// the test cannot fail by chance from one run to the next.
    const SHARE_TOLERANCE: f64 = 0.06;

    /// Checks that `drawn`, the identifiers drawn on floor `depth` over at
    /// least 600 draws, follow `table`, the weights of that floor:
    /// nothing outside the table, every entry at its share within
    /// [`SHARE_TOLERANCE`].
    fn assert_follows(drawn: &[&str], table: &[(&str, u32)], depth: u32, what: &str) {
        let total: u32 = table.iter().map(|&(_, weight)| weight).sum();
        for id in drawn {
            assert!(
                table.iter().any(|(listed, _)| listed == id),
                "depth {depth}: {what} {id} has no weight there"
            );
        }
        for &(id, weight) in table {
            let count = drawn.iter().filter(|&&drawn| drawn == id).count();
            let share = count as f64 / drawn.len() as f64;
            let expected = f64::from(weight) / f64::from(total);
            assert!(
                (share - expected).abs() <= SHARE_TOLERANCE,
                "depth {depth}: {what} {id} drawn {share:.3} of the time, \
                 its weight promises {expected:.3}"
            );
        }
    }

    #[test]
    fn spawns_follow_the_tables_on_200_seeds_at_every_depth() {
        let content = content();
        // Two floors past the deepest written for, which reuse its weights.
        for depth in FIRST_DEPTH..=MAX_DEPTH + 2 {
            let floors: Vec<Floor> = (0..TABLE_SEEDS)
                .map(|value| generate_with_monsters(Seed::new(value), depth, &content))
                .collect();

            let monsters: Vec<&str> = floors
                .iter()
                .flat_map(|floor| floor.monsters().iter().map(|m| m.definition().id()))
                .collect();
            let count = usize::try_from(TABLE_SEEDS).unwrap() * monsters_per_floor(depth);
            assert_eq!(monsters.len(), count, "depth {depth}: monsters per floor");
            let table: Vec<(&str, u32)> = content
                .monster_weights(depth)
                .map(|(monster, weight)| (monster.id(), weight))
                .collect();
            assert_follows(&monsters, &table, depth, "monster");

            let items: Vec<&str> = floors
                .iter()
                .flat_map(|floor| floor.items().iter().map(|i| i.definition().id()))
                .collect();
            let count = usize::try_from(TABLE_SEEDS).unwrap() * ITEMS_PER_FLOOR;
            assert_eq!(items.len(), count, "depth {depth}: items per floor");
            let table: Vec<(&str, u32)> = content
                .item_weights(depth)
                .map(|(item, weight)| (item.id(), weight))
                .collect();
            assert_follows(&items, &table, depth, "item");
        }
    }

    #[test]
    fn deeper_floors_hold_more_monsters_up_to_a_cap() {
        let expected = [
            (0, 6),
            (FIRST_DEPTH, MONSTERS_PER_FLOOR),
            (2, 7),
            (3, 8),
            (6, 11),
            (7, MAX_MONSTERS_PER_FLOOR),
            (8, MAX_MONSTERS_PER_FLOOR),
            (u32::MAX, MAX_MONSTERS_PER_FLOOR),
        ];
        for (depth, count) in expected {
            assert_eq!(monsters_per_floor(depth), count, "depth {depth}");
        }
        for depth in FIRST_DEPTH..MAX_DEPTH {
            assert!(monsters_per_floor(depth + 1) >= monsters_per_floor(depth));
        }
    }

    /// Threat of a creature: its hit points times the damage it deals per
    /// player turn before defense (attack times speed, a normal speed being
    /// one action per turn). A rough measure, but one that grows with every
    /// way a creature gets stronger.
    fn threat(monster: &MonsterDef) -> f64 {
        f64::from(monster.hp()) * f64::from(monster.attack()) * f64::from(monster.speed())
            / f64::from(crate::schedule::NORMAL_SPEED)
    }

    #[test]
    fn the_expected_threat_of_a_floor_grows_with_depth() {
        let content = content();
        // Expected threat of a floor: its number of monsters times the
        // threat of a monster averaged over the weights of the floor.
        let floor_threat = |depth| {
            let (sum, total) = content.monster_weights(depth).fold(
                (0.0, 0.0),
                |(sum, total), (monster, weight)| {
                    let weight = f64::from(weight);
                    (sum + threat(monster) * weight, total + weight)
                },
            );
            sum / total * monsters_per_floor(depth) as f64
        };

        for depth in FIRST_DEPTH..MAX_DEPTH {
            assert!(
                floor_threat(depth + 1) > floor_threat(depth),
                "floor {} ({:.0}) is not more threatening than floor {depth} ({:.0})",
                depth + 1,
                floor_threat(depth + 1),
                floor_threat(depth)
            );
        }
        assert!(floor_threat(MAX_DEPTH) > 4.0 * floor_threat(FIRST_DEPTH));
    }

    #[test]
    fn a_weighted_draw_with_equal_weights_is_a_uniform_draw_of_an_index() {
        let names = ["a", "b", "c", "d"];
        let table: Vec<(&&str, u32)> = names.iter().map(|name| (name, 1)).collect();
        let mut weighted = Seed::new(7).stream(FIRST_DEPTH, Purpose::Generation);
        let mut uniform = Seed::new(7).stream(FIRST_DEPTH, Purpose::Generation);

        for _ in 0..200 {
            let drawn = draw_weighted(&table, &mut weighted);
            assert_eq!(*drawn, names[uniform.random_range(0..names.len())]);
        }
    }

    #[test]
    fn a_weighted_draw_follows_the_weights() {
        let table = [(&"rare", 1), (&"common", 3)];
        let mut rng = Seed::new(3).stream(FIRST_DEPTH, Purpose::Generation);

        let common = (0..4000)
            .filter(|_| *draw_weighted(&table, &mut rng) == "common")
            .count();

        assert!((2850..=3150).contains(&common), "{common} of 4000");
    }

    #[test]
    fn other_seeds_and_depths_give_other_floors() {
        let reference = generate(REFERENCE_SEED, FIRST_DEPTH);

        assert_ne!(generate(Seed::new(43), FIRST_DEPTH), reference);
        assert_ne!(generate(REFERENCE_SEED, FIRST_DEPTH + 1), reference);
    }
}
