//! Grid geometry: cell positions and the eight directions an actor can step in.
//!
//! Exposes [`Position`] and [`Direction`]. This module knows nothing about
//! tiles or walls: whether a step is allowed is decided by the `action` module
//! against a `map::Map`.

use serde::{Deserialize, Serialize};

/// A cell on the dungeon grid.
///
/// `x` grows to the right and `y` grows downwards, matching terminal rows.
/// Coordinates are signed so that stepping off the edge of a map yields a
/// position that can be checked, instead of an integer underflow. A save
/// writes it as `(x: 5, y: 3)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Position {
    /// Column, `0` being the left edge of the map.
    pub x: i32,
    /// Row, `0` being the top edge of the map.
    pub y: i32,
}

impl Position {
    /// Creates the position at column `x` and row `y`.
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    /// Returns the neighbouring cell one step away in `direction`.
    ///
    /// The result may lie outside any map; callers check it against the map.
    pub const fn step(self, direction: Direction) -> Self {
        let (dx, dy) = direction.delta();
        Self::new(self.x + dx, self.y + dy)
    }
}

/// One of the eight compass directions, diagonals included.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    /// Up (towards row 0).
    North,
    /// Up and right.
    NorthEast,
    /// Right.
    East,
    /// Down and right.
    SouthEast,
    /// Down.
    South,
    /// Down and left.
    SouthWest,
    /// Left.
    West,
    /// Up and left.
    NorthWest,
}

impl Direction {
    /// Every direction, clockwise from [`Direction::North`].
    pub const ALL: [Self; 8] = [
        Self::North,
        Self::NorthEast,
        Self::East,
        Self::SouthEast,
        Self::South,
        Self::SouthWest,
        Self::West,
        Self::NorthWest,
    ];

    /// Returns the `(dx, dy)` offset of one step in this direction.
    ///
    /// Each component is `-1`, `0` or `1`, with `y` growing downwards, so
    /// [`Direction::North`] is `(0, -1)`.
    pub const fn delta(self) -> (i32, i32) {
        match self {
            Self::North => (0, -1),
            Self::NorthEast => (1, -1),
            Self::East => (1, 0),
            Self::SouthEast => (1, 1),
            Self::South => (0, 1),
            Self::SouthWest => (-1, 1),
            Self::West => (-1, 0),
            Self::NorthWest => (-1, -1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deltas_cover_the_eight_neighbours_exactly_once() {
        let mut deltas: Vec<(i32, i32)> = Direction::ALL.iter().map(|d| d.delta()).collect();
        deltas.sort_unstable();
        deltas.dedup();

        assert_eq!(deltas.len(), 8);
        assert!(!deltas.contains(&(0, 0)));
        assert!(
            deltas
                .iter()
                .all(|&(dx, dy)| (-1..=1).contains(&dx) && (-1..=1).contains(&dy))
        );
    }

    #[test]
    fn north_points_towards_row_zero() {
        assert_eq!(
            Position::new(3, 3).step(Direction::North),
            Position::new(3, 2)
        );
        assert_eq!(
            Position::new(3, 3).step(Direction::SouthWest),
            Position::new(2, 4)
        );
    }

    #[test]
    fn step_can_leave_the_positive_quadrant() {
        assert_eq!(
            Position::new(0, 0).step(Direction::NorthWest),
            Position::new(-1, -1)
        );
    }
}
