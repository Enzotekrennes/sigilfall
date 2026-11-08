//! Acceptance tests of F11 (the daily seed), binary half.
//!
//! They drive the application as `main` does, on scratch folders and an
//! injected clock only, and read what the player reads: the rendered
//! screens and the files on disk. They check that two players of the same
//! UTC date get the same dungeon (whatever the hour and the way they start
//! it), that the date turns at midnight UTC, that `--daily` and `--seed`
//! exclude each other, the "Daily" entry of the title with its date, the
//! date a daily run keeps when it is resumed on another day, and the line to
//! share on the end screen, whose numbers are read back from the same screen
//! and from the core's statistics.
//!
//! Written independently of the unit tests of `app` and `messages`: the
//! dates are turned into Unix time here, the share line is parsed from the
//! drawn screen, and its numbers are compared with those the core counted.

use std::fs;

use clap::Parser as _;
use clap::error::ErrorKind;
use ratatui::crossterm::event::KeyCode;
use sigilfall_core::game::{Game, Status};
use sigilfall_core::geometry::Direction;
use sigilfall_core::rng::{DailyDate, Seed};
use sigilfall_core::save;

use crate::app::{App, State};
use crate::cli::Cli;
use crate::clock::ManualClock;
use crate::f10_acceptance::{
    SIZES, ctrl_c, go_to, launch, launch_with, press, rows, shipped, text, type_text,
};
use crate::persist::ScratchFolder;
use crate::theme::{Appearance, ColorChoice, ColorSupport, GlyphSet, Options};
use crate::ui::title::TitleItem;

/// Seconds in a UTC day.
const DAY: u64 = 86_400;

/// The date `year`-`month`-`day`, which the test knows to exist.
fn date(year: u16, month: u8, day: u8) -> DailyDate {
    DailyDate::new(year, month, day).expect("a real date")
}

/// Unix time of midnight UTC opening `year`-`month`-`day`, counted here from
/// the civil calendar (days from 1970-01-01 by the months of each year).
fn midnight(year: u16, month: u8, day: u8) -> u64 {
    let leap = |y: u16| y.is_multiple_of(4) && (!y.is_multiple_of(100) || y.is_multiple_of(400));
    let mut days: u64 = (1970..year).map(|y| if leap(y) { 366 } else { 365 }).sum();
    let lengths = [
        31,
        if leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    days += lengths[..usize::from(month) - 1].iter().sum::<u64>();
    days += u64::from(day) - 1;
    days * DAY
}

/// A clock whose wall time is `unix_seconds`.
fn clock_at(unix_seconds: u64) -> ManualClock {
    ManualClock::at_unix_seconds(unix_seconds)
}

/// The text of the whole game in `app`, random stream included.
fn state_of(game: &Game) -> String {
    save::to_string(game).expect("a game can be written")
}

/// The rows of the map area (rows 1 to 16, 58 columns) of an 80 × 24 game
/// screen.
fn map_area(app: &App) -> Vec<String> {
    rows(app, 80, 24)
        .into_iter()
        .skip(1)
        .take(16)
        .map(|row| row.chars().take(58).collect())
        .collect()
}

/// The "Daily" row of the title menu.
fn daily_row(app: &App) -> String {
    rows(app, 80, 24)
        .into_iter()
        .find(|row| row.contains("Daily"))
        .expect("a Daily entry")
}

// --- Scenario 1: the same UTC date gives the same dungeon -------------------

#[test]
fn two_players_of_the_same_utc_date_get_the_same_dungeon_whatever_the_hour() {
    let day = date(2026, 11, 8);
    let first_second = midnight(2026, 11, 8);
    let hours = [
        first_second,
        first_second + 3 * 3600 + 17 * 60,
        first_second + 12 * 3600,
        first_second + DAY - 1,
    ];
    let mut screens = Vec::new();
    let mut states = Vec::new();
    for time in hours {
        let folder = ScratchFolder::new("f11-same-date");
        let app = launch_with(
            &folder,
            &["--daily"],
            ColorSupport::TrueColor,
            &clock_at(time),
        )
        .unwrap();
        assert_eq!(app.state(), State::Playing, "--daily starts at once");
        assert_eq!(app.game().daily_date(), Some(day), "{time}");
        assert_eq!(app.game().seed(), Seed::daily(day), "{time}");
        screens.push(SIZES.map(|(w, h)| rows(&app, w, h)));
        states.push(state_of(app.game()));
    }
    for index in 1..hours.len() {
        assert_eq!(
            screens[0], screens[index],
            "the same screens at {}",
            hours[index]
        );
        assert_eq!(
            states[0], states[index],
            "the same whole state at {}",
            hours[index]
        );
    }
    let status = &screens[0][0][0];
    assert!(status.contains("Daily 2026-11-08"), "{status}");
    assert!(
        !status.contains("Seed"),
        "a daily run shows its date, not its seed: {status}"
    );

    // Starting it from the title menu is the same run.
    let folder = ScratchFolder::new("f11-same-date-title");
    let mut from_title = launch_with(
        &folder,
        &[],
        ColorSupport::TrueColor,
        &clock_at(first_second + 5 * 3600),
    )
    .unwrap();
    go_to(&mut from_title, TitleItem::Daily);
    from_title.handle_key(press(KeyCode::Enter));
    assert_eq!(from_title.state(), State::Playing);
    assert_eq!(state_of(from_title.game()), states[0]);
    assert_eq!(SIZES.map(|(w, h)| rows(&from_title, w, h)), screens[0]);

    // The dungeon of the daily seed, replayed with --seed any other day: the
    // same map, a "Seed" on the status line instead of the date.
    let other = ScratchFolder::new("f11-same-date-seed");
    let by_seed = launch_with(
        &other,
        &["--seed", &Seed::daily(day).to_string()],
        ColorSupport::TrueColor,
        &clock_at(midnight(2030, 1, 1)),
    )
    .unwrap();
    assert_eq!(
        map_area(&by_seed),
        screens[0][0][1..17]
            .iter()
            .map(|row| row.chars().take(58).collect::<String>())
            .collect::<Vec<_>>()
    );
    assert!(
        rows(&by_seed, 80, 24)[0].contains("Seed 4A8B-CAEC-21D9-3697"),
        "{}",
        rows(&by_seed, 80, 24)[0]
    );
}

#[test]
fn the_date_turns_at_midnight_utc_and_the_next_day_is_another_dungeon() {
    let last = midnight(2026, 11, 9) - 1;
    let before = ScratchFolder::new("f11-midnight-a");
    let one = launch_with(
        &before,
        &["--daily"],
        ColorSupport::TrueColor,
        &clock_at(last),
    )
    .unwrap();
    let after = ScratchFolder::new("f11-midnight-b");
    let two = launch_with(
        &after,
        &["--daily"],
        ColorSupport::TrueColor,
        &clock_at(last + 1),
    )
    .unwrap();
    assert!(rows(&one, 80, 24)[0].contains("Daily 2026-11-08"));
    assert!(rows(&two, 80, 24)[0].contains("Daily 2026-11-09"));
    assert_ne!(
        map_area(&one),
        map_area(&two),
        "one second later, another dungeon"
    );
    assert_ne!(state_of(one.game()), state_of(two.game()));

    // A title left open across midnight changes its date, and "Daily" then
    // starts the new day's dungeon.
    let folder = ScratchFolder::new("f11-midnight-title");
    let clock = clock_at(last);
    let mut title = launch_with(&folder, &[], ColorSupport::TrueColor, &clock).unwrap();
    assert!(
        daily_row(&title).contains("2026-11-08"),
        "{}",
        daily_row(&title)
    );
    clock.advance(std::time::Duration::from_secs(1));
    assert!(
        daily_row(&title).contains("2026-11-09"),
        "{}",
        daily_row(&title)
    );
    go_to(&mut title, TitleItem::Daily);
    title.handle_key(press(KeyCode::Enter));
    assert_eq!(title.game().daily_date(), Some(date(2026, 11, 9)));

    // Month, year and leap-day boundaries turn like any other.
    for (before, after) in [
        (midnight(2026, 11, 30), "2026-12-01"),
        (midnight(2026, 12, 31), "2027-01-01"),
        (midnight(2028, 2, 28), "2028-02-29"),
        (midnight(2028, 2, 29), "2028-03-01"),
    ] {
        let folder = ScratchFolder::new("f11-boundary");
        let app = launch_with(
            &folder,
            &["--daily"],
            ColorSupport::TrueColor,
            &clock_at(before + DAY),
        )
        .unwrap();
        assert!(
            rows(&app, 80, 24)[0].contains(&format!("Daily {after}")),
            "{after}"
        );
    }

    // Thirty consecutive days: thirty different first floors.
    let mut seen = std::collections::BTreeSet::new();
    for offset in 0..30 {
        let folder = ScratchFolder::new("f11-thirty");
        let app = launch_with(
            &folder,
            &["--daily"],
            ColorSupport::TrueColor,
            &clock_at(midnight(2026, 11, 1) + offset * DAY + 3600),
        )
        .unwrap();
        assert!(
            seen.insert(map_area(&app).join("\n")),
            "day {offset} repeats an earlier map"
        );
    }
}

// --- --daily and --seed exclude each other -----------------------------------

#[test]
fn daily_cannot_be_combined_with_seed_in_any_order() {
    for arguments in [
        &["--daily", "--seed", "42"][..],
        &["--seed", "42", "--daily"],
        &["--daily", "--seed=7F3A-91C2-0B5E-D4A8"],
        &["--seed", "0", "--ascii", "--daily"],
    ] {
        let error =
            Cli::try_parse_from(std::iter::once("sigilfall").chain(arguments.iter().copied()))
                .expect_err("refused");
        assert_eq!(error.kind(), ErrorKind::ArgumentConflict, "{arguments:?}");
        assert_eq!(error.exit_code(), 2, "{arguments:?}");
        let message = error.to_string();
        assert!(
            message.contains("--daily") && message.contains("--seed"),
            "{message}"
        );
    }
    for arguments in [
        &["--daily"][..],
        &["--daily", "--ascii"],
        &["--ascii", "--daily"],
    ] {
        assert!(
            Cli::try_parse_from(std::iter::once("sigilfall").chain(arguments.iter().copied()))
                .is_ok(),
            "{arguments:?}"
        );
    }
    assert!(
        Cli::try_parse_from(["sigilfall", "--daily=2026-11-08"]).is_err(),
        "--daily takes no value"
    );
}

#[test]
fn daily_is_refused_while_a_game_is_saved_and_the_save_is_left_alone() {
    let folder = ScratchFolder::new("f11-daily-saved");
    let game = Game::new(Seed::from(42), &shipped());
    folder.save_file().write(&game).unwrap();
    let saved = fs::read(folder.save_file().path()).unwrap();

    let error = launch(&folder, &["--daily"]).expect_err("refused while a game is saved");
    let message = format!("{error:#}");
    assert!(message.contains("--daily"), "{message}");
    assert!(message.contains("a game in progress is saved"), "{message}");
    assert_eq!(fs::read(folder.save_file().path()).unwrap(), saved);
    assert!(
        launch(&folder, &["--seed", "5"]).is_err(),
        "--seed is refused the same way"
    );
}

// --- The title shows Daily with the date -------------------------------------

#[test]
fn the_title_lists_daily_after_seed_with_the_date_of_the_clock() {
    let folder = ScratchFolder::new("f11-title");
    let app = launch_with(
        &folder,
        &[],
        ColorSupport::TrueColor,
        &clock_at(midnight(2026, 11, 8) + 40_000),
    )
    .unwrap();
    for (width, height) in SIZES {
        let screen = rows(&app, width, height);
        let seed = screen.iter().position(|row| row.contains("Seed")).unwrap();
        let daily = screen.iter().position(|row| row.contains("Daily")).unwrap();
        let options = screen
            .iter()
            .position(|row| row.contains("Options"))
            .unwrap();
        assert!(
            seed < daily && daily < options,
            "{width}x{height}: Seed, Daily, Options"
        );
        assert!(screen[daily].contains("2026-11-08"), "{}", screen[daily]);
    }
    assert!(
        daily_row(&app)
            .split_whitespace()
            .eq(["Daily", "2026-11-08"])
            || daily_row(&app).contains("› Daily")
    );
}

// --- The date is kept after a resume another day ------------------------------

#[test]
fn a_daily_run_resumed_three_days_later_is_still_the_run_of_its_date() {
    let folder = ScratchFolder::new("f11-resume");
    let day = date(2026, 11, 8);
    let start = clock_at(midnight(2026, 11, 8) + 9 * 3600);
    let mut app = launch_with(&folder, &["--daily"], ColorSupport::TrueColor, &start).unwrap();
    let keys = "lljjhkyub.nl..jjhhkk";
    let (first, second) = keys.split_at(10);
    type_text(&mut app, first);
    let at_quit = state_of(app.game());
    app.handle_key(press(KeyCode::Char('q')));
    assert_eq!(app.state(), State::Quit);
    app.save_on_exit().expect("the game is saved");
    assert!(folder.save_file().path().exists());

    // Three days later: the title offers today's daily dungeon, but
    // Continue resumes the old one, with its own date.
    let later = clock_at(midnight(2026, 11, 11) + 100);
    let mut resumed = launch_with(&folder, &[], ColorSupport::TrueColor, &later).unwrap();
    assert!(
        daily_row(&resumed).contains("2026-11-11"),
        "{}",
        daily_row(&resumed)
    );
    assert!(
        rows(&resumed, 80, 24)
            .iter()
            .any(|row| row.contains("Continue"))
    );
    resumed.handle_key(press(KeyCode::Enter));
    assert_eq!(resumed.state(), State::Playing);
    assert_eq!(resumed.game().daily_date(), Some(day));
    assert_eq!(
        state_of(resumed.game()),
        at_quit,
        "exactly the state it was left in"
    );
    for (width, height) in SIZES {
        let status = &rows(&resumed, width, height)[0];
        assert!(status.contains("Daily 2026-11-08"), "{status}");
        assert!(!status.contains("2026-11-11"), "{status}");
    }

    // It plays on as if it had never stopped, and keeps the date.
    let control_folder = ScratchFolder::new("f11-resume-control");
    let mut control = launch_with(
        &control_folder,
        &["--daily"],
        ColorSupport::TrueColor,
        &start,
    )
    .unwrap();
    type_text(&mut control, first);
    type_text(&mut control, second);
    type_text(&mut resumed, second);
    assert_eq!(state_of(resumed.game()), state_of(control.game()));
    assert_eq!(resumed.game().daily_date(), Some(day));
    resumed.handle_key(press(KeyCode::Char('q')));
    resumed.save_on_exit().unwrap();

    // Saved again, resumed again, still the same day's run; and "Daily" on
    // the title with that save asks before abandoning it.
    let mut again = launch_with(
        &folder,
        &[],
        ColorSupport::TrueColor,
        &clock_at(midnight(2027, 3, 1)),
    )
    .unwrap();
    go_to(&mut again, TitleItem::Daily);
    again.handle_key(press(KeyCode::Enter));
    assert!(
        matches!(again.state(), State::Confirm(_)),
        "{:?}",
        again.state()
    );
    again.handle_key(press(KeyCode::Esc));
    go_to(&mut again, TitleItem::Continue);
    again.handle_key(press(KeyCode::Enter));
    assert!(rows(&again, 80, 24)[0].contains("Daily 2026-11-08"));

    // A run of a plain seed shows its seed after a resume, never a date.
    let plain = ScratchFolder::new("f11-resume-plain");
    let mut run = launch(&plain, &["--seed", "42"]).unwrap();
    run.handle_key(press(KeyCode::Char('q')));
    run.save_on_exit().unwrap();
    let mut back = launch_with(&plain, &[], ColorSupport::TrueColor, &later).unwrap();
    back.handle_key(press(KeyCode::Enter));
    let status = &rows(&back, 80, 24)[0];
    assert!(
        status.contains("Seed 0000-0000-0000-002A") && !status.contains("Daily"),
        "{status}"
    );
}

// --- Scenario 2: the end of a run gives one line to share ----------------------

/// A game with one monster of each of `kinds` beside the player, the glyph
/// of Ember on a free cell near it, and a lead homunculus that starts at
/// least six steps away, so the player has time to read the glyph, fight and
/// cast before the end.
fn crowded(mut game: Game, kinds: &[&str]) -> (Game, Direction) {
    let content = shipped();
    let me = game.player_actor().position();
    let mut kinds = kinds.iter().copied();
    let mut glyph_at = None;
    for direction in Direction::ALL {
        let cell = me.step(direction);
        let free = game.map().can_step(me, direction)
            && game.actors().all(|(_, actor)| actor.position() != cell);
        if !free {
            continue;
        }
        if let Some(kind) = kinds.next() {
            game.spawn_monster(cell, content.monster(kind).unwrap());
        } else if glyph_at.is_none() && game.engraving_at(cell).is_none() {
            glyph_at = Some(direction);
        }
    }
    let direction = glyph_at.expect("a free cell for the glyph");
    game.engrave(me.step(direction), content.sigil("ember").unwrap());
    if let Some(far) = cell_at_least(&game, 6) {
        game.spawn_monster(far, content.monster("lead_homunculus").unwrap());
    }
    (game, direction)
}

/// A walkable cell at least `steps` steps from the player, found by a
/// breadth-first walk over the map; `None` when the floor has none.
fn cell_at_least(game: &Game, steps: usize) -> Option<sigilfall_core::geometry::Position> {
    use std::collections::{HashMap, VecDeque};
    let start = game.player_actor().position();
    let mut distance = HashMap::from([(start, 0_usize)]);
    let mut queue = VecDeque::from([start]);
    while let Some(cell) = queue.pop_front() {
        let here = distance[&cell];
        if here >= steps && game.actors().all(|(_, actor)| actor.position() != cell) {
            return Some(cell);
        }
        for direction in Direction::ALL {
            let next = cell.step(direction);
            if game.map().can_step(cell, direction) && !distance.contains_key(&next) {
                distance.insert(next, here + 1);
                queue.push_back(next);
            }
        }
    }
    None
}

/// The key that moves in `direction`.
const fn key_of(direction: Direction) -> char {
    match direction {
        Direction::North => 'k',
        Direction::NorthEast => 'u',
        Direction::East => 'l',
        Direction::SouthEast => 'n',
        Direction::South => 'j',
        Direction::SouthWest => 'b',
        Direction::West => 'h',
        Direction::NorthWest => 'y',
    }
}

/// Saves `game` in a folder, resumes it on a title opened at `time`, reads
/// the glyph, fights whatever stands beside, casts each of `traces` and
/// waits until the player dies. `None` if the player survives 400 turns.
fn play_to_the_end(game: Game, glyph: Direction, traces: &[&str], time: u64) -> Option<App> {
    let folder = ScratchFolder::new("f11-share");
    folder.save_file().write(&game).unwrap();
    let mut app = launch_with(&folder, &[], ColorSupport::TrueColor, &clock_at(time)).unwrap();
    app.handle_key(press(KeyCode::Enter));
    app.handle_key(press(KeyCode::Char(key_of(glyph))));
    for direction in Direction::ALL {
        for _ in 0..3 {
            if app.state() != State::Playing {
                break;
            }
            app.handle_key(press(KeyCode::Char(key_of(direction))));
        }
    }
    for trace in traces {
        if app.state() != State::Playing {
            break;
        }
        app.handle_key(press(KeyCode::Char('t')));
        type_text(&mut app, trace);
        app.handle_key(press(KeyCode::Enter));
        if matches!(app.state(), State::Casting(_)) {
            app.handle_key(press(KeyCode::Char('x')));
        }
    }
    for _ in 0..400 {
        if app.state() == State::GameOver {
            return Some(app);
        }
        app.handle_key(press(KeyCode::Char('.')));
    }
    (app.state() == State::GameOver).then_some(app)
}

/// The value of the summary row `label` ("Depth reached .... 3").
fn summary_value(screen: &[String], label: &str) -> String {
    let row = screen
        .iter()
        .find(|row| row.trim_start().starts_with(&format!("{label} .")))
        .unwrap_or_else(|| panic!("a `{label}` row in:\n{}", screen.join("\n")));
    row.trim_start()[label.len()..]
        .trim_start()
        .trim_start_matches('.')
        .split("   ")
        .next()
        .unwrap()
        .trim()
        .to_owned()
}

/// The lines of `screen` that start with "Sigilfall " (the shareable line).
fn share_lines(screen: &[String]) -> Vec<String> {
    screen
        .iter()
        .map(|row| row.trim().to_owned())
        .filter(|row| row.starts_with("Sigilfall "))
        .collect()
}

/// The `count` first daily runs, from `first`, that end in a death at the
/// summary, with the date they were played for.
fn daily_endings(first: DailyDate, count: usize, traces: &[&str]) -> Vec<(DailyDate, App)> {
    let content = shipped();
    let mut endings = Vec::new();
    let (year, mut month, mut day) = (first.year(), first.month(), first.day());
    for _ in 0..40 {
        let current = date(year, month, day);
        let (game, glyph) = crowded(
            Game::daily(current, &content),
            &["ossuary_rat", "wax_beetle"],
        );
        if let Some(app) = play_to_the_end(game, glyph, traces, midnight(year, month, day) + 7200) {
            endings.push((current, app));
            if endings.len() == count {
                break;
            }
        }
        day += 1;
        if day > 28 {
            (day, month) = (1, month + 1);
        }
    }
    endings
}

#[test]
fn the_summary_of_a_daily_run_ends_with_a_line_that_says_so_and_matches_its_numbers() {
    let endings = daily_endings(date(2026, 11, 1), 4, &["753", "75", "159"]);
    assert!(
        endings.len() >= 3,
        "only {} daily runs reached the summary",
        endings.len()
    );
    let mut with_kills = 0;
    let mut with_casts = 0;
    for (day, app) in &endings {
        let game = app.game();
        let stats = game.stats();
        assert!(matches!(game.status(), Status::GameOver { .. }));
        assert_eq!(game.daily_date(), Some(*day));
        for (width, height) in SIZES {
            let screen = rows(app, width, height);
            let lines = share_lines(&screen);
            assert_eq!(
                lines.len(),
                1,
                "{day} at {width}x{height}: one line to share:\n{}",
                screen.join("\n")
            );
            let casts = stats.casts().total();
            let expected = format!(
                "Sigilfall daily {day} - depth {}, turn {}, {} slain, {casts} {}",
                summary_value(&screen, "Depth reached"),
                summary_value(&screen, "Turns"),
                summary_value(&screen, "Creatures slain"),
                if casts == 1 { "sigil" } else { "sigils" },
            );
            assert_eq!(
                lines[0], expected,
                "{day}: the line says what the summary says"
            );
            assert!(lines[0].is_ascii(), "{}", lines[0]);
            assert!(lines[0].chars().count() <= 80, "{}", lines[0]);
            assert_eq!(summary_value(&screen, "Sigils cast"), casts.to_string());
            // The seed and the replay command stay on the screen.
            let text = screen.join("\n");
            assert!(text.contains(&Seed::daily(*day).to_string()), "{text}");
            assert!(
                text.contains(&format!("--seed {}", Seed::daily(*day))),
                "{text}"
            );
        }
        // The line sits under the replay command, in the block, and alone.
        let screen = rows(app, 80, 24);
        let replay = screen
            .iter()
            .position(|row| row.contains("Replay this seed"))
            .unwrap();
        let line = screen
            .iter()
            .position(|row| row.contains("Sigilfall daily"))
            .unwrap();
        assert_eq!(line, replay + 1, "right under the replay command");
        with_kills += usize::from(stats.total_kills() > 0);
        with_casts += usize::from(stats.casts().total() > 0);
    }
    assert!(with_kills >= 1, "at least one run killed something");
    assert!(with_casts >= 1, "at least one run cast a sigil");
}

#[test]
fn the_line_is_plain_ascii_in_every_mode_and_with_the_ascii_option() {
    let (day, app) = daily_endings(date(2026, 11, 1), 1, &["753"]).remove(0);
    let expected = share_lines(&rows(&app, 80, 24));
    assert_eq!(expected.len(), 1);
    assert!(expected[0].contains(&format!("daily {day}")));

    let game = app.game().clone();
    let content = shipped();
    for (glyphs, colors, appearance) in [
        (GlyphSet::Unicode, ColorChoice::Auto, Appearance::Dark),
        (GlyphSet::Unicode, ColorChoice::Auto, Appearance::Light),
        (GlyphSet::Ascii, ColorChoice::Auto, Appearance::Dark),
        (GlyphSet::Unicode, ColorChoice::Ansi16, Appearance::Dark),
        (GlyphSet::Ascii, ColorChoice::Monochrome, Appearance::Dark),
    ] {
        let settings = crate::theme::Settings {
            options: Options {
                glyphs,
                colors,
                appearance,
            },
            detected: ColorSupport::TrueColor,
            forced_ascii: false,
        };
        let mut shown = App::with_game(settings.theme(), game.clone(), content.clone());
        shown.handle_key(press(KeyCode::Char('.')));
        assert_eq!(
            shown.state(),
            State::GameOver,
            "a game that is over opens on its summary"
        );
        let lines = share_lines(&rows(&shown, 80, 24));
        assert_eq!(
            lines, expected,
            "{glyphs:?}/{colors:?}/{appearance:?}: the same words"
        );
    }
}

#[test]
fn a_normal_run_shares_its_seed_and_a_one_sigil_run_says_sigil() {
    let content = shipped();
    let mut checked = 0;
    let mut singular = 0;
    for value in [3_u64, 11, 21, 33, 58, 64, 99, 123] {
        let (game, glyph) = crowded(
            Game::new(Seed::from(value), &content),
            &["ossuary_rat", "wax_beetle"],
        );
        let Some(app) = play_to_the_end(game, glyph, &["753"], midnight(2026, 11, 8)) else {
            continue;
        };
        let game = app.game();
        assert_eq!(game.daily_date(), None);
        let screen = rows(&app, 80, 24);
        let lines = share_lines(&screen);
        assert_eq!(lines.len(), 1, "seed {value}:\n{}", screen.join("\n"));
        let casts = game.stats().casts().total();
        let noun = if casts == 1 { "sigil" } else { "sigils" };
        let expected = format!(
            "Sigilfall seed {} - depth {}, turn {}, {} slain, {casts} {noun}",
            game.seed(),
            game.stats().deepest(),
            game.player_turns(),
            game.stats().total_kills()
        );
        assert_eq!(lines[0], expected, "seed {value}");
        assert!(!lines[0].contains("daily"), "{}", lines[0]);
        assert!(!lines[0].ends_with("1 sigils"), "{}", lines[0]);
        singular += usize::from(casts == 1);
        checked += 1;
    }
    assert!(checked >= 3, "only {checked} runs reached the summary");
    assert!(
        singular >= 1,
        "no run cast exactly one sigil, so the singular was not seen"
    );
}

#[test]
fn two_players_who_play_the_same_daily_run_share_the_same_line() {
    let first = daily_endings(date(2026, 11, 8), 1, &["753"]);
    let second = daily_endings(date(2026, 11, 8), 1, &["753"]);
    let (a, b) = (&first[0], &second[0]);
    assert_eq!(a.0, b.0);
    let line_a = share_lines(&rows(&a.1, 80, 24));
    let line_b = share_lines(&rows(&b.1, 80, 24));
    assert_eq!(line_a, line_b, "lines compare as text");
    assert_eq!(line_a.len(), 1);
}

#[test]
fn no_line_to_share_is_shown_while_the_game_is_on_and_none_after_returning_to_the_title() {
    let folder = ScratchFolder::new("f11-no-line");
    let app = launch(&folder, &["--daily"]).unwrap();
    for (width, height) in SIZES {
        assert!(
            !rows(&app, width, height)
                .join("\n")
                .contains("Sigilfall daily"),
            "{width}x{height}"
        );
    }
    let (_, mut ended) = daily_endings(date(2026, 11, 1), 1, &["753"]).remove(0);
    let clock = ManualClock::new();
    ended = ended.with_clock(clock.clone());
    clock.advance(crate::app::END_SCREEN_GRACE);
    assert!(text(&ended).contains("Sigilfall daily"));
    ended.handle_key(press(KeyCode::Enter));
    assert!(matches!(ended.state(), State::Title(_)));
    assert!(!text(&ended).contains("Sigilfall daily"));
    ended.handle_key(ctrl_c());
    assert_eq!(ended.state(), State::Quit);
}
