//! What an actor can attempt on its turn, and whether the attempt succeeds.
//!
//! Exposes [`Action`], [`ActionOutcome`] and [`resolve`], a pure function
//! that checks an action against the map. It does not move anything, spend
//! time or energy, or produce game events: the caller applies the outcome.

use crate::geometry::{Direction, Position};
use crate::item::ItemId;
use crate::map::Map;
use crate::sigil::Trace;

/// An action an actor attempts on its turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    /// Step one cell in the given direction.
    Move(Direction),
    /// Stay in place and let time pass.
    Wait,
    /// Cast the sigil drawn as `trace`, submitted whole once the player
    /// validates it: the strokes and the validation are paid at once (see
    /// `schedule::action_cost`), and the monsters act for that long
    /// afterwards. Only the player casts.
    Cast {
        /// The finished trace, as the player drew it.
        trace: Trace,
        /// Where the spell is aimed, for a sigil aimed in a direction
        /// (`content::Targeting::Direction`); ignored by the others.
        direction: Option<Direction>,
    },
    /// Pick up the item lying on the actor's cell. Only the player handles
    /// items.
    PickUp,
    /// Use the carried item, which is spent. Only the player handles items.
    Use(ItemId),
    /// Lay the carried item on the actor's cell. Only the player handles
    /// items.
    Drop(ItemId),
    /// Go down the stairs the actor stands on, to the next floor. Only the
    /// player descends; the game refuses it anywhere but on the stairs.
    Descend,
}

/// The result of resolving an [`Action`] against the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionOutcome {
    /// The action happened; the actor now stands at `position`, which equals
    /// its starting cell for [`Action::Wait`].
    Performed {
        /// Cell the actor occupies after the action.
        position: Position,
    },
    /// The action could not happen (a wall or the edge of the map is in the
    /// way); the actor stays where it was.
    Blocked,
}

/// Works out what `action` does for an actor standing at `from` on `map`.
///
/// A move succeeds when [`Map::can_step`] allows it: the destination cell is
/// walkable, diagonals included even between two walls touching by a corner.
/// Walls and cells outside the map block it. Waiting, casting, handling
/// items and descending always succeed here and keep the actor in place;
/// what a cast, an item or a descent does, and whether it is allowed, is
/// decided by the game. The
/// function is pure: neither `map` nor any actor is modified.
pub fn resolve(action: Action, from: Position, map: &Map) -> ActionOutcome {
    match action {
        Action::Wait
        | Action::Cast { .. }
        | Action::PickUp
        | Action::Use(_)
        | Action::Drop(_)
        | Action::Descend => ActionOutcome::Performed { position: from },
        Action::Move(direction) => {
            if map.can_step(from, direction) {
                ActionOutcome::Performed {
                    position: from.step(direction),
                }
            } else {
                ActionOutcome::Blocked
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{FIXED_ROOM_PILLAR, Tile};

    /// An open cell of the fixed room with floor on all eight sides.
    const ROOM_CENTRE: Position = Position::new(5, 5);

    #[test]
    fn moves_one_cell_in_each_of_the_eight_directions() {
        let map = Map::fixed_room();
        let expected = [
            (Direction::North, Position::new(5, 4)),
            (Direction::NorthEast, Position::new(6, 4)),
            (Direction::East, Position::new(6, 5)),
            (Direction::SouthEast, Position::new(6, 6)),
            (Direction::South, Position::new(5, 6)),
            (Direction::SouthWest, Position::new(4, 6)),
            (Direction::West, Position::new(4, 5)),
            (Direction::NorthWest, Position::new(4, 4)),
        ];

        for (direction, position) in expected {
            assert_eq!(
                resolve(Action::Move(direction), ROOM_CENTRE, &map),
                ActionOutcome::Performed { position },
                "moving {direction:?}"
            );
        }
    }

    #[test]
    fn outer_wall_blocks_every_move_towards_it() {
        let map = Map::fixed_room();
        let top_left_corner = Position::new(1, 1);

        for direction in [
            Direction::North,
            Direction::NorthEast,
            Direction::West,
            Direction::NorthWest,
            Direction::SouthWest,
        ] {
            assert_eq!(
                resolve(Action::Move(direction), top_left_corner, &map),
                ActionOutcome::Blocked,
                "moving {direction:?}"
            );
        }
        assert_eq!(
            resolve(Action::Move(Direction::SouthEast), top_left_corner, &map),
            ActionOutcome::Performed {
                position: Position::new(2, 2)
            }
        );
    }

    #[test]
    fn pillar_blocks_moves_from_all_eight_sides() {
        let map = Map::fixed_room();

        for direction in Direction::ALL {
            let (dx, dy) = direction.delta();
            let neighbour = Position::new(FIXED_ROOM_PILLAR.x - dx, FIXED_ROOM_PILLAR.y - dy);
            assert_eq!(
                resolve(Action::Move(direction), neighbour, &map),
                ActionOutcome::Blocked,
                "moving {direction:?} from {neighbour:?}"
            );
        }
    }

    #[test]
    fn map_edge_blocks_moves_even_without_a_wall() {
        let map = Map::new(2, 2, Tile::Floor);

        for direction in [Direction::North, Direction::West, Direction::NorthWest] {
            assert_eq!(
                resolve(Action::Move(direction), Position::new(0, 0), &map),
                ActionOutcome::Blocked,
                "moving {direction:?}"
            );
        }
        for direction in [Direction::South, Direction::East, Direction::SouthEast] {
            assert_eq!(
                resolve(Action::Move(direction), Position::new(1, 1), &map),
                ActionOutcome::Blocked,
                "moving {direction:?}"
            );
        }
    }

    #[test]
    fn diagonal_move_cuts_between_two_walls() {
        // .#
        // #.
        let mut map = Map::new(2, 2, Tile::Floor);
        map.set_tile(Position::new(1, 0), Tile::Wall);
        map.set_tile(Position::new(0, 1), Tile::Wall);

        assert_eq!(
            resolve(
                Action::Move(Direction::SouthEast),
                Position::new(0, 0),
                &map
            ),
            ActionOutcome::Performed {
                position: Position::new(1, 1)
            }
        );
    }

    #[test]
    fn waiting_casting_and_handling_items_keep_the_actor_in_place() {
        let map = Map::fixed_room();
        let cast = Action::Cast {
            trace: Trace::from_pattern(&[7, 5, 3]).expect("a drawable pattern"),
            direction: Some(Direction::East),
        };
        let item = ItemId::default();

        for action in [
            Action::Wait,
            cast,
            Action::PickUp,
            Action::Use(item),
            Action::Drop(item),
        ] {
            assert_eq!(
                resolve(action, ROOM_CENTRE, &map),
                ActionOutcome::Performed {
                    position: ROOM_CENTRE
                },
                "{action:?}"
            );
        }
    }
}
