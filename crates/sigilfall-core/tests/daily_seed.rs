//! The daily seed, core half.
//!
//! Written against the public API only. The rule: two players who start the
//! daily dungeon of the same UTC date get the same dungeon. The tests check
//! it on the whole state of a new game (the text of its save, random stream
//! included), on the floors below the first one whatever was played above
//! them, and on the seed itself, which is recomputed here from the rule
//! written in the documentation (the date packed into one number, combined
//! with a fixed tag, then mixed by SplitMix64) instead of being read from the
//! production code. They also check that two dates give two dungeons, that a
//! daily run remembers its date through a save, and that a plain run of the
//! same seed is the same dungeon without being a daily run.
//!
//! The player cannot be placed from outside the crate, so the tests that
//! need the second floor walk the player to the stairs along the shortest
//! path. Their monsters are harmless (one hit point, no attack) so the
//! walker always arrives; the floors below are compared with the floors the
//! generator builds for the same seed and depth.

use std::collections::{HashMap, HashSet, VecDeque};

use sigilfall_core::action::Action;
use sigilfall_core::content::Content;
use sigilfall_core::game::Game;
use sigilfall_core::generation;
use sigilfall_core::geometry::{Direction, Position};
use sigilfall_core::map::{Map, Tile};
use sigilfall_core::rng::{DailyDate, Seed};
use sigilfall_core::save;

/// The tag the documentation says is mixed into every daily seed: the ASCII
/// bytes of `SIGILDAY` read as a big-endian number.
const TAG: u64 = u64::from_be_bytes(*b"SIGILDAY");

/// Most actions spent walking to the stairs of one floor.
const WALK_BUDGET: usize = 1500;

/// Monsters that never get to hurt: at speed 1 they gain a hundredth of the
/// energy they need to act, and one blow kills them.
const HARMLESS: &str = r#"[
    Monster(id: "mote_one", name: "mote one", family: Vermin, elite: false,
        speed: 1, hp: 1, attack: 0, defense: 0, min_depth: 1),
]"#;

/// The spawn table of the harmless content: motes and every item.
const SPAWNS: &str = r#"[
    Spawn(of: Monster("mote_one"), from_depth: 1, to_depth: 10, weight: 1),
    Spawn(of: Item("healing_draught"), from_depth: 1, to_depth: 10, weight: 1),
    Spawn(of: Item("essence_phial"), from_depth: 1, to_depth: 10, weight: 1),
    Spawn(of: Item("steadying_ink"), from_depth: 1, to_depth: 10, weight: 1),
    Spawn(of: Item("veil_page"), from_depth: 2, to_depth: 10, weight: 1),
]"#;

/// The shipped sigils and items, for the test content.
const SIGILS: &str = include_str!("../content/sigils.ron");
const ITEMS: &str = include_str!("../content/items.ron");

/// The content shipped with the game.
fn shipped() -> Content {
    Content::load().expect("shipped content is valid")
}

/// Harmless monsters, so a walker always reaches the stairs.
fn harmless() -> Content {
    Content::parse_with_spawns(HARMLESS, SIGILS, ITEMS, SPAWNS).expect("valid test content")
}

/// The date `year`-`month`-`day`, which the test knows to exist.
fn date(year: u16, month: u8, day: u8) -> DailyDate {
    DailyDate::new(year, month, day).expect("a real date")
}

/// The SplitMix64 finaliser, written out again here.
fn splitmix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The daily seed of `year`-`month`-`day` by the documented rule.
fn expected_seed(year: u16, month: u8, day: u8) -> u64 {
    let packed = (u64::from(year) << 16) | (u64::from(month) << 8) | u64::from(day);
    splitmix(packed ^ TAG)
}

/// Every day from 2026-01-01 on, `count` of them, by plain calendar rules.
fn days_from_2026(count: usize) -> Vec<DailyDate> {
    const LENGTHS: [u8; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let (mut year, mut month, mut day) = (2026_u16, 1_u8, 1_u8);
    let mut days = Vec::new();
    for _ in 0..count {
        days.push(date(year, month, day));
        let leap =
            year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
        let length = LENGTHS[usize::from(month) - 1] + u8::from(month == 2 && leap);
        if day < length {
            day += 1;
        } else if month < 12 {
            (month, day) = (month + 1, 1);
        } else {
            (year, month, day) = (year + 1, 1, 1);
        }
    }
    days
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

/// The tiles of `map` in reading order, written as one string.
fn layout(map: &Map) -> String {
    cells(map)
        .into_iter()
        .map(|cell| format!("{:?}", map.tile(cell)))
        .collect::<Vec<_>>()
        .join(",")
}

/// What a player of the current floor can compare: its tiles, its own cell
/// and the monsters (cell and kind), sorted by cell.
fn floor_view(game: &Game) -> (String, Position, Vec<(Position, String)>) {
    let mut monsters: Vec<_> = game
        .actors()
        .filter_map(|(_, actor)| {
            let monster = actor.as_monster()?;
            Some((actor.position(), monster.definition().to_owned()))
        })
        .collect();
    monsters.sort_by_key(|monster| (monster.0.y, monster.0.x));
    (layout(game.map()), game.player_actor().position(), monsters)
}

/// The cell of the stairs of the current floor of `game`.
fn stairs_of(game: &Game) -> Position {
    cells(game.map())
        .into_iter()
        .find(|&cell| game.map().tile(cell) == Some(Tile::StairsDown))
        .expect("every generated floor has stairs")
}

/// The first step of a shortest walk from `from` to `to`, ignoring
/// creatures; `None` when `to` cannot be reached.
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

/// Walks the player to the stairs, striking what stands in the way, and
/// descends. Returns whether the descent happened.
fn descend(game: &mut Game) -> bool {
    for _ in 0..WALK_BUDGET {
        if game.is_over() {
            return false;
        }
        let here = game.player_actor().position();
        let stairs = stairs_of(game);
        if here == stairs {
            let depth = game.depth();
            game.submit(Action::Descend);
            return game.depth() == depth + 1;
        }
        let Some(direction) = first_step(game.map(), here, stairs) else {
            return false;
        };
        game.submit(Action::Move(direction));
    }
    false
}

/// Waits `turns` times, so a second player's game differs from the first.
fn dawdle(game: &mut Game, turns: usize) {
    for _ in 0..turns {
        game.submit(Action::Wait);
    }
}

// --- The same UTC date gives the same dungeon ------------------

#[test]
fn the_daily_seed_follows_the_documented_rule_for_dates_of_every_kind() {
    let named = [
        (2026, 11, 8),
        (2026, 1, 1),
        (2026, 12, 31),
        (2024, 2, 29),
        (2000, 2, 29),
        (1, 1, 1),
        (9999, 12, 31),
        (1970, 1, 1),
    ];
    for (year, month, day) in named {
        assert_eq!(
            Seed::daily(date(year, month, day)).value(),
            expected_seed(year, month, day),
            "{year}-{month:02}-{day:02}"
        );
    }
}

#[test]
fn the_daily_seed_of_a_known_day_is_pinned() {
    // The value the real binary showed for 2026-11-08 ("Seed 4A8B-CAEC-...").
    // Every player of that date depends on it: changing the rule gives every
    // date another dungeon.
    assert_eq!(
        Seed::daily(date(2026, 11, 8)).to_string(),
        "4A8B-CAEC-21D9-3697"
    );
}

#[test]
fn two_players_of_the_same_date_get_the_same_game_down_to_the_random_stream() {
    let content = shipped();
    for day in [
        date(2026, 11, 8),
        date(2024, 2, 29),
        date(1, 1, 1),
        date(9999, 12, 31),
    ] {
        let first = Game::daily(day, &content);
        let second = Game::daily(day, &content);
        assert_eq!(
            save::to_string(&first).unwrap(),
            save::to_string(&second).unwrap(),
            "{day}: the whole saved state is identical"
        );
        assert_eq!(first.seed(), Seed::daily(day), "{day}");
        assert_eq!(first.daily_date(), Some(day), "{day}");
        assert_eq!(floor_view(&first), floor_view(&second), "{day}");
    }
}

#[test]
fn the_first_floor_of_a_daily_run_is_the_floor_the_generator_builds_for_its_seed() {
    let content = shipped();
    let day = date(2026, 11, 8);
    let game = Game::daily(day, &content);
    let floor = generation::generate_with_monsters(Seed::daily(day), 1, &content);

    assert_eq!(layout(game.map()), layout(floor.map()));
    assert_eq!(game.player_actor().position(), floor.start());
    let mut expected: Vec<_> = floor
        .monsters()
        .iter()
        .map(|spawn| (spawn.position(), spawn.definition().id().to_owned()))
        .collect();
    expected.sort_by_key(|monster| (monster.0.y, monster.0.x));
    assert_eq!(floor_view(&game).2, expected);
}

#[test]
fn the_floors_below_are_the_same_whatever_each_player_did_above() {
    let content = harmless();
    for day in [date(2026, 11, 8), date(2026, 11, 9), date(2025, 6, 30)] {
        let mut straight = Game::daily(day, &content);
        let mut detour = Game::daily(day, &content);
        dawdle(&mut detour, 40);
        assert_ne!(straight.ticks(), detour.ticks(), "{day}: the plays differ");

        for depth in 2..=4_u32 {
            assert!(descend(&mut straight), "{day}: straight reaches {depth}");
            assert!(descend(&mut detour), "{day}: detour reaches {depth}");
            assert_eq!(straight.depth(), depth);
            assert_eq!(
                floor_view(&straight),
                floor_view(&detour),
                "{day}, floor {depth}"
            );
            let floor = generation::generate_with_monsters(Seed::daily(day), depth, &content);
            assert_eq!(
                layout(straight.map()),
                layout(floor.map()),
                "{day}, floor {depth}: the floor of the daily seed at that depth"
            );
            dawdle(&mut detour, 7);
        }
    }
}

#[test]
fn on_the_shipped_content_the_second_floor_is_shared_too() {
    let content = shipped();
    let day = date(2026, 11, 8);
    let mut compared = 0;
    for waits in [0_usize, 2, 5] {
        let mut direct = Game::daily(day, &content);
        let mut slow = Game::daily(day, &content);
        dawdle(&mut slow, waits);
        // The shipped monsters can kill the walker; a seed where either
        // dies is not compared (the date is fixed, so the outcome is too).
        if !descend(&mut direct) || !descend(&mut slow) {
            continue;
        }
        compared += 1;
        assert_eq!(floor_view(&direct), floor_view(&slow), "waits {waits}");
    }
    assert!(
        compared >= 1,
        "at least one pair of players reached the second floor alive"
    );
}

// --- A different date is a different dungeon --------------------------------

#[test]
fn a_year_of_dates_gives_a_year_of_different_seeds_and_different_first_floors() {
    let content = shipped();
    let days = days_from_2026(366);
    assert_eq!(days.len(), 366);
    let seeds: HashSet<u64> = days.iter().map(|&day| Seed::daily(day).value()).collect();
    assert_eq!(seeds.len(), 366, "no two dates share a seed");

    let first_floors: HashSet<String> = days
        .iter()
        .map(|&day| layout(Game::daily(day, &content).map()))
        .collect();
    assert_eq!(
        first_floors.len(),
        366,
        "no two dates share the layout of the first floor"
    );
}

#[test]
fn the_day_after_is_not_the_day_before_even_across_a_month_a_year_and_a_leap_day() {
    let content = shipped();
    for (before, after) in [
        (date(2026, 11, 8), date(2026, 11, 9)),
        (date(2026, 11, 30), date(2026, 12, 1)),
        (date(2026, 12, 31), date(2027, 1, 1)),
        (date(2024, 2, 28), date(2024, 2, 29)),
        (date(2024, 2, 29), date(2024, 3, 1)),
    ] {
        assert_ne!(Seed::daily(before), Seed::daily(after), "{before}/{after}");
        assert_ne!(
            layout(Game::daily(before, &content).map()),
            layout(Game::daily(after, &content).map()),
            "{before}/{after}"
        );
    }
}

// --- What a daily run remembers ----------------------------------------------

#[test]
fn a_daily_run_keeps_its_date_through_a_save_and_a_plain_run_has_none() {
    let content = shipped();
    let day = date(2026, 1, 2);
    let mut game = Game::daily(day, &content);
    dawdle(&mut game, 5);

    let text = save::to_string(&game).unwrap();
    let back = save::from_str(&text, &content).expect("a saved daily run reads back");
    assert_eq!(back.daily_date(), Some(day));
    assert_eq!(back.seed(), Seed::daily(day));
    assert_eq!(save::to_string(&back).unwrap(), text, "state identical");

    let plain = Game::new(Seed::from(42), &content);
    assert_eq!(plain.daily_date(), None);
    let plain_back =
        save::from_str(&save::to_string(&plain).unwrap(), &content).expect("a plain run");
    assert_eq!(plain_back.daily_date(), None);
}

#[test]
fn a_run_of_the_daily_seed_is_the_same_dungeon_but_is_not_a_daily_run() {
    let content = shipped();
    let day = date(2026, 11, 8);
    let daily = Game::daily(day, &content);
    let replay = Game::new(Seed::daily(day), &content);

    assert_eq!(floor_view(&daily), floor_view(&replay));
    assert_eq!(daily.daily_date(), Some(day));
    assert_eq!(replay.daily_date(), None);
}

#[test]
fn a_resumed_daily_run_plays_on_exactly_as_if_it_was_never_stopped() {
    let content = shipped();
    let day = date(2026, 1, 2);
    let mut straight = Game::daily(day, &content);
    let mut stopped = Game::daily(day, &content);
    let script = [
        Direction::East,
        Direction::South,
        Direction::West,
        Direction::North,
    ];
    for step in 0..12 {
        let action = if step % 3 == 0 {
            Action::Wait
        } else {
            Action::Move(script[step % script.len()])
        };
        straight.submit(action);
        stopped.submit(action);
    }
    let mut stopped = save::from_str(&save::to_string(&stopped).unwrap(), &content).unwrap();
    for step in 0..40 {
        let action = if step % 4 == 0 {
            Action::Wait
        } else {
            Action::Move(script[step % script.len()])
        };
        assert_eq!(
            straight.submit(action),
            stopped.submit(action),
            "step {step}"
        );
    }
    assert_eq!(
        save::to_string(&straight).unwrap(),
        save::to_string(&stopped).unwrap()
    );
}
