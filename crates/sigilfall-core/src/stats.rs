//! What the player did during a run, counted for the end-of-run summary.
//!
//! Exposes [`Stats`] and [`CastCounts`]. The game keeps one [`Stats`] and
//! updates it from the events of each `Game::submit` (decision D4), so the
//! counts always agree with what the message log reported: creatures killed
//! by kind, sigils cast by quality, glyphs read, items used, the deepest
//! floor reached and the player's turns. The binary only reads it
//! (`Game::stats`).
//!
//! Stats are part of the state of a game: a save keeps them, last, as
//! `stats: (turns: 12, deepest: 2, kills: {"ossuary_rat": 3}, casts: (pure:
//! 1, unstable: 0, fizzled: 0), glyphs_learnt: 1, items_used: 0)` (one field
//! per line), and a game read back goes on counting from there. This module neither names
//! nor draws anything: monster names and the layout of the summary belong
//! to the binary.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::combat::Combatant;
use crate::event::Event;
use crate::generation::FIRST_DEPTH;
use crate::sigil::CastQuality;

/// Sigils the player cast, counted by how well the trace matched
/// ([`CastQuality`]).
///
/// Only casts that were paid count: a trace refused for want of essence or
/// of a direction casts nothing. A cast made pure by a steadying item
/// counts as pure, as the game reports it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CastCounts {
    /// Casts whose trace was exactly the pattern of a known sigil.
    pub pure: u32,
    /// Casts one edit away from a known sigil, followed by a backlash.
    pub unstable: u32,
    /// Traces that matched no known sigil and fizzled.
    pub fizzled: u32,
}

impl CastCounts {
    /// Returns the number of casts of every quality together.
    pub const fn total(&self) -> u32 {
        self.pure
            .saturating_add(self.unstable)
            .saturating_add(self.fizzled)
    }
}

/// The counts of a run, kept by the game for the end-of-run summary.
///
/// Every count starts at 0 in a new game, except [`Stats::deepest`], which
/// starts at the first floor, and only grows. Counts saturate at their
/// maximum instead of overflowing, far beyond any real run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stats {
    /// Player actions that took time since the game started.
    turns: u64,
    /// Number of the deepest floor the player has stood on.
    deepest: u32,
    /// Monsters killed, by identifier of their definition
    /// (`MonsterDef::id`); a kind never killed has no entry. Sorted by
    /// identifier, so a save writes them in a stable order.
    kills: BTreeMap<String, u32>,
    /// Sigils cast, by quality.
    casts: CastCounts,
    /// Engraved glyphs read that taught a new sigil (`Event::GlyphLearned`).
    glyphs_learnt: u32,
    /// Carried items used up (`Event::Used`).
    items_used: u32,
}

impl Default for Stats {
    /// Returns the stats of a new game: nothing done yet, on the first
    /// floor.
    fn default() -> Self {
        Self {
            turns: 0,
            deepest: FIRST_DEPTH,
            kills: BTreeMap::new(),
            casts: CastCounts::default(),
            glyphs_learnt: 0,
            items_used: 0,
        }
    }
}

impl Stats {
    /// Returns the number of player actions that took time: 0 in a new
    /// game, one more per `Game::submit` that was not refused. The same
    /// count as `Game::player_turns`.
    pub const fn turns(&self) -> u64 {
        self.turns
    }

    /// Returns the number of the deepest floor the player reached:
    /// `generation::FIRST_DEPTH` in a new game, then the depth of the
    /// latest `Event::Descended` (the player never climbs back).
    pub const fn deepest(&self) -> u32 {
        self.deepest
    }

    /// Iterates over the monsters the player killed, as the identifier of
    /// their definition with how many died, sorted by identifier. Kinds
    /// never killed are left out, so every count is at least 1.
    pub fn kills(&self) -> impl Iterator<Item = (&str, u32)> {
        self.kills
            .iter()
            .map(|(definition, &count)| (definition.as_str(), count))
    }

    /// Returns the number of monsters killed, of every kind together.
    pub fn total_kills(&self) -> u32 {
        self.kills
            .values()
            .fold(0, |total, &count| total.saturating_add(count))
    }

    /// Returns the sigils cast, by quality.
    pub const fn casts(&self) -> CastCounts {
        self.casts
    }

    /// Returns the number of engraved glyphs whose reading taught a new
    /// sigil. Sigils learnt from a page (`Event::SigilLearned`) are not
    /// glyphs and count among [`Stats::items_used`] instead.
    pub const fn glyphs_learnt(&self) -> u32 {
        self.glyphs_learnt
    }

    /// Returns the number of carried items the player used up, whatever
    /// their effect.
    pub const fn items_used(&self) -> u32 {
        self.items_used
    }

    /// Counts one player action that took time.
    pub(crate) const fn count_turn(&mut self) {
        self.turns = self.turns.saturating_add(1);
    }

    /// Counts what `events`, the events of one `Game::submit`, report.
    ///
    /// - [`Event::Died`] of a monster: one more kill of its kind, whoever
    ///   dealt the blow (only the player fights monsters);
    /// - [`Event::SigilCast`]: one more cast of its quality;
    /// - [`Event::GlyphLearned`]: one more glyph learnt;
    /// - [`Event::Used`]: one more item used;
    /// - [`Event::Descended`]: the deepest floor becomes its depth if it is
    ///   deeper.
    ///
    /// Every other event, refusals included, changes nothing.
    pub(crate) fn record(&mut self, events: &[Event]) {
        for event in events {
            match event {
                Event::Died {
                    who: Combatant::Monster { definition },
                    ..
                } => {
                    let count = self.kills.entry(definition.clone()).or_default();
                    *count = count.saturating_add(1);
                }
                Event::SigilCast { quality, .. } => {
                    let count = match quality {
                        CastQuality::Pure => &mut self.casts.pure,
                        CastQuality::Unstable => &mut self.casts.unstable,
                        CastQuality::Fizzled => &mut self.casts.fizzled,
                    };
                    *count = count.saturating_add(1);
                }
                Event::GlyphLearned { .. } => {
                    self.glyphs_learnt = self.glyphs_learnt.saturating_add(1);
                }
                Event::Used { .. } => self.items_used = self.items_used.saturating_add(1),
                Event::Descended { depth } => self.deepest = self.deepest.max(*depth),
                _ => {}
            }
        }
    }

    /// Returns the identifiers of the monster kinds killed and their
    /// counts, as kept, for the checks of a save read back.
    pub(crate) const fn kill_entries(&self) -> &BTreeMap<String, u32> {
        &self.kills
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::ActorId;
    use crate::geometry::Position;
    use crate::item::ItemId;
    use crate::sigil::Backlash;

    /// A monster of kind `definition`, as events name it.
    fn monster(definition: &str) -> Combatant {
        Combatant::Monster {
            definition: definition.to_owned(),
        }
    }

    /// The death of a monster of kind `definition` at the player's hands.
    fn death_of(definition: &str) -> Event {
        Event::Died {
            actor: ActorId::default(),
            who: monster(definition),
            killed_by: Combatant::Player,
        }
    }

    /// A cast of `quality`.
    fn cast(quality: CastQuality) -> Event {
        Event::SigilCast {
            sigil: Some("ember".to_owned()),
            quality,
        }
    }

    #[test]
    fn a_new_game_has_done_nothing_on_the_first_floor() {
        let stats = Stats::default();

        assert_eq!(stats.turns(), 0);
        assert_eq!(stats.deepest(), FIRST_DEPTH);
        assert_eq!(stats.kills().count(), 0);
        assert_eq!(stats.total_kills(), 0);
        assert_eq!(stats.casts(), CastCounts::default());
        assert_eq!(stats.glyphs_learnt(), 0);
        assert_eq!(stats.items_used(), 0);
    }

    #[test]
    fn each_counted_event_adds_to_its_own_count() {
        let mut stats = Stats::default();
        stats.record(&[
            death_of("shade"),
            death_of("acolyte"),
            death_of("shade"),
            cast(CastQuality::Pure),
            cast(CastQuality::Pure),
            cast(CastQuality::Unstable),
            Event::Backlash {
                caster: ActorId::default(),
                backlash: Backlash::Daze,
            },
            cast(CastQuality::Fizzled),
            Event::GlyphLearned {
                sigil: "ember".to_owned(),
                position: Position::new(1, 1),
            },
            Event::Used {
                item: ItemId::default(),
                definition: "healing_draught".to_owned(),
            },
            Event::SigilLearned {
                sigil: "veil".to_owned(),
            },
            Event::Descended { depth: 3 },
        ]);

        assert_eq!(
            stats.kills().collect::<Vec<_>>(),
            [("acolyte", 1), ("shade", 2)]
        );
        assert_eq!(stats.total_kills(), 3);
        assert_eq!(
            stats.casts(),
            CastCounts {
                pure: 2,
                unstable: 1,
                fizzled: 1
            }
        );
        assert_eq!(stats.casts().total(), 4);
        assert_eq!(stats.glyphs_learnt(), 1, "a page is not a glyph");
        assert_eq!(stats.items_used(), 1);
        assert_eq!(stats.deepest(), 3);
        assert_eq!(stats.turns(), 0, "turns are counted apart");
    }

    #[test]
    fn the_player_death_and_refusals_count_nothing() {
        let mut stats = Stats::default();
        stats.record(&[
            Event::Died {
                actor: ActorId::default(),
                who: Combatant::Player,
                killed_by: monster("shade"),
            },
            Event::NotEnoughEssence {
                essence: 0,
                cost: 2,
            },
            Event::DirectionMissing {
                sigil: "lance".to_owned(),
            },
            Event::NoStairsHere {
                position: Position::new(1, 1),
            },
        ]);

        assert_eq!(stats, Stats::default());
    }

    #[test]
    fn the_deepest_floor_never_goes_back_up() {
        let mut stats = Stats::default();
        stats.record(&[Event::Descended { depth: 4 }]);
        stats.record(&[Event::Descended { depth: 2 }]);

        assert_eq!(stats.deepest(), 4);
    }

    #[test]
    fn counts_saturate_instead_of_overflowing() {
        let mut stats = Stats {
            turns: u64::MAX,
            glyphs_learnt: u32::MAX,
            ..Stats::default()
        };
        stats.kills.insert("shade".to_owned(), u32::MAX);
        stats.kills.insert("acolyte".to_owned(), 1);

        stats.count_turn();
        stats.record(&[
            death_of("shade"),
            Event::GlyphLearned {
                sigil: "ember".to_owned(),
                position: Position::new(1, 1),
            },
        ]);

        assert_eq!(stats.turns(), u64::MAX);
        assert_eq!(stats.glyphs_learnt(), u32::MAX);
        assert_eq!(stats.total_kills(), u32::MAX);
    }
}
