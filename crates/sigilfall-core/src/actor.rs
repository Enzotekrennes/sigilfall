//! Creatures that take turns on the map, the player included.
//!
//! Exposes [`ActorId`], a generational key that never points to a removed
//! actor, [`Actor`], which holds where an actor stands, how fast it acts,
//! how it fights, how hurt it is, how much essence it has, how many turns
//! it is held for and what it is, [`ActorKind`] and [`Monster`]. How energy
//! turns into turns is decided by the `schedule` module, what a monster does
//! by the `ai` module, how much a blow hurts by the `combat` module, and
//! what casting costs by the `sigil` module.

use crate::combat::{CombatStats, PLAYER_STATS};
use crate::content::{Family, MonsterDef};
use crate::geometry::Position;
use crate::schedule::NORMAL_SPEED;
use crate::sigil::MAX_ESSENCE;

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
    /// Fixed fighting values: hit points when unhurt, attack and defense.
    stats: CombatStats,
    /// Hit points left, from 0 to `stats.max_hp`; 0 means dead.
    hp: i32,
    /// Most essence the actor can hold: `sigil::MAX_ESSENCE` for the
    /// player, 0 for creatures, which never cast.
    max_essence: i32,
    /// Essence left for casting, from 0 to `max_essence`.
    essence: i32,
    /// Number of the actor's next turns it loses, held by a sigil or dazed
    /// by a backlash; 0 when it acts freely.
    pub(crate) held_turns: u32,
    /// Whether this is the player or a monster, and which one.
    pub(crate) kind: ActorKind,
}

impl Actor {
    /// Creates an unhurt actor of the given `kind` standing at `position`
    /// with no stored energy, no essence and nothing holding it.
    ///
    /// `speed` is the energy gained per tick: `schedule::NORMAL_SPEED` acts
    /// once per tick with actions at the normal cost, twice that speed acts
    /// twice as often. `stats` are its fighting values; it starts with
    /// `stats.max_hp` hit points.
    ///
    /// # Panics
    ///
    /// Panics if `speed` or `stats.max_hp` is zero or negative: such an actor
    /// would never act, or would be dead from the start, so it is a bug in
    /// the code creating it.
    pub fn new(position: Position, speed: i32, stats: CombatStats, kind: ActorKind) -> Self {
        assert!(
            speed > 0,
            "actor speed must be positive, got {speed} for the actor at ({}, {})",
            position.x,
            position.y
        );
        assert!(
            stats.max_hp > 0,
            "actor hit points must be positive, got {} for the actor at ({}, {})",
            stats.max_hp,
            position.x,
            position.y
        );
        Self {
            position,
            speed,
            energy: 0,
            stats,
            hp: stats.max_hp,
            max_essence: 0,
            essence: 0,
            held_turns: 0,
            kind,
        }
    }

    /// Creates the player, unhurt, standing at `position`, at normal speed,
    /// with the starting values of `combat::PLAYER_STATS` and a full reserve
    /// of `sigil::MAX_ESSENCE` essence.
    pub fn player(position: Position) -> Self {
        let mut player = Self::new(position, NORMAL_SPEED, PLAYER_STATS, ActorKind::Player);
        player.max_essence = MAX_ESSENCE;
        player.essence = MAX_ESSENCE;
        player
    }

    /// Creates a monster of kind `definition` standing at `position`, with
    /// the definition's speed, hit points, attack and defense, and `energy`
    /// already stored.
    ///
    /// `energy` is a head start towards the monster's first turn: `0` for
    /// none, and below `schedule::ACTION_THRESHOLD` for monsters placed on a
    /// new floor (see `generation::MonsterSpawn::energy`). The monster has
    /// no memory of the player yet.
    pub fn monster(position: Position, definition: &MonsterDef, energy: i32) -> Self {
        let mut actor = Self::new(
            position,
            definition.speed(),
            CombatStats::of_monster(definition),
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

    /// Returns the hit points left: from 0 (dead) to [`Actor::max_hp`].
    pub const fn hp(&self) -> i32 {
        self.hp
    }

    /// Returns the hit points of the actor when unhurt; always positive.
    pub const fn max_hp(&self) -> i32 {
        self.stats.max_hp
    }

    /// Returns the attack value, which raises the damage of its blows.
    pub const fn attack(&self) -> i32 {
        self.stats.attack
    }

    /// Returns the defense value, which lowers the damage of the blows it
    /// takes.
    pub const fn defense(&self) -> i32 {
        self.stats.defense
    }

    /// Returns whether the actor still has hit points left.
    pub const fn is_alive(&self) -> bool {
        self.hp > 0
    }

    /// Takes `damage` hit points off the actor, stopping at 0.
    ///
    /// `damage` is expected positive (see `combat::damage`). Removing a dead
    /// monster or ending the game is up to the caller.
    pub(crate) fn take_damage(&mut self, damage: i32) {
        self.hp = (self.hp - damage).max(0);
    }

    /// Restores up to `amount` hit points, never above [`Actor::max_hp`],
    /// and returns how many were actually restored (0 when unhurt).
    ///
    /// `amount` is expected positive, such as a sigil's power.
    pub(crate) fn heal(&mut self, amount: i32) -> i32 {
        let before = self.hp;
        self.hp = (self.hp + amount).min(self.stats.max_hp);
        self.hp - before
    }

    /// Returns the essence left for casting, from 0 to
    /// [`Actor::max_essence`]; always 0 for a creature.
    pub const fn essence(&self) -> i32 {
        self.essence
    }

    /// Returns the most essence the actor can hold: `sigil::MAX_ESSENCE` for
    /// the player, 0 for a creature.
    pub const fn max_essence(&self) -> i32 {
        self.max_essence
    }

    /// Sets the essence to `essence`, kept within 0 and
    /// [`Actor::max_essence`].
    pub(crate) fn set_essence(&mut self, essence: i32) {
        self.essence = essence.clamp(0, self.max_essence);
    }

    /// Returns how many of its next turns the actor will lose, held by a
    /// sigil or dazed by a backlash; 0 when it acts freely.
    pub const fn held_turns(&self) -> u32 {
        self.held_turns
    }

    /// Returns whether this is the player or a monster, and which one.
    pub const fn kind(&self) -> &ActorKind {
        &self.kind
    }

    /// Returns whether this actor is the player.
    pub const fn is_player(&self) -> bool {
        matches!(self.kind, ActorKind::Player)
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

    /// Fighting values of the test actors.
    const STATS: CombatStats = CombatStats {
        max_hp: 7,
        attack: 3,
        defense: 1,
    };

    #[test]
    fn new_actor_starts_unhurt_and_without_energy() {
        let actor = Actor::new(Position::new(2, 3), 150, STATS, ActorKind::Player);

        assert_eq!(actor.position(), Position::new(2, 3));
        assert_eq!(actor.speed(), 150);
        assert_eq!(actor.energy(), 0);
        assert_eq!(actor.hp(), 7);
        assert_eq!(actor.max_hp(), 7);
        assert_eq!(actor.attack(), 3);
        assert_eq!(actor.defense(), 1);
        assert!(actor.is_alive());
        assert_eq!(actor.kind(), &ActorKind::Player);
        assert!(actor.is_player());
        assert!(actor.as_monster().is_none());
        assert_eq!((actor.essence(), actor.max_essence()), (0, 0));
        assert_eq!(actor.held_turns(), 0);
    }

    #[test]
    fn the_player_starts_with_the_player_values() {
        let player = Actor::player(Position::new(1, 2));

        assert_eq!(player.position(), Position::new(1, 2));
        assert_eq!(player.speed(), NORMAL_SPEED);
        assert_eq!(player.hp(), PLAYER_STATS.max_hp);
        assert_eq!(player.attack(), PLAYER_STATS.attack);
        assert_eq!(player.defense(), PLAYER_STATS.defense);
        assert!(player.is_player());
        assert_eq!(player.essence(), MAX_ESSENCE);
        assert_eq!(player.max_essence(), MAX_ESSENCE);
    }

    #[test]
    fn healing_stops_at_the_maximum_and_essence_stays_in_bounds() {
        let mut player = Actor::player(Position::new(1, 1));
        player.take_damage(5);

        assert_eq!(player.heal(3), 3);
        assert_eq!(player.heal(10), 2, "only what was missing");
        assert_eq!(player.hp(), player.max_hp());
        assert_eq!(player.heal(4), 0);

        player.set_essence(-3);
        assert_eq!(player.essence(), 0);
        player.set_essence(MAX_ESSENCE + 5);
        assert_eq!(player.essence(), MAX_ESSENCE);
    }

    #[test]
    fn damage_lowers_hit_points_down_to_zero_only() {
        let mut actor = Actor::new(Position::new(1, 1), NORMAL_SPEED, STATS, ActorKind::Player);

        actor.take_damage(3);
        assert_eq!(actor.hp(), 4);
        assert!(actor.is_alive());

        actor.take_damage(10);
        assert_eq!(actor.hp(), 0);
        assert!(!actor.is_alive());
        assert_eq!(actor.max_hp(), 7, "the maximum is untouched");
    }

    #[test]
    fn a_monster_takes_its_definition_and_head_start() {
        let content = Content::load().expect("shipped content is valid");
        let definition = content.monster("ossuary_rat").expect("shipped monster");

        let actor = Actor::monster(Position::new(4, 1), definition, 37);

        assert_eq!(actor.position(), Position::new(4, 1));
        assert_eq!(actor.speed(), definition.speed());
        assert_eq!(actor.energy(), 37);
        assert_eq!(actor.hp(), definition.hp());
        assert_eq!(actor.max_hp(), definition.hp());
        assert_eq!(actor.attack(), definition.attack());
        assert_eq!(actor.defense(), definition.defense());
        assert!(!actor.is_player());
        let monster = actor.as_monster().expect("a monster");
        assert_eq!(monster.definition(), "ossuary_rat");
        assert_eq!(monster.family(), definition.family());
        assert_eq!(monster.elite(), definition.elite());
        assert_eq!(monster.last_seen_player(), None);
    }

    #[test]
    #[should_panic(expected = "actor speed must be positive, got 0")]
    fn zero_speed_is_rejected() {
        Actor::new(Position::new(1, 1), 0, STATS, ActorKind::Player);
    }

    #[test]
    #[should_panic(expected = "actor hit points must be positive, got 0")]
    fn zero_hit_points_are_rejected() {
        let stats = CombatStats { max_hp: 0, ..STATS };
        Actor::new(Position::new(1, 1), NORMAL_SPEED, stats, ActorKind::Player);
    }
}
