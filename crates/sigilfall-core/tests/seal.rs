//! Acceptance tests of F14, "victory at the bottom", core half (task T33,
//! decision D23): the bottom floor has no stairs but a seal, reachable and
//! on the cell farthest from the start, whether the floor is laid out in
//! rooms or as a cave; breaking the seal wins the run and ends the game;
//! trying to break it elsewhere is refused at no cost; a game on the bottom
//! floor is saved and read back. The floors above keep their stairs and are
//! checked cell for cell by `frozen_floors.rs`.
//!
//! Written against the public API only. Distances are recomputed here with
//! a plain breadth-first search over the eight moves of the game, never with
//! the generator's own helpers, so a bug shared by the generator and its
//! checks cannot hide. The player cannot be placed from outside the crate,
//! so the end-to-end tests walk it to the stairs, then to the seal, among
//! harmless monsters (speed 1, 1 hit point, no attack). Every loop is
//! bounded.

use std::collections::{HashMap, VecDeque};

use sigilfall_core::action::Action;
use sigilfall_core::content::{Content, FloorKind};
use sigilfall_core::event::Event;
use sigilfall_core::game::{Game, Status};
use sigilfall_core::generation::{
    Floor, floor_kind, generate_of_kind, generate_with_monsters, is_bottom_floor,
};
use sigilfall_core::geometry::{Direction, Position};
use sigilfall_core::map::{Map, Tile};
use sigilfall_core::rng::Seed;
use sigilfall_core::save;

/// The bottom floor of the dungeon, written out here rather than read from
/// the production constant (`content::MAX_DEPTH`).
const BOTTOM: u32 = 10;

/// Seeds of each kind of layout the bottom floor is checked on.
const LAYOUT_SEEDS: u64 = 250;

/// Seeds the bottom floor of the shipped content is checked on, with its
/// monsters, glyphs and items.
const SHIPPED_SEEDS: u64 = 200;

/// Seeds of the end-to-end runs: both ends of the range and two others.
const RUN_SEEDS: [u64; 4] = [0, 42, 9940, u64::MAX];

/// Most actions spent walking to the way down of one floor.
const WALK_BUDGET: usize = 1500;

/// A single kind of monster that never gets to hurt: at speed 1 it gains a
/// hundredth of the energy it needs to act, and one blow kills it.
const HARMLESS: &str = r#"[
    Monster(id: "mote", name: "mote", family: Vermin, elite: false,
        speed: 1, hp: 1, attack: 0, defense: 0, min_depth: 1),
]"#;

/// Every cell of `map`, in reading order.
fn cells(map: &Map) -> Vec<Position> {
    let width = i32::try_from(map.width()).unwrap();
    let height = i32::try_from(map.height()).unwrap();
    (0..height)
        .flat_map(|y| (0..width).map(move |x| Position::new(x, y)))
        .collect()
}

/// The cells of `map` holding `tile`, in reading order.
fn cells_of(map: &Map, tile: Tile) -> Vec<Position> {
    cells(map)
        .into_iter()
        .filter(|&cell| map.tile(cell) == Some(tile))
        .collect()
}

/// Steps from `from` to every walkable cell it can reach, by breadth-first
/// search over the eight moves onto walkable cells.
fn steps_from(map: &Map, from: Position) -> HashMap<Position, u32> {
    let mut steps = HashMap::from([(from, 0)]);
    let mut queue = VecDeque::from([from]);
    while let Some(cell) = queue.pop_front() {
        let here = steps[&cell];
        for direction in Direction::ALL {
            let next = cell.step(direction);
            if map.is_walkable(next) && !steps.contains_key(&next) {
                steps.insert(next, here + 1);
                queue.push_back(next);
            }
        }
    }
    steps
}

/// Checks that `floor` is a bottom floor: no stairs, a single seal on the
/// cell `Floor::stairs` names, reachable from the start, and no walkable
/// cell farther from the start than it. `context` names the floor.
fn check_bottom_floor(floor: &Floor, context: &str) {
    let map = floor.map();
    assert_eq!(cells_of(map, Tile::StairsDown), [], "{context}: stairs");
    let seal = floor.stairs();
    assert_eq!(cells_of(map, Tile::Seal), [seal], "{context}: one seal");
    let steps = steps_from(map, floor.start());
    let farthest = steps.values().copied().max().unwrap();
    assert_eq!(
        steps.get(&seal),
        Some(&farthest),
        "{context}: seal not farthest"
    );
    assert!(farthest > 0, "{context}: the seal is not the start");
}

// --- The bottom floor -----------------------------------------------------------

#[test]
fn the_bottom_floor_is_the_tenth() {
    assert!(is_bottom_floor(BOTTOM));
    assert!((1..BOTTOM).all(|depth| !is_bottom_floor(depth)));
}

#[test]
fn the_bottom_floor_has_a_reachable_seal_farthest_from_the_start_in_rooms_and_caves() {
    for value in 0..LAYOUT_SEEDS {
        for kind in [FloorKind::Rooms, FloorKind::Caves] {
            let floor = generate_of_kind(Seed::new(value), BOTTOM, kind);
            check_bottom_floor(&floor, &format!("seed {value}, {kind:?}"));
        }
    }
}

#[test]
fn the_shipped_bottom_floor_keeps_its_seal_free_of_monsters_glyphs_and_items() {
    let content = Content::load().expect("the shipped content is valid");
    for value in 0..SHIPPED_SEEDS {
        let seed = Seed::new(value);
        let floor = generate_with_monsters(seed, BOTTOM, &content);
        let context = format!("seed {value}");
        check_bottom_floor(&floor, &context);
        let bare = generate_of_kind(seed, BOTTOM, floor_kind(seed, BOTTOM, &content));
        assert_eq!(floor.map(), bare.map(), "{context}");
        let seal = floor.stairs();
        assert!(floor.monsters().iter().all(|m| m.position() != seal));
        assert!(floor.glyphs().iter().all(|g| g.position() != seal));
        assert!(floor.items().iter().all(|i| i.position() != seal));
    }
}

#[test]
fn no_floor_above_the_bottom_has_a_seal() {
    for value in 0..SHIPPED_SEEDS {
        for depth in 1..BOTTOM {
            for kind in [FloorKind::Rooms, FloorKind::Caves] {
                let floor = generate_of_kind(Seed::new(value), depth, kind);
                assert_eq!(cells_of(floor.map(), Tile::Seal), [], "seed {value}");
                assert_eq!(
                    floor.map().tile(floor.stairs()),
                    Some(Tile::StairsDown),
                    "seed {value}, floor {depth}, {kind:?}"
                );
            }
        }
    }
}

// --- Playing to the seal ----------------------------------------------------------

/// Harmless monsters on the shipped sigils, items and floor kinds.
fn harmless() -> Content {
    Content::parse(HARMLESS).expect("valid test content")
}

/// The way down of the current floor of `game`: its stairs, or its seal.
fn way_down(game: &Game) -> Position {
    let map = game.map();
    cells(map)
        .into_iter()
        .find(|&cell| matches!(map.tile(cell), Some(Tile::StairsDown | Tile::Seal)))
        .expect("every floor has stairs or a seal")
}

/// The first step of a shortest walk from `from` to `to`, ignoring
/// creatures; `None` when `to` cannot be reached.
fn first_step(map: &Map, from: Position, to: Position) -> Option<Direction> {
    let back = steps_from(map, to);
    let here = *back.get(&from)?;
    Direction::ALL.into_iter().find(|&direction| {
        let next = from.step(direction);
        map.is_walkable(next) && back.get(&next).is_some_and(|&steps| steps + 1 == here)
    })
}

/// Walks the player of `game` to the way down of its floor, striking what
/// stands in the way, and returns every event list on the way. Panics if
/// it is not there, alive, within [`WALK_BUDGET`] actions.
fn walk_down(game: &mut Game) -> Vec<Vec<Event>> {
    let mut walked = Vec::new();
    for _ in 0..WALK_BUDGET {
        assert!(!game.is_over(), "the walker died");
        let here = game.player_actor().position();
        let goal = way_down(game);
        if here == goal {
            return walked;
        }
        let direction = first_step(game.map(), here, goal).expect("the way down is reachable");
        walked.push(game.submit(Action::Move(direction)));
    }
    panic!("the way down was not reached in {WALK_BUDGET} actions");
}

/// A game of `seed` played down to the bottom floor, its player on the
/// start cell there.
fn on_the_bottom_floor(seed: Seed, content: &Content) -> Game {
    let mut game = Game::new(seed, content);
    while game.depth() < BOTTOM {
        walk_down(&mut game);
        let below = game.depth() + 1;
        assert_eq!(
            game.submit(Action::Descend),
            [Event::Descended { depth: below }]
        );
    }
    game
}

/// The clock and the energy of every actor, to check that nothing was paid.
fn clock(game: &Game) -> (u64, u64, Vec<i32>) {
    let energies = game.actors().map(|(_, actor)| actor.energy()).collect();
    (game.ticks(), game.player_turns(), energies)
}

#[test]
fn breaking_the_seal_wins_the_run_and_no_action_is_taken_afterwards() {
    let content = harmless();
    for value in RUN_SEEDS {
        let mut game = on_the_bottom_floor(Seed::new(value), &content);
        walk_down(&mut game);
        let position = game.player_actor().position();
        assert_eq!(game.map().tile(position), Some(Tile::Seal));
        let turns = game.player_turns();

        let events = game.submit(Action::Descend);

        assert_eq!(events, [Event::SealBroken { position }], "seed {value}");
        assert_eq!(game.status(), &Status::Won);
        assert!(game.is_over());
        assert_eq!(game.depth(), BOTTOM);
        assert_eq!(game.player_turns(), turns + 1);
        assert_eq!(game.stats().deepest(), BOTTOM);
        let finished = clock(&game);
        for action in [
            Action::Descend,
            Action::Wait,
            Action::Explore,
            Action::Move(Direction::North),
        ] {
            assert!(game.submit(action).is_empty(), "seed {value}: {action:?}");
        }
        assert_eq!(clock(&game), finished, "seed {value}: nothing moved");
        assert_eq!(game.player_actor().position(), position);
    }
}

#[test]
fn breaking_the_seal_away_from_it_is_refused_without_time_passing() {
    let content = harmless();
    for value in RUN_SEEDS {
        let mut game = on_the_bottom_floor(Seed::new(value), &content);
        let position = game.player_actor().position();
        assert_ne!(game.map().tile(position), Some(Tile::Seal));
        let before = clock(&game);

        let events = game.submit(Action::Descend);

        assert_eq!(events, [Event::NoStairsHere { position }], "seed {value}");
        assert_eq!(clock(&game), before, "seed {value}: nothing was paid");
        assert_eq!(game.status(), &Status::Playing);
        assert_eq!(game.player_actor().position(), position);
    }
}

#[test]
fn a_game_saved_on_the_bottom_floor_plays_on_to_the_same_victory() {
    let content = harmless();
    for value in RUN_SEEDS {
        let mut game = on_the_bottom_floor(Seed::new(value), &content);
        let text = save::to_string(&game).expect("a game is always written");

        let mut loaded = save::from_str(&text, &content).expect("the save reads back");

        assert_eq!(loaded.depth(), BOTTOM);
        assert_eq!(way_down(&loaded), way_down(&game));
        assert_eq!(walk_down(&mut loaded), walk_down(&mut game), "seed {value}");
        assert_eq!(
            loaded.submit(Action::Descend),
            game.submit(Action::Descend),
            "seed {value}"
        );
        assert_eq!(loaded.status(), &Status::Won);
        let won = save::to_string(&game).expect("a game is always written");
        assert_eq!(save::to_string(&loaded).expect("written"), won);
        let reloaded = save::from_str(&won, &content).expect("a won game reads back");
        assert_eq!(reloaded.status(), &Status::Won);
    }
}
