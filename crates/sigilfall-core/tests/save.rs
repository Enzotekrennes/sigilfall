//! Saves read back into the very same game, and faulty saves refused
//! without panicking (task T23 of the plan, feature F9).
//!
//! A game played for a while is saved and read back; then the original and
//! the loaded game receive the same 200 actions and must produce exactly the
//! same events, keys of actors and items included, on several seeds, in the
//! middle of a fight and on the second floor. The other tests refuse saves
//! of another format version, truncated or corrupted text, references to
//! definitions the content lacks and inconsistent values, and save a
//! finished game. The run's statistics (`Game::stats`, T25) are part of
//! every round trip and have their own, and so does the date of a daily run
//! (T28). Written against the public API only.

use rand::RngExt as _;
use rand_chacha::ChaCha8Rng;
use sigilfall_core::action::Action;
use sigilfall_core::content::Content;
use sigilfall_core::event::Event;
use sigilfall_core::game::Game;
use sigilfall_core::geometry::{Direction, Position};
use sigilfall_core::map::Tile;
use sigilfall_core::path::find_path;
use sigilfall_core::rng::{DailyDate, Purpose, Seed};
use sigilfall_core::save::{self, FORMAT_VERSION, Reference, SaveError};
use sigilfall_core::sigil::Trace;

/// Seeds of the round trips.
const SEEDS: [u64; 6] = [0, 1, 2, 7, 42, 1234];

/// Actions both games receive after the save is read back.
const ACTIONS_AFTER_LOAD: usize = 200;

/// Most player turns spent looking for the moment of a save (a fight, the
/// second floor, a death) before the test gives up.
const MAX_SEARCH_TURNS: u64 = 4000;

/// Actions a game read from a corrupted save is played for, to show it runs.
const ACTIONS_AFTER_CORRUPTION: usize = 30;

/// Every how many bytes the large save is truncated, every length being
/// tried on the small one.
const TRUNCATION_STRIDE: usize = 7;

/// Bytes written over the large save, one at a time, at sampled places.
const CORRUPTIONS: [u8; 8] = *b"09-()\"x ";

/// Number of places of the large save corrupted.
const CORRUPTED_PLACES: usize = 400;

/// The content shipped with the game.
fn content() -> Content {
    Content::load().expect("the shipped content is valid")
}

/// Writes `game` as a save.
fn saved(game: &Game) -> String {
    save::to_string(game).expect("a game is always written")
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
        .filter_map(|monster| {
            let path = find_path(map, me, monster.position, occupied)?;
            Some((path.len(), monster.position.y, monster.position.x, path))
        })
        .min_by_key(|&(steps, y, x, _)| (steps, y, x));
    if let Some((_, _, _, path)) = charge
        && let Some(&direction) = path.first()
    {
        return Action::Move(direction);
    }
    if map.tile(me) == Some(Tile::StairsDown) {
        return Action::Descend;
    }
    let stairs =
        cells(map.width(), map.height()).find(|&cell| map.tile(cell) == Some(Tile::StairsDown));
    stairs
        .and_then(|stairs| {
            find_path(map, me, stairs, occupied).or_else(|| find_path(map, me, stairs, |_| false))
        })
        .and_then(|path| path.first().copied())
        .map_or(Action::Wait, Action::Move)
}

/// Every cell of a `width` × `height` map, row by row.
fn cells(width: usize, height: usize) -> impl Iterator<Item = Position> {
    let width = i32::try_from(width).expect("a map fits in i32");
    let height = i32::try_from(height).expect("a map fits in i32");
    (0..height).flat_map(move |y| (0..width).map(move |x| Position::new(x, y)))
}

/// An action of every kind the game knows, drawn from `rng` on the state of
/// `game`: mostly the reckless player's, then random steps, waits, casts of
/// the content's sigils (some of them misdrawn, some aimed), and handling of
/// items, the inventory's own keys included.
fn varied_action(game: &Game, content: &Content, rng: &mut ChaCha8Rng) -> Action {
    let carried: Vec<_> = game.inventory().map(|(id, _)| id).collect();
    let direction =
        |rng: &mut ChaCha8Rng| Direction::ALL[rng.random_range(0..Direction::ALL.len())];
    match rng.random_range(0..20) {
        0..=7 => reckless_action(game),
        8..=10 => Action::Move(direction(rng)),
        11 => Action::Wait,
        12..=14 => {
            let sigils = content.sigils();
            let mut trace = *sigils[rng.random_range(0..sigils.len())].pattern();
            if rng.random_range(0..3) == 0 {
                let digits: Vec<u8> = (0..rng.random_range(1..=4))
                    .map(|_| rng.random_range(1..=9))
                    .collect();
                trace = Trace::from_pattern(&digits).unwrap_or(trace);
            }
            let aimed = rng.random_range(0..4) != 0;
            Action::Cast {
                trace,
                direction: aimed.then(|| direction(rng)),
            }
        }
        15 => Action::PickUp,
        16 | 17 if !carried.is_empty() => {
            let item = carried[rng.random_range(0..carried.len())];
            if rng.random_range(0..2) == 0 {
                Action::Use(item)
            } else {
                Action::Drop(item)
            }
        }
        16 | 17 => Action::Wait,
        _ => Action::Descend,
    }
}

/// Plays `game` with [`varied_action`] (or the reckless player alone when
/// `reckless`) until `stop` holds after an action, and returns whether it
/// did within [`MAX_SEARCH_TURNS`].
fn play_until(
    game: &mut Game,
    content: &Content,
    rng: &mut ChaCha8Rng,
    reckless: bool,
    mut stop: impl FnMut(&Game, &[Event]) -> bool,
) -> bool {
    while game.player_turns() < MAX_SEARCH_TURNS && !game.is_over() {
        let action = if reckless {
            reckless_action(game)
        } else {
            varied_action(game, content, rng)
        };
        let events = game.submit(action);
        if events.is_empty() || matches!(events.as_slice(), [Event::MoveBlocked { .. }]) {
            game.submit(Action::Wait);
        }
        if stop(game, &events) {
            return true;
        }
    }
    false
}

/// What a round trip went through, so the tests can check they covered the
/// cases that matter.
#[derive(Debug, Clone, Copy, Default)]
struct Coverage {
    /// The save held freed slots, whose keys the next actors or items reuse.
    freed_slots: bool,
    /// The games went down a floor after the save was read back, creating
    /// new actors and items.
    descended: bool,
    /// The games fought after the save was read back.
    fought: bool,
}

/// Saves `original`, reads it back, then gives both games the same
/// [`ACTIONS_AFTER_LOAD`] actions and checks that they produce the same
/// events, action by action, and end in the same state; returns what the
/// round trip went through.
fn assert_same_future(
    original: &mut Game,
    content: &Content,
    rng: &mut ChaCha8Rng,
    case: &str,
) -> Coverage {
    let text = saved(original);
    let mut loaded = save::from_str(&text, content)
        .unwrap_or_else(|error| panic!("{case}: the save reads back: {error}"));
    assert_eq!(
        saved(&loaded),
        text,
        "{case}: the loaded game saves the same"
    );
    assert_eq!(loaded.stats(), original.stats(), "{case}: same stats");
    let mut coverage = Coverage {
        freed_slots: text.matches("vacant: [],").count() < 2,
        ..Coverage::default()
    };

    for step in 0..ACTIONS_AFTER_LOAD {
        let action = varied_action(original, content, rng);
        let expected = original.submit(action);
        let events = loaded.submit(action);
        assert_eq!(events, expected, "{case}: action {step}, {action:?}");
        coverage.descended |= events.iter().any(|e| matches!(e, Event::Descended { .. }));
        coverage.fought |= events.iter().any(|e| matches!(e, Event::Attacked { .. }));
    }
    assert_eq!(saved(&loaded), saved(original), "{case}: same final state");
    assert_eq!(loaded.stats(), original.stats(), "{case}: same final stats");
    coverage
}

#[test]
fn a_game_saved_in_the_middle_of_a_fight_plays_on_identically() {
    let content = content();
    for seed in SEEDS {
        let mut rng = Seed::new(seed).stream(0, Purpose::Gameplay);
        let mut game = Game::new(Seed::new(seed), &content);
        let fighting = play_until(&mut game, &content, &mut rng, true, |game, events| {
            let blows = events
                .iter()
                .filter(|event| matches!(event, Event::Attacked { .. }))
                .count();
            // Both sides struck and both still stand: the fight goes on.
            blows >= 2 && !game.is_over() && game.visible_monsters().next().is_some()
        });
        assert!(fighting, "seed {seed}: a fight starts");

        let coverage = assert_same_future(
            &mut game,
            &content,
            &mut rng,
            &format!("seed {seed}, fight"),
        );

        assert!(
            coverage.fought,
            "seed {seed}: the fight goes on after the load"
        );
    }
}

#[test]
fn a_game_saved_on_the_second_floor_plays_on_identically() {
    let content = content();
    let mut saved_below = 0;
    for seed in SEEDS {
        let mut rng = Seed::new(seed).stream(1, Purpose::Gameplay);
        let mut game = Game::new(Seed::new(seed), &content);
        let below = play_until(&mut game, &content, &mut rng, true, |game, _| {
            game.depth() == 2
        });
        assert!(below, "seed {seed}: the player reaches the second floor");
        // A few varied turns there, so the save holds a floor in play.
        let mut turns = 0;
        play_until(&mut game, &content, &mut rng, false, |game, _| {
            turns += 1;
            turns == 20 || game.depth() != 2
        });
        if game.is_over() || game.depth() != 2 {
            continue;
        }

        assert_same_future(
            &mut game,
            &content,
            &mut rng,
            &format!("seed {seed}, floor 2"),
        );
        saved_below += 1;
    }
    assert!(
        saved_below >= SEEDS.len() / 2,
        "only {saved_below} games were saved on the second floor"
    );
}

#[test]
fn a_game_saved_after_varied_play_plays_on_identically() {
    let content = content();
    let mut covered = Coverage::default();
    for seed in SEEDS {
        let mut rng = Seed::new(seed).stream(2, Purpose::Gameplay);
        let mut game = Game::new(Seed::new(seed), &content);
        for round in 0..3 {
            let mut turns = 0;
            play_until(&mut game, &content, &mut rng, false, |_, _| {
                turns += 1;
                turns == 60
            });
            let coverage = assert_same_future(
                &mut game,
                &content,
                &mut rng,
                &format!("seed {seed}, round {round}"),
            );
            covered.freed_slots |= coverage.freed_slots;
            covered.descended |= coverage.descended && coverage.freed_slots;
        }
    }
    // The keys handed out after a load only depend on the order of the
    // freed slots when a save holds some and new actors or items follow.
    assert!(
        covered.descended,
        "no round trip saved freed slots then went down a floor: {covered:?}"
    );
}

#[test]
fn a_finished_game_is_saved_and_read_back_finished() {
    let content = content();
    let mut game = SEEDS
        .iter()
        .find_map(|&seed| {
            let mut game = Game::new(Seed::new(seed), &content);
            let mut rng = Seed::new(seed).stream(3, Purpose::Gameplay);
            play_until(&mut game, &content, &mut rng, true, |game, _| {
                game.is_over()
            })
            .then_some(game)
        })
        .expect("a reckless player dies on one of the seeds");
    let text = saved(&game);

    let mut loaded = save::from_str(&text, &content).expect("a finished game reads back");

    assert!(loaded.is_over());
    assert_eq!(loaded.status(), game.status());
    assert_eq!(loaded.player_actor().hp(), 0);
    assert_eq!(loaded.depth(), game.depth());
    assert_eq!(loaded.player_turns(), game.player_turns());
    assert_eq!(loaded.stats(), game.stats());
    assert!(game.stats().total_kills() > 0, "the reckless player killed");
    assert!(loaded.submit(Action::Wait).is_empty());
    assert!(game.submit(Action::Wait).is_empty());
    assert_eq!(saved(&loaded), text);
}

/// A small game in the fixed room with a monster, an item on the floor, a
/// carried item and a glyph, already played for a few turns.
fn small_game(content: &Content) -> Game {
    let mut game = Game::in_fixed_room(Seed::new(9));
    let rat = content.monster("ossuary_rat").expect("a shipped monster");
    game.spawn_monster(Position::new(25, 3), rat);
    game.spawn_monster(Position::new(27, 8), rat);
    let draught = content.item("healing_draught").expect("a shipped item");
    game.place_item(Position::new(6, 5), draught);
    game.place_item(Position::new(3, 9), draught);
    game.engrave(
        Position::new(8, 8),
        content.sigil("ember").expect("a shipped sigil"),
    );
    game.submit(Action::Move(Direction::East));
    game.submit(Action::PickUp);
    game.submit(Action::Move(Direction::SouthEast));
    game
}

#[test]
fn a_save_of_another_format_version_is_refused() {
    let content = content();
    let text = saved(&small_game(&content));
    let current = format!("format_version: {FORMAT_VERSION}");
    assert!(text.contains(&current), "the version comes first:\n{text}");

    for other in [0, FORMAT_VERSION + 1, u32::MAX] {
        let edited = text.replacen(&current, &format!("format_version: {other}"), 1);

        let error = save::from_str(&edited, &content).expect_err("another version");

        assert_eq!(
            error,
            SaveError::UnsupportedVersion {
                found: other,
                expected: FORMAT_VERSION
            }
        );
        assert!(
            error
                .to_string()
                .contains(&format!("format version {other}"))
        );
    }
}

#[test]
fn a_save_that_is_not_ron_is_refused_with_its_line() {
    let content = content();
    let text = saved(&small_game(&content));
    let mut lines: Vec<&str> = text.lines().collect();
    lines[4] = "    depth: two,";

    let error = save::from_str(&lines.join("\n"), &content).expect_err("not RON");

    assert!(
        matches!(error, SaveError::Malformed { line: 5, .. }),
        "the fault is on line 5: {error}"
    );
    assert!(error.to_string().contains("line 5"), "{error}");
}

#[test]
fn a_truncated_save_is_refused_at_every_length() {
    let content = content();
    let small = saved(&small_game(&content));
    let large = saved(&Game::new(Seed::new(3), &content));

    for (text, stride) in [(&small, 1), (&large, TRUNCATION_STRIDE)] {
        let whole = text.trim_end().len();
        for length in (0..whole).step_by(stride) {
            let truncated = &text[..length];

            let result = save::from_str(truncated, &content);

            assert!(
                matches!(result, Err(SaveError::Malformed { .. })),
                "a save cut at byte {length} of {whole} is refused as malformed"
            );
        }
    }
}

#[test]
fn a_corrupted_save_is_refused_or_read_into_a_playable_game() {
    let content = content();
    let mut game = Game::new(Seed::new(5), &content);
    let mut rng = Seed::new(5).stream(4, Purpose::Gameplay);
    let mut turns = 0;
    play_until(&mut game, &content, &mut rng, false, |_, _| {
        turns += 1;
        turns == 80
    });
    let text = saved(&game);
    let (mut refused, mut read) = (0, 0);

    for _ in 0..CORRUPTED_PLACES {
        let place = rng.random_range(0..text.len());
        let byte = CORRUPTIONS[rng.random_range(0..CORRUPTIONS.len())];
        let mut bytes = text.clone().into_bytes();
        bytes[place] = byte;
        let corrupted = String::from_utf8(bytes).expect("the save is ASCII");

        match save::from_str(&corrupted, &content) {
            Err(_) => refused += 1,
            Ok(mut loaded) => {
                read += 1;
                for _ in 0..ACTIONS_AFTER_CORRUPTION {
                    let action = varied_action(&loaded, &content, &mut rng);
                    loaded.submit(action);
                }
            }
        }
    }

    assert!(refused > 0 && read > 0, "{refused} refused, {read} read");
}

/// Reads `text` after replacing `from` by `to` once, and returns the error.
fn edited_error(text: &str, from: &str, to: &str, content: &Content) -> SaveError {
    assert!(text.contains(from), "`{from}` is in the save:\n{text}");
    let edited = text.replacen(from, to, 1);
    match save::from_str(&edited, content) {
        Ok(_) => panic!("the save with `{to}` instead of `{from}` is refused"),
        Err(error) => error,
    }
}

#[test]
fn a_reference_to_a_definition_the_content_lacks_is_refused() {
    let content = content();
    let mut game = small_game(&content);
    // From (7, 6), onto the glyph of ember at (8, 8).
    game.submit(Action::Move(Direction::SouthEast));
    game.submit(Action::Move(Direction::South));
    assert!(game.grimoire().knows("ember"), "the glyph was read");
    let text = saved(&game);
    let cases = [
        ("\"ossuary_rat\"", Reference::Monster),
        ("\"healing_draught\"", Reference::Item),
        ("grimoire: [\n        \"ember\"", Reference::Sigil),
        ("sigil: \"ember\"", Reference::Sigil),
    ];

    for (from, kind) in cases {
        let to = from
            .replace("ossuary_rat", "xyz")
            .replace("healing_draught", "xyz");
        let to = to.replace("ember", "xyz");

        let error = edited_error(&text, from, &to, &content);

        assert_eq!(
            error,
            SaveError::UnknownReference {
                kind,
                id: "xyz".to_owned()
            },
            "{from}"
        );
        assert_eq!(
            error.to_string(),
            format!("the save references {kind} `xyz`, absent from the content")
        );
    }
}

#[test]
fn an_inconsistent_save_is_refused() {
    let content = content();
    let text = saved(&small_game(&content));
    let cases = [
        (
            "position: (x: 25",
            "position: (x: 95",
            "not a walkable cell of the 30x12 map",
        ),
        ("position: (x: 25", "position: (x: 0", "not a walkable cell"),
        (
            "player: (1, 1)",
            "player: (5, 1)",
            "the player is the actor in slot 5",
        ),
        (
            "kind: Player",
            "kind: Monster(definition: \"ossuary_rat\", last_seen_player: None)",
            "exactly one player",
        ),
        (
            "status: Playing",
            "status: GameOver(killed_by: Player)",
            "the game is over but",
        ),
        (
            "turn_order: [",
            "turn_order: [(2, 1),",
            "lists slot 2 twice",
        ),
        (
            "speed: 150",
            "speed: 0",
            "the speed of the actor in slot 2 is 0",
        ),
        (
            "hp: 30,",
            "hp: 31,",
            "the hit points of the actor in slot 1 is 31",
        ),
        (
            "essence_since: ",
            "essence_since: 9999",
            "the tick of the last essence recovery",
        ),
        (
            "inventory: [\n        (1, 1),\n    ]",
            "inventory: []",
            "the inventory lists 0 items, but 1 items are carried",
        ),
        (
            "(key: (2, 1), value: (definition",
            "(key: (1, 1), value: (definition",
            "the item slot 1 is out of the slots 1 to 2 or listed twice",
        ),
        (
            "Some((x: 3, y: 9))",
            "Some((x: 0, y: 9))",
            "the item in slot 2 is at (0, 9)",
        ),
        ("\"#.", "\"#~", "row 1 of the map tiles holds `~`"),
    ];

    for (from, to, expected) in cases {
        let error = edited_error(&text, from, to, &content);

        assert!(
            matches!(&error, SaveError::Inconsistent(problem) if problem.contains(expected)),
            "`{to}` gives `{expected}`, got: {error}"
        );
    }
}

/// The date of the daily runs of these tests.
fn daily_date() -> DailyDate {
    DailyDate::new(2026, 11, 8).expect("a real date")
}

#[test]
fn a_daily_run_is_saved_with_its_date_and_plays_on_identically() {
    let content = content();
    let mut rng = Seed::new(28).stream(0, Purpose::Gameplay);
    let mut game = Game::daily(daily_date(), &content);
    let below = play_until(&mut game, &content, &mut rng, true, |game, _| {
        game.depth() == 2
    });
    assert!(below, "the daily run reaches the second floor");

    let text = saved(&game);
    assert!(
        text.contains(
            "    daily: Some((
        year: 2026,
        month: 11,
        day: 8,
    )),
"
        ),
        "the date is written:\n{text}"
    );
    let loaded = save::from_str(&text, &content).expect("the save reads back");
    assert_eq!(loaded.daily_date(), Some(daily_date()));
    assert_eq!(loaded.seed(), Seed::daily(daily_date()));

    assert_same_future(&mut game, &content, &mut rng, "daily run");
    assert_eq!(game.daily_date(), Some(daily_date()), "still the daily run");
}

#[test]
fn a_run_of_a_seed_is_saved_as_no_daily_run() {
    let content = content();
    let game = Game::new(Seed::daily(daily_date()), &content);

    let text = saved(&game);
    let loaded = save::from_str(&text, &content).expect("the save reads back");

    assert!(text.contains("daily: None,"), "{text}");
    assert_eq!(
        loaded.daily_date(),
        None,
        "the daily seed typed by hand is not the daily run"
    );
}

#[test]
fn a_faulty_daily_date_is_refused() {
    let content = content();
    let daily = saved(&Game::daily(daily_date(), &content));
    let typed = saved(&Game::new(Seed::new(42), &content));
    let cases = [
        (
            &daily,
            "day: 8,",
            "day: 31,",
            "the daily date 2026-11-31 does not exist",
        ),
        (
            &daily,
            "month: 11,",
            "month: 13,",
            "the daily date 2026-13-8 does not exist",
        ),
        (
            &daily,
            "day: 8,",
            "day: 9,",
            "the daily run of 2026-11-09 has the seed",
        ),
        (
            &typed,
            "daily: None,",
            "daily: Some((year: 2026, month: 11, day: 8)),",
            "but the save gives 0000-0000-0000-002A",
        ),
    ];

    for (text, from, to, expected) in cases {
        let error = edited_error(text, from, to, &content);

        assert!(
            matches!(&error, SaveError::Inconsistent(problem) if problem.contains(expected)),
            "`{to}` gives `{expected}`, got: {error}"
        );
    }
}

/// [`small_game`] after the player read the glyph of ember, killed a rat
/// and cast ember (at nothing) and an unknown trace: every count of the
/// stats holds something.
fn game_with_stats(content: &Content) -> Game {
    let mut game = small_game(content);
    // From (7, 6), onto the glyph of ember at (8, 8).
    game.submit(Action::Move(Direction::SouthEast));
    game.submit(Action::Move(Direction::South));
    let (item, _) = game.inventory().next().expect("the draught picked up");
    game.submit(Action::Use(item));
    let rat = content.monster("ossuary_rat").expect("a shipped monster");
    game.spawn_monster(Position::new(9, 8), rat);
    for _ in 0..10 {
        if game.stats().total_kills() > 0 {
            break;
        }
        game.submit(Action::Move(Direction::East));
    }
    for digits in [[7, 5, 3], [1, 9, 7]] {
        let mut trace = Trace::new();
        for digit in digits {
            trace
                .push(sigilfall_core::sigil::Node::new(digit).unwrap())
                .unwrap();
        }
        game.submit(Action::Cast {
            trace,
            direction: None,
        });
    }
    let stats = game.stats();
    assert_eq!(stats.kills().collect::<Vec<_>>(), [("ossuary_rat", 1)]);
    assert_eq!((stats.casts().pure, stats.casts().fizzled), (1, 1));
    assert_eq!((stats.glyphs_learnt(), stats.items_used()), (1, 1));
    game
}

#[test]
fn the_stats_are_saved_and_read_back_and_go_on_counting() {
    let content = content();
    let mut game = game_with_stats(&content);
    let text = saved(&game);
    assert!(
        text.contains(
            "stats: (
        turns: "
        ) && text.contains(
            "kills: {
            \"ossuary_rat\": 1,
        },"
        ) && text.contains(
            "pure: 1,
            unstable: 0,
            fizzled: 1,"
        ),
        "the stats are readable in the save:
{text}"
    );

    let mut loaded = save::from_str(&text, &content).expect("the save reads back");

    assert_eq!(loaded.stats(), game.stats());
    assert_eq!(loaded.player_turns(), game.player_turns());
    let rat = content.monster("ossuary_rat").expect("a shipped monster");
    for game in [&mut game, &mut loaded] {
        let here = game.player_actor().position();
        game.spawn_monster(here.step(Direction::East), rat);
        for _ in 0..10 {
            game.submit(Action::Move(Direction::East));
        }
    }
    assert_eq!(loaded.stats(), game.stats());
    assert_eq!(loaded.stats().total_kills(), 2, "the count goes on");
}

#[test]
fn faulty_stats_are_refused() {
    let content = content();
    let game = game_with_stats(&content);
    let text = saved(&game);
    let turns = format!(
        "stats: (
        turns: {},",
        game.player_turns()
    );
    // Above the largest count a save accepts, half the range of `u64`.
    let too_many_turns = "stats: (
        turns: 9300000000000000000,";

    let unknown = edited_error(&text, "\"ossuary_rat\": 1,", "\"xyz\": 1,", &content);
    assert_eq!(
        unknown,
        SaveError::UnknownReference {
            kind: Reference::Monster,
            id: "xyz".to_owned()
        }
    );
    let cases = [
        (
            "\"ossuary_rat\": 1,",
            "\"ossuary_rat\": 0,",
            "the count of `ossuary_rat` killed is 0",
        ),
        (
            "deepest: 1,",
            "deepest: 0,",
            "the deepest floor reached is 0",
        ),
        (turns.as_str(), too_many_turns, "the player turn count"),
    ];
    for (from, to, expected) in cases {
        let error = edited_error(&text, from, to, &content);

        assert!(
            matches!(&error, SaveError::Inconsistent(problem) if problem.contains(expected)),
            "`{to}` gives `{expected}`, got: {error}"
        );
    }
    let misspelt = edited_error(&text, "glyphs_learnt", "glyphs_read", &content);
    assert!(
        matches!(misspelt, SaveError::Malformed { .. }),
        "{misspelt}"
    );
}
