//! Creatures that take turns on the map, the player included.
//!
//! Exposes [`ActorId`], a generational key that never points to a removed
//! actor, [`Actor`], which holds where an actor stands, how fast it acts and
//! what it is, [`ActorKind`] and [`Monster`]. Hit points and attacks are not
//! modelled yet. How energy turns into turns is decided by the `schedule`
//! module, what a monster does by the `ai` module.

use crate::content::{Family, MonsterDef};
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
    /// Whether this is the player or a monster, and which one.
    pub(crate) kind: ActorKind,
}

impl Actor {
    /// Creates an actor of the given `kind` standing at `position` with no
    /// stored energy.
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
    pub fn new(position: Position, speed: i32, kind: ActorKind) -> Self {
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
            kind,
        }
    }

    /// Creates a monster of kind `definition` standing at `position`, with
    /// the definition's speed and `energy` already stored.
    ///
    /// `energy` is a head start towards the monster's first turn: `0` for
    /// none, and below `schedule::ACTION_THRESHOLD` for monsters placed on a
    /// new floor (see `generation::MonsterSpawn::energy`). The monster has
    /// no memory of the player yet.
    pub fn monster(position: Position, definition: &MonsterDef, energy: i32) -> Self {
        let mut actor = Self::new(
            position,
            definition.speed(),
            ActorKind::Monster(Monster::new(definition)),
        );
        actor.energy = energy;
        actor
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

    /// Returns whether this is the player or a monster, and which one.
    pub const fn kind(&self) -> &ActorKind {
        &self.kind
    }

    /// Returns the monster's own data, or `None` for the player.
    pub const fn as_monster(&self) -> Option<&Monster> {
        match &self.kind {
            ActorKind::Monster(monster) => Some(monster),
            ActorKind::Player => None,
        }
    }
}

/// What an actor is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActorKind {
    /// The character controlled by the person playing.
    Player,
    /// A creature described in the content files, moved by the `ai` module.
    Monster(Monster),
}

/// What makes a monster more than a position and a speed: the kind of
/// creature it is, and what it remembers of the player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Monster {
    /// Identifier of its definition in the content files
    /// (`MonsterDef::id`).
    definition: String,
    /// Family of its definition, which gives the letter drawn on the map.
    family: Family,
    /// Whether its definition is the elite form of the family.
    elite: bool,
    /// Where the monster last saw the player, until it gets there; `None`
    /// when it has not seen the player since it last reached that cell.
    /// Read and updated by the `ai` module.
    pub(crate) last_seen_player: Option<Position>,
}

impl Monster {
    /// Creates the monster data of a creature of kind `definition`, with no
    /// memory of the player.
    fn new(definition: &MonsterDef) -> Self {
        Self {
            definition: definition.id().to_owned(),
            family: definition.family(),
            elite: definition.elite(),
            last_seen_player: None,
        }
    }

    /// Returns the identifier of the monster's definition in the content
    /// files, the key `Content::monster` finds it by.
    pub fn definition(&self) -> &str {
        &self.definition
    }

    /// Returns the monster's family.
    pub const fn family(&self) -> Family {
        self.family
    }

    /// Returns whether the monster is the elite form of its family.
    pub const fn elite(&self) -> bool {
        self.elite
    }

    /// Returns the cell where the monster last saw the player and is still
    /// heading to, or `None` (see the `ai` module).
    pub const fn last_seen_player(&self) -> Option<Position> {
        self.last_seen_player
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::content::Content;

    #[test]
    fn new_actor_starts_without_energy() {
        let actor = Actor::new(Position::new(2, 3), 150, ActorKind::Player);

        assert_eq!(actor.position(), Position::new(2, 3));
        assert_eq!(actor.speed(), 150);
        assert_eq!(actor.energy(), 0);
        assert_eq!(actor.kind(), &ActorKind::Player);
        assert!(actor.as_monster().is_none());
    }

    #[test]
    fn a_monster_takes_its_definition_and_head_start() {
        let content = Content::load().expect("shipped content is valid");
        let definition = content.monster("ossuary_rat").expect("shipped monster");

        let actor = Actor::monster(Position::new(4, 1), definition, 37);

        assert_eq!(actor.position(), Position::new(4, 1));
        assert_eq!(actor.speed(), definition.speed());
        assert_eq!(actor.energy(), 37);
        let monster = actor.as_monster().expect("a monster");
        assert_eq!(monster.definition(), "ossuary_rat");
        assert_eq!(monster.family(), definition.family());
        assert_eq!(monster.elite(), definition.elite());
        assert_eq!(monster.last_seen_player(), None);
    }

    #[test]
    #[should_panic(expected = "actor speed must be positive, got 0")]
    fn zero_speed_is_rejected() {
        Actor::new(Position::new(1, 1), 0, ActorKind::Player);
    }
}
