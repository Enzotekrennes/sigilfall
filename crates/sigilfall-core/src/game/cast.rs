//! Casting a sigil: what `Action::Cast` does to the game.
//!
//! Adds to [`Game`] the private steps of a cast: paying for it, finding the
//! target, applying the effect and, for an unstable cast, the backlash. The
//! rules themselves (costs, reach, backlash list) are constants of the
//! `schedule` and `sigil` modules; recognition is `sigil::recognize`.
//!
//! A cast goes in this order:
//!
//! 1. Without `sigil::ESSENCE_COST` essence, it is refused
//!    ([`Event::NotEnoughEssence`]); nothing is spent.
//! 2. The trace is recognised against the grimoire. If it casts a sigil
//!    aimed in a direction and no direction was given, it is refused
//!    ([`Event::DirectionMissing`]); nothing is spent.
//! 3. The essence and the energy (`schedule::action_cost`) are spent, and
//!    [`Event::SigilCast`] reports the sigil and the quality. An unstable
//!    cast while the player has steadied casts left (`Game::steady_casts`,
//!    given by items) uses one up and is cast pure instead: the event says
//!    pure and [`Event::CastSteadied`] follows. A fizzled cast stops here:
//!    the essence is lost; when its trace is no near miss of a known sigil,
//!    [`Event::UnknownPattern`] follows (see `sigil::is_unknown_pattern`).
//! 4. The effect applies to its target (see [`Game::apply_effect`]).
//! 5. An unstable cast then suffers one backlash drawn from the gameplay
//!    stream (see [`Game::suffer_backlash`]).

use super::Game;
use crate::action::Action;
use crate::actor::ActorId;
use crate::content::{Effect, SigilDef, Targeting};
use crate::event::Event;
use crate::geometry::{Direction, Position};
use crate::schedule::{action_cost, spend_energy};
use crate::sigil::{
    Backlash, CastQuality, DAZE_TURNS, ESSENCE_COST, NEAREST_FOE_REACH, Recognition, SCORCH_DAMAGE,
    SPELL_RANGE, Trace, draw_backlash, is_unknown_pattern, recognize,
};

impl Game {
    /// Makes `caster` cast `trace`, aimed in `direction` if given, appends
    /// the events to `events` and returns whether the cast took time (see
    /// the steps in the module documentation).
    pub(super) fn cast(
        &mut self,
        caster: ActorId,
        trace: Trace,
        direction: Option<Direction>,
        events: &mut Vec<Event>,
    ) -> bool {
        let essence = self.world.actors[caster].essence();
        if essence < ESSENCE_COST {
            events.push(Event::NotEnoughEssence {
                essence,
                cost: ESSENCE_COST,
            });
            return false;
        }
        let recognition = recognize(&trace, self.grimoire.sigils());
        let mut quality = CastQuality::of(&recognition);
        let sigil = match recognition {
            Recognition::Success(sigil) | Recognition::Unstable(sigil) => Some(sigil.clone()),
            Recognition::Fizzle => None,
        };
        if let Some(sigil) = &sigil
            && sigil.targeting() == Targeting::Direction
            && direction.is_none()
        {
            events.push(Event::DirectionMissing {
                sigil: sigil.id().to_owned(),
            });
            return false;
        }

        let steadied = quality == CastQuality::Unstable && self.steady_casts > 0;
        if steadied {
            self.steady_casts -= 1;
            quality = CastQuality::Pure;
        }
        let actor = &mut self.world.actors[caster];
        actor.set_essence(essence - ESSENCE_COST);
        spend_energy(actor, action_cost(Action::Cast { trace, direction }));
        events.push(Event::SigilCast {
            sigil: sigil.as_ref().map(|sigil| sigil.id().to_owned()),
            quality,
        });
        let Some(sigil) = sigil else {
            if is_unknown_pattern(&trace, self.grimoire.sigils()) {
                events.push(Event::UnknownPattern);
            }
            return true;
        };
        if steadied {
            events.push(Event::CastSteadied {
                sigil: sigil.id().to_owned(),
                casts_left: self.steady_casts,
            });
        }
        self.apply_effect(caster, &sigil, direction, events);
        if quality == CastQuality::Unstable {
            self.suffer_backlash(caster, events);
        }
        true
    }

    /// Applies the effect of `sigil`, cast by `caster` and aimed in
    /// `direction` if given, and appends its events to `events`.
    ///
    /// - Damage takes the sigil's power in hit points off the target
    ///   ([`Event::Wounded`]), which may kill it.
    /// - Heal restores up to the power in hit points, never above the
    ///   maximum ([`Event::Healed`]).
    /// - Hold makes the target lose its next turns, as many as the power
    ///   ([`Event::Held`], see [`Game::hold`]).
    /// - Blink carries the caster along `direction` (see [`Game::blink`]).
    ///
    /// When no target is found, [`Event::SpellMissed`] is appended instead.
    fn apply_effect(
        &mut self,
        caster: ActorId,
        sigil: &SigilDef,
        direction: Option<Direction>,
        events: &mut Vec<Event>,
    ) {
        if sigil.effect() == Effect::Blink {
            if let Some(direction) = direction {
                self.blink(caster, direction, sigil.power(), events);
            }
            return;
        }
        let Some(target) = self.spell_target(caster, sigil.targeting(), direction) else {
            events.push(Event::SpellMissed {
                sigil: sigil.id().to_owned(),
            });
            return;
        };
        let power = sigil.power();
        match sigil.effect() {
            Effect::Damage => self.wound(target, power, caster, events),
            Effect::Heal => {
                let healed = &mut self.world.actors[target];
                let amount = healed.heal(power);
                events.push(Event::Healed {
                    actor: target,
                    amount,
                    hp: healed.hp(),
                });
            }
            Effect::Hold => self.hold(target, power.unsigned_abs(), events),
            Effect::Blink => {}
        }
    }

    /// Returns whom a spell of `caster` with `targeting` reaches, aimed in
    /// `direction` if given, or `None` if nobody.
    ///
    /// - [`Targeting::Caster`]: the caster itself.
    /// - [`Targeting::NearestEnemy`]: among the foes on a cell the player
    ///   sees and within [`NEAREST_FOE_REACH`] of the caster (the disc of
    ///   radius [`SPELL_RANGE`]), the one the fewest steps away (Chebyshev
    ///   distance), ties broken in reading order (upper row first, then left
    ///   column first), so the choice never depends on anything but the
    ///   positions. A foe seen farther along a row is out of reach.
    /// - [`Targeting::Direction`]: the first actor on the straight line from
    ///   the caster in `direction`, within [`SPELL_RANGE`] cells; the line
    ///   stops at the first step [`Map::can_step`](crate::map::Map::can_step)
    ///   refuses, as a walk would.
    fn spell_target(
        &self,
        caster: ActorId,
        targeting: Targeting,
        direction: Option<Direction>,
    ) -> Option<ActorId> {
        match targeting {
            Targeting::Caster => Some(caster),
            Targeting::NearestEnemy => self.nearest_visible_foe(caster),
            Targeting::Direction => direction.and_then(|direction| {
                self.line(caster, direction, SPELL_RANGE)
                    .into_iter()
                    .find_map(|cell| self.world.actor_at(cell))
            }),
        }
    }

    /// Returns the foe of `caster` standing on a visible cell within
    /// [`NEAREST_FOE_REACH`] of it that is the fewest steps away, ties broken
    /// in reading order, if any.
    fn nearest_visible_foe(&self, caster: ActorId) -> Option<ActorId> {
        let from = self.world.actors[caster].position;
        self.world
            .actors
            .iter()
            .filter(|&(id, actor)| {
                actor.is_player() != self.world.actors[caster].is_player()
                    && self.world.map.is_visible(actor.position)
                    && NEAREST_FOE_REACH
                        .reaches(actor.position.x - from.x, actor.position.y - from.y)
                    && id != caster
            })
            .min_by_key(|(_, actor)| {
                let cell = actor.position;
                (steps_between(from, cell), cell.y, cell.x)
            })
            .map(|(id, _)| id)
    }

    /// Returns the cells of the straight line from the actor `id` in
    /// `direction`, nearest first, at most `range` of them: it stops before
    /// the first step that `Map::can_step` refuses (a wall or the edge of the
    /// map). Actors on the way do not stop it.
    fn line(&self, id: ActorId, direction: Direction, range: u32) -> Vec<Position> {
        let mut cells = Vec::new();
        let mut cell = self.world.actors[id].position;
        for _ in 0..range {
            if !self.world.map.can_step(cell, direction) {
                break;
            }
            cell = cell.step(direction);
            cells.push(cell);
        }
        cells
    }

    /// Carries `caster` up to `power` cells in `direction`, onto the last
    /// cell of the line (see [`Game::line`]) that no other actor stands on,
    /// and appends [`Event::Blinked`]; the caster passes over actors in the
    /// way. If even the first cell is out of reach, the caster stays in
    /// place (the event then has `to` equal to `from`). The player reads the
    /// glyph it lands on, as if it had stepped there.
    fn blink(
        &mut self,
        caster: ActorId,
        direction: Direction,
        power: i32,
        events: &mut Vec<Event>,
    ) {
        let from = self.world.actors[caster].position;
        let to = self
            .line(caster, direction, power.unsigned_abs())
            .into_iter()
            .rfind(|&cell| self.world.actor_at(cell).is_none())
            .unwrap_or(from);
        self.world.actors[caster].position = to;
        events.push(Event::Blinked {
            actor: caster,
            from,
            to,
        });
        if caster == self.world.player {
            self.read_glyph(events);
        }
    }

    /// Takes `damage` hit points off `target` by magic and appends
    /// [`Event::Wounded`]; if none are left, `target` dies at the hands of
    /// `source` (see [`Game::kill`]).
    fn wound(&mut self, target: ActorId, damage: i32, source: ActorId, events: &mut Vec<Event>) {
        let wounded = &mut self.world.actors[target];
        wounded.take_damage(damage);
        let hp_left = wounded.hp();
        events.push(Event::Wounded {
            actor: target,
            damage,
            hp_left,
        });
        if hp_left == 0 {
            self.kill(target, source, events);
        }
    }

    /// Holds `target` for its next `turns` turns and appends [`Event::Held`]
    /// with the turns it now loses.
    ///
    /// Holds do not add up: a target already held keeps the longer of its
    /// current hold and the new one, so stacking short holds cannot freeze
    /// a creature for good.
    fn hold(&mut self, target: ActorId, turns: u32, events: &mut Vec<Event>) {
        let held = &mut self.world.actors[target];
        held.held_turns = held.held_turns.max(turns);
        events.push(Event::Held {
            actor: target,
            turns: held.held_turns,
        });
    }

    /// Draws the backlash of an unstable cast by `caster` from the gameplay
    /// stream, appends [`Event::Backlash`], then applies it with the event of
    /// its consequence:
    ///
    /// - [`Backlash::Scorch`]: [`SCORCH_DAMAGE`] hit points off the caster
    ///   ([`Event::Wounded`]), but never the last one;
    /// - [`Backlash::Drain`]: the caster's essence falls to 0
    ///   ([`Event::EssenceDrained`]);
    /// - [`Backlash::Daze`]: the caster is held for [`DAZE_TURNS`] turns
    ///   ([`Event::Held`]); for the player, the first lost turn usually
    ///   comes before the end of the same [`Game::submit`].
    fn suffer_backlash(&mut self, caster: ActorId, events: &mut Vec<Event>) {
        let backlash = draw_backlash(&mut self.gameplay_rng);
        events.push(Event::Backlash { caster, backlash });
        match backlash {
            Backlash::Scorch => {
                let spared = (self.world.actors[caster].hp() - 1).max(0);
                self.wound(caster, SCORCH_DAMAGE.min(spared), caster, events);
            }
            Backlash::Drain => {
                let drained = &mut self.world.actors[caster];
                let lost = drained.essence();
                drained.set_essence(0);
                events.push(Event::EssenceDrained {
                    actor: caster,
                    lost,
                });
            }
            Backlash::Daze => self.hold(caster, DAZE_TURNS, events),
        }
    }
}

/// Returns the number of steps between `a` and `b` when diagonal steps are
/// allowed (Chebyshev distance), the measure of "nearest" for spells.
fn steps_between(a: Position, b: Position) -> u32 {
    a.x.abs_diff(b.x).max(a.y.abs_diff(b.y))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::{Combatant, PLAYER_MAX_HP};
    use crate::content::{Content, MonsterDef};
    use crate::game::PLAYER_START as START;
    use crate::generation::{self, FIRST_DEPTH};
    use crate::map::FIXED_ROOM_PILLAR;
    use crate::rng::{Purpose, Seed};
    use crate::schedule::{CAST_COST, NORMAL_SPEED, STROKE_COST, cast_cost};
    use crate::sigil::{ESSENCE_REGEN_TICKS, MAX_ESSENCE, Node};

    /// Seed of the test games; the fixed room does not depend on it.
    const SEED: Seed = Seed::new(42);

    /// The monster file shipped with the game, for content with test sigils.
    const MONSTERS: &str = include_str!("../../content/monsters.ron");

    /// The content shipped with the game.
    fn content() -> Content {
        Content::load().expect("the shipped content is valid")
    }

    /// The shipped sigil identified by `id`.
    fn shipped(id: &str) -> SigilDef {
        content().sigil(id).expect("a shipped sigil").clone()
    }

    /// A sigil list made of one sigil with the given values and the pattern
    /// 7-5-3, for effects and targetings the shipped file does not pair.
    fn single_sigil(effect: &str, targeting: &str, power: i32) -> SigilDef {
        let source = format!(
            "[Sigil(id: \"test_sigil\", name: \"test sigil\", name_fr: \"sigille\", pattern: [7, 5, 3], \
             effect: {effect}, power: {power}, targeting: {targeting}, min_depth: 1, \
             known_at_start: false)]"
        );
        let content = Content::parse_files(MONSTERS, &source).expect("valid test sigil");
        content.sigils()[0].clone()
    }

    /// A monster definition with `speed` and `hp`, harmless enough not to
    /// kill the player in a test: attack 1, no defense.
    fn creature(speed: i32, hp: i32) -> MonsterDef {
        let source = format!(
            "[Monster(id: \"test_creature\", name: \"test creature\", name_fr: Masculine(\"le monstre\"), plural: \"test creatures\", plural_fr: \"monstres\", family: Vermin, \
             elite: false, speed: {speed}, hp: {hp}, attack: 1, defense: 0, min_depth: 1)]"
        );
        let content = Content::parse(&source).expect("valid test monster");
        content.monsters()[0].clone()
    }

    /// A new game in the fixed room whose grimoire holds `sigils`.
    fn game_knowing(sigils: &[&SigilDef]) -> Game {
        let mut game = Game::in_fixed_room(SEED);
        for sigil in sigils {
            game.grimoire.learn(sigil);
        }
        game
    }

    /// The trace obtained by pressing `digits` in order, each accepted.
    fn pressed(digits: &[u8]) -> Trace {
        let mut trace = Trace::new();
        for &digit in digits {
            let node = Node::new(digit).expect("a digit from 1 to 9");
            trace.push(node).expect("a node not linked yet");
        }
        trace
    }

    /// Casts the trace pressed as `digits`, aimed in `direction`.
    fn cast(game: &mut Game, digits: &[u8], direction: Option<Direction>) -> Vec<Event> {
        game.submit(Action::Cast {
            trace: pressed(digits),
            direction,
        })
    }

    /// The `SigilCast` event of a cast of `sigil` with `quality`.
    fn cast_event(sigil: Option<&str>, quality: CastQuality) -> Event {
        Event::SigilCast {
            sigil: sigil.map(str::to_owned),
            quality,
        }
    }

    /// Energy the player spent during the `submit` that went from `before`
    /// (its energy and the ticks then) to now: what it had, plus what it
    /// gained since, minus what it has left.
    fn energy_spent(game: &Game, before: (i32, u64)) -> i32 {
        let (energy, ticks) = before;
        let gained = i32::try_from(game.ticks() - ticks).unwrap() * NORMAL_SPEED;
        energy + gained - game.player_actor().energy()
    }

    /// The player's energy and the elapsed ticks, for [`energy_spent`].
    fn clock(game: &Game) -> (i32, u64) {
        (game.player_actor().energy(), game.ticks())
    }

    /// The number of turns `id` took (waits) among `events`.
    fn waits_of(events: &[Event], id: ActorId) -> usize {
        events
            .iter()
            .filter(|event| **event == Event::Waited { actor: id })
            .count()
    }

    /// The gameplay backlash a fresh game of `seed` draws first.
    fn first_backlash(seed: Seed) -> Backlash {
        draw_backlash(&mut seed.stream(FIRST_DEPTH, Purpose::Gameplay))
    }

    // --- Cost ----------------------------------------------------------------

    #[test]
    fn a_cast_costs_each_stroke_middle_nodes_included_plus_the_validation() {
        // Unknown traces fizzle, which costs the same time as a success;
        // with nothing known, every pattern is unknown.
        let cases: [(&[u8], usize); 4] = [
            (&[7, 5, 3], 3),
            // 7 to 3 links 5 on the way: three strokes.
            (&[7, 3], 3),
            // 5 is linked first, so 4 to 6 crosses it for free.
            (&[5, 4, 6], 3),
            // 1 to 3 links 2 and 3 to 9 links 6: five strokes.
            (&[1, 3, 9], 5),
        ];
        for (digits, strokes) in cases {
            let mut game = game_knowing(&[]);
            let before = clock(&game);

            let events = cast(&mut game, digits, None);

            assert_eq!(
                events,
                [
                    cast_event(None, CastQuality::Fizzled),
                    Event::UnknownPattern
                ]
            );
            assert_eq!(cast_cost(strokes), strokes as i32 * STROKE_COST + CAST_COST);
            assert_eq!(
                energy_spent(&game, before),
                cast_cost(strokes),
                "{digits:?}"
            );
            assert_eq!(game.player_turns(), 1);
        }
    }

    #[test]
    fn a_long_cast_lets_a_fast_monster_act_more_often() {
        // Energy left over from one action carries to the next, so the turns
        // are counted over a run of identical actions.
        let fast_turns = |digits: &[u8]| {
            let mut game = game_knowing(&[]);
            // Far out of sight and unaware of the player, it waits each turn.
            let fast = game.spawn_monster(Position::new(27, 8), &creature(2 * NORMAL_SPEED, 5));
            game.submit(Action::Wait);
            let mut turns = 0;
            for _ in 0..8 {
                game.world.actors[game.world.player].set_essence(MAX_ESSENCE);
                let events = if digits.is_empty() {
                    game.submit(Action::Wait)
                } else {
                    cast(&mut game, digits, None)
                };
                turns += waits_of(&events, fast);
            }
            turns
        };

        let wait = fast_turns(&[]);
        let short = fast_turns(&[7, 5, 3]);
        let long = fast_turns(&[7, 8, 9, 6, 5, 4, 1, 2, 3]);

        assert_eq!(wait, 16, "speed 200 acts twice per normal action");
        assert!(short > wait, "3 strokes ({short}) against waits ({wait})");
        assert!(long > 2 * short, "9 strokes ({long}) against 3 ({short})");
    }

    // --- Pure casts: the four effects ------------------------------------------

    #[test]
    fn a_pure_damage_sigil_wounds_the_nearest_visible_enemy() {
        let ember = shipped("ember");
        let mut game = game_knowing(&[&ember]);
        let near = game.spawn_monster(Position::new(8, 5), &creature(NORMAL_SPEED, 20));
        let far = game.spawn_monster(Position::new(11, 5), &creature(NORMAL_SPEED, 20));

        let events = cast(&mut game, &[7, 5, 3], None);

        let hp_left = 20 - ember.power();
        assert_eq!(
            events[..2],
            [
                cast_event(Some("ember"), CastQuality::Pure),
                Event::Wounded {
                    actor: near,
                    damage: ember.power(),
                    hp_left,
                },
            ]
        );
        assert_eq!(game.actor(near).map(|a| a.hp()), Some(hp_left));
        assert_eq!(game.actor(far).map(|a| a.hp()), Some(20));
        assert_eq!(game.player_actor().essence(), MAX_ESSENCE - ESSENCE_COST);
        assert!(!events.iter().any(|e| matches!(e, Event::Backlash { .. })));
    }

    #[test]
    fn the_nearest_enemy_ties_go_to_the_upper_then_left_one() {
        let ember = shipped("ember");
        let mut game = game_knowing(&[&ember]);
        // All three are three steps away; (3, 2) is on the upper row.
        let right = game.spawn_monster(Position::new(8, 2), &creature(NORMAL_SPEED, 20));
        let left = game.spawn_monster(Position::new(3, 2), &creature(NORMAL_SPEED, 20));
        let below = game.spawn_monster(Position::new(2, 8), &creature(NORMAL_SPEED, 20));

        let events = cast(&mut game, &[7, 5, 3], None);

        assert!(
            matches!(events[1], Event::Wounded { actor, .. } if actor == left),
            "{events:?}"
        );
        for id in [right, below] {
            assert_eq!(game.actor(id).map(|a| a.hp()), Some(20));
        }
    }

    #[test]
    fn a_damage_sigil_that_kills_reports_the_death_by_the_player() {
        let ember = shipped("ember");
        let mut game = game_knowing(&[&ember]);
        let frail = game.spawn_monster(Position::new(8, 5), &creature(NORMAL_SPEED, 3));

        let events = cast(&mut game, &[7, 5, 3], None);

        assert_eq!(
            events[1..3],
            [
                Event::Wounded {
                    actor: frail,
                    damage: ember.power(),
                    hp_left: 0,
                },
                Event::Died {
                    actor: frail,
                    who: Combatant::Monster {
                        definition: "test_creature".to_owned()
                    },
                    killed_by: Combatant::Player,
                },
            ]
        );
        assert!(game.actor(frail).is_none());
    }

    #[test]
    fn a_spell_with_no_enemy_in_sight_is_lost_but_paid() {
        let ember = shipped("ember");
        let mut game = game_knowing(&[&ember]);
        // Out of sight: it does not count as a target.
        let hidden = game.spawn_monster(Position::new(27, 8), &creature(NORMAL_SPEED, 20));
        let before = clock(&game);

        let events = cast(&mut game, &[7, 5, 3], None);

        assert_eq!(
            events[..2],
            [
                cast_event(Some("ember"), CastQuality::Pure),
                Event::SpellMissed {
                    sigil: "ember".to_owned()
                },
            ]
        );
        assert_eq!(game.actor(hidden).map(|a| a.hp()), Some(20));
        assert_eq!(game.player_actor().essence(), MAX_ESSENCE - ESSENCE_COST);
        assert_eq!(energy_spent(&game, before), cast_cost(3));
    }

    #[test]
    fn a_spell_on_the_nearest_foe_misses_a_foe_seen_beyond_eight_cells() {
        for id in ["ember", "veil"] {
            let sigil = shipped(id);
            let mut game = game_knowing(&[&sigil]);
            // Nine cells east: in sight (16 along a row), out of the disc.
            let seen = Position::new(START.x + 9, START.y);
            let foe = game.spawn_monster(seen, &creature(NORMAL_SPEED, 20));
            assert!(game.is_visible(seen));

            let events = game.submit(Action::Cast {
                trace: *sigil.pattern(),
                direction: None,
            });

            assert_eq!(
                events[1],
                Event::SpellMissed {
                    sigil: id.to_owned()
                },
                "{id}"
            );
            assert_eq!(
                game.actor(foe).map(|a| (a.hp(), a.held_turns())),
                Some((20, 0))
            );
        }
    }

    #[test]
    fn a_spell_on_the_nearest_foe_reaches_eight_cells_along_a_row() {
        for id in ["ember", "veil"] {
            let sigil = shipped(id);
            let mut game = game_knowing(&[&sigil]);
            let foe = game.spawn_monster(
                Position::new(START.x + 8, START.y),
                &creature(NORMAL_SPEED, 20),
            );

            let events = game.submit(Action::Cast {
                trace: *sigil.pattern(),
                direction: None,
            });

            assert!(
                matches!(
                    events[1],
                    Event::Wounded { actor, .. } | Event::Held { actor, .. } if actor == foe
                ),
                "{id}: {events:?}"
            );
        }
    }

    #[test]
    fn a_foe_fewer_steps_away_but_outside_the_disc_is_passed_over() {
        let ember = shipped("ember");
        let mut game = game_knowing(&[&ember]);
        // 7 steps away (7 across, 5 down) but 7² + 5² = 74 > 8² + 8.
        let outside = Position::new(START.x + 7, START.y + 5);
        let skipped = game.spawn_monster(outside, &creature(NORMAL_SPEED, 20));
        // 8 steps away, straight along the row: inside the disc.
        let target = game.spawn_monster(
            Position::new(START.x + 8, START.y),
            &creature(NORMAL_SPEED, 20),
        );
        assert!(game.is_visible(outside));
        assert!(!NEAREST_FOE_REACH.reaches(7, 5));

        let events = cast(&mut game, &[7, 5, 3], None);

        assert!(
            matches!(events[1], Event::Wounded { actor, .. } if actor == target),
            "{events:?}"
        );
        assert_eq!(game.actor(skipped).map(|a| a.hp()), Some(20));
    }

    #[test]
    fn a_damage_sigil_aimed_in_a_direction_hits_the_first_creature_on_the_line() {
        let lance = shipped("lance");
        let mut game = game_knowing(&[&lance]);
        let first = game.spawn_monster(Position::new(8, 5), &creature(NORMAL_SPEED, 20));
        let behind = game.spawn_monster(Position::new(10, 5), &creature(NORMAL_SPEED, 20));
        let aside = game.spawn_monster(Position::new(6, 6), &creature(NORMAL_SPEED, 20));

        let events = cast(&mut game, &[1, 5, 9, 6], Some(Direction::East));

        assert_eq!(
            events[1],
            Event::Wounded {
                actor: first,
                damage: lance.power(),
                hp_left: 20 - lance.power(),
            }
        );
        for id in [behind, aside] {
            assert_eq!(game.actor(id).map(|a| a.hp()), Some(20));
        }

        // Westwards, the wall comes first: nothing is hit.
        game.world.actors[game.world.player].set_essence(MAX_ESSENCE);
        let events = cast(&mut game, &[1, 5, 9, 6], Some(Direction::West));
        assert_eq!(
            events[1],
            Event::SpellMissed {
                sigil: "lance".to_owned()
            }
        );
    }

    #[test]
    fn a_pure_heal_restores_hit_points_up_to_the_maximum() {
        let recall = shipped("recall");
        let mut game = game_knowing(&[&recall]);
        let player = game.player();
        game.world.actors[player].take_damage(10);

        let events = cast(&mut game, &[8, 5, 2, 1], None);

        let hp = PLAYER_MAX_HP - 10 + recall.power();
        assert_eq!(
            events,
            [
                cast_event(Some("recall"), CastQuality::Pure),
                Event::Healed {
                    actor: player,
                    amount: recall.power(),
                    hp,
                },
            ]
        );

        let events = cast(&mut game, &[8, 5, 2, 1], None);
        assert_eq!(
            events[1],
            Event::Healed {
                actor: player,
                amount: PLAYER_MAX_HP - hp,
                hp: PLAYER_MAX_HP,
            },
            "capped at the maximum"
        );
    }

    #[test]
    fn a_pure_hold_makes_the_target_lose_its_next_turns() {
        let veil = shipped("veil");
        let mut game = game_knowing(&[&veil]);
        let player = game.player();
        // Next to the player: free, it would strike during the cast.
        let held = game.spawn_monster(START.step(Direction::East), &creature(NORMAL_SPEED, 20));
        let turns = veil.power().unsigned_abs();

        let events = cast(&mut game, &[4, 7, 8, 9, 6], None);

        assert_eq!(
            events[..2],
            [
                cast_event(Some("veil"), CastQuality::Pure),
                Event::Held { actor: held, turns },
            ]
        );
        // Its next two turns come during the cast and the following wait:
        // both are lost, and it never strikes.
        let mut events = events;
        events.extend(game.submit(Action::Wait));
        let lost = events
            .iter()
            .filter(|e| **e == Event::TurnLost { actor: held })
            .count();
        assert_eq!(lost, usize::try_from(turns).unwrap(), "{events:?}");
        assert!(!events.iter().any(|e| matches!(e, Event::Attacked { .. })));
        assert_eq!(game.actor(held).map(|a| a.held_turns()), Some(0));

        // Free again, it strikes on its next turn.
        let events = game.submit(Action::Wait);
        let struck = |e: &Event| matches!(e, Event::Attacked { attacker, target, .. } if *attacker == held && *target == player);
        assert!(events.iter().any(struck), "{events:?}");
    }

    #[test]
    fn holds_do_not_add_up() {
        let hold = single_sigil("Hold", "Direction", 3);
        let mut game = game_knowing(&[]);
        let target = game.spawn_monster(Position::new(8, 5), &creature(NORMAL_SPEED, 20));
        let mut events = Vec::new();

        game.hold(target, 2, &mut events);
        game.hold(target, 1, &mut events);
        game.apply_effect(game.world.player, &hold, Some(Direction::East), &mut events);

        let turns: Vec<u32> = events
            .iter()
            .filter_map(|e| match e {
                Event::Held { turns, .. } => Some(*turns),
                _ => None,
            })
            .collect();
        assert_eq!(turns, [2, 2, 3]);
    }

    #[test]
    fn a_pure_blink_lands_on_the_last_free_cell_of_the_line() {
        let passage = shipped("passage");
        let power = passage.power();
        assert_eq!(power, 4, "the cells below assume a blink of 4");
        let cases = [
            // Open floor: the full distance.
            (START, Direction::East, None, Position::new(9, 5)),
            // A creature on the landing cell: the cell before it.
            (
                START,
                Direction::East,
                Some(Position::new(9, 5)),
                Position::new(8, 5),
            ),
            // A creature on the way: passed over.
            (
                START,
                Direction::East,
                Some(Position::new(7, 5)),
                Position::new(9, 5),
            ),
            // The right wall is three cells away.
            (
                Position::new(25, 5),
                Direction::East,
                None,
                Position::new(28, 5),
            ),
            // The pillar stops the line.
            (
                Position::new(13, 6),
                Direction::East,
                None,
                Position::new(14, 6),
            ),
            // Against the wall: no move at all.
            (
                Position::new(1, 5),
                Direction::West,
                None,
                Position::new(1, 5),
            ),
        ];
        for (from, direction, blocker, landing) in cases {
            let mut game = game_knowing(&[&passage]);
            let player = game.player();
            game.world.actors[player].position = from;
            if let Some(cell) = blocker {
                game.spawn_monster(cell, &creature(NORMAL_SPEED, 20));
            }

            let events = cast(&mut game, &[7, 4, 1, 2, 3], Some(direction));

            assert_eq!(
                events[1],
                Event::Blinked {
                    actor: player,
                    from,
                    to: landing
                },
                "from {from:?} {direction:?}"
            );
            assert_eq!(game.player_actor().position(), landing);
            assert_ne!(landing, FIXED_ROOM_PILLAR);
        }
    }

    #[test]
    fn a_blink_onto_a_glyph_reads_it() {
        let passage = shipped("passage");
        let ember = shipped("ember");
        let mut game = game_knowing(&[&passage]);
        let landing = Position::new(9, 5);
        game.engrave(landing, &ember);

        let events = cast(&mut game, &[7, 4, 1, 2, 3], Some(Direction::East));

        assert_eq!(
            events[2],
            Event::GlyphLearned {
                sigil: "ember".to_owned(),
                position: landing,
            }
        );
        assert!(game.grimoire().knows("ember"));
    }

    #[test]
    fn a_direction_sigil_without_a_direction_is_refused_without_cost() {
        let passage = shipped("passage");
        let mut game = game_knowing(&[&passage]);
        let before = clock(&game);

        let events = cast(&mut game, &[7, 4, 1, 2, 3], None);

        assert_eq!(
            events,
            [Event::DirectionMissing {
                sigil: "passage".to_owned()
            }]
        );
        assert_eq!(clock(&game), before);
        assert_eq!(game.player_actor().essence(), MAX_ESSENCE);
        assert_eq!(game.player_turns(), 0);
    }

    // --- Unstable casts ----------------------------------------------------------

    #[test]
    fn an_unstable_cast_applies_its_effect_then_the_backlash_of_the_seed() {
        let ember = shipped("ember");
        let mut seen = Vec::new();
        for value in 0..40 {
            let seed = Seed::new(value);
            let mut game = Game::in_fixed_room(seed);
            game.grimoire.learn(&ember);
            let player = game.player();
            let target = game.spawn_monster(Position::new(8, 5), &creature(NORMAL_SPEED, 20));
            let expected = first_backlash(seed);

            // 7-5-2 is one edit from Ember's 7-5-3.
            let events = cast(&mut game, &[7, 5, 2], None);

            let context = format!("seed {value}: {events:?}");
            assert_eq!(
                events[..3],
                [
                    cast_event(Some("ember"), CastQuality::Unstable),
                    Event::Wounded {
                        actor: target,
                        damage: ember.power(),
                        hp_left: 20 - ember.power(),
                    },
                    Event::Backlash {
                        caster: player,
                        backlash: expected,
                    },
                ],
                "{context}"
            );
            let essence_left = MAX_ESSENCE - ESSENCE_COST;
            let consequence = match expected {
                Backlash::Scorch => Event::Wounded {
                    actor: player,
                    damage: SCORCH_DAMAGE,
                    hp_left: PLAYER_MAX_HP - SCORCH_DAMAGE,
                },
                Backlash::Drain => Event::EssenceDrained {
                    actor: player,
                    lost: essence_left,
                },
                Backlash::Daze => Event::Held {
                    actor: player,
                    turns: DAZE_TURNS,
                },
            };
            assert_eq!(events[3], consequence, "{context}");
            match expected {
                Backlash::Scorch => {
                    assert_eq!(game.player_actor().hp(), PLAYER_MAX_HP - SCORCH_DAMAGE);
                }
                Backlash::Drain => assert_eq!(game.player_actor().essence(), 0),
                Backlash::Daze => {
                    assert!(
                        events.contains(&Event::TurnLost { actor: player }),
                        "{context}"
                    );
                    assert_eq!(game.player_actor().held_turns(), 0);
                }
            }
            if expected != Backlash::Drain {
                assert_eq!(game.player_actor().essence(), essence_left, "{context}");
            }
            if !seen.contains(&expected) {
                seen.push(expected);
            }

            // The same seed and actions give the same events.
            let mut twin = Game::in_fixed_room(seed);
            twin.grimoire.learn(&ember);
            twin.spawn_monster(Position::new(8, 5), &creature(NORMAL_SPEED, 20));
            assert_eq!(cast(&mut twin, &[7, 5, 2], None), events, "{context}");
        }
        assert_eq!(
            seen.len(),
            Backlash::ALL.len(),
            "every backlash met: {seen:?}"
        );
    }

    #[test]
    fn a_scorching_backlash_never_kills() {
        let seed = (0..)
            .map(Seed::new)
            .find(|&seed| first_backlash(seed) == Backlash::Scorch)
            .expect("some seed scorches first");
        let recall = shipped("recall");
        let mut game = Game::in_fixed_room(seed);
        game.grimoire.learn(&recall);
        let player = game.player();
        game.world.actors[player].take_damage(PLAYER_MAX_HP - 1);

        // 8-5-2 is one edit from Recall's 8-5-2-1: heal, then scorch.
        let events = cast(&mut game, &[8, 5, 2], None);

        let healed = 1 + recall.power();
        assert_eq!(
            events[3],
            Event::Wounded {
                actor: player,
                damage: SCORCH_DAMAGE,
                hp_left: healed - SCORCH_DAMAGE,
            }
        );

        // At 1 hit point, a scorch takes nothing.
        game.world.actors[player].take_damage(PLAYER_MAX_HP);
        game.world.actors[player].heal(1);
        let mut events = Vec::new();
        for _ in 0..30 {
            game.suffer_backlash(player, &mut events);
        }
        assert!(!game.is_over());
        assert_eq!(game.player_actor().hp(), 1);
        let scorch = Event::Wounded {
            actor: player,
            damage: 0,
            hp_left: 1,
        };
        assert!(events.contains(&scorch), "{events:?}");
        assert!(!events.iter().any(|e| matches!(e, Event::Died { .. })));
    }

    // --- Fizzles, essence and the grimoire ---------------------------------------

    #[test]
    fn a_fizzled_cast_loses_the_essence_and_the_time() {
        let ember = shipped("ember");
        let mut game = game_knowing(&[&ember]);
        let monster = game.spawn_monster(Position::new(8, 5), &creature(NORMAL_SPEED, 20));
        let before = clock(&game);

        // 4-8-6 is far from every pattern.
        let events = cast(&mut game, &[4, 8, 6], None);

        assert_eq!(events[0], cast_event(None, CastQuality::Fizzled));
        assert!(
            !events.iter().any(|e| matches!(
                e,
                Event::Wounded { .. } | Event::Backlash { .. } | Event::SpellMissed { .. }
            )),
            "{events:?}"
        );
        assert_eq!(game.player_actor().essence(), MAX_ESSENCE - ESSENCE_COST);
        assert_eq!(energy_spent(&game, before), cast_cost(3));
        assert_eq!(game.actor(monster).map(|a| a.hp()), Some(20));
    }

    #[test]
    fn a_perfect_trace_of_a_sigil_not_learnt_fizzles() {
        let ember = shipped("ember");
        let mut game = game_knowing(&[&ember]);
        let monster = game.spawn_monster(Position::new(8, 5), &creature(NORMAL_SPEED, 20));

        // Lance's exact pattern, but only Ember is known.
        let events = cast(&mut game, &[1, 5, 9, 6], Some(Direction::East));

        assert_eq!(events[0], cast_event(None, CastQuality::Fizzled));
        assert_eq!(game.player_actor().essence(), MAX_ESSENCE - ESSENCE_COST);
        assert_eq!(game.actor(monster).map(|a| a.hp()), Some(20));
    }

    #[test]
    fn without_enough_essence_a_cast_is_refused_and_no_time_passes() {
        let ember = shipped("ember");
        let mut game = game_knowing(&[&ember]);
        let monster = game.spawn_monster(Position::new(27, 8), &creature(2 * NORMAL_SPEED, 20));
        game.submit(Action::Wait);
        let player = game.player();
        game.world.actors[player].set_essence(ESSENCE_COST - 1);
        let before = clock(&game);
        let monster_energy = game.actor(monster).map(|a| a.energy());
        let turns = game.player_turns();

        let events = cast(&mut game, &[7, 5, 3], None);

        assert_eq!(
            events,
            [Event::NotEnoughEssence {
                essence: ESSENCE_COST - 1,
                cost: ESSENCE_COST,
            }]
        );
        assert_eq!(clock(&game), before);
        assert_eq!(game.actor(monster).map(|a| a.energy()), monster_energy);
        assert_eq!(game.player_turns(), turns);
        assert_eq!(game.player_actor().essence(), ESSENCE_COST - 1);
    }

    #[test]
    fn essence_comes_back_one_point_per_period_up_to_the_maximum() {
        let mut game = game_knowing(&[]);
        let player = game.player();
        game.world.actors[player].set_essence(0);
        let period = usize::try_from(ESSENCE_REGEN_TICKS).unwrap();

        for _ in 0..period - 1 {
            game.submit(Action::Wait);
        }
        assert_eq!(game.player_actor().essence(), 0);
        game.submit(Action::Wait);
        assert_eq!(game.player_actor().essence(), 1);

        let to_full = period * usize::try_from(MAX_ESSENCE - 1).unwrap();
        for _ in 0..to_full + 3 * period {
            game.submit(Action::Wait);
        }
        assert_eq!(game.player_actor().essence(), MAX_ESSENCE);
    }

    #[test]
    fn stepping_on_a_glyph_teaches_its_sigil_once() {
        let ember = shipped("ember");
        let mut game = game_knowing(&[]);
        let player = game.player();
        let glyph = START.step(Direction::East);
        game.engrave(glyph, &ember);

        let events = game.submit(Action::Move(Direction::East));

        assert_eq!(
            events,
            [
                Event::Moved {
                    actor: player,
                    from: START,
                    to: glyph
                },
                Event::GlyphLearned {
                    sigil: "ember".to_owned(),
                    position: glyph,
                },
            ]
        );
        assert!(game.grimoire().knows("ember"));
        assert_eq!(
            game.engraving_at(glyph).map(|e| e.sigil().id()),
            Some("ember")
        );

        // Reading it again does nothing.
        game.submit(Action::Move(Direction::West));
        let before = game.grimoire().clone();
        let events = game.submit(Action::Move(Direction::East));
        assert!(matches!(events[..], [Event::Moved { .. }]), "{events:?}");
        assert_eq!(game.grimoire(), &before);
        assert_eq!(game.grimoire().len(), 1);
    }

    #[test]
    fn a_learnt_glyph_makes_its_sigil_castable_in_file_order() {
        let content = content();
        let ember = content.sigil("ember").expect("shipped sigil");
        let recall = content.sigil("recall").expect("shipped sigil");
        let mut game = game_knowing(&[]);
        game.engrave(START.step(Direction::East), recall);
        game.engrave(START.step(Direction::East).step(Direction::East), ember);

        assert_eq!(
            cast(&mut game, &[7, 5, 3], None)[0],
            cast_event(None, CastQuality::Fizzled)
        );
        game.submit(Action::Move(Direction::East));
        game.submit(Action::Move(Direction::East));

        let ids: Vec<&str> = game.grimoire().sigils().iter().map(SigilDef::id).collect();
        assert_eq!(ids, ["ember", "recall"], "file order, not learning order");
        assert_eq!(
            cast(&mut game, &[7, 5, 3], None)[0],
            cast_event(Some("ember"), CastQuality::Pure)
        );
    }

    #[test]
    #[should_panic(expected = "cannot engrave a glyph at (0, 0)")]
    fn engraving_in_a_wall_panics() {
        let mut game = game_knowing(&[]);
        game.engrave(Position::new(0, 0), &shipped("ember"));
    }

    #[test]
    fn a_new_game_holds_the_glyphs_of_its_first_floor() {
        let content = content();
        for value in [0, 42, 7] {
            let seed = Seed::new(value);
            let floor = generation::generate_with_monsters(seed, FIRST_DEPTH, &content);

            let game = Game::new(seed, &content);

            assert_eq!(game.engravings(), floor.glyphs());
            assert!(!game.engravings().is_empty());
            let known: Vec<&str> = game.grimoire().sigils().iter().map(SigilDef::id).collect();
            assert_eq!(known, ["ember"], "the sigils known at start only");
            assert_eq!(game.player_actor().essence(), MAX_ESSENCE);
        }
    }
}
