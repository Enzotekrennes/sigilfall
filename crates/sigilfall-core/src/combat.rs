//! Fighting rules: the player's starting values and how much a blow hurts.
//!
//! Exposes [`CombatStats`], the player's starting values ([`PLAYER_STATS`]
//! and its parts), the damage formula ([`damage`], [`roll_damage`] and their
//! constants) and [`Combatant`], who took part in a fight as recorded in
//! events. This module neither decides who fights nor changes the world:
//! the `game` module finds the target of an attack, applies the damage,
//! spends the energy and removes the dead.
//!
//! The player's values are named constants rather than content data: there
//! is a single player, items and sigils will change its values through code
//! anyway, and a constant keeps the rule and its justification side by side.

use rand::RngExt as _;
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

use crate::content::MonsterDef;

/// Hit points of the player at the start of a run.
///
/// Hit points never come back on their own, only through healing items and
/// sigils, so this is the budget of a whole run rather than of one fight:
/// an acolyte, met from the second floor, deals 2 per blow on average
/// against [`PLAYER_DEFENSE`], and the player survives about fifteen of its
/// blows. Measured in `tests/balance.rs`, a player who charges every
/// creature and never heals spends it between the second and the fourth
/// floor, the balance aimed at; deeper floors ask for items and sigils.
pub const PLAYER_MAX_HP: i32 = 30;

/// Attack value of the player at the start of a run.
///
/// Provisional: it kills an ossuary rat (4 hit points, no defense) in one or
/// two blows and an acolyte (10 hit points, defense 1) in about four.
pub const PLAYER_ATTACK: i32 = 4;

/// Defense value of the player at the start of a run.
///
/// Provisional: it halves on average the blows of the weakest creatures
/// (attack 2) without making any of them harmless, since a blow always
/// deals at least [`MIN_DAMAGE`].
pub const PLAYER_DEFENSE: i32 = 1;

/// Every starting combat value of the player.
pub const PLAYER_STATS: CombatStats = CombatStats {
    max_hp: PLAYER_MAX_HP,
    attack: PLAYER_ATTACK,
    defense: PLAYER_DEFENSE,
};

/// Largest amount a blow deals above or below `attack - defense`.
///
/// 1 keeps fights readable (a blow is always within one point of what the
/// values promise) while keeping two identical fights from playing out the
/// same way.
pub const DAMAGE_SPREAD: i32 = 1;

/// Least damage a blow deals, whatever the defense.
///
/// 1, so that every fight ends: a creature whose defense matches or exceeds
/// its foe's attack can still be worn down.
pub const MIN_DAMAGE: i32 = 1;

/// The fixed fighting values of an actor; its current hit points are kept on
/// the actor itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CombatStats {
    /// Hit points of the actor when unhurt; strictly positive.
    pub max_hp: i32,
    /// Attack value, added to the damage of its blows; 0 or more.
    pub attack: i32,
    /// Defense value, subtracted from the damage of blows it takes; 0 or
    /// more.
    pub defense: i32,
}

impl CombatStats {
    /// Returns the values of a fresh creature of kind `definition`, taken
    /// from the content files as they are.
    pub const fn of_monster(definition: &MonsterDef) -> Self {
        Self {
            max_hp: definition.hp(),
            attack: definition.attack(),
            defense: definition.defense(),
        }
    }
}

/// Returns the damage of a blow of an actor with `attack` against one with
/// `defense`, for a given `roll`.
///
/// The formula is `max(MIN_DAMAGE, attack - defense + roll)`. `roll` is
/// expected from `-DAMAGE_SPREAD` to `DAMAGE_SPREAD` (see [`roll_damage`]),
/// but any value is accepted. The result is always at least [`MIN_DAMAGE`].
/// Values come from content bounds (at most 99), so the sum cannot overflow.
pub const fn damage(attack: i32, defense: i32, roll: i32) -> i32 {
    let raw = attack - defense + roll;
    if raw < MIN_DAMAGE { MIN_DAMAGE } else { raw }
}

/// Draws the roll of one blow from `rng` and returns its [`damage`].
///
/// The roll is uniform from `-DAMAGE_SPREAD` to `DAMAGE_SPREAD` and takes
/// exactly one draw, so a given gameplay stream (`Seed::stream` with
/// `Purpose::Gameplay`) always gives the same blows in the same order.
pub fn roll_damage(attack: i32, defense: i32, rng: &mut ChaCha8Rng) -> i32 {
    let roll = rng.random_range(-DAMAGE_SPREAD..=DAMAGE_SPREAD);
    damage(attack, defense, roll)
}

/// Who took part in a fight, kept in events so the message log and the
/// end-of-run summary can name an actor after it has been removed.
///
/// A save keeps it in the status of a finished game, as `Player` or
/// `Monster(definition: "ossuary_rat")`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Combatant {
    /// The character controlled by the person playing.
    Player,
    /// A creature of the content files.
    Monster {
        /// Identifier of its definition (`MonsterDef::id`), the key
        /// `Content::monster` finds its name by.
        definition: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::Content;
    use crate::generation::FIRST_DEPTH;
    use crate::rng::{Purpose, Seed};

    /// Draws `count` blows of `attack` against `defense` from the gameplay
    /// stream of `seed` on the first floor.
    fn blows(seed: u64, attack: i32, defense: i32, count: usize) -> Vec<i32> {
        let mut rng = Seed::new(seed).stream(FIRST_DEPTH, Purpose::Gameplay);
        (0..count)
            .map(|_| roll_damage(attack, defense, &mut rng))
            .collect()
    }

    #[test]
    fn damage_is_attack_minus_defense_plus_the_roll() {
        assert_eq!(damage(4, 1, 0), 3);
        assert_eq!(damage(4, 1, DAMAGE_SPREAD), 4);
        assert_eq!(damage(4, 1, -DAMAGE_SPREAD), 2);
        assert_eq!(damage(10, 0, 0), 10);
    }

    #[test]
    fn a_blow_always_deals_the_minimum() {
        assert_eq!(damage(2, 2, -DAMAGE_SPREAD), MIN_DAMAGE);
        assert_eq!(damage(0, 0, 0), MIN_DAMAGE);
        assert_eq!(damage(1, 99, DAMAGE_SPREAD), MIN_DAMAGE);
    }

    #[test]
    fn the_same_seed_gives_the_same_blows() {
        for seed in [0, 42, u64::MAX] {
            assert_eq!(blows(seed, 4, 1, 50), blows(seed, 4, 1, 50), "{seed}");
        }
        assert_ne!(blows(1, 4, 1, 50), blows(2, 4, 1, 50));
    }

    #[test]
    fn blows_of_seed_42_are_frozen() {
        // Pins the formula and the use of the gameplay stream: if this
        // fails, a shared seed no longer replays the same fights.
        assert_eq!(
            blows(42, PLAYER_ATTACK, 1, 12),
            [4, 2, 3, 4, 2, 4, 2, 2, 2, 4, 2, 2]
        );
    }

    #[test]
    fn rolls_cover_the_whole_spread_and_nothing_more() {
        let mut seen = blows(7, 10, 0, 300);
        seen.sort_unstable();
        seen.dedup();

        assert_eq!(seen, [9, 10, 11]);
    }

    #[test]
    fn monster_stats_come_from_their_definition() {
        let content = Content::load().expect("shipped content is valid");
        for definition in content.monsters() {
            let stats = CombatStats::of_monster(definition);

            assert_eq!(stats.max_hp, definition.hp(), "{}", definition.id());
            assert_eq!(stats.attack, definition.attack(), "{}", definition.id());
            assert_eq!(stats.defense, definition.defense(), "{}", definition.id());
        }
    }
}
