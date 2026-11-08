//! What happened during a turn, reported by `Game::submit`.
//!
//! Exposes [`Event`]. Events are plain data with no text: the binary turns
//! them into message log lines and animations, and end-of-run statistics are
//! computed from them.
//!
//! Every [`ActorId`] in the events of one `Game::submit` call names either an
//! actor still in the game afterwards, or one that died during that call, in
//! which case the [`Event::Died`] of that actor follows in the same list and
//! tells who it was. Keys are generational, so a dead actor's key never names
//! another actor later.

use crate::actor::ActorId;
use crate::combat::Combatant;
use crate::geometry::Position;

/// Something that happened in the game, in the order it happened.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
    /// of the map or another actor it does not fight). It did not move,
    /// spent no energy and no time passed.
    MoveBlocked {
        /// Actor whose move was refused.
        actor: ActorId,
        /// Cell it stands on, unchanged.
        from: Position,
        /// Cell it tried to enter.
        to: Position,
    },
    /// An actor struck a foe on a neighbouring cell. Blows always land: the
    /// player strikes monsters and monsters strike the player, never each
    /// other.
    Attacked {
        /// Actor that struck.
        attacker: ActorId,
        /// Actor that was struck.
        target: ActorId,
        /// Hit points the blow took off, at least `combat::MIN_DAMAGE`. It
        /// may exceed what the target had left.
        damage: i32,
        /// Hit points the target has left after the blow, from 0 (the blow
        /// killed it, and [`Event::Died`] follows) to its maximum.
        hp_left: i32,
    },
    /// An actor's hit points fell to 0.
    ///
    /// A dead monster has already left the map and the turn order. When the
    /// actor is the player, the game is over (`Game::status`): the player
    /// stays on its cell and nothing else happens.
    Died {
        /// Actor that died; its key no longer finds a monster in the game.
        actor: ActorId,
        /// Who it was, so it can still be named.
        who: Combatant,
        /// Who dealt the killing blow: the cause of death, kept for the
        /// end-of-run summary.
        killed_by: Combatant,
    },
}
