//! Flood fill: which cells an actor can reach from a given cell.
//!
//! Exposes [`flood_fill`]. It answers "is this area connected?" questions,
//! such as checking that every room of a generated floor is reachable; it does
//! not measure distances (see [`DijkstraMap`]).

use super::DijkstraMap;
use crate::geometry::Position;
use crate::map::Map;

/// Returns every cell reachable from `start` by a chain of allowed steps
/// (eight directions, [`Map::can_step`]), `start` included.
///
/// The cells come in reading order (row by row from the top, left to right),
/// so iterating over them is deterministic and safe to feed into seeded
/// random choices. The result is empty when `start` is a wall or lies outside
/// the map: nothing can stand there.
///
/// Reachability is the set of cells a [`DijkstraMap`] from `start` assigns a
/// distance to, so the two can never disagree.
pub fn flood_fill(map: &Map, start: Position) -> Vec<Position> {
    let distances = DijkstraMap::new(map, [start]);
    map.positions()
        .filter(|&position| distances.distance(position).is_some())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::test_maps::from_rows;

    #[test]
    fn fills_a_room_around_a_pillar_entirely() {
        let map = Map::fixed_room();
        let floor_cells = map.positions().filter(|&p| map.is_walkable(p)).count();

        let region = flood_fill(&map, Position::new(1, 1));

        assert_eq!(region.len(), floor_cells);
        assert!(region.iter().all(|&p| map.is_walkable(p)));
    }

    #[test]
    fn stops_at_walls_and_leaves_isolated_areas_out() {
        let map = from_rows(&[
            "#######", //
            "#..#..#", //
            "#..#..#", //
            "#######", //
        ]);

        let left = flood_fill(&map, Position::new(1, 1));

        assert_eq!(
            left,
            [(1, 1), (2, 1), (1, 2), (2, 2)].map(|(x, y)| Position::new(x, y))
        );
        assert!(!left.contains(&Position::new(4, 1)));
    }

    #[test]
    fn crosses_a_diagonal_gap_between_two_walls() {
        // The two halves touch only through the corner between (2, 1) and
        // (3, 2); corner cutting is allowed, as for movement.
        let map = from_rows(&[
            "######", //
            "#..###", //
            "###..#", //
            "######", //
        ]);

        let region = flood_fill(&map, Position::new(1, 1));

        assert_eq!(
            region,
            [(1, 1), (2, 1), (3, 2), (4, 2)].map(|(x, y)| Position::new(x, y))
        );
    }

    #[test]
    fn is_empty_from_a_wall_or_outside_the_map() {
        let map = from_rows(&[
            "#.", //
        ]);

        assert!(flood_fill(&map, Position::new(0, 0)).is_empty());
        assert!(flood_fill(&map, Position::new(5, 0)).is_empty());
        assert!(flood_fill(&map, Position::new(-1, 0)).is_empty());
    }
}
