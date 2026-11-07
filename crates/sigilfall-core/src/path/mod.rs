//! Reachability and distance computations on a [`Map`](crate::map::Map).
//!
//! Exposes [`flood_fill`], the set of cells reachable from a starting cell,
//! and [`DijkstraMap`], step distances from one or more goals with the
//! farthest cell and the way downhill towards the nearest goal.
//!
//! Every computation moves in the eight directions and asks
//! [`Map::can_step`](crate::map::Map::can_step) whether a step is allowed,
//! the same rule as actor movement: a cell is reachable here exactly when an
//! actor could walk to it. Shortest paths between two given cells (A*) are
//! not handled here.

mod dijkstra;
mod flood;

pub use dijkstra::{DijkstraMap, STEP_COST};
pub use flood::flood_fill;

#[cfg(test)]
pub(crate) mod test_maps {
    //! Hand-drawn maps for the tests of this module.

    use crate::geometry::Position;
    use crate::map::{Map, Tile};

    /// Builds a map from text rows, `#` being a wall and any other character
    /// a floor cell.
    ///
    /// # Panics
    ///
    /// Panics if `rows` is empty or the rows differ in length.
    pub(crate) fn from_rows(rows: &[&str]) -> Map {
        let width = rows[0].chars().count();
        let mut map = Map::new(width, rows.len(), Tile::Floor);
        for (y, row) in rows.iter().enumerate() {
            assert_eq!(row.chars().count(), width, "row {y} has a different length");
            for (x, glyph) in row.chars().enumerate() {
                if glyph == '#' {
                    let position = Position::new(
                        i32::try_from(x).expect("test map fits in i32"),
                        i32::try_from(y).expect("test map fits in i32"),
                    );
                    map.set_tile(position, Tile::Wall);
                }
            }
        }
        map
    }
}
