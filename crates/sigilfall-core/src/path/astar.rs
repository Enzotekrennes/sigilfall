//! A* search: the shortest walk between two given cells.
//!
//! Exposes [`find_path`]. Where a [`DijkstraMap`](super::DijkstraMap)
//! measures every cell's distance to a set of goals, A* looks for one path
//! between one start and one goal, and explores only the cells that may lie
//! on a shortest one. Monsters use it to chase the player.
//!
//! Steps follow the movement rule of
//! [`Map::can_step`](crate::map::Map::can_step) in the eight directions, and
//! every step costs [`STEP_COST`], so the length of a path is the number of
//! moves an actor needs. Cells held by other actors are passed in by the
//! caller; this module knows nothing about actors.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use super::STEP_COST;
use crate::geometry::{Direction, Position};
use crate::map::Map;

/// Returns the directions of a shortest walk from `from` to `to` on `map`,
/// or `None` when `to` cannot be reached.
///
/// Each step is allowed by [`Map::can_step`] and must not enter a cell for
/// which `occupied` returns `true`, except `to` itself: the goal may hold an
/// actor (the player being chased), the cells on the way may not. `from` is
/// never checked, since the walker already stands there. The path has the
/// fewest steps possible under these rules; it is empty when `from == to`.
/// `None` is returned when `to` is a wall, when `from` or `to` lies outside
/// the map, or when walls and occupied cells cut every way to `to`.
///
/// # Heuristic
///
/// The estimate of the remaining cost from a cell is its Chebyshev distance
/// to `to` (the larger of the column and row gaps) times [`STEP_COST`]. A
/// step changes each coordinate by at most one, whatever its direction, so
/// no walk can be shorter than that distance: the estimate never
/// overestimates (it is admissible), and it drops by at most one step cost
/// per step (it is consistent). The first time a cell is taken from the
/// open set, its cost is therefore final, and the path found is a shortest
/// one.
///
/// # Ties
///
/// Several shortest paths often exist. The open set is ordered by estimated
/// total cost, then by estimate (the cell closer to the goal first), then by
/// squared straight-line distance to the goal, then by the order in which
/// cells were reached; neighbours are reached in [`Direction::ALL`] order
/// (clockwise from north). The straight-line rule keeps the path close to
/// the line between the two cells, so a chasing monster walks straight at
/// its prey instead of zigzagging along an equally short path. Every key is
/// an integer, so the same arguments always give the same path, on every
/// platform.
///
/// The cost is at most proportional to the number of cells of the map times
/// the logarithm of that number.
pub fn find_path(
    map: &Map,
    from: Position,
    to: Position,
    occupied: impl Fn(Position) -> bool,
) -> Option<Vec<Direction>> {
    let goal_index = map.index(to)?;
    let start_index = map.index(from)?;
    if !map.is_walkable(to) {
        return None;
    }

    let cells = map.width() * map.height();
    let mut cost_so_far: Vec<Option<u32>> = vec![None; cells];
    // The step that first reached each cell on its best known path.
    let mut arrived_by: Vec<Option<Direction>> = vec![None; cells];
    let mut closed = vec![false; cells];
    let mut open = BinaryHeap::new();
    // Counts cells pushed on the open set, to break the last ties in the
    // order cells were reached.
    let mut pushed: u64 = 0;

    cost_so_far[start_index] = Some(0);
    open.push(Reverse(OpenCell {
        estimate: estimate(from, to),
        remaining: estimate(from, to),
        straightness: squared_distance(from, to),
        order: pushed,
        position: from,
    }));

    while let Some(Reverse(current)) = open.pop() {
        let index = map
            .index(current.position)
            .expect("only cells of the map enter the open set");
        if closed[index] {
            continue;
        }
        if index == goal_index {
            return Some(walk_back(map, &arrived_by, from, to));
        }
        closed[index] = true;
        let cost = cost_so_far[index].expect("a cell in the open set has a cost");

        for direction in Direction::ALL {
            let next = current.position.step(direction);
            if !map.can_step(current.position, direction) || (next != to && occupied(next)) {
                continue;
            }
            let next_index = map.index(next).expect("a walkable cell lies on the map");
            let next_cost = cost + STEP_COST;
            if closed[next_index] || cost_so_far[next_index].is_some_and(|c| c <= next_cost) {
                continue;
            }
            cost_so_far[next_index] = Some(next_cost);
            arrived_by[next_index] = Some(direction);
            pushed += 1;
            let remaining = estimate(next, to);
            open.push(Reverse(OpenCell {
                estimate: next_cost + remaining,
                remaining,
                straightness: squared_distance(next, to),
                order: pushed,
                position: next,
            }));
        }
    }
    None
}

/// A cell waiting in the open set of [`find_path`].
///
/// Cells are ordered by estimated total cost, then by estimate of the
/// remaining cost, then by squared straight-line distance to the goal, then
/// by the order in which they were reached: the tie-breaking order of the
/// search. The position never decides, since `order` is unique.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OpenCell {
    /// Cost from the start plus the estimate of the cost to the goal.
    estimate: u32,
    /// Estimate of the cost from this cell to the goal (see [`estimate`]).
    remaining: u32,
    /// Squared straight-line distance from this cell to the goal (see
    /// [`squared_distance`]); only breaks ties.
    straightness: u64,
    /// Rank of this entry among all the entries pushed so far.
    order: u64,
    /// The cell itself.
    position: Position,
}

impl OpenCell {
    /// Returns the values the open set is ordered by, most significant first.
    const fn rank(&self) -> (u32, u32, u64, u64) {
        (self.estimate, self.remaining, self.straightness, self.order)
    }
}

impl Ord for OpenCell {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.rank().cmp(&other.rank())
    }
}

impl PartialOrd for OpenCell {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Returns the heuristic of [`find_path`]: the Chebyshev distance from
/// `from` to `to`, in units of [`STEP_COST`], a lower bound of the number of
/// steps between them.
fn estimate(from: Position, to: Position) -> u32 {
    let columns = from.x.abs_diff(to.x);
    let rows = from.y.abs_diff(to.y);
    columns.max(rows) * STEP_COST
}

/// Returns the squared straight-line distance between the centres of
/// `from` and `to`, which ranks cells by how close they lie to the goal
/// without any rounding.
fn squared_distance(from: Position, to: Position) -> u64 {
    let columns = u64::from(from.x.abs_diff(to.x));
    let rows = u64::from(from.y.abs_diff(to.y));
    columns * columns + rows * rows
}

/// Rebuilds the path to `to` by following `arrived_by` back to `from`, and
/// returns its directions from `from` onwards.
fn walk_back(
    map: &Map,
    arrived_by: &[Option<Direction>],
    from: Position,
    to: Position,
) -> Vec<Direction> {
    let mut directions = Vec::new();
    let mut position = to;
    while position != from {
        let index = map.index(position).expect("the path stays on the map");
        let direction = arrived_by[index].expect("every cell of the path was reached by a step");
        directions.push(direction);
        let (dx, dy) = direction.delta();
        position = Position::new(position.x - dx, position.y - dy);
    }
    directions.reverse();
    directions
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::map::Tile;
    use crate::path::DijkstraMap;
    use crate::path::test_maps::from_rows;

    /// Shorthand for a position in the test maps.
    const fn at(x: i32, y: i32) -> Position {
        Position::new(x, y)
    }

    /// A predicate under which no cell is occupied.
    const fn nobody(_: Position) -> bool {
        false
    }

    /// Follows `path` from `from`, checking every step against the movement
    /// rule and `occupied` (the last cell excepted), and returns the cells
    /// visited after `from`.
    fn walk(
        map: &Map,
        from: Position,
        path: &[Direction],
        occupied: impl Fn(Position) -> bool,
    ) -> Vec<Position> {
        let mut cells = Vec::new();
        let mut position = from;
        for (step, &direction) in path.iter().enumerate() {
            assert!(map.can_step(position, direction), "step {step} is illegal");
            position = position.step(direction);
            if step + 1 < path.len() {
                assert!(!occupied(position), "step {step} enters an occupied cell");
            }
            cells.push(position);
        }
        cells
    }

    /// Asserts that `find_path` from `from` to `to` ends on `to` in exactly
    /// the Dijkstra distance between them.
    fn assert_shortest(map: &Map, from: Position, to: Position) {
        let path = find_path(map, from, to, nobody).expect("a path exists");
        let cells = walk(map, from, &path, nobody);
        assert_eq!(cells.last().copied().unwrap_or(from), to);
        let distance = DijkstraMap::new(map, [to]).distance(from);
        assert_eq!(Some(u32::try_from(path.len()).unwrap()), distance);
    }

    #[test]
    fn crosses_an_open_room_diagonally() {
        let map = from_rows(&[
            ".....", //
            ".....", //
            ".....", //
        ]);

        let path = find_path(&map, at(0, 0), at(4, 2), nobody).expect("open room");

        assert_eq!(path.len(), 4);
        assert_shortest(&map, at(0, 0), at(4, 2));
    }

    #[test]
    fn matches_dijkstra_distances_around_walls() {
        let map = from_rows(&[
            "##########", //
            "#........#", //
            "#.######.#", //
            "#.#....#.#", //
            "#.#.##.#.#", //
            "#...#..#.#", //
            "######...#", //
            "##########", //
        ]);
        let cells: Vec<Position> = map.positions().filter(|&p| map.is_walkable(p)).collect();

        // Every pair of floor cells, both ways.
        for &from in &cells {
            for &to in &cells {
                assert_shortest(&map, from, to);
            }
        }
    }

    #[test]
    fn winds_through_a_maze_in_the_fewest_steps() {
        let map = from_rows(&[
            "#########", //
            "#.#.....#", //
            "#.#.###.#", //
            "#.#.#...#", //
            "#.#.#.###", //
            "#...#...#", //
            "#########", //
        ]);

        let path = find_path(&map, at(1, 1), at(7, 5), nobody).expect("connected");

        // Down, along the bottom, up, across the top, then back down
        // through the right-hand pocket, cutting each bend diagonally.
        assert_eq!(path.len(), 15);
        assert_shortest(&map, at(1, 1), at(7, 5));
    }

    #[test]
    fn cuts_a_diagonal_gap_like_the_movement_rule() {
        let map = from_rows(&[
            ".#", //
            "#.", //
        ]);

        assert_eq!(
            find_path(&map, at(0, 0), at(1, 1), nobody),
            Some(vec![Direction::SouthEast])
        );
    }

    #[test]
    fn the_path_to_the_start_itself_is_empty() {
        let map = from_rows(&["..."]);

        assert_eq!(find_path(&map, at(1, 0), at(1, 0), nobody), Some(vec![]));
    }

    #[test]
    fn no_path_to_a_walled_off_area() {
        let map = from_rows(&[
            "..#..", //
            "..#..", //
        ]);

        assert_eq!(find_path(&map, at(0, 0), at(4, 1), nobody), None);
    }

    #[test]
    fn no_path_to_a_wall_or_outside_the_map() {
        let map = from_rows(&["..#"]);

        assert_eq!(find_path(&map, at(0, 0), at(2, 0), nobody), None);
        assert_eq!(find_path(&map, at(0, 0), at(5, 0), nobody), None);
        assert_eq!(find_path(&map, at(0, 0), at(-1, 0), nobody), None);
    }

    #[test]
    fn walks_around_occupied_cells() {
        let map = from_rows(&[
            ".....", //
            ".....", //
            ".....", //
        ]);
        // A wall of actors across the middle row, open at the right end.
        let blocked = [at(0, 1), at(1, 1), at(2, 1), at(3, 1)];
        let occupied = |p: Position| blocked.contains(&p);

        let path = find_path(&map, at(0, 0), at(0, 2), occupied).expect("way round");

        let cells = walk(&map, at(0, 0), &path, occupied);
        assert_eq!(cells.last(), Some(&at(0, 2)));
        assert!(cells.contains(&at(4, 1)), "goes through the gap: {cells:?}");
        assert_eq!(path.len(), 8);
    }

    #[test]
    fn the_goal_may_be_occupied() {
        let map = from_rows(&["....."]);
        let occupied = |p: Position| p == at(4, 0);

        let path = find_path(&map, at(0, 0), at(4, 0), occupied).expect("goal held");

        assert_eq!(path, vec![Direction::East; 4]);
    }

    #[test]
    fn no_path_when_actors_fill_the_only_corridor() {
        let map = from_rows(&[
            "#####", //
            ".....", //
            "#####", //
        ]);
        let occupied = |p: Position| p == at(2, 1);

        assert_eq!(find_path(&map, at(0, 1), at(4, 1), occupied), None);
    }

    #[test]
    fn ties_are_broken_the_same_way_every_time() {
        let map = from_rows(&[
            ".......", //
            ".......", //
            ".......", //
            ".......", //
        ]);

        let first = find_path(&map, at(0, 0), at(6, 3), nobody);

        for _ in 0..10 {
            assert_eq!(find_path(&map, at(0, 0), at(6, 3), nobody), first);
        }
        // East and south-east tie on cost and estimate; south-east ends
        // closer to the straight line towards the goal.
        assert_eq!(
            first,
            Some(vec![
                Direction::SouthEast,
                Direction::SouthEast,
                Direction::SouthEast,
                Direction::East,
                Direction::East,
                Direction::East,
            ])
        );
    }

    #[test]
    fn a_straight_line_is_preferred_to_an_equally_short_zigzag() {
        let map = from_rows(&[
            ".........", //
            ".........", //
            ".........", //
        ]);

        let path = find_path(&map, at(8, 1), at(0, 1), nobody);

        assert_eq!(path, Some(vec![Direction::West; 8]));
    }

    #[test]
    fn the_heuristic_never_overestimates_the_true_distance() {
        let map = from_rows(&[
            "#########", //
            "#...#...#", //
            "#.#.#.#.#", //
            "#.#...#.#", //
            "#########", //
        ]);
        let goal = at(7, 3);
        let distances = DijkstraMap::new(&map, [goal]);

        for cell in map.positions().filter(|&p| map.is_walkable(p)) {
            let distance = distances.distance(cell).expect("connected map");
            assert!(estimate(cell, goal) <= distance, "{cell:?}");
        }
    }

    /// A random 12 × 8 map where each inner cell is a wall with probability
    /// one in three, the border included as open floor.
    fn random_map() -> impl Strategy<Value = Map> {
        proptest::collection::vec(prop::bool::weighted(1.0 / 3.0), 12 * 8).prop_map(|walls| {
            let mut map = Map::new(12, 8, Tile::Floor);
            let positions: Vec<Position> = map.positions().collect();
            for (position, wall) in positions.into_iter().zip(walls) {
                if wall {
                    map.set_tile(position, Tile::Wall);
                }
            }
            map
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(300))]

        /// On random maps, a path exists exactly when Dijkstra reaches the
        /// goal, and it is exactly as long as the Dijkstra distance.
        #[test]
        fn path_length_equals_the_dijkstra_distance(
            map in random_map(),
            from in (0..12, 0..8),
            to in (0..12, 0..8),
        ) {
            let (from, to) = (at(from.0, from.1), at(to.0, to.1));
            prop_assume!(map.is_walkable(from));

            let distance = DijkstraMap::new(&map, [to]).distance(from);
            let path = find_path(&map, from, to, nobody);

            prop_assert_eq!(path.as_ref().map(|p| u32::try_from(p.len()).unwrap()), distance);
            if let Some(path) = path {
                let cells = walk(&map, from, &path, nobody);
                prop_assert_eq!(cells.last().copied().unwrap_or(from), to);
            }
        }
    }
}
