//! Dungeon floor layout: a rectangular grid of tiles.
//!
//! Exposes [`Tile`] and [`Map`], with bounds-checked access where anything
//! outside the grid counts as impassable and opaque, [`Map::can_step`], the
//! one rule deciding which single steps are allowed, [`Map::blocks_sight`],
//! and [`Map::fixed_room`], a hand-made room kept for tests.
//!
//! The map also keeps the player's sight: which cells are visible right now
//! and which have been seen at least once (remembered). Computing what is
//! visible is the job of the `fov` module, building generated floors that of
//! the `generation` module. Engraved glyphs are not handled here yet.

use crate::geometry::{Direction, Position};

/// Width of the room built by [`Map::fixed_room`], walls included.
///
/// Kept well under 80 columns so the whole room fits on an 80 × 24 terminal
/// before the scrolling camera exists.
const FIXED_ROOM_WIDTH: usize = 30;

/// Height of the room built by [`Map::fixed_room`], walls included.
///
/// Kept well under the roughly 18 map rows left on an 80 × 24 terminal once
/// the status bar and message log are drawn.
const FIXED_ROOM_HEIGHT: usize = 12;

/// Position of the single wall pillar inside [`Map::fixed_room`].
///
/// Placed near the centre, away from the outer walls, so collisions with an
/// obstacle can be tested from all eight sides.
pub const FIXED_ROOM_PILLAR: Position = Position::new(15, 6);

/// What occupies one cell of the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tile {
    /// Open ground that actors can stand on.
    Floor,
    /// Solid rock that blocks movement.
    Wall,
    /// Stairs leading down to the next floor. Actors can stand on them like
    /// on floor; going down is a separate action of the player.
    StairsDown,
}

impl Tile {
    /// Returns whether an actor can stand on this tile.
    pub const fn is_walkable(self) -> bool {
        match self {
            Self::Floor | Self::StairsDown => true,
            Self::Wall => false,
        }
    }

    /// Returns whether this tile stops sight: walls do, floor and stairs do
    /// not.
    pub const fn blocks_sight(self) -> bool {
        match self {
            Self::Wall => true,
            Self::Floor | Self::StairsDown => false,
        }
    }
}

/// A rectangular grid of [`Tile`]s, stored row by row, with what the player
/// sees and remembers of it.
///
/// Two maps are equal when both their tiles and the player's sight of them
/// are equal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Map {
    /// Number of columns.
    width: usize,
    /// Number of rows.
    height: usize,
    /// `width * height` tiles, row-major: the tile at `(x, y)` is at index
    /// `y * width + x`.
    tiles: Vec<Tile>,
    /// One flag per cell, indexed like `tiles`: whether the player sees the
    /// cell right now.
    visible: Vec<bool>,
    /// One flag per cell, indexed like `tiles`: whether the player has seen
    /// the cell at least once on this floor. Never cleared.
    remembered: Vec<bool>,
}

impl Map {
    /// Creates a `width` × `height` map where every cell is `fill`, none of
    /// them visible or remembered yet.
    pub(crate) fn new(width: usize, height: usize, fill: Tile) -> Self {
        Self {
            width,
            height,
            tiles: vec![fill; width * height],
            visible: vec![false; width * height],
            remembered: vec![false; width * height],
        }
    }

    /// Builds the fixed test room: a floor area enclosed by walls on every
    /// edge, with one wall pillar at [`FIXED_ROOM_PILLAR`].
    ///
    /// The room is 30 columns by 12 rows, walls included. Its layout never
    /// changes, which makes it the ground of the movement and scheduling
    /// tests; games are played on generated floors.
    pub fn fixed_room() -> Self {
        let mut map = Self::new(FIXED_ROOM_WIDTH, FIXED_ROOM_HEIGHT, Tile::Floor);
        for (index, tile) in map.tiles.iter_mut().enumerate() {
            let (x, y) = (index % FIXED_ROOM_WIDTH, index / FIXED_ROOM_WIDTH);
            let on_border =
                x == 0 || y == 0 || x == FIXED_ROOM_WIDTH - 1 || y == FIXED_ROOM_HEIGHT - 1;
            if on_border {
                *tile = Tile::Wall;
            }
        }
        map.set_tile(FIXED_ROOM_PILLAR, Tile::Wall);
        map
    }

    /// Returns the number of columns.
    pub const fn width(&self) -> usize {
        self.width
    }

    /// Returns the number of rows.
    pub const fn height(&self) -> usize {
        self.height
    }

    /// Returns the tile at `position`, or `None` when it lies outside the map.
    pub fn tile(&self, position: Position) -> Option<Tile> {
        self.index(position).map(|index| self.tiles[index])
    }

    /// Returns whether an actor can stand at `position`.
    ///
    /// Cells outside the map are never walkable, so movement code needs no
    /// separate bounds check.
    pub fn is_walkable(&self, position: Position) -> bool {
        self.tile(position).is_some_and(Tile::is_walkable)
    }

    /// Returns whether an actor at `from` may step one cell in `direction`.
    ///
    /// This is the single movement rule of the game, shared by player and
    /// monster moves (`action::resolve`) and by every path computation
    /// (`path` module), so that "reachable" always means "walkable to".
    ///
    /// A step is allowed when the destination cell is walkable. Diagonal steps
    /// may cut corners: moving diagonally between two walls that touch only by
    /// their corners is allowed, as long as the destination is open. The rule
    /// looks at the destination only, so it is symmetric between two walkable
    /// cells: if `b` can be reached from `a` in one step, `a` can be reached
    /// from `b`.
    pub fn can_step(&self, from: Position, direction: Direction) -> bool {
        self.is_walkable(from.step(direction))
    }

    /// Returns whether `position` stops sight.
    ///
    /// Walls do, floor and stairs do not ([`Tile::blocks_sight`]). Cells
    /// outside the map always block sight, so nothing is ever seen past the
    /// edge of the map.
    pub fn blocks_sight(&self, position: Position) -> bool {
        self.tile(position).is_none_or(Tile::blocks_sight)
    }

    /// Returns whether the player sees `position` right now.
    ///
    /// Cells outside the map are never visible. Only the field of view last
    /// recorded with `Map::set_visible` counts.
    pub fn is_visible(&self, position: Position) -> bool {
        self.index(position)
            .is_some_and(|index| self.visible[index])
    }

    /// Returns whether the player has seen `position` at least once on this
    /// floor, whether or not it is visible now.
    ///
    /// Every visible cell is remembered, and a remembered cell stays so for
    /// the rest of the floor. Cells outside the map are never remembered.
    pub fn is_remembered(&self, position: Position) -> bool {
        self.index(position)
            .is_some_and(|index| self.remembered[index])
    }

    /// Records `cells` as the player's whole field of view: they become the
    /// only visible cells, and each of them is remembered for good.
    ///
    /// Cells visible before and missing from `cells` stop being visible but
    /// stay remembered.
    ///
    /// # Panics
    ///
    /// Panics if one of `cells` lies outside the map: a field of view only
    /// holds cells of its map, so that would be a bug in the caller.
    pub(crate) fn set_visible(&mut self, cells: impl IntoIterator<Item = Position>) {
        self.visible.fill(false);
        for position in cells {
            let index = self.index(position).unwrap_or_else(|| {
                panic!(
                    "cannot see ({}, {}): outside the {}x{} map",
                    position.x, position.y, self.width, self.height
                )
            });
            self.visible[index] = true;
            self.remembered[index] = true;
        }
    }

    /// Returns every position of the map in reading order: row by row from
    /// the top, left to right within a row.
    ///
    /// This is also the order of the internal tile storage, so the `n`-th
    /// position matches index `n` of a grid laid out like this map.
    ///
    /// # Panics
    ///
    /// Panics if the width or height does not fit in an `i32`, which no
    /// dungeon floor comes close to.
    pub(crate) fn positions(&self) -> impl Iterator<Item = Position> {
        let width = i32::try_from(self.width).expect("map width fits in i32");
        let height = i32::try_from(self.height).expect("map height fits in i32");
        (0..height).flat_map(move |y| (0..width).map(move |x| Position::new(x, y)))
    }

    /// Replaces the tile at `position` with `tile`.
    ///
    /// # Panics
    ///
    /// Panics if `position` lies outside the map: writing there is a bug in
    /// the code building the map, not a game situation.
    pub(crate) fn set_tile(&mut self, position: Position, tile: Tile) {
        let index = self.index(position).unwrap_or_else(|| {
            panic!(
                "cannot set tile at ({}, {}): outside the {}x{} map",
                position.x, position.y, self.width, self.height
            )
        });
        self.tiles[index] = tile;
    }

    /// Converts `position` into an index in `tiles`, or `None` when it lies
    /// outside the map (negative or beyond the width or height).
    ///
    /// Grids laid out like this map (one value per cell, in the order of
    /// [`Map::positions`]) use the same index.
    pub(crate) fn index(&self, position: Position) -> Option<usize> {
        let x = usize::try_from(position.x).ok()?;
        let y = usize::try_from(position.y).ok()?;
        (x < self.width && y < self.height).then(|| y * self.width + x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_room_is_enclosed_by_walls() {
        let map = Map::fixed_room();
        let last_x = i32::try_from(map.width()).unwrap() - 1;
        let last_y = i32::try_from(map.height()).unwrap() - 1;

        for x in 0..=last_x {
            assert_eq!(map.tile(Position::new(x, 0)), Some(Tile::Wall));
            assert_eq!(map.tile(Position::new(x, last_y)), Some(Tile::Wall));
        }
        for y in 0..=last_y {
            assert_eq!(map.tile(Position::new(0, y)), Some(Tile::Wall));
            assert_eq!(map.tile(Position::new(last_x, y)), Some(Tile::Wall));
        }
    }

    #[test]
    fn fixed_room_has_floor_inside_and_a_pillar() {
        let map = Map::fixed_room();

        assert_eq!(map.tile(FIXED_ROOM_PILLAR), Some(Tile::Wall));
        assert_eq!(map.tile(Position::new(1, 1)), Some(Tile::Floor));
        assert_eq!(map.tile(Position::new(14, 6)), Some(Tile::Floor));

        let walls = map.tiles.iter().filter(|&&t| t == Tile::Wall).count();
        let border = 2 * FIXED_ROOM_WIDTH + 2 * (FIXED_ROOM_HEIGHT - 2);
        assert_eq!(
            walls,
            border + 1,
            "only the border and the pillar are walls"
        );
    }

    #[test]
    fn outside_the_map_has_no_tile_and_is_not_walkable() {
        let map = Map::new(3, 2, Tile::Floor);

        for outside in [
            Position::new(-1, 0),
            Position::new(0, -1),
            Position::new(3, 0),
            Position::new(0, 2),
        ] {
            assert_eq!(map.tile(outside), None);
            assert!(!map.is_walkable(outside));
        }
        assert!(map.is_walkable(Position::new(2, 1)));
    }

    #[test]
    fn stairs_are_walkable_like_floor() {
        let mut map = Map::new(2, 1, Tile::Wall);
        map.set_tile(Position::new(1, 0), Tile::StairsDown);

        assert!(map.is_walkable(Position::new(1, 0)));
        assert!(map.can_step(Position::new(0, 0), Direction::East));
    }

    #[test]
    fn walls_are_not_walkable() {
        let mut map = Map::new(3, 3, Tile::Floor);
        map.set_tile(Position::new(1, 1), Tile::Wall);

        assert!(!map.is_walkable(Position::new(1, 1)));
        assert!(map.is_walkable(Position::new(0, 1)));
    }

    #[test]
    fn can_step_onto_walkable_cells_only() {
        let mut map = Map::new(3, 1, Tile::Floor);
        map.set_tile(Position::new(2, 0), Tile::Wall);

        assert!(map.can_step(Position::new(0, 0), Direction::East));
        assert!(!map.can_step(Position::new(1, 0), Direction::East));
        assert!(!map.can_step(Position::new(0, 0), Direction::West));
    }

    #[test]
    fn diagonal_step_may_cut_between_two_walls() {
        // .#
        // #.
        let mut map = Map::new(2, 2, Tile::Floor);
        map.set_tile(Position::new(1, 0), Tile::Wall);
        map.set_tile(Position::new(0, 1), Tile::Wall);

        assert!(map.can_step(Position::new(0, 0), Direction::SouthEast));
        assert!(map.can_step(Position::new(1, 1), Direction::NorthWest));
    }

    #[test]
    fn positions_follow_reading_order_and_match_indices() {
        let map = Map::new(3, 2, Tile::Floor);
        let positions: Vec<Position> = map.positions().collect();

        assert_eq!(
            positions,
            [(0, 0), (1, 0), (2, 0), (0, 1), (1, 1), (2, 1)].map(|(x, y)| Position::new(x, y))
        );
        for (index, &position) in positions.iter().enumerate() {
            assert_eq!(map.index(position), Some(index));
        }
    }

    #[test]
    fn walls_and_the_outside_block_sight_floor_and_stairs_do_not() {
        let mut map = Map::new(3, 1, Tile::Floor);
        map.set_tile(Position::new(1, 0), Tile::Wall);
        map.set_tile(Position::new(2, 0), Tile::StairsDown);

        assert!(!map.blocks_sight(Position::new(0, 0)));
        assert!(map.blocks_sight(Position::new(1, 0)));
        assert!(!map.blocks_sight(Position::new(2, 0)));
        assert!(map.blocks_sight(Position::new(3, 0)));
        assert!(map.blocks_sight(Position::new(0, -1)));
    }

    #[test]
    fn a_new_map_shows_and_remembers_nothing() {
        let map = Map::fixed_room();

        assert!(
            map.positions()
                .all(|p| !map.is_visible(p) && !map.is_remembered(p))
        );
    }

    #[test]
    fn seen_cells_stay_remembered_after_they_leave_the_view() {
        let mut map = Map::new(4, 1, Tile::Floor);
        let [a, b, c, d] = [0, 1, 2, 3].map(|x| Position::new(x, 0));

        map.set_visible([a, b]);
        map.set_visible([c]);

        assert!(!map.is_visible(a) && !map.is_visible(b));
        assert!(map.is_visible(c));
        assert!(map.is_remembered(a) && map.is_remembered(b) && map.is_remembered(c));
        assert!(!map.is_visible(d) && !map.is_remembered(d), "never seen");
        assert!(!map.is_visible(Position::new(-1, 0)));
        assert!(!map.is_remembered(Position::new(4, 0)));
    }

    #[test]
    #[should_panic(expected = "cannot see (5, 0): outside the 4x1 map")]
    fn seeing_outside_the_map_panics() {
        Map::new(4, 1, Tile::Floor).set_visible([Position::new(5, 0)]);
    }

    #[test]
    #[should_panic(expected = "outside the 2x2 map")]
    fn set_tile_outside_the_map_panics() {
        Map::new(2, 2, Tile::Floor).set_tile(Position::new(2, 0), Tile::Wall);
    }
}
