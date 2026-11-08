//! The camera: which part of the dungeon map the map area of the screen
//! shows, and where each map cell lands on the screen.
//!
//! Exposes [`view_origin`], the pure rule that places the view, and
//! [`Camera`], which applies it to an area of the terminal. A floor
//! (80 × 40) is larger than the map area of an 80 × 24 terminal (58 × 16),
//! so the view follows the player:
//! - on an axis where the map is larger than the area, the view is centred
//!   on the player and stopped at the map's edges, so no empty space beyond
//!   the map is ever shown on that axis;
//! - on an axis where the area is as large as the map or larger, the whole
//!   map is shown, centred in the area, and does not move with the player.
//!
//! This module draws nothing and knows nothing about tiles or styles.

use ratatui::layout::{Position as ScreenPosition, Rect, Size};
use sigilfall_core::geometry::Position;
use sigilfall_core::map::Map;

/// Returns the map cell shown at the top-left corner of a view of size
/// `view` on a map of size `map`, for a view that follows `focus`.
///
/// Sizes are in cells (columns × rows); `focus` is a map position, normally
/// the player's. Each axis is placed on its own (see [`axis_origin`]):
/// centred on `focus` and clamped to the map's edges when the map is larger
/// than the view, otherwise the map is centred in the view and the result is
/// zero or negative (the view starts before the map's first cell).
///
/// The result never depends on `focus` along an axis where the whole map
/// fits. Any input is accepted, including a zero-sized view or map and a
/// `focus` outside the map, without panicking.
pub fn view_origin(view: Size, map: Size, focus: Position) -> Position {
    Position::new(
        axis_origin(view.width, map.width, focus.x),
        axis_origin(view.height, map.height, focus.y),
    )
}

/// Places the view along one axis: returns the map coordinate shown in the
/// view's first column (or row).
///
/// - `view` cells or more for a `map` of `map` cells: the map is centred in
///   the view; when the spare cells cannot be split evenly, the extra one
///   goes after the map (right or bottom). The result is `-(spare / 2)`.
/// - Fewer view cells than map cells: `focus` is put in the middle cell of
///   the view (`view / 2`, so the cell right of the centre for an even view),
///   then the view is shifted back inside the map, so its first coordinate
///   lies in `0..=map - view` and every cell it shows exists.
fn axis_origin(view: u16, map: u16, focus: i32) -> i32 {
    let (view, map) = (i32::from(view), i32::from(map));
    if view >= map {
        return -((view - map) / 2);
    }
    (focus - view / 2).clamp(0, map - view)
}

/// The size of `map` in cells, saturated to the largest terminal size.
///
/// Floors are far smaller than `u16::MAX` cells on a side; saturating only
/// keeps the conversion total.
fn map_size(map: &Map) -> Size {
    let side = |cells: usize| u16::try_from(cells).unwrap_or(u16::MAX);
    Size::new(side(map.width()), side(map.height()))
}

/// A view of the map placed in an area of the terminal, as computed by
/// [`Camera::new`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Camera {
    /// The terminal cells the map is drawn into.
    area: Rect,
    /// The map cell shown at the top-left corner of `area` (see
    /// [`view_origin`]); negative when the map is centred in a larger area.
    origin: Position,
}

impl Camera {
    /// Places a view of `map` in `area`, following `focus` (normally the
    /// player's position) by the rule of [`view_origin`].
    ///
    /// When `focus` is a cell of the map, it is always inside the view:
    /// [`Camera::screen_position`] returns `Some` for it.
    pub fn new(area: Rect, map: &Map, focus: Position) -> Self {
        Self {
            area,
            origin: view_origin(area.as_size(), map_size(map), focus),
        }
    }

    /// Returns the map cell shown at the top-left corner of the area, so
    /// tests can tell whether the view moved.
    #[cfg(test)]
    pub const fn origin(self) -> Position {
        self.origin
    }

    /// Returns the terminal cell where the map cell `position` is drawn, or
    /// `None` when that cell is outside the view.
    pub fn screen_position(self, position: Position) -> Option<ScreenPosition> {
        let column = u16::try_from(position.x.checked_sub(self.origin.x)?).ok()?;
        let row = u16::try_from(position.y.checked_sub(self.origin.y)?).ok()?;
        let screen = ScreenPosition::new(
            self.area.x.checked_add(column)?,
            self.area.y.checked_add(row)?,
        );
        self.area.contains(screen).then_some(screen)
    }

    /// Returns every terminal cell of the area with the map position it
    /// shows, row by row from the top-left corner.
    ///
    /// Positions may lie outside the map (when the map is centred in a
    /// larger area); the caller leaves those cells empty.
    pub fn cells(self) -> impl Iterator<Item = (ScreenPosition, Position)> {
        self.area.positions().map(move |screen| {
            let map = Position::new(
                self.origin.x + i32::from(screen.x - self.area.x),
                self.origin.y + i32::from(screen.y - self.area.y),
            );
            (screen, map)
        })
    }
}

#[cfg(test)]
mod tests {
    use sigilfall_core::generation::{self, FIRST_DEPTH, FLOOR_HEIGHT, FLOOR_WIDTH};
    use sigilfall_core::rng::Seed;

    use super::*;

    /// Size of a generated floor.
    const FLOOR: Size = Size::new(80, 40);
    /// Map area of an 80 × 24 terminal (see `ui::game`).
    const SMALLEST_VIEW: Size = Size::new(58, 16);
    /// Map area of a 120 × 40 terminal: wider than a floor, shorter than it.
    const WIDE_VIEW: Size = Size::new(98, 32);

    #[test]
    fn the_floor_size_used_here_is_the_generated_one() {
        assert_eq!(
            (usize::from(FLOOR.width), usize::from(FLOOR.height)),
            (FLOOR_WIDTH, FLOOR_HEIGHT)
        );
    }

    #[test]
    fn in_the_middle_of_the_map_the_focus_is_at_the_centre_of_the_view() {
        let focus = Position::new(40, 20);

        let origin = view_origin(SMALLEST_VIEW, FLOOR, focus);

        assert_eq!(origin, Position::new(40 - 29, 20 - 8));
    }

    #[test]
    fn the_view_stops_at_each_edge_of_the_map() {
        // Focus near one edge, centred on the other axis.
        let cases = [
            (Position::new(3, 20), Position::new(0, 12), "left"),
            (Position::new(78, 20), Position::new(80 - 58, 12), "right"),
            (Position::new(40, 1), Position::new(11, 0), "top"),
            (Position::new(40, 38), Position::new(11, 40 - 16), "bottom"),
        ];

        for (focus, expected, edge) in cases {
            assert_eq!(view_origin(SMALLEST_VIEW, FLOOR, focus), expected, "{edge}");
        }
    }

    #[test]
    fn the_view_stops_at_each_corner_of_the_map() {
        let cases = [
            (Position::new(1, 1), Position::new(0, 0), "top-left"),
            (Position::new(78, 1), Position::new(22, 0), "top-right"),
            (Position::new(1, 38), Position::new(0, 24), "bottom-left"),
            (Position::new(78, 38), Position::new(22, 24), "bottom-right"),
        ];

        for (focus, expected, corner) in cases {
            assert_eq!(
                view_origin(SMALLEST_VIEW, FLOOR, focus),
                expected,
                "{corner}"
            );
        }
    }

    #[test]
    fn a_view_larger_than_the_map_centres_it_and_ignores_the_focus() {
        let view = Size::new(85, 47);

        for focus in [
            Position::new(0, 0),
            Position::new(79, 39),
            Position::new(40, 20),
        ] {
            // 5 spare columns: 2 before the map, 3 after; 7 spare rows: 3 and
            // 4.
            assert_eq!(view_origin(view, FLOOR, focus), Position::new(-2, -3));
        }
    }

    #[test]
    fn a_view_of_exactly_the_map_size_shows_it_whole() {
        for focus in [Position::new(0, 0), Position::new(79, 39)] {
            assert_eq!(view_origin(FLOOR, FLOOR, focus), Position::new(0, 0));
        }
    }

    #[test]
    fn a_wide_but_short_view_centres_one_axis_and_scrolls_the_other() {
        let top = view_origin(WIDE_VIEW, FLOOR, Position::new(10, 2));
        let bottom = view_origin(WIDE_VIEW, FLOOR, Position::new(70, 37));

        assert_eq!(top, Position::new(-9, 0));
        assert_eq!(bottom, Position::new(-9, 40 - 32));
    }

    #[test]
    fn degenerate_sizes_and_a_focus_off_the_map_do_not_panic() {
        assert_eq!(
            view_origin(Size::new(0, 0), FLOOR, Position::new(-5, 99)),
            Position::new(0, 40)
        );
        assert_eq!(
            view_origin(Size::new(10, 10), Size::new(0, 0), Position::new(3, 3)),
            Position::new(-5, -5)
        );
        assert_eq!(
            view_origin(SMALLEST_VIEW, FLOOR, Position::new(-100, 500)),
            Position::new(0, 24)
        );
    }

    #[test]
    fn along_one_axis_any_focus_on_the_map_is_in_view_and_no_cell_is_off_the_map() {
        // Every view length from 1 to past the map, every map cell as focus.
        for map in [1_u16, 2, 16, 40, 80] {
            for view in 1..=map + 3 {
                for focus in 0..i32::from(map) {
                    let origin = axis_origin(view, map, focus);
                    let end = origin + i32::from(view);
                    let case = format!("view {view}, map {map}, focus {focus}");
                    assert!((origin..end).contains(&focus), "focus out of view: {case}");
                    if view < map {
                        assert!(origin >= 0 && end <= i32::from(map), "empty space: {case}");
                    } else {
                        let (before, after) = (-origin, end - i32::from(map));
                        assert!(
                            after - before == 0 || after - before == 1,
                            "not centred: {case}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_player_is_on_screen_from_every_floor_cell_of_generated_floors() {
        // Map areas of 80 × 24, 81 × 25 (odd sizes), 120 × 40 and a very
        // large terminal, at the offsets the game screen gives them.
        let areas = [
            Rect::new(0, 1, 58, 16),
            Rect::new(0, 1, 59, 17),
            Rect::new(0, 1, 98, 32),
            Rect::new(0, 1, 178, 72),
        ];
        for seed in [0, 42, 0x7F3A_91C2_0B5E_D4A8] {
            let floor = generation::generate(Seed::new(seed), FIRST_DEPTH);
            let map = floor.map();
            let floor_cells = (0..FLOOR_HEIGHT).flat_map(|y| {
                (0..FLOOR_WIDTH).map(move |x| {
                    Position::new(i32::try_from(x).unwrap(), i32::try_from(y).unwrap())
                })
            });
            let walkable: Vec<_> = floor_cells.filter(|&cell| map.is_walkable(cell)).collect();
            assert!(walkable.len() > 100, "seed {seed}: a real floor");

            for area in areas {
                for &player in &walkable {
                    let camera = Camera::new(area, map, player);
                    let screen = camera.screen_position(player);
                    assert!(
                        screen.is_some_and(|cell| area.contains(cell)),
                        "seed {seed}, area {area}, player {player:?}"
                    );
                    if area.width < FLOOR.width && area.height < FLOOR.height {
                        let shown_off_map = camera
                            .cells()
                            .filter(|&(_, cell)| map.tile(cell).is_none())
                            .count();
                        assert_eq!(
                            shown_off_map, 0,
                            "seed {seed}, area {area}, player {player:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn screen_and_map_positions_agree() {
        let floor = generation::generate(Seed::new(42), FIRST_DEPTH);
        let area = Rect::new(3, 1, 58, 16);
        let camera = Camera::new(area, floor.map(), Position::new(40, 20));

        let cells: Vec<_> = camera.cells().collect();

        assert_eq!(cells.len(), 58 * 16);
        for (screen, map) in cells {
            assert_eq!(camera.screen_position(map), Some(screen));
        }
        assert_eq!(
            camera.screen_position(camera.origin()),
            Some(ScreenPosition::new(3, 1))
        );
        assert_eq!(
            camera.screen_position(Position::new(10, 20)),
            None,
            "left of the view"
        );
        assert_eq!(
            camera.screen_position(Position::new(69, 20)),
            None,
            "right of the view"
        );
    }
}
