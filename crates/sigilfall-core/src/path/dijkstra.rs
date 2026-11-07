//! Dijkstra maps: the step distance from every cell to the nearest of a set
//! of goals.
//!
//! Exposes [`DijkstraMap`] and [`STEP_COST`]. A Dijkstra map answers "how
//! far is the nearest goal from here, and which way is it?" for every cell at
//! once, which is what placing the stairs far from the start, fleeing or
//! chasing, and walking towards unexplored cells need. It does not find a
//! path between two given cells (A* does that more cheaply).

use std::collections::VecDeque;

use crate::geometry::{Direction, Position};
use crate::map::Map;

/// Distance added by one step, orthogonal or diagonal.
///
/// Every move costs the same game time whatever its direction, so distances
/// count steps (Chebyshev distance in an open room) and one unit is one move.
pub const STEP_COST: u32 = 1;

/// Step distances from every cell of a [`Map`] to the nearest goal.
///
/// Distances follow the movement rule ([`Map::can_step`], eight directions),
/// so a distance is the number of moves an actor needs to reach the nearest
/// goal. Walls, cells outside the map and floor cells cut off from every goal
/// have no distance (`None`).
///
/// The map borrows the [`Map`] it was computed on: tiles cannot change while
/// it exists, so its distances can never be stale.
#[derive(Debug, Clone)]
pub struct DijkstraMap<'map> {
    /// The map the distances were computed on.
    map: &'map Map,
    /// One entry per cell, indexed like the map: the distance to the nearest
    /// goal, or `None` when no goal can be reached from that cell.
    distances: Vec<Option<u32>>,
}

impl<'map> DijkstraMap<'map> {
    /// Computes the distance from every cell of `map` to the nearest of
    /// `goals`.
    ///
    /// Goals sit at distance `0`. Goals on a wall or outside the map are
    /// skipped, since nothing can stand there; duplicate goals are harmless.
    /// With no usable goal, every cell is unreachable.
    ///
    /// All steps cost the same ([`STEP_COST`]), so Dijkstra's algorithm
    /// reduces to a breadth-first wave from all goals at once: each cell is
    /// visited once, in time linear in the size of the map.
    pub fn new(map: &'map Map, goals: impl IntoIterator<Item = Position>) -> Self {
        let mut distances = vec![None; map.width() * map.height()];
        let mut frontier = VecDeque::new();

        for goal in goals {
            let Some(index) = map.index(goal) else {
                continue;
            };
            if map.is_walkable(goal) && distances[index].is_none() {
                distances[index] = Some(0);
                frontier.push_back((goal, 0));
            }
        }

        while let Some((cell, distance)) = frontier.pop_front() {
            for direction in Direction::ALL {
                if !map.can_step(cell, direction) {
                    continue;
                }
                let neighbour = cell.step(direction);
                let Some(index) = map.index(neighbour) else {
                    continue;
                };
                if distances[index].is_none() {
                    distances[index] = Some(distance + STEP_COST);
                    frontier.push_back((neighbour, distance + STEP_COST));
                }
            }
        }

        Self { map, distances }
    }

    /// Returns the distance from `position` to the nearest goal, in units of
    /// [`STEP_COST`], or `None` when `position` is a wall, lies outside the
    /// map or cannot reach any goal.
    pub fn distance(&self, position: Position) -> Option<u32> {
        self.map
            .index(position)
            .and_then(|index| self.distances[index])
    }

    /// Returns the reachable cell farthest from every goal, with its distance.
    ///
    /// Used to place the stairs as far as possible from the start. When
    /// several cells share the greatest distance, the first one in reading
    /// order (row by row from the top, left to right) wins, so the result is
    /// deterministic. Returns `None` when no cell is reachable (no usable
    /// goal).
    pub fn farthest(&self) -> Option<(Position, u32)> {
        let mut farthest: Option<(Position, u32)> = None;
        for (position, &distance) in self.map.positions().zip(&self.distances) {
            let Some(distance) = distance else {
                continue;
            };
            if farthest.is_none_or(|(_, best)| distance > best) {
                farthest = Some((position, distance));
            }
        }
        farthest
    }

    /// Returns the direction of a step from `from` that brings an actor one
    /// step closer to the nearest goal, or `None` when there is no way down.
    ///
    /// Following this direction repeatedly reaches a goal in exactly
    /// [`DijkstraMap::distance`] steps. Only steps allowed by
    /// [`Map::can_step`] are considered. With a uniform [`STEP_COST`] and a
    /// symmetric step rule, every lower neighbour is exactly one step closer,
    /// so the first one in [`Direction::ALL`] order (clockwise from north) is
    /// taken, which keeps the choice deterministic.
    ///
    /// Returns `None` when `from` is a goal (nowhere lower to go), or when it
    /// is a wall, lies outside the map or cannot reach any goal.
    pub fn downhill(&self, from: Position) -> Option<Direction> {
        let current = self.distance(from)?;
        Direction::ALL.into_iter().find(|&direction| {
            self.map.can_step(from, direction)
                && self
                    .distance(from.step(direction))
                    .is_some_and(|distance| distance < current)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::test_maps::from_rows;

    /// Draws the distances of `distances` as text rows: `#` for walls, `-`
    /// for unreachable floor and the distance digit otherwise.
    ///
    /// Panics on a distance above 9, which the test maps never reach.
    fn draw(map: &Map, distances: &DijkstraMap<'_>) -> Vec<String> {
        let mut rows = vec![String::new(); map.height()];
        for position in map.positions() {
            let glyph = match distances.distance(position) {
                Some(distance) => char::from_digit(distance, 10).expect("single digit"),
                None if map.is_walkable(position) => '-',
                None => '#',
            };
            rows[usize::try_from(position.y).expect("on the map")].push(glyph);
        }
        rows
    }

    #[test]
    fn counts_diagonal_steps_like_orthogonal_ones_in_an_open_room() {
        let map = from_rows(&[
            ".....", //
            ".....", //
            ".....", //
        ]);

        let distances = DijkstraMap::new(&map, [Position::new(0, 0)]);

        assert_eq!(draw(&map, &distances), ["01234", "11234", "22234"]);
    }

    #[test]
    fn measures_exact_distances_around_walls() {
        let map = from_rows(&[
            "#######", //
            "#.....#", //
            "#.###.#", //
            "#.#...#", //
            "#######", //
        ]);

        let distances = DijkstraMap::new(&map, [Position::new(1, 3)]);

        assert_eq!(
            draw(&map, &distances),
            ["#######", "#22345#", "#1###5#", "#0#766#", "#######"]
        );
    }

    #[test]
    fn leaves_isolated_areas_walls_and_outside_without_distance() {
        let map = from_rows(&[
            "..#..", //
            "..#..", //
        ]);

        let distances = DijkstraMap::new(&map, [Position::new(0, 0)]);

        assert_eq!(draw(&map, &distances), ["01#--", "11#--"]);
        assert_eq!(distances.distance(Position::new(2, 0)), None);
        assert_eq!(distances.distance(Position::new(-1, 0)), None);
        assert_eq!(distances.distance(Position::new(0, 2)), None);
    }

    #[test]
    fn uses_the_nearest_of_several_goals() {
        let map = from_rows(&[
            "#.........#", //
        ]);

        let distances = DijkstraMap::new(&map, [Position::new(1, 0), Position::new(9, 0)]);

        assert_eq!(draw(&map, &distances), ["#012343210#"]);
    }

    #[test]
    fn reaches_each_isolated_area_from_its_own_goal() {
        let map = from_rows(&[
            "...#...", //
        ]);

        let distances = DijkstraMap::new(&map, [Position::new(0, 0), Position::new(6, 0)]);

        assert_eq!(draw(&map, &distances), ["012#210"]);
    }

    #[test]
    fn skips_goals_on_walls_or_outside_the_map() {
        let map = from_rows(&[
            "#..", //
        ]);

        let distances = DijkstraMap::new(&map, [Position::new(0, 0), Position::new(7, 7)]);

        assert_eq!(draw(&map, &distances), ["#--"]);
        assert_eq!(distances.farthest(), None);
    }

    #[test]
    fn crosses_a_diagonal_gap_between_two_walls_in_one_step() {
        let map = from_rows(&[
            ".#", //
            "#.", //
        ]);

        let distances = DijkstraMap::new(&map, [Position::new(0, 0)]);

        assert_eq!(distances.distance(Position::new(1, 1)), Some(1));
        assert_eq!(
            distances.downhill(Position::new(1, 1)),
            Some(Direction::NorthWest)
        );
    }

    #[test]
    fn farthest_ignores_isolated_areas() {
        // The isolated cell at the far right would be the farthest if walls
        // were ignored.
        let map = from_rows(&[
            "#####.", //
            "....#.", //
            "#####.", //
        ]);

        let distances = DijkstraMap::new(&map, [Position::new(0, 1)]);

        assert_eq!(distances.farthest(), Some((Position::new(3, 1), 3)));
    }

    #[test]
    fn farthest_breaks_ties_in_reading_order() {
        let map = from_rows(&[
            "...", //
            "...", //
            "...", //
        ]);

        let distances = DijkstraMap::new(&map, [Position::new(1, 1)]);

        assert_eq!(distances.farthest(), Some((Position::new(0, 0), 1)));
    }

    #[test]
    fn farthest_finds_the_end_of_a_winding_corridor() {
        let map = from_rows(&[
            "#######", //
            "#.....#", //
            "#.###.#", //
            "#.#...#", //
            "#######", //
        ]);

        let distances = DijkstraMap::new(&map, [Position::new(1, 3)]);

        assert_eq!(distances.farthest(), Some((Position::new(3, 3), 7)));
    }

    #[test]
    fn walking_downhill_reaches_a_goal_in_exactly_distance_steps() {
        let map = from_rows(&[
            "#######", //
            "#.....#", //
            "#.###.#", //
            "#.#...#", //
            "#######", //
        ]);
        let goal = Position::new(1, 3);
        let distances = DijkstraMap::new(&map, [goal]);
        let start = Position::new(3, 3);

        let mut position = start;
        let mut steps = 0;
        while let Some(direction) = distances.downhill(position) {
            assert!(map.can_step(position, direction));
            position = position.step(direction);
            steps += 1;
        }

        assert_eq!(position, goal);
        assert_eq!(Some(steps), distances.distance(start));
    }

    #[test]
    fn downhill_breaks_ties_clockwise_from_north() {
        let map = from_rows(&[
            "...", //
            "...", //
            "...", //
        ]);
        let distances = DijkstraMap::new(
            &map,
            [
                Position::new(0, 0),
                Position::new(1, 0),
                Position::new(2, 0),
            ],
        );

        // North, NorthEast and NorthWest all lead to distance 1.
        assert_eq!(
            distances.downhill(Position::new(1, 2)),
            Some(Direction::North)
        );
    }

    #[test]
    fn downhill_skips_blocked_directions() {
        let map = from_rows(&[
            "...", //
            ".#.", //
            "...", //
        ]);
        let distances = DijkstraMap::new(&map, [Position::new(0, 0), Position::new(2, 0)]);

        // North is a wall; NorthEast and NorthWest tie, NorthEast comes first.
        assert_eq!(
            distances.downhill(Position::new(1, 2)),
            Some(Direction::NorthEast)
        );
    }

    #[test]
    fn no_way_down_from_a_goal_or_an_unreachable_cell() {
        let map = from_rows(&[
            "..#.", //
        ]);

        let distances = DijkstraMap::new(&map, [Position::new(0, 0)]);

        assert_eq!(distances.downhill(Position::new(0, 0)), None);
        assert_eq!(distances.downhill(Position::new(3, 0)), None);
        assert_eq!(distances.downhill(Position::new(2, 0)), None);
        assert_eq!(distances.downhill(Position::new(9, 9)), None);
    }
}
