//! Creatures that take turns on the map, the player included.
//!
//! Exposes [`ActorId`], a generational key that never points to a removed
//! actor, and [`Actor`], which holds where an actor stands and how fast it
//! acts. Hit points, attacks and behaviour are not modelled yet. How energy
//! turns into turns is decided by the `schedule` module.

use crate::geometry::Position;

slotmap::new_key_type! {
    /// Identifies one actor in the game.
    ///
    /// Keys are generational: once an actor is removed, its old key no
    /// longer finds anything, even if its storage slot is reused.
    pub struct ActorId;
}

/// A creature that occupies one cell and acts when it has stored enough
/// energy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    /// Cell the actor stands on.
    pub(crate) position: Position,
    /// Energy gained every tick; always strictly positive.
    pub(crate) speed: i32,
    /// Energy stored towards the next turn. It can drop below zero when an
    /// action costs more than the actor had stored.
    pub(crate) energy: i32,
}

impl Actor {
    /// Creates an actor standing at `position` with no stored energy.
    ///
    /// `speed` is the energy gained per tick: `schedule::NORMAL_SPEED` acts
    /// once per tick with actions at the normal cost, twice that speed acts
    /// twice as often.
    ///
    /// # Panics
    ///
    /// Panics if `speed` is zero or negative: such an actor would never act
    /// and the scheduler would wait for it forever, so it is a bug in the code
    /// creating it.
    pub fn new(position: Position, speed: i32) -> Self {
        assert!(
            speed > 0,
            "actor speed must be positive, got {speed} for the actor at ({}, {})",
            position.x,
            position.y
        );
        Self {
            position,
            speed,
            energy: 0,
        }
    }

    /// Returns the cell the actor stands on.
    pub const fn position(&self) -> Position {
        self.position
    }

    /// Returns the energy the actor gains every tick.
    pub const fn speed(&self) -> i32 {
        self.speed
    }

    /// Returns the energy stored towards the actor's next turn; negative after
    /// an action that cost more than was stored.
    pub const fn energy(&self) -> i32 {
        self.energy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_actor_starts_without_energy() {
        let actor = Actor::new(Position::new(2, 3), 150);

        assert_eq!(actor.position(), Position::new(2, 3));
        assert_eq!(actor.speed(), 150);
        assert_eq!(actor.energy(), 0);
    }

    #[test]
    #[should_panic(expected = "actor speed must be positive, got 0")]
    fn zero_speed_is_rejected() {
        Actor::new(Position::new(1, 1), 0);
    }
}
