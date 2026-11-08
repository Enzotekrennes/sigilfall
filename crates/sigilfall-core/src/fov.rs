//! Field of view: which cells can be seen from a given cell.
//!
//! Exposes [`visible_cells`], the [`SightRange`] that bounds it and the
//! player's [`SIGHT_RANGE`]. The computation is Albert Ford's symmetric
//! shadowcasting (<https://www.albertford.com/shadowcasting/>), with every
//! slope kept as a fraction of two integers so that no rounding error can
//! make two cells disagree about whether they see each other.
//!
//! # Rules
//!
//! - Walls and cells outside the map block sight; floor and stairs let it
//!   through ([`Map::blocks_sight`]).
//! - Between two cells that let sight through, sight is symmetric: if `a`
//!   sees `b`, then `b` sees `a`. A floor cell is seen when the line to its
//!   centre stays clear.
//! - A wall is seen as soon as any part of its face is lit, which is not
//!   symmetric (a wall sees nothing) but keeps the walls of a room and its
//!   corners visible from inside.
//! - Sight goes through a diagonal gap between two walls that touch only by
//!   their corners. This matches the movement rule of
//!   [`Map::can_step`](crate::map::Map::can_step), which lets actors cut
//!   such corners: a passage an actor can walk through is never a blind spot,
//!   and a monster cannot hide behind a gap it can cross.
//! - A cell is in range when its centre lies inside an ellipse whose
//!   half-axes are half a cell longer than the [`SightRange`] (see
//!   [`SightRange::reaches`]), so the lit area is an ellipse rounded to whole
//!   cells, not a rectangle or a diamond. The range test depends only on the
//!   gaps between the two cells, so it keeps sight symmetric.
//!
//! Remembering what was seen is the job of [`Map`]; deciding when to look
//! again is the job of `game::Game`.

use std::ops::RangeInclusive;

use crate::geometry::Position;
use crate::map::Map;

/// How far sight reaches from a cell along each axis, in cells: the half-axes
/// of the ellipse of cells in range.
///
/// A range of `0` along both axes shows only the cell looking around. A
/// range with equal axes draws a disc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SightRange {
    /// Cells seen to the left and to the right, along a row.
    pub columns: u16,
    /// Cells seen above and below, along a column.
    pub rows: u16,
}

impl SightRange {
    /// Returns a range reaching `columns` cells along a row and `rows` cells
    /// along a column.
    pub const fn new(columns: u16, rows: u16) -> Self {
        Self { columns, rows }
    }

    /// Returns whether a cell `dx` columns and `dy` rows away (in either
    /// direction) is within this range, walls ignored.
    ///
    /// The cell is in range when its centre lies strictly inside the ellipse
    /// whose half-axes are `columns + ½` and `rows + ½`, computed exactly in
    /// integers as
    ///
    /// `4·dx²·(2·rows + 1)² + 4·dy²·(2·columns + 1)² < (2·columns + 1)²·(2·rows + 1)²`.
    ///
    /// Along a row this reaches exactly `columns` cells and along a column
    /// exactly `rows` cells; with equal axes `r` it is the disc rule
    /// `dx² + dy² ≤ r² + r`. Only the sizes of the gaps count, so the test
    /// gives the same answer from either cell: it is symmetric.
    pub fn reaches(self, dx: i32, dy: i32) -> bool {
        let dx = u128::from(dx.unsigned_abs());
        let dy = u128::from(dy.unsigned_abs());
        let columns = u128::from(self.columns);
        let rows = u128::from(self.rows);
        // Beyond an axis a cell is out of the ellipse; checking it first also
        // bounds dx and dy by a u16, so no product below can overflow a u128.
        if dx > columns || dy > rows {
            return false;
        }
        let width = 2 * columns + 1;
        let height = 2 * rows + 1;
        4 * dx * dx * height * height + 4 * dy * dy * width * width
            < width * width * height * height
    }

    /// Returns how many cells the range reaches along the axis of
    /// `quadrant`: the farthest row depth a scan of that quadrant can light.
    const fn reach_towards(self, quadrant: Quadrant) -> u16 {
        match quadrant {
            Quadrant::North | Quadrant::South => self.rows,
            Quadrant::East | Quadrant::West => self.columns,
        }
    }
}

/// How far the player sees: 16 cells along a row, 8 along a column.
///
/// On the smallest supported terminal (80 × 24) the map area is 16 rows high,
/// so eight cells above and below the player reach its top and bottom edges:
/// the field of view fills the screen height without lighting cells the
/// camera cannot show. A terminal cell is about twice as tall as it is wide,
/// so 16 cells across cover the same distance on screen as 8 cells up or
/// down: the lit area looks round to the player instead of squashed. It
/// covers most of a generated room from its middle, while leaving long
/// corridors partly dark.
pub const SIGHT_RANGE: SightRange = SightRange::new(16, 8);

/// Returns every cell of `map` visible from `origin` within `range`, in
/// reading order (row by row from the top, left to right).
///
/// `origin` itself is always visible when it lies on the map; when it does
/// not, nothing is visible and the result is empty. A range of `0` along
/// both axes shows only `origin`. A cell is in range by the rule of
/// [`SightRange::reaches`], a test that is symmetric between the two cells.
///
/// The rules of what blocks sight, and the choice about diagonal gaps, are
/// described in the module documentation. The cost is linear in the number of
/// cells in range, plus one pass over the map to list the result.
///
/// # Panics
///
/// Panics if the width or height of `map` does not fit in an `i32`, which no
/// dungeon floor comes close to.
pub fn visible_cells(map: &Map, origin: Position, range: SightRange) -> Vec<Position> {
    let mut visible = vec![false; map.width() * map.height()];
    let Some(origin_index) = map.index(origin) else {
        return Vec::new();
    };
    visible[origin_index] = true;

    let mut caster = Shadowcaster {
        map,
        origin,
        range,
        visible: &mut visible,
    };
    for quadrant in Quadrant::ALL {
        caster.scan(quadrant);
    }

    // `positions` follows the index order of the map, so the n-th flag
    // belongs to the n-th position.
    map.positions()
        .zip(visible)
        .filter_map(|(position, seen)| seen.then_some(position))
        .collect()
}

/// Returns the depth of the last row worth scanning in `quadrant`: the reach
/// of `range` along its axis, capped by the larger side of `map`, since a row
/// farther than that from a cell of the map lies entirely outside it.
fn max_depth(map: &Map, range: SightRange, quadrant: Quadrant) -> i32 {
    let width = i32::try_from(map.width()).expect("map width fits in i32");
    let height = i32::try_from(map.height()).expect("map height fits in i32");
    i32::from(range.reach_towards(quadrant)).min(width.max(height))
}

/// Returns whether `target` lies within `range` of `origin` by the rule of
/// [`SightRange::reaches`], walls ignored.
///
/// Every cell visible from `origin` within `range` passes this test, so a
/// cell that fails it cannot be seen from `origin`, whatever the map. The
/// test is symmetric between the two cells.
pub(crate) fn in_sight_range(origin: Position, target: Position, range: SightRange) -> bool {
    range.reaches(target.x - origin.x, target.y - origin.y)
}

/// One of the four quarters of the view around the origin, each a 90° cone
/// bounded by the two diagonals on either side of one axis.
///
/// A quadrant is scanned in rows moving away from the origin. Inside a
/// quadrant, a cell is located by its `depth` (rows away from the origin,
/// along the axis) and its `column` (offset across the axis); `transform`
/// turns that pair back into a map position.
#[derive(Debug, Clone, Copy)]
enum Quadrant {
    /// Rows above the origin (towards row 0).
    North,
    /// Columns to the right of the origin.
    East,
    /// Rows below the origin.
    South,
    /// Columns to the left of the origin.
    West,
}

impl Quadrant {
    /// The four quadrants; together they cover every direction.
    const ALL: [Self; 4] = [Self::North, Self::East, Self::South, Self::West];

    /// Returns the map position `depth` rows and `column` columns away from
    /// `origin` in this quadrant.
    const fn transform(self, origin: Position, depth: i32, column: i32) -> Position {
        match self {
            Self::North => Position::new(origin.x + column, origin.y - depth),
            Self::East => Position::new(origin.x + depth, origin.y + column),
            Self::South => Position::new(origin.x + column, origin.y + depth),
            Self::West => Position::new(origin.x - depth, origin.y + column),
        }
    }
}

/// A slope `numerator / denominator` of a line from the origin, in columns
/// per row. The denominator is always positive.
///
/// Slopes are exact fractions so that comparing a cell with the edge of a
/// shadow never depends on floating-point rounding.
#[derive(Debug, Clone, Copy)]
struct Slope {
    /// Columns covered over `denominator` rows; may be negative.
    numerator: i32,
    /// Rows; always positive.
    denominator: i32,
}

impl Slope {
    /// The slope of the left diagonal bounding a quadrant (−1).
    const LEFT_DIAGONAL: Self = Self {
        numerator: -1,
        denominator: 1,
    };

    /// The slope of the right diagonal bounding a quadrant (+1).
    const RIGHT_DIAGONAL: Self = Self {
        numerator: 1,
        denominator: 1,
    };

    /// Returns the slope of the line from the origin to the middle of the
    /// left edge of the cell at `column` in the row at `depth`:
    /// `(2 × column − 1) / (2 × depth)`.
    ///
    /// Shadows start and end on these edges: a wall casts its shadow from its
    /// left edge to the left edge of the next cell.
    const fn left_edge(depth: i32, column: i32) -> Self {
        Self {
            numerator: 2 * column - 1,
            denominator: 2 * depth,
        }
    }
}

/// A row of a quadrant still to scan: the cells at `depth` that lie between
/// the `start` and `end` slopes, the part of the row no shadow covers so far.
#[derive(Debug, Clone, Copy)]
struct Row {
    /// Distance from the origin along the quadrant's axis, from `1`.
    depth: i32,
    /// Slope of the left limit of the lit part of the row.
    start: Slope,
    /// Slope of the right limit of the lit part of the row.
    end: Slope,
}

impl Row {
    /// Returns the columns of the cells this row touches.
    ///
    /// The first column is `depth × start` rounded half up and the last one
    /// `depth × end` rounded half down, both computed exactly: a cell is
    /// scanned when more than its half lies between the two slopes.
    fn columns(&self) -> RangeInclusive<i32> {
        // round_half_up(a / b) = floor((2a + b) / 2b), with a = depth × num
        // and b = den > 0.
        let start = self.start;
        let first = (2 * self.depth * start.numerator + start.denominator)
            .div_euclid(2 * start.denominator);
        // round_half_down(a / b) = ceil((2a − b) / 2b) = −floor((b − 2a) / 2b).
        let end = self.end;
        let last =
            -(end.denominator - 2 * self.depth * end.numerator).div_euclid(2 * end.denominator);
        first..=last
    }

    /// Returns whether the centre of the cell at `column` lies between the
    /// start and end slopes, edges included.
    ///
    /// This is what makes the algorithm symmetric: a floor cell is revealed
    /// only when the line to its centre is clear.
    const fn holds_centre(&self, column: i32) -> bool {
        column * self.start.denominator >= self.depth * self.start.numerator
            && column * self.end.denominator <= self.depth * self.end.numerator
    }

    /// Returns the next row away from the origin, between the same slopes.
    const fn next(&self) -> Self {
        Self {
            depth: self.depth + 1,
            ..*self
        }
    }
}

/// The state of one field-of-view computation, shared by the four
/// quadrant scans.
struct Shadowcaster<'a> {
    /// The map whose walls block sight.
    map: &'a Map,
    /// The cell looking around.
    origin: Position,
    /// How far sight reaches (see [`SightRange::reaches`]).
    range: SightRange,
    /// One flag per cell of `map`, indexed like it: whether it is visible.
    visible: &'a mut [bool],
}

impl Shadowcaster<'_> {
    /// Marks every cell of `quadrant` visible from the origin.
    ///
    /// Rows are scanned outwards. Along a row, each run of cells that let
    /// sight through gives the next row its lit part, bounded by the walls
    /// on either side; a wall therefore narrows or splits the rows behind it.
    /// Pending rows are kept on a stack instead of recursing.
    fn scan(&mut self, quadrant: Quadrant) {
        let max_depth = max_depth(self.map, self.range, quadrant);
        if max_depth < 1 {
            return;
        }
        let mut rows = vec![Row {
            depth: 1,
            start: Slope::LEFT_DIAGONAL,
            end: Slope::RIGHT_DIAGONAL,
        }];
        while let Some(mut row) = rows.pop() {
            let deeper = row.depth < max_depth;
            // Whether the previous cell of the row blocked sight; `None`
            // before the first cell.
            let mut previous_blocks: Option<bool> = None;
            for column in row.columns() {
                let position = quadrant.transform(self.origin, row.depth, column);
                let blocks = self.map.blocks_sight(position);
                if (blocks || row.holds_centre(column))
                    && in_sight_range(self.origin, position, self.range)
                {
                    self.reveal(position);
                }
                match (previous_blocks, blocks) {
                    // Leaving a wall: the lit part of the row starts here.
                    (Some(true), false) => row.start = Slope::left_edge(row.depth, column),
                    // Entering a wall: the lit run before it continues behind.
                    (Some(false), true) if deeper => rows.push(Row {
                        end: Slope::left_edge(row.depth, column),
                        ..row.next()
                    }),
                    _ => {}
                }
                previous_blocks = Some(blocks);
            }
            if previous_blocks == Some(false) && deeper {
                rows.push(row.next());
            }
        }
    }

    /// Marks `position` visible, if it lies on the map.
    fn reveal(&mut self, position: Position) {
        if let Some(index) = self.map.index(position) {
            self.visible[index] = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::map::Tile;
    use crate::path::flood_fill;
    use crate::path::test_maps::from_rows;

    /// A range larger than any test map, so that only walls limit sight.
    const UNLIMITED: SightRange = SightRange::new(100, 100);

    /// Number of random maps the symmetry property is checked on.
    const PROPERTY_CASES: u32 = 500;

    /// Largest side of the random maps of the property tests: a bit more
    /// than twice the rows of [`SIGHT_RANGE`], so the range cuts sight on
    /// most maps while the check of every pair of cells stays fast.
    const MAX_RANDOM_SIDE: usize = 20;

    /// Whether `position` is visible from `origin` within `range`.
    fn sees(map: &Map, origin: Position, target: Position, range: SightRange) -> bool {
        visible_cells(map, origin, range).contains(&target)
    }

    /// A random range up to one and a half times [`SIGHT_RANGE`] on each
    /// axis, the two axes drawn apart: discs, wide and tall ellipses, and
    /// lines (an axis of 0) all occur.
    fn random_range() -> impl Strategy<Value = SightRange> {
        let columns = SIGHT_RANGE.columns * 3 / 2;
        let rows = SIGHT_RANGE.rows * 3 / 2;
        (0..=columns, 0..=rows).prop_map(|(columns, rows)| SightRange::new(columns, rows))
    }

    /// Shorthand for a position in the tests.
    const fn at(x: i32, y: i32) -> Position {
        Position::new(x, y)
    }

    /// A random map up to [`MAX_RANDOM_SIDE`] cells on each side, about one
    /// cell in three being a wall.
    fn random_map() -> impl Strategy<Value = Map> {
        (1..=MAX_RANDOM_SIDE, 1..=MAX_RANDOM_SIDE).prop_flat_map(|(width, height)| {
            prop::collection::vec(prop::bool::weighted(0.35), width * height).prop_map(
                move |walls| {
                    let mut map = Map::new(width, height, Tile::Floor);
                    let positions: Vec<Position> = map.positions().collect();
                    for (position, wall) in positions.into_iter().zip(walls) {
                        if wall {
                            map.set_tile(position, Tile::Wall);
                        }
                    }
                    map
                },
            )
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(PROPERTY_CASES))]

        /// If a floor cell sees another floor cell, the second one sees the
        /// first, whatever the walls and the range, elliptical or not. Every
        /// pair of walkable cells of each map is checked.
        #[test]
        fn sight_between_walkable_cells_is_symmetric(
            map in random_map(),
            range in random_range(),
        ) {
            let walkable: Vec<Position> =
                map.positions().filter(|&p| map.is_walkable(p)).collect();
            let fields: Vec<Vec<Position>> = walkable
                .iter()
                .map(|&origin| visible_cells(&map, origin, range))
                .collect();

            for (a, field_of_a) in walkable.iter().zip(&fields) {
                for (b, field_of_b) in walkable.iter().zip(&fields) {
                    prop_assert_eq!(
                        field_of_a.contains(b),
                        field_of_b.contains(a),
                        "{:?} and {:?} disagree ({:?})", a, b, range
                    );
                }
            }
        }

        /// The origin is always visible and nothing beyond the range is.
        #[test]
        fn sight_includes_the_origin_and_stays_in_range(
            map in random_map(),
            range in random_range(),
        ) {
            for origin in map.positions().filter(|&p| map.is_walkable(p)) {
                let field = visible_cells(&map, origin, range);

                prop_assert!(field.contains(&origin));
                for cell in field {
                    prop_assert!(
                        in_sight_range(origin, cell, range),
                        "{:?} seen from {:?} beyond {:?}", cell, origin, range
                    );
                }
            }
        }

        /// The range test gives the same answer from either cell, and the
        /// same for the mirror images of a gap.
        #[test]
        fn the_range_test_is_symmetric(
            range in random_range(),
            dx in -40..=40_i32,
            dy in -40..=40_i32,
        ) {
            let a = at(3, -7);
            let b = at(3 + dx, -7 + dy);

            prop_assert_eq!(in_sight_range(a, b, range), in_sight_range(b, a, range));
            prop_assert_eq!(range.reaches(dx, dy), range.reaches(-dx, dy));
            prop_assert_eq!(range.reaches(dx, dy), range.reaches(dx, -dy));
        }
    }

    #[test]
    fn a_range_with_equal_axes_is_the_former_disc_rule() {
        for radius in 0..=20_u16 {
            let range = SightRange::new(radius, radius);
            let squared = i32::from(radius) * i32::from(radius) + i32::from(radius);
            for dx in -25..=25 {
                for dy in -25..=25 {
                    assert_eq!(
                        range.reaches(dx, dy),
                        dx * dx + dy * dy <= squared,
                        "radius {radius}, gap ({dx}, {dy})"
                    );
                }
            }
        }
    }

    #[test]
    fn a_range_reaches_its_axes_exactly_and_never_beyond() {
        for columns in 0..=20_u16 {
            for rows in 0..=20_u16 {
                let range = SightRange::new(columns, rows);
                let (columns, rows) = (i32::from(columns), i32::from(rows));
                for sign in [-1, 1] {
                    assert!(range.reaches(sign * columns, 0), "{range:?}");
                    assert!(!range.reaches(sign * (columns + 1), 0), "{range:?}");
                    assert!(range.reaches(0, sign * rows), "{range:?}");
                    assert!(!range.reaches(0, sign * (rows + 1)), "{range:?}");
                }
            }
        }
    }

    #[test]
    fn the_largest_range_does_not_overflow() {
        let range = SightRange::new(u16::MAX, u16::MAX);
        let far = i32::from(u16::MAX);

        assert!(range.reaches(far, 0));
        assert!(range.reaches(0, -far));
        assert!(!range.reaches(far, far));
        assert!(!range.reaches(i32::MIN, i32::MAX));
    }

    #[test]
    fn an_empty_room_is_seen_entirely_walls_and_corners_included() {
        let map = from_rows(&[
            "###########", //
            "#.........#", //
            "#.........#", //
            "#.........#", //
            "#.........#", //
            "#.........#", //
            "###########", //
        ]);

        let field = visible_cells(&map, at(5, 3), UNLIMITED);

        assert_eq!(field, map.positions().collect::<Vec<_>>());
    }

    #[test]
    fn the_walls_bounding_the_fixed_room_are_seen_from_inside() {
        let map = Map::fixed_room();
        let origin = at(5, 5);

        let field = visible_cells(&map, origin, SIGHT_RANGE);

        // Every wall of the left side and the nearby top and bottom walls,
        // corners included, are in range and lit.
        for y in 0..=11 {
            assert!(field.contains(&at(0, y)), "left wall at row {y}");
        }
        for x in 0..=8 {
            assert!(field.contains(&at(x, 0)), "top wall at column {x}");
            assert!(field.contains(&at(x, 11)), "bottom wall at column {x}");
        }
    }

    #[test]
    fn in_an_open_area_the_player_sees_an_ellipse_16_across_and_8_up_and_down() {
        let map = Map::new(41, 21, Tile::Floor);
        let origin = at(20, 10);

        let field = visible_cells(&map, origin, SIGHT_RANGE);

        let expected: Vec<Position> = map
            .positions()
            .filter(|&p| in_sight_range(origin, p, SIGHT_RANGE))
            .collect();
        assert_eq!(field, expected);
        // Along the row, 16 cells on either side and not one more...
        for x in [20 - 16, 20 + 16] {
            assert!(field.contains(&at(x, 10)), "column {x}");
        }
        for x in [20 - 17, 20 + 17] {
            assert!(!field.contains(&at(x, 10)), "column {x}");
        }
        // ...along the column, 8 cells above and below and not one more...
        for y in [10 - 8, 10 + 8] {
            assert!(field.contains(&at(20, y)), "row {y}");
        }
        for y in [10 - 9, 10 + 9] {
            assert!(!field.contains(&at(20, y)), "row {y}");
        }
        // ...and in between the edge is rounded, not square.
        assert!(field.contains(&at(20 + 9, 10 + 7)));
        assert!(!field.contains(&at(20 + 10, 10 + 7)));
        assert!(field.contains(&at(20 - 16, 10 - 2)));
        assert!(!field.contains(&at(20 - 16, 10 - 3)));
        assert!(!field.contains(&at(20 + 16, 10 + 8)));
    }

    #[test]
    fn a_pillar_casts_a_widening_shadow() {
        // The pillar of the fixed room is at (15, 6); look at it from three
        // cells to its left, along its row.
        let map = Map::fixed_room();
        let origin = at(12, 6);

        let field = visible_cells(&map, origin, UNLIMITED);

        assert!(field.contains(&at(15, 6)), "the pillar itself is seen");
        // Right behind the pillar, only its own row is hidden...
        assert!(!field.contains(&at(16, 6)));
        assert!(field.contains(&at(16, 5)));
        assert!(field.contains(&at(16, 7)));
        // ...and farther away the shadow widens to the rows beside it.
        for y in 5..=7 {
            assert!(!field.contains(&at(22, y)), "row {y} behind the pillar");
        }
        assert!(field.contains(&at(22, 4)));
        assert!(field.contains(&at(22, 8)));
        // The wall at the end of the room is in the shadow too.
        assert!(!field.contains(&at(29, 6)));
    }

    #[test]
    fn a_corridor_is_seen_to_its_end_but_not_around_the_bend() {
        let map = from_rows(&[
            "##########", //
            "#........#", //
            "########.#", //
            "########.#", //
            "########.#", //
            "##########", //
        ]);
        let entrance = at(1, 1);

        let field = visible_cells(&map, entrance, UNLIMITED);

        for x in 0..=9 {
            assert!(field.contains(&at(x, 0)), "wall above, column {x}");
            assert!(field.contains(&at(x, 1)), "corridor, column {x}");
        }
        for hidden in [at(8, 3), at(8, 4)] {
            assert!(!field.contains(&hidden), "{hidden:?} is around the bend");
            assert!(!sees(&map, hidden, entrance, UNLIMITED));
        }
        // The far end of the corridor sees the bend in both directions.
        assert!(sees(&map, at(8, 4), at(8, 1), UNLIMITED));
        assert!(sees(&map, at(8, 1), at(8, 4), UNLIMITED));
    }

    #[test]
    fn the_origin_is_visible_even_when_walled_in_or_with_no_range() {
        let map = from_rows(&[
            "###", //
            "#.#", //
            "###", //
        ]);

        assert_eq!(
            visible_cells(&map, at(1, 1), UNLIMITED),
            map.positions().collect::<Vec<_>>()
        );
        assert_eq!(
            visible_cells(&map, at(1, 1), SightRange::new(0, 0)),
            [at(1, 1)]
        );
        assert_eq!(
            visible_cells(&Map::fixed_room(), at(5, 5), SightRange::new(0, 0)),
            [at(5, 5)]
        );
    }

    #[test]
    fn nothing_is_visible_from_outside_the_map() {
        let map = Map::fixed_room();

        assert!(visible_cells(&map, at(-1, 3), UNLIMITED).is_empty());
        assert!(visible_cells(&map, at(3, 30), UNLIMITED).is_empty());
    }

    #[test]
    fn sight_goes_through_a_diagonal_gap_an_actor_can_walk_through() {
        // The two walls at (2, 1) and (1, 2) touch by their corners, as do
        // those at (3, 2) and (2, 3); actors may step across both gaps.
        let map = from_rows(&[
            "#####", //
            "#.###", //
            "##.##", //
            "###.#", //
            "#####", //
        ]);
        let (near, far) = (at(1, 1), at(3, 3));
        assert!(flood_fill(&map, near).contains(&far));

        assert!(sees(&map, near, far, SIGHT_RANGE));
        assert!(sees(&map, far, near, SIGHT_RANGE));
    }

    #[test]
    fn sight_crosses_a_diagonal_wall_only_through_its_gaps() {
        // A wall running diagonally, each stone touching the next by a
        // corner only. Actors can step across it between any two stones.
        let map = from_rows(&[
            "............", //
            ".......#....", //
            "......#.....", //
            ".....#......", //
            "....#.......", //
            "...#........", //
            "..#.........", //
            "............", //
        ]);
        let origin = at(2, 1);
        assert!(flood_fill(&map, origin).contains(&at(8, 4)));

        let field = visible_cells(&map, origin, SIGHT_RANGE);

        // Thin lines of sight pass between the stones, from both sides...
        for beyond in [at(10, 2), at(8, 4), at(9, 4)] {
            assert!(field.contains(&beyond), "{beyond:?} through a gap");
            assert!(sees(&map, beyond, origin, SIGHT_RANGE));
        }
        // ...but the stones still hide most of what lies behind them.
        for hidden in [at(6, 4), at(5, 6), at(9, 2)] {
            assert!(!field.contains(&hidden), "{hidden:?} behind the wall");
        }
    }

    #[test]
    fn stairs_let_sight_through() {
        let mut map = from_rows(&[
            "#######", //
            "#.....#", //
            "#######", //
        ]);
        map.set_tile(at(3, 1), Tile::StairsDown);

        let field = visible_cells(&map, at(1, 1), UNLIMITED);

        assert!(field.contains(&at(3, 1)));
        assert!(field.contains(&at(5, 1)), "the stairs cast no shadow");
    }
}
