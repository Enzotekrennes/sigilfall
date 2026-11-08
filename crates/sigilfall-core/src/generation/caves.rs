//! Natural caves grown by a cellular automaton.
//!
//! Exposes [`carve`], which grows one cave into a map of solid rock, and
//! [`MIN_CAVE_CELLS`], the size a cave must reach. The map is first filled
//! with random rock and open cells ([`INITIAL_ROCK_PERCENT`]), then smoothed
//! [`SMOOTHING_PASSES`] times: at each pass, every cell becomes rock or open
//! ground depending on how many of its eight neighbours are rock
//! ([`ROCK_BIRTH_NEIGHBOURS`], [`ROCK_SURVIVAL_NEIGHBOURS`]). Scattered noise
//! turns into wide rounded halls studded with rock pillars and joined by
//! narrower passages.
//!
//! Smoothing leaves several pockets of open ground, cut off from each other.
//! Only the largest one is kept and the others are filled with rock, so the
//! cave is a single connected area under the movement rule of the game
//! (eight directions, corners may be cut: [`crate::map::Map::can_step`]).
//! A cave smaller than [`MIN_CAVE_CELLS`] is grown again from the same
//! stream, at most [`MAX_CAVE_ATTEMPTS`] times.
//!
//! Choosing the start and the stairs is left to the parent module.

use rand::RngExt as _;
use rand_chacha::ChaCha8Rng;

use crate::geometry::Position;
use crate::map::{Map, Tile};
use crate::path::flood_fill;

/// Chance, in percent, that a cell inside the border starts as rock.
///
/// 45 % is the usual balance for the rule of [`ROCK_BIRTH_NEIGHBOURS`] and
/// [`ROCK_SURVIVAL_NEIGHBOURS`]: lower, the rock dissolves into one open
/// plain; higher, it closes into small pockets cut off from each other (at
/// 50 %, the largest pocket of two caves in three falls below
/// [`MIN_CAVE_CELLS`]). At 45 %, on 2 000 streams, the largest pocket holds
/// 1 820 cells for the median cave, and halls alternate with pillars and
/// narrower passages.
const INITIAL_ROCK_PERCENT: u32 = 45;

/// Number of smoothing passes of the automaton.
///
/// Each pass rounds the walls a little more. Measured on 200 streams of an
/// 80 × 40 map, the first pass changes about 1 080 cells, the next ones 400,
/// 155, 77 and 47: after the fifth, under 2 % of the 2 964 cells inside the
/// border still change, only single corners. More passes would cost time
/// without changing the cave.
const SMOOTHING_PASSES: u32 = 5;

/// An open cell becomes rock when at least this many of its eight
/// neighbours are rock.
///
/// Five is a strict majority of the eight neighbours: open ground filled in
/// by rock on most sides is noise, and closes.
const ROCK_BIRTH_NEIGHBOURS: usize = 5;

/// A rock cell stays rock when at least this many of its eight neighbours
/// are rock; with fewer, it crumbles into open ground.
///
/// Four, one less than [`ROCK_BIRTH_NEIGHBOURS`]: rock already standing
/// holds with half its neighbours, which keeps the walls of the passages
/// from eroding pass after pass. With equal thresholds the caves would open
/// into a single plain.
const ROCK_SURVIVAL_NEIGHBOURS: usize = 4;

/// Fewest open cells a cave must keep once its smaller pockets are filled.
///
/// A floor of rooms has from about 640 to 1 600 walkable cells, 1 100 for
/// the median floor. A cave smaller than the smallest of them would crowd
/// up to 12 monsters (`generation::MAX_MONSTERS_PER_FLOOR`), glyphs and
/// items, all out of sight of the start, into a short crossing. 900 cells,
/// under a third of the 78 × 38 cells inside the border, leaves room for
/// all of them; with [`INITIAL_ROCK_PERCENT`], the largest pocket falls
/// below it for about one cave in a hundred (24 of 2 000 streams), so a
/// second attempt is rare.
pub(crate) const MIN_CAVE_CELLS: usize = 900;

/// Most caves grown for one floor before settling for the largest of them.
///
/// Attempts draw from the same stream one after the other, so the result
/// stays a pure function of the stream. With about one cave in a hundred
/// below [`MIN_CAVE_CELLS`], eight failures in a row have a chance of about
/// one in 10^16: the bound only guarantees that generation ends.
const MAX_CAVE_ATTEMPTS: u32 = 8;

/// Grows a cave into a `width` × `height` map of solid rock and returns the
/// map.
///
/// Every random choice is drawn from `rng` in a fixed order, so the same
/// stream always gives the same cave. The cells of the outer border are
/// always rock, the open cells ([`Tile::Floor`]) form a single area in which
/// every cell can be walked to from every other, and that area holds at
/// least [`MIN_CAVE_CELLS`] cells unless [`MAX_CAVE_ATTEMPTS`] attempts all
/// fell short, in which case the largest cave drawn is returned (the first
/// one on ties). The stream is left right after the draws of the attempt
/// that succeeded, or of the last attempt.
///
/// # Panics
///
/// Panics if a side is shorter than 3 cells (no cell inside the border) or
/// does not fit in an `i32`: generation is only ever asked for the fixed
/// size of a floor.
pub(crate) fn carve(width: usize, height: usize, rng: &mut ChaCha8Rng) -> Map {
    assert!(
        width >= 3 && height >= 3,
        "cannot grow a cave in a {width}x{height} map: each side needs at least 3 cells"
    );
    let mut best: Option<(Map, usize)> = None;
    for _ in 0..MAX_CAVE_ATTEMPTS {
        let rock = smooth(seed_rock(width, height, rng), width, height);
        let (map, size) = keep_largest_area(&rock, width, height);
        if size >= MIN_CAVE_CELLS {
            return map;
        }
        if best.as_ref().is_none_or(|&(_, best_size)| size > best_size) {
            best = Some((map, size));
        }
    }
    best.map(|(map, _)| map)
        .expect("at least one cave is grown")
}

/// Returns a rock grid of `width` × `height` cells in reading order, `true`
/// for rock: the border is rock, and each cell inside it is rock with a
/// chance of [`INITIAL_ROCK_PERCENT`], drawn from `rng` in reading order.
fn seed_rock(width: usize, height: usize, rng: &mut ChaCha8Rng) -> Vec<bool> {
    (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .map(|(x, y)| on_border(x, y, width, height) || rng.random_ratio(INITIAL_ROCK_PERCENT, 100))
        .collect()
}

/// Applies [`SMOOTHING_PASSES`] passes of the automaton to `rock`, a grid of
/// `width` × `height` cells, and returns the result.
///
/// Each pass reads the previous grid whole and writes a new one, so the
/// order cells are visited in does not matter. The border stays rock.
fn smooth(mut rock: Vec<bool>, width: usize, height: usize) -> Vec<bool> {
    for _ in 0..SMOOTHING_PASSES {
        let next = (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .map(|(x, y)| {
                on_border(x, y, width, height)
                    || stays_or_becomes_rock(
                        rock[y * width + x],
                        rock_neighbours(&rock, x, y, width),
                    )
            })
            .collect();
        rock = next;
    }
    rock
}

/// Returns whether a cell is rock after a pass, given whether it was rock
/// (`was_rock`) and how many of its eight neighbours were: rock holds with
/// [`ROCK_SURVIVAL_NEIGHBOURS`], open ground closes with
/// [`ROCK_BIRTH_NEIGHBOURS`].
const fn stays_or_becomes_rock(was_rock: bool, neighbours: usize) -> bool {
    if was_rock {
        neighbours >= ROCK_SURVIVAL_NEIGHBOURS
    } else {
        neighbours >= ROCK_BIRTH_NEIGHBOURS
    }
}

/// Counts the rock cells among the eight neighbours of the cell at (`x`,
/// `y`) of `rock`, a grid `width` cells wide; the cell must not lie on the
/// border, so every neighbour exists.
fn rock_neighbours(rock: &[bool], x: usize, y: usize, width: usize) -> usize {
    (y - 1..=y + 1)
        .flat_map(|ny| (x - 1..=x + 1).map(move |nx| (nx, ny)))
        .filter(|&(nx, ny)| (nx, ny) != (x, y) && rock[ny * width + nx])
        .count()
}

/// Returns whether the cell at (`x`, `y`) lies on the outer border of a
/// `width` × `height` grid.
const fn on_border(x: usize, y: usize, width: usize, height: usize) -> bool {
    x == 0 || y == 0 || x == width - 1 || y == height - 1
}

/// Builds the map of `rock`, a grid of `width` × `height` cells, keeping
/// only its largest open area and filling every other one with rock; returns
/// the map with the number of open cells left.
///
/// Areas are the sets of cells [`flood_fill`] joins, so they follow the
/// movement rule. On ties, the area whose first cell comes first in reading
/// order is kept, so the choice is deterministic.
fn keep_largest_area(rock: &[bool], width: usize, height: usize) -> (Map, usize) {
    let mut map = Map::new(width, height, Tile::Wall);
    for position in map.positions().collect::<Vec<_>>() {
        if map.index(position).is_some_and(|index| !rock[index]) {
            map.set_tile(position, Tile::Floor);
        }
    }

    let mut seen = vec![false; width * height];
    let mut largest: Vec<Position> = Vec::new();
    for position in map.positions() {
        let Some(index) = map.index(position) else {
            continue;
        };
        if rock[index] || seen[index] {
            continue;
        }
        let area = flood_fill(&map, position);
        for &cell in &area {
            if let Some(index) = map.index(cell) {
                seen[index] = true;
            }
        }
        if area.len() > largest.len() {
            largest = area;
        }
    }

    let mut cave = Map::new(width, height, Tile::Wall);
    for &cell in &largest {
        cave.set_tile(cell, Tile::Floor);
    }
    (cave, largest.len())
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng as _;

    use super::*;

    /// Width of the maps the tests grow caves in: that of a floor.
    const WIDTH: usize = 80;

    /// Height of the maps the tests grow caves in: that of a floor.
    const HEIGHT: usize = 40;

    /// Number of streams the cave rules are checked on here; the parent
    /// module checks whole floors on many more seeds.
    const STREAMS: u64 = 100;

    /// Every open cell of `map`, in reading order.
    fn open_cells(map: &Map) -> Vec<Position> {
        map.positions().filter(|&p| map.is_walkable(p)).collect()
    }

    #[test]
    fn a_cave_is_one_area_large_enough_inside_a_rock_border() {
        for value in 0..STREAMS {
            let map = carve(WIDTH, HEIGHT, &mut ChaCha8Rng::seed_from_u64(value));
            let open = open_cells(&map);

            assert!(
                open.len() >= MIN_CAVE_CELLS,
                "stream {value}: {}",
                open.len()
            );
            assert_eq!(flood_fill(&map, open[0]), open, "stream {value}");
            for position in map.positions() {
                let (x, y) = (
                    usize::try_from(position.x).unwrap(),
                    usize::try_from(position.y).unwrap(),
                );
                if on_border(x, y, WIDTH, HEIGHT) {
                    assert_eq!(map.tile(position), Some(Tile::Wall), "stream {value}");
                }
            }
        }
    }

    #[test]
    fn the_same_stream_grows_the_same_cave() {
        let grow = || carve(WIDTH, HEIGHT, &mut ChaCha8Rng::seed_from_u64(7));

        assert_eq!(grow(), grow());
        assert_ne!(
            grow(),
            carve(WIDTH, HEIGHT, &mut ChaCha8Rng::seed_from_u64(8))
        );
    }

    #[test]
    fn smoothing_follows_the_birth_and_survival_thresholds() {
        assert!(stays_or_becomes_rock(false, 5), "open ground closes at 5");
        assert!(!stays_or_becomes_rock(false, 4), "open ground stays at 4");
        assert!(stays_or_becomes_rock(true, 4), "rock holds at 4");
        assert!(!stays_or_becomes_rock(true, 3), "rock crumbles at 3");
    }

    #[test]
    fn neighbours_are_the_eight_cells_around() {
        // A 4 × 3 grid: rock in reading order, the cell (1, 1) has the rock
        // cells (0, 0), (2, 0), (0, 2) and (2, 1) around it; (3, 1) is two
        // cells away and (1, 1) itself does not count.
        let rows = [
            "#.#.", //
            ".###", //
            "#...", //
        ];
        let rock: Vec<bool> = rows
            .iter()
            .flat_map(|row| row.chars().map(|glyph| glyph == '#'))
            .collect();

        assert_eq!(rock_neighbours(&rock, 1, 1, 4), 4);
    }

    #[test]
    fn a_pass_keeps_the_border_as_rock_and_fills_isolated_holes() {
        // A single open cell in solid rock has eight rock neighbours: it
        // closes, and the border never opens.
        let mut rock = vec![true; 25];
        rock[12] = false;

        assert!(smooth(rock, 5, 5).iter().all(|&cell| cell));
    }

    #[test]
    fn only_the_largest_area_is_kept() {
        // Two pockets: 3 open cells on the left, 2 on the right.
        let rows = [
            "#######", //
            "#...#.#", //
            "#####.#", //
            "#######", //
        ];
        let rock: Vec<bool> = rows
            .iter()
            .flat_map(|row| row.chars().map(|glyph| glyph == '#'))
            .collect();

        let (map, size) = keep_largest_area(&rock, 7, 4);

        assert_eq!(size, 3);
        assert_eq!(
            open_cells(&map),
            [(1, 1), (2, 1), (3, 1)].map(|(x, y)| Position::new(x, y))
        );
    }

    #[test]
    fn areas_touching_by_a_corner_count_as_one() {
        // Joined through the corner between (2, 1) and (3, 2), as an actor
        // may step.
        let rows = [
            "######", //
            "#..###", //
            "###..#", //
            "######", //
        ];
        let rock: Vec<bool> = rows
            .iter()
            .flat_map(|row| row.chars().map(|glyph| glyph == '#'))
            .collect();

        let (_, size) = keep_largest_area(&rock, 6, 4);

        assert_eq!(size, 4);
    }

    #[test]
    #[should_panic(expected = "cannot grow a cave in a 2x40 map")]
    fn a_map_without_inner_cells_is_refused() {
        carve(2, 40, &mut ChaCha8Rng::seed_from_u64(0));
    }
}
