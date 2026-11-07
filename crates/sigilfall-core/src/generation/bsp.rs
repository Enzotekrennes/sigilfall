//! Rooms and corridors laid out by binary space partitioning (BSP).
//!
//! Exposes [`carve`], which digs a floor of rooms and corridors into solid
//! rock, and [`Rect`], the rectangles it works with. The area of the floor
//! is cut in two, each half is cut again, and so on; every final part (a
//! leaf of the cutting tree) receives one room, and on the way back up each
//! pair of sister parts is joined by one corridor. Because every cut is
//! joined exactly once, all rooms end up connected.
//!
//! Choosing the start and the stairs is left to the parent module.

use rand::RngExt as _;
use rand_chacha::ChaCha8Rng;

use crate::geometry::Position;
use crate::map::{Map, Tile};

/// Smallest width and height of a room, in floor cells.
///
/// Four cells leave room for a monster, an item and the player to stand
/// apart in the smallest room, and keep rooms recognisable next to
/// one-cell-wide corridors.
const MIN_ROOM_SIDE: i32 = 4;

/// Wall cells kept between a room and the edge of its part.
///
/// One cell on each side means two rooms in neighbouring parts are always
/// separated by at least two wall cells, so they never merge, and rooms on
/// the edge of the floor never touch its outer border.
const ROOM_MARGIN: i32 = 1;

/// Smallest width and height of a part: the smallest room plus its margin
/// on both sides. A part is cut only if both halves keep at least this size.
const MIN_PART_SIDE: i32 = MIN_ROOM_SIDE + 2 * ROOM_MARGIN;

/// Number of successive cuts from the whole floor down to a leaf.
///
/// Four levels give at most 16 rooms. On an 80 × 40 floor, that makes parts
/// of about 20 × 10 cells, which leaves rooms of a comfortable size with
/// enough corridors between them; a fifth level would crowd the floor with
/// rooms barely larger than [`MIN_ROOM_SIDE`].
const MAX_CUT_DEPTH: u32 = 4;

/// How many times taller than wide a terminal cell is drawn, roughly.
///
/// Parts are compared by their size on screen, not in cells: an 80 × 40
/// area looks square in a terminal, and cutting by on-screen size keeps
/// rooms from looking stretched.
const CELL_ASPECT: i32 = 2;

/// A part is called elongated, and cut across its long side, when that side
/// is more than `ELONGATION_NUMERATOR / ELONGATION_DENOMINATOR` times its
/// short side on screen. Below that ratio the direction of the cut is
/// random, which varies the layouts.
const ELONGATION_NUMERATOR: i32 = 3;

/// See [`ELONGATION_NUMERATOR`].
const ELONGATION_DENOMINATOR: i32 = 2;

/// Fraction of a side, from each end, where no cut may fall: cuts land in
/// the middle third, so the two halves stay comparable and no room is
/// squeezed next to a much larger neighbour.
const CUT_EDGE_DIVISOR: i32 = 3;

/// An axis-aligned rectangle of cells: a part of the floor or a room.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Rect {
    /// Column of the left-most cells.
    pub(crate) x: i32,
    /// Row of the top-most cells.
    pub(crate) y: i32,
    /// Number of columns; always positive.
    pub(crate) width: i32,
    /// Number of rows; always positive.
    pub(crate) height: i32,
}

impl Rect {
    /// Creates the rectangle whose top-left cell is `(x, y)`.
    pub(crate) const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Returns the central cell, rounded towards the top-left for even
    /// sides.
    pub(crate) const fn center(self) -> Position {
        Position::new(
            self.x + (self.width - 1) / 2,
            self.y + (self.height - 1) / 2,
        )
    }

    /// Returns every cell of the rectangle in reading order.
    pub(crate) fn cells(self) -> impl Iterator<Item = Position> {
        (self.y..self.y + self.height)
            .flat_map(move |y| (self.x..self.x + self.width).map(move |x| Position::new(x, y)))
    }

    /// Draws one cell of the rectangle uniformly from `rng`.
    pub(crate) fn random_cell(self, rng: &mut ChaCha8Rng) -> Position {
        Position::new(
            rng.random_range(self.x..self.x + self.width),
            rng.random_range(self.y..self.y + self.height),
        )
    }
}

/// Direction of a cut through a part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cut {
    /// A vertical line: the halves lie side by side, left then right.
    Vertical,
    /// A horizontal line: the halves lie one above the other, top then
    /// bottom.
    Horizontal,
}

/// Digs rooms and corridors into a `width` × `height` map of solid rock and
/// returns the map with its rooms, in the order the cutting tree visits its
/// leaves (first half before second half).
///
/// Every random choice is drawn from `rng` in a fixed order, so the same
/// stream always gives the same layout. Rooms are at least
/// [`MIN_ROOM_SIDE`] cells wide and high, never touch each other or the
/// outer border, and every floor cell is connected to every other one.
///
/// # Panics
///
/// Panics if the map is smaller than one part ([`MIN_PART_SIDE`] cells on
/// each side) or does not fit in an `i32`: generation is only ever asked for
/// the fixed size of a floor.
pub(crate) fn carve(width: usize, height: usize, rng: &mut ChaCha8Rng) -> (Map, Vec<Rect>) {
    let area = Rect::new(
        0,
        0,
        i32::try_from(width).expect("floor width fits in i32"),
        i32::try_from(height).expect("floor height fits in i32"),
    );
    assert!(
        area.width >= MIN_PART_SIDE && area.height >= MIN_PART_SIDE,
        "cannot lay out a {width}x{height} floor: each side needs at least {MIN_PART_SIDE} cells"
    );
    let mut map = Map::new(width, height, Tile::Wall);
    let rooms = carve_part(area, 0, &mut map, rng);
    (map, rooms)
}

/// Lays out the part `area` at `depth` cuts from the whole floor and returns
/// its rooms.
///
/// A part that may still be cut is split in two, each half is laid out in
/// turn, and the two halves are joined by one corridor. Otherwise the part is
/// a leaf and receives a single room.
fn carve_part(area: Rect, depth: u32, map: &mut Map, rng: &mut ChaCha8Rng) -> Vec<Rect> {
    let halves = if depth < MAX_CUT_DEPTH {
        split(area, rng)
    } else {
        None
    };
    let Some((first, second)) = halves else {
        let room = place_room(area, rng);
        dig(map, room.cells());
        return vec![room];
    };

    let mut rooms = carve_part(first, depth + 1, map, rng);
    let second_rooms = carve_part(second, depth + 1, map, rng);
    join(&rooms, &second_rooms, map, rng);
    rooms.extend(second_rooms);
    rooms
}

/// Cuts `area` in two, or returns `None` when neither direction leaves two
/// halves of at least [`MIN_PART_SIDE`] cells.
///
/// The preferred direction is chosen by [`preferred_cut`]; when the part is
/// too narrow for it, the other direction is tried.
fn split(area: Rect, rng: &mut ChaCha8Rng) -> Option<(Rect, Rect)> {
    let preferred = preferred_cut(area, rng);
    let other = match preferred {
        Cut::Vertical => Cut::Horizontal,
        Cut::Horizontal => Cut::Vertical,
    };
    [preferred, other]
        .into_iter()
        .find_map(|cut| split_along(area, cut, rng))
}

/// Chooses which way to cut `area`: across its long side when it is
/// elongated on screen (see [`ELONGATION_NUMERATOR`]), at random otherwise.
fn preferred_cut(area: Rect, rng: &mut ChaCha8Rng) -> Cut {
    let screen_width = area.width;
    let screen_height = area.height * CELL_ASPECT;
    if screen_width * ELONGATION_DENOMINATOR > screen_height * ELONGATION_NUMERATOR {
        Cut::Vertical
    } else if screen_height * ELONGATION_DENOMINATOR > screen_width * ELONGATION_NUMERATOR {
        Cut::Horizontal
    } else if rng.random() {
        Cut::Vertical
    } else {
        Cut::Horizontal
    }
}

/// Cuts `area` with a `cut` line at a random offset in the middle third of
/// the side it crosses (see [`CUT_EDGE_DIVISOR`]), or returns `None` when
/// that side is shorter than two minimal parts.
fn split_along(area: Rect, cut: Cut, rng: &mut ChaCha8Rng) -> Option<(Rect, Rect)> {
    let side = match cut {
        Cut::Vertical => area.width,
        Cut::Horizontal => area.height,
    };
    if side < 2 * MIN_PART_SIDE {
        return None;
    }
    let edge = side / CUT_EDGE_DIVISOR;
    let lowest = MIN_PART_SIDE.max(edge);
    let highest = (side - MIN_PART_SIDE).min(side - edge);
    let offset = rng.random_range(lowest..=highest);

    let Rect {
        x,
        y,
        width,
        height,
    } = area;
    Some(match cut {
        Cut::Vertical => (
            Rect::new(x, y, offset, height),
            Rect::new(x + offset, y, width - offset, height),
        ),
        Cut::Horizontal => (
            Rect::new(x, y, width, offset),
            Rect::new(x, y + offset, width, height - offset),
        ),
    })
}

/// Places a room of random size and position inside the leaf `area`,
/// keeping [`ROOM_MARGIN`] wall cells to each of its edges.
fn place_room(area: Rect, rng: &mut ChaCha8Rng) -> Rect {
    let width = rng.random_range(MIN_ROOM_SIDE..=area.width - 2 * ROOM_MARGIN);
    let height = rng.random_range(MIN_ROOM_SIDE..=area.height - 2 * ROOM_MARGIN);
    let x = rng.random_range(area.x + ROOM_MARGIN..=area.x + area.width - ROOM_MARGIN - width);
    let y = rng.random_range(area.y + ROOM_MARGIN..=area.y + area.height - ROOM_MARGIN - height);
    Rect::new(x, y, width, height)
}

/// Joins two sister parts with one corridor, from a room of `first` to a
/// room of `second`.
///
/// The two rooms are the closest pair across the cut (by distance between
/// their centres, the first pair found winning ties), so the corridor stays
/// short. It runs between a random cell of each room, in an L shape whose
/// bend is randomly placed before or after the horizontal leg. Both ends lie
/// inside rooms, so the corridor never leaves the area of the two parts and
/// never reaches the outer border.
fn join(first: &[Rect], second: &[Rect], map: &mut Map, rng: &mut ChaCha8Rng) {
    let Some((from_room, to_room)) = closest_pair(first, second) else {
        return;
    };
    let from = from_room.random_cell(rng);
    let to = to_room.random_cell(rng);
    let bend = if rng.random() {
        Position::new(to.x, from.y)
    } else {
        Position::new(from.x, to.y)
    };
    dig(map, straight_line(from, bend));
    dig(map, straight_line(bend, to));
}

/// Returns the room of `first` and the room of `second` whose centres are
/// the closest (Manhattan distance), or `None` if either list is empty.
fn closest_pair(first: &[Rect], second: &[Rect]) -> Option<(Rect, Rect)> {
    let distance = |a: Rect, b: Rect| {
        let (a, b) = (a.center(), b.center());
        (a.x - b.x).abs() + (a.y - b.y).abs()
    };
    let mut best: Option<(Rect, Rect)> = None;
    for &a in first {
        for &b in second {
            if best.is_none_or(|(x, y)| distance(a, b) < distance(x, y)) {
                best = Some((a, b));
            }
        }
    }
    best
}

/// Returns the cells of the horizontal or vertical segment from `from` to
/// `to`, both ends included, in reading order whatever the direction.
///
/// The two cells must share a row or a column (checked in debug builds);
/// otherwise every cell of the rectangle between them would be returned.
fn straight_line(from: Position, to: Position) -> impl Iterator<Item = Position> {
    debug_assert!(
        from.x == to.x || from.y == to.y,
        "corridor legs are straight"
    );
    let left = from.x.min(to.x);
    let top = from.y.min(to.y);
    Rect::new(
        left,
        top,
        from.x.max(to.x) - left + 1,
        from.y.max(to.y) - top + 1,
    )
    .cells()
}

/// Turns every cell of `cells` into floor.
fn dig(map: &mut Map, cells: impl Iterator<Item = Position>) {
    for cell in cells {
        map.set_tile(cell, Tile::Floor);
    }
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng as _;

    use super::*;
    use crate::path::flood_fill;

    /// Width of the floors laid out by these tests: the real floor size.
    const WIDTH: usize = 80;
    /// Height of the floors laid out by these tests.
    const HEIGHT: usize = 40;

    /// Lays out a floor from a stream seeded with `seed`.
    fn layout(seed: u64) -> (Map, Vec<Rect>) {
        carve(WIDTH, HEIGHT, &mut ChaCha8Rng::seed_from_u64(seed))
    }

    /// Returns whether `position` is one of the cells of `rect`.
    const fn contains(rect: Rect, position: Position) -> bool {
        position.x >= rect.x
            && position.x < rect.x + rect.width
            && position.y >= rect.y
            && position.y < rect.y + rect.height
    }

    /// Returns `room` grown by `cells` on every side.
    const fn grown(room: Rect, cells: i32) -> Rect {
        Rect::new(
            room.x - cells,
            room.y - cells,
            room.width + 2 * cells,
            room.height + 2 * cells,
        )
    }

    #[test]
    fn rooms_have_the_minimum_size_and_stay_off_the_border() {
        for seed in 0..200 {
            let (map, rooms) = layout(seed);
            let inner = Rect::new(1, 1, 78, 38);
            for room in rooms {
                assert!(room.width >= MIN_ROOM_SIDE && room.height >= MIN_ROOM_SIDE);
                assert!(room.cells().all(|cell| contains(inner, cell)), "{room:?}");
                assert!(room.cells().all(|cell| map.tile(cell) == Some(Tile::Floor)));
            }
        }
    }

    #[test]
    fn rooms_never_overlap_or_touch() {
        for seed in 0..200 {
            let (_, rooms) = layout(seed);
            for (index, &a) in rooms.iter().enumerate() {
                for &b in &rooms[index + 1..] {
                    // Two wall cells separate rooms: growing one room by one
                    // cell must still leave the other untouched.
                    let halo = grown(a, 1);
                    assert!(
                        b.cells().all(|cell| !contains(halo, cell)),
                        "seed {seed}: {a:?} touches {b:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_cutting_tree_gives_between_eight_and_sixteen_rooms() {
        // 80 × 40 can always be cut three times (parts stay at least 12
        // wide on screen), so at least 8 leaves; four cuts give at most 16.
        for seed in 0..200 {
            let (_, rooms) = layout(seed);
            assert!(
                (8..=16).contains(&rooms.len()),
                "seed {seed}: {}",
                rooms.len()
            );
        }
    }

    #[test]
    fn every_floor_cell_is_connected() {
        for seed in 0..200 {
            let (map, rooms) = layout(seed);
            let floor = map.positions().filter(|&p| map.is_walkable(p)).count();

            assert_eq!(
                flood_fill(&map, rooms[0].center()).len(),
                floor,
                "seed {seed}"
            );
        }
    }

    #[test]
    fn cuts_fall_in_the_middle_third_and_keep_minimal_parts() {
        let mut rng = ChaCha8Rng::seed_from_u64(7);
        let area = Rect::new(0, 0, 30, 12);
        for _ in 0..500 {
            let (left, right) = split_along(area, Cut::Vertical, &mut rng).unwrap();
            assert!((10..=20).contains(&left.width), "{left:?}");
            assert_eq!(left.width + right.width, 30);
            assert_eq!(right.x, left.width);

            let (top, bottom) = split_along(area, Cut::Horizontal, &mut rng).unwrap();
            assert_eq!(top.height, MIN_PART_SIDE, "only one cut leaves two parts");
            assert_eq!(bottom.height, 12 - MIN_PART_SIDE);
        }
    }

    #[test]
    fn a_part_too_small_for_two_halves_is_not_cut() {
        let mut rng = ChaCha8Rng::seed_from_u64(7);

        assert_eq!(split(Rect::new(0, 0, 11, 11), &mut rng), None);
        assert_eq!(
            split_along(Rect::new(0, 0, 11, 40), Cut::Vertical, &mut rng),
            None
        );
        assert!(split(Rect::new(0, 0, 11, 12), &mut rng).is_some());
    }

    #[test]
    fn elongated_parts_are_cut_across_their_long_side_on_screen() {
        let mut rng = ChaCha8Rng::seed_from_u64(7);
        for _ in 0..100 {
            // 40 × 10 is 40 by 20 on screen: wide.
            assert_eq!(
                preferred_cut(Rect::new(0, 0, 40, 10), &mut rng),
                Cut::Vertical
            );
            // 20 × 20 is 20 by 40 on screen: tall.
            assert_eq!(
                preferred_cut(Rect::new(0, 0, 20, 20), &mut rng),
                Cut::Horizontal
            );
        }
        // 40 × 20 looks square: both directions occur.
        let cuts: Vec<Cut> = (0..100)
            .map(|_| preferred_cut(Rect::new(0, 0, 40, 20), &mut rng))
            .collect();
        assert!(cuts.contains(&Cut::Vertical) && cuts.contains(&Cut::Horizontal));
    }

    #[test]
    fn rooms_keep_their_margin_inside_the_leaf() {
        let mut rng = ChaCha8Rng::seed_from_u64(7);
        let leaf = Rect::new(10, 5, 8, 6);
        let inner = grown(leaf, -ROOM_MARGIN);
        for _ in 0..500 {
            let room = place_room(leaf, &mut rng);
            assert!(room.cells().all(|cell| contains(inner, cell)), "{room:?}");
            assert!(room.width >= MIN_ROOM_SIDE && room.height >= MIN_ROOM_SIDE);
        }
    }

    #[test]
    fn closest_pair_takes_the_nearest_rooms_across_the_cut() {
        let far = Rect::new(1, 1, 4, 4);
        let near = Rect::new(20, 1, 4, 4);
        let other = Rect::new(26, 1, 4, 4);
        let distant = Rect::new(60, 1, 4, 4);

        assert_eq!(
            closest_pair(&[far, near], &[distant, other]),
            Some((near, other))
        );
        assert_eq!(closest_pair(&[], &[other]), None);
    }

    #[test]
    fn straight_lines_include_both_ends_in_any_direction() {
        let line: Vec<Position> = straight_line(Position::new(5, 2), Position::new(3, 2)).collect();
        assert_eq!(
            line,
            [(3, 2), (4, 2), (5, 2)].map(|(x, y)| Position::new(x, y))
        );

        let column: Vec<Position> =
            straight_line(Position::new(1, 1), Position::new(1, 3)).collect();
        assert_eq!(
            column,
            [(1, 1), (1, 2), (1, 3)].map(|(x, y)| Position::new(x, y))
        );
    }

    #[test]
    #[should_panic(expected = "cannot lay out a 5x40 floor")]
    fn a_floor_narrower_than_one_part_is_refused() {
        carve(5, 40, &mut ChaCha8Rng::seed_from_u64(0));
    }
}
