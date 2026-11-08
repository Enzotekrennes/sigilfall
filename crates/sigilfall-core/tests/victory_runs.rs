//! Acceptance tests of F14, "victory at the bottom", seen from the player's
//! side of the public API of the core (decision D23).
//!
//! The three scenarios of the plan are checked on whole runs, not on
//! hand-made states: every shipped bottom floor has a seal and no stairs,
//! far from the start and reachable by a search written here; a player that
//! walks down the real floors of a seed, saves on the way, reloads, and
//! breaks the seal wins and cannot act any more; breaking it from anywhere
//! else is refused and costs nothing, cell after cell of the walk. The
//! floors above the bottom keep their stairs and never hold a seal. A save
//! of the previous format is refused with both versions named.
//!
//! The walker uses harmless monsters (speed 1, one hit point, no attack), so
//! a run is never lost on the way. Distances come from a plain breadth-first
//! search over the eight moves, never from the generator's own helpers.
//! Every loop is bounded.

use std::collections::{HashMap, VecDeque};

use sigilfall_core::action::Action;
use sigilfall_core::content::Content;
use sigilfall_core::event::Event;
use sigilfall_core::game::{Game, Status};
use sigilfall_core::generation::generate_with_monsters;
use sigilfall_core::geometry::{Direction, Position};
use sigilfall_core::map::{Map, Tile};
use sigilfall_core::rng::{DailyDate, Seed};
use sigilfall_core::save::{self, FORMAT_VERSION, SaveError};

/// The bottom floor, written out here and not read from the game.
const BOTTOM: u32 = 10;

/// Seeds the bottom floors are generated for.
const FLOOR_SEEDS: u64 = 300;

/// Seeds whose floors above the bottom are checked.
const UPPER_SEEDS: u64 = 120;

/// Seeds of the played runs.
const RUN_SEEDS: [u64; 5] = [1, 42, 9940, 31_337, u64::MAX];

/// Fewest steps the seal may lie from the start, on any seed: "far from the
/// start" (a floor is 80 × 40 cells, and the stairs of every floor lie on
/// the farthest cell, not merely a far one).
const MIN_STEPS_TO_THE_SEAL: u32 = 40;

/// Most actions spent walking to the way down of one floor.
const WALK_BUDGET: usize = 1500;

/// One kind of monster that never gets to hurt: at speed 1 it gains a
/// hundredth of the energy it needs to act, and one blow kills it.
const HARMLESS: &str = r#"[
    Monster(id: "dust", name: "dust", family: Vermin, elite: false,
        speed: 1, hp: 1, attack: 0, defense: 0, min_depth: 1),
]"#;

/// The shipped content.
fn shipped() -> Content {
    Content::load().expect("the shipped content is valid")
}

/// The shipped sigils, items and floors with harmless monsters.
fn harmless() -> Content {
    Content::parse(HARMLESS).expect("valid test content")
}

/// Every cell of `map`, in reading order.
fn cells(map: &Map) -> Vec<Position> {
    let width = i32::try_from(map.width()).unwrap();
    let height = i32::try_from(map.height()).unwrap();
    (0..height)
        .flat_map(|y| (0..width).map(move |x| Position::new(x, y)))
        .collect()
}

/// How many cells of `map` hold `tile`.
fn count(map: &Map, tile: Tile) -> usize {
    cells(map)
        .into_iter()
        .filter(|&cell| map.tile(cell) == Some(tile))
        .count()
}

/// The first cell of `map` holding `tile`.
fn find(map: &Map, tile: Tile) -> Option<Position> {
    cells(map)
        .into_iter()
        .find(|&cell| map.tile(cell) == Some(tile))
}

/// Steps from `from` to every walkable cell it reaches, by breadth-first
/// search over the eight moves.
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

/// The way down of the current floor of `game`: its stairs, or its seal.
fn way_down(game: &Game) -> Position {
    let map = game.map();
    cells(map)
        .into_iter()
        .find(|&cell| matches!(map.tile(cell), Some(Tile::StairsDown | Tile::Seal)))
        .expect("every floor has stairs or a seal")
}

/// The first step of a shortest walk from `from` to `to`.
fn first_step(map: &Map, from: Position, to: Position) -> Option<Direction> {
    let back = steps_from(map, to);
    let here = *back.get(&from)?;
    Direction::ALL.into_iter().find(|&direction| {
        let next = from.step(direction);
        map.is_walkable(next) && back.get(&next).is_some_and(|&steps| steps + 1 == here)
    })
}

/// Plays one step towards the way down of the floor of `game`; `false` once
/// the player stands on it.
fn step_towards_the_way_down(game: &mut Game) -> bool {
    let here = game.player_actor().position();
    let goal = way_down(game);
    if here == goal {
        return false;
    }
    let direction = first_step(game.map(), here, goal).expect("the way down is reachable");
    game.submit(Action::Move(direction));
    true
}

/// Walks the player of `game` onto the way down of its floor.
fn walk_down(game: &mut Game) {
    for _ in 0..WALK_BUDGET {
        assert!(!game.is_over(), "the walker is still playing");
        if !step_towards_the_way_down(game) {
            return;
        }
    }
    panic!("the way down was not reached in {WALK_BUDGET} actions");
}

/// Plays `game` down the stairs of every floor to the bottom one, ending on
/// its start cell.
fn to_the_bottom(game: &mut Game) {
    while game.depth() < BOTTOM {
        walk_down(game);
        let below = game.depth() + 1;
        assert_eq!(
            game.submit(Action::Descend),
            [Event::Descended { depth: below }]
        );
    }
}

/// What a refused action must leave alone: the clock, the turns, the energy
/// of every actor, and the whole saved state.
fn state_of(game: &Game) -> (u64, u64, Vec<i32>, String) {
    let energies = game.actors().map(|(_, actor)| actor.energy()).collect();
    let text = save::to_string(game).expect("a game is always written");
    (game.ticks(), game.player_turns(), energies, text)
}

// --- Scenario 1: the generated bottom floor -------------------------------------------

#[test]
fn the_tenth_floor_of_every_seed_has_a_far_reachable_seal_and_no_stairs() {
    let content = shipped();
    let mut nearest = u32::MAX;
    for value in 0..FLOOR_SEEDS {
        let floor = generate_with_monsters(Seed::new(value), BOTTOM, &content);
        let map = floor.map();
        let context = format!("seed {value}");
        assert_eq!(count(map, Tile::StairsDown), 0, "{context}: stairs");
        assert_eq!(count(map, Tile::Seal), 1, "{context}: seals");
        let seal = find(map, Tile::Seal).unwrap();
        let steps = steps_from(map, floor.start());
        let to_seal = *steps
            .get(&seal)
            .unwrap_or_else(|| panic!("{context}: the seal is out of reach"));
        let farthest = steps.values().copied().max().unwrap();
        assert_eq!(to_seal, farthest, "{context}: not the farthest cell");
        assert_ne!(seal, floor.start(), "{context}");
        assert!(map.is_walkable(seal), "{context}: the seal can be stood on");
        nearest = nearest.min(to_seal);
    }
    assert!(
        nearest >= MIN_STEPS_TO_THE_SEAL,
        "the nearest seal is {nearest} steps from the start"
    );
}

#[test]
fn floors_one_to_nine_keep_one_staircase_and_never_hold_a_seal() {
    let content = shipped();
    for value in 0..UPPER_SEEDS {
        for depth in 1..BOTTOM {
            let floor = generate_with_monsters(Seed::new(value), depth, &content);
            let context = format!("seed {value}, depth {depth}");
            assert_eq!(count(floor.map(), Tile::Seal), 0, "{context}");
            assert_eq!(count(floor.map(), Tile::StairsDown), 1, "{context}");
        }
    }
}

#[test]
fn a_run_reaches_the_tenth_floor_by_the_stairs_and_finds_the_seal_there() {
    let content = harmless();
    for value in RUN_SEEDS {
        let mut game = Game::new(Seed::new(value), &content);
        for depth in 1..BOTTOM {
            assert_eq!(game.depth(), depth);
            assert_eq!(
                count(game.map(), Tile::Seal),
                0,
                "seed {value}, floor {depth}"
            );
            walk_down(&mut game);
            assert_eq!(
                game.map().tile(game.player_actor().position()),
                Some(Tile::StairsDown)
            );
            assert_eq!(
                game.submit(Action::Descend),
                [Event::Descended { depth: depth + 1 }],
                "seed {value}"
            );
        }
        assert_eq!(game.depth(), BOTTOM);
        assert_eq!(count(game.map(), Tile::StairsDown), 0, "seed {value}");
        let seal = find(game.map(), Tile::Seal).expect("a seal on the bottom floor");
        let start = game.player_actor().position();
        assert!(steps_from(game.map(), start)[&seal] >= MIN_STEPS_TO_THE_SEAL);
        // The floor the player stands on is the floor of the seed.
        let floor = generate_with_monsters(Seed::new(value), BOTTOM, &content);
        let tiles = |map: &Map| -> Vec<Option<Tile>> {
            cells(map).into_iter().map(|cell| map.tile(cell)).collect()
        };
        assert_eq!(tiles(floor.map()), tiles(game.map()), "seed {value}");
    }
}

// --- Scenario 2: breaking the seal wins the run --------------------------------------------

#[test]
fn breaking_the_seal_ends_the_run_in_victory_and_nothing_more_can_be_done() {
    let content = harmless();
    for value in RUN_SEEDS {
        let mut game = Game::new(Seed::new(value), &content);
        to_the_bottom(&mut game);
        walk_down(&mut game);
        let seal = game.player_actor().position();
        assert_eq!(game.map().tile(seal), Some(Tile::Seal));
        assert_eq!(game.status(), &Status::Playing);
        let turns = game.stats().turns();

        let events = game.submit(Action::Descend);

        assert_eq!(
            events,
            [Event::SealBroken { position: seal }],
            "seed {value}"
        );
        assert_eq!(game.status(), &Status::Won, "seed {value}");
        assert!(game.is_over());
        assert!(game.player_actor().is_alive(), "a victory is not a death");
        assert_eq!(game.depth(), BOTTOM, "nothing lies below");
        assert_eq!(game.stats().deepest(), BOTTOM);
        assert_eq!(game.stats().turns(), turns + 1, "breaking it is a turn");
        let finished = state_of(&game);
        for action in [
            Action::Descend,
            Action::Wait,
            Action::Explore,
            Action::PickUp,
            Action::Move(Direction::North),
            Action::Move(Direction::South),
        ] {
            assert!(game.submit(action).is_empty(), "seed {value}: {action:?}");
        }
        assert_eq!(state_of(&game), finished, "seed {value}: nothing changed");
    }
}

#[test]
fn the_daily_dungeon_can_be_won_and_the_victory_keeps_its_date() {
    let content = harmless();
    let date = DailyDate::new(2026, 11, 8).expect("a real date");
    let mut game = Game::daily(date, &content);
    to_the_bottom(&mut game);
    walk_down(&mut game);

    let events = game.submit(Action::Descend);

    assert!(matches!(events.as_slice(), [Event::SealBroken { .. }]));
    assert_eq!(game.status(), &Status::Won);
    assert_eq!(game.daily_date(), Some(date));
    let text = save::to_string(&game).expect("written");
    let reloaded = save::from_str(&text, &content).expect("a won daily game reads back");
    assert_eq!(reloaded.status(), &Status::Won);
    assert_eq!(reloaded.daily_date(), Some(date));
    assert_eq!(reloaded.seed(), Seed::daily(date));
}

#[test]
fn a_run_saved_and_reloaded_on_the_way_to_the_seal_is_still_won_the_same_way() {
    let content = harmless();
    for value in RUN_SEEDS {
        let mut game = Game::new(Seed::new(value), &content);
        to_the_bottom(&mut game);
        // Saved one step short of the seal, the last moment before the win.
        while steps_from(game.map(), game.player_actor().position())[&way_down(&game)] > 1 {
            assert!(step_towards_the_way_down(&mut game));
        }
        let text = save::to_string(&game).expect("written");
        let mut twin = save::from_str(&text, &content).expect("the save reads back");
        assert_eq!(state_of(&twin), state_of(&game), "seed {value}");
        assert_eq!(twin.depth(), BOTTOM);
        assert_eq!(find(twin.map(), Tile::Seal), find(game.map(), Tile::Seal));

        walk_down(&mut game);
        walk_down(&mut twin);
        let original = game.submit(Action::Descend);
        let reloaded = twin.submit(Action::Descend);

        assert_eq!(original, reloaded, "seed {value}");
        assert!(matches!(original.as_slice(), [Event::SealBroken { .. }]));
        assert_eq!(twin.status(), &Status::Won);
        assert_eq!(state_of(&twin), state_of(&game), "seed {value}");
    }
}

#[test]
fn a_won_game_that_is_saved_is_read_back_won_and_stays_inert() {
    let content = harmless();
    let mut game = Game::new(Seed::new(42), &content);
    to_the_bottom(&mut game);
    walk_down(&mut game);
    game.submit(Action::Descend);
    let text = save::to_string(&game).expect("written");
    assert!(text.contains(&format!("format_version: {FORMAT_VERSION}")));

    let mut loaded = save::from_str(&text, &content).expect("a won game reads back");

    assert_eq!(loaded.status(), &Status::Won);
    assert!(loaded.is_over());
    assert!(loaded.submit(Action::Wait).is_empty());
    assert!(loaded.submit(Action::Descend).is_empty());
    assert_eq!(save::to_string(&loaded).unwrap(), text);
}

// --- Scenario 3: breaking it elsewhere does nothing ---------------------------------------------

#[test]
fn trying_to_break_the_seal_from_any_cell_of_the_walk_but_the_seal_does_nothing() {
    let content = harmless();
    for value in RUN_SEEDS {
        let mut game = Game::new(Seed::new(value), &content);
        to_the_bottom(&mut game);
        let mut refusals = 0;
        for _ in 0..WALK_BUDGET {
            let here = game.player_actor().position();
            if game.map().tile(here) == Some(Tile::Seal) {
                break;
            }
            let before = state_of(&game);

            let events = game.submit(Action::Descend);

            assert_eq!(
                events,
                [Event::NoStairsHere { position: here }],
                "seed {value}"
            );
            assert_eq!(
                state_of(&game),
                before,
                "seed {value}: the refusal cost something"
            );
            assert_eq!(game.status(), &Status::Playing);
            refusals += 1;
            step_towards_the_way_down(&mut game);
        }
        assert!(
            refusals >= MIN_STEPS_TO_THE_SEAL,
            "seed {value}: only {refusals} cells tried"
        );
        assert_eq!(
            game.map().tile(game.player_actor().position()),
            Some(Tile::Seal)
        );
    }
}

#[test]
fn a_refusal_off_the_seal_and_off_the_stairs_is_the_same_event_on_every_floor() {
    let content = harmless();
    let mut game = Game::new(Seed::new(7), &content);
    for depth in 1..=BOTTOM {
        let here = game.player_actor().position();
        assert_eq!(
            game.submit(Action::Descend),
            [Event::NoStairsHere { position: here }],
            "floor {depth}"
        );
        assert_eq!(game.depth(), depth);
        if depth < BOTTOM {
            walk_down(&mut game);
            game.submit(Action::Descend);
        }
    }
}

// --- Saves ------------------------------------------------------------------------------------------

#[test]
fn a_save_of_the_previous_format_is_refused_and_both_versions_are_named() {
    let content = shipped();
    let mut game = Game::new(Seed::new(42), &content);
    for _ in 0..5 {
        game.submit(Action::Wait);
    }
    let current = save::to_string(&game).expect("written");
    assert_eq!(
        FORMAT_VERSION, 4,
        "the seal and the won status made format 4"
    );
    for found in [1_u32, 2, 3, 5, 99] {
        let old = current.replacen(
            &format!("format_version: {FORMAT_VERSION}"),
            &format!("format_version: {found}"),
            1,
        );
        assert_ne!(old, current);

        let error = save::from_str(&old, &content).expect_err("another version is refused");

        assert!(
            matches!(
                error,
                SaveError::UnsupportedVersion { found: f, expected } if f == found && expected == FORMAT_VERSION
            ),
            "{error:?}"
        );
        let words = error.to_string();
        assert!(words.contains(&found.to_string()), "{words}");
        assert!(words.contains(&FORMAT_VERSION.to_string()), "{words}");
    }
    save::from_str(&current, &content).expect("the current format reads");
}
