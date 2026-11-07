//! What happened during a turn, reported by `Game::submit`.
//!
//! Exposes [`Event`]. Events are plain data with no text: the binary turns
//! them into message log lines and animations, and end-of-run statistics are
//! computed from them.

use crate::actor::ActorId;
use crate::geometry::Position;

/// Something that happened in the game, in the order it happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Event {
    /// An actor stepped from one cell to a neighbouring one.
    Moved {
        /// Actor that moved.
        actor: ActorId,
        /// Cell it left.
        from: Position,
        /// Cell it now stands on.
        to: Position,
    },
    /// An actor stayed in place and let time pass.
    Waited {
        /// Actor that waited.
        actor: ActorId,
    },
    /// An actor tried to step into a cell it cannot enter (a wall, the edge
    /// of the map or another actor). It did not move, spent no energy and no
    /// time passed.
    MoveBlocked {
        /// Actor whose move was refused.
        actor: ActorId,
        /// Cell it stands on, unchanged.
        from: Position,
        /// Cell it tried to enter.
        to: Position,
    },
}
