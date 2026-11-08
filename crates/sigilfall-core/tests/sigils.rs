//! Acceptance tests of F6 (sigils) on the core crate.
//!
//! Written against the public API only. They check the acceptance scenarios
//! at the level of events and game state: a glyph read joins the grimoire, an
//! exact trace casts and every stroke is paid in time, a trace one node off is
//! unstable and suffers a backlash, an unknown trace fizzles and loses the
//! essence. The screens are checked by the binary's tests.
//!
//! The rules are written out again here from decisions D7, D18 and D19 (25
//! energy per stroke plus 50 per cast, 2 essence per cast, 6 essence at most,
//! a backlash drawn from the gameplay stream) rather than read from the
//! `schedule` or `sigil` constants, so that a silent change of a rule is
//! noticed. The cost of an action is measured, not computed: it is the energy
//! the player had, plus what it gained while the clock ran, minus what it has
//! left. Every loop is bounded.

use std::collections::{HashMap, HashSet, VecDeque};

use sigilfall_core::action::Action;
use sigilfall_core::actor::ActorId;
use sigilfall_core::content::{Content, SigilDef};
use sigilfall_core::event::Event;
use sigilfall_core::game::{Game, PLAYER_START};
use sigilfall_core::generation::FIRST_DEPTH;
use sigilfall_core::geometry::{Direction, Position};
use sigilfall_core::map::Map;
use sigilfall_core::rng::{Purpose, Seed};
use sigilfall_core::sigil::{Backlash, CastQuality, Node, Trace, draw_backlash, edit_distance};

/// Energy per stroke, energy per cast and essence per cast (decision D19).
const STROKE: i32 = 25;
const CAST: i32 = 50;
const ESSENCE_COST: i32 = 2;
const MAX_ESSENCE: i32 = 6;
/// Energy the player gains per tick (normal speed).
const GAIN_PER_TICK: i32 = 100;

/// Creatures whose behaviour is known in advance.
const TEST_MONSTERS: &str = r#"[
    Monster(id: "gnat", name: "cave gnat", family: Vermin, elite: false,
        speed: 100, hp: 999, attack: 0, defense: 0, min_depth: 1),
    Monster(id: "moth", name: "wax moth", family: Vermin, elite: false,
        speed: 100, hp: 1, attack: 0, defense: 0, min_depth: 1),
    Monster(id: "sloth", name: "sloth", family: Vermin, elite: false,
        speed: 1, hp: 999, attack: 0, defense: 0, min_depth: 1),
    Monster(id: "dart", name: "dart", family: Vermin, elite: false,
        speed: 200, hp: 999, attack: 0, defense: 0, min_depth: 1),
]"#;

/// The sigil file shipped with the game and the item file, for test content.
const SIGILS: &str = include_str!("../content/sigils.ron");
const ITEMS: &str = include_str!("../content/items.ron");

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

/// The shipped sigils with the test creatures of [`TEST_MONSTERS`].
fn test_content() -> Content {
    Content::parse_all(TEST_MONSTERS, SIGILS, ITEMS).expect("valid test content")
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

/// The digits of the pattern of the shipped sigil `id`.
fn pattern(id: &str) -> Vec<u8> {
    sigil(id)
        .pattern()
        .nodes()
        .iter()
        .map(|n| n.digit())
        .collect()
}

/// Casts the trace pressed as `digits`, aimed in `direction`.
fn cast(game: &mut Game, digits: &[u8], direction: Option<Direction>) -> Vec<Event> {
    game.submit(Action::Cast {
        trace: pressed(digits),
        direction,
    })
}

/// A fixed-room game of `seed` where the player has read the glyphs of the
/// shipped sigils `ids` (engraved in a row east of the start) and walked
/// back to the start.
fn knowing(seed: Seed, ids: &[&str]) -> Game {
    let mut game = Game::in_fixed_room(seed);
    let mut cell = PLAYER_START;
    for id in ids {
        cell = cell.step(Direction::East);
        game.engrave(cell, &sigil(id));
    }
    for _ in ids {
        game.submit(Action::Move(Direction::East));
    }
    for _ in ids {
        game.submit(Action::Move(Direction::West));
    }
    assert_eq!(game.grimoire().len(), ids.len(), "every glyph was read");
    assert_eq!(game.player_actor().position(), PLAYER_START);
    game
}

/// Puts a creature of the test content at `cell` and returns its key.
fn foe(game: &mut Game, definition: &str, cell: Position) -> ActorId {
    let content = test_content();
    game.spawn_monster(cell, content.monster(definition).expect("a test creature"))
}

/// A cell in view of the start, not adjacent: 4 east, 2 up.
const FOE_CELL: Position = Position::new(9, 3);

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

/// The price of a cast of `strokes` strokes, from decision D19.
const fn price(strokes: usize) -> i32 {
    strokes as i32 * STROKE + CAST
}

/// The `SigilCast` of `events` as `(sigil, quality)`, if there is one.
fn sigil_cast(events: &[Event]) -> Option<(Option<String>, CastQuality)> {
    events.iter().find_map(|event| match event {
        Event::SigilCast { sigil, quality } => Some((sigil.clone(), *quality)),
        _ => None,
    })
}

/// HP of the actor `id`.
fn hp_of(game: &Game, id: ActorId) -> i32 {
    game.actor(id).expect("actor is in the game").hp()
}

// --- Scenario 1: reading a glyph --------------------------------------------

#[test]
fn stepping_on_a_glyph_adds_its_pattern_to_the_grimoire_once() {
    for seed in named_seeds() {
        for def in shipped().sigils() {
            let mut game = Game::in_fixed_room(seed);
            let glyph = PLAYER_START.step(Direction::East);
            game.engrave(glyph, def);
            assert!(game.grimoire().is_empty(), "nothing is known yet");

            let events = game.submit(Action::Move(Direction::East));

            assert!(
                events.contains(&Event::GlyphLearned {
                    sigil: def.id().to_owned(),
                    position: glyph,
                }),
                "seed {seed}, {}: {events:?}",
                def.id()
            );
            assert!(game.grimoire().knows(def.id()));
            assert_eq!(game.grimoire().len(), 1);
            assert_eq!(game.grimoire().sigils()[0].pattern(), def.pattern());

            // Reading the same glyph again teaches nothing and says nothing.
            game.submit(Action::Move(Direction::West));
            let again = game.submit(Action::Move(Direction::East));
            assert!(
                !again
                    .iter()
                    .any(|e| matches!(e, Event::GlyphLearned { .. })),
                "seed {seed}, {}: read twice",
                def.id()
            );
            assert_eq!(game.grimoire().len(), 1);
        }
    }
}

#[test]
fn standing_next_to_or_on_a_glyph_does_not_read_it() {
    let mut game = Game::in_fixed_room(Seed::from(42));
    // Under the player's feet at the start: read only once stepped onto.
    game.engrave(PLAYER_START, &sigil("ember"));
    for _ in 0..3 {
        let events = game.submit(Action::Wait);
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::GlyphLearned { .. }))
        );
    }
    assert!(game.grimoire().is_empty());
    game.submit(Action::Move(Direction::East));
    let events = game.submit(Action::Move(Direction::West));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::GlyphLearned { sigil, .. } if sigil == "ember"))
    );
}

#[test]
fn the_grimoire_keeps_file_order_whatever_the_reading_order() {
    let game = knowing(
        Seed::from(7),
        &["lance", "veil", "ember", "passage", "recall"],
    );
    let ids: Vec<&str> = game.grimoire().sigils().iter().map(SigilDef::id).collect();
    assert_eq!(ids, ["ember", "recall", "veil", "passage", "lance"]);
}

#[test]
fn the_first_floor_of_every_named_seed_holds_glyphs_and_walking_onto_one_teaches_it() {
    let content = shipped();
    for seed in named_seeds() {
        let mut game = Game::new(seed, &content);
        let glyphs: Vec<_> = game.engravings().to_vec();
        assert!(
            !glyphs.is_empty(),
            "seed {seed}: no glyph on the first floor"
        );
        let cells: HashSet<Position> = glyphs.iter().map(|g| g.position()).collect();
        assert_eq!(
            cells.len(),
            glyphs.len(),
            "seed {seed}: two glyphs on a cell"
        );
        for glyph in &glyphs {
            assert!(game.map().is_walkable(glyph.position()), "seed {seed}");
            assert!(
                glyph.sigil().can_appear_at(FIRST_DEPTH),
                "seed {seed}: {} is too deep for floor 1",
                glyph.sigil().id()
            );
        }

        // Walk to the nearest glyph, fighting whatever stands in the way.
        let mut learned = None;
        for _ in 0..800 {
            if game.is_over() || learned.is_some() {
                break;
            }
            let from = game.player_actor().position();
            let dist = distances(game.map(), from);
            let Some(target) = glyphs
                .iter()
                .filter(|g| !game.grimoire().knows(g.sigil().id()))
                .min_by_key(|g| dist.get(&g.position()).copied().unwrap_or(u32::MAX))
            else {
                break;
            };
            let step = first_step(game.map(), from, target.position());
            let events = game.submit(Action::Move(step.expect("the glyph is reachable")));
            learned = events.iter().find_map(|e| match e {
                Event::GlyphLearned { sigil, .. } => Some(sigil.clone()),
                _ => None,
            });
        }
        if game.is_over() {
            panic!("seed {seed}: the player died on the way to a glyph");
        }
        let learned = learned.unwrap_or_else(|| panic!("seed {seed}: no glyph was read"));
        assert!(game.grimoire().knows(&learned), "seed {seed}");
    }
}

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

/// First step of a shortest walk from `from` to `to`, ignoring actors.
fn first_step(map: &Map, from: Position, to: Position) -> Option<Direction> {
    let mut first: HashMap<Position, Direction> = HashMap::new();
    let mut seen = HashSet::from([from]);
    let mut queue = VecDeque::from([from]);
    while let Some(cell) = queue.pop_front() {
        for direction in Direction::ALL {
            let next = cell.step(direction);
            if !map.can_step(cell, direction) || seen.contains(&next) {
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

// --- Scenario 2: an exact trace succeeds and every stroke costs time -------

#[test]
fn every_shipped_sigil_traced_exactly_succeeds_at_the_price_of_its_strokes() {
    // (sigil, strokes, where the foe stands, direction given)
    let cases: [(&str, usize, Option<Direction>); 5] = [
        ("ember", 3, None),
        ("recall", 4, None),
        ("veil", 5, None),
        ("passage", 5, Some(Direction::East)),
        ("lance", 4, Some(Direction::East)),
    ];
    for seed in named_seeds() {
        for (id, strokes, direction) in cases {
            let mut game = knowing(seed, &[id]);
            // In sight, off the east line (for the aimed ones the line is
            // empty: Passage needs no target and Lance may miss, both pay).
            foe(&mut game, "gnat", FOE_CELL);
            let before = clock(&game);
            let essence = game.player_actor().essence();
            let digits = pattern(id);
            assert_eq!(digits.len(), strokes, "{id}: pattern length");

            let events = cast(&mut game, &digits, direction);

            let (cast_id, quality) = sigil_cast(&events).expect("a cast is reported");
            assert_eq!(cast_id.as_deref(), Some(id), "seed {seed}");
            assert_eq!(quality, CastQuality::Pure, "seed {seed}, {id}");
            assert!(
                !events.iter().any(|e| matches!(e, Event::Backlash { .. })),
                "seed {seed}, {id}: a pure cast has no backlash"
            );
            assert_eq!(
                spent(&game, before),
                price(strokes),
                "seed {seed}, {id}: {strokes} strokes of {STROKE} plus {CAST}"
            );
            assert_eq!(
                game.player_actor().essence(),
                essence - ESSENCE_COST,
                "seed {seed}, {id}"
            );
        }
    }
}

#[test]
fn ember_wounds_the_foe_in_sight_by_exactly_its_power() {
    for seed in named_seeds() {
        let mut game = knowing(seed, &["ember"]);
        let gnat = foe(&mut game, "gnat", FOE_CELL);

        let events = cast(&mut game, &[7, 5, 3], None);

        assert!(
            events.contains(&Event::Wounded {
                actor: gnat,
                damage: 6,
                hp_left: 993
            }),
            "seed {seed}: {events:?}"
        );
        assert_eq!(hp_of(&game, gnat), 993);
    }
}

#[test]
fn one_more_node_always_costs_a_quarter_of_a_turn_up_to_the_nine_nodes() {
    // 1-2-3-6-5-4-7-8-9 links the grid with no stroke over a middle node.
    let snake: [u8; 9] = [1, 2, 3, 6, 5, 4, 7, 8, 9];
    let mut last = None;
    for strokes in 1..=9usize {
        let mut game = knowing(Seed::from(42), &[]);
        let before = clock(&game);
        let events = cast(&mut game, &snake[..strokes], None);
        assert_eq!(sigil_cast(&events).map(|c| c.1), Some(CastQuality::Fizzled));
        let cost = spent(&game, before);
        assert_eq!(cost, price(strokes), "{strokes} strokes");
        if let Some(previous) = last {
            assert_eq!(cost - previous, STROKE, "node {strokes} adds a stroke");
        }
        last = Some(cost);
    }
}

#[test]
fn a_stroke_over_a_middle_node_pays_for_the_node_it_links_on_the_way() {
    // (digits pressed, strokes actually linked)
    let cases: [(&[u8], usize); 4] = [
        (&[7, 3], 3),    // 7-5-3
        (&[1, 9], 3),    // 1-5-9
        (&[1, 3, 9], 5), // 1-2-3-6-9
        (&[5, 4, 6], 3), // 5 first: 4 to 6 crosses nothing new
    ];
    for (digits, strokes) in cases {
        let mut game = knowing(Seed::from(42), &[]);
        let before = clock(&game);
        cast(&mut game, digits, None);
        assert_eq!(spent(&game, before), price(strokes), "{digits:?}");
    }
}

#[test]
fn a_long_cast_gives_a_fast_monster_more_turns_than_a_short_cast_or_a_wait() {
    // A creature of speed 200 far out of sight and unaware of the player
    // waits on each of its turns, so its `Waited` events count the time that
    // passed. Three identical actions use up the six essence of the player.
    let dart_turns = |action: &dyn Fn(&mut Game) -> Vec<Event>| {
        let mut game = knowing(Seed::from(42), &[]);
        let dart = foe(&mut game, "dart", Position::new(27, 8));
        game.submit(Action::Wait);
        let mut turns = 0;
        for _ in 0..3 {
            let events = action(&mut game);
            turns += events
                .iter()
                .filter(|e| **e == Event::Waited { actor: dart })
                .count();
        }
        turns
    };
    let waiting = dart_turns(&|game| game.submit(Action::Wait));
    let short = dart_turns(&|game| cast(game, &[1, 2, 3], None));
    let long = dart_turns(&|game| cast(game, &[1, 2, 3, 6, 5], None));

    // 3 x 125 and 3 x 175 energy: the dart acts twice per 100 energy.
    assert!(short > waiting, "short {short}, wait {waiting}");
    assert!(long > short, "long {long}, short {short}");
    assert!(
        long >= short + 3,
        "50 energy more, three times: {long} vs {short}"
    );
}

#[test]
fn a_long_cast_lets_an_adjacent_monster_strike_more_often() {
    // The gnat deals exactly 1 per blow, so the player's lost hit points
    // are its blows. Three casts of 5 strokes against three of 3.
    let blows = |digits: &[u8]| {
        let mut game = knowing(Seed::from(42), &[]);
        foe(&mut game, "gnat", PLAYER_START.step(Direction::East));
        game.submit(Action::Wait);
        let start = game.player_actor().hp();
        for _ in 0..3 {
            cast(&mut game, digits, None);
        }
        start - game.player_actor().hp()
    };
    let short = blows(&[1, 2, 3]);
    let long = blows(&[1, 2, 3, 6, 5]);
    assert!(long > short, "long {long} blows, short {short}");
}

// --- Scenario 3: a trace one node off is unstable ---------------------------

/// Every kind of trace one edit away from the pattern of ember (7-5-3): a
/// node replaced at the end, in the middle and at the start, one removed,
/// one added.
const EMBER_NEAR_MISSES: [&[u8]; 5] = [&[7, 5, 2], &[7, 8, 3], &[4, 5, 3], &[7, 5], &[7, 5, 3, 6]];

/// Number of strokes the digits pressed link, middle nodes included.
fn strokes_of(digits: &[u8]) -> usize {
    pressed(digits).len()
}

/// Number of blows struck among `events`.
fn blows_in(events: &[Event]) -> i32 {
    let blows = events
        .iter()
        .filter(|e| matches!(e, Event::Attacked { .. }))
        .count();
    i32::try_from(blows).unwrap()
}

#[test]
fn a_trace_one_node_off_casts_the_sigil_unstable_with_its_effect_then_a_backlash() {
    for seed in named_seeds() {
        for digits in EMBER_NEAR_MISSES {
            let mut game = knowing(seed, &["ember"]);
            let gnat = foe(&mut game, "gnat", FOE_CELL);
            let player = game.player();
            let before = clock(&game);

            let events = cast(&mut game, digits, None);

            assert_eq!(
                sigil_cast(&events),
                Some((Some("ember".to_owned()), CastQuality::Unstable)),
                "seed {seed}, {digits:?}"
            );
            // Order: the cast, its effect, the backlash, its consequence.
            let at = |wanted: &dyn Fn(&Event) -> bool| events.iter().position(wanted);
            let cast_at = at(&|e| matches!(e, Event::SigilCast { .. })).unwrap();
            let effect_at =
                at(&|e| matches!(e, Event::Wounded { actor, .. } if *actor == gnat)).unwrap();
            let backlash_at =
                at(&|e| matches!(e, Event::Backlash { caster, .. } if *caster == player)).unwrap();
            assert!(cast_at < effect_at && effect_at < backlash_at, "{events:?}");
            assert_eq!(hp_of(&game, gnat), 993, "the effect is not weakened");

            let Event::Backlash { backlash, .. } = events[backlash_at] else {
                unreachable!()
            };
            let consequence = &events[backlash_at + 1..];
            // A dazed player also loses a turn inside the same submit, which
            // costs the energy of a wait on top of the cast.
            let dazed_turn = if matches!(backlash, Backlash::Daze) {
                100
            } else {
                0
            };
            assert_eq!(
                spent(&game, before),
                price(strokes_of(digits)) + dazed_turn,
                "seed {seed}, {digits:?}"
            );
            match backlash {
                Backlash::Scorch => {
                    assert!(
                        consequence.contains(&Event::Wounded {
                            actor: player,
                            damage: 2,
                            hp_left: 28
                        }),
                        "{consequence:?}"
                    );
                    assert_eq!(game.player_actor().hp(), 28 - blows_in(consequence));
                }
                Backlash::Drain => {
                    assert!(
                        consequence.contains(&Event::EssenceDrained {
                            actor: player,
                            lost: MAX_ESSENCE - ESSENCE_COST
                        }),
                        "{consequence:?}"
                    );
                    assert_eq!(game.player_actor().essence(), 0);
                }
                Backlash::Daze => {
                    assert!(
                        consequence.contains(&Event::Held {
                            actor: player,
                            turns: 1
                        }),
                        "{consequence:?}"
                    );
                    assert!(
                        consequence.contains(&Event::TurnLost { actor: player }),
                        "the dazed turn is lost: {consequence:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn the_backlash_of_the_first_unstable_cast_is_the_one_the_seed_draws() {
    // No creature, so nothing else uses the gameplay stream: the backlash is
    // the first draw of the stream of the seed, the same on every replay.
    let mut seen = HashSet::new();
    for value in 0..60 {
        let seed = Seed::from(value);
        let expected = draw_backlash(&mut seed.stream(FIRST_DEPTH, Purpose::Gameplay));
        let mut game = knowing(seed, &["ember"]);
        let events = cast(&mut game, &[7, 5, 2], None);
        let backlash = events
            .iter()
            .find_map(|e| match e {
                Event::Backlash { backlash, .. } => Some(*backlash),
                _ => None,
            })
            .expect("an unstable cast suffers a backlash");
        assert_eq!(backlash, expected, "seed {seed}");
        seen.insert(backlash);
    }
    assert_eq!(seen.len(), 3, "60 seeds draw every backlash: {seen:?}");
}

#[test]
fn a_scorching_backlash_leaves_at_least_one_hit_point() {
    // Bring the player to 1 hit point with a moth that deals 1 per blow,
    // kill the moth, then cast unstable on a seed whose first backlash is a
    // scorch.
    let mut found = 0;
    for value in 0..60 {
        let seed = Seed::from(value);
        if draw_backlash(&mut seed.stream(FIRST_DEPTH, Purpose::Gameplay)) != Backlash::Scorch {
            continue;
        }
        let mut game = knowing(seed, &["ember"]);
        foe(&mut game, "moth", PLAYER_START.step(Direction::North));
        let mut waits = 0;
        while game.player_actor().hp() > 1 && waits < 60 {
            game.submit(Action::Wait);
            waits += 1;
        }
        assert_eq!(game.player_actor().hp(), 1, "seed {seed}");
        game.submit(Action::Move(Direction::North));
        assert!(
            !game.is_over(),
            "seed {seed}: the moth struck the last blow"
        );
        let events = cast(&mut game, &[7, 5, 2], None);
        assert!(
            !game.is_over(),
            "seed {seed}: the backlash killed the player: {events:?}"
        );
        assert!(game.player_actor().hp() >= 1, "seed {seed}");
        found += 1;
    }
    assert!(found > 0, "no scorch was exercised");
}

// --- Scenario 4: an unknown trace fizzles and the essence is lost -----------

#[test]
fn a_trace_that_matches_nothing_fizzles_alone_and_loses_essence_and_time() {
    // Further than one edit from every shipped pattern.
    let unknown: [&[u8]; 3] = [&[2, 4, 6, 8, 1], &[9, 8, 7], &[3, 6, 9, 8, 7]];
    let content = shipped();
    for digits in unknown {
        for def in content.sigils() {
            assert!(
                edit_distance(pressed(digits).nodes(), def.pattern().nodes()) >= 2,
                "{digits:?} is too close to {}",
                def.id()
            );
        }
    }
    for seed in named_seeds() {
        for digits in unknown {
            let mut game = knowing(seed, &["ember", "recall", "veil", "passage", "lance"]);
            let gnat = foe(&mut game, "gnat", FOE_CELL);
            let before = clock(&game);

            let events = cast(&mut game, digits, Some(Direction::East));
            assert_eq!(
                events.first(),
                Some(&Event::SigilCast {
                    sigil: None,
                    quality: CastQuality::Fizzled
                }),
                "seed {seed}, {digits:?}"
            );
            // Nothing but the monsters' own turns follows: no effect, no
            // backlash, no refusal.
            assert!(
                events[1..].iter().all(|e| matches!(
                    e,
                    Event::Moved { .. } | Event::Waited { .. } | Event::Attacked { .. }
                )),
                "seed {seed}, {digits:?}: a fizzle reports nothing else: {events:?}"
            );
            assert_eq!(game.player_actor().essence(), MAX_ESSENCE - ESSENCE_COST);
            assert_eq!(spent(&game, before), price(strokes_of(digits)));
            assert_eq!(hp_of(&game, gnat), 999, "nothing was hurt");
        }
    }
}

#[test]
fn a_valid_sigil_not_learnt_yet_fizzles_even_when_traced_exactly_or_one_off() {
    for seed in named_seeds() {
        // Nothing known: ember exact and ember one off both fizzle.
        for digits in [&[7u8, 5, 3][..], &[7, 5, 2][..]] {
            let mut game = knowing(seed, &[]);
            let events = cast(&mut game, digits, None);
            assert_eq!(
                sigil_cast(&events),
                Some((None, CastQuality::Fizzled)),
                "seed {seed}, {digits:?}"
            );
            assert!(!events.iter().any(|e| matches!(e, Event::Backlash { .. })));
        }
        // Another sigil known: still ember that is missing.
        let mut game = knowing(seed, &["recall"]);
        let events = cast(&mut game, &[7, 5, 3], None);
        assert_eq!(sigil_cast(&events), Some((None, CastQuality::Fizzled)));
        // Once its glyph is read, the same trace succeeds.
        let mut game = knowing(seed, &["recall", "ember"]);
        foe(&mut game, "gnat", FOE_CELL);
        let events = cast(&mut game, &[7, 5, 3], None);
        assert_eq!(
            sigil_cast(&events),
            Some((Some("ember".to_owned()), CastQuality::Pure))
        );
    }
}

// --- Essence ----------------------------------------------------------------

#[test]
fn without_essence_a_cast_is_refused_and_nothing_is_spent() {
    for seed in named_seeds() {
        let mut game = knowing(seed, &["ember"]);
        foe(&mut game, "gnat", FOE_CELL);
        // Spend the essence with fizzles (they cost the same as a success).
        let mut refused = None;
        for _ in 0..8 {
            let before = (
                clock(&game),
                game.player_turns(),
                game.player_actor().hp(),
                game.player_actor().position(),
            );
            let events = cast(&mut game, &[9, 8, 7], None);
            if let [Event::NotEnoughEssence { essence, cost }] = events[..] {
                assert_eq!(cost, ESSENCE_COST, "seed {seed}");
                assert!(essence < ESSENCE_COST, "seed {seed}: essence {essence}");
                assert_eq!(clock(&game), before.0, "no energy, no tick");
                assert_eq!(game.player_turns(), before.1, "no turn");
                assert_eq!(game.player_actor().hp(), before.2, "no monster acted");
                assert_eq!(game.player_actor().position(), before.3);
                assert_eq!(game.player_actor().essence(), essence, "nothing taken");
                refused = Some(essence);
                break;
            }
            assert_eq!(sigil_cast(&events).map(|c| c.1), Some(CastQuality::Fizzled));
        }
        let essence = refused.unwrap_or_else(|| panic!("seed {seed}: never refused"));
        // A refused exact trace of a known sigil is refused the same way.
        let events = cast(&mut game, &[7, 5, 3], None);
        assert_eq!(
            events,
            [Event::NotEnoughEssence {
                essence,
                cost: ESSENCE_COST
            }]
        );
        // Time passing gives the essence back, one point per 12 ticks.
        for _ in 0..24 {
            game.submit(Action::Wait);
        }
        let events = cast(&mut game, &[7, 5, 3], None);
        assert_eq!(
            sigil_cast(&events).map(|c| c.1),
            Some(CastQuality::Pure),
            "seed {seed}: 24 ticks later the cast goes through"
        );
    }
}

// --- Aimed sigils -----------------------------------------------------------

/// The cell the player stands on in the aimed tests and the cell 4 steps
/// away from it along `direction`.
fn from_the_middle(direction: Direction) -> (Position, Position) {
    let start = Position::new(10, 5);
    let mut end = start;
    for _ in 0..4 {
        end = end.step(direction);
    }
    (start, end)
}

/// A game where the player stands on (10, 5), having read the glyph of
/// `id` engraved there.
fn standing_in_the_middle(seed: Seed, id: &str) -> Game {
    let mut game = Game::in_fixed_room(seed);
    game.engrave(Position::new(10, 5), &sigil(id));
    for _ in 0..5 {
        game.submit(Action::Move(Direction::East));
    }
    assert_eq!(game.player_actor().position(), Position::new(10, 5));
    assert!(game.grimoire().knows(id));
    game
}

#[test]
fn passage_needs_a_direction_and_carries_the_player_four_cells_that_way() {
    for direction in Direction::ALL {
        let mut game = standing_in_the_middle(Seed::from(42), "passage");
        let (start, end) = from_the_middle(direction);
        let essence = game.player_actor().essence();

        // Without a direction: refused, nothing spent.
        let before = clock(&game);
        let events = cast(&mut game, &[7, 4, 1, 2, 3], None);
        assert_eq!(
            events,
            [Event::DirectionMissing {
                sigil: "passage".to_owned()
            }]
        );
        assert_eq!(clock(&game), before);
        assert_eq!(game.player_actor().essence(), essence);

        let events = cast(&mut game, &[7, 4, 1, 2, 3], Some(direction));
        assert!(
            events.contains(&Event::Blinked {
                actor: game.player(),
                from: start,
                to: end
            }),
            "{direction:?}: {events:?}"
        );
        assert_eq!(game.player_actor().position(), end, "{direction:?}");
    }
}

#[test]
fn passage_stops_at_the_wall_and_is_still_paid() {
    let mut game = standing_in_the_middle(Seed::from(42), "passage");
    // Walk towards the west wall: x = 1 is the last floor column.
    for _ in 0..8 {
        game.submit(Action::Move(Direction::West));
    }
    assert_eq!(game.player_actor().position().x, 2);
    let before = clock(&game);
    let events = cast(&mut game, &[7, 4, 1, 2, 3], Some(Direction::West));
    assert_eq!(game.player_actor().position().x, 1, "one cell, not four");
    assert!(events.iter().any(|e| matches!(e, Event::Blinked { .. })));
    assert_eq!(spent(&game, before), price(5));
}

#[test]
fn lance_hits_the_first_creature_on_the_line_in_each_of_the_eight_directions() {
    for direction in Direction::ALL {
        let mut game = standing_in_the_middle(Seed::from(42), "lance");
        let mut near = Position::new(10, 5);
        for _ in 0..3 {
            near = near.step(direction);
        }
        let target = foe(&mut game, "gnat", near);
        // A second creature behind the first one, which must stay untouched.
        let behind = foe(&mut game, "gnat", near.step(direction));
        let before = clock(&game);

        let events = cast(&mut game, &[1, 5, 9, 6], Some(direction));

        assert!(
            events.contains(&Event::Wounded {
                actor: target,
                damage: 9,
                hp_left: 990
            }),
            "{direction:?}: {events:?}"
        );
        assert_eq!(hp_of(&game, behind), 999, "{direction:?}: hit through");
        assert_eq!(spent(&game, before), price(4));
    }
}

#[test]
fn lance_on_an_empty_line_misses_but_is_paid() {
    let mut game = standing_in_the_middle(Seed::from(42), "lance");
    let gnat = foe(&mut game, "gnat", Position::new(12, 8));
    let essence = game.player_actor().essence();
    let before = clock(&game);

    let events = cast(&mut game, &[1, 5, 9, 6], Some(Direction::East));

    assert!(
        events.contains(&Event::SpellMissed {
            sigil: "lance".to_owned()
        }),
        "{events:?}"
    );
    assert_eq!(hp_of(&game, gnat), 999);
    assert_eq!(game.player_actor().essence(), essence - ESSENCE_COST);
    assert_eq!(spent(&game, before), price(4));
}

#[test]
fn recall_heals_what_is_missing_and_never_more() {
    let mut game = knowing(Seed::from(42), &["recall"]);
    let moth = foe(&mut game, "moth", PLAYER_START.step(Direction::North));
    game.submit(Action::Wait);
    game.submit(Action::Wait);
    // Kill the moth so that it does not strike during the cast.
    game.submit(Action::Move(Direction::North));
    assert!(game.actor(moth).is_none(), "the moth is dead");
    let missing = 30 - game.player_actor().hp();
    assert!(missing >= 1, "the moth struck at least once");

    let events = cast(&mut game, &[8, 5, 2, 1], None);

    let healed = events.iter().find_map(|e| match e {
        Event::Healed { amount, hp, .. } => Some((*amount, *hp)),
        _ => None,
    });
    assert_eq!(healed, Some((missing.min(8), 30)), "{events:?}");
    assert_eq!(game.player_actor().hp(), 30);
}

// --- Edge cases of the trace ------------------------------------------------

#[test]
fn an_empty_trace_that_reached_the_core_is_never_a_sigil() {
    // The binary never submits one (the overlay refuses it); should it
    // happen, the game must not treat it as a sigil of any kind.
    let mut game = knowing(Seed::from(42), &["ember", "recall"]);
    let events = game.submit(Action::Cast {
        trace: Trace::new(),
        direction: None,
    });
    if let Some((id, quality)) = sigil_cast(&events) {
        assert_eq!(id, None, "an empty trace is no sigil");
        assert_eq!(quality, CastQuality::Fizzled);
    }
}

// --- Determinism ------------------------------------------------------------

/// What a game shows of itself after a step, for comparing two replays.
#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    hp: i32,
    essence: i32,
    position: Position,
    ticks: u64,
    turns: u64,
    known: Vec<String>,
    over: bool,
}

/// The snapshot of `game`.
fn snapshot(game: &Game) -> Snapshot {
    Snapshot {
        hp: game.player_actor().hp(),
        essence: game.player_actor().essence(),
        position: game.player_actor().position(),
        ticks: game.ticks(),
        turns: game.player_turns(),
        known: game
            .grimoire()
            .sigils()
            .iter()
            .map(|s| s.id().to_owned())
            .collect(),
        over: game.is_over(),
    }
}

/// The scripted actions of a replay: a fixed pseudo-random mix of waits,
/// steps, exact casts, near misses and unknown traces, drawn from a small
/// generator of this test, never from the game's own streams.
fn script(length: usize) -> Vec<Action> {
    let traces: [(&[u8], Option<Direction>); 9] = [
        (&[7, 5, 3], None),
        (&[7, 5, 2], None),
        (&[8, 5, 2, 1], None),
        (&[8, 5, 2], None),
        (&[4, 7, 8, 9, 6], None),
        (&[7, 4, 1, 2, 3], Some(Direction::East)),
        (&[1, 5, 9, 6], Some(Direction::East)),
        (&[1, 5, 9], Some(Direction::East)),
        (&[2, 4, 6, 8, 1], None),
    ];
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        usize::try_from(state >> 33).unwrap()
    };
    (0..length)
        .map(|_| match next() % 4 {
            0 => Action::Move(Direction::ALL[next() % 8]),
            1 | 2 => Action::Wait,
            _ => {
                let (digits, direction) = traces[next() % traces.len()];
                Action::Cast {
                    trace: pressed(digits),
                    direction,
                }
            }
        })
        .collect()
}

/// Plays `actions` on a fixed-room game of `seed` where every sigil is known
/// and a gnat stands in sight; returns the events and snapshot after each.
fn replay(seed: Seed, actions: &[Action]) -> Vec<(Vec<Event>, Snapshot)> {
    let mut game = knowing(seed, &["ember", "recall", "veil", "passage", "lance"]);
    foe(&mut game, "sloth", FOE_CELL);
    actions
        .iter()
        .map(|&action| {
            let events = game.submit(action);
            (events, snapshot(&game))
        })
        .collect()
}

#[test]
fn the_same_seed_and_inputs_replay_the_same_casts_and_backlashes() {
    let actions = script(400);
    let mut backlashes = HashSet::new();
    for seed in named_seeds() {
        let first = replay(seed, &actions);
        let second = replay(seed, &actions);
        assert_eq!(first, second, "seed {seed}: the replay diverged");
        for (events, _) in &first {
            for event in events {
                if let Event::Backlash { backlash, .. } = event {
                    backlashes.insert(*backlash);
                }
            }
        }
        let casts = first
            .iter()
            .flat_map(|(events, _)| events)
            .filter(|e| matches!(e, Event::SigilCast { .. }))
            .count();
        assert!(casts >= 5, "seed {seed}: only {casts} casts in the script");
    }
    assert!(
        !backlashes.is_empty(),
        "the script never suffered a backlash"
    );
}

#[test]
fn real_floors_replay_the_same_with_casts_in_the_script() {
    let content = shipped();
    let actions = script(300);
    for seed in named_seeds() {
        let run = || {
            let mut game = Game::new(seed, &content);
            actions
                .iter()
                .map(|&action| {
                    let events = game.submit(action);
                    (events, snapshot(&game))
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(run(), run(), "seed {seed}: the replay diverged");
    }
}
