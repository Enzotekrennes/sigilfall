//! Acceptance tests of F7 (items and inventory) on the core crate.
//!
//! Written against the public API only. They check the three acceptance
//! scenarios at the level of events and game state: an item on the floor
//! picked up appears in the inventory, a healing draught drunk restores hit
//! points without passing the maximum, and a full inventory leaves the item
//! on the floor. Beyond them they check dropping, the refusals that cost no
//! time, the essence phial capped at the maximum, the ink that makes the
//! next two unstable casts pure, the page that teaches Veil, items out of
//! sight and the deterministic replay. The screens are checked by the
//! binary's tests.
//!
//! The rules are written out again here from the design (100 energy to pick
//! up or use, 50 to drop, ten slots, 30 hit points, 6 essence at most) rather
//! than read from the `schedule` or `item` constants, so that a silent change
//! of a rule is noticed. The cost of an action is measured, not computed: it
//! is the energy the player had, plus what it gained while the clock ran,
//! minus what it has left. Every loop is bounded.

use std::collections::{HashMap, VecDeque};

use sigilfall_core::action::Action;
use sigilfall_core::content::{Content, SigilDef};
use sigilfall_core::event::Event;
use sigilfall_core::game::{Game, PLAYER_START};
use sigilfall_core::generation::{self, FIRST_DEPTH};
use sigilfall_core::geometry::{Direction, Position};
use sigilfall_core::item::{ItemId, ItemRefusal};
use sigilfall_core::map::Map;
use sigilfall_core::rng::Seed;
use sigilfall_core::sigil::{CastQuality, Node, Trace};

/// Energy to pick up an item and to use one, and to drop one.
const PICK_UP: i32 = 100;
const USE: i32 = 100;
const DROP: i32 = 50;
/// Slots of the inventory.
const CAPACITY: usize = 10;
/// Hit points of the player at most, and what a healing draught restores.
const MAX_HP: i32 = 30;
const HEAL_POWER: i32 = 10;
/// Essence at most, and what an essence phial restores.
const MAX_ESSENCE: i32 = 6;
const PHIAL_POWER: i32 = 4;
/// Energy the player gains per tick (normal speed).
const GAIN_PER_TICK: i32 = 100;

/// Creatures whose behaviour is known in advance: a gnat that never hurts,
/// a biter that strikes hard and dies in a few blows and a pest that barely
/// scratches.
const TEST_MONSTERS: &str = r#"[
    Monster(id: "gnat", name: "cave gnat", family: Vermin, elite: false,
        speed: 100, hp: 999, attack: 0, defense: 0, min_depth: 1),
    Monster(id: "biter", name: "biter", family: Vermin, elite: false,
        speed: 100, hp: 5, attack: 6, defense: 0, min_depth: 1),
    Monster(id: "pest", name: "pest", family: Vermin, elite: false,
        speed: 100, hp: 1, attack: 2, defense: 0, min_depth: 1),
]"#;

/// The sigil and item files shipped with the game, for test content.
const SIGILS: &str = include_str!("../content/sigils.ron");
const ITEMS: &str = include_str!("../content/items.ron");

/// A cell in view of the start, not adjacent: 4 east, 2 up.
const FOE_CELL: Position = Position::new(9, 3);

/// A cell of the fixed room out of sight from the start (more than eight
/// cells away).
const FAR_CELL: Position = Position::new(25, 3);

/// Seeds named by the acceptance brief.
fn named_seeds() -> Vec<Seed> {
    vec![
        Seed::from(0),
        Seed::from(42),
        "7F3A-91C2-0B5E-D4A8".parse().expect("valid grouped seed"),
    ]
}

/// The content shipped with the game.
fn shipped() -> Content {
    Content::load().expect("shipped content is valid")
}

/// The shipped content with the test creatures of [`TEST_MONSTERS`].
fn test_content() -> Content {
    Content::parse_all(TEST_MONSTERS, SIGILS, ITEMS).expect("valid test content")
}

/// The shipped item `id`.
fn item_def(id: &str) -> sigilfall_core::content::ItemDef {
    shipped().item(id).expect("a shipped item").clone()
}

/// The shipped sigil `id`.
fn sigil(id: &str) -> SigilDef {
    shipped().sigil(id).expect("a shipped sigil").clone()
}

/// The trace obtained by pressing `digits` in order.
fn pressed(digits: &[u8]) -> Trace {
    let mut trace = Trace::new();
    for &digit in digits {
        let node = Node::new(digit).expect("a digit from 1 to 9");
        trace.push(node).expect("a node not linked yet");
    }
    trace
}

/// A fixed-room game of `seed` where the player knows the shipped sigil
/// `id` (its glyph engraved east of the start, read, and walked back from).
fn knowing(seed: Seed, id: &str) -> Game {
    let mut game = Game::in_fixed_room(seed);
    game.engrave(PLAYER_START.step(Direction::East), &sigil(id));
    game.submit(Action::Move(Direction::East));
    game.submit(Action::Move(Direction::West));
    assert!(game.grimoire().knows(id), "the glyph was read");
    game
}

/// Puts a creature of the test content at `cell` and returns its key.
fn foe(game: &mut Game, definition: &str, cell: Position) -> sigilfall_core::actor::ActorId {
    let content = test_content();
    game.spawn_monster(cell, content.monster(definition).expect("a test creature"))
}

/// The player's energy and the ticks elapsed, to measure a cost later.
fn clock(game: &Game) -> (i32, u64) {
    (game.player_actor().energy(), game.ticks())
}

/// Energy the player spent since `before` (see [`clock`]).
fn spent(game: &Game, before: (i32, u64)) -> i32 {
    let (energy, ticks) = before;
    let gained = i32::try_from(game.ticks() - ticks).unwrap() * GAIN_PER_TICK;
    energy + gained - game.player_actor().energy()
}

/// Places an item `id` under the player and picks it up, returning its key.
fn carry(game: &mut Game, id: &str) -> ItemId {
    let cell = game.player_actor().position();
    let item = game.place_item(cell, &item_def(id));
    let events = game.submit(Action::PickUp);
    assert!(
        matches!(events.first(), Some(Event::PickedUp { item: got, .. }) if *got == item),
        "{events:?}"
    );
    item
}

/// The keys of the items the player carries, in order.
fn carried(game: &Game) -> Vec<ItemId> {
    game.inventory().map(|(id, _)| id).collect()
}

/// The definition identifiers of the items the player carries, in order.
fn carried_names(game: &Game) -> Vec<String> {
    game.inventory()
        .map(|(_, item)| item.definition().id().to_owned())
        .collect()
}

/// Whether `game` lists an item lying on `cell` among the visible ones.
fn visible_item_at(game: &Game, cell: Position) -> bool {
    game.visible_items().any(|item| item.position == cell)
}

/// Asserts that `events` is exactly `expected` and that nothing was paid:
/// same clock, no turn counted.
fn assert_free_refusal(game: &Game, before: (i32, u64), turns: u64, events: &[Event], e: &Event) {
    assert_eq!(events, std::slice::from_ref(e));
    assert_eq!(clock(game), before, "no energy and no time spent");
    assert_eq!(game.player_turns(), turns, "the refusal is not a turn");
}

/// The shortest walk from `from` to `to` over walkable cells, or `None`.
/// Bounded by the number of cells of the map.
fn walk(map: &Map, from: Position, to: Position) -> Option<Vec<Direction>> {
    let mut came: HashMap<Position, (Position, Direction)> = HashMap::new();
    let mut queue = VecDeque::from([from]);
    let mut seen = vec![from];
    while let Some(cell) = queue.pop_front() {
        if cell == to {
            let mut path = Vec::new();
            let mut at = to;
            while at != from {
                let (before, direction) = came[&at];
                path.push(direction);
                at = before;
            }
            path.reverse();
            return Some(path);
        }
        for direction in Direction::ALL {
            let next = cell.step(direction);
            if map.can_step(cell, direction) && !seen.contains(&next) {
                seen.push(next);
                came.insert(next, (cell, direction));
                queue.push_back(next);
            }
        }
    }
    None
}

/// Wounds the player until it has lost at least `deficit` hit points, with a
/// creature `striker` next to it, then kills it. Returns the game.
/// Bounded: the loops stop after a fixed number of turns.
fn wounded(seed: Seed, deficit: i32, striker: &str) -> Game {
    let mut game = Game::in_fixed_room(seed);
    let biter = foe(&mut game, striker, PLAYER_START.step(Direction::East));
    for _ in 0..40 {
        if MAX_HP - game.player_actor().hp() >= deficit {
            break;
        }
        game.submit(Action::Wait);
    }
    assert!(MAX_HP - game.player_actor().hp() >= deficit, "wounded");
    for _ in 0..40 {
        if game.actor(biter).is_none() || game.is_over() {
            break;
        }
        game.submit(Action::Move(Direction::East));
    }
    assert!(!game.is_over(), "the player survived the striker");
    assert!(game.actor(biter).is_none(), "the striker is dead");
    game
}

// --- Scenario 1: an item on the floor, picked up, is in the inventory -------

#[test]
fn a_picked_up_item_leaves_the_floor_and_joins_the_inventory() {
    for seed in named_seeds() {
        for def in shipped().items() {
            let mut game = Game::in_fixed_room(seed);
            let cell = PLAYER_START;
            let item = game.place_item(cell, def);
            assert!(visible_item_at(&game, cell), "the item lies on the floor");
            assert!(carried(&game).is_empty(), "nothing carried yet");
            let before = clock(&game);

            let events = game.submit(Action::PickUp);

            assert_eq!(
                events,
                vec![Event::PickedUp {
                    item,
                    definition: def.id().to_owned()
                }],
                "seed {seed}, {}",
                def.id()
            );
            assert_eq!(carried(&game), vec![item]);
            assert!(game.item(item).unwrap().is_carried());
            assert_eq!(game.item(item).unwrap().position(), None);
            assert!(!visible_item_at(&game, cell), "the floor is empty again");
            assert_eq!(spent(&game, before), PICK_UP, "stooping costs a turn");
            assert_eq!(game.player_turns(), 1);
        }
    }
}

#[test]
fn items_are_carried_in_the_order_they_were_picked_up() {
    let mut game = Game::in_fixed_room(Seed::from(42));
    let wanted = ["steadying_ink", "healing_draught", "essence_phial"];
    let keys: Vec<ItemId> = wanted.iter().map(|id| carry(&mut game, id)).collect();

    assert_eq!(carried(&game), keys);
    assert_eq!(carried_names(&game), wanted);
}

#[test]
fn a_picked_up_item_can_be_picked_up_on_another_cell_after_walking() {
    let mut game = Game::in_fixed_room(Seed::from(7));
    let cell = PLAYER_START.step(Direction::South);
    let item = game.place_item(cell, &item_def("healing_draught"));
    let events = game.submit(Action::PickUp);
    assert_eq!(
        events,
        vec![Event::NothingToPickUp {
            position: PLAYER_START
        }],
        "the item is one cell away: not reachable from here"
    );

    game.submit(Action::Move(Direction::South));
    let events = game.submit(Action::PickUp);

    assert!(matches!(events.first(), Some(Event::PickedUp { item: got, .. }) if *got == item));
    assert_eq!(carried(&game), vec![item]);
}

#[test]
fn items_of_a_generated_floor_can_all_be_walked_to_and_picked_up() {
    for seed in named_seeds() {
        let floor = generation::generate_with_monsters(seed, FIRST_DEPTH, &shipped());
        let spawns: Vec<(Position, String)> = floor
            .items()
            .iter()
            .map(|s| (s.position(), s.definition().id().to_owned()))
            .collect();
        assert_eq!(spawns.len(), 3, "three items per floor, seed {seed}");

        // The same floor without its monsters, so that nothing blocks the way.
        let empty = generation::generate(seed, FIRST_DEPTH);
        let map = empty.map().clone();
        let start = empty.start();
        let mut game = Game::on_floor(seed, empty);
        for (position, id) in &spawns {
            game.place_item(*position, &item_def(id));
        }

        for (position, id) in &spawns {
            let here = game.player_actor().position();
            let path = walk(&map, here, *position).expect("an item the player can reach");
            assert!(path.len() < 400, "a bounded walk");
            for direction in path {
                game.submit(Action::Move(direction));
            }
            let events = game.submit(Action::PickUp);
            assert!(
                matches!(events.first(), Some(Event::PickedUp { definition, .. }) if definition == id),
                "seed {seed}, {id} at {position:?} from {start:?}: {events:?}"
            );
        }
        let mut names = carried_names(&game);
        let mut expected: Vec<String> = spawns.iter().map(|(_, id)| id.clone()).collect();
        names.sort();
        expected.sort();
        assert_eq!(names, expected, "seed {seed}: all three items carried");
    }
}

// --- Scenario 2: a healing draught restores hit points, capped --------------

#[test]
fn a_healing_draught_restores_its_power_when_the_wound_is_deeper() {
    for seed in named_seeds() {
        let mut game = wounded(seed, 11, "biter");
        let player = game.player();
        let draught = carry(&mut game, "healing_draught");
        let hp = game.player_actor().hp();
        assert!(
            hp < MAX_HP - HEAL_POWER,
            "the wound is deeper than the heal"
        );
        let before = clock(&game);

        let events = game.submit(Action::Use(draught));

        assert_eq!(
            events,
            vec![
                Event::Used {
                    item: draught,
                    definition: "healing_draught".to_owned()
                },
                Event::Healed {
                    actor: player,
                    amount: HEAL_POWER,
                    hp: hp + HEAL_POWER
                }
            ],
            "seed {seed}"
        );
        assert_eq!(game.player_actor().hp(), hp + HEAL_POWER);
        assert_eq!(spent(&game, before), USE);
    }
}

#[test]
fn a_healing_draught_never_takes_the_player_above_the_maximum() {
    for seed in named_seeds() {
        let mut game = wounded(seed, 1, "pest");
        let player = game.player();
        let draught = carry(&mut game, "healing_draught");
        let hp = game.player_actor().hp();
        let lacking = MAX_HP - hp;
        assert!(
            (1..HEAL_POWER).contains(&lacking),
            "a small wound: {lacking}"
        );

        let events = game.submit(Action::Use(draught));

        assert!(
            events.contains(&Event::Healed {
                actor: player,
                amount: lacking,
                hp: MAX_HP
            }),
            "seed {seed}: {events:?}"
        );
        assert_eq!(game.player_actor().hp(), MAX_HP);
        assert_eq!(
            game.player_actor().max_hp(),
            MAX_HP,
            "the maximum is intact"
        );
    }
}

#[test]
fn a_healing_draught_drunk_at_full_health_restores_nothing_and_is_spent() {
    for seed in named_seeds() {
        let mut game = Game::in_fixed_room(seed);
        let player = game.player();
        let draught = carry(&mut game, "healing_draught");
        assert_eq!(game.player_actor().hp(), MAX_HP);

        let events = game.submit(Action::Use(draught));

        assert!(
            events.contains(&Event::Healed {
                actor: player,
                amount: 0,
                hp: MAX_HP
            }),
            "{events:?}"
        );
        assert_eq!(game.player_actor().hp(), MAX_HP);
        assert!(carried(&game).is_empty(), "the draught is spent");
    }
}

#[test]
fn a_spent_item_is_gone_and_cannot_be_used_again() {
    let mut game = Game::in_fixed_room(Seed::from(42));
    let draught = carry(&mut game, "healing_draught");
    game.submit(Action::Use(draught));
    assert!(game.item(draught).is_none(), "the key finds nothing");
    let before = clock(&game);
    let turns = game.player_turns();

    let events = game.submit(Action::Use(draught));

    assert_free_refusal(
        &game,
        before,
        turns,
        &events,
        &Event::ItemRefused {
            item: draught,
            reason: ItemRefusal::NotCarried,
        },
    );
}

#[test]
fn only_the_item_used_is_spent() {
    let mut game = Game::in_fixed_room(Seed::from(42));
    let ink = carry(&mut game, "steadying_ink");
    let draught = carry(&mut game, "healing_draught");
    let phial = carry(&mut game, "essence_phial");

    game.submit(Action::Use(draught));

    assert_eq!(carried(&game), vec![ink, phial]);
}

// --- Scenario 3: a full inventory leaves the item on the floor --------------

#[test]
fn with_a_full_inventory_the_item_stays_on_the_floor_and_nothing_is_paid() {
    for seed in named_seeds() {
        let mut game = Game::in_fixed_room(seed);
        let held: Vec<ItemId> = (0..CAPACITY)
            .map(|_| carry(&mut game, "healing_draught"))
            .collect();
        assert_eq!(carried(&game).len(), CAPACITY, "ten slots, all used");
        let cell = game.player_actor().position();
        let phial = game.place_item(cell, &item_def("essence_phial"));
        let before = clock(&game);
        let turns = game.player_turns();

        let events = game.submit(Action::PickUp);

        assert_free_refusal(
            &game,
            before,
            turns,
            &events,
            &Event::InventoryFull {
                item: phial,
                definition: "essence_phial".to_owned(),
            },
        );
        assert_eq!(carried(&game), held, "the inventory did not change");
        assert!(!game.item(phial).unwrap().is_carried());
        assert_eq!(game.item(phial).unwrap().position(), Some(cell));
        assert!(visible_item_at(&game, cell), "the phial still lies there");
    }
}

#[test]
fn a_slot_freed_by_using_or_dropping_lets_the_player_pick_up_again() {
    let mut game = Game::in_fixed_room(Seed::from(42));
    for _ in 0..CAPACITY {
        carry(&mut game, "healing_draught");
    }
    let cell = game.player_actor().position();
    let phial = game.place_item(cell, &item_def("essence_phial"));
    assert!(matches!(
        game.submit(Action::PickUp).as_slice(),
        [Event::InventoryFull { .. }]
    ));

    // Drinking one frees a slot.
    let first = carried(&game)[0];
    game.submit(Action::Use(first));
    let events = game.submit(Action::PickUp);

    assert!(matches!(events.first(), Some(Event::PickedUp { item, .. }) if *item == phial));
    assert_eq!(carried(&game).len(), CAPACITY);

    // Walking on and dropping one frees a slot too.
    game.submit(Action::Move(Direction::South));
    let ink = game.place_item(game.player_actor().position(), &item_def("steadying_ink"));
    assert!(matches!(
        game.submit(Action::PickUp).as_slice(),
        [Event::InventoryFull { .. }]
    ));
    game.submit(Action::Move(Direction::South));
    let second = carried(&game)[0];
    game.submit(Action::Drop(second));
    game.submit(Action::Move(Direction::North));
    let events = game.submit(Action::PickUp);
    assert!(matches!(events.first(), Some(Event::PickedUp { item, .. }) if *item == ink));
}

// --- Dropping --------------------------------------------------------------

#[test]
fn a_dropped_item_lies_on_the_players_cell_for_half_a_turn() {
    for seed in named_seeds() {
        let mut game = Game::in_fixed_room(seed);
        let draught = carry(&mut game, "healing_draught");
        let before = clock(&game);

        let events = game.submit(Action::Drop(draught));

        assert_eq!(
            events,
            vec![Event::Dropped {
                item: draught,
                definition: "healing_draught".to_owned(),
                position: PLAYER_START
            }]
        );
        assert!(carried(&game).is_empty());
        assert_eq!(game.item(draught).unwrap().position(), Some(PLAYER_START));
        assert!(visible_item_at(&game, PLAYER_START));
        assert_eq!(spent(&game, before), DROP, "half a normal action");
    }
}

#[test]
fn a_dropped_item_can_be_picked_up_again_unchanged() {
    let mut game = Game::in_fixed_room(Seed::from(42));
    let phial = carry(&mut game, "essence_phial");
    game.submit(Action::Drop(phial));

    let events = game.submit(Action::PickUp);

    assert!(
        matches!(events.first(), Some(Event::PickedUp { item, definition })
        if *item == phial && definition == "essence_phial")
    );
    assert_eq!(carried(&game), vec![phial]);
}

#[test]
fn dropping_on_a_cell_that_holds_an_item_is_refused_for_free() {
    let mut game = Game::in_fixed_room(Seed::from(42));
    let ink = carry(&mut game, "steadying_ink");
    let cell = game.player_actor().position();
    game.place_item(cell, &item_def("healing_draught"));
    let before = clock(&game);
    let turns = game.player_turns();

    let events = game.submit(Action::Drop(ink));

    assert_free_refusal(
        &game,
        before,
        turns,
        &events,
        &Event::ItemRefused {
            item: ink,
            reason: ItemRefusal::CellTaken,
        },
    );
    assert_eq!(carried(&game), vec![ink], "the ink is still carried");
}

#[test]
fn dropping_an_item_not_carried_is_refused_for_free() {
    let mut game = Game::in_fixed_room(Seed::from(42));
    let lying = game.place_item(
        PLAYER_START.step(Direction::East),
        &item_def("essence_phial"),
    );
    let before = clock(&game);
    let turns = game.player_turns();

    let events = game.submit(Action::Drop(lying));

    assert_free_refusal(
        &game,
        before,
        turns,
        &events,
        &Event::ItemRefused {
            item: lying,
            reason: ItemRefusal::NotCarried,
        },
    );
    assert_eq!(
        game.item(lying).unwrap().position(),
        Some(PLAYER_START.step(Direction::East)),
        "the item did not move"
    );
}

#[test]
fn using_an_item_lying_on_the_floor_is_refused_for_free() {
    let mut game = Game::in_fixed_room(Seed::from(42));
    let lying = game.place_item(PLAYER_START, &item_def("healing_draught"));
    let before = clock(&game);

    let events = game.submit(Action::Use(lying));

    assert_free_refusal(
        &game,
        before,
        0,
        &events,
        &Event::ItemRefused {
            item: lying,
            reason: ItemRefusal::NotCarried,
        },
    );
    assert!(game.item(lying).is_some(), "the draught is still there");
}

// --- Refusals that cost no time ---------------------------------------------

#[test]
fn picking_up_where_nothing_lies_is_refused_for_free_and_lets_no_monster_act() {
    for seed in named_seeds() {
        let mut game = Game::in_fixed_room(seed);
        let biter = foe(&mut game, "biter", FOE_CELL);
        let at = game.actor(biter).unwrap().position();
        let before = clock(&game);

        for _ in 0..5 {
            let events = game.submit(Action::PickUp);
            assert_free_refusal(
                &game,
                before,
                0,
                &events,
                &Event::NothingToPickUp {
                    position: PLAYER_START,
                },
            );
        }
        assert_eq!(
            game.actor(biter).unwrap().position(),
            at,
            "no monster moved"
        );
        assert_eq!(game.ticks(), before.1);
    }
}

// --- The essence phial -----------------------------------------------------

#[test]
fn an_essence_phial_restores_essence_up_to_the_maximum_only() {
    // 0 to 3 casts of Ember first, so that the reserve is lacking 0, 2, 4 or
    // 6 points: the heal of 4 is whole, then capped, then useless.
    for seed in named_seeds() {
        for casts in 0..=3 {
            let mut game = knowing(seed, "ember");
            foe(&mut game, "gnat", FOE_CELL);
            for _ in 0..casts {
                game.submit(Action::Cast {
                    trace: pressed(&[7, 5, 3]),
                    direction: None,
                });
            }
            let phial = carry(&mut game, "essence_phial");
            let player = game.player();
            let essence = game.player_actor().essence();
            assert!(
                (0..=MAX_ESSENCE).contains(&essence),
                "seed {seed}, {casts} casts: {essence}"
            );
            let expected = PHIAL_POWER.min(MAX_ESSENCE - essence);

            let events = game.submit(Action::Use(phial));

            assert!(
                events.contains(&Event::EssenceRestored {
                    actor: player,
                    amount: expected,
                    essence: essence + expected
                }),
                "seed {seed}, {casts} casts, essence {essence}: {events:?}"
            );
            assert!(game.player_actor().essence() <= MAX_ESSENCE);
            assert_eq!(game.player_actor().max_essence(), MAX_ESSENCE);
            assert!(carried(&game).is_empty(), "the phial is spent");
        }
    }
}

#[test]
fn an_essence_phial_at_a_full_reserve_restores_nothing() {
    let mut game = Game::in_fixed_room(Seed::from(42));
    let player = game.player();
    let phial = carry(&mut game, "essence_phial");
    assert_eq!(game.player_actor().essence(), MAX_ESSENCE);

    let events = game.submit(Action::Use(phial));

    assert!(
        events.contains(&Event::EssenceRestored {
            actor: player,
            amount: 0,
            essence: MAX_ESSENCE
        }),
        "{events:?}"
    );
    assert_eq!(game.player_actor().essence(), MAX_ESSENCE);
}

// --- The ink: the next two unstable casts are pure ---------------------------

/// Casts Ember with the trace pressed as `digits`.
fn ember(game: &mut Game, digits: &[u8]) -> Vec<Event> {
    game.submit(Action::Cast {
        trace: pressed(digits),
        direction: None,
    })
}

/// Whether `events` hold a backlash.
fn has_backlash(events: &[Event]) -> bool {
    events.iter().any(|e| matches!(e, Event::Backlash { .. }))
}

#[test]
fn the_ink_makes_the_next_two_unstable_casts_pure_and_not_the_third() {
    for seed in named_seeds() {
        let mut game = knowing(seed, "ember");
        foe(&mut game, "gnat", FOE_CELL);
        let ink = carry(&mut game, "steadying_ink");
        assert_eq!(game.steady_casts(), 0);

        let events = game.submit(Action::Use(ink));
        assert!(events.contains(&Event::Steadied { casts: 2 }), "{events:?}");
        assert_eq!(game.steady_casts(), 2);

        // Two unstable traces, each one node off: pure, no backlash.
        for (n, left) in [(&[7, 5, 2][..], 1), (&[7, 8, 3][..], 0)] {
            let events = ember(&mut game, n);
            assert!(
                events.contains(&Event::CastSteadied {
                    sigil: "ember".to_owned(),
                    casts_left: left
                }),
                "seed {seed}, {n:?}: {events:?}"
            );
            assert!(!has_backlash(&events), "seed {seed}, {n:?}: {events:?}");
            assert!(
                events.iter().any(|e| matches!(
                    e,
                    Event::SigilCast {
                        quality: CastQuality::Pure,
                        ..
                    }
                )),
                "reported pure: {events:?}"
            );
            assert_eq!(game.steady_casts(), left);
        }

        // The third unstable trace is on its own: a backlash.
        let events = ember(&mut game, &[4, 5, 3]);
        assert!(has_backlash(&events), "seed {seed}: {events:?}");
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::CastSteadied { .. }))
        );
        assert_eq!(game.steady_casts(), 0);
    }
}

#[test]
fn an_exact_cast_does_not_use_up_the_ink() {
    let mut game = knowing(Seed::from(42), "ember");
    foe(&mut game, "gnat", FOE_CELL);
    let ink = carry(&mut game, "steadying_ink");
    game.submit(Action::Use(ink));

    let events = ember(&mut game, &[7, 5, 3]);

    assert!(!has_backlash(&events));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Event::CastSteadied { .. })),
        "an exact trace needs no help: {events:?}"
    );
    assert_eq!(game.steady_casts(), 2, "both steadied casts are kept");
}

#[test]
fn two_inks_add_up_to_the_cap_of_three_and_an_unknown_trace_still_fizzles() {
    let mut game = knowing(Seed::from(42), "ember");
    foe(&mut game, "gnat", FOE_CELL);
    let first = carry(&mut game, "steadying_ink");
    let second = carry(&mut game, "steadying_ink");

    game.submit(Action::Use(first));
    let events = game.submit(Action::Use(second));

    assert!(events.contains(&Event::Steadied { casts: 3 }), "{events:?}");
    assert_eq!(game.steady_casts(), 3);
    // A trace far from every known pattern is not steadied into a spell.
    let events = ember(&mut game, &[9, 1]);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::SigilCast { sigil: None, .. })),
        "{events:?}"
    );
    assert_eq!(game.steady_casts(), 3, "a fizzle does not use the ink up");
}

// --- The page that teaches Veil --------------------------------------------

#[test]
fn the_page_of_the_veil_teaches_veil_once_and_veil_can_then_be_cast() {
    for seed in named_seeds() {
        let mut game = Game::in_fixed_room(seed);
        assert!(!game.grimoire().knows("veil"));
        let page = carry(&mut game, "veil_page");
        let before = clock(&game);

        let events = game.submit(Action::Use(page));

        assert_eq!(
            events,
            vec![
                Event::Used {
                    item: page,
                    definition: "veil_page".to_owned()
                },
                Event::SigilLearned {
                    sigil: "veil".to_owned()
                }
            ]
        );
        assert!(game.grimoire().knows("veil"));
        assert_eq!(game.grimoire().len(), 1);
        assert_eq!(spent(&game, before), USE);

        // The sigil taught is the real one: its trace casts it.
        let gnat = foe(&mut game, "gnat", FOE_CELL);
        let events = game.submit(Action::Cast {
            trace: pressed(&[4, 7, 8, 9, 6]),
            direction: None,
        });
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::SigilCast { sigil: Some(id), quality: CastQuality::Pure } if id == "veil"
            )),
            "seed {seed}: {events:?}"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::Held { actor, .. } if *actor == gnat)),
            "Veil holds its target: {events:?}"
        );
    }
}

#[test]
fn a_page_of_a_known_sigil_is_refused_for_free_and_kept() {
    for seed in named_seeds() {
        // Known through its glyph.
        let mut game = knowing(seed, "veil");
        let page = carry(&mut game, "veil_page");
        let before = clock(&game);
        let turns = game.player_turns();

        let events = game.submit(Action::Use(page));

        assert_free_refusal(
            &game,
            before,
            turns,
            &events,
            &Event::ItemRefused {
                item: page,
                reason: ItemRefusal::NothingToLearn,
            },
        );
        assert_eq!(carried(&game), vec![page], "the page is not wasted");
        assert_eq!(game.grimoire().len(), 1);
    }
}

#[test]
fn a_second_page_of_the_veil_is_refused_once_the_first_is_read() {
    let mut game = Game::in_fixed_room(Seed::from(42));
    let first = carry(&mut game, "veil_page");
    let second = carry(&mut game, "veil_page");
    game.submit(Action::Use(first));
    let before = clock(&game);
    let turns = game.player_turns();

    let events = game.submit(Action::Use(second));

    assert_free_refusal(
        &game,
        before,
        turns,
        &events,
        &Event::ItemRefused {
            item: second,
            reason: ItemRefusal::NothingToLearn,
        },
    );
    assert_eq!(carried(&game), vec![second]);
}

#[test]
fn the_page_of_the_veil_is_never_found_on_the_first_floor() {
    // The shipped page is for depth 2 and below (decision of the content).
    for raw in 0..60_u64 {
        let seed = Seed::from(raw);
        let floor = generation::generate_with_monsters(seed, FIRST_DEPTH, &shipped());
        assert!(
            floor
                .items()
                .iter()
                .all(|spawn| spawn.definition().id() != "veil_page"),
            "seed {seed}"
        );
    }
}

// --- Items out of sight ------------------------------------------------------

#[test]
fn an_item_out_of_sight_is_not_visible_until_the_player_comes_close() {
    for seed in named_seeds() {
        let mut game = Game::in_fixed_room(seed);
        let far = game.place_item(FAR_CELL, &item_def("healing_draught"));
        assert!(!game.is_visible(FAR_CELL), "the cell is out of sight");
        assert!(!visible_item_at(&game, FAR_CELL), "seed {seed}: hidden");
        assert_eq!(game.visible_items().count(), 0);

        // Walk east along row 5 until the item comes into view.
        let mut seen_at = None;
        for step in 0..30 {
            if visible_item_at(&game, FAR_CELL) {
                seen_at = Some(step);
                break;
            }
            game.submit(Action::Move(Direction::East));
        }
        let seen_at = seen_at.expect("the item comes into view");
        assert!(seen_at > 0, "not visible from the start");
        assert!(game.is_visible(FAR_CELL));
        assert!(game.visible_items().any(|item| item.id == far));

        // Walk back out of view: the cell is remembered, the item is not shown.
        for _ in 0..30 {
            if !game.is_visible(FAR_CELL) {
                break;
            }
            game.submit(Action::Move(Direction::West));
        }
        assert!(!game.is_visible(FAR_CELL));
        assert!(game.is_remembered(FAR_CELL), "the cell was seen once");
        assert!(
            !visible_item_at(&game, FAR_CELL),
            "an item never stays drawn out of sight"
        );
    }
}

#[test]
fn carried_items_are_never_listed_as_lying_on_the_floor() {
    let mut game = Game::in_fixed_room(Seed::from(42));
    carry(&mut game, "essence_phial");
    assert_eq!(game.visible_items().count(), 0);
}

#[test]
fn visible_items_of_generated_floors_are_all_on_cells_in_sight() {
    for seed in named_seeds() {
        let game = Game::new(seed, &shipped());
        for item in game.visible_items() {
            assert!(game.is_visible(item.position), "seed {seed}");
        }
    }
}

// --- Deterministic replay ----------------------------------------------------

#[test]
fn the_floor_items_of_a_seed_are_always_the_same_and_differ_between_seeds() {
    let lists: Vec<Vec<(Position, String)>> = named_seeds()
        .into_iter()
        .map(|seed| {
            let a = generation::generate_with_monsters(seed, FIRST_DEPTH, &shipped());
            let b = generation::generate_with_monsters(seed, FIRST_DEPTH, &shipped());
            assert_eq!(a.items(), b.items(), "seed {seed}: reproducible");
            a.items()
                .iter()
                .map(|s| (s.position(), s.definition().id().to_owned()))
                .collect()
        })
        .collect();
    assert_ne!(lists[0], lists[1]);
    assert_ne!(lists[1], lists[2]);
}

/// Plays a fixed script of item actions, fights and casts in the fixed room
/// and returns everything that happened.
fn scripted_run(seed: Seed) -> (Vec<Vec<Event>>, Vec<String>, i32, i32, u64) {
    let mut game = knowing(seed, "ember");
    let content = test_content();
    for (i, id) in ["healing_draught", "essence_phial", "steadying_ink"]
        .iter()
        .enumerate()
    {
        let x = i32::try_from(i).unwrap();
        game.place_item(Position::new(5 + x, 8), content.item(id).unwrap());
    }
    foe(&mut game, "biter", Position::new(9, 3));
    let mut log = Vec::new();
    let mut keys: Vec<ItemId> = Vec::new();
    let play = |game: &mut Game, action: Action, log: &mut Vec<Vec<Event>>| {
        let events = game.submit(action);
        log.push(events.clone());
        events
    };
    for _ in 0..3 {
        play(&mut game, Action::Move(Direction::South), &mut log);
    }
    for _ in 0..3 {
        play(&mut game, Action::PickUp, &mut log);
        play(&mut game, Action::Move(Direction::East), &mut log);
    }
    keys.extend(carried(&game));
    for key in &keys {
        play(&mut game, Action::Use(*key), &mut log);
    }
    play(&mut game, Action::PickUp, &mut log);
    play(
        &mut game,
        Action::Cast {
            trace: pressed(&[7, 5, 2]),
            direction: None,
        },
        &mut log,
    );
    for _ in 0..6 {
        play(&mut game, Action::Move(Direction::North), &mut log);
    }
    (
        log,
        carried_names(&game),
        game.player_actor().hp(),
        game.player_actor().essence(),
        game.ticks(),
    )
}

#[test]
fn the_same_seed_and_actions_replay_identically() {
    for seed in named_seeds() {
        let first = scripted_run(seed);
        let second = scripted_run(seed);
        assert!(!first.0.is_empty());
        assert_eq!(first, second, "seed {seed}");
    }
}

#[test]
fn a_generated_game_replays_identically_with_item_actions() {
    for seed in named_seeds() {
        let run = |seed: Seed| {
            let mut game = Game::new(seed, &shipped());
            let mut log = Vec::new();
            for turn in 0..120_usize {
                let action = match turn % 6 {
                    0 => Action::PickUp,
                    1 => Action::Move(Direction::ALL[turn % 8]),
                    2 => Action::Wait,
                    3 => carried(&game)
                        .first()
                        .map_or(Action::Wait, |item| Action::Use(*item)),
                    4 => carried(&game)
                        .last()
                        .map_or(Action::PickUp, |item| Action::Drop(*item)),
                    _ => Action::Move(Direction::ALL[(turn / 3) % 8]),
                };
                log.push(game.submit(action));
                if game.is_over() {
                    break;
                }
            }
            (log, carried_names(&game), game.ticks())
        };
        assert_eq!(run(seed), run(seed), "seed {seed}");
    }
}

#[test]
fn a_third_ink_cannot_push_the_steadied_casts_past_the_cap() {
    let mut game = knowing(Seed::from(42), "ember");
    for _ in 0..3 {
        let ink = carry(&mut game, "steadying_ink");
        game.submit(Action::Use(ink));
    }
    assert_eq!(game.steady_casts(), 3, "decision D20: three at most");
}

#[test]
fn floor_items_never_share_a_cell_with_the_start_the_stairs_a_glyph_or_a_monster() {
    for raw in 0..120_u64 {
        for depth in 1..=3 {
            let seed = Seed::from(raw);
            let floor = generation::generate_with_monsters(seed, depth, &shipped());
            let cells: Vec<Position> = floor.items().iter().map(|s| s.position()).collect();
            for (i, cell) in cells.iter().enumerate() {
                assert!(!cells[i + 1..].contains(cell), "seed {seed}, depth {depth}");
                assert_ne!(*cell, floor.start(), "seed {seed}, depth {depth}");
                assert_ne!(*cell, floor.stairs(), "seed {seed}, depth {depth}");
                assert!(floor.map().is_walkable(*cell));
                assert!(
                    floor.glyphs().iter().all(|g| g.position() != *cell),
                    "seed {seed}, depth {depth}: item on a glyph"
                );
                assert!(
                    floor.monsters().iter().all(|m| m.position() != *cell),
                    "seed {seed}, depth {depth}: item under a monster"
                );
            }
        }
    }
}

#[test]
fn the_page_of_the_veil_can_only_appear_from_the_second_floor() {
    let mut seen_deep = false;
    for raw in 0..120_u64 {
        for depth in 1..=4_u32 {
            let floor = generation::generate_with_monsters(Seed::from(raw), depth, &shipped());
            let pages = floor
                .items()
                .iter()
                .filter(|s| s.definition().id() == "veil_page")
                .count();
            assert!(depth >= 2 || pages == 0, "seed {raw}, depth {depth}");
            seen_deep |= pages > 0;
        }
    }
    assert!(seen_deep, "the page does turn up on deeper floors");
}
