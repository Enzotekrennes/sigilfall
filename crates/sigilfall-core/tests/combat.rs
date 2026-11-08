//! Acceptance tests of F5 (combat, life and deaths) on the core crate.
//!
//! Written against the public API only. They check the three acceptance
//! scenarios at the level of events and game state (the screens are checked
//! by the binary's tests): a move into a monster strikes it and the hit
//! points follow the events exactly; a monster at 0 hit points leaves the
//! world; the death of the player ends the game and nothing happens after
//! it; the same seed and the same inputs replay the same blows.
//!
//! The expected damage is recomputed here from the rule of decision D17
//! (`max(1, attack - defense + roll)`, roll from -1 to 1) rather than taken
//! from the `combat` module, and every loop is bounded.

use std::collections::{HashMap, HashSet, VecDeque};

use sigilfall_core::action::Action;
use sigilfall_core::actor::ActorId;
use sigilfall_core::combat::Combatant;
use sigilfall_core::content::Content;
use sigilfall_core::event::Event;
use sigilfall_core::game::{Game, PLAYER_START, Status};
use sigilfall_core::geometry::{Direction, Position};
use sigilfall_core::map::Map;
use sigilfall_core::rng::Seed;

/// Starting values of the player (decision D17).
const PLAYER_HP: i32 = 30;
const PLAYER_ATTACK: i32 = 4;
const PLAYER_DEFENSE: i32 = 1;

/// Creatures whose fights are known in advance.
const TEST_MONSTERS: &str = r#"[
    Monster(id: "brute", name: "brute", family: Acolyte, elite: false,
        speed: 100, hp: 40, attack: 0, defense: 2, min_depth: 1),
    Monster(id: "gnat", name: "cave gnat", family: Vermin, elite: false,
        speed: 100, hp: 999, attack: 0, defense: 0, min_depth: 1),
    Monster(id: "moth", name: "wax moth", family: Vermin, elite: false,
        speed: 100, hp: 1, attack: 0, defense: 0, min_depth: 1),
    Monster(id: "executioner", name: "executioner", family: Acolyte, elite: true,
        speed: 100, hp: 999, attack: 99, defense: 0, min_depth: 1),
    Monster(id: "stinger", name: "stinger", family: Vermin, elite: false,
        speed: 400, hp: 999, attack: 29, defense: 0, min_depth: 1),
]"#;

/// Seeds named by the acceptance brief.
fn named_seeds() -> Vec<Seed> {
    vec![
        Seed::from(0),
        Seed::from(42),
        "7F3A-91C2-0B5E-D4A8".parse().expect("valid grouped seed"),
    ]
}

/// The game's own content.
fn shipped() -> Content {
    Content::load().expect("shipped content is valid")
}

/// The test creatures of [`TEST_MONSTERS`].
fn test_content() -> Content {
    Content::parse(TEST_MONSTERS).expect("valid test monsters")
}

/// The cell east of the player's start.
fn east() -> Position {
    PLAYER_START.step(Direction::East)
}

/// A fixed-room game with a `definition` creature east of the player.
fn duel(seed: Seed, definition: &str) -> (Game, ActorId) {
    let content = test_content();
    let mut game = Game::in_fixed_room(seed);
    let id = game.spawn_monster(
        east(),
        content.monster(definition).expect("known test monster"),
    );
    (game, id)
}

/// Lets the player wait once, which a freshly spawned monster (no energy)
/// uses to get ready: it first acts during the player's second action.
fn warm_up(game: &mut Game) {
    let events = game.submit(Action::Wait);
    assert!(blows(&events).is_empty(), "nobody strikes yet: {events:?}");
}

/// Expected range of the damage of a blow, from decision D17.
fn damage_range(attack: i32, defense: i32) -> std::ops::RangeInclusive<i32> {
    (attack - defense - 1).max(1)..=(attack - defense + 1).max(1)
}

/// Hit points of the actor `id`, which must be in the game.
fn hp_of(game: &Game, id: ActorId) -> i32 {
    game.actor(id).expect("actor is in the game").hp()
}

/// Every blow of `events` as `(attacker, target, damage, hp_left)`.
fn blows(events: &[Event]) -> Vec<(ActorId, ActorId, i32, i32)> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Attacked {
                attacker,
                target,
                damage,
                hp_left,
            } => Some((*attacker, *target, *damage, *hp_left)),
            _ => None,
        })
        .collect()
}

/// The damage of every blow of `events`, in order.
fn damages(events: &[Event]) -> Vec<i32> {
    blows(events).into_iter().map(|blow| blow.2).collect()
}

// --- Scenario 1: bump to attack, damage and hit points ----------------------

#[test]
fn moving_into_an_adjacent_monster_strikes_it_and_nobody_moves() {
    for seed in named_seeds() {
        let (mut game, brute) = duel(seed, "brute");
        let player = game.player();
        let turns = game.player_turns();

        let events = game.submit(Action::Move(Direction::East));

        let first = events.first().expect("the action reports something");
        let Event::Attacked {
            attacker,
            target,
            damage,
            hp_left,
        } = first
        else {
            panic!("seed {seed}: first event is {first:?}, not a blow");
        };
        assert_eq!((*attacker, *target), (player, brute), "seed {seed}");
        assert!(
            damage_range(PLAYER_ATTACK, 2).contains(damage),
            "seed {seed}: damage {damage}"
        );
        assert_eq!(*hp_left, 40 - damage, "seed {seed}");
        assert_eq!(hp_of(&game, brute), 40 - damage, "seed {seed}");
        assert_eq!(game.player_actor().position(), PLAYER_START, "seed {seed}");
        assert_eq!(game.actor(brute).unwrap().position(), east(), "seed {seed}");
        assert!(
            events
                .iter()
                .all(|e| !matches!(e, Event::Moved { actor, .. } if *actor == player)),
            "seed {seed}: the player moved while attacking"
        );
        assert_eq!(game.player_turns(), turns + 1, "a blow takes a turn");
    }
}

#[test]
fn the_player_blows_follow_the_formula_and_the_monster_loses_exactly_that() {
    let (mut game, brute) = duel(Seed::from(42), "brute");
    let mut seen = HashSet::new();
    let mut remaining = 40;
    for blow in 0..60 {
        if remaining == 0 {
            break;
        }
        let events = game.submit(Action::Move(Direction::East));
        let player_blows: Vec<_> = blows(&events)
            .into_iter()
            .filter(|&(attacker, ..)| attacker == game.player())
            .collect();
        let [(attacker, target, damage, hp_left)] = player_blows[..] else {
            panic!("blow {blow}: expected exactly one player blow, got {events:?}");
        };
        assert_eq!((attacker, target), (game.player(), brute));
        assert!(damage_range(PLAYER_ATTACK, 2).contains(&damage), "{damage}");
        seen.insert(damage);
        remaining = (remaining - damage).max(0);
        assert_eq!(hp_left, remaining, "blow {blow}");
        if remaining > 0 {
            assert_eq!(hp_of(&game, brute), remaining, "blow {blow}");
        }
    }
    assert_eq!(
        remaining, 0,
        "forty hit points are worn down in sixty blows"
    );
    assert!(seen.len() > 1, "the roll never varies: {seen:?}");
}

#[test]
fn a_monster_strikes_the_player_and_the_player_loses_exactly_that() {
    for seed in named_seeds() {
        let (mut game, _gnat) = duel(seed, "gnat");
        warm_up(&mut game);
        let mut expected = PLAYER_HP;
        for turn in 0..10 {
            let events = game.submit(Action::Wait);
            let hits = blows(&events);
            assert_eq!(hits.len(), 1, "seed {seed}, turn {turn}: {events:?}");
            let (attacker, target, damage, hp_left) = hits[0];
            assert_eq!(target, game.player(), "seed {seed}");
            assert_ne!(attacker, game.player(), "seed {seed}");
            // Attack 0 against defense 1: the minimum, 1, whatever the roll.
            assert_eq!(damage, 1, "seed {seed}, turn {turn}");
            expected -= damage;
            assert_eq!(hp_left, expected, "seed {seed}, turn {turn}");
            assert_eq!(
                game.player_actor().hp(),
                expected,
                "seed {seed}, turn {turn}"
            );
            assert_eq!(game.player_actor().max_hp(), PLAYER_HP);
        }
    }
}

#[test]
fn the_damage_of_a_strong_monster_follows_attack_minus_defense() {
    let (mut game, _) = duel(Seed::from(7), "stinger");
    warm_up(&mut game);
    // Attack 29 against defense 1: 27 to 29. Only the first blow is read.
    let events = game.submit(Action::Wait);
    let first = blows(&events)[0];
    assert!(
        damage_range(29, PLAYER_DEFENSE).contains(&first.2),
        "{}",
        first.2
    );
    assert_eq!(first.3, PLAYER_HP - first.2);
}

#[test]
fn two_monsters_never_fight_each_other() {
    let content = test_content();
    let mut game = Game::in_fixed_room(Seed::from(42));
    // Far from the player, side by side; the player waits out of reach.
    let a = game.spawn_monster(Position::new(24, 8), content.monster("gnat").unwrap());
    let b = game.spawn_monster(Position::new(25, 8), content.monster("gnat").unwrap());
    for turn in 0..60 {
        let events = game.submit(Action::Wait);
        for (attacker, target, ..) in blows(&events) {
            assert!(
                target == game.player() || attacker == game.player(),
                "turn {turn}: monsters fought each other"
            );
        }
    }
    assert_eq!(hp_of(&game, a), 999);
    assert_eq!(hp_of(&game, b), 999);
}

// --- Scenario 2: a dead monster disappears ----------------------------------

#[test]
fn a_monster_at_zero_hit_points_dies_and_leaves_the_world() {
    for seed in named_seeds() {
        let (mut game, moth) = duel(seed, "moth");
        let before = game.actors().count();

        let events = game.submit(Action::Move(Direction::East));

        let hits = blows(&events);
        assert_eq!(hits.len(), 1, "seed {seed}: {events:?}");
        let (_, target, damage, hp_left) = hits[0];
        assert_eq!(target, moth);
        assert!(damage > 1, "overkill is reported as dealt: {damage}");
        assert_eq!(hp_left, 0, "hit points never go below zero, seed {seed}");
        let pos = events
            .iter()
            .position(|e| matches!(e, Event::Died { .. }))
            .expect("a death is reported");
        assert!(
            matches!(events[pos - 1], Event::Attacked { target, .. } if target == moth),
            "the death follows the blow"
        );
        let Event::Died {
            actor,
            who,
            killed_by,
        } = &events[pos]
        else {
            unreachable!()
        };
        assert_eq!(*actor, moth);
        assert_eq!(
            *who,
            Combatant::Monster {
                definition: "moth".into()
            }
        );
        assert_eq!(*killed_by, Combatant::Player);

        assert!(
            game.actor(moth).is_none(),
            "seed {seed}: the corpse remains"
        );
        assert_eq!(game.actors().count(), before - 1);
        assert!(
            game.actors().all(|(_, a)| a.position() != east()),
            "its cell is free"
        );
        assert_eq!(
            *game.status(),
            Status::Playing,
            "killing does not end the game"
        );
        assert!(
            events[pos + 1..].iter().all(|e| match e {
                Event::Attacked {
                    attacker, target, ..
                } => *attacker != moth && *target != moth,
                Event::Moved { actor, .. }
                | Event::Waited { actor }
                | Event::MoveBlocked { actor, .. } => *actor != moth,
                Event::Died { actor, .. } => *actor != moth,
                _ => true,
            }),
            "seed {seed}: the dead monster still acts"
        );
    }
}

#[test]
fn after_a_kill_the_cell_can_be_walked_into_and_the_monster_never_acts_again() {
    let (mut game, moth) = duel(Seed::from(42), "moth");
    game.submit(Action::Move(Direction::East));
    assert!(game.actor(moth).is_none());

    let events = game.submit(Action::Move(Direction::East));

    assert!(
        matches!(events[0], Event::Moved { to, .. } if to == east()),
        "the player steps onto the freed cell: {events:?}"
    );
    assert!(blows(&events).is_empty());
    for _ in 0..20 {
        let events = game.submit(Action::Wait);
        assert!(blows(&events).is_empty(), "nobody is left to fight");
    }
}

#[test]
fn a_monster_is_removed_exactly_when_a_blow_leaves_it_at_zero() {
    // Hit points 1 to 12 against the player's blows: with hp <= damage the
    // first blow kills, otherwise the monster survives with hp - damage and
    // is not reported dead.
    for hp in 1..=12 {
        let text = format!(
            r#"[Monster(id: "dummy", name: "dummy", family: Vermin, elite: false,
                speed: 100, hp: {hp}, attack: 0, defense: 0, min_depth: 1)]"#
        );
        let content = Content::parse(&text).expect("valid");
        let mut game = Game::in_fixed_room(Seed::from(42));
        let id = game.spawn_monster(east(), content.monster("dummy").unwrap());
        let mut guard = 0;
        while game.actor(id).is_some() {
            guard += 1;
            assert!(guard <= 15, "hp {hp}: the dummy never dies");
            let before = hp_of(&game, id);
            let events = game.submit(Action::Move(Direction::East));
            let (_, _, damage, hp_left) = blows(&events)[0];
            assert_eq!(hp_left, (before - damage).max(0), "hp {hp}");
            let died = events
                .iter()
                .any(|e| matches!(e, Event::Died { actor, .. } if *actor == id));
            assert_eq!(died, hp_left == 0, "hp {hp}: death reported iff 0 left");
            assert_eq!(game.actor(id).is_none(), hp_left == 0, "hp {hp}");
        }
    }
}

// --- Scenario 3: the player's death ends the game ---------------------------

#[test]
fn the_death_of_the_player_ends_the_game_and_names_the_killer() {
    for seed in named_seeds() {
        let (mut game, executioner) = duel(seed, "executioner");
        let mut all = Vec::new();
        for _ in 0..10 {
            if game.is_over() {
                break;
            }
            all.extend(game.submit(Action::Wait));
        }

        assert!(game.is_over(), "seed {seed}");
        assert_eq!(
            *game.status(),
            Status::GameOver {
                killed_by: Combatant::Monster {
                    definition: "executioner".into()
                }
            }
        );
        assert_eq!(game.player_actor().hp(), 0, "seed {seed}");
        assert!(!game.player_actor().is_alive());
        assert_eq!(game.player_actor().position(), PLAYER_START);
        let Some(Event::Died {
            actor,
            who,
            killed_by,
        }) = all.last()
        else {
            panic!(
                "seed {seed}: the last event is not a death: {:?}",
                all.last()
            );
        };
        assert_eq!(*actor, game.player());
        assert_eq!(*who, Combatant::Player);
        assert_eq!(
            *killed_by,
            Combatant::Monster {
                definition: "executioner".into()
            }
        );
        let (attacker, target, _, hp_left) = *blows(&all).last().unwrap();
        assert_eq!((attacker, target, hp_left), (executioner, game.player(), 0));
    }
}

#[test]
fn nothing_changes_once_the_player_is_dead() {
    let (mut game, _) = duel(Seed::from(42), "executioner");
    for _ in 0..10 {
        if game.is_over() {
            break;
        }
        game.submit(Action::Wait);
    }
    assert!(game.is_over());
    let snapshot = |game: &Game| {
        (
            game.ticks(),
            game.player_turns(),
            game.player_actor().hp(),
            game.player_actor().position(),
            game.actors()
                .map(|(id, a)| (id, a.position(), a.hp(), a.energy()))
                .collect::<Vec<_>>(),
        )
    };
    let before = snapshot(&game);

    let actions = [Action::Wait]
        .into_iter()
        .chain(Direction::ALL.into_iter().map(Action::Move));
    for action in actions {
        for _ in 0..3 {
            assert!(
                game.submit(action).is_empty(),
                "{action:?} reports events after death"
            );
        }
    }

    assert_eq!(snapshot(&game), before);
    assert!(game.is_over());
}

#[test]
fn a_fast_monster_stops_striking_the_moment_the_player_dies() {
    // Speed 400 gives it four actions per player action; two blows of 27 to
    // 29 kill 30 hit points, so the third and fourth must never happen.
    for seed in named_seeds() {
        let (mut game, stinger) = duel(seed, "stinger");
        warm_up(&mut game);
        let events = game.submit(Action::Wait);

        let hits = blows(&events);
        assert_eq!(hits.len(), 2, "seed {seed}: {events:?}");
        assert!(
            hits.iter()
                .all(|&(a, t, ..)| a == stinger && t == game.player())
        );
        assert_eq!(hits[0].3, PLAYER_HP - hits[0].2);
        assert_eq!(
            hits[1].3, 0,
            "seed {seed}: the second blow is the killing one"
        );
        assert!(
            matches!(
                events.last(),
                Some(Event::Died {
                    who: Combatant::Player,
                    ..
                })
            ),
            "seed {seed}: events continue after the death: {events:?}"
        );
        assert!(game.is_over());
    }
}

#[test]
fn only_one_of_two_adjacent_killers_strikes_the_dead_player() {
    let content = test_content();
    let mut game = Game::in_fixed_room(Seed::from(42));
    game.spawn_monster(east(), content.monster("executioner").unwrap());
    game.spawn_monster(
        PLAYER_START.step(Direction::West),
        content.monster("executioner").unwrap(),
    );
    warm_up(&mut game);

    let events = game.submit(Action::Wait);

    assert_eq!(blows(&events).len(), 1, "{events:?}");
    assert!(matches!(
        events.last(),
        Some(Event::Died {
            who: Combatant::Player,
            ..
        })
    ));
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, Event::Died { .. }))
            .count(),
        1,
        "the player dies once"
    );
}

#[test]
fn the_killing_blow_of_a_monster_and_a_player_kill_in_one_turn_stay_consistent() {
    // The player kills the moth; the executioner then kills the player in the
    // same call: both deaths are reported, in order, and the game is over.
    let content = test_content();
    let mut game = Game::in_fixed_room(Seed::from(42));
    game.spawn_monster(
        PLAYER_START.step(Direction::West),
        content.monster("executioner").unwrap(),
    );
    warm_up(&mut game);
    let moth = game.spawn_monster(east(), content.monster("moth").unwrap());

    let events = game.submit(Action::Move(Direction::East));

    let deaths: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::Died { actor, who, .. } => Some((*actor, who.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(deaths.len(), 2, "{events:?}");
    assert_eq!(deaths[0].0, moth);
    assert_eq!(deaths[1].1, Combatant::Player);
    assert!(game.is_over());
    assert!(game.actor(moth).is_none());
}

// --- Coherence of hit points with the events, on real content ---------------

/// Checks one `submit` result against the game: every blow follows the
/// rule, the hit points tracked in `hp` follow the blows, the game state
/// agrees with them, and deaths come exactly after a blow that left 0.
fn check_turn(game: &Game, events: &[Event], hp: &mut HashMap<ActorId, i32>, context: &str) {
    let mut dead: HashSet<ActorId> = HashSet::new();
    for (index, event) in events.iter().enumerate() {
        match event {
            Event::Attacked {
                attacker,
                target,
                damage,
                hp_left,
            } => {
                assert!(!dead.contains(attacker), "{context}: a dead actor strikes");
                assert!(!dead.contains(target), "{context}: a dead actor is struck");
                assert_ne!(
                    attacker == &game.player(),
                    target == &game.player(),
                    "{context}: a blow must involve the player and one monster"
                );
                let attack_stat = stats_of(game, *attacker, events);
                let defense_stat = stats_of(game, *target, events);
                assert!(
                    damage_range(attack_stat.0, defense_stat.1).contains(damage),
                    "{context}: damage {damage} outside the formula for attack {} against defense {}",
                    attack_stat.0,
                    defense_stat.1
                );
                let before = *hp.get(target).expect("target hit points are known");
                assert_eq!(*hp_left, (before - damage).max(0), "{context}: hp_left");
                assert!(*hp_left >= 0, "{context}: negative hit points");
                hp.insert(*target, *hp_left);
                let died_next = matches!(events.get(index + 1), Some(Event::Died { actor, .. }) if actor == target);
                assert_eq!(died_next, *hp_left == 0, "{context}: death iff 0 left");
            }
            Event::Died { actor, .. } => {
                assert_eq!(
                    hp.get(actor),
                    Some(&0),
                    "{context}: died with hit points left"
                );
                assert!(dead.insert(*actor), "{context}: died twice");
            }
            Event::Moved { actor, .. }
            | Event::Waited { actor }
            | Event::MoveBlocked { actor, .. } => {
                assert!(!dead.contains(actor), "{context}: a dead actor acts");
            }
            _ => {}
        }
    }
    // The game agrees with the tracked hit points.
    for (id, actor) in game.actors() {
        assert_eq!(hp.get(&id), Some(&actor.hp()), "{context}: hp of {id:?}");
        assert!(
            actor.hp() > 0 || id == game.player(),
            "{context}: a dead monster is kept"
        );
        assert!(
            actor.hp() <= actor.max_hp(),
            "{context}: hp above the maximum"
        );
    }
    for id in &dead {
        if *id != game.player() {
            assert!(
                game.actor(*id).is_none(),
                "{context}: dead monster still in the game"
            );
        }
    }
    assert_eq!(game.is_over(), dead.contains(&game.player()), "{context}");
}

/// `(attack, defense)` of the actor `id`: from the game while it is there,
/// from the death event otherwise (looked up in the shipped content).
fn stats_of(game: &Game, id: ActorId, events: &[Event]) -> (i32, i32) {
    if let Some(actor) = game.actor(id) {
        return (actor.attack(), actor.defense());
    }
    if id == game.player() {
        return (PLAYER_ATTACK, PLAYER_DEFENSE);
    }
    let who = events
        .iter()
        .find_map(|e| match e {
            Event::Died { actor, who, .. } if *actor == id => Some(who),
            _ => None,
        })
        .expect("a removed actor has a death event");
    match who {
        Combatant::Player => (PLAYER_ATTACK, PLAYER_DEFENSE),
        Combatant::Monster { definition } => {
            let content = shipped();
            let def = content.monster(definition).expect("shipped monster");
            (def.attack(), def.defense())
        }
    }
}

#[test]
fn every_shipped_monster_fought_to_the_end_keeps_hit_points_coherent() {
    let content = shipped();
    for def in content.monsters() {
        for seed in named_seeds() {
            let mut game = Game::in_fixed_room(seed);
            let id = game.spawn_monster(east(), def);
            let mut hp: HashMap<ActorId, i32> = game.actors().map(|(id, a)| (id, a.hp())).collect();
            let mut ended = false;
            for turn in 0..400 {
                let events = game.submit(Action::Move(Direction::East));
                check_turn(
                    &game,
                    &events,
                    &mut hp,
                    &format!("{} seed {seed} turn {turn}", def.id()),
                );
                if game.is_over() || game.actor(id).is_none() {
                    ended = true;
                    break;
                }
            }
            assert!(ended, "{} seed {seed}: the fight never ends", def.id());
        }
    }
}

// --- A walking tester on generated floors ----------------------------------

/// Steps from `from` to every reachable cell, ignoring every actor.
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

/// First step of a shortest walk from `from` to `to`, walking through no
/// other monster cell, or `None`.
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

/// The tester's next action: a step towards the nearest monster (it may
/// look at every actor), or waiting when there is none to reach.
fn next_action(game: &Game) -> Action {
    let player = game.player_actor().position();
    let targets: HashSet<Position> = game
        .actors()
        .filter(|(_, a)| !a.is_player())
        .map(|(_, a)| a.position())
        .collect();
    let reach = distances(game.map(), player);
    targets
        .iter()
        .filter(|cell| reach.contains_key(cell))
        .min_by_key(|cell| (reach[cell], cell.y, cell.x))
        .and_then(|&goal| first_step(game.map(), player, goal, &targets))
        .map_or(Action::Wait, Action::Move)
}

/// Plays up to `turns` turns of the first floor of `seed` with the tester
/// above, checking every turn, and returns every event of the run.
fn play(seed: Seed, turns: usize, check: bool) -> (Vec<Event>, Game) {
    let content = shipped();
    let mut game = Game::new(seed, &content);
    let mut hp: HashMap<ActorId, i32> = game.actors().map(|(id, a)| (id, a.hp())).collect();
    let mut all = Vec::new();
    for turn in 0..turns {
        if game.is_over() {
            break;
        }
        let action = next_action(&game);
        let events = game.submit(action);
        if check {
            check_turn(&game, &events, &mut hp, &format!("seed {seed} turn {turn}"));
        }
        all.extend(events);
    }
    (all, game)
}

#[test]
fn real_play_keeps_every_blow_and_hit_point_coherent() {
    let mut seeds = named_seeds();
    seeds.extend((9000..9010).map(Seed::from));
    let (mut blows_seen, mut kills, mut deaths) = (0, 0, 0);
    for seed in seeds {
        let (events, game) = play(seed, 400, true);
        blows_seen += blows(&events).len();
        kills += events
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    Event::Died {
                        who: Combatant::Monster { .. },
                        ..
                    }
                )
            })
            .count();
        deaths += usize::from(game.is_over());
    }
    assert!(
        blows_seen > 100,
        "only {blows_seen} blows: the test proves little"
    );
    assert!(kills > 10, "only {kills} kills");
    println!("real play: {blows_seen} blows, {kills} kills, {deaths} deaths");
}

// --- Replay -------------------------------------------------------------------

#[test]
fn the_same_seed_and_inputs_replay_the_same_blows_exactly() {
    for seed in named_seeds() {
        let (first, game_a) = play(seed, 500, false);
        let (second, game_b) = play(seed, 500, false);
        assert_eq!(first, second, "seed {seed}: events differ between two runs");
        assert_eq!(damages(&first), damages(&second));
        assert_eq!(game_a.player_actor().hp(), game_b.player_actor().hp());
        assert_eq!(game_a.is_over(), game_b.is_over());
        assert!(
            !damages(&first).is_empty(),
            "seed {seed}: no blow, nothing proved"
        );
    }
}

#[test]
fn a_cloned_game_replays_the_same_blows_as_the_original() {
    let (mut game, _) = duel(Seed::from(42), "brute");
    for _ in 0..3 {
        game.submit(Action::Move(Direction::East));
    }
    let mut twin = game.clone();
    let mut left = Vec::new();
    let mut right = Vec::new();
    for _ in 0..15 {
        left.extend(game.submit(Action::Move(Direction::East)));
        right.extend(twin.submit(Action::Move(Direction::East)));
    }
    assert_eq!(left, right);
}

#[test]
fn another_seed_deals_other_blows() {
    let sequence = |seed: u64| {
        let (mut game, _) = duel(Seed::from(seed), "brute");
        let mut all = Vec::new();
        for _ in 0..30 {
            all.extend(damages(&game.submit(Action::Move(Direction::East))));
        }
        all
    };
    assert_eq!(sequence(42), sequence(42));
    assert_ne!(
        sequence(42),
        sequence(43),
        "the seed does not drive the blows"
    );
    assert_ne!(sequence(0), sequence(42));
}

#[test]
fn blows_do_not_depend_on_unrelated_actions_taken_in_between() {
    // Walls cost no random draw: bumping into the room's north wall (no time
    // passes) between two blows must not change the damage that follows.
    let content = test_content();
    let run = |bumps: bool| {
        let mut game = Game::in_fixed_room(Seed::from(42));
        // Fight at the top row of the room, next to the wall.
        let top = Position::new(5, 1);
        for _ in 0..4 {
            game.submit(Action::Move(Direction::North));
        }
        assert_eq!(game.player_actor().position(), top);
        let id = game.spawn_monster(top.step(Direction::East), content.monster("brute").unwrap());
        let mut all = Vec::new();
        for _ in 0..12 {
            if bumps {
                let events = game.submit(Action::Move(Direction::North));
                assert!(
                    matches!(events[..], [Event::MoveBlocked { .. }]),
                    "{events:?}"
                );
            }
            if game.actor(id).is_none() {
                break;
            }
            all.extend(damages(&game.submit(Action::Move(Direction::East))));
        }
        all
    };
    assert_eq!(run(false), run(true));
}
