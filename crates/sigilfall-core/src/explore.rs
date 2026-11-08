//! Automatic exploration: which way the player goes to see more of the floor.
//!
//! Exposes [`explore_step`], the direction of one step towards the nearest
//! unexplored cell, [`ExploreStep`], its answer, and [`frontier`], the cells
//! it heads for. The `game` module turns the answer into `Action::Explore`.
//!
//! The goals are the *frontier* cells: cells of the map the player has
//! never seen (not remembered, see `Map::is_remembered`) that touch, by a
//! side or a corner, a remembered walkable cell. They are the edge of what
//! the player knows that one step from known ground can reach; the player
//! cannot tell what lies there, so any of them may open onto more of the
//! floor. Unseen cells deeper inside the unknown are not goals: nothing
//! known leads to them yet.
//!
//! The way is planned on what the player knows only, never on the true
//! floor: remembered cells keep their tile, frontier cells count as open
//! (the hope that makes them worth a visit) and every other unseen cell as
//! rock. A [`DijkstraMap`] from all frontier cells at once then gives, from
//! the player's cell, the step that brings it closer to the nearest of
//! them. Every cell next to the player is always in its field of view
//! (`fov` module), so that step always lands on a remembered cell whose
//! tile is known, and walking downhill reveals the frontier long before
//! the player would stand on it.
//!
//! This module only decides; it moves nothing, spends no time and knows
//! nothing of the actors but the cells they hold, given by the caller.

use crate::geometry::{Direction, Position};
use crate::map::{Map, Tile};
use crate::path::DijkstraMap;

/// What one step of automatic exploration does, from a given cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExploreStep {
    /// Step this way: one step closer to the nearest frontier cell, onto a
    /// free cell.
    Step(Direction),
    /// The way to the nearest frontier cell goes this way, but every step
    /// closer to it is held by an actor; nothing should move.
    Blocked(Direction),
    /// No frontier cell can be reached from here: the floor is explored as
    /// far as the player can walk.
    Explored,
}

/// Returns the frontier cells of `map`, the goals of automatic exploration,
/// in reading order (row by row from the top, left to right): cells never
/// seen by the player that touch a remembered walkable cell by a side or a
/// corner.
///
/// A floor seen in full, or not seen at all, has none.
pub fn frontier(map: &Map) -> Vec<Position> {
    map.positions()
        .filter(|&cell| is_frontier(map, cell))
        .collect()
}

/// Returns the step of automatic exploration for the player standing at
/// `from` on `map`, planned on what the player remembers of `map` (see the
/// module documentation).
///
/// `occupied` tells which cells hold an actor; `from` itself may be
/// included. The step never enters such a cell, so exploring never strikes
/// a monster.
///
/// The answer is a pure function of the tiles, of the cells remembered and
/// of `occupied`, so the same floor seen the same way gives the same step:
/// - [`ExploreStep::Step`] with the first direction, clockwise from north
///   ([`Direction::ALL`]), that brings the player one step closer to the
///   nearest frontier cell and is free; when several frontier cells are
///   equally near, the step leads towards one of them, decided by that
///   order alone;
/// - [`ExploreStep::Blocked`] with the first direction that brings it
///   closer, when every such direction is held by an actor;
/// - [`ExploreStep::Explored`] when there is no frontier cell, or none the
///   player can walk to over remembered cells, and when `from` is not a
///   remembered walkable cell (nothing known leads anywhere from there).
///
/// Costs one pass over the map to find the frontier and one breadth-first
/// wave over it ([`DijkstraMap::new`]): linear in the size of the map.
pub fn explore_step(map: &Map, from: Position, occupied: impl Fn(Position) -> bool) -> ExploreStep {
    if !map.is_remembered(from) {
        return ExploreStep::Explored;
    }
    let (known, goals) = known_map(map);
    if goals.is_empty() {
        return ExploreStep::Explored;
    }
    let distances = DijkstraMap::new(&known, goals);
    let Some(current) = distances.distance(from) else {
        return ExploreStep::Explored;
    };
    let mut closer = Direction::ALL.into_iter().filter(|&direction| {
        known.can_step(from, direction)
            && distances
                .distance(from.step(direction))
                .is_some_and(|distance| distance < current)
    });
    let Some(first) = closer.next() else {
        // Only a frontier cell has no way down, and `from` is remembered.
        return ExploreStep::Explored;
    };
    if !occupied(from.step(first)) {
        return ExploreStep::Step(first);
    }
    closer
        .find(|&direction| !occupied(from.step(direction)))
        .map_or(ExploreStep::Blocked(first), ExploreStep::Step)
}

/// Returns whether `cell` is a frontier cell of `map`: inside the map,
/// never seen, and next to a remembered walkable cell.
fn is_frontier(map: &Map, cell: Position) -> bool {
    map.index(cell).is_some()
        && !map.is_remembered(cell)
        && Direction::ALL.into_iter().any(|direction| {
            let neighbour = cell.step(direction);
            map.is_remembered(neighbour) && map.is_walkable(neighbour)
        })
}

/// Returns the floor as the player knows it, to plan on, with its frontier
/// cells in reading order: remembered cells keep their tile, frontier cells
/// are open floor and every other cell is a wall.
///
/// One pass over `map` gives both, so a step of exploration reads the map
/// once before its wave ([`DijkstraMap::new`]).
fn known_map(map: &Map) -> (Map, Vec<Position>) {
    let mut known = Map::new(map.width(), map.height(), Tile::Wall);
    let mut goals = Vec::new();
    for cell in map.positions() {
        if map.is_remembered(cell) {
            if let Some(tile) = map.tile(cell) {
                known.set_tile(cell, tile);
            }
        } else if is_frontier(map, cell) {
            known.set_tile(cell, Tile::Floor);
            goals.push(cell);
        }
    }
    (known, goals)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::test_maps::from_rows;

    /// Shorthand for a position in the test maps.
    const fn at(x: i32, y: i32) -> Position {
        Position::new(x, y)
    }

    /// A predicate under which no cell is occupied.
    const fn nobody(_: Position) -> bool {
        false
    }

    /// Builds the map drawn by `rows` (`#` a wall, anything else floor) and
    /// marks as remembered every cell whose glyph in `seen` is not `?`.
    ///
    /// # Panics
    ///
    /// Panics if `seen` does not have the shape of `rows`.
    fn seen_map(rows: &[&str], seen: &[&str]) -> Map {
        let mut map = from_rows(rows);
        let remembered: Vec<Position> = map
            .positions()
            .filter(|cell| {
                let row = seen[usize::try_from(cell.y).expect("on the map")];
                row.chars()
                    .nth(usize::try_from(cell.x).expect("on the map"))
                    != Some('?')
            })
            .collect();
        map.set_visible(remembered);
        map
    }

    /// Follows the steps of [`explore_step`] from `from` without revealing
    /// anything, as long as they land on remembered cells, and returns the
    /// last cell reached: the cell next to the frontier where, in a game,
    /// the field of view would already show what lies beyond. Panics after
    /// `limit` steps.
    fn walk(map: &Map, mut from: Position, limit: usize) -> Position {
        for _ in 0..limit {
            match explore_step(map, from, nobody) {
                ExploreStep::Step(direction) if map.is_remembered(from.step(direction)) => {
                    from = from.step(direction);
                }
                ExploreStep::Step(_) | ExploreStep::Blocked(_) | ExploreStep::Explored => {
                    return from;
                }
            }
        }
        panic!("still walking after {limit} steps");
    }

    #[test]
    fn the_frontier_is_the_unseen_cells_next_to_known_walkable_ones() {
        let map = seen_map(
            &[
                "#######", //
                "#.....#", //
                "#######", //
            ],
            &[
                "###????", //
                "#..????", //
                "###????", //
            ],
        );

        // (3, 0) and (3, 2) are walls the player has not seen: they are on
        // the frontier too, since nothing known says what they are.
        assert_eq!(frontier(&map), [at(3, 0), at(3, 1), at(3, 2)]);
    }

    #[test]
    fn unseen_cells_next_to_known_walls_only_are_not_on_the_frontier() {
        let map = seen_map(
            &[
                "....", //
                "#...", //
            ],
            &[
                "??..", //
                "#?..", //
            ],
        );

        // (0, 0) touches the known wall at (0, 1) only.
        assert_eq!(frontier(&map), [at(1, 0), at(1, 1)]);
    }

    #[test]
    fn a_floor_seen_in_full_or_not_at_all_has_no_frontier() {
        let rows = [
            "#####", //
            "#...#", //
            "#####", //
        ];
        let all = seen_map(&rows, &rows);
        let none = seen_map(&rows, &["?????", "?????", "?????"]);

        assert!(frontier(&all).is_empty());
        assert!(frontier(&none).is_empty());
        assert_eq!(explore_step(&all, at(2, 1), nobody), ExploreStep::Explored);
        assert_eq!(explore_step(&none, at(2, 1), nobody), ExploreStep::Explored);
    }

    #[test]
    fn it_heads_for_the_nearest_unexplored_cell() {
        // A corridor seen from x = 2 to x = 13: from x = 4, the unknown
        // begins 3 steps away on the west side and 10 on the east side.
        let map = seen_map(
            &[
                "################", //
                "#..............#", //
                "################", //
            ],
            &[
                "??############??", //
                "??............??", //
                "??############??", //
            ],
        );
        let from = at(4, 1);

        assert_eq!(
            explore_step(&map, from, nobody),
            ExploreStep::Step(Direction::West)
        );
        assert_eq!(walk(&map, from, 20), at(2, 1), "next to the frontier");
    }

    #[test]
    fn an_unseen_dead_end_is_explored_without_peeking_at_its_wall() {
        // The corridor ends in a wall at (8, 1), but the player has not seen
        // it: for all the player knows the corridor goes on, so its end is
        // worth a visit. Planning on the true floor would skip that wall.
        let map = seen_map(
            &[
                "#########", //
                "#.......#", //
                "#########", //
            ],
            &[
                "########?", //
                "#.......?", //
                "########?", //
            ],
        );

        assert_eq!(frontier(&map), [at(8, 0), at(8, 1), at(8, 2)]);
        assert_eq!(
            explore_step(&map, at(2, 1), nobody),
            ExploreStep::Step(Direction::East)
        );
        assert_eq!(walk(&map, at(2, 1), 20), at(7, 1));
    }

    #[test]
    fn ties_between_equally_near_cells_are_broken_clockwise_from_north() {
        // The unknown lies 3 steps north and 3 steps south.
        let map = seen_map(
            &[
                "###.###", //
                "#.....#", //
                "#.....#", //
                "#.....#", //
                "###.###", //
            ],
            &[
                "###?###", //
                "#.....#", //
                "#.....#", //
                "#.....#", //
                "###?###", //
            ],
        );

        assert_eq!(
            explore_step(&map, at(3, 2), nobody),
            ExploreStep::Step(Direction::North)
        );
        assert_eq!(
            explore_step(&map, at(1, 2), nobody),
            ExploreStep::Step(Direction::NorthEast)
        );
    }

    #[test]
    fn the_same_floor_seen_the_same_way_gives_the_same_step() {
        let rows = [
            "##########", //
            "#........#", //
            "#..#..#..#", //
            "#........#", //
            "##########", //
        ];
        let seen = [
            "?#########", //
            "?........#", //
            "?..#..#..#", //
            "?........#", //
            "??########", //
        ];
        let first = seen_map(&rows, &seen);
        let second = seen_map(&rows, &seen);

        for cell in first.positions() {
            assert_eq!(
                explore_step(&first, cell, nobody),
                explore_step(&second, cell, nobody),
                "from {cell:?}"
            );
        }
    }

    #[test]
    fn it_steps_around_an_actor_on_the_way() {
        let map = seen_map(
            &[
                "#######", //
                "#.....#", //
                "#.....#", //
                "#.....#", //
                "#######", //
            ],
            &[
                "#######", //
                "?.....#", //
                "?.....#", //
                "?.....#", //
                "#######", //
            ],
        );
        // South-west, west and north-west all lead one step closer, in that
        // order; the first of them is held.
        assert_eq!(
            explore_step(&map, at(4, 2), nobody),
            ExploreStep::Step(Direction::SouthWest)
        );
        let occupied = |cell: Position| cell == at(3, 3);

        assert_eq!(
            explore_step(&map, at(4, 2), occupied),
            ExploreStep::Step(Direction::West)
        );
    }

    #[test]
    fn it_is_blocked_when_every_step_closer_is_held() {
        let map = seen_map(
            &[
                "#######", //
                "#.....#", //
                "#######", //
            ],
            &[
                "#######", //
                "?.....#", //
                "#######", //
            ],
        );
        let occupied = |cell: Position| cell == at(2, 1);

        assert_eq!(
            explore_step(&map, at(3, 1), occupied),
            ExploreStep::Blocked(Direction::West)
        );
    }

    #[test]
    fn an_unknown_area_out_of_reach_counts_as_explored() {
        // The player remembers an area beyond a wall it cannot cross; the
        // unknown next to that area is out of its reach.
        let map = seen_map(
            &[
                "#########", //
                "#...#...#", //
                "#########", //
            ],
            &[
                "#########", //
                "#...#..??", //
                "#########", //
            ],
        );

        assert_eq!(frontier(&map), [at(7, 1)]);
        assert_eq!(explore_step(&map, at(2, 1), nobody), ExploreStep::Explored);
    }

    #[test]
    fn an_unseen_starting_cell_explores_nothing() {
        let map = seen_map(
            &[
                ".....", //
            ],
            &[
                "..???", //
            ],
        );

        assert_eq!(explore_step(&map, at(4, 0), nobody), ExploreStep::Explored);
    }
}
