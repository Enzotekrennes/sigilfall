//! Pursuit: monsters spend energy by speed, chase by the shortest path while
//! they see the player, then go to the last cell where they saw it and wait
//! there.
//!
//! Written against the public API of the core crate only. Every distance
//! is recomputed here with a plain breadth-first search over `Map::can_step`,
//! not with the game's own A* or Dijkstra code, so a mistake shared by the
//! game and its own helpers cannot hide. Floors come from `generate` (no
//! monster) and the monsters under test are placed by hand with
//! `Game::spawn_monster`, so nothing else moves on the floor.

use std::collections::{HashMap, VecDeque};

use sigilfall_core::action::Action;
use sigilfall_core::actor::ActorId;
use sigilfall_core::content::{Content, MonsterDef};
use sigilfall_core::event::Event;
use sigilfall_core::game::Game;
use sigilfall_core::generation::{FIRST_DEPTH, generate, generate_with_monsters};
use sigilfall_core::geometry::{Direction, Position};
use sigilfall_core::map::Map;
use sigilfall_core::rng::Seed;

/// The reference seeds of the tests, among them the seed shown to players as
/// an example, plus the extra ones used when a check needs many floors.
fn named_seeds() -> Vec<Seed> {
    vec![
        Seed::from(0),
        Seed::from(42),
        "7F3A-91C2-0B5E-D4A8".parse().expect("valid grouped seed"),
    ]
}

/// A file of harmless test creatures (5000 hit points, attack 0, so a blow
/// does the minimum) of the given speeds, named `s<speed>`.
fn content_with_speeds(speeds: &[i32]) -> Content {
    let entries: String = speeds
        .iter()
        .map(|speed| {
            format!(
                "    Monster(id: \"s{speed}\", name: \"test creature\", name_fr: Masculine(\"le monstre\"), plural: \"test creatures\", plural_fr: \"monstres\", family: Vermin, \
                 elite: false, speed: {speed}, hp: 900, attack: 0, defense: 0, min_depth: 1),\n"
            )
        })
        .collect();
    Content::parse(&format!("[\n{entries}]\n")).expect("test content must be valid")
}

/// The test creature of speed `speed` (see `content_with_speeds`).
fn creature(content: &Content, speed: i32) -> MonsterDef {
    content
        .monster(&format!("s{speed}"))
        .expect("speed present in the test content")
        .clone()
}

/// A game on the monster-free floor 1 of `seed`.
fn empty_floor(seed: Seed) -> Game {
    Game::on_floor(seed, generate(seed, FIRST_DEPTH))
}

/// Lets one turn pass after `Game::spawn_monster`.
///
/// A monster placed by hand starts with no energy. After the player's first
/// action it has gained its speed, but the player, created first, wins the
/// tie at the next tick and plays again before it: the monster takes its first
/// turn during the player's second action. (The doc of `spawn_monster` says
/// "the player's next action"; see the report.) Tests that measure the
/// monster therefore start after this turn.
fn warm_up(game: &mut Game, monster: ActorId) {
    let events = game.submit(Action::Wait);
    assert_eq!(
        turns_taken(&events, monster),
        0,
        "the monster is not expected to act during the first turn: {events:?}"
    );
}

/// Position of actor `id`.
fn at(game: &Game, id: ActorId) -> Position {
    game.actor(id).expect("actor exists").position()
}

/// Steps in `map` from `from` to every reachable cell (eight directions,
/// `Map::can_step`), ignoring every actor.
fn distances(map: &Map, from: Position) -> HashMap<Position, u32> {
    let mut seen = HashMap::from([(from, 0)]);
    let mut queue = VecDeque::from([from]);
    while let Some(cell) = queue.pop_front() {
        let here = seen[&cell];
        for direction in Direction::ALL {
            let next = cell.step(direction);
            if map.can_step(cell, direction) && !seen.contains_key(&next) {
                seen.insert(next, here + 1);
                queue.push_back(next);
            }
        }
    }
    seen
}

/// Every walkable cell the player sees now, other than its own, sorted in
/// reading order.
fn visible_floor_cells(game: &Game) -> Vec<Position> {
    let player = at(game, game.player());
    let map = game.map();
    let mut cells = Vec::new();
    for y in 0..map.height() as i32 {
        for x in 0..map.width() as i32 {
            let cell = Position::new(x, y);
            if cell != player && map.is_walkable(cell) && game.is_visible(cell) {
                cells.push(cell);
            }
        }
    }
    cells
}

/// How many turns `id` took in `events`: it moved, waited or struck.
fn turns_taken(events: &[Event], id: ActorId) -> usize {
    events
        .iter()
        .filter(|event| match event {
            Event::Moved { actor, .. } | Event::Waited { actor } => *actor == id,
            Event::Attacked { attacker, .. } => *attacker == id,
            _ => false,
        })
        .count()
}

/// A direction in which the player can step without bumping into anything,
/// and the opposite one, for a back-and-forth walk that always stays legal.
fn free_direction(game: &Game) -> Direction {
    let player = at(game, game.player());
    Direction::ALL
        .into_iter()
        .find(|&direction| {
            game.map().can_step(player, direction)
                && game
                    .actors()
                    .all(|(_, a)| a.position() != player.step(direction))
        })
        .expect("the player has a free neighbour")
}

/// The opposite of `direction`.
fn opposite(direction: Direction) -> Direction {
    let (dx, dy) = direction.delta();
    Direction::ALL
        .into_iter()
        .find(|d| d.delta() == (-dx, -dy))
        .expect("every direction has an opposite")
}

/// A walkable cell the player does not see, as far as possible from it, so
/// the monster placed there cannot see the player.
fn farthest_hidden_cell(game: &Game) -> Position {
    let player = at(game, game.player());
    let from_player = distances(game.map(), player);
    from_player
        .iter()
        .filter(|(cell, _)| !game.is_visible(**cell))
        .max_by_key(|(cell, steps)| (**steps, cell.y, cell.x))
        .map(|(cell, _)| *cell)
        .expect("a floor has cells out of sight")
}

// --- Energy and speed ------------------------------------------

#[test]
fn a_speed_200_monster_acts_twice_per_normal_action() {
    let content = content_with_speeds(&[200]);
    for seed in named_seeds() {
        let mut game = empty_floor(seed);
        let hidden = farthest_hidden_cell(&game);
        let fast = game.spawn_monster(hidden, &creature(&content, 200));
        warm_up(&mut game, fast);

        for turn in 1..=10 {
            let events = game.submit(Action::Wait);
            assert_eq!(
                turns_taken(&events, fast),
                2,
                "seed {seed}, turn {turn}: a speed 200 monster must act twice, events: {events:?}"
            );
        }
    }
}

#[test]
fn on_a_generated_floor_every_monster_acts_twice_from_the_first_turn() {
    // The real path: monsters placed by the generator, with their random
    // starting energy, all of speed 200 (a content file that holds only that).
    let content = content_with_speeds(&[200]);
    for seed in named_seeds() {
        let floor = generate_with_monsters(seed, FIRST_DEPTH, &content);
        let mut game = Game::on_floor(seed, floor);
        let monsters: Vec<ActorId> = game
            .actors()
            .filter(|(_, actor)| !actor.is_player())
            .map(|(id, _)| id)
            .collect();
        assert_eq!(monsters.len(), 6);

        for turn in 1..=6 {
            let events = game.submit(Action::Wait);
            for &monster in &monsters {
                assert_eq!(
                    turns_taken(&events, monster),
                    2,
                    "seed {seed}, turn {turn}: events: {events:?}"
                );
            }
        }
    }
}

#[test]
fn still_holds_when_the_normal_action_is_a_move() {
    // The player walks back and forth: cost of a move equals cost of a wait.
    let content = content_with_speeds(&[200]);
    for seed in named_seeds() {
        let mut game = empty_floor(seed);
        let hidden = farthest_hidden_cell(&game);
        let fast = game.spawn_monster(hidden, &creature(&content, 200));
        warm_up(&mut game, fast);
        let out = free_direction(&game);

        for turn in 1..=10 {
            let direction = if turn % 2 == 1 { out } else { opposite(out) };
            let events = game.submit(Action::Move(direction));
            assert!(matches!(events[0], Event::Moved { .. }), "{events:?}");
            assert_eq!(
                turns_taken(&events, fast),
                2,
                "seed {seed}, turn {turn}: events: {events:?}"
            );
        }
    }
}

#[test]
fn the_player_turn_counter_ignores_monster_turns() {
    let content = content_with_speeds(&[200]);
    let mut game = empty_floor(Seed::from(42));
    let hidden = farthest_hidden_cell(&game);
    let fast = game.spawn_monster(hidden, &creature(&content, 200));
    warm_up(&mut game, fast);

    for _ in 0..4 {
        game.submit(Action::Wait);
    }

    assert_eq!(game.player_turns(), 5);
}

#[test]
fn the_number_of_monster_turns_follows_its_speed_exactly() {
    let speeds = [50, 75, 100, 125, 150, 200, 300, 400];
    let content = content_with_speeds(&speeds);
    for speed in speeds {
        let mut game = empty_floor(Seed::from(42));
        let hidden = farthest_hidden_cell(&game);
        let monster = game.spawn_monster(hidden, &creature(&content, speed));
        let mut total = 0;
        for turn in 1..=20_i32 {
            total += turns_taken(&game.submit(Action::Wait), monster) as i32;
            assert_eq!(
                total,
                (turn - 1) * speed / 100,
                "speed {speed}: after {turn} player turns the monster must have acted \
                 floor({} x {speed} / 100) times (one tick of lag, see `warm_up`)",
                turn - 1
            );
        }
    }
}

#[test]
fn a_fast_monster_that_sees_the_player_covers_two_cells_per_turn() {
    let content = content_with_speeds(&[100, 200]);
    let mut checked = 0;
    for seed in named_seeds() {
        let base = empty_floor(seed);
        let player = at(&base, base.player());
        let from_player = distances(base.map(), player);
        let starts: Vec<Position> = visible_floor_cells(&base)
            .into_iter()
            .filter(|cell| from_player[cell] >= 6)
            .collect();
        for start in starts.iter().step_by(3).take(6) {
            let mut game = base.clone();
            let fast = game.spawn_monster(*start, &creature(&content, 200));
            warm_up(&mut game, fast);
            let before = distances(game.map(), player)[start];

            let events = game.submit(Action::Wait);

            let after = distances(game.map(), player)[&at(&game, fast)];
            assert_eq!(
                before - after,
                2,
                "seed {seed}, from {start:?}: the monster must close 2 cells in one turn, \
                 events: {events:?}"
            );
            checked += 1;
        }
    }
    assert!(
        checked >= 3,
        "only {checked} cases found, the test is too weak"
    );
}

// --- Pursuit by the shortest path -----------------------------

#[test]
fn a_monster_that_sees_the_player_closes_in_by_a_shortest_path() {
    let content = content_with_speeds(&[100]);
    let mut seeds = named_seeds();
    seeds.extend((1000..1015).map(Seed::from));
    let (mut cases, mut detours) = (0, 0);
    for seed in seeds {
        let base = empty_floor(seed);
        let player = at(&base, base.player());
        let from_player = distances(base.map(), player);
        let starts: Vec<Position> = visible_floor_cells(&base)
            .into_iter()
            .filter(|cell| (2..=8).contains(&from_player[cell]))
            .collect();
        for start in starts.iter().step_by(4).take(8) {
            let mut game = base.clone();
            let monster = game.spawn_monster(*start, &creature(&content, 100));
            warm_up(&mut game, monster);
            let chebyshev = start.x.abs_diff(player.x).max(start.y.abs_diff(player.y));
            if from_player[start] > chebyshev {
                detours += 1;
            }

            let mut steps = from_player[start];
            while steps > 1 {
                let events = game.submit(Action::Wait);
                let here = at(&game, monster);
                assert_eq!(
                    from_player[&here],
                    steps - 1,
                    "seed {seed}, from {start:?}: the monster left the shortest path \
                     (events: {events:?})"
                );
                assert!(
                    events
                        .iter()
                        .any(|e| matches!(e, Event::Moved { actor, .. } if *actor == monster))
                );
                steps -= 1;
            }
            // Next to the player, it strikes instead of moving.
            let events = game.submit(Action::Wait);
            assert!(
                events.iter().any(|e| matches!(e,
                    Event::Attacked { attacker, target, .. }
                        if *attacker == monster && *target == game.player())),
                "seed {seed}, from {start:?}: the adjacent monster must attack, got {events:?}"
            );
            assert_eq!(from_player[&at(&game, monster)], 1);
            cases += 1;
        }
    }
    assert!(
        cases >= 20,
        "only {cases} cases found, the test is too weak"
    );
    println!("shortest path: {cases} approaches checked, {detours} of them needing a detour");
}

#[test]
fn a_monster_does_not_move_before_the_player_acts_and_does_not_skip_its_turn() {
    // Every turn of a visible speed 100 monster is one step or one blow,
    // never a wait.
    let content = content_with_speeds(&[100]);
    let base = empty_floor(Seed::from(42));
    let player = at(&base, base.player());
    let from_player = distances(base.map(), player);
    let start = *visible_floor_cells(&base)
        .iter()
        .find(|cell| from_player[cell] == 6)
        .expect("a visible cell six steps away");
    let mut game = base;
    let monster = game.spawn_monster(start, &creature(&content, 100));
    warm_up(&mut game, monster);
    assert_eq!(at(&game, monster), start);

    for _ in 0..3 {
        let events = game.submit(Action::Wait);
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::Waited { actor } if *actor == monster)),
            "a monster that sees the player must not wait: {events:?}"
        );
    }
}

// --- Lost sight: go to the last known position, then wait -----------------

/// Next value of a small linear congruential generator, so the walks of the
/// tests are reproducible and independent from the game's streams.
fn next(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    *state >> 33
}

/// The direction in which the player gets farthest from `monster` in one
/// step (ties drawn from `state`), or `None` if it cannot move.
fn flee_direction(game: &Game, monster: ActorId, state: &mut u64) -> Option<Direction> {
    let player = at(game, game.player());
    let from_monster = distances(game.map(), at(game, monster));
    let free: Vec<Direction> = Direction::ALL
        .into_iter()
        .filter(|&d| game.map().can_step(player, d))
        .filter(|&d| game.actors().all(|(_, a)| a.position() != player.step(d)))
        .collect();
    let best = free.iter().map(|&d| from_monster[&player.step(d)]).max()?;
    let options: Vec<Direction> = free
        .into_iter()
        .filter(|&d| from_monster[&player.step(d)] == best)
        .collect();
    Some(options[next(state) as usize % options.len()])
}

/// Plays one "lost sight" scenario starting from `start`: the monster sees
/// the player, then the player flees until the monster is out of sight.
///
/// The monster is slow (speed 50) so that the player can outrun it. Returns
/// `None` when the player did not manage to lose the monster, or when the
/// monster, on its way, came back into sight (the rule under test no longer
/// applies then). Otherwise checks the rule and returns the cell the monster
/// went to.
fn lost_sight_run(base: &Game, content: &Content, start: Position, walk: u64) -> Option<Position> {
    let mut game = base.clone();
    let monster = game.spawn_monster(start, &creature(content, 50));
    warm_up(&mut game, monster);
    // Two more player turns give a speed 50 monster one turn to act.
    game.submit(Action::Wait);
    game.submit(Action::Wait);
    let seen_at = at(&game, game.player());
    assert_eq!(
        game.actor(monster)?.as_monster()?.last_seen_player(),
        Some(seen_at),
        "the monster saw the player and must remember where"
    );

    // The player flees (greedily away from the monster) until the monster
    // stands on a cell it cannot see.
    let mut state = walk;
    let mut lost = false;
    for _ in 0..16 {
        let direction = flee_direction(&game, monster, &mut state)?;
        game.submit(Action::Move(direction));
        if game.is_over() {
            return None;
        }
        let remembered = game.actor(monster)?.as_monster()?.last_seen_player();
        if !game.is_visible(at(&game, monster)) && remembered.is_some() {
            lost = true;
            break;
        }
    }
    if !lost {
        return None;
    }

    let goal = game.actor(monster)?.as_monster()?.last_seen_player()?;
    let to_goal = distances(game.map(), goal);
    let mut previous_cell = at(&game, monster);
    let mut arrived_turns = 0;
    let mut waits_on_goal = 0;
    for turn in 0..120 {
        // The player keeps walking away for a while, then stands still.
        let action = match flee_direction(&game, monster, &mut state) {
            Some(direction) if turn < 12 => Action::Move(direction),
            _ => Action::Wait,
        };
        let events = game.submit(action);
        if game.is_over() {
            return None;
        }
        if game.visible_monsters().next().is_some() {
            return None; // came back into sight: it hunts the player again.
        }
        assert!(
            !events.iter().any(|e| matches!(e, Event::Attacked { .. })),
            "a monster that cannot see the player must not strike: {events:?}"
        );
        let here = at(&game, monster);
        if previous_cell == goal {
            // Already on the last known cell: it stays and waits.
            assert_eq!(here, goal, "once there, the monster must stay and wait");
            waits_on_goal += turns_taken(&events, monster);
            arrived_turns += 1;
            if arrived_turns >= 8 {
                break;
            }
        } else if here != previous_cell {
            assert_eq!(
                to_goal[&here] + 1,
                to_goal[&previous_cell],
                "the monster must walk a shortest path to the last known cell {goal:?}:                  it went from {previous_cell:?} to {here:?}"
            );
        }
        previous_cell = here;
    }
    assert_eq!(
        previous_cell, goal,
        "the monster never reached the last known cell"
    );
    // It forgets the cell on its first turn there: nothing left to look for.
    assert_eq!(game.actor(monster)?.as_monster()?.last_seen_player(), None);
    assert!(
        waits_on_goal >= 2,
        "it must wait on the cell, waited {waits_on_goal} times"
    );
    let player_now = at(&game, game.player());
    assert_ne!(
        previous_cell, player_now,
        "it must not know where the player is"
    );
    assert!(
        distances(game.map(), goal)[&player_now] >= 2,
        "the player must have moved away from the last known cell for the check to mean anything"
    );
    Some(goal)
}

#[test]
fn a_monster_that_loses_sight_goes_to_the_last_known_cell_then_waits() {
    let content = content_with_speeds(&[50]);
    let mut seeds = named_seeds();
    seeds.extend((2000..2040).map(Seed::from));
    let mut validated = 0;
    for seed in seeds {
        let base = empty_floor(seed);
        let player = at(&base, base.player());
        let from_player = distances(base.map(), player);
        let starts: Vec<Position> = visible_floor_cells(&base)
            .into_iter()
            .filter(|cell| (4..=8).contains(&from_player[cell]))
            .collect();
        for (index, start) in starts.iter().step_by(5).take(6).enumerate() {
            if lost_sight_run(&base, &content, *start, seed.value() ^ index as u64).is_some() {
                validated += 1;
            }
        }
    }
    println!("lost sight: {validated} complete scenarios validated");
    assert!(
        validated >= 5,
        "only {validated} scenarios could be built, the test is too weak"
    );
}
