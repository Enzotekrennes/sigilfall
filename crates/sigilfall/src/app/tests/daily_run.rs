//! The daily dungeon, driven by simulated keys on scratch folders and an
//! injected clock only: "Daily" on the title screen with the UTC date of the
//! clock, which turns at midnight; the daily run started from the menu (asking
//! first over a saved game) and from `--daily`, which is refused while a game
//! is saved, as `--seed` is; the date on the status line; the daily run saved,
//! resumed on another day and still recognised; and the line to share on the
//! death summary of a daily run and of a run of a seed.
//!
//! The tests written from the player's side check that two players of the same
//! UTC date get the same dungeon (whatever the hour and the way they start
//! it), that the date turns at midnight UTC, that `--daily` and `--seed`
//! exclude each other, the "Daily" entry of the title with its date, the date
//! a daily run keeps when it is resumed on another day, and the line to share
//! on the end screen, whose numbers are read back from the same screen and from
//! the core's statistics. They are written independently of the unit tests of
//! `app` and `messages`: the dates are turned into Unix time here, the share
//! line is parsed from the drawn screen, and its numbers are compared with
//! those the core counted.

use super::player_view::{
    SIZES, ctrl_c, go_to, launch_with_arguments, launch_with_terminal, rows, shipped, text,
    type_text,
};
use super::save_files::{WALK, state_of, type_keys};
use super::*;
use crate::app::{App, State};
use crate::cli::Cli;
use crate::clock::ManualClock;
use crate::persist::ScratchFolder;
use crate::theme::{Appearance, ColorChoice, ColorSupport, GlyphSet, Options};
use crate::ui::title::{Confirmation, NewRun, TitleItem, TitleMenu};
use clap::Parser as _;
use clap::error::ErrorKind;
use ratatui::crossterm::event::KeyCode;
use sigilfall_core::game::{Game, Status};
use sigilfall_core::geometry::Direction;
use sigilfall_core::rng::{DailyDate, Seed};
use std::fs;
use std::time::Duration;

/// Unix time of 2026-11-08 at 23:59:59 UTC, one second before the date
/// turns.
const LAST_SECOND_OF_NOV_8: u64 = ManualClock::START + 12 * 3600 - 1;

/// A monster that kills the player in one blow.
const EXECUTIONER: &str = r#"[
    Monster(id: "executioner", name: "executioner", family: Acolyte, elite: true,
        speed: 100, hp: 999, attack: 99, defense: 0, min_depth: 1),
]"#;

/// The date `year`-`month`-`day`, which the test knows to exist.
fn date(year: u16, month: u8, day: u8) -> DailyDate {
    DailyDate::new(year, month, day).expect("a real date")
}

/// The day [`ManualClock::new`] starts on.
fn clock_day() -> DailyDate {
    date(2026, 11, 8)
}

/// The command line `sigilfall`, with `--daily` when `daily` is true.
const fn command_line(daily: bool) -> Cli {
    Cli {
        seed: None,
        daily,
        ascii: false,
    }
}

/// Launches the application as `main` does on `folder`, with `--daily`
/// when `daily` is true, reading the time from `clock`.
fn launch_at(folder: &ScratchFolder, daily: bool, clock: &ManualClock) -> anyhow::Result<App> {
    App::launch_with_clock(
        ColorSupport::TrueColor,
        &command_line(daily),
        &shipped_content(),
        folder.save_file(),
        Box::new(clock.clone()),
    )
}

/// The rows of `app` drawn on an 80 × 24 screen.
fn screen_rows(app: &App) -> Vec<String> {
    let buffer = render(app, 80, 24).backend().buffer().clone();
    (0..24).map(|y| row_text(&buffer, y)).collect()
}

/// The row of the title menu that holds "Daily".
fn daily_row(app: &App) -> String {
    screen_rows(app)
        .into_iter()
        .find(|row| row.contains(messages::DAILY_ITEM))
        .expect("a Daily line")
}

/// Moves the title menu of `app` onto "Daily" and presses Enter.
fn choose_daily(app: &mut App) {
    press_all(app, &[KeyCode::End, KeyCode::Up, KeyCode::Up]);
    let State::Title(menu) = app.state() else {
        panic!("on the title menu: {:?}", app.state());
    };
    assert_eq!(menu.chosen(app.has_saved_game()), TitleItem::Daily);
    app.handle_key(press(KeyCode::Enter));
}

/// A scratch folder holding the save of a game of [`SEED`] played for a
/// few keys and quit.
fn folder_with_a_save(label: &str) -> ScratchFolder {
    let folder = ScratchFolder::new(label);
    let mut app = App::with_game(
        dark(),
        Game::new(SEED, &shipped_content()),
        shipped_content(),
    )
    .saving_to(folder.save_file());
    type_keys(&mut app, &WALK[..6]);
    app.save_on_exit().unwrap();
    folder
}

/// The default theme of Windows Terminal.
fn dark() -> Theme {
    Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode)
}

#[test]
fn the_title_lists_daily_with_the_utc_date_of_the_clock() {
    let folder = ScratchFolder::new("daily-title");
    let clock = ManualClock::new();

    let app = launch_at(&folder, false, &clock).unwrap();

    assert_eq!(app.state(), State::Title(TitleMenu::open()));
    let row = daily_row(&app);
    assert!(row.contains("  Daily       2026-11-08"), "{row}");
}

#[test]
fn the_date_on_the_title_turns_at_midnight_utc() {
    let folder = ScratchFolder::new("daily-midnight");
    let clock = ManualClock::at_unix_seconds(LAST_SECOND_OF_NOV_8);
    let mut app = launch_at(&folder, false, &clock).unwrap();
    assert!(daily_row(&app).contains("2026-11-08"));

    clock.advance(Duration::from_secs(1));
    assert!(daily_row(&app).contains("2026-11-09"), "drawn anew");

    choose_daily(&mut app);
    assert_eq!(app.state(), State::Playing);
    assert_eq!(app.game().daily_date(), Some(date(2026, 11, 9)));
}

#[test]
fn daily_on_the_title_starts_the_daily_dungeon_of_the_day() {
    let folder = ScratchFolder::new("daily-menu");
    let clock = ManualClock::new();
    let mut app = launch_at(&folder, false, &clock).unwrap();

    choose_daily(&mut app);

    assert_eq!(app.state(), State::Playing);
    assert_eq!(app.game().daily_date(), Some(clock_day()));
    assert_eq!(app.game().seed(), Seed::daily(clock_day()));
    assert_eq!(
        state_of(app.game()),
        state_of(&Game::daily(clock_day(), &shipped_content())),
        "the same dungeon for every player of the day"
    );
    let status = &screen_rows(&app)[0];
    assert!(status.ends_with(" Daily 2026-11-08 "), "{status:?}");
    assert!(!status.contains("Seed"), "{status:?}");
}

#[test]
fn a_run_of_a_seed_keeps_the_seed_on_the_status_line() {
    let folder = ScratchFolder::new("daily-not");
    let clock = ManualClock::new();
    let mut app = launch_at(&folder, false, &clock).unwrap();
    app.handle_key(press(KeyCode::Enter));

    assert_eq!(app.game().daily_date(), None, "New game is not daily");
    let status = &screen_rows(&app)[0];
    assert!(
        status.ends_with(&format!(" Seed {} ", app.game().seed())),
        "{status:?}"
    );
}

#[test]
fn daily_over_a_saved_game_asks_first_and_keep_comes_back_to_daily() {
    let folder = folder_with_a_save("daily-confirm");
    let before = fs::read(folder.save_file().path()).unwrap();
    let clock = ManualClock::new();
    let mut app = launch_at(&folder, false, &clock).unwrap();

    choose_daily(&mut app);
    assert_eq!(
        app.state(),
        State::Confirm(Confirmation::new(NewRun::Daily(clock_day())))
    );
    app.handle_key(press(KeyCode::Enter));
    assert_eq!(
        app.state(),
        State::Title(TitleMenu::on(TitleItem::Daily, true)),
        "Keep goes back to Daily"
    );
    assert_eq!(fs::read(folder.save_file().path()).unwrap(), before);

    app.handle_key(press(KeyCode::Enter));
    press_all(&mut app, &[KeyCode::Down, KeyCode::Enter]);

    assert_eq!(app.state(), State::Playing);
    assert_eq!(app.game().daily_date(), Some(clock_day()));
    assert!(!folder.save_file().path().exists(), "the old save is gone");
}

#[test]
fn daily_on_the_command_line_starts_the_daily_dungeon_at_once() {
    let folder = ScratchFolder::new("daily-cli");
    let clock = ManualClock::at_unix_seconds(LAST_SECOND_OF_NOV_8 + 1);

    let app = launch_at(&folder, true, &clock).unwrap();

    assert_eq!(app.state(), State::Playing, "no title screen");
    assert_eq!(app.game().daily_date(), Some(date(2026, 11, 9)));
    assert_eq!(app.game().seed(), Seed::daily(date(2026, 11, 9)));
}

#[test]
fn daily_on_the_command_line_is_refused_while_a_game_is_saved() {
    let folder = folder_with_a_save("daily-refused");
    let before = fs::read(folder.save_file().path()).unwrap();

    let error = launch_at(&folder, true, &ManualClock::new()).unwrap_err();
    let message = format!("{error:#}");

    assert!(message.contains("without --daily"), "{message}");
    assert!(message.contains("New game, Seed or Daily"), "{message}");
    assert!(message.contains("title screen"), "{message}");
    assert!(message.contains("delete that file"), "{message}");
    assert!(
        message.contains("the daily dungeon of 2026-11-08"),
        "{message}"
    );
    assert_eq!(
        fs::read(folder.save_file().path()).unwrap(),
        before,
        "the save is untouched"
    );
}

#[test]
fn a_daily_run_is_saved_and_resumed_as_the_daily_run_of_its_date() {
    let folder = ScratchFolder::new("daily-resume");
    let clock = ManualClock::new();
    let mut app = launch_at(&folder, true, &clock).unwrap();
    type_keys(&mut app, &WALK[..12]);
    type_keys(&mut app, &['q']);
    app.save_on_exit().unwrap();
    let played = state_of(app.game());

    // Resumed the next day: still the daily run of the day it began.
    clock.advance(Duration::from_secs(24 * 3600));
    let mut resumed = launch_at(&folder, false, &clock).unwrap();
    assert!(resumed.has_saved_game());
    assert!(daily_row(&resumed).contains("2026-11-09"));
    resumed.handle_key(press(KeyCode::Enter));

    assert_eq!(resumed.state(), State::Playing);
    assert_eq!(state_of(resumed.game()), played, "exactly as it was");
    assert_eq!(resumed.game().daily_date(), Some(clock_day()));
    let status = &screen_rows(&resumed)[0];
    assert!(status.ends_with(" Daily 2026-11-08 "), "{status:?}");
}

/// `game` with an executioner next to the player, played until it kills
/// the player, then shown by an application on its end screen.
fn ended(mut game: Game) -> App {
    let arena = Content::parse(EXECUTIONER).expect("valid test monster");
    let player = game.player_actor().position();
    let beside = Direction::ALL
        .into_iter()
        .map(|direction| player.step(direction))
        .find(|&cell| {
            game.map().is_walkable(cell) && game.actors().all(|(_, a)| a.position() != cell)
        })
        .expect("a free cell beside the start");
    game.spawn_monster(beside, arena.monster("executioner").unwrap());
    for _ in 0..10 {
        if game.is_over() {
            break;
        }
        game.submit(Action::Wait);
    }
    assert!(game.is_over(), "the executioner kills");
    let app = App::with_game(dark(), game, shipped_content());
    assert_eq!(app.state(), State::GameOver);
    app
}

#[test]
fn the_death_summary_of_a_daily_run_has_a_line_to_share_with_the_date() {
    let app = ended(Game::daily(clock_day(), &shipped_content()));
    let stats = app.game().stats().clone();

    let rows = screen_rows(&app);
    let expected = format!(
        "Sigilfall daily 2026-11-08 - depth 1, turn {}, {} slain, {} sigils",
        stats.turns(),
        stats.total_kills(),
        stats.casts().total()
    );
    let share = rows
        .iter()
        .find(|row| row.trim() == expected)
        .unwrap_or_else(|| panic!("{expected}:\n{}", rows.join("\n")));

    assert!(share.is_ascii());
    let replay = rows
        .iter()
        .position(|row| row.contains("Replay this seed"))
        .expect("the replay command");
    assert_eq!(rows[replay + 1], *share, "right under the replay command");
    assert!(
        rows[replay].contains(&Seed::daily(clock_day()).to_string()),
        "the seed of the day still replays it: {}",
        rows[replay]
    );
}

#[test]
fn the_death_summary_of_a_run_of_a_seed_shares_the_seed() {
    let app = ended(Game::new(SEED, &shipped_content()));
    let stats = app.game().stats().clone();

    let rows = screen_rows(&app);
    let expected = format!(
        "Sigilfall seed 7F3A-91C2-0B5E-D4A8 - depth 1, turn {}, {} slain, {} sigils",
        stats.turns(),
        stats.total_kills(),
        stats.casts().total()
    );

    assert!(
        rows.iter().any(|row| row.trim() == expected),
        "{expected}:\n{}",
        rows.join("\n")
    );
    assert!(!rows.iter().any(|row| row.contains("daily")));
}

/// Seconds in a UTC day.
const DAY: u64 = 86_400;

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
fn daily_entry_row(app: &App) -> String {
    rows(app, 80, 24)
        .into_iter()
        .find(|row| row.contains("Daily"))
        .expect("a Daily entry")
}

// --- The same UTC date gives the same dungeon -------------------

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
        let folder = ScratchFolder::new("same-date");
        let app = launch_with_terminal(
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
    let folder = ScratchFolder::new("same-date-title");
    let mut from_title = launch_with_terminal(
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
    let other = ScratchFolder::new("same-date-seed");
    let by_seed = launch_with_terminal(
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
    let before = ScratchFolder::new("midnight-a");
    let one = launch_with_terminal(
        &before,
        &["--daily"],
        ColorSupport::TrueColor,
        &clock_at(last),
    )
    .unwrap();
    let after = ScratchFolder::new("midnight-b");
    let two = launch_with_terminal(
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
    let folder = ScratchFolder::new("midnight-title");
    let clock = clock_at(last);
    let mut title = launch_with_terminal(&folder, &[], ColorSupport::TrueColor, &clock).unwrap();
    assert!(
        daily_entry_row(&title).contains("2026-11-08"),
        "{}",
        daily_entry_row(&title)
    );
    clock.advance(std::time::Duration::from_secs(1));
    assert!(
        daily_entry_row(&title).contains("2026-11-09"),
        "{}",
        daily_entry_row(&title)
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
        let folder = ScratchFolder::new("boundary");
        let app = launch_with_terminal(
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
        let folder = ScratchFolder::new("thirty");
        let app = launch_with_terminal(
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
    let folder = ScratchFolder::new("daily-saved");
    let game = Game::new(Seed::from(42), &shipped());
    folder.save_file().write(&game).unwrap();
    let saved = fs::read(folder.save_file().path()).unwrap();

    let error =
        launch_with_arguments(&folder, &["--daily"]).expect_err("refused while a game is saved");
    let message = format!("{error:#}");
    assert!(message.contains("--daily"), "{message}");
    assert!(message.contains("a game in progress is saved"), "{message}");
    assert_eq!(fs::read(folder.save_file().path()).unwrap(), saved);
    assert!(
        launch_with_arguments(&folder, &["--seed", "5"]).is_err(),
        "--seed is refused the same way"
    );
}

// --- The title shows Daily with the date -------------------------------------

#[test]
fn the_title_lists_daily_after_seed_with_the_date_of_the_clock() {
    let folder = ScratchFolder::new("title");
    let app = launch_with_terminal(
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
        daily_entry_row(&app)
            .split_whitespace()
            .eq(["Daily", "2026-11-08"])
            || daily_entry_row(&app).contains("› Daily")
    );
}

// --- The date is kept after a resume another day ------------------------------

#[test]
fn a_daily_run_resumed_three_days_later_is_still_the_run_of_its_date() {
    let folder = ScratchFolder::new("resume");
    let day = date(2026, 11, 8);
    let start = clock_at(midnight(2026, 11, 8) + 9 * 3600);
    let mut app =
        launch_with_terminal(&folder, &["--daily"], ColorSupport::TrueColor, &start).unwrap();
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
    let mut resumed = launch_with_terminal(&folder, &[], ColorSupport::TrueColor, &later).unwrap();
    assert!(
        daily_entry_row(&resumed).contains("2026-11-11"),
        "{}",
        daily_entry_row(&resumed)
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
    let control_folder = ScratchFolder::new("resume-control");
    let mut control = launch_with_terminal(
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
    let mut again = launch_with_terminal(
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
    let plain = ScratchFolder::new("resume-plain");
    let mut run = launch_with_arguments(&plain, &["--seed", "42"]).unwrap();
    run.handle_key(press(KeyCode::Char('q')));
    run.save_on_exit().unwrap();
    let mut back = launch_with_terminal(&plain, &[], ColorSupport::TrueColor, &later).unwrap();
    back.handle_key(press(KeyCode::Enter));
    let status = &rows(&back, 80, 24)[0];
    assert!(
        status.contains("Seed 0000-0000-0000-002A") && !status.contains("Daily"),
        "{status}"
    );
}

// --- The end of a run gives one line to share ----------------------

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
    let folder = ScratchFolder::new("share");
    folder.save_file().write(&game).unwrap();
    let mut app =
        launch_with_terminal(&folder, &[], ColorSupport::TrueColor, &clock_at(time)).unwrap();
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
    let folder = ScratchFolder::new("no-line");
    let app = launch_with_arguments(&folder, &["--daily"]).unwrap();
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
