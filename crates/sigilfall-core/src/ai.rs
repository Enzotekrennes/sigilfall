//! Monster behaviour: what a monster does when its turn comes.
//!
//! Exposes [`choose_action`]. A monster chases the player it can see by the
//! shortest path ([`find_path`], A*), walks to where it last saw the player
//! once the player is out of sight, then waits there. A monster next to the
//! player it sees attacks it, by moving into its cell.
//!
//! This module only decides. Whether the monster sees the player is given
//! by the caller (the `game` module), and the chosen action is applied, or
//! refused, by the caller too.

use crate::action::Action;
use crate::geometry::{Direction, Position};
use crate::map::Map;
use crate::path::find_path;

/// Returns the action of a monster standing at `monster` whose turn it is,
/// and updates `last_seen`, its memory of the player.
///
/// - `player` is where the player stands, and `sees_player` whether the
///   monster sees that cell right now. The monster sees exactly as far as
///   the player does (the `fov` module), so by the symmetry of sight
///   between floor cells the caller may answer with the player's own field
///   of view.
/// - `last_seen` holds the cell where the monster last saw the player and
///   has not reached yet, or `None`; it is kept from one turn to the next.
/// - `occupied` tells which cells hold an actor (the monster's own cell and
///   the player's may be included); the path never goes through them.
///
/// The rules, in order:
///
/// 1. A monster that sees the player remembers the player's cell.
/// 2. With nothing remembered, it waits.
/// 3. Next to the player it sees, it moves into the player's cell, which
///    the caller turns into an attack (see the `game` module).
/// 4. On the remembered cell, or next to it while another actor holds it,
///    it forgets the cell and waits: the player is gone.
/// 5. Otherwise it steps along a shortest path to the remembered cell (the
///    player's cell when it is in sight), around walls and other actors. If
///    no such path exists, it waits and keeps the memory, since actors in
///    the way may move.
pub fn choose_action(
    map: &Map,
    monster: Position,
    player: Position,
    sees_player: bool,
    last_seen: &mut Option<Position>,
    occupied: impl Fn(Position) -> bool,
) -> Action {
    if sees_player {
        *last_seen = Some(player);
    }
    let Some(target) = *last_seen else {
        return Action::Wait;
    };
    if sees_player && let Some(direction) = direction_between(monster, player) {
        return Action::Move(direction);
    }
    if monster == target || (are_adjacent(monster, target) && occupied(target)) {
        *last_seen = None;
        return Action::Wait;
    }
    find_path(map, monster, target, occupied)
        .and_then(|path| path.first().copied())
        .map_or(Action::Wait, Action::Move)
}

/// Returns whether `a` and `b` are distinct cells one step apart, diagonals
/// included.
fn are_adjacent(a: Position, b: Position) -> bool {
    a != b && a.x.abs_diff(b.x) <= 1 && a.y.abs_diff(b.y) <= 1
}

/// Returns the direction of the single step from `from` to `to`, or `None`
/// when the two cells are not adjacent.
fn direction_between(from: Position, to: Position) -> Option<Direction> {
    Direction::ALL
        .into_iter()
        .find(|&direction| from.step(direction) == to)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Direction;
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

    /// An open room with a wall in the middle, so straight lines are not
    /// always shortest paths.
    fn room() -> Map {
        from_rows(&[
            "#########", //
            "#.......#", //
            "#...#...#", //
            "#...#...#", //
            "#.......#", //
            "#########", //
        ])
    }

    #[test]
    fn a_monster_that_sees_the_player_steps_along_a_shortest_path() {
        let map = room();
        let player = at(6, 2);
        let distances = DijkstraMap::new(&map, [player]);
        let mut memory = None;

        let action = choose_action(&map, at(2, 3), player, true, &mut memory, nobody);

        let Action::Move(direction) = action else {
            panic!("expected a move, got {action:?}");
        };
        let next = at(2, 3).step(direction);
        assert_eq!(
            distances.distance(next),
            distances.distance(at(2, 3)).map(|d| d - 1),
            "one step closer by the shortest path"
        );
        assert_eq!(memory, Some(player));
    }

    #[test]
    fn a_monster_next_to_the_player_moves_into_it_to_attack() {
        let map = room();
        let player = at(2, 2);
        let mut memory = None;

        for direction in Direction::ALL {
            let monster = player.step(direction);
            // The player's cell is occupied, which must not stop the blow.
            let occupied = |p: Position| p == player;
            let action = choose_action(&map, monster, player, true, &mut memory, occupied);

            let Action::Move(towards) = action else {
                panic!("expected a move into the player, got {action:?}");
            };
            assert_eq!(monster.step(towards), player, "from {direction:?}");
            assert_eq!(memory, Some(player));
        }
    }

    #[test]
    fn a_monster_next_to_a_player_it_does_not_see_does_not_attack() {
        // Only reachable if sight said no; the monster then acts on memory.
        let map = room();
        let mut memory = None;

        let action = choose_action(&map, at(3, 2), at(2, 2), false, &mut memory, nobody);

        assert_eq!(action, Action::Wait);
    }

    #[test]
    fn the_direction_between_two_cells_is_the_single_step_joining_them() {
        for direction in Direction::ALL {
            assert_eq!(
                direction_between(at(4, 4), at(4, 4).step(direction)),
                Some(direction)
            );
        }
        assert_eq!(direction_between(at(4, 4), at(4, 4)), None);
        assert_eq!(direction_between(at(4, 4), at(6, 4)), None);
    }

    #[test]
    fn a_monster_that_never_saw_the_player_waits() {
        let map = room();
        let mut memory = None;

        let action = choose_action(&map, at(1, 1), at(7, 4), false, &mut memory, nobody);

        assert_eq!(action, Action::Wait);
        assert_eq!(memory, None);
    }

    #[test]
    fn out_of_sight_it_heads_for_the_last_seen_cell_not_the_player() {
        let map = room();
        let mut memory = Some(at(7, 1));

        // The player is now somewhere else, unseen.
        let action = choose_action(&map, at(5, 1), at(1, 4), false, &mut memory, nobody);

        assert_eq!(action, Action::Move(Direction::East));
        assert_eq!(memory, Some(at(7, 1)), "the memory is kept on the way");
    }

    #[test]
    fn on_the_last_seen_cell_it_forgets_and_waits() {
        let map = room();
        let mut memory = Some(at(7, 1));

        let action = choose_action(&map, at(7, 1), at(1, 4), false, &mut memory, nobody);

        assert_eq!(action, Action::Wait);
        assert_eq!(memory, None);
        // And it keeps waiting afterwards.
        let again = choose_action(&map, at(7, 1), at(1, 4), false, &mut memory, nobody);
        assert_eq!(again, Action::Wait);
    }

    #[test]
    fn next_to_a_last_seen_cell_held_by_another_actor_it_forgets_and_waits() {
        let map = room();
        let mut memory = Some(at(7, 1));
        let occupied = |p: Position| p == at(7, 1);

        let action = choose_action(&map, at(6, 1), at(1, 4), false, &mut memory, occupied);

        assert_eq!(action, Action::Wait);
        assert_eq!(memory, None);
    }

    #[test]
    fn it_goes_around_other_actors() {
        let map = from_rows(&[
            ".....", //
            ".....", //
            ".....", //
        ]);
        // Another monster stands right between the two.
        let occupied = |p: Position| p == at(1, 1);
        let mut memory = None;

        let action = choose_action(&map, at(0, 1), at(3, 1), true, &mut memory, occupied);

        assert!(
            matches!(
                action,
                Action::Move(Direction::NorthEast | Direction::SouthEast)
            ),
            "{action:?}"
        );
    }

    #[test]
    fn with_no_way_through_it_waits_and_keeps_the_memory() {
        let map = from_rows(&[
            "#####", //
            ".....", //
            "#####", //
        ]);
        let occupied = |p: Position| p == at(2, 1);
        let mut memory = None;

        let action = choose_action(&map, at(0, 1), at(4, 1), true, &mut memory, occupied);

        assert_eq!(action, Action::Wait);
        assert_eq!(memory, Some(at(4, 1)));
    }

    #[test]
    fn adjacency_includes_diagonals_and_excludes_the_cell_itself() {
        assert!(are_adjacent(at(3, 3), at(4, 4)));
        assert!(are_adjacent(at(3, 3), at(3, 2)));
        assert!(!are_adjacent(at(3, 3), at(3, 3)));
        assert!(!are_adjacent(at(3, 3), at(5, 3)));
    }
}
