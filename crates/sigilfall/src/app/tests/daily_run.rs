//! The daily dungeon (T28), driven by simulated keys on scratch folders
//! and an injected clock only: "Daily" on the title screen with the UTC
//! date of the clock, which turns at midnight; the daily run started from
//! the menu (asking first over a saved game) and from `--daily`, which is
//! refused while a game is saved, as `--seed` is; the date on the status
//! line; the daily run saved, resumed on another day and still recognised;
//! and the line to share on the death summary of a daily run and of a run
//! of a seed.

use std::fs;
use std::time::Duration;

use sigilfall_core::rng::DailyDate;

use super::save_files::{WALK, state_of, type_keys};
use super::*;
use crate::persist::ScratchFolder;
use crate::theme::ColorSupport;
use crate::ui::title::{Confirmation, NewRun, TitleItem, TitleMenu};

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
