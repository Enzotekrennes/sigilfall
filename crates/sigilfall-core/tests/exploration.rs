//! Automatic exploration, core half: what `Action::Explore` does to the
//! game.
//!
//! Written against the public API of the core crate only. The nearest
//! unexplored place, the cells the player knows and the distances to the
//! frontier are recomputed here with a plain breadth-first search from the
//! rules of exploration, not with `sigilfall_core::explore`, so a bug shared
//! by the code and its own tests cannot hide.
//!
//! What the tests call the *known* floor is the set of cells the player has
//! seen (`Game::is_remembered`). The *frontier* is the set of cells never
//! seen, inside the map, that touch a known walkable cell. A step of
//! exploration must land on a known cell that is exactly one step closer,
//! through known cells only, to the nearest frontier cell: that is "walk to
//! the nearest unexplored place" and "never through an unknown wall" at the
//! same time. The screens and the stopping rules are checked by the binary's
//! tests (`crates/sigilfall/src/app/tests/exploration.rs`).

use std::collections::{HashMap, HashSet, VecDeque};

use sigilfall_core::action::Action;
use sigilfall_core::content::{Content, FloorKind};
use sigilfall_core::event::Event;
use sigilfall_core::game::Game;
use sigilfall_core::generation::{generate, generate_of_kind, generate_with_monsters};
use sigilfall_core::geometry::Position;
use sigilfall_core::map::Tile;
use sigilfall_core::rng::Seed;
use sigilfall_core::save;

/// Most steps a whole bare floor may take to explore: far above what any
/// floor needs (80 x 40 cells), so a loop in the exploration is caught.
const STEP_LIMIT: usize = 6000;

fn content() -> Content {
    Content::load().expect("the shipped content is valid")
}

/// A seed spread over the whole range from a small number.
fn seed_of(n: u64) -> Seed {
    Seed::new(n.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xA5A5)
}

/// A game alone on a bare floor of rooms.
fn rooms_game(n: u64) -> Game {
    Game::on_floor(seed_of(n), generate(seed_of(n), 1))
}

/// A game alone on a bare cave.
fn cave_game(n: u64) -> Game {
    Game::on_floor(
        seed_of(n),
        generate_of_kind(seed_of(n), 5, FloorKind::Caves),
    )
}

/// The cells of the map in reading order.
fn cells(game: &Game) -> Vec<Position> {
    let map = game.map();
    let (w, h) = (
        i32::try_from(map.width()).unwrap(),
        i32::try_from(map.height()).unwrap(),
    );
    (0..h)
        .flat_map(|y| (0..w).map(move |x| Position::new(x, y)))
        .collect()
}

/// Whether `p` is walkable ground.
fn walkable(game: &Game, p: Position) -> bool {
    matches!(game.map().tile(p), Some(Tile::Floor | Tile::StairsDown))
}

/// Whether `p` is a known walkable cell.
fn known_walkable(game: &Game, p: Position) -> bool {
    game.is_remembered(p) && walkable(game, p)
}

/// Whether `p` is inside the map.
fn inside(game: &Game, p: Position) -> bool {
    game.map().tile(p).is_some()
}

/// The eight neighbours of `p`.
fn neighbours(p: Position) -> impl Iterator<Item = Position> {
    (-1..=1)
        .flat_map(|dx| (-1..=1).map(move |dy| (dx, dy)))
        .filter(|&(dx, dy)| (dx, dy) != (0, 0))
        .map(move |(dx, dy)| Position::new(p.x + dx, p.y + dy))
}

/// Whether `p` is a frontier cell: inside the map, never seen, and next to a
/// known walkable cell.
fn is_frontier(game: &Game, p: Position) -> bool {
    inside(game, p) && !game.is_remembered(p) && neighbours(p).any(|n| known_walkable(game, n))
}

/// Steps from every known walkable cell to the nearest frontier cell,
/// walking through known walkable cells only and entering the frontier as
/// the last step. Cells that cannot reach any are absent.
fn distance_to_unexplored(game: &Game) -> HashMap<Position, u32> {
    let mut best: HashMap<Position, u32> = HashMap::new();
    let mut queue = VecDeque::new();
    // Distance 1: known walkable cells next to a frontier cell.
    for p in cells(game) {
        if known_walkable(game, p) && neighbours(p).any(|n| is_frontier(game, n)) {
            best.insert(p, 1);
            queue.push_back(p);
        }
    }
    while let Some(here) = queue.pop_front() {
        let d = best[&here];
        for n in neighbours(here) {
            if known_walkable(game, n) && !best.contains_key(&n) {
                best.insert(n, d + 1);
                queue.push_back(n);
            }
        }
    }
    best
}

/// Looks `p` up in a table built by [`distance_to_unexplored`].
fn lookup(table: &HashMap<Position, u32>, p: Position) -> Option<u32> {
    table.get(&p).copied()
}

/// Chebyshev distance.
fn king_distance(a: Position, b: Position) -> i32 {
    (a.x - b.x).abs().max((a.y - b.y).abs())
}

/// Explores `game` to the end, checking every step against the reference
/// distances. Returns the number of steps taken.
fn explore_checked(game: &mut Game, label: &str) -> usize {
    let player = game.player();
    for step in 0..STEP_LIMIT {
        let here = game.player_actor().position();
        let table = distance_to_unexplored(game);
        let nearest = lookup(&table, here);
        let ticks = game.ticks();
        let turns = game.player_turns();
        let events = game.submit(Action::Explore);
        match nearest {
            None => {
                // Nothing reachable is unexplored: a refusal, and no time.
                assert_eq!(
                    events,
                    [Event::NothingToExplore { position: here }],
                    "{label}: step {step}"
                );
                assert_eq!(game.ticks(), ticks, "{label}: time passed on a refusal");
                assert_eq!(game.player_turns(), turns, "{label}");
                assert_eq!(game.player_actor().position(), here, "{label}");
                return step;
            }
            Some(d) => {
                let moved: Vec<&Event> = events
                    .iter()
                    .filter(|e| matches!(e, Event::Moved { actor, .. } if *actor == player))
                    .collect();
                assert_eq!(moved.len(), 1, "{label}: step {step}: {events:?}");
                let there = game.player_actor().position();
                assert_eq!(king_distance(here, there), 1, "{label}: step {step}");
                assert_eq!(game.player_turns(), turns + 1, "{label}: one turn per step");
                assert!(game.ticks() > ticks, "{label}: a step takes time");
                // The cell stepped on was known before the step, and was
                // one step closer to the nearest unexplored cell.
                // Entering the frontier itself is the last step: distance 0.
                let closer = lookup(&table, there).unwrap_or(0);
                assert!(
                    closer + 1 == d,
                    "{label}: step {step} from {here:?} (distance {d}) to {there:?} \
                     (distance {closer}): not one step closer through known cells"
                );
            }
        }
    }
    panic!("{label}: still exploring after {STEP_LIMIT} steps");
}

/// Every walkable cell reachable from `start` over the real floor.
fn all_reachable(game: &Game, start: Position) -> Vec<Position> {
    let mut seen = HashSet::from([start]);
    let mut queue = VecDeque::from([start]);
    while let Some(here) = queue.pop_front() {
        for n in neighbours(here) {
            if walkable(game, n) && seen.insert(n) {
                queue.push_back(n);
            }
        }
    }
    seen.into_iter().collect()
}

// --- Toward the nearest unexplored place ------------------------

#[test]
fn each_step_goes_one_closer_to_the_nearest_unexplored_cell_through_known_cells() {
    let mut steps = Vec::new();
    for n in 0..4 {
        let mut game = rooms_game(n);
        steps.push(explore_checked(&mut game, &format!("rooms {n}")));
    }
    for n in 0..4 {
        let mut game = cave_game(n);
        steps.push(explore_checked(&mut game, &format!("cave {n}")));
    }
    println!("steps to explore 4 floors of rooms then 4 caves: {steps:?}");
    assert!(steps.iter().all(|&s| s > 20), "{steps:?}");
}

#[test]
fn the_floor_is_known_as_far_as_the_player_can_walk_when_exploration_ends() {
    for (label, mut game) in [
        ("rooms", rooms_game(11)),
        ("rooms", rooms_game(12)),
        ("cave", cave_game(11)),
        ("cave", cave_game(12)),
    ] {
        let start = game.player_actor().position();
        explore_checked(&mut game, label);
        let unknown: Vec<Position> = all_reachable(&game, start)
            .into_iter()
            .filter(|&p| !game.is_remembered(p))
            .collect();
        assert!(
            unknown.is_empty(),
            "{label}: walkable cells never seen: {unknown:?}"
        );
        // No frontier is left.
        assert!(
            cells(&game).into_iter().all(|p| !is_frontier(&game, p)),
            "{label}: a frontier cell is left"
        );
    }
}

/// On 40 seeds of each kind, exploring takes a bounded, reasonable number of
/// steps: the walk never loops.
#[test]
fn exploration_ends_in_a_bounded_number_of_steps_on_40_seeds_of_each_kind() {
    let mut worst = 0;
    for n in 100..140 {
        for (label, mut game) in [("rooms", rooms_game(n)), ("cave", cave_game(n))] {
            let mut count = 0;
            loop {
                let events = game.submit(Action::Explore);
                if events
                    .iter()
                    .any(|e| matches!(e, Event::NothingToExplore { .. }))
                {
                    break;
                }
                count += 1;
                assert!(count < STEP_LIMIT, "{label} {n}: still exploring");
            }
            worst = worst.max(count);
        }
    }
    println!("most steps to explore a bare floor over 80 floors: {worst}");
}

// --- Nothing left, nothing happens ------------------------------

#[test]
fn an_explored_floor_is_refused_and_no_time_passes() {
    let mut game = rooms_game(7);
    explore_checked(&mut game, "rooms 7");
    let before = save::to_string(&game).unwrap();
    let here = game.player_actor().position();
    let (ticks, turns) = (game.ticks(), game.player_turns());
    let (hp, energy) = (game.player_actor().hp(), game.player_actor().energy());

    for _ in 0..5 {
        let events = game.submit(Action::Explore);
        assert_eq!(events, [Event::NothingToExplore { position: here }]);
    }

    assert_eq!(game.ticks(), ticks);
    assert_eq!(game.player_turns(), turns);
    assert_eq!(game.player_actor().hp(), hp);
    assert_eq!(game.player_actor().energy(), energy);
    assert_eq!(game.player_actor().position(), here);
    assert_eq!(
        save::to_string(&game).unwrap(),
        before,
        "the whole state, random streams included, is unchanged"
    );
    assert!(!game.is_over());
}

/// The hand-made room (30 x 12) is wider than the sight range: it takes a
/// few steps to see it all, and then the key is refused like on any floor.
#[test]
fn the_fixed_room_is_explored_in_a_few_steps_and_then_refused() {
    let mut game = Game::in_fixed_room(Seed::new(12));
    let mut steps = 0;
    let events = loop {
        let events = game.submit(Action::Explore);
        if events
            .iter()
            .any(|e| matches!(e, Event::NothingToExplore { .. }))
        {
            break events;
        }
        steps += 1;
        assert!(steps < 200, "the fixed room is never explored");
    };
    assert!(steps > 0, "a 30-column room is not seen in one glance");
    assert!(matches!(
        events.as_slice(),
        [Event::NothingToExplore { .. }]
    ));
    let before = save::to_string(&game).unwrap();
    let ticks = game.ticks();

    let again = game.submit(Action::Explore);

    assert_eq!(again, events);
    assert_eq!(game.ticks(), ticks);
    assert_eq!(save::to_string(&game).unwrap(), before);
}

// --- Never strikes ----------------------------------------------------------

/// Runs one step of exploration on `game` and returns the events.
fn explore_once(game: &mut Game) -> Vec<Event> {
    game.submit(Action::Explore)
}

/// A game alone on a bare floor, played until its next step has several
/// ways closer; returns it with the cells of those ways.
fn game_with_choice() -> (Game, Vec<Position>) {
    for n in 0..60 {
        let mut game = rooms_game(200 + n);
        for _ in 0..400 {
            let here = game.player_actor().position();
            let table = distance_to_unexplored(&game);
            let Some(d) = lookup(&table, here) else { break };
            let closer: Vec<Position> = neighbours(here)
                .filter(|&q| known_walkable(&game, q) && lookup(&table, q) == Some(d - 1))
                .collect();
            if closer.len() >= 2 {
                return (game, closer);
            }
            explore_once(&mut game);
        }
    }
    panic!("no step with a choice of ways");
}

#[test]
fn exploring_never_strikes_a_monster_that_holds_every_way_closer() {
    let content = content();
    let rat = content.monsters().first().expect("a monster");
    let (mut game, closer) = game_with_choice();
    let before_pos = game.player_actor().position();
    let mut hp = Vec::new();
    for &cell in &closer {
        game.spawn_monster(cell, rat);
    }
    for (_, a) in game.actors().filter(|(_, a)| !a.is_player()) {
        hp.push(a.hp());
    }
    let ticks = game.ticks();
    let snapshot = save::to_string(&game).unwrap();

    let events = explore_once(&mut game);

    assert!(
        events
            .iter()
            .all(|e| !matches!(e, Event::Attacked { .. } | Event::Moved { .. })),
        "{events:?}"
    );
    assert!(
        matches!(events.as_slice(), [Event::MoveBlocked { from, .. }] if *from == before_pos),
        "{events:?}"
    );
    assert_eq!(game.ticks(), ticks, "a blocked step costs no time");
    assert_eq!(game.player_actor().position(), before_pos);
    assert_eq!(save::to_string(&game).unwrap(), snapshot);
    let after: Vec<i32> = game
        .actors()
        .filter(|(_, a)| !a.is_player())
        .map(|(_, a)| a.hp())
        .collect();
    assert_eq!(after, hp, "no monster lost a point");
}

#[test]
fn exploring_steps_around_a_monster_when_another_way_is_free() {
    let content = content();
    let rat = content.monsters().first().expect("a monster");
    let (mut game, closer) = game_with_choice();
    let player = game.player();
    // Hold all but the last way.
    for &cell in &closer[..closer.len() - 1] {
        game.spawn_monster(cell, rat);
    }

    let events = explore_once(&mut game);

    assert!(
        events
            .iter()
            .all(|e| !matches!(e, Event::Attacked { attacker, .. } if *attacker == player)),
        "{events:?}"
    );
    assert_eq!(
        game.player_actor().position(),
        *closer.last().unwrap(),
        "the free way is taken"
    );
}

/// Over whole games with monsters that chase and bite, the player never
/// strikes while exploring.
#[test]
fn the_player_never_strikes_during_exploration_on_30_populated_floors() {
    let content = content();
    let mut steps_taken = 0_u32;
    let mut refusals = 0_u32;
    for n in 300..330 {
        let seed = seed_of(n);
        let mut game = Game::new(seed, &content);
        let player = game.player();
        for _ in 0..400 {
            if game.is_over() {
                break;
            }
            let events = game.submit(Action::Explore);
            for event in &events {
                if let Event::Attacked { attacker, .. } = event {
                    assert_ne!(*attacker, player, "seed {seed}: exploring struck");
                }
            }
            if events.iter().any(|e| {
                matches!(
                    e,
                    Event::NothingToExplore { .. } | Event::MoveBlocked { .. }
                )
            }) {
                refusals += 1;
                // Let time pass so a monster that blocks the way moves on.
                game.submit(Action::Wait);
            } else {
                steps_taken += 1;
            }
        }
    }
    println!("{steps_taken} steps and {refusals} refusals over 30 floors");
    assert!(steps_taken > 1000);
}

// --- Determinism ------------------------------------------------------------

#[test]
fn the_same_seed_explores_the_same_way_with_the_same_state_at_every_step() {
    let content = content();
    for n in 400..406 {
        let seed = seed_of(n);
        let mut a = Game::new(seed, &content);
        let mut b = Game::new(seed, &content);
        for step in 0..300 {
            let (ea, eb) = (a.submit(Action::Explore), b.submit(Action::Explore));
            assert_eq!(ea, eb, "seed {seed} step {step}");
            if step % 25 == 0 {
                assert_eq!(
                    save::to_string(&a).unwrap(),
                    save::to_string(&b).unwrap(),
                    "seed {seed} step {step}"
                );
            }
            if a.is_over() {
                break;
            }
        }
    }
}

/// A save taken in the middle of exploration reads back into a game that
/// explores the same way as the one that was never saved.
#[test]
fn a_game_saved_between_two_steps_explores_on_exactly_as_the_original() {
    let content = content();
    let seed = seed_of(500);
    let mut original = Game::new(seed, &content);
    for _ in 0..40 {
        original.submit(Action::Explore);
    }
    let text = save::to_string(&original).unwrap();
    let mut reloaded = save::from_str(&text, &content).expect("the save reads back");
    assert_eq!(save::to_string(&reloaded).unwrap(), text);
    for step in 0..200 {
        let (a, b) = (
            original.submit(Action::Explore),
            reloaded.submit(Action::Explore),
        );
        assert_eq!(a, b, "step {step}");
        if original.is_over() {
            break;
        }
    }
    assert_eq!(
        save::to_string(&original).unwrap(),
        save::to_string(&reloaded).unwrap()
    );
}

/// Exploration does not change what a bare floor is: exploring then
/// comparing the tiles with a fresh generation gives the same floor.
#[test]
fn exploring_does_not_alter_the_floor() {
    let mut game = rooms_game(9);
    let fresh = generate(seed_of(9), 1);
    explore_checked(&mut game, "rooms 9");
    for p in cells(&game) {
        assert_eq!(game.map().tile(p), fresh.map().tile(p), "{p:?}");
    }
}

/// Exploring a populated cave works as well (the monsters are placed out of
/// sight, so the first steps are always taken).
#[test]
fn a_populated_cave_can_be_explored_from_the_first_step() {
    let content = content();
    for n in 600..610 {
        let seed = seed_of(n);
        let floor = generate_with_monsters(seed, 10, &content);
        let mut game = Game::on_floor(seed, floor);
        let events = game.submit(Action::Explore);
        assert!(
            matches!(events.first(), Some(Event::Moved { .. })),
            "seed {seed}: {events:?}"
        );
    }
}
