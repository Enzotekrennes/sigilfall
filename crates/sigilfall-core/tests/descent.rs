//! Acceptance tests of F8 (floors and difficulty), descent half, on the core
//! crate.
//!
//! Written against the public API only. They check the first and third
//! acceptance scenarios at the level of events and game state: the player on
//! the stairs who descends reaches a new floor of the seed one level deeper,
//! and the player off the stairs who tries to descend sees nothing happen.
//! Beyond them they check what the player keeps (hit points, essence, pack,
//! grimoire, steadied casts), that floor N does not depend on how the floors
//! above were played, the deepest floors (10, the bottom, whose seal ends
//! the run since task T33, and the floors generated beyond it) and the
//! deterministic replay across several floors. The spawn tables and the
//! difficulty are in `spawns.rs`; the screens are checked by the binary's
//! tests.
//!
//! The player cannot be placed from outside the crate, so the tests walk it
//! to the stairs along the shortest path, striking what blocks the way. On
//! the shipped content the monsters can kill the walker, so those tests keep
//! the seeds where it arrives alive and require enough of them; elsewhere the
//! monsters of the tests are harmless (1 hit point, no attack). The rules
//! are written out here (30 hit points at most, 6 essence, depth + 1, 6
//! monsters on the first floor then one more per floor up to 12) instead of
//! being read from the production constants. Every loop is bounded.

use std::collections::{HashMap, HashSet, VecDeque};

use sigilfall_core::action::Action;
use sigilfall_core::content::Content;
use sigilfall_core::event::Event;
use sigilfall_core::game::Game;
use sigilfall_core::generation::{self, Floor};
use sigilfall_core::geometry::{Direction, Position};
use sigilfall_core::map::{Map, Tile};
use sigilfall_core::rng::Seed;
use sigilfall_core::sigil::{Node, Trace};

/// Monsters that never get to hurt: at speed 1 they gain a hundredth of the
/// energy they need to act (a blow always deals at least 1, so attack 0 is
/// not enough), and one blow kills them. Four tiers that the spawn table
/// below keeps on disjoint bands of floors.
const HARMLESS: &str = r#"[
    Monster(id: "mote_one", name: "mote one", family: Vermin, elite: false,
        speed: 1, hp: 1, attack: 0, defense: 0, min_depth: 1),
    Monster(id: "mote_two", name: "mote two", family: Vermin, elite: false,
        speed: 1, hp: 1, attack: 0, defense: 0, min_depth: 2),
    Monster(id: "mote_three", name: "mote three", family: Vermin, elite: false,
        speed: 1, hp: 1, attack: 0, defense: 0, min_depth: 4),
    Monster(id: "mote_four", name: "mote four", family: Vermin, elite: false,
        speed: 1, hp: 1, attack: 0, defense: 0, min_depth: 7),
]"#;

/// One tier per band of floors, so the creatures of a floor name its depth.
const TIERS: &str = r#"[
    Spawn(of: Monster("mote_one"), from_depth: 1, to_depth: 1, weight: 1),
    Spawn(of: Monster("mote_two"), from_depth: 2, to_depth: 3, weight: 1),
    Spawn(of: Monster("mote_three"), from_depth: 4, to_depth: 6, weight: 1),
    Spawn(of: Monster("mote_four"), from_depth: 7, to_depth: 10, weight: 1),
    Spawn(of: Item("healing_draught"), from_depth: 1, to_depth: 10, weight: 1),
    Spawn(of: Item("essence_phial"), from_depth: 1, to_depth: 10, weight: 1),
    Spawn(of: Item("steadying_ink"), from_depth: 1, to_depth: 10, weight: 1),
    Spawn(of: Item("veil_page"), from_depth: 2, to_depth: 10, weight: 1),
]"#;

/// A biter that strikes hard, to wound the player on purpose.
const BITER: &str = r#"[
    Monster(id: "biter", name: "biter", family: Vermin, elite: false,
        speed: 100, hp: 5, attack: 6, defense: 0, min_depth: 1),
]"#;

/// The shipped files, for test content.
const SIGILS: &str = include_str!("../content/sigils.ron");
const ITEMS: &str = include_str!("../content/items.ron");

/// Most actions spent walking to the stairs of one floor.
const WALK_BUDGET: usize = 1500;

/// Floors of the harmless tour: down to the bottom floor (10), the deepest
/// floor the content is written for, which has a seal instead of stairs
/// (decision D23).
const TOUR_DEPTH: u32 = 10;

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

/// Harmless monsters on the banded table of [`TIERS`].
fn tiers() -> Content {
    Content::parse_with_spawns(HARMLESS, SIGILS, ITEMS, TIERS).expect("valid test content")
}

/// The trace obtained by pressing `digits` in order.
fn pressed(digits: &[u8]) -> Trace {
    let mut trace = Trace::new();
    for &digit in digits {
        trace
            .push(Node::new(digit).expect("a digit from 1 to 9"))
            .expect("a node not linked yet");
    }
    trace
}

/// Every cell of `map`, in reading order.
fn cells(map: &Map) -> Vec<Position> {
    let (width, height) = (
        i32::try_from(map.width()).unwrap(),
        i32::try_from(map.height()).unwrap(),
    );
    (0..height)
        .flat_map(|y| (0..width).map(move |x| Position::new(x, y)))
        .collect()
}

/// The cell of the way down of the current floor of `game`: its stairs, or
/// on the bottom floor the seal that lies there instead, whose breaking
/// ends the run.
fn stairs_of(game: &Game) -> Position {
    cells(game.map())
        .into_iter()
        .find(|&cell| matches!(game.map().tile(cell), Some(Tile::StairsDown | Tile::Seal)))
        .expect("every generated floor has stairs or a seal")
}

/// The first step of a shortest walk from `from` to `to` over walkable
/// cells, ignoring creatures; `None` when `to` cannot be reached. Bounded by
/// the number of cells.
fn first_step(map: &Map, from: Position, to: Position) -> Option<Direction> {
    let mut first: HashMap<Position, Direction> = HashMap::new();
    let mut seen = HashSet::from([from]);
    let mut queue = VecDeque::from([from]);
    while let Some(cell) = queue.pop_front() {
        for direction in Direction::ALL {
            let next = cell.step(direction);
            if !map.can_step(cell, direction) || !seen.insert(next) {
                continue;
            }
            first.insert(
                next,
                if cell == from {
                    direction
                } else {
                    first[&cell]
                },
            );
            if next == to {
                return Some(first[&next]);
            }
            queue.push_back(next);
        }
    }
    None
}

/// Walks the player of `game` to the stairs of its floor, striking what
/// stands in the way (a move into a creature is a blow). Returns whether it
/// stands on the stairs, alive, after at most [`WALK_BUDGET`] actions.
fn reach_stairs(game: &mut Game) -> bool {
    for _ in 0..WALK_BUDGET {
        if game.is_over() {
            return false;
        }
        let here = game.player_actor().position();
        let stairs = stairs_of(game);
        if here == stairs {
            return true;
        }
        let Some(direction) = first_step(game.map(), here, stairs) else {
            return false;
        };
        game.submit(Action::Move(direction));
    }
    false
}

/// Walks to the stairs and descends. Returns the events of the descent, or
/// `None` when the stairs could not be reached alive.
fn descend(game: &mut Game) -> Option<Vec<Event>> {
    reach_stairs(game).then(|| game.submit(Action::Descend))
}

/// What a floor holds that a player can compare: tiles, the player's cell,
/// and the monsters (cell, kind, hit points, energy), sorted by cell.
type FloorState = (
    Vec<Option<Tile>>,
    Position,
    Vec<(Position, String, i32, i32)>,
);

/// The state of the current floor of `game`.
fn floor_state(game: &Game) -> FloorState {
    let tiles = cells(game.map())
        .into_iter()
        .map(|cell| game.map().tile(cell))
        .collect();
    let mut monsters: Vec<_> = game
        .actors()
        .filter_map(|(_, actor)| {
            let monster = actor.as_monster()?;
            Some((
                actor.position(),
                monster.definition().to_owned(),
                actor.hp(),
                actor.energy(),
            ))
        })
        .collect();
    monsters.sort_by_key(|monster| (monster.0.y, monster.0.x));
    (tiles, game.player_actor().position(), monsters)
}

/// The tiles and the sorted monster cells and kinds `floor` was generated
/// with, to compare with what a game made of it.
fn generated(floor: &Floor) -> (Vec<Option<Tile>>, Position, Vec<(Position, String)>) {
    let tiles = cells(floor.map())
        .into_iter()
        .map(|cell| floor.map().tile(cell))
        .collect();
    let mut monsters: Vec<_> = floor
        .monsters()
        .iter()
        .map(|spawn| (spawn.position(), spawn.definition().id().to_owned()))
        .collect();
    monsters.sort_by_key(|monster| (monster.0.y, monster.0.x));
    (tiles, floor.start(), monsters)
}

/// The tiles, the player's cell and the sorted monster cells and kinds of
/// the current floor of `game`, in the shape of [`generated`].
fn observed(game: &Game) -> (Vec<Option<Tile>>, Position, Vec<(Position, String)>) {
    let (tiles, start, monsters) = floor_state(game);
    (
        tiles,
        start,
        monsters
            .into_iter()
            .map(|(cell, kind, _, _)| (cell, kind))
            .collect(),
    )
}

/// The kinds of monster on the current floor of `game`, without duplicates.
fn kinds(game: &Game) -> Vec<String> {
    let mut kinds: Vec<String> = floor_state(game)
        .2
        .into_iter()
        .map(|(_, kind, _, _)| kind)
        .collect();
    kinds.sort();
    kinds.dedup();
    kinds
}

/// Number of monsters on the current floor of `game`.
fn monster_count(game: &Game) -> usize {
    game.actors()
        .filter(|(_, actor)| actor.as_monster().is_some())
        .count()
}

/// The clock and the energy of every actor, to check that nothing was paid.
fn clock(game: &Game) -> (u64, u64, Vec<i32>) {
    let mut energies: Vec<i32> = game.actors().map(|(_, actor)| actor.energy()).collect();
    energies.sort_unstable();
    (game.ticks(), game.player_turns(), energies)
}

// --- Scenario 1: on the stairs, descending gives the next floor of the seed ---

#[test]
fn scenario_1_descending_generates_the_next_floor_of_the_seed_one_level_deeper() {
    let content = tiers();
    for seed in named_seeds() {
        let mut game = Game::new(seed, &content);
        assert_eq!(game.depth(), 1);
        let floor_above = observed(&game);
        let ticks = game.ticks();

        let events = descend(&mut game).expect("the harmless walker reaches the stairs");

        assert_eq!(events, [Event::Descended { depth: 2 }], "seed {seed}");
        assert_eq!(game.depth(), 2, "seed {seed}");
        let below = generation::generate_with_monsters(seed, 2, &content);
        assert_eq!(
            observed(&game),
            generated(&below),
            "seed {seed}: the floor is the second floor of the seed"
        );
        assert_eq!(game.player_actor().position(), below.start());
        assert_ne!(observed(&game).0, floor_above.0, "a different layout");
        assert!(game.ticks() > ticks && !game.is_over());
        assert_eq!(game.map().tile(below.stairs()), Some(Tile::StairsDown));
    }
}

#[test]
fn scenario_1_each_descent_adds_one_to_the_depth_and_every_floor_is_the_floor_of_its_depth() {
    let content = tiers();
    for seed in named_seeds() {
        let mut game = Game::new(seed, &content);
        for depth in 2..=TOUR_DEPTH {
            let events = descend(&mut game).expect("the harmless walker reaches the stairs");
            assert_eq!(events, [Event::Descended { depth }], "seed {seed}");
            assert_eq!(game.depth(), depth);
            let floor = generation::generate_with_monsters(seed, depth, &content);
            assert_eq!(
                observed(&game),
                generated(&floor),
                "seed {seed}, depth {depth}"
            );
        }
    }
}

#[test]
fn scenario_1_the_same_seed_gives_the_same_second_floor_and_another_seed_another_one() {
    let content = tiers();
    let (first, second) = (Seed::from(42), Seed::from(43));
    let (mut a, mut b, mut other) = (
        Game::new(first, &content),
        Game::new(first, &content),
        Game::new(second, &content),
    );
    for game in [&mut a, &mut b, &mut other] {
        descend(game).expect("the stairs are reached");
    }

    assert_eq!(floor_state(&a), floor_state(&b));
    assert_ne!(
        observed(&a).0,
        observed(&other).0,
        "another seed, another floor"
    );
    // Generated from the same seed outside a game, twice.
    let one = generation::generate_with_monsters(first, 2, &content);
    let two = generation::generate_with_monsters(first, 2, &content);
    assert_eq!(generated(&one), generated(&two));
}

#[test]
fn scenario_1_on_the_shipped_content_the_descent_reaches_the_shipped_second_floor() {
    let content = shipped();
    let mut arrived = 0;
    for value in 0..24 {
        let seed = Seed::from(value);
        let mut game = Game::new(seed, &content);
        let Some(events) = descend(&mut game) else {
            continue; // the walker died on the way: not what is measured here
        };
        arrived += 1;
        assert!(
            events.contains(&Event::Descended { depth: 2 }),
            "seed {seed}: {events:?}"
        );
        assert_eq!(game.depth(), 2);
        let floor = generation::generate_with_monsters(seed, 2, &content);
        let (tiles, start, _) = generated(&floor);
        let (seen_tiles, here, _) = observed(&game);
        assert_eq!((seen_tiles, here), (tiles, start), "seed {seed}");
        assert_eq!(
            monster_count(&game),
            7,
            "6 on the first floor, 7 on the second"
        );
    }
    assert!(
        arrived >= 8,
        "only {arrived} of 24 seeds reached the stairs alive"
    );
}

// --- Scenario 3: off the stairs, nothing happens ------------------------------

#[test]
fn scenario_3_descending_off_the_stairs_changes_nothing_and_names_the_cell() {
    let content = tiers();
    for seed in named_seeds() {
        let mut game = Game::new(seed, &content);
        let here = game.player_actor().position();
        assert_ne!(game.map().tile(here), Some(Tile::StairsDown));
        let (before, floor, hp, essence) = (
            clock(&game),
            floor_state(&game),
            game.player_actor().hp(),
            game.player_actor().essence(),
        );

        for _ in 0..5 {
            let events = game.submit(Action::Descend);
            assert_eq!(events, [Event::NoStairsHere { position: here }]);
            assert_eq!(game.depth(), 1);
            assert_eq!(clock(&game), before, "no energy, no time, no turn");
            assert_eq!(floor_state(&game), floor, "no monster moved");
            assert_eq!(
                (game.player_actor().hp(), game.player_actor().essence()),
                (hp, essence)
            );
        }
    }
}

#[test]
fn scenario_3_a_cell_next_to_the_stairs_is_not_the_stairs() {
    let content = tiers();
    let mut game = Game::new(Seed::from(42), &content);
    let stairs = stairs_of(&game);
    // Walk until one step from the stairs: the shortest path ends there.
    for _ in 0..WALK_BUDGET {
        let here = game.player_actor().position();
        let direction = first_step(game.map(), here, stairs).expect("reachable");
        if here.step(direction) == stairs {
            break;
        }
        game.submit(Action::Move(direction));
    }
    let here = game.player_actor().position();
    assert_eq!(
        here.step(first_step(game.map(), here, stairs).unwrap()),
        stairs
    );
    let before = clock(&game);

    let events = game.submit(Action::Descend);

    assert_eq!(events, [Event::NoStairsHere { position: here }]);
    assert_eq!(game.depth(), 1);
    assert_eq!(clock(&game), before);
}

#[test]
fn scenario_3_the_refusal_also_holds_on_a_deeper_floor_and_on_the_fixed_room() {
    let content = tiers();
    let mut game = Game::new(Seed::from(7), &content);
    descend(&mut game).expect("the stairs are reached");
    let here = game.player_actor().position();
    let before = clock(&game);

    assert_eq!(
        game.submit(Action::Descend),
        [Event::NoStairsHere { position: here }]
    );
    assert_eq!((game.depth(), clock(&game)), (2, before));

    let mut room = Game::in_fixed_room(Seed::from(7));
    let cell = room.player_actor().position();
    assert_eq!(
        room.submit(Action::Descend),
        [Event::NoStairsHere { position: cell }]
    );
    assert_eq!(room.depth(), 1);
}

#[test]
fn scenario_3_after_a_refusal_the_game_goes_on_normally_and_the_stairs_still_work() {
    let content = tiers();
    let mut game = Game::new(Seed::from(42), &content);
    game.submit(Action::Descend);
    game.submit(Action::Descend);

    let events = descend(&mut game).expect("the stairs are reached");

    assert_eq!(events, [Event::Descended { depth: 2 }]);
    assert_eq!(game.depth(), 2);
}

#[test]
fn scenario_3_descending_costs_exactly_one_action_when_it_succeeds() {
    let content = tiers();
    let mut game = Game::new(Seed::from(42), &content);
    assert!(reach_stairs(&mut game));
    let (ticks, turns) = (game.ticks(), game.player_turns());

    game.submit(Action::Descend);

    assert_eq!(game.player_turns(), turns + 1);
    assert_eq!(
        game.ticks(),
        ticks + 1,
        "one action of 100 energy at speed 100"
    );
}

// --- What the player keeps ---------------------------------------------------

/// What the player owns, as it can be observed.
#[derive(Debug, PartialEq, Eq)]
struct Owned {
    hp: i32,
    essence: i32,
    max_hp: i32,
    pack: Vec<(String, bool)>,
    pack_ids: usize,
    sigils: Vec<String>,
    steady: u32,
}

fn owned(game: &Game) -> Owned {
    let player = game.player_actor();
    Owned {
        hp: player.hp(),
        essence: player.essence(),
        max_hp: player.max_hp(),
        pack: game
            .inventory()
            .map(|(_, item)| (item.definition().id().to_owned(), item.is_carried()))
            .collect(),
        pack_ids: game.inventory().count(),
        sigils: game
            .grimoire()
            .sigils()
            .iter()
            .map(|sigil| sigil.id().to_owned())
            .collect(),
        steady: game.steady_casts(),
    }
}

/// A game on the first floor, with its player on the stairs, wounded, short
/// of essence, knowing Ember, carrying a draught, a phial, a page of the veil
/// and a steadying ink drunk once. Returns it and the pack keys.
fn prepared(seed: Seed) -> Game {
    let content = tiers();
    let helpers = Content::parse_with_spawns(BITER, SIGILS, ITEMS, TIERS_FOR_BITER)
        .expect("valid helper content");
    let mut game = Game::new(seed, &content);
    assert!(
        reach_stairs(&mut game),
        "seed {seed}: the stairs are reached"
    );
    let stairs = game.player_actor().position();
    let shipped = shipped();

    // Learn Ember: engrave it on a free neighbour, step on it, come back.
    let neighbour = Direction::ALL
        .into_iter()
        .find(|&d| {
            let cell = stairs.step(d);
            game.map().can_step(stairs, d)
                && game.map().tile(cell) == Some(Tile::Floor)
                && game.actors().all(|(_, a)| a.position() != cell)
        })
        .expect("a free floor cell next to the stairs");
    game.engrave(stairs.step(neighbour), shipped.sigil("ember").unwrap());
    game.submit(Action::Move(neighbour));
    game.submit(Action::Move(opposite(neighbour)));
    assert!(game.grimoire().knows("ember"), "seed {seed}");
    assert_eq!(game.player_actor().position(), stairs);

    // Carry items.
    for id in [
        "healing_draught",
        "essence_phial",
        "veil_page",
        "steadying_ink",
    ] {
        game.place_item(stairs, shipped.item(id).unwrap());
        let events = game.submit(Action::PickUp);
        assert!(
            matches!(events.first(), Some(Event::PickedUp { .. })),
            "{id}: {events:?}"
        );
    }
    // A second ink, used: two steadied casts.
    game.place_item(stairs, shipped.item("steadying_ink").unwrap());
    game.submit(Action::PickUp);
    let ink = game
        .inventory()
        .find(|(_, item)| item.definition().id() == "steadying_ink")
        .map(|(id, _)| id)
        .unwrap();
    game.submit(Action::Use(ink));
    assert_eq!(game.steady_casts(), 2);

    // Wound the player with a biter, then kill it.
    let biter = game.spawn_monster(stairs.step(neighbour), helpers.monster("biter").unwrap());
    for _ in 0..40 {
        if game.player_actor().hp() < game.player_actor().max_hp() {
            break;
        }
        game.submit(Action::Wait);
    }
    for _ in 0..40 {
        if game.actor(biter).is_none() || game.is_over() {
            break;
        }
        game.submit(Action::Move(neighbour));
    }
    assert!(!game.is_over() && game.actor(biter).is_none());
    assert!(game.player_actor().hp() < game.player_actor().max_hp());

    // Spend essence on an Ember at a creature in sight.
    let target = cells(game.map())
        .into_iter()
        .find(|&cell| {
            let (dx, dy) = ((cell.x - stairs.x).abs(), (cell.y - stairs.y).abs());
            game.is_visible(cell)
                && game.map().is_walkable(cell)
                && dx.max(dy) >= 3
                && game.actors().all(|(_, a)| a.position() != cell)
        })
        .expect("a free cell in view");
    game.spawn_monster(target, content.monster("mote_one").unwrap());
    game.submit(Action::Cast {
        trace: pressed(&[7, 5, 3]),
        direction: None,
    });
    assert!(game.player_actor().essence() < game.player_actor().max_essence());
    assert!(!game.is_over());
    game
}

/// The spawn table that goes with [`BITER`].
const TIERS_FOR_BITER: &str = r#"[
    Spawn(of: Monster("biter"), from_depth: 1, to_depth: 10, weight: 1),
    Spawn(of: Item("healing_draught"), from_depth: 1, to_depth: 10, weight: 1),
    Spawn(of: Item("essence_phial"), from_depth: 1, to_depth: 10, weight: 1),
    Spawn(of: Item("steadying_ink"), from_depth: 1, to_depth: 10, weight: 1),
    Spawn(of: Item("veil_page"), from_depth: 2, to_depth: 10, weight: 1),
]"#;

/// The direction that undoes `direction`.
fn opposite(direction: Direction) -> Direction {
    match direction {
        Direction::North => Direction::South,
        Direction::NorthEast => Direction::SouthWest,
        Direction::East => Direction::West,
        Direction::SouthEast => Direction::NorthWest,
        Direction::South => Direction::North,
        Direction::SouthWest => Direction::NorthEast,
        Direction::West => Direction::East,
        Direction::NorthWest => Direction::SouthEast,
    }
}

#[test]
fn the_player_keeps_hit_points_essence_pack_grimoire_and_ink_through_a_descent() {
    for seed in named_seeds() {
        let mut game = prepared(seed);
        let mut stairs_game = game.player_actor().position();
        // `prepared` leaves the player on the stairs.
        assert_eq!(game.map().tile(stairs_game), Some(Tile::StairsDown));
        let before = owned(&game);
        assert!(before.hp < before.max_hp, "wounded");
        assert!(before.essence < 6, "short of essence");
        assert_eq!(before.pack.len(), 4, "draught, phial, page, one ink left");
        assert!(before.sigils.contains(&"ember".to_owned()));
        assert_eq!(before.steady, 2);

        let events = game.submit(Action::Descend);
        stairs_game = game.player_actor().position();

        assert!(
            events.contains(&Event::Descended { depth: 2 }),
            "seed {seed}: {events:?}"
        );
        let after = owned(&game);
        assert_eq!(after.pack, before.pack, "seed {seed}: the pack");
        assert_eq!(after.sigils, before.sigils, "seed {seed}: the grimoire");
        assert_eq!(after.steady, before.steady, "seed {seed}: the ink");
        assert_eq!(after.max_hp, before.max_hp);
        assert_eq!(after.hp, before.hp, "seed {seed}: hit points");
        // The descent takes a tick: one essence point may be recovered if
        // its period ends exactly there, which is the clock's doing.
        assert!(
            (before.essence..=before.essence + 1).contains(&after.essence),
            "seed {seed}: essence {} -> {}",
            before.essence,
            after.essence
        );
        assert_ne!(
            game.map().tile(stairs_game),
            Some(Tile::StairsDown),
            "the player arrives on the start cell, not on the new stairs"
        );
    }
}

#[test]
fn what_the_player_kept_still_works_on_the_next_floor() {
    let mut game = prepared(Seed::from(42));
    game.submit(Action::Descend);
    let hp = game.player_actor().hp();

    // Drink the draught: it is the very item carried from above.
    let draught = game
        .inventory()
        .find(|(_, item)| item.definition().id() == "healing_draught")
        .map(|(id, _)| id)
        .expect("the draught came down");
    let events = game.submit(Action::Use(draught));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Healed { .. })),
        "{events:?}"
    );
    assert!(game.player_actor().hp() > hp);

    // The learned sigil is cast on the new floor, and the ink steadies it.
    let content = tiers();
    let here = game.player_actor().position();
    let target = cells(game.map())
        .into_iter()
        .find(|&cell| {
            game.is_visible(cell)
                && game.map().is_walkable(cell)
                && (cell.x - here.x).abs().max((cell.y - here.y).abs()) >= 3
                && game.actors().all(|(_, a)| a.position() != cell)
        })
        .expect("a free cell in view on the new floor");
    game.spawn_monster(target, content.monster("mote_two").unwrap());
    let events = game.submit(Action::Cast {
        trace: pressed(&[7, 5, 2]),
        direction: None,
    });
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::CastSteadied { .. })),
        "the steadied cast carried over: {events:?}"
    );
    assert_eq!(game.steady_casts(), 1);
}

#[test]
fn floor_items_and_monsters_of_the_floor_left_do_not_follow() {
    let content = tiers();
    let mut game = Game::new(Seed::from(42), &content);
    let above: Vec<_> = game.actors().map(|(id, _)| id).collect();
    assert!(above.len() > 1);
    let player = game.player();

    descend(&mut game).expect("the stairs are reached");

    for id in above.into_iter().filter(|&id| id != player) {
        assert!(game.actor(id).is_none(), "{id:?} was left behind");
    }
    assert_eq!(
        game.actors().count() - 1,
        7,
        "the monsters of the new floor"
    );
    assert_eq!(game.inventory().count(), 0);
}

// --- Floor N does not depend on how the floors above were played --------------

/// Plays differently from `walk`: waits so the monsters come, fights them,
/// then walks to the stairs.
fn dawdle(game: &mut Game, waits: usize) {
    for _ in 0..waits {
        game.submit(Action::Wait);
    }
}

/// The state of the floor after each of `actions` actions of a fixed
/// script on the current floor: wait twice then try to walk one step towards
/// the stairs, and so on. The states, not the events, are compared: events
/// carry actor keys, which depend on the storage slots earlier floors freed.
fn script(game: &mut Game, actions: usize) -> Vec<FloorState> {
    let mut all = Vec::new();
    for turn in 0..actions {
        if turn % 3 == 2 {
            let here = game.player_actor().position();
            let stairs = stairs_of(game);
            match first_step(game.map(), here, stairs) {
                Some(direction) if here != stairs => game.submit(Action::Move(direction)),
                _ => game.submit(Action::Wait),
            };
        } else {
            game.submit(Action::Wait);
        }
        all.push(floor_state(game));
    }
    all
}

#[test]
fn floor_two_and_three_are_the_same_whatever_was_played_above() {
    let content = tiers();
    for seed in named_seeds() {
        let mut direct = Game::new(seed, &content);
        let mut detour = Game::new(seed, &content);
        dawdle(&mut detour, 40);
        assert_ne!(direct.ticks(), detour.ticks());

        descend(&mut direct).expect("direct reaches the stairs");
        descend(&mut detour).expect("detour reaches the stairs");
        assert_eq!(
            floor_state(&direct),
            floor_state(&detour),
            "seed {seed}, floor 2"
        );

        dawdle(&mut detour, 17);
        descend(&mut direct).expect("direct reaches the stairs");
        descend(&mut detour).expect("detour reaches the stairs");
        assert_eq!(
            floor_state(&direct),
            floor_state(&detour),
            "seed {seed}, floor 3"
        );
        assert_eq!(direct.depth(), 3);
        assert_eq!(
            script(&mut direct, 60),
            script(&mut detour, 60),
            "seed {seed}: the third floor plays out the same from its start"
        );
        assert_eq!(floor_state(&direct), floor_state(&detour));
    }
}

#[test]
fn on_the_shipped_content_floor_two_is_the_same_whatever_was_played_above() {
    let content = shipped();
    let mut compared = 0;
    for value in 0..24 {
        let seed = Seed::from(value);
        let mut direct = Game::new(seed, &content);
        let mut detour = Game::new(seed, &content);
        dawdle(&mut detour, 3);
        if descend(&mut direct).is_none() || descend(&mut detour).is_none() {
            continue;
        }
        compared += 1;
        assert_eq!(floor_state(&direct), floor_state(&detour), "seed {seed}");
        assert_eq!(
            script(&mut direct, 10),
            script(&mut detour, 10),
            "seed {seed}: monsters of floor 2 behave the same"
        );
    }
    assert!(compared >= 5, "only {compared} of 24 seeds compared");
}

/// A free floor cell next to the player of `game` and the direction of the
/// step that leads to it.
fn free_neighbour(game: &Game) -> (Direction, Position) {
    let here = game.player_actor().position();
    Direction::ALL
        .into_iter()
        .map(|direction| (direction, here.step(direction)))
        .find(|&(direction, cell)| {
            game.map().can_step(here, direction)
                && game.actors().all(|(_, actor)| actor.position() != cell)
        })
        .expect("a free cell next to the player")
}

/// Puts a biter next to the player of `game`, lets it strike during `waits`
/// turns and returns the damage of its blows, in order.
fn blows_of_a_biter(game: &mut Game, biter: &Content, waits: usize) -> Vec<i32> {
    let (_, cell) = free_neighbour(game);
    game.spawn_monster(cell, biter.monster("biter").expect("the biter"));
    let mut damages = Vec::new();
    for _ in 0..waits {
        for event in game.submit(Action::Wait) {
            if let Event::Attacked { damage, .. } = event {
                damages.push(damage);
            }
        }
    }
    damages
}

#[test]
fn the_blows_of_a_floor_do_not_depend_on_the_fights_of_the_floors_above() {
    let content = tiers();
    let biter = Content::parse_with_spawns(BITER, SIGILS, ITEMS, TIERS_FOR_BITER)
        .expect("valid helper content");
    for seed in named_seeds() {
        let mut direct = Game::new(seed, &content);
        let mut detour = Game::new(seed, &content);
        // The detour fights a biter on the first floor: its blows and the
        // player's draw from the gameplay stream.
        let (direction, cell) = free_neighbour(&detour);
        let foe = detour.spawn_monster(cell, biter.monster("biter").expect("the biter"));
        for _ in 0..40 {
            if detour.actor(foe).is_none() || detour.is_over() {
                break;
            }
            detour.submit(Action::Move(direction));
        }
        assert!(
            detour.actor(foe).is_none() && !detour.is_over(),
            "seed {seed}"
        );
        descend(&mut direct).expect("direct reaches the stairs");
        descend(&mut detour).expect("detour reaches the stairs");

        let straight = blows_of_a_biter(&mut direct, &biter, 4);
        let wandered = blows_of_a_biter(&mut detour, &biter, 4);

        assert!(
            straight.len() >= 3,
            "seed {seed}: the biter struck {straight:?}"
        );
        assert_eq!(
            straight, wandered,
            "seed {seed}: the same blows on the second floor"
        );
    }
}

// --- The deepest floors -------------------------------------------------------

/// Monsters per floor: 6 on the first, one more per floor, 12 at most.
fn expected_monsters(depth: u32) -> usize {
    usize::try_from((5 + depth).min(12)).unwrap()
}

#[test]
fn the_monsters_per_floor_grow_by_one_up_to_twelve_and_stay_there() {
    let content = tiers();
    for seed in named_seeds() {
        let mut game = Game::new(seed, &content);
        for depth in 1..=TOUR_DEPTH {
            if depth > 1 {
                descend(&mut game).expect("the stairs are reached");
            }
            assert_eq!(
                monster_count(&game),
                expected_monsters(depth),
                "seed {seed}, depth {depth}"
            );
        }
    }
}

#[test]
fn the_tenth_floor_uses_the_last_table() {
    let content = tiers();
    let mut game = Game::new(Seed::from(42), &content);
    for depth in 2..=TOUR_DEPTH {
        descend(&mut game).expect("the stairs are reached");
        let expected: &[&str] = match depth {
            2..=3 => &["mote_two"],
            4..=6 => &["mote_three"],
            _ => &["mote_four"],
        };
        assert_eq!(kinds(&game), expected, "depth {depth}");
    }
    assert_eq!(game.depth(), TOUR_DEPTH);
    assert!(!game.is_over());
}

#[test]
fn the_tenth_floor_is_the_bottom_its_seal_ends_the_run() {
    let content = tiers();
    let mut game = Game::new(Seed::from(0), &content);
    for _ in 1..TOUR_DEPTH {
        descend(&mut game).expect("the stairs are reached");
    }
    assert_eq!(game.depth(), TOUR_DEPTH);
    let map = game.map();
    assert!(
        cells(map)
            .into_iter()
            .all(|cell| map.tile(cell) != Some(Tile::StairsDown)),
        "the bottom floor has no stairs"
    );
    let seal = cells(map)
        .into_iter()
        .find(|&cell| map.tile(cell) == Some(Tile::Seal))
        .expect("the bottom floor has a seal");
    assert!(
        first_step(map, game.player_actor().position(), seal).is_some(),
        "the seal is reachable"
    );
    assert_eq!(monster_count(&game), 12);
    assert_eq!(kinds(&game), ["mote_four"]);
}

#[test]
fn very_deep_floors_generate_without_panic_and_obey_the_last_table() {
    let content = shipped();
    for depth in [10, 11, 25, 100, 1000, u32::MAX] {
        let floor = generation::generate_with_monsters(Seed::from(42), depth, &content);
        assert_eq!(floor.monsters().len(), 12, "depth {depth}");
        let allowed: Vec<_> = content.monster_weights(10).map(|(m, _)| m.id()).collect();
        for spawn in floor.monsters() {
            assert!(
                allowed.contains(&spawn.definition().id()),
                "depth {depth}: {} is not in the table of floor 10",
                spawn.definition().id()
            );
        }
        assert_eq!(
            floor.map().tile(floor.stairs()),
            Some(Tile::Seal),
            "depth {depth}: the bottom floor, and any generated beyond it, has the seal"
        );
    }
}

// --- Deterministic replay across several floors --------------------------------

/// Plays a fixed policy (walk to the stairs, descend, strike what blocks;
/// on the bottom floor, break the seal, which ends the run) for at most
/// `actions` actions and returns every event list, the depth and
/// the final state of the player.
fn scripted_run(
    seed: Seed,
    content: &Content,
    actions: usize,
) -> (Vec<Vec<Event>>, u32, Owned, u64) {
    let mut game = Game::new(seed, content);
    let mut all = Vec::new();
    for _ in 0..actions {
        if game.is_over() {
            break;
        }
        let here = game.player_actor().position();
        let stairs = stairs_of(&game);
        let events = if here == stairs {
            game.submit(Action::Descend)
        } else if let Some(direction) = first_step(game.map(), here, stairs) {
            game.submit(Action::Move(direction))
        } else {
            game.submit(Action::Wait)
        };
        all.push(events);
    }
    (all, game.depth(), owned(&game), game.ticks())
}

#[test]
fn the_same_seed_and_the_same_decisions_replay_identically_through_several_floors() {
    let content = tiers();
    for seed in named_seeds() {
        let first = scripted_run(seed, &content, 900);
        let second = scripted_run(seed, &content, 900);
        assert!(first.1 >= 5, "seed {seed}: reached only depth {}", first.1);
        assert_eq!(first, second, "seed {seed}");
    }
}

#[test]
fn a_replay_on_the_shipped_content_is_identical_even_when_the_player_dies() {
    let content = shipped();
    let mut deepest = 0;
    for value in 0..12 {
        let seed = Seed::from(value);
        let first = scripted_run(seed, &content, 700);
        let second = scripted_run(seed, &content, 700);
        deepest = deepest.max(first.1);
        assert_eq!(first, second, "seed {seed}");
    }
    assert!(deepest >= 2, "no seed went below the first floor");
}
