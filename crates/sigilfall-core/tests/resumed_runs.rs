//! Saved and resumed runs, permadeath and the summary, core half, written
//! against the public API only.
//!
//! They check at the level of the game state that a run saved at any moment
//! and read back goes on exactly as the run that was never interrupted,
//! random stream included, across several floors, with a pack, a grimoire,
//! steadied casts and wounded monsters. A bot that is a pure function of the
//! public state plays the run and its reloaded twin side by side, and every
//! event, every actor and the whole text of the save are compared. They also
//! check at the level of the format that unreadable texts of every kind are
//! errors, never panics, and the numbers of the summary (the statistics equal
//! a tally of the events, and the seed shown replays the first floor).
//!
//! The monsters of the runs are slow and weak but have many hit points, so
//! the bot survives several floors and leaves wounded monsters behind. Every
//! loop is bounded.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use sigilfall_core::action::Action;
use sigilfall_core::combat::Combatant;
use sigilfall_core::content::Content;
use sigilfall_core::event::Event;
use sigilfall_core::game::{Game, Status};
use sigilfall_core::geometry::{Direction, Position};
use sigilfall_core::map::{Map, Tile};
use sigilfall_core::rng::Seed;
use sigilfall_core::save::{self, FORMAT_VERSION, SaveError};
use sigilfall_core::sigil::{CastQuality, ESSENCE_COST, Node, Trace};
use sigilfall_core::stats::CastCounts;

/// Slow, weak, sturdy monsters: they wound the bot slowly, and a few blows
/// of the bot leave them hurt but alive.
const MONSTERS: &str = r#"[
    Monster(id: "brute", name: "brute", name_fr: Masculine("le monstre"), plural: "brutes", plural_fr: "monstres", family: Vermin, elite: false,
        speed: 25, hp: 60, attack: 1, defense: 0, min_depth: 1),
]"#;

/// Brutes everywhere and every item of the game.
const SPAWNS: &str = r#"[
    Spawn(of: Monster("brute"), from_depth: 1, to_depth: 10, weight: 1),
    Spawn(of: Item("healing_draught"), from_depth: 1, to_depth: 10, weight: 2),
    Spawn(of: Item("essence_phial"), from_depth: 1, to_depth: 10, weight: 2),
    Spawn(of: Item("steadying_ink"), from_depth: 1, to_depth: 10, weight: 4),
    Spawn(of: Item("veil_page"), from_depth: 2, to_depth: 10, weight: 2),
]"#;

/// The sigil and item files shipped with the game.
const SIGILS: &str = include_str!("../content/sigils.ron");
const ITEMS: &str = include_str!("../content/items.ron");

/// Pattern of the shipped sigil `ember` (damages the nearest enemy).
const EMBER: [u8; 3] = [7, 5, 3];
/// One node short of [`EMBER`]: an unstable cast, followed by a backlash.
const EMBER_UNSTABLE: [u8; 2] = [7, 5];
/// Three edits from every shipped sigil: it fizzles.
const NOTHING: [u8; 3] = [1, 5, 9];

/// Floors the bot goes down before the run is considered rich enough.
const TARGET_DEPTH: u32 = 4;
/// Most actions the bot plays on one run.
const RUN_BUDGET: usize = 1200;
/// Actions between two checkpoints.
const CHECKPOINT_EVERY: usize = 37;
/// Actions both twins play after a checkpoint.
const CONTINUATION: usize = 70;
/// Seeds tried for the long runs.
const SEEDS: std::ops::Range<u64> = 0..60;

/// Test content: sturdy brutes, the shipped sigils and items.
fn rich_content() -> Content {
    Content::parse_with_spawns(MONSTERS, SIGILS, ITEMS, SPAWNS).expect("valid test content")
}

/// The content shipped with the game.
fn shipped() -> Content {
    Content::load().expect("shipped content is valid")
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

/// The cell of the stairs of the current floor of `game`.
fn stairs_of(game: &Game) -> Position {
    cells(game.map())
        .into_iter()
        .find(|&cell| game.map().tile(cell) == Some(Tile::StairsDown))
        .expect("every generated floor has stairs")
}

/// First step of a shortest walk from `from` to `to`, ignoring creatures.
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

/// The direction of the adjacent monster that is still healthy (above half
/// its hit points), if any: the bot strikes those, so the ones it leaves
/// behind are wounded.
fn healthy_neighbour(game: &Game) -> Option<Direction> {
    let me = game.player_actor().position();
    Direction::ALL.into_iter().find(|&direction| {
        let cell = me.step(direction);
        game.actors().any(|(_, actor)| {
            actor.as_monster().is_some()
                && actor.position() == cell
                && actor.hp() * 2 > actor.max_hp()
        })
    })
}

/// The bot: a pure function of the public state of `game`, so two games in
/// the same state get the same action. It reads what lies underfoot, uses
/// its items, strikes healthy neighbours, casts at what it sees (pure,
/// unstable and fizzled casts in turn) and otherwise heads for the stairs,
/// going down until `TARGET_DEPTH`.
fn bot(game: &Game) -> Action {
    let me = game.player_actor();
    let here = me.position();
    if game
        .visible_items()
        .any(|item| item.position == here && game.inventory().count() < 8)
    {
        return Action::PickUp;
    }
    for (id, item) in game.inventory() {
        let wanted = match item.definition().id() {
            // Used in peace, so the steadied casts wait for the next fight.
            "steadying_ink" => game.steady_casts() == 0 && game.visible_monsters().next().is_none(),
            "healing_draught" => me.hp() + 10 <= me.max_hp(),
            "essence_phial" => me.essence() + 4 <= me.max_essence(),
            "veil_page" => !game.grimoire().knows("veil"),
            _ => false,
        };
        if wanted {
            return Action::Use(id);
        }
    }
    if let Some(direction) = healthy_neighbour(game) {
        return Action::Move(direction);
    }
    if game.visible_monsters().next().is_some() && me.essence() >= ESSENCE_COST {
        let digits: &[u8] = match game.ticks() % 3 {
            0 => &EMBER,
            1 => &EMBER_UNSTABLE,
            _ => &NOTHING,
        };
        return Action::Cast {
            trace: pressed(digits),
            direction: None,
        };
    }
    if here == stairs_of(game) {
        return if game.depth() < TARGET_DEPTH {
            Action::Descend
        } else {
            Action::Wait
        };
    }
    first_step(game.map(), here, stairs_of(game)).map_or(Action::Wait, Action::Move)
}

/// A new game of `seed` with the glyphs of `ember` and `recall` carved
/// beside the start, so that the grimoire is not empty from the first
/// steps.
fn started(seed: Seed, content: &Content) -> Game {
    let mut game = Game::new(seed, content);
    let start = game.player_actor().position();
    for (direction, id) in [(Direction::East, "ember"), (Direction::West, "recall")] {
        let cell = start.step(direction);
        if game.map().can_step(start, direction) && game.engraving_at(cell).is_none() {
            game.engrave(cell, content.sigil(id).expect("a shipped sigil"));
        }
    }
    game
}

/// What the saves have to carry, noted at a checkpoint.
#[derive(Debug, Default, Clone, Copy)]
struct Coverage {
    /// A pack with something in it.
    pack: bool,
    /// A grimoire with a sigil in it.
    grimoire: bool,
    /// Steadied casts waiting.
    ink: bool,
    /// A monster that has lost hit points and still lives.
    wounded: bool,
    /// A floor below the third.
    deep: bool,
    /// Essence spent.
    spent: bool,
}

impl Coverage {
    /// Notes what `game` holds.
    fn note(&mut self, game: &Game) {
        let me = game.player_actor();
        self.pack |= game.inventory().next().is_some();
        self.grimoire |= !game.grimoire().is_empty();
        self.ink |= game.steady_casts() > 0;
        self.wounded |= game
            .actors()
            .any(|(_, a)| a.as_monster().is_some() && a.hp() < a.max_hp());
        self.deep |= game.depth() >= 3;
        self.spent |= me.essence() < me.max_essence();
    }

    /// Adds what `other` noted.
    fn merge(&mut self, other: Self) {
        self.pack |= other.pack;
        self.grimoire |= other.grimoire;
        self.ink |= other.ink;
        self.wounded |= other.wounded;
        self.deep |= other.deep;
        self.spent |= other.spent;
    }
}

/// Plays `actions` actions of the bot on `twin` and `original` in step and
/// checks that they stay the same: same events, same state of the player,
/// same text of the save, at every step.
fn assert_twins_agree(original: &mut Game, twin: &mut Game, actions: usize, context: &str) {
    for step in 0..actions {
        let (a, b) = (bot(original), bot(twin));
        assert_eq!(a, b, "{context}: the bot decides alike at step {step}");
        let events = original.submit(a);
        let reloaded = twin.submit(b);
        assert_eq!(events, reloaded, "{context}: events differ at step {step}");
        assert_eq!(original.ticks(), twin.ticks(), "{context}: clock");
        assert_eq!(
            original.player_actor(),
            twin.player_actor(),
            "{context}: player at step {step}"
        );
        if original.is_over() {
            break;
        }
    }
    assert_eq!(
        save::to_string(original).unwrap(),
        save::to_string(twin).unwrap(),
        "{context}: the whole state, random stream included, ends the same"
    );
}

/// Runs the bot on `seed`, checkpointing: at each checkpoint the game is
/// saved, read back, and the reloaded twin plays on in step with the game.
/// Returns what the checkpoints covered and the deepest floor reached, or
/// `None` when the bot died before `TARGET_DEPTH`.
fn run_with_checkpoints(seed: Seed, content: &Content) -> Option<(Coverage, u32)> {
    let mut game = started(seed, content);
    let mut coverage = Coverage::default();
    for step in 0..RUN_BUDGET {
        if game.is_over() {
            return None;
        }
        if step % CHECKPOINT_EVERY == 0 {
            coverage.note(&game);
            let text = save::to_string(&game).unwrap();
            let mut twin = save::from_str(&text, content).expect("a save reads back");
            assert_eq!(save::to_string(&twin).unwrap(), text, "stable text");
            assert_twins_agree(
                &mut game,
                &mut twin,
                CONTINUATION,
                &format!("{seed} @{step}"),
            );
            if game.depth() >= TARGET_DEPTH && step > 200 {
                return Some((coverage, game.depth()));
            }
            continue;
        }
        let action = bot(&game);
        game.submit(action);
    }
    None
}

// --- Quit, relaunch, the same state, random stream included ---

#[test]
fn a_run_saved_at_any_moment_goes_on_exactly_as_if_it_was_never_stopped() {
    let content = rich_content();
    let mut survivors = 0;
    let mut coverage = Coverage::default();
    for value in SEEDS {
        let Some((seen, depth)) = run_with_checkpoints(Seed::from(value), &content) else {
            continue;
        };
        assert!(depth >= TARGET_DEPTH);
        survivors += 1;
        coverage.merge(seen);
        if survivors == 6 {
            break;
        }
    }
    assert!(survivors >= 4, "only {survivors} seeds survived the tour");
    // The comparison must not be vacuous: the saves held all of it.
    assert!(coverage.pack, "a pack was saved");
    assert!(coverage.grimoire, "a grimoire was saved");
    assert!(coverage.wounded, "wounded monsters were saved");
    assert!(coverage.deep, "a deep floor was saved");
    assert!(coverage.spent, "spent essence was saved");
}

#[test]
fn the_reloaded_game_keeps_every_visible_part_of_the_state() {
    let content = rich_content();
    let mut checked = 0;
    for value in SEEDS {
        let mut game = started(Seed::from(value), &content);
        for _ in 0..400 {
            if game.is_over() {
                break;
            }
            let action = bot(&game);
            game.submit(action);
        }
        if game.is_over() || game.depth() < 2 {
            continue;
        }
        let back = save::from_str(&save::to_string(&game).unwrap(), &content).unwrap();
        assert_eq!(back.seed(), game.seed());
        assert_eq!(back.depth(), game.depth());
        assert_eq!(back.ticks(), game.ticks());
        assert_eq!(back.player_turns(), game.player_turns());
        assert_eq!(back.steady_casts(), game.steady_casts());
        assert_eq!(back.stats(), game.stats());
        assert_eq!(back.status(), game.status());
        assert_eq!(back.engravings(), game.engravings());
        assert_eq!(back.grimoire(), game.grimoire());
        assert_eq!(back.player_actor(), game.player_actor());
        let pack = |g: &Game| -> Vec<String> {
            g.inventory()
                .map(|(_, i)| i.definition().id().to_owned())
                .collect()
        };
        assert_eq!(pack(&back), pack(&game));
        let monsters = |g: &Game| -> Vec<(Position, i32, i32, String)> {
            let mut all: Vec<_> = g
                .actors()
                .filter_map(|(_, a)| {
                    Some((
                        a.position(),
                        a.hp(),
                        a.energy(),
                        a.as_monster()?.definition().to_owned(),
                    ))
                })
                .collect();
            all.sort_by_key(|m| (m.0.y, m.0.x));
            all
        };
        assert_eq!(monsters(&back), monsters(&game));
        for cell in cells(game.map()) {
            assert_eq!(back.map().tile(cell), game.map().tile(cell));
            assert_eq!(back.is_visible(cell), game.is_visible(cell));
            assert_eq!(back.is_remembered(cell), game.is_remembered(cell));
        }
        checked += 1;
    }
    assert!(checked >= 3, "only {checked} runs reached a second floor");
}

#[test]
fn the_random_stream_is_part_of_the_save() {
    // Two games of one seed that differ only in how far their gameplay
    // random stream has gone must not play out alike; and a save keeps that
    // position, so the reload follows the original, not a fresh stream.
    let content = shipped();
    let mut compared = 0;
    for value in 0..20 {
        let seed = Seed::from(value);
        let mut game = Game::new(seed, &content);
        for _ in 0..60 {
            if game.is_over() {
                break;
            }
            let action = bot(&game);
            game.submit(action);
        }
        if game.is_over() {
            continue;
        }
        let text = save::to_string(&game).unwrap();
        let mut reloaded = save::from_str(&text, &content).unwrap();
        // A game replayed from scratch to the same point, by contrast, is
        // the same state too: the stream is a function of the actions.
        let mut replay = Game::new(seed, &content);
        for _ in 0..60 {
            if replay.is_over() {
                break;
            }
            let action = bot(&replay);
            replay.submit(action);
        }
        assert_eq!(save::to_string(&replay).unwrap(), text);
        assert_twins_agree(&mut game, &mut reloaded, 150, &format!("seed {seed}"));
        compared += 1;
    }
    assert!(compared >= 10);
}

#[test]
fn the_position_of_the_random_stream_is_what_the_future_depends_on() {
    // Moving the saved position of the gameplay stream must change what
    // happens next (damage rolls, backlashes): proof that the reload really
    // continues the stream from the position written, and that the position
    // is not recomputed or ignored.
    let content = shipped();
    let (mut diverged, mut tried) = (0, 0);
    for value in 0..12 {
        let mut game = Game::new(Seed::from(value), &content);
        for _ in 0..60 {
            if game.is_over() {
                break;
            }
            let action = bot(&game);
            game.submit(action);
        }
        if game.is_over() {
            continue;
        }
        let text = save::to_string(&game).unwrap();
        let line = text
            .lines()
            .find(|line| line.trim_start().starts_with("word_pos:"))
            .expect("a word_pos line");
        let position: u64 = line
            .trim()
            .trim_start_matches("word_pos:")
            .trim()
            .trim_end_matches(',')
            .parse()
            .expect("a number");
        let moved = text.replacen(line, &format!("        word_pos: {},", position + 17), 1);
        assert_ne!(moved, text);
        let mut faithful = save::from_str(&text, &content).unwrap();
        let mut shifted = save::from_str(&moved, &content).unwrap();
        tried += 1;
        for _ in 0..250 {
            if faithful.is_over() || shifted.is_over() {
                break;
            }
            let (a, b) = (bot(&faithful), bot(&shifted));
            let (x, y) = (faithful.submit(a), shifted.submit(b));
            if x != y {
                diverged += 1;
                break;
            }
        }
    }
    assert!(tried >= 6, "{tried} runs tried");
    assert!(
        diverged * 2 >= tried,
        "the shifted stream changed the future in only {diverged} of {tried} runs"
    );
}

// --- Unreadable saves are errors, never panics ---

#[test]
fn texts_that_are_not_saves_are_refused_with_an_error() {
    let content = shipped();
    let good = save::to_string(&Game::new(Seed::from(1), &content)).unwrap();
    let cases: Vec<(&str, String)> = vec![
        ("empty", String::new()),
        ("blank", " \n\t\n".to_owned()),
        ("garbage", "this is not a save {{{ ]".to_owned()),
        ("json", r#"{"format_version": 2}"#.to_owned()),
        (
            "only the version",
            format!("(format_version: {FORMAT_VERSION})"),
        ),
        ("unicode", "\u{feff}\u{202e}ŝ𝔞𝔳𝔢 💾".repeat(50)),
        ("deep nesting", "(".repeat(5000)),
        ("long line", "a".repeat(2_000_000)),
        ("nul bytes", "\0\0\0\0".to_owned()),
        ("duplicated", format!("{good}{good}")),
        ("half", good[..good.len() / 2].to_owned()),
        ("minus one byte", good[..good.len() - 1].to_owned()),
    ];
    for (name, text) in cases {
        let result = std::panic::catch_unwind(|| save::from_str(&text, &shipped()));
        let outcome = result.unwrap_or_else(|_| panic!("{name}: from_str panicked"));
        assert!(outcome.is_err(), "{name}: accepted as a save");
    }
}

#[test]
fn another_format_version_is_named_not_guessed() {
    let content = shipped();
    let good = save::to_string(&Game::new(Seed::from(1), &content)).unwrap();
    let head = format!("format_version: {FORMAT_VERSION}");
    assert!(good.contains(&head), "the version is written in the head");
    for other in [0_u32, 1, FORMAT_VERSION + 1, 99, u32::MAX] {
        let text = good.replacen(&head, &format!("format_version: {other}"), 1);
        match save::from_str(&text, &content) {
            Err(SaveError::UnsupportedVersion { found, expected }) => {
                assert_eq!((found, expected), (other, FORMAT_VERSION));
            }
            other_result => panic!("version {other}: {:?}", other_result.map(|_| ())),
        }
    }
    // A version out of the integer range is still an error, not a panic.
    let wild = good.replacen(&head, "format_version: 99999999999999999999", 1);
    assert!(save::from_str(&wild, &content).is_err());
    let negative = good.replacen(&head, "format_version: -1", 1);
    assert!(save::from_str(&negative, &content).is_err());
}

#[test]
fn a_save_whose_line_endings_were_changed_is_read_or_refused_never_a_panic() {
    // A save opened and re-saved by a Windows editor.
    let content = shipped();
    let game = Game::new(Seed::from(3), &content);
    let good = save::to_string(&game).unwrap();
    let crlf = good.replace('\n', "\r\n");
    match save::from_str(&crlf, &content) {
        Ok(back) => assert_eq!(save::to_string(&back).unwrap(), good),
        Err(error) => assert!(!error.to_string().is_empty()),
    }
}

#[test]
fn every_single_byte_damage_of_a_save_is_an_error_or_a_valid_game() {
    let content = shipped();
    let mut game = Game::new(Seed::from(9), &content);
    for _ in 0..30 {
        let action = bot(&game);
        game.submit(action);
    }
    let good = save::to_string(&game).unwrap();
    let bytes = good.as_bytes();
    let mut refused = 0;
    let stride = (bytes.len() / 700).max(1);
    for at in (0..bytes.len()).step_by(stride) {
        for &replacement in b"0x(\"-9 " {
            let mut damaged = bytes.to_vec();
            damaged[at] = replacement;
            let Ok(text) = String::from_utf8(damaged) else {
                continue;
            };
            let outcome = std::panic::catch_unwind(|| save::from_str(&text, &shipped()));
            match outcome {
                Err(_) => panic!("panic loading a save damaged at byte {at} with {replacement}"),
                Ok(Err(_)) => refused += 1,
                Ok(Ok(mut back)) => {
                    // Accepted: it must at least be playable.
                    for _ in 0..20 {
                        let action = bot(&back);
                        back.submit(action);
                    }
                }
            }
        }
    }
    assert!(refused > 100, "{refused} damaged saves refused");
}

#[test]
fn a_save_of_a_dead_game_reads_back_dead_and_stays_dead() {
    let content = shipped();
    let mut game = Game::new(Seed::from(2), &content);
    let strong = content
        .monsters()
        .iter()
        .max_by_key(|m| m.attack())
        .expect("a monster");
    let start = game.player_actor().position();
    let beside = Direction::ALL
        .into_iter()
        .find(|&d| game.map().can_step(start, d))
        .expect("a free neighbour");
    game.spawn_monster(start.step(beside), strong);
    for _ in 0..300 {
        if game.is_over() {
            break;
        }
        game.submit(Action::Wait);
    }
    assert!(game.is_over(), "the monster kills the waiting player");
    let text = save::to_string(&game).unwrap();
    let mut back = save::from_str(&text, &content).expect("a finished game reads back");
    assert!(back.is_over());
    assert_eq!(back.status(), game.status());
    assert_eq!(back.stats(), game.stats());
    let ticks = back.ticks();
    assert!(
        back.submit(Action::Wait).is_empty(),
        "no action after death"
    );
    assert_eq!(back.ticks(), ticks);
    assert_eq!(save::to_string(&back).unwrap(), text);
}

// --- The numbers of the summary come from the game ---

/// Counts tallied by the test from the events, for comparison with
/// `Game::stats`.
#[derive(Debug, Default, PartialEq, Eq)]
struct Tally {
    kills: BTreeMap<String, u32>,
    casts: CastCounts,
    deepest: u32,
    turns: u64,
}

impl Tally {
    /// Adds what `events` of one action report.
    fn add(&mut self, events: &[Event], took_time: bool) {
        self.turns += u64::from(took_time);
        for event in events {
            match event {
                Event::Died {
                    who: Combatant::Monster { definition },
                    killed_by: Combatant::Player,
                    ..
                } => *self.kills.entry(definition.clone()).or_default() += 1,
                Event::SigilCast { quality, .. } => match quality {
                    CastQuality::Pure => self.casts.pure += 1,
                    CastQuality::Unstable => self.casts.unstable += 1,
                    CastQuality::Fizzled => self.casts.fizzled += 1,
                },
                Event::Descended { depth } => self.deepest = self.deepest.max(*depth),
                _ => {}
            }
        }
    }
}

#[test]
fn the_statistics_equal_an_independent_tally_of_the_events_through_saves() {
    // The counts keep adding up across a save and a reload in the middle of
    // the run: the summary after a resumed run is the whole run's.
    let content = rich_content();
    let mut runs = 0;
    for value in SEEDS.take(25) {
        let mut game = started(Seed::from(value), &content);
        let mut tally = Tally {
            deepest: 1,
            ..Tally::default()
        };
        for step in 0..600 {
            if game.is_over() {
                break;
            }
            if step % 90 == 45 {
                let text = save::to_string(&game).unwrap();
                game = save::from_str(&text, &content).unwrap();
            }
            let before = game.ticks();
            let action = bot(&game);
            let events = game.submit(action);
            // The killing action took time too, though the clock stops with it.
            tally.add(&events, game.ticks() != before || game.is_over());
        }
        let stats = game.stats();
        let kills: BTreeMap<String, u32> =
            stats.kills().map(|(id, n)| (id.to_owned(), n)).collect();
        assert_eq!(kills, tally.kills, "seed {value}: kills");
        assert_eq!(stats.total_kills(), tally.kills.values().sum::<u32>());
        assert_eq!(stats.casts(), tally.casts, "seed {value}: casts");
        assert_eq!(stats.deepest(), tally.deepest, "seed {value}: deepest");
        assert_eq!(stats.turns(), tally.turns, "seed {value}: turns");
        assert_eq!(game.player_turns(), tally.turns);
        runs += usize::from(stats.casts().total() > 0);
    }
    assert!(runs >= 10, "only {runs} runs cast anything");
}

#[test]
fn the_seed_shown_in_the_summary_gives_back_the_first_floor() {
    let content = shipped();
    for value in [0_u64, 1, 42, 0xDEAD_BEEF, u64::MAX, 0x7F3A_91C2_0B5E_D4A8] {
        let seed = Seed::from(value);
        let mut game = Game::new(seed, &content);
        let first_floor = save::to_string(&game).unwrap();
        for _ in 0..40 {
            let action = bot(&game);
            game.submit(action);
        }
        // What the summary prints, typed back by the player.
        let shown = game.seed().to_string();
        assert_eq!(shown.len(), 19, "four groups of four hexadecimal digits");
        let typed: Seed = shown.parse().expect("the shown seed is accepted back");
        assert_eq!(typed, seed);
        let again = Game::new(typed, &content);
        assert_eq!(save::to_string(&again).unwrap(), first_floor);
        // Lower case and the plain number are accepted as well.
        let lower: Seed = shown.to_lowercase().parse().expect("lower case");
        assert_eq!(lower, seed);
    }
}

#[test]
fn a_dead_player_is_reported_with_the_cause_and_the_depth_it_died_at() {
    let content = shipped();
    let mut found = 0;
    for value in 0..30 {
        let mut game = Game::new(Seed::from(value), &content);
        let mut last_blow = None;
        for _ in 0..2500 {
            if game.is_over() {
                break;
            }
            let action = bot(&game);
            for event in game.submit(action) {
                if let Event::Died {
                    who: Combatant::Player,
                    killed_by,
                    ..
                } = event
                {
                    last_blow = Some(killed_by);
                }
            }
        }
        if let Status::GameOver { killed_by } = game.status() {
            assert_eq!(Some(killed_by.clone()), last_blow, "seed {value}");
            assert!(game.stats().deepest() >= game.depth());
            assert!(game.stats().turns() > 0);
            found += 1;
        }
    }
    assert!(found >= 3, "only {found} runs ended in death");
}

#[test]
fn steadied_casts_survive_a_save_and_are_spent_one_by_one_after_it() {
    let content = shipped();
    let mut game = Game::in_fixed_room(Seed::from(5));
    let start = game.player_actor().position();
    let east = start.step(Direction::East);
    game.engrave(east, content.sigil("ember").unwrap());
    game.place_item(
        east.step(Direction::East),
        content.item("steadying_ink").unwrap(),
    );
    game.submit(Action::Move(Direction::East));
    game.submit(Action::Move(Direction::East));
    game.submit(Action::PickUp);
    let (ink, _) = game.inventory().next().expect("the ink is carried");
    game.submit(Action::Use(ink));
    assert_eq!(game.steady_casts(), 2, "a Steady(2) item");

    let mut reloaded = save::from_str(&save::to_string(&game).unwrap(), &content).unwrap();
    assert_eq!(reloaded.steady_casts(), 2, "the casts waiting are kept");
    let cast = |game: &mut Game| {
        game.submit(Action::Cast {
            trace: pressed(&EMBER_UNSTABLE),
            direction: None,
        })
        .into_iter()
        .find_map(|event| match event {
            Event::SigilCast { quality, .. } => Some(quality),
            _ => None,
        })
    };
    for (original, again) in [(Some(CastQuality::Pure), 1), (Some(CastQuality::Pure), 0)] {
        assert_eq!(cast(&mut game), original);
        assert_eq!(cast(&mut reloaded), original);
        assert_eq!(game.steady_casts(), again);
        assert_eq!(reloaded.steady_casts(), again);
    }
    assert_eq!(cast(&mut game), Some(CastQuality::Unstable));
    assert_eq!(cast(&mut reloaded), Some(CastQuality::Unstable));
    assert_eq!(
        save::to_string(&game).unwrap(),
        save::to_string(&reloaded).unwrap()
    );
}
