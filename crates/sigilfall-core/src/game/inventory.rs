//! Handling items: what `Action::PickUp`, `Action::Use` and `Action::Drop`
//! do to the game.
//!
//! Adds to [`Game`] the private steps of the three item actions. Only the
//! player handles items, and the inventory is the player's; the costs are
//! constants of the `schedule` module, the capacity is
//! [`INVENTORY_CAPACITY`]. Every refusal is like a move into a wall: one
//! event explains it, and no energy, no item and no time are spent.
//!
//! - Picking up takes the item lying on the player's cell into the
//!   inventory ([`Event::PickedUp`]). With nothing there it is refused
//!   ([`Event::NothingToPickUp`]); with a full inventory too, and the item
//!   stays on the floor ([`Event::InventoryFull`]).
//! - Using spends a carried item ([`Event::Used`]), then applies its effect
//!   (see [`Game::apply_item_effect`]). An item the player does not carry,
//!   or a page teaching a sigil already known, is refused
//!   ([`Event::ItemRefused`]).
//! - Dropping lays a carried item on the player's cell ([`Event::Dropped`]),
//!   unless the player does not carry it or an item already lies there
//!   ([`Event::ItemRefused`]).

use super::Game;
use crate::action::Action;
use crate::actor::ActorId;
use crate::content::ItemEffect;
use crate::event::Event;
use crate::item::{INVENTORY_CAPACITY, ItemId, ItemRefusal};
use crate::schedule::{action_cost, spend_energy};
use crate::sigil::MAX_STEADY_CASTS;

impl Game {
    /// Makes the player `actor` pick up the item lying on its cell, appends
    /// the events to `events` and returns whether it took time.
    pub(super) fn pick_up(&mut self, actor: ActorId, events: &mut Vec<Event>) -> bool {
        debug_assert_eq!(actor, self.world.player, "only the player handles items");
        let position = self.world.actors[actor].position;
        let Some(item) = self.world.item_at(position) else {
            events.push(Event::NothingToPickUp { position });
            return false;
        };
        let definition = self.item_definition_id(item);
        if self.world.inventory().len() >= INVENTORY_CAPACITY {
            events.push(Event::InventoryFull { item, definition });
            return false;
        }
        self.world.carry(item);
        spend_energy(&mut self.world.actors[actor], action_cost(Action::PickUp));
        events.push(Event::PickedUp { item, definition });
        true
    }

    /// Makes the player `actor` use the carried `item`, appends the events
    /// to `events` and returns whether it took time.
    pub(super) fn use_item(
        &mut self,
        actor: ActorId,
        item: ItemId,
        events: &mut Vec<Event>,
    ) -> bool {
        debug_assert_eq!(actor, self.world.player, "only the player handles items");
        if !self.world.carries(item) {
            events.push(Event::ItemRefused {
                item,
                reason: ItemRefusal::NotCarried,
            });
            return false;
        }
        let carried = self
            .world
            .item(item)
            .expect("a carried item is in the game");
        if let ItemEffect::Teach(sigil) = carried.definition().effect()
            && self.grimoire.knows(sigil.id())
        {
            events.push(Event::ItemRefused {
                item,
                reason: ItemRefusal::NothingToLearn,
            });
            return false;
        }
        let used = self.world.use_up(item);
        spend_energy(
            &mut self.world.actors[actor],
            action_cost(Action::Use(item)),
        );
        events.push(Event::Used {
            item,
            definition: used.definition().id().to_owned(),
        });
        self.apply_item_effect(actor, used.definition().effect(), events);
        true
    }

    /// Applies `effect`, the effect of an item `actor` just used, and
    /// appends the event of its consequence to `events`:
    ///
    /// - [`ItemEffect::Heal`] restores hit points, never above the maximum
    ///   ([`Event::Healed`]);
    /// - [`ItemEffect::RestoreEssence`] restores essence, never above the
    ///   maximum ([`Event::EssenceRestored`]);
    /// - [`ItemEffect::Teach`] adds the sigil to the grimoire
    ///   ([`Event::SigilLearned`]);
    /// - [`ItemEffect::Steady`] adds to the casts that an unstable trace will
    ///   cast pure, never above `sigil::MAX_STEADY_CASTS`
    ///   ([`Event::Steadied`], see the `cast` submodule).
    fn apply_item_effect(&mut self, actor: ActorId, effect: &ItemEffect, events: &mut Vec<Event>) {
        match effect {
            ItemEffect::Heal(power) => {
                let healed = &mut self.world.actors[actor];
                let amount = healed.heal(*power);
                events.push(Event::Healed {
                    actor,
                    amount,
                    hp: healed.hp(),
                });
            }
            ItemEffect::RestoreEssence(power) => {
                let restored = &mut self.world.actors[actor];
                let before = restored.essence();
                restored.set_essence(before + power);
                events.push(Event::EssenceRestored {
                    actor,
                    amount: restored.essence() - before,
                    essence: restored.essence(),
                });
            }
            ItemEffect::Teach(sigil) => {
                self.grimoire.learn(sigil);
                events.push(Event::SigilLearned {
                    sigil: sigil.id().to_owned(),
                });
            }
            ItemEffect::Steady(casts) => {
                self.steady_casts = self
                    .steady_casts
                    .saturating_add(*casts)
                    .min(MAX_STEADY_CASTS);
                events.push(Event::Steadied {
                    casts: self.steady_casts,
                });
            }
        }
    }

    /// Makes the player `actor` drop the carried `item` on its cell, appends
    /// the events to `events` and returns whether it took time.
    pub(super) fn drop_item(
        &mut self,
        actor: ActorId,
        item: ItemId,
        events: &mut Vec<Event>,
    ) -> bool {
        debug_assert_eq!(actor, self.world.player, "only the player handles items");
        let position = self.world.actors[actor].position;
        let refusal = if !self.world.carries(item) {
            Some(ItemRefusal::NotCarried)
        } else if self.world.item_at(position).is_some() {
            Some(ItemRefusal::CellTaken)
        } else {
            None
        };
        if let Some(reason) = refusal {
            events.push(Event::ItemRefused { item, reason });
            return false;
        }
        self.world.put_down(item, position);
        spend_energy(
            &mut self.world.actors[actor],
            action_cost(Action::Drop(item)),
        );
        events.push(Event::Dropped {
            item,
            definition: self.item_definition_id(item),
            position,
        });
        true
    }

    /// Returns the identifier of the definition of `item`, which must still
    /// be in the game, as events name it.
    fn item_definition_id(&self, item: ItemId) -> String {
        self.world
            .item(item)
            .expect("the item is still in the game")
            .definition()
            .id()
            .to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::PLAYER_MAX_HP;
    use crate::content::{Content, ItemDef, MonsterDef};
    use crate::game::{PLAYER_START, VisibleItem};
    use crate::generation::{self, FIRST_DEPTH};
    use crate::geometry::{Direction, Position};
    use crate::item::Item;
    use crate::rng::Seed;
    use crate::schedule::{DROP_COST, NORMAL_SPEED, PICK_UP_COST, USE_COST};
    use crate::sigil::{CastQuality, ESSENCE_COST, MAX_ESSENCE, Node, Trace};

    /// Seed of the test games; the fixed room does not depend on it.
    const SEED: Seed = Seed::new(42);

    /// A cell of the fixed room out of the player's sight from
    /// [`PLAYER_START`] (more than 16 columns away).
    const FAR_CELL: Position = Position::new(25, 3);

    /// The content shipped with the game.
    fn content() -> Content {
        Content::load().expect("the shipped content is valid")
    }

    /// The shipped item identified by `id`.
    fn shipped(id: &str) -> ItemDef {
        content().item(id).expect("a shipped item").clone()
    }

    /// A new game in the fixed room, the player alone at [`PLAYER_START`].
    fn fixed_room_game() -> Game {
        Game::in_fixed_room(SEED)
    }

    /// Places an item of kind `id` under the player and picks it up,
    /// returning its key.
    fn carry(game: &mut Game, id: &str) -> ItemId {
        let item = game.place_item(game.player_actor().position(), &shipped(id));
        let events = game.submit(Action::PickUp);
        assert!(
            matches!(events.first(), Some(Event::PickedUp { item: picked, .. }) if *picked == item),
            "{events:?}"
        );
        item
    }

    /// The player's energy and the elapsed ticks, for [`energy_spent`].
    fn clock(game: &Game) -> (i32, u64) {
        (game.player_actor().energy(), game.ticks())
    }

    /// Energy the player spent since `before` (its energy and the ticks
    /// then): what it had, plus what it gained since, minus what it has left.
    fn energy_spent(game: &Game, before: (i32, u64)) -> i32 {
        let (energy, ticks) = before;
        let gained = i32::try_from(game.ticks() - ticks).unwrap() * NORMAL_SPEED;
        energy + gained - game.player_actor().energy()
    }

    /// The keys of the items the player carries, in order.
    fn carried(game: &Game) -> Vec<ItemId> {
        game.inventory().map(|(id, _)| id).collect()
    }

    /// Asserts that `events`, the answer to an action of a fresh game, is
    /// the single refusal `expected`, and that nothing moved: same clock, no
    /// turn counted.
    fn assert_refused(game: &Game, before: (i32, u64), events: &[Event], expected: &Event) {
        assert_eq!(events, std::slice::from_ref(expected));
        assert_eq!(clock(game), before, "no energy and no time spent");
        assert_eq!(game.player_turns(), 0, "the refusal is not a turn");
    }

    /// A monster definition with `hp`, harmless enough not to kill the player
    /// in a test: normal speed, attack 1, no defense.
    fn creature(hp: i32) -> MonsterDef {
        let source = format!(
            "[Monster(id: \"test_creature\", name: \"test creature\", name_fr: Masculine(\"le monstre\"), plural: \"test creatures\", plural_fr: \"monstres\", family: Vermin, \
             elite: false, speed: {NORMAL_SPEED}, hp: {hp}, attack: 1, defense: 0, \
             min_depth: 1)]"
        );
        Content::parse(&source)
            .expect("valid test monster")
            .monsters()[0]
            .clone()
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

    // --- Picking up ------------------------------------------------------------

    #[test]
    fn picking_up_moves_the_item_into_the_inventory_for_its_cost() {
        let mut game = fixed_room_game();
        let draught = shipped("healing_draught");
        let item = game.place_item(PLAYER_START, &draught);
        let before = clock(&game);

        let events = game.submit(Action::PickUp);

        assert_eq!(
            events,
            [Event::PickedUp {
                item,
                definition: "healing_draught".to_owned(),
            }]
        );
        assert_eq!(carried(&game), [item]);
        let picked = game.item(item).expect("still in the game");
        assert!(picked.is_carried());
        assert_eq!(picked.definition(), &draught);
        assert_eq!(game.world.item_at(PLAYER_START), None, "the floor is bare");
        assert_eq!(
            game.visible_items().count(),
            0,
            "a carried item is not drawn"
        );
        assert_eq!(energy_spent(&game, before), PICK_UP_COST);
        assert_eq!(game.player_turns(), 1);
    }

    #[test]
    fn picking_up_keeps_the_order_items_were_taken_in() {
        let mut game = fixed_room_game();
        let first = carry(&mut game, "essence_phial");
        game.submit(Action::Move(Direction::East));
        let second = carry(&mut game, "healing_draught");

        assert_eq!(carried(&game), [first, second]);
    }

    #[test]
    fn picking_up_where_nothing_lies_is_refused_without_cost() {
        let mut game = fixed_room_game();
        game.place_item(
            PLAYER_START.step(Direction::East),
            &shipped("healing_draught"),
        );
        let before = clock(&game);

        let events = game.submit(Action::PickUp);

        assert_refused(
            &game,
            before,
            &events,
            &Event::NothingToPickUp {
                position: PLAYER_START,
            },
        );
        assert!(carried(&game).is_empty());
    }

    #[test]
    fn a_full_inventory_leaves_the_item_on_the_floor() {
        let mut game = fixed_room_game();
        for _ in 0..INVENTORY_CAPACITY {
            carry(&mut game, "healing_draught");
        }
        let full = carried(&game);
        let phial = game.place_item(PLAYER_START, &shipped("essence_phial"));
        let turns = game.player_turns();
        let before = clock(&game);

        let events = game.submit(Action::PickUp);

        assert_eq!(
            events,
            [Event::InventoryFull {
                item: phial,
                definition: "essence_phial".to_owned(),
            }]
        );
        assert_eq!(clock(&game), before, "no energy and no time spent");
        assert_eq!(game.player_turns(), turns);
        assert_eq!(carried(&game), full, "the inventory is unchanged");
        assert_eq!(game.world.item_at(PLAYER_START), Some(phial));
        assert_eq!(
            game.item(phial).and_then(Item::position),
            Some(PLAYER_START)
        );

        // Using one item frees a slot: the same pickup now succeeds.
        game.submit(Action::Use(full[0]));
        let events = game.submit(Action::PickUp);
        assert!(matches!(events[0], Event::PickedUp { item, .. } if item == phial));
        assert_eq!(carried(&game).len(), INVENTORY_CAPACITY);
    }

    // --- Using ----------------------------------------------------------------

    #[test]
    fn a_healing_draught_heals_for_its_cost_and_is_spent() {
        let mut game = fixed_room_game();
        let player = game.player();
        let item = carry(&mut game, "healing_draught");
        let ItemEffect::Heal(power) = *shipped("healing_draught").effect() else {
            panic!("the healing draught heals");
        };
        game.world.actors[player].take_damage(power + 3);
        let before = clock(&game);
        let turns = game.player_turns();

        let events = game.submit(Action::Use(item));

        assert_eq!(
            events,
            [
                Event::Used {
                    item,
                    definition: "healing_draught".to_owned(),
                },
                Event::Healed {
                    actor: player,
                    amount: power,
                    hp: PLAYER_MAX_HP - 3,
                },
            ]
        );
        assert_eq!(game.player_actor().hp(), PLAYER_MAX_HP - 3);
        assert_eq!(game.item(item), None, "the key no longer finds anything");
        assert!(carried(&game).is_empty());
        assert_eq!(energy_spent(&game, before), USE_COST);
        assert_eq!(game.player_turns(), turns + 1);
    }

    #[test]
    fn a_heal_stops_at_the_maximum_hit_points() {
        for missing in [0, 1, 4] {
            let mut game = fixed_room_game();
            let player = game.player();
            let item = carry(&mut game, "healing_draught");
            game.world.actors[player].take_damage(missing);

            let events = game.submit(Action::Use(item));

            assert_eq!(
                events[1],
                Event::Healed {
                    actor: player,
                    amount: missing,
                    hp: PLAYER_MAX_HP,
                },
                "{missing} hit points missing"
            );
            assert_eq!(game.player_actor().hp(), PLAYER_MAX_HP);
        }
    }

    #[test]
    fn essence_is_restored_up_to_its_maximum() {
        let ItemEffect::RestoreEssence(power) = *shipped("essence_phial").effect() else {
            panic!("the essence phial restores essence");
        };
        // From empty the whole power comes back; near the maximum it stops
        // there.
        for (essence, restored) in [(0, power), (MAX_ESSENCE - 1, 1), (MAX_ESSENCE, 0)] {
            let mut game = fixed_room_game();
            let player = game.player();
            let item = carry(&mut game, "essence_phial");
            game.world.actors[player].set_essence(essence);

            let events = game.submit(Action::Use(item));

            assert_eq!(
                events[1],
                Event::EssenceRestored {
                    actor: player,
                    amount: restored,
                    essence: essence + restored,
                },
                "from {essence}"
            );
            assert_eq!(game.player_actor().essence(), essence + restored);
            assert!(game.player_actor().essence() <= MAX_ESSENCE);
        }
    }

    #[test]
    fn using_an_item_not_carried_is_refused_without_cost() {
        let mut game = fixed_room_game();
        let player = game.player();
        game.world.actors[player].take_damage(5);
        let on_floor = game.place_item(PLAYER_START, &shipped("healing_draught"));

        for item in [on_floor, ItemId::default()] {
            let before = clock(&game);

            let events = game.submit(Action::Use(item));

            assert_refused(
                &game,
                before,
                &events,
                &Event::ItemRefused {
                    item,
                    reason: ItemRefusal::NotCarried,
                },
            );
            assert_eq!(game.player_actor().hp(), PLAYER_MAX_HP - 5);
        }
        assert_eq!(game.world.item_at(PLAYER_START), Some(on_floor));
    }

    #[test]
    fn an_item_used_up_cannot_be_used_twice() {
        let mut game = fixed_room_game();
        let item = carry(&mut game, "healing_draught");
        game.submit(Action::Use(item));
        let turns = game.player_turns();
        let before = clock(&game);

        let events = game.submit(Action::Use(item));

        assert_eq!(
            events,
            [Event::ItemRefused {
                item,
                reason: ItemRefusal::NotCarried,
            }]
        );
        assert_eq!(clock(&game), before);
        assert_eq!(game.player_turns(), turns);
    }

    #[test]
    fn a_page_teaches_its_sigil_once() {
        let mut game = fixed_room_game();
        let ItemEffect::Teach(veil) = shipped("veil_page").effect().clone() else {
            panic!("the page teaches a sigil");
        };
        let first = carry(&mut game, "veil_page");
        game.submit(Action::Move(Direction::East));
        let second = carry(&mut game, "veil_page");
        assert!(!game.grimoire().knows(veil.id()));

        let events = game.submit(Action::Use(first));

        assert_eq!(
            events[..2],
            [
                Event::Used {
                    item: first,
                    definition: "veil_page".to_owned(),
                },
                Event::SigilLearned {
                    sigil: veil.id().to_owned(),
                },
            ]
        );
        assert!(game.grimoire().knows(veil.id()));

        // A second page of a known sigil would teach nothing: it is kept.
        let turns = game.player_turns();
        let before = clock(&game);
        let events = game.submit(Action::Use(second));
        assert_eq!(
            events,
            [Event::ItemRefused {
                item: second,
                reason: ItemRefusal::NothingToLearn,
            }]
        );
        assert_eq!(clock(&game), before);
        assert_eq!(game.player_turns(), turns);
        assert_eq!(carried(&game), [second]);
    }

    #[test]
    fn steadied_casts_never_exceed_their_cap() {
        let ItemEffect::Steady(casts) = *shipped("steadying_ink").effect() else {
            panic!("the ink steadies casts");
        };
        assert!(2 * casts > MAX_STEADY_CASTS, "two inks reach the cap");
        let mut game = fixed_room_game();
        let first = carry(&mut game, "steadying_ink");
        game.submit(Action::Use(first));
        assert_eq!(game.steady_casts(), casts);

        let second = carry(&mut game, "steadying_ink");
        let events = game.submit(Action::Use(second));

        assert_eq!(
            events[1],
            Event::Steadied {
                casts: MAX_STEADY_CASTS
            }
        );
        assert_eq!(game.steady_casts(), MAX_STEADY_CASTS);
        let third = carry(&mut game, "steadying_ink");
        game.submit(Action::Use(third));
        assert_eq!(game.steady_casts(), MAX_STEADY_CASTS, "wasted at the cap");
    }

    #[test]
    fn steadying_ink_makes_the_next_unstable_casts_pure() {
        let content = content();
        let ember = content.sigil("ember").expect("shipped sigil");
        let ItemEffect::Steady(casts) = *shipped("steadying_ink").effect() else {
            panic!("the ink steadies casts");
        };
        let mut game = fixed_room_game();
        game.grimoire.learn(ember);
        let player = game.player();
        game.spawn_monster(Position::new(8, 5), &creature(99));
        let ink = carry(&mut game, "steadying_ink");

        let events = game.submit(Action::Use(ink));
        assert_eq!(events[1], Event::Steadied { casts });
        assert_eq!(game.steady_casts(), casts);

        // 7-5-2 is one edit from Ember's 7-5-3: unstable, but steadied.
        for left in (0..casts).rev() {
            game.world.actors[player].set_essence(MAX_ESSENCE);
            let events = game.submit(Action::Cast {
                trace: pressed(&[7, 5, 2]),
                direction: None,
            });

            assert_eq!(
                events[..2],
                [
                    Event::SigilCast {
                        sigil: Some("ember".to_owned()),
                        quality: CastQuality::Pure,
                    },
                    Event::CastSteadied {
                        sigil: "ember".to_owned(),
                        casts_left: left,
                    },
                ]
            );
            assert!(matches!(events[2], Event::Wounded { .. }), "{events:?}");
            assert!(
                !events.iter().any(|e| matches!(e, Event::Backlash { .. })),
                "{events:?}"
            );
            assert_eq!(game.steady_casts(), left);
        }

        // Once the ink is spent, the same trace is unstable again.
        game.world.actors[player].set_essence(MAX_ESSENCE);
        let events = game.submit(Action::Cast {
            trace: pressed(&[7, 5, 2]),
            direction: None,
        });
        assert_eq!(
            events[0],
            Event::SigilCast {
                sigil: Some("ember".to_owned()),
                quality: CastQuality::Unstable,
            }
        );
        assert!(events.iter().any(|e| matches!(e, Event::Backlash { .. })));
        assert!(game.player_actor().essence() <= MAX_ESSENCE - ESSENCE_COST);
    }

    #[test]
    fn a_pure_cast_keeps_the_steadied_casts() {
        let content = content();
        let ember = content.sigil("ember").expect("shipped sigil");
        let mut game = fixed_room_game();
        game.grimoire.learn(ember);
        game.spawn_monster(Position::new(8, 5), &creature(99));
        let ink = carry(&mut game, "steadying_ink");
        game.submit(Action::Use(ink));
        let casts = game.steady_casts();

        let events = game.submit(Action::Cast {
            trace: pressed(&[7, 5, 3]),
            direction: None,
        });

        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::CastSteadied { .. }))
        );
        assert_eq!(game.steady_casts(), casts);
    }

    // --- Dropping ---------------------------------------------------------------

    #[test]
    fn dropping_lays_the_item_on_the_player_cell_for_its_cost() {
        let mut game = fixed_room_game();
        let item = carry(&mut game, "essence_phial");
        game.submit(Action::Move(Direction::East));
        let here = PLAYER_START.step(Direction::East);
        let before = clock(&game);
        let turns = game.player_turns();

        let events = game.submit(Action::Drop(item));

        assert_eq!(
            events,
            [Event::Dropped {
                item,
                definition: "essence_phial".to_owned(),
                position: here,
            }]
        );
        assert!(carried(&game).is_empty());
        assert_eq!(game.item(item).and_then(Item::position), Some(here));
        assert_eq!(game.world.item_at(here), Some(item));
        assert_eq!(energy_spent(&game, before), DROP_COST);
        assert_eq!(game.player_turns(), turns + 1);

        // The same item can be picked up again, under the same key.
        let events = game.submit(Action::PickUp);
        assert!(matches!(events[0], Event::PickedUp { item: again, .. } if again == item));
    }

    #[test]
    fn dropping_an_item_not_carried_or_onto_another_is_refused_without_cost() {
        let mut game = fixed_room_game();
        let item = carry(&mut game, "essence_phial");
        let lying = game.place_item(PLAYER_START, &shipped("healing_draught"));
        let turns = game.player_turns();

        for (key, reason) in [
            (lying, ItemRefusal::NotCarried),
            (item, ItemRefusal::CellTaken),
        ] {
            let before = clock(&game);

            let events = game.submit(Action::Drop(key));

            assert_eq!(events, [Event::ItemRefused { item: key, reason }]);
            assert_eq!(clock(&game), before);
            assert_eq!(game.player_turns(), turns);
        }
        assert_eq!(carried(&game), [item]);
        assert_eq!(game.world.item_at(PLAYER_START), Some(lying));
    }

    // --- Reading for the binary ----------------------------------------------

    #[test]
    fn items_out_of_sight_are_not_listed_as_visible() {
        let mut game = fixed_room_game();
        let draught = shipped("healing_draught");
        let near = PLAYER_START.step(Direction::East);
        let seen = game.place_item(near, &draught);
        game.place_item(FAR_CELL, &draught);
        assert!(!game.is_visible(FAR_CELL));

        let visible: Vec<VisibleItem<'_>> = game.visible_items().collect();

        assert_eq!(
            visible,
            [VisibleItem {
                id: seen,
                position: near,
                definition: "healing_draught",
                family: draught.family(),
            }]
        );

        // Walking within sight of the far item shows it and leaves the near
        // one behind, out of sight; stepping back swaps them again, although
        // the far cell stays remembered.
        for _ in 0..18 {
            game.submit(Action::Move(Direction::East));
        }
        assert!(game.is_visible(FAR_CELL) && !game.is_visible(near));
        let positions: Vec<Position> = game.visible_items().map(|i| i.position).collect();
        assert_eq!(positions, [FAR_CELL]);
        for _ in 0..18 {
            game.submit(Action::Move(Direction::West));
        }
        assert!(game.is_remembered(FAR_CELL) && !game.is_visible(FAR_CELL));
        let positions: Vec<Position> = game.visible_items().map(|i| i.position).collect();
        assert_eq!(positions, [near]);
    }

    #[test]
    fn a_new_game_lays_the_items_of_its_first_floor() {
        let content = content();
        for value in 0..20 {
            let seed = Seed::new(value);
            let floor = generation::generate_with_monsters(seed, FIRST_DEPTH, &content);
            let game = Game::new(seed, &content);

            let mut expected: Vec<(Position, &str)> = floor
                .items()
                .iter()
                .map(|spawn| (spawn.position(), spawn.definition().id()))
                .collect();
            let mut placed: Vec<(Position, &str)> = game
                .world
                .items()
                .map(|(_, item)| {
                    let cell = item.position().expect("on the floor");
                    (cell, item.definition().id())
                })
                .collect();
            expected.sort_unstable_by_key(|&(p, _)| (p.y, p.x));
            placed.sort_unstable_by_key(|&(p, _)| (p.y, p.x));
            assert!(!expected.is_empty(), "seed {value}");
            assert_eq!(placed, expected, "seed {value}");
            assert!(game.inventory().next().is_none());
            assert_eq!(game.steady_casts(), 0);
        }
    }

    #[test]
    #[should_panic(expected = "cannot place an item")]
    fn placing_an_item_on_another_panics() {
        let mut game = fixed_room_game();
        let draught = shipped("healing_draught");
        game.place_item(PLAYER_START, &draught);
        game.place_item(PLAYER_START, &draught);
    }
}
