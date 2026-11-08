//! The run's statistics counted by the game for the death summary.
//!
//! A scripted game in the fixed room checks each count of `Game::stats`
//! against what was done: creatures killed by kind, casts by quality, a
//! refused cast left out, glyphs learnt apart from pages, items used, turns.
//! A generated floor checks the deepest floor reached, and long reckless
//! games on several seeds check that the counts always equal a tally of the
//! events `Game::submit` returned (decision D4). Written against the public
//! API only.

use std::collections::BTreeMap;

use rand::RngExt as _;

use sigilfall_core::action::Action;
use sigilfall_core::combat::Combatant;
use sigilfall_core::content::Content;
use sigilfall_core::event::Event;
use sigilfall_core::game::{Game, PLAYER_START};
use sigilfall_core::generation::{self, FIRST_DEPTH};
use sigilfall_core::geometry::{Direction, Position};
use sigilfall_core::map::Tile;
use sigilfall_core::path::find_path;
use sigilfall_core::rng::{Purpose, Seed};
use sigilfall_core::sigil::{CastQuality, Node, Trace};
use sigilfall_core::stats::CastCounts;

/// Creatures that die from any blow of the player and never hurt it much:
/// two kinds, so kills are counted apart.
const TEST_MONSTERS: &str = r#"[
    Monster(id: "moth", name: "wax moth", family: Vermin, elite: false,
        speed: 100, hp: 1, attack: 0, defense: 0, min_depth: 1),
    Monster(id: "mite", name: "dust mite", family: Vermin, elite: false,
        speed: 100, hp: 1, attack: 0, defense: 0, min_depth: 1),
]"#;

/// The sigil and item files shipped with the game.
const SIGILS: &str = include_str!("../content/sigils.ron");
const ITEMS: &str = include_str!("../content/items.ron");

/// Pattern of the shipped sigil `recall` (heal the caster), cast with no
/// target and no direction.
const RECALL: [u8; 4] = [8, 5, 2, 1];
/// One node short of [`RECALL`]: an unstable cast of it.
const RECALL_UNSTABLE: [u8; 3] = [8, 5, 2];
/// Three edits away from [`RECALL`], the only sigil known: it fizzles.
const UNKNOWN_TRACE: [u8; 3] = [7, 5, 3];

/// Seeds of the long games.
const SEEDS: [u64; 5] = [0, 3, 42, 99, 1234];
/// Most player turns a long game is played for.
const MAX_TURNS: u64 = 1500;

/// The shipped sigils and items with the test creatures.
fn test_content() -> Content {
    Content::parse_all(TEST_MONSTERS, SIGILS, ITEMS).expect("valid test content")
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

/// The cast of the trace `digits`, with no direction.
fn cast_action(digits: &[u8]) -> Action {
    Action::Cast {
        trace: pressed(digits),
        direction: None,
    }
}

/// Every cell of a `width` × `height` map, row by row.
fn cells(width: usize, height: usize) -> impl Iterator<Item = Position> {
    let width = i32::try_from(width).expect("a map fits in i32");
    let height = i32::try_from(height).expect("a map fits in i32");
    (0..height).flat_map(move |y| (0..width).map(move |x| Position::new(x, y)))
}

/// The action of a reckless player: charge the nearest visible monster, or
/// go down the stairs, walking there first.
fn reckless_action(game: &Game) -> Action {
    let map = game.map();
    let me = game.player_actor().position();
    let monsters: Vec<Position> = game
        .actors()
        .filter(|(_, actor)| !actor.is_player())
        .map(|(_, actor)| actor.position())
        .collect();
    let occupied = |cell: Position| monsters.contains(&cell);
    let charge = game
        .visible_monsters()
        .filter_map(|monster| find_path(map, me, monster.position, occupied))
        .min_by_key(Vec::len);
    if let Some(&direction) = charge.as_ref().and_then(|path| path.first()) {
        return Action::Move(direction);
    }
    if map.tile(me) == Some(Tile::StairsDown) {
        return Action::Descend;
    }
    cells(map.width(), map.height())
        .find(|&cell| map.tile(cell) == Some(Tile::StairsDown))
        .and_then(|stairs| {
            find_path(map, me, stairs, occupied).or_else(|| find_path(map, me, stairs, |_| false))
        })
        .and_then(|path| path.first().copied())
        .map_or(Action::Wait, Action::Move)
}

/// The counts the summary needs, tallied by the test itself from events.
#[derive(Debug, Default, PartialEq, Eq)]
struct Tally {
    kills: BTreeMap<String, u32>,
    casts: CastCounts,
    glyphs: u32,
    items: u32,
    deepest: u32,
}

impl Tally {
    /// Adds what `events` report.
    fn add(&mut self, events: &[Event]) {
        for event in events {
            match event {
                Event::Died {
                    who: Combatant::Monster { definition },
                    ..
                } => *self.kills.entry(definition.clone()).or_default() += 1,
                Event::SigilCast {
                    quality: CastQuality::Pure,
                    ..
                } => self.casts.pure += 1,
                Event::SigilCast {
                    quality: CastQuality::Unstable,
                    ..
                } => self.casts.unstable += 1,
                Event::SigilCast {
                    quality: CastQuality::Fizzled,
                    ..
                } => self.casts.fizzled += 1,
                Event::GlyphLearned { .. } => self.glyphs += 1,
                Event::Used { .. } => self.items += 1,
                Event::Descended { depth } => self.deepest = self.deepest.max(*depth),
                _ => {}
            }
        }
    }

    /// The same counts, read from the game.
    fn of(game: &Game) -> Self {
        let stats = game.stats();
        Self {
            kills: stats
                .kills()
                .map(|(definition, count)| (definition.to_owned(), count))
                .collect(),
            casts: stats.casts(),
            glyphs: stats.glyphs_learnt(),
            items: stats.items_used(),
            deepest: stats.deepest(),
        }
    }
}

#[test]
fn a_scripted_game_counts_each_thing_the_player_did() {
    let content = test_content();
    let mut game = Game::in_fixed_room(Seed::new(5));
    let east = PLAYER_START.step(Direction::East);
    game.engrave(east, content.sigil("recall").unwrap());
    game.place_item(
        east.step(Direction::East),
        content.item("healing_draught").unwrap(),
    );
    game.place_item(
        east.step(Direction::East).step(Direction::East),
        content.item("veil_page").unwrap(),
    );
    // Turns are counted here from the actions that took time: those after
    // which the monsters had their turn, seen on the clock.
    let mut turns = 0;
    let mut act = |game: &mut Game, action: Action| {
        let before = game.ticks();
        let events = game.submit(action);
        turns += u64::from(game.ticks() != before);
        events
    };

    // A glyph read, two items picked up.
    act(&mut game, Action::Move(Direction::East));
    act(&mut game, Action::Move(Direction::East));
    act(&mut game, Action::PickUp);
    act(&mut game, Action::Move(Direction::East));
    act(&mut game, Action::PickUp);
    // A move into the wall above is refused: no turn.
    for _ in 0..10 {
        act(&mut game, Action::Move(Direction::North));
    }
    assert_eq!(game.stats().glyphs_learnt(), 1);

    // Pure, unstable, fizzled: all the essence of the start (6, 2 per cast),
    // so a fourth cast right away is refused and does not count.
    for digits in [&RECALL[..], &RECALL_UNSTABLE, &UNKNOWN_TRACE] {
        let events = act(&mut game, cast_action(digits));
        assert!(
            events
                .iter()
                .any(|event| matches!(event, Event::SigilCast { .. })),
            "{digits:?}: {events:?}"
        );
    }
    let refused = act(&mut game, cast_action(&RECALL));
    assert!(
        matches!(refused.as_slice(), [Event::NotEnoughEssence { .. }]),
        "{refused:?}"
    );
    assert_eq!(
        game.stats().casts(),
        CastCounts {
            pure: 1,
            unstable: 1,
            fizzled: 1
        }
    );

    // Both items used; the page teaches a sigil but is not a glyph.
    let carried: Vec<_> = game.inventory().map(|(id, _)| id).collect();
    assert_eq!(carried.len(), 2);
    for item in carried {
        act(&mut game, Action::Use(item));
    }
    assert!(game.grimoire().knows("veil"));
    assert_eq!(game.stats().items_used(), 2);
    assert_eq!(game.stats().glyphs_learnt(), 1, "a page is not a glyph");

    // Two moths and a mite, each killed in one blow on the cell east of
    // the player, then nothing else dies.
    let here = game.player_actor().position();
    for kind in ["moth", "mite", "moth"] {
        game.spawn_monster(here.step(Direction::East), content.monster(kind).unwrap());
        let events = act(&mut game, Action::Move(Direction::East));
        assert!(
            events
                .iter()
                .any(|event| matches!(event, Event::Died { .. })),
            "{kind}: {events:?}"
        );
    }
    act(&mut game, Action::Wait);

    let stats = game.stats();
    assert_eq!(
        stats.kills().collect::<Vec<_>>(),
        [("mite", 1), ("moth", 2)]
    );
    assert_eq!(stats.total_kills(), 3);
    assert_eq!(stats.deepest(), FIRST_DEPTH, "the room has no stairs");
    assert_eq!(stats.turns(), turns);
    assert_eq!(stats.turns(), game.player_turns());
}

#[test]
fn the_deepest_floor_follows_each_descent() {
    let seed = Seed::new(17);
    let mut game = Game::on_floor(seed, generation::generate(seed, FIRST_DEPTH));
    for expected in [FIRST_DEPTH + 1, FIRST_DEPTH + 2] {
        for _ in 0..500 {
            if game.depth() == expected {
                break;
            }
            game.submit(reckless_action(&game));
        }
        assert_eq!(game.depth(), expected, "the stairs are reached");
        assert_eq!(game.stats().deepest(), expected);
    }
}

#[test]
fn the_stats_always_equal_a_tally_of_the_events() {
    let content = Content::load().expect("shipped content is valid");
    let mut deaths = 0;
    let mut covered = Tally::default();
    for seed in SEEDS {
        let mut game = Game::new(Seed::new(seed), &content);
        let mut tally = Tally {
            deepest: FIRST_DEPTH,
            ..Tally::default()
        };
        // Mostly reckless, with a cast of the known sigils every few turns
        // so every quality shows up.
        let mut rng = Seed::new(seed).stream(9, Purpose::Gameplay);
        while game.player_turns() < MAX_TURNS && !game.is_over() {
            let turns = game.player_turns();
            let action = if turns % 7 == 3 {
                // Ember once its glyph is read, one node off, or nothing.
                let digits: &[u8] = match rng.random_range(0..3) {
                    0 => &[7, 5, 3],
                    1 => &[7, 5],
                    _ => &[1, 9, 7],
                };
                cast_action(digits)
            } else {
                reckless_action(&game)
            };
            let mut events = game.submit(action);
            // A refused action took no time: wait so the game moves on.
            if game.player_turns() == turns && !game.is_over() {
                events.extend(game.submit(Action::Wait));
            }
            tally.add(&events);
            assert_eq!(Tally::of(&game), tally, "seed {seed}");
        }
        assert!(
            tally.kills.values().sum::<u32>() > 0,
            "seed {seed}: something was killed"
        );
        deaths += u32::from(game.is_over());
        covered.casts.pure += tally.casts.pure;
        covered.casts.unstable += tally.casts.unstable;
        covered.casts.fizzled += tally.casts.fizzled;
        covered.glyphs += tally.glyphs;
        covered.deepest = covered.deepest.max(tally.deepest);
        // The game over, nothing changes any more.
        let before = game.stats().clone();
        game.submit(Action::Wait);
        assert_eq!(game.stats(), &before);
    }
    assert!(deaths > 0, "a reckless player dies on one of the seeds");
    let casts = covered.casts;
    assert!(
        casts.pure > 0 && casts.unstable > 0 && casts.fizzled > 0 && covered.glyphs > 0,
        "every count was exercised: {covered:?}"
    );
    assert!(covered.deepest > FIRST_DEPTH + 1, "{covered:?}");
}
