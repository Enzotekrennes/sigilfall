//! Acceptance tests of F4 on the real game: monsters appear out of sight of
//! the start, are listed for drawing only when the player sees them, never
//! block the player without a visible reason, and the combat of T15 does
//! not break the pursuit.
//!
//! Written against the public API of the core crate only, on games built the
//! way the binary builds them (`Game::new(seed, &Content::load())`). The
//! player is driven by the tests, which may look at every actor (a tester's
//! privilege) to decide where to go; the game itself is only observed.

use std::collections::{HashMap, HashSet, VecDeque};

use sigilfall_core::action::Action;
use sigilfall_core::actor::ActorId;
use sigilfall_core::combat::Combatant;
use sigilfall_core::content::Content;
use sigilfall_core::event::Event;
use sigilfall_core::fov::SIGHT_RADIUS;
use sigilfall_core::game::{Game, Status};
use sigilfall_core::generation::MONSTERS_PER_FLOOR;
use sigilfall_core::geometry::{Direction, Position};
use sigilfall_core::map::Map;
use sigilfall_core::rng::Seed;

/// Seeds named by the acceptance brief.
fn named_seeds() -> Vec<Seed> {
    vec![
        Seed::from(0),
        Seed::from(42),
        "7F3A-91C2-0B5E-D4A8".parse().expect("valid grouped seed"),
    ]
}

/// The named seeds followed by `extra` consecutive ones.
fn seeds(extra: u64) -> Vec<Seed> {
    let mut all = named_seeds();
    all.extend((5000..5000 + extra).map(Seed::from));
    all
}

/// A new game of `seed` with the shipped content, as the binary starts it.
fn new_game(seed: Seed) -> Game {
    Game::new(seed, &Content::load().expect("shipped content is valid"))
}

/// Position of the actor `id`, which must be alive.
fn at(game: &Game, id: ActorId) -> Position {
    game.actor(id).expect("actor exists").position()
}

/// Largest of the horizontal and vertical gaps between two cells.
fn chebyshev(a: Position, b: Position) -> u32 {
    a.x.abs_diff(b.x).max(a.y.abs_diff(b.y))
}

/// Squared straight-line distance between two cells.
fn squared_distance(a: Position, b: Position) -> u32 {
    let (dx, dy) = (a.x.abs_diff(b.x), a.y.abs_diff(b.y));
    dx * dx + dy * dy
}

/// Squared distance up to which a cell can be seen (`fov::visible_cells`: a
/// cell is in range when `dx^2 + dy^2 <= radius^2 + radius`).
const SIGHT_RANGE_SQUARED: u32 = SIGHT_RADIUS * SIGHT_RADIUS + SIGHT_RADIUS;

/// Keys of every monster, alive now.
fn monsters(game: &Game) -> Vec<ActorId> {
    game.actors()
        .filter(|(_, actor)| !actor.is_player())
        .map(|(id, _)| id)
        .collect()
}

/// Steps from `from` to every reachable cell (eight directions,
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

/// First step of a shortest walk from `from` to `to` that crosses no cell of
/// `blocked` (`to` itself may be one), or `None` when there is none.
fn first_step(
    map: &Map,
    from: Position,
    to: Position,
    blocked: &HashSet<Position>,
) -> Option<Direction> {
    let mut first: HashMap<Position, Direction> = HashMap::new();
    let mut seen = HashSet::from([from]);
    let mut queue = VecDeque::from([from]);
    while let Some(cell) = queue.pop_front() {
        for direction in Direction::ALL {
            let next = cell.step(direction);
            if !map.can_step(cell, direction) || seen.contains(&next) {
                continue;
            }
            if blocked.contains(&next) && next != to {
                continue;
            }
            seen.insert(next);
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

/// Monsters the player sees, from `Game::visible_monsters`.
fn listed(game: &Game) -> HashSet<ActorId> {
    game.visible_monsters().map(|m| m.id).collect()
}

/// Monsters standing on a cell the player sees, found independently of
/// `Game::visible_monsters`: from every actor and `Game::is_visible`.
fn standing_in_view(game: &Game) -> HashSet<ActorId> {
    game.actors()
        .filter(|(_, actor)| !actor.is_player())
        .filter(|(_, actor)| game.is_visible(actor.position()))
        .map(|(id, _)| id)
        .collect()
}

/// Checks, after every turn, what the drawing relies on: the monsters listed
/// are exactly those in view, at the position the actor has, within sight
/// range, and with the family and elite flag of the actor.
fn assert_listing_is_consistent(game: &Game, context: &str) {
    assert_eq!(
        listed(game),
        standing_in_view(game),
        "{context}: visible_monsters differs from the monsters on visible cells"
    );
    let player = at(game, game.player());
    for monster in game.visible_monsters() {
        let actor = game.actor(monster.id).expect("listed monster exists");
        assert_eq!(monster.position, actor.position(), "{context}");
        let data = actor.as_monster().expect("a monster");
        assert_eq!(monster.definition, data.definition(), "{context}");
        assert_eq!(monster.family, data.family(), "{context}");
        assert_eq!(monster.elite, data.elite(), "{context}");
        assert!(
            squared_distance(player, monster.position) <= SIGHT_RANGE_SQUARED,
            "{context}: a monster beyond the sight range is listed"
        );
    }
}

/// Next value of a small linear congruential generator, independent from the
/// game's streams.
fn next(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    *state >> 33
}

// --- Spawning ----------------------------------------------------------------

#[test]
fn six_monsters_per_floor_none_in_view_none_within_sight_range_of_the_start() {
    for seed in seeds(300) {
        let game = new_game(seed);
        let start = at(&game, game.player());

        let all = monsters(&game);
        assert_eq!(all.len(), MONSTERS_PER_FLOOR, "seed {seed}");
        assert_eq!(game.visible_monsters().count(), 0, "seed {seed}");
        for id in all {
            let position = at(&game, id);
            assert!(
                !game.is_visible(position),
                "seed {seed}: {position:?} in view"
            );
            assert!(
                squared_distance(start, position) > SIGHT_RANGE_SQUARED,
                "seed {seed}: a monster starts at {position:?}, within sight range of {start:?}"
            );
            assert!(game.map().is_walkable(position), "seed {seed}");
        }
    }
}

#[test]
fn monsters_start_on_distinct_cells_and_not_on_the_stairs() {
    for seed in seeds(100) {
        let game = new_game(seed);
        let cells: Vec<Position> = game.actors().map(|(_, a)| a.position()).collect();
        let distinct: HashSet<Position> = cells.iter().copied().collect();
        assert_eq!(
            cells.len(),
            distinct.len(),
            "seed {seed}: two actors share a cell"
        );
        // The stairs are the farthest cell: a Tile::StairsDown cell is never occupied.
        for cell in cells {
            assert_ne!(
                game.map().tile(cell),
                Some(sigilfall_core::map::Tile::StairsDown),
                "seed {seed}: an actor stands on the stairs"
            );
        }
    }
}

#[test]
fn the_same_seed_gives_the_same_monsters() {
    for seed in named_seeds() {
        let describe = |game: &Game| -> Vec<(Position, String, i32)> {
            game.actors()
                .filter_map(|(_, a)| {
                    a.as_monster()
                        .map(|m| (a.position(), m.definition().to_owned(), a.energy()))
                })
                .collect()
        };
        assert_eq!(
            describe(&new_game(seed)),
            describe(&new_game(seed)),
            "seed {seed}"
        );
    }
}

// --- Drawn only when visible ---------------------------------------------

#[test]
fn monsters_are_listed_only_while_their_cell_is_visible() {
    let mut sightings = 0;
    for seed in seeds(12) {
        let mut game = new_game(seed);
        let mut state = seed.value() | 1;
        assert_listing_is_consistent(&game, &format!("seed {seed}, start"));
        for turn in 1..=250 {
            if game.is_over() {
                break;
            }
            let player = at(&game, game.player());
            let free: Vec<Direction> = Direction::ALL
                .into_iter()
                .filter(|&d| game.map().can_step(player, d))
                .collect();
            let direction = free[next(&mut state) as usize % free.len()];
            game.submit(Action::Move(direction));
            assert_listing_is_consistent(&game, &format!("seed {seed}, turn {turn}"));
            sightings += game.visible_monsters().count();
        }
    }
    assert!(
        sightings > 0,
        "no monster was ever seen, the test proves nothing"
    );
}

// --- A walking tester: pursuit and combat together ------------------------

/// What a scripted play-through found.
#[derive(Default, Debug)]
struct Tally {
    /// Blows struck by monsters at the player.
    monster_blows: usize,
    /// Blows struck by the player.
    player_blows: usize,
    /// Monsters killed.
    kills: usize,
    /// Games where the player died.
    deaths: usize,
}

/// Plays up to `turns` turns of `seed`, the player walking to the nearest
/// monster (it can look at every actor) and hitting it, and checks after
/// each turn what the rules promise. Adds what happened to `tally`.
fn play_towards_monsters(seed: Seed, turns: usize, tally: &mut Tally) {
    let mut game = new_game(seed);
    let player_id = game.player();
    for turn in 1..=turns {
        if game.is_over() {
            break;
        }
        let context = format!("seed {seed}, turn {turn}");
        let player = at(&game, player_id);
        let targets: HashSet<Position> = monsters(&game).iter().map(|&m| at(&game, m)).collect();
        let to_player = distances(game.map(), player);
        let Some(&goal) = targets
            .iter()
            .filter(|cell| to_player.contains_key(cell))
            .min_by_key(|cell| (to_player[cell], cell.y, cell.x))
        else {
            break; // nobody left to fight.
        };
        let Some(direction) = first_step(game.map(), player, goal, &targets) else {
            break;
        };
        let target_cell = player.step(direction);
        let victim = game
            .actors()
            .find(|(id, a)| *id != player_id && a.position() == target_cell)
            .map(|(id, _)| id);
        let seen_before = listed(&game);

        let events = game.submit(Action::Move(direction));

        assert!(
            !events.is_empty(),
            "{context}: a submitted action must report something"
        );
        check_events(
            &game,
            &events,
            player_id,
            victim,
            &seen_before,
            &context,
            tally,
        );
        assert_listing_is_consistent(&game, &context);
    }
    if game.is_over() {
        tally.deaths += 1;
    }
}

/// Checks the events of one turn against the game state after it.
fn check_events(
    game: &Game,
    events: &[Event],
    player_id: ActorId,
    victim: Option<ActorId>,
    seen_before: &HashSet<ActorId>,
    context: &str,
    tally: &mut Tally,
) {
    let player = at(game, player_id);
    let mut last_hp_left = None;
    for event in events {
        match event {
            Event::MoveBlocked { actor, to, .. } if *actor == player_id => {
                assert!(
                    victim.is_none(),
                    "{context}: the player was refused a move into an actor ({to:?}): an \
                     unexplained block"
                );
            }
            Event::Attacked {
                attacker,
                target,
                damage,
                hp_left,
            } => {
                assert!(*damage >= 1, "{context}: a blow must do at least 1 damage");
                if *attacker == player_id {
                    tally.player_blows += 1;
                    assert_eq!(Some(*target), victim, "{context}");
                    assert!(
                        seen_before.contains(target),
                        "{context}: the player struck a monster it could not see listed"
                    );
                } else {
                    tally.monster_blows += 1;
                    assert_eq!(
                        *target, player_id,
                        "{context}: monsters only strike the player"
                    );
                    let striker = game.actor(*attacker).expect("the striker is alive");
                    assert_eq!(
                        chebyshev(striker.position(), player),
                        1,
                        "{context}: a monster struck from a distance"
                    );
                    last_hp_left = Some(*hp_left);
                    assert!(
                        game.is_visible(striker.position()),
                        "{context}: an invisible monster struck the player"
                    );
                }
            }
            Event::Died {
                actor,
                who,
                killed_by,
            } => {
                if *actor == player_id {
                    assert_eq!(*who, Combatant::Player, "{context}");
                    assert!(
                        matches!(killed_by, Combatant::Monster { .. }),
                        "{context}: the player must be killed by a monster"
                    );
                    assert_eq!(
                        game.status(),
                        &Status::GameOver {
                            killed_by: killed_by.clone()
                        },
                        "{context}"
                    );
                } else {
                    tally.kills += 1;
                    assert!(
                        game.actor(*actor).is_none(),
                        "{context}: a dead monster remains"
                    );
                    assert!(
                        matches!(who, Combatant::Monster { .. }),
                        "{context}: {who:?}"
                    );
                }
            }
            _ => {}
        }
    }
    // The last blow the player took left it with the hit points it has now.
    if let Some(hp_left) = last_hp_left {
        assert_eq!(hp_left, game.player_actor().hp(), "{context}");
    }
}

#[test]
fn combat_and_pursuit_coexist_over_whole_runs() {
    let mut tally = Tally::default();
    for seed in seeds(10) {
        play_towards_monsters(seed, 400, &mut tally);
    }
    println!("real play: {tally:?}");
    assert!(tally.player_blows > 0, "the player never struck: {tally:?}");
    assert!(tally.monster_blows > 0, "no monster ever struck: {tally:?}");
    assert!(tally.kills > 0, "no monster was ever killed: {tally:?}");
}

#[test]
fn a_dead_player_ends_the_game_and_submit_changes_nothing_afterwards() {
    let mut found = 0;
    for seed in seeds(30) {
        let mut game = new_game(seed);
        let player_id = game.player();
        for _ in 0..600 {
            if game.is_over() {
                break;
            }
            let player = at(&game, player_id);
            let targets: HashSet<Position> =
                monsters(&game).iter().map(|&m| at(&game, m)).collect();
            let to_player = distances(game.map(), player);
            let Some(&goal) = targets
                .iter()
                .filter(|cell| to_player.contains_key(cell))
                .min_by_key(|cell| (to_player[cell], cell.y, cell.x))
            else {
                break;
            };
            // Walk to the monster, then stand still and let it kill us.
            if to_player[&goal] > 2 {
                if let Some(direction) = first_step(game.map(), player, goal, &targets) {
                    game.submit(Action::Move(direction));
                    continue;
                }
                break;
            }
            game.submit(Action::Wait);
        }
        if game.is_over() {
            found += 1;
            let turns = game.player_turns();
            let ticks = game.ticks();
            let positions: Vec<Position> = game.actors().map(|(_, a)| a.position()).collect();

            assert!(game.submit(Action::Wait).is_empty(), "seed {seed}");
            assert!(
                game.submit(Action::Move(Direction::North)).is_empty(),
                "seed {seed}"
            );

            assert_eq!(game.player_turns(), turns, "seed {seed}");
            assert_eq!(game.ticks(), ticks, "seed {seed}");
            assert_eq!(
                game.actors().map(|(_, a)| a.position()).collect::<Vec<_>>(),
                positions,
                "seed {seed}"
            );
            assert_eq!(game.player_actor().hp(), 0, "seed {seed}");
        }
    }
    assert!(
        found >= 1,
        "the player never died in these runs, the test proves nothing"
    );
}

// --- Order and determinism -------------------------------------------------

/// Plays `turns` turns of `seed` with a fixed pseudo-random script of moves
/// and waits, and returns every event in order.
fn scripted_events(seed: Seed, turns: usize) -> Vec<Event> {
    let mut game = new_game(seed);
    let mut state = seed.value() ^ 0x1234;
    let mut all = Vec::new();
    for _ in 0..turns {
        let action = match next(&mut state) % 3 {
            0 => Action::Wait,
            n => Action::Move(Direction::ALL[(next(&mut state) as usize + n as usize) % 8]),
        };
        all.extend(game.submit(action));
    }
    all
}

#[test]
fn the_same_seed_and_inputs_give_the_same_events_with_monsters_about() {
    for seed in named_seeds() {
        let first = scripted_events(seed, 300);
        assert!(
            first.len() > 300,
            "seed {seed}: the script produced too little"
        );
        assert_eq!(first, scripted_events(seed, 300), "seed {seed}");
    }
}

#[test]
fn the_player_always_acts_first_in_a_turn() {
    for seed in seeds(30) {
        let mut game = new_game(seed);
        let player = game.player();
        let mut state = seed.value() ^ 0x77;
        for turn in 1..=200 {
            if game.is_over() {
                break;
            }
            let action = if next(&mut state).is_multiple_of(4) {
                Action::Wait
            } else {
                Action::Move(Direction::ALL[next(&mut state) as usize % 8])
            };
            let events = game.submit(action);
            let first = events.first().expect("an action reports something");
            let actor = match first {
                Event::Moved { actor, .. }
                | Event::Waited { actor }
                | Event::MoveBlocked { actor, .. } => *actor,
                Event::Attacked { attacker, .. } => *attacker,
                Event::Died { actor, .. } => *actor,
            };
            assert_eq!(actor, player, "seed {seed}, turn {turn}: {events:?}");
        }
    }
}

// --- Invisible monsters never block without a reason ----------------------

#[test]
fn no_invisible_monster_blocks_the_player_without_an_explanation() {
    let mut moves = 0;
    let mut blocked_by_walls = 0;
    for seed in seeds(40) {
        let mut game = new_game(seed);
        let player_id = game.player();
        let mut state = seed.value() ^ 0xA5A5;
        for turn in 1..=500 {
            if game.is_over() {
                break;
            }
            let player = at(&game, player_id);
            // Any of the eight directions, walls included.
            let direction = Direction::ALL[next(&mut state) as usize % 8];
            let target = player.step(direction);
            let occupant = game
                .actors()
                .find(|(id, a)| *id != player_id && a.position() == target)
                .map(|(id, _)| id);
            let seen_before = listed(&game);

            let events = game.submit(Action::Move(direction));
            moves += 1;

            let context = format!("seed {seed}, turn {turn}");
            match (&events[0], occupant) {
                (Event::MoveBlocked { .. }, None) => {
                    blocked_by_walls += 1;
                    assert!(
                        !game.map().can_step(player, direction),
                        "{context}: a move onto a free walkable cell was refused"
                    );
                    assert_eq!(events.len(), 1, "{context}: a refused move takes no time");
                }
                (Event::MoveBlocked { .. }, Some(_)) => {
                    panic!("{context}: refused by an actor instead of a blow: {events:?}")
                }
                (
                    Event::Attacked {
                        attacker, target, ..
                    },
                    Some(victim),
                ) => {
                    assert_eq!((*attacker, *target), (player_id, victim), "{context}");
                    assert!(
                        seen_before.contains(&victim),
                        "{context}: bumped into a monster the player could not see"
                    );
                }
                (Event::Moved { .. }, None) => {}
                (first, occupant) => panic!("{context}: unexpected {first:?} with {occupant:?}"),
            }
            // A monster next to the player is always in view: nothing can hide
            // right beside it.
            let player = at(&game, player_id);
            for (id, actor) in game.actors().filter(|(id, _)| *id != player_id) {
                if chebyshev(actor.position(), player) == 1 {
                    assert!(
                        game.is_visible(actor.position()) && listed(&game).contains(&id),
                        "{context}: a monster beside the player is not shown"
                    );
                }
            }
        }
    }
    println!("{moves} random moves, {blocked_by_walls} refused by a wall");
    assert!(moves > 1000 && blocked_by_walls > 0);
}

// --- A visible monster is pursued all the way ------------------------------

#[test]
fn a_monster_that_comes_into_view_reaches_a_waiting_player() {
    // The player walks until the first monster comes into view, then stands
    // still. That monster must come adjacent and strike (or the player dies
    // first), within a bound that depends on how far it was.
    let mut cases = 0;
    for seed in seeds(40) {
        let mut game = new_game(seed);
        let player_id = game.player();
        let mut spotted = None;
        for _ in 0..400 {
            if game.is_over() {
                break;
            }
            if let Some(first) = game.visible_monsters().next() {
                spotted = Some(first.id);
                break;
            }
            // Walk towards the nearest monster until it comes into view.
            let player = at(&game, player_id);
            let targets: HashSet<Position> =
                monsters(&game).iter().map(|&m| at(&game, m)).collect();
            let to_player = distances(game.map(), player);
            let Some(&goal) = targets
                .iter()
                .filter(|cell| to_player.contains_key(cell))
                .min_by_key(|cell| (to_player[cell], cell.y, cell.x))
            else {
                break;
            };
            let Some(direction) = first_step(game.map(), player, goal, &targets) else {
                break;
            };
            game.submit(Action::Move(direction));
        }
        let Some(hunter) = spotted else { continue };
        if game.is_over() {
            continue;
        }

        let player = at(&game, player_id);
        let steps = distances(game.map(), player)[&at(&game, hunter)];
        // Slowest shipped monster: 75, so a cell takes at most 2 turns.
        let bound = 2 * steps + 6;
        let mut struck = false;
        for _ in 0..bound {
            let events = game.submit(Action::Wait);
            if events
                .iter()
                .any(|e| matches!(e, Event::Attacked { target, .. } if *target == player_id))
            {
                struck = true;
                break;
            }
            if game.is_over() {
                struck = true;
                break;
            }
        }
        assert!(
            struck,
            "seed {seed}: a monster {steps} steps away never reached the waiting player in \
             {bound} turns"
        );
        cases += 1;
    }
    assert!(
        cases >= 15,
        "only {cases} games had a sighting, the test is too weak"
    );
}
