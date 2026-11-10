//! The title screen, driven by simulated keys on scratch folders only: the menu
//! with and without "Continue", its order, its moves and the quit keys, a new
//! game with and without a saved one (and the confirmation that guards it), the
//! seed entry with valid and invalid seeds, the way back from the end screen,
//! `--seed` and `--ascii` on the command line, the styles of the chosen line in
//! every rendering mode, and the snapshots of each step.

use super::player_view::{
    SIZES, chosen, ctrl_c, files, go_to, launch_with_arguments, row_of, rows, save_game, text,
};
use super::save_files::{WALK, doomed_app, launch, log_lines, state_of, type_keys, wait_for_death};
use super::*;
use crate::app::{App, State};
use crate::cli::Cli;
use crate::clock::ManualClock;
use crate::persist::ScratchFolder;
use crate::theme::{Appearance, ColorChoice, ColorSupport, Options};
use crate::ui::title::{Confirmation, SeedEntry, SeedProblem, TitleItem, TitleMenu};
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::KeyCode;
use ratatui::style::{Color, Modifier};
use sigilfall_core::game::Game;
use sigilfall_core::rng::Seed;
use std::fs;

/// The command line `sigilfall`, with `--seed` and `--ascii` as given.
const fn command_line(seed: Option<Seed>, ascii: bool) -> Cli {
    Cli {
        seed,
        daily: false,
        ascii,
        lang: None,
    }
}

/// Launches the application as `main` does on `folder`, from `cli`, on a
/// terminal detected as 24-bit.
fn launch_with(folder: &ScratchFolder, cli: &Cli) -> App {
    App::launch_with_clock(
        ColorSupport::TrueColor,
        Language::English,
        cli,
        &shipped_content(),
        folder.save_file(),
        Box::new(ManualClock::new()),
    )
    .expect("the launch succeeds")
}

/// A scratch folder holding the save of a game of [`SEED`] played for a
/// few keys and quit, and that game.
fn folder_with_a_save(label: &str) -> (ScratchFolder, Game) {
    let folder = ScratchFolder::new(label);
    let mut app = launch(&folder, Some(SEED)).unwrap();
    type_keys(&mut app, &WALK[..8]);
    type_keys(&mut app, &['q']);
    app.save_on_exit().unwrap();
    let game = app.game().clone();
    (folder, game)
}

/// The text of `app` drawn on an 80 × 24 screen, one line per row.
fn screen_text(app: &App) -> String {
    render(app, 80, 24).backend().to_string()
}

/// The rows of `app` drawn on an 80 × 24 screen, without the quotes the
/// test backend's text adds.
fn screen_rows(app: &App) -> Vec<String> {
    let buffer = render(app, 80, 24).backend().buffer().clone();
    (0..24).map(|y| row_text(&buffer, y)).collect()
}

/// What the title screen writes beside "Continue" for `game`.
fn saved_detail(game: &Game) -> String {
    messages::saved_run(
        Language::English,
        game.depth(),
        game.player_turns() + 1,
        '·',
    )
}

/// Presses each key of `codes`, in order.
fn keys(app: &mut App, codes: &[KeyCode]) {
    press_all(app, codes);
}

/// Types `text` into the application, one character per key press.
fn type_text(app: &mut App, text: &str) {
    for typed in text.chars() {
        app.handle_key(press(KeyCode::Char(typed)));
    }
}

#[test]
fn without_a_save_the_title_offers_new_game_seed_daily_options_and_quit() {
    let folder = ScratchFolder::new("title-fresh");

    let app = launch(&folder, None).unwrap();

    assert_eq!(app.state(), State::Title(TitleMenu::open()));
    assert!(!app.has_saved_game());
    let text = screen_text(&app);
    for needle in [
        "S I G I L F A L L",
        "Trace true. The dungeon listens.",
        "› New game",
        "  Seed",
        "  Daily       2026-11-08",
        "  Options",
        "  Quit",
        " ↑↓ choose   Enter confirm   q quit",
        "v0.1.0",
    ] {
        assert!(text.contains(needle), "{needle}:\n{text}");
    }
    assert!(!text.contains("Continue"), "{text}");
    assert_snapshot!("title_without_continue", text);
}

#[test]
fn with_a_save_continue_comes_first_with_the_depth_and_turn_of_the_game() {
    let (folder, saved) = folder_with_a_save("title-saved");

    let app = launch(&folder, None).unwrap();

    assert!(app.has_saved_game());
    let text = screen_text(&app);
    let continue_row = screen_rows(&app)
        .into_iter()
        .find(|row| row.contains("Continue"))
        .expect("a Continue line");
    assert!(continue_row.contains("› Continue"), "{continue_row}");
    assert!(
        continue_row.contains(&saved_detail(&saved)),
        "{continue_row}"
    );
    assert!(text.contains("  New game"), "{text}");
    assert_snapshot!("title_with_continue", text);
}

#[test]
fn the_menu_wraps_around_and_every_entry_is_reachable() {
    let (folder, _) = folder_with_a_save("title-moves");
    let mut app = launch(&folder, None).unwrap();
    let on = |item| State::Title(TitleMenu::on(item, true));

    keys(&mut app, &[KeyCode::Up]);
    assert_eq!(app.state(), on(TitleItem::Quit), "from the first, the last");
    keys(&mut app, &[KeyCode::Down]);
    assert_eq!(
        app.state(),
        on(TitleItem::Continue),
        "from the last, the first"
    );
    for item in [
        TitleItem::NewGame,
        TitleItem::Seed,
        TitleItem::Daily,
        TitleItem::Options,
        TitleItem::Quit,
    ] {
        keys(&mut app, &[KeyCode::Char('j')]);
        assert_eq!(app.state(), on(item));
    }
    keys(&mut app, &[KeyCode::Home]);
    assert_eq!(app.state(), on(TitleItem::Continue));
    keys(&mut app, &[KeyCode::End]);
    assert_eq!(app.state(), on(TitleItem::Quit));
    keys(&mut app, &[KeyCode::Char('k')]);
    assert_eq!(app.state(), on(TitleItem::Options));
    let text = screen_text(&app);
    assert!(text.contains("› Options"), "{text}");
    assert!(text.contains("  Continue"), "{text}");
}

#[test]
fn escape_does_nothing_and_q_or_quit_leave_with_the_save_untouched() {
    let (folder, _) = folder_with_a_save("title-quit");
    let before = fs::read(folder.save_file().path()).unwrap();

    let mut app = launch(&folder, None).unwrap();
    keys(&mut app, &[KeyCode::Esc, KeyCode::Esc]);
    assert_eq!(
        app.state(),
        State::Title(TitleMenu::open()),
        "Escape never quits"
    );
    keys(&mut app, &[KeyCode::Char('q')]);
    assert_eq!(app.state(), State::Quit);
    app.save_on_exit().unwrap();
    assert_eq!(fs::read(folder.save_file().path()).unwrap(), before);

    let mut app = launch(&folder, None).unwrap();
    keys(&mut app, &[KeyCode::End, KeyCode::Enter]);
    assert_eq!(app.state(), State::Quit, "the Quit entry");
    app.save_on_exit().unwrap();
    assert_eq!(fs::read(folder.save_file().path()).unwrap(), before);
}

#[test]
fn continue_resumes_the_saved_game_exactly() {
    let (folder, saved) = folder_with_a_save("title-continue");
    let mut app = launch(&folder, None).unwrap();

    keys(&mut app, &[KeyCode::Enter]);

    assert_eq!(app.state(), State::Playing);
    assert_eq!(state_of(app.game()), state_of(&saved));
    assert_eq!(
        log_lines(&app),
        vec![messages::welcome_back(Language::English)]
    );
    assert!(!app.has_saved_game(), "resumed, not offered twice");
}

#[test]
fn new_game_without_a_save_starts_at_once_on_a_random_seed() {
    let first_folder = ScratchFolder::new("title-new-1");
    let second_folder = ScratchFolder::new("title-new-2");

    let mut first = launch(&first_folder, None).unwrap();
    let mut second = launch(&second_folder, None).unwrap();
    keys(&mut first, &[KeyCode::Enter]);
    keys(&mut second, &[KeyCode::Enter]);

    assert_eq!(first.state(), State::Playing);
    assert_eq!(first.game().player_turns(), 0);
    assert_eq!(
        log_lines(&first),
        vec![messages::welcome(Language::English)]
    );
    // Two draws of 64 random bits are equal once in 2^64 runs.
    assert_ne!(first.game().seed(), second.game().seed());
}

#[test]
fn new_game_with_a_save_asks_first_and_keeps_the_save_unless_told_otherwise() {
    let (folder, saved) = folder_with_a_save("title-confirm");
    let before = fs::read(folder.save_file().path()).unwrap();
    let mut app = launch(&folder, None).unwrap();
    let asking = State::Confirm(Confirmation::new(NewRun::Random));

    keys(&mut app, &[KeyCode::Down, KeyCode::Enter]);
    assert_eq!(app.state(), asking);
    let text = screen_text(&app);
    for needle in [
        "Start a new game?".to_owned(),
        format!(
            "The game in progress ({}) will be abandoned",
            saved_detail(&saved)
        ),
        "and its save deleted for good.".to_owned(),
        "› Keep the game in progress".to_owned(),
        "  Abandon it and start anew".to_owned(),
        " ↑↓ choose   Enter confirm   Esc keep it".to_owned(),
    ] {
        assert!(text.contains(&needle), "{needle}:\n{text}");
    }
    assert_snapshot!("confirmation", text);

    // Enter on "Keep", which is chosen first, keeps the game.
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(
        app.state(),
        State::Title(TitleMenu::on(TitleItem::NewGame, true))
    );
    // So do Escape and q pressed for "no", which does not quit here.
    keys(&mut app, &[KeyCode::Enter, KeyCode::Char('q')]);
    assert_eq!(app.state(), asking, "q does nothing on the confirmation");
    keys(&mut app, &[KeyCode::Esc]);
    assert_eq!(
        app.state(),
        State::Title(TitleMenu::on(TitleItem::NewGame, true))
    );
    assert!(app.has_saved_game());
    assert_eq!(fs::read(folder.save_file().path()).unwrap(), before);

    // Choosing "Abandon" deletes the save and starts the new game.
    keys(&mut app, &[KeyCode::Enter, KeyCode::Down]);
    assert!(screen_text(&app).contains("› Abandon it and start anew"));
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.state(), State::Playing);
    assert!(!folder.save_file().path().exists(), "the save is deleted");
    assert!(!app.has_saved_game());
    assert_eq!(app.game().player_turns(), 0);
    assert_ne!(app.game().seed(), saved.seed());
    assert_eq!(log_lines(&app), vec![messages::welcome(Language::English)]);
}

#[test]
fn a_save_that_cannot_be_deleted_starts_no_game_and_says_why() {
    let (folder, _) = folder_with_a_save("title-undeletable");
    let mut app = launch(&folder, None).unwrap();
    // Swapped for a folder after the launch read it: deleting it fails.
    fs::remove_file(folder.save_file().path()).unwrap();
    fs::create_dir(folder.save_file().path()).unwrap();

    keys(
        &mut app,
        &[KeyCode::Down, KeyCode::Enter, KeyCode::Down, KeyCode::Enter],
    );

    assert_eq!(
        app.state(),
        State::Title(TitleMenu::on(TitleItem::NewGame, true))
    );
    let text = screen_text(&app);
    assert!(
        text.contains("! No new game was started: the save"),
        "{text}"
    );
    assert!(app.has_saved_game(), "still offered");
    // The notice goes once a game starts.
    keys(&mut app, &[KeyCode::Up, KeyCode::Enter]);
    assert_eq!(app.state(), State::Playing);
}

#[test]
fn a_typed_seed_starts_that_dungeon_in_either_form() {
    for (typed, seed) in [
        ("7F3A-91C2-0B5E-D4A8", SEED),
        ("7f3a-91c2-0b5e-d4a8", SEED),
        ("42", Seed::new(42)),
        ("  42 ", Seed::new(42)),
        ("18446744073709551615", Seed::new(u64::MAX)),
    ] {
        let folder = ScratchFolder::new("title-seed");
        let mut app = launch(&folder, None).unwrap();
        keys(&mut app, &[KeyCode::Down, KeyCode::Enter]);
        assert_eq!(app.state(), State::SeedEntry(SeedEntry::open()), "{typed}");

        type_text(&mut app, typed);
        keys(&mut app, &[KeyCode::Enter]);

        assert_eq!(app.state(), State::Playing, "{typed}");
        assert_eq!(app.game().seed(), seed, "{typed}");
        assert_eq!(app.game().player_turns(), 0, "{typed}");
    }
}

#[test]
fn the_seed_entry_shows_the_field_and_the_accepted_forms() {
    let folder = ScratchFolder::new("title-seed-screen");
    let mut app = launch(&folder, None).unwrap();
    keys(&mut app, &[KeyCode::Down, KeyCode::Enter]);

    type_text(&mut app, "7F3A-91C2");

    let text = screen_text(&app);
    for needle in [
        "Type the seed of the dungeon, then press Enter.",
        "Seed  7F3A-91C2_",
        "A number such as 42, or the shared form 7F3A-91C2-0B5E-D4A8.",
        " Enter start   Backspace erase   Esc back",
    ] {
        assert!(text.contains(needle), "{needle}:\n{text}");
    }
    assert_snapshot!("seed_entry", text);
}

#[test]
fn an_invalid_seed_is_refused_with_a_clear_message_and_can_be_corrected() {
    let folder = ScratchFolder::new("title-seed-invalid");
    let mut app = launch(&folder, None).unwrap();
    keys(&mut app, &[KeyCode::Down, KeyCode::Enter]);

    // q is typed like any letter, and does not quit.
    type_text(&mut app, "7F3A-91C2q");
    keys(&mut app, &[KeyCode::Enter]);

    let State::SeedEntry(entry) = app.state() else {
        panic!("the entry stays open: {:?}", app.state());
    };
    assert_eq!(entry.problem(), Some(SeedProblem::Invalid));
    assert_eq!(entry.text(), "7F3A-91C2q", "kept for correction");
    let text = screen_text(&app);
    assert!(
        text.contains("! \"7F3A-91C2q\" is not a seed: use one of the forms above."),
        "{text}"
    );
    assert_snapshot!("seed_entry_refused", text);

    // Erasing clears the message.
    keys(&mut app, &[KeyCode::Backspace]);
    assert!(!screen_text(&app).contains("is not a seed"));
    // An empty field is refused with its own message.
    for _ in 0..12 {
        keys(&mut app, &[KeyCode::Backspace]);
    }
    keys(&mut app, &[KeyCode::Enter]);
    assert!(screen_text(&app).contains("! Type a seed first."));
    // A number out of range is not a seed either.
    type_text(&mut app, "18446744073709551616");
    keys(&mut app, &[KeyCode::Enter]);
    assert!(
        matches!(app.state(), State::SeedEntry(entry) if entry.problem() == Some(SeedProblem::Invalid))
    );

    // Escape goes back to the menu, on Seed.
    keys(&mut app, &[KeyCode::Esc]);
    assert_eq!(
        app.state(),
        State::Title(TitleMenu::on(TitleItem::Seed, false))
    );
}

#[test]
fn a_typed_seed_with_a_save_asks_first() {
    let (folder, _) = folder_with_a_save("title-seed-saved");
    let mut app = launch(&folder, None).unwrap();
    keys(&mut app, &[KeyCode::Down, KeyCode::Down, KeyCode::Enter]);
    type_text(&mut app, "42");

    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(
        app.state(),
        State::Confirm(Confirmation::new(NewRun::Seed(Seed::new(42))))
    );
    keys(&mut app, &[KeyCode::Esc]);
    assert_eq!(
        app.state(),
        State::Title(TitleMenu::on(TitleItem::Seed, true)),
        "kept: back on Seed"
    );
    assert!(folder.save_file().path().is_file());

    keys(&mut app, &[KeyCode::Enter]);
    type_text(&mut app, "42");
    keys(&mut app, &[KeyCode::Enter, KeyCode::End, KeyCode::Enter]);
    assert_eq!(app.state(), State::Playing);
    assert_eq!(app.game().seed(), Seed::new(42));
    assert!(!folder.save_file().path().exists());
}

#[test]
fn after_death_enter_goes_back_to_the_title_without_continue() {
    let folder = ScratchFolder::new("title-after-death");
    let clock = ManualClock::new();
    let mut app = doomed_app(folder.save_file()).with_clock(clock.clone());
    app.save_on_exit().unwrap();
    wait_for_death(&mut app);
    clock.advance(END_SCREEN_GRACE);
    let rows = screen_rows(&app);
    assert!(
        rows[23].starts_with(" Enter title screen   q quit"),
        "{rows:?}"
    );

    // Escape and a stray game key do nothing on the end screen.
    keys(&mut app, &[KeyCode::Esc, KeyCode::Char('.')]);
    assert_eq!(app.state(), State::GameOver);
    keys(&mut app, &[KeyCode::Enter]);

    assert_eq!(app.state(), State::Title(TitleMenu::open()));
    assert!(!app.has_saved_game());
    assert!(!folder.save_file().path().exists());
    let text = screen_text(&app);
    assert!(text.contains("› New game"), "{text}");
    assert!(!text.contains("Continue"), "{text}");
    // Nothing is saved when quitting from there.
    app.save_on_exit().unwrap();
    assert!(!folder.save_file().path().exists());

    // A new game starts from there, and is saved on quitting.
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.state(), State::Playing);
    keys(&mut app, &[KeyCode::Char('q')]);
    app.save_on_exit().unwrap();
    assert!(folder.save_file().path().is_file());
}

#[test]
fn seed_on_the_command_line_skips_the_title() {
    let folder = ScratchFolder::new("title-cli-seed");

    let app = launch_with(&folder, &command_line(Some(SEED), false));

    assert_eq!(app.state(), State::Playing);
    assert_eq!(app.game().seed(), SEED);
    let rows = screen_rows(&app);
    assert!(rows[0].contains("Seed 7F3A-91C2-0B5E-D4A8"), "{rows:?}");
    assert!(rows[23].ends_with("q save & quit "), "{rows:?}");
}

#[test]
fn ascii_on_the_command_line_draws_the_title_in_plain_ascii() {
    let (folder, _) = folder_with_a_save("title-cli-ascii");

    let mut app = launch_with(&folder, &command_line(None, true));

    let text = screen_text(&app);
    assert!(text.is_ascii(), "{text}");
    assert!(text.contains("> Continue"), "{text}");
    assert!(text.contains("O-----O-----O"), "the emblem: {text}");
    assert!(
        text.contains(" ^v choose   Enter confirm   q quit"),
        "{text}"
    );
    keys(&mut app, &[KeyCode::Down, KeyCode::Down, KeyCode::Enter]);
    let text = screen_text(&app);
    assert!(text.is_ascii(), "{text}");
    keys(
        &mut app,
        &[KeyCode::Esc, KeyCode::Up, KeyCode::Up, KeyCode::Enter],
    );
    assert_eq!(app.state(), State::Playing);
    assert!(screen_text(&app).is_ascii(), "the game too");
}

#[test]
fn a_terminal_too_small_shows_the_resize_message_and_only_quits() {
    let folder = ScratchFolder::new("title-small");
    let mut app = launch(&folder, None).unwrap();

    let text = render(&app, 60, 20).backend().to_string();
    assert!(text.contains("Enlarge the terminal to 80 × 24"), "{text}");
    keys(&mut app, &[KeyCode::Enter, KeyCode::Down]);
    assert_eq!(
        app.state(),
        State::Title(TitleMenu::open()),
        "nothing chosen blind"
    );

    render(&app, 80, 24);
    keys(&mut app, &[KeyCode::Down]);
    assert_eq!(
        app.state(),
        State::Title(TitleMenu::on(TitleItem::Seed, false))
    );
    render(&app, 60, 20);
    keys(&mut app, &[KeyCode::Char('q')]);
    assert_eq!(app.state(), State::Quit);
}

/// Launches the application on a fresh folder whose options file holds
/// `options`, on a terminal `detected` as such, and returns it with its
/// 80 × 24 screen.
fn title_in(options: Options, detected: ColorSupport, label: &str) -> (App, Buffer) {
    let folder = ScratchFolder::new(label);
    folder.options_file().write(&options).unwrap();
    let app = App::launch_with_clock(
        detected,
        Language::English,
        &command_line(None, false),
        &shipped_content(),
        folder.save_file(),
        Box::new(ManualClock::new()),
    )
    .unwrap();
    let buffer = render(&app, 80, 24).backend().buffer().clone();
    (app, buffer)
}

/// The cells of the row of `buffer` that holds `needle`, with its row and
/// the column where `needle` starts.
fn find(buffer: &Buffer, needle: &str) -> (u16, u16) {
    for y in 0..buffer.area.height {
        let row = row_text(buffer, y);
        if let Some(byte) = row.find(needle) {
            let column = row[..byte].chars().count();
            return (u16::try_from(column).unwrap(), y);
        }
    }
    panic!("{needle} is on screen");
}

#[test]
fn the_chosen_line_reads_without_colour_in_every_mode_and_the_light_theme() {
    let modes = [
        (Options::default(), ColorSupport::TrueColor, "dark"),
        (
            Options {
                appearance: Appearance::Light,
                ..Options::default()
            },
            ColorSupport::TrueColor,
            "light",
        ),
        (
            Options {
                colors: ColorChoice::Ansi16,
                ..Options::default()
            },
            ColorSupport::TrueColor,
            "ansi16",
        ),
        (Options::default(), ColorSupport::None, "monochrome"),
    ];
    for (options, detected, label) in modes {
        let (app, buffer) = title_in(options, detected, label);
        let theme = app.theme();

        let (x, y) = find(&buffer, "› New game");
        let marker = &buffer[(x, y)];
        let other = &buffer[find(&buffer, "Seed")];
        // The marker and the selection carry the choice in every mode.
        assert_eq!(marker.symbol(), "›", "{label}");
        let selected = &buffer[(x + 4, y)];
        match theme.bg_selection.bg {
            Some(selection) => {
                assert_eq!(selected.bg, selection, "{label}");
                assert_ne!(other.bg, selection, "{label}");
            }
            None => {
                assert!(selected.modifier.contains(Modifier::REVERSED), "{label}");
                assert!(!other.modifier.contains(Modifier::REVERSED), "{label}");
            }
        }
        assert_eq!(
            Some(selected.fg),
            theme.text.fg.or(Some(Color::Reset)),
            "{label}"
        );

        // The name in the title token, bold in every mode.
        let name = &buffer[find(&buffer, "S I G I L")];
        assert_eq!(
            Some(name.fg),
            theme.text_title.fg.or(Some(Color::Reset)),
            "{label}"
        );
        assert!(name.modifier.contains(Modifier::BOLD), "{label}");
        // The emblem's strokes in the sigil token, its free nodes apart.
        let stroke = &buffer[find(&buffer, "●─────●")];
        assert_eq!(
            Some(stroke.fg),
            theme.sigil_stroke.fg.or(Some(Color::Reset)),
            "{label}"
        );
        // The help: keys in text, labels muted.
        let help_key = &buffer[(1, 23)];
        assert_eq!(
            Some(help_key.fg),
            theme.text.fg.or(Some(Color::Reset)),
            "{label}"
        );
        // The background is painted everywhere.
        assert_eq!(Some(buffer[(0, 0)].bg), theme.bg.bg, "{label}");
    }
}

#[test]
fn the_light_theme_paints_the_parchment_on_the_title() {
    let (_, buffer) = title_in(
        Options {
            appearance: Appearance::Light,
            ..Options::default()
        },
        ColorSupport::TrueColor,
        "light-parchment",
    );

    assert_eq!(buffer[(0, 0)].bg, Color::Rgb(0xF2, 0xEB, 0xDC));
    let (x, y) = find(&buffer, "› New game");
    assert_eq!(buffer[(x + 4, y)].bg, Color::Rgb(0xD6, 0xCA, 0xB0));
    assert_eq!(buffer[(x + 4, y)].fg, Color::Rgb(0x2A, 0x22, 0x30));
}

#[test]
fn every_title_step_keeps_the_terminal_colours_in_monochrome() {
    let (folder, _) = folder_with_a_save("title-monochrome");
    let mut app = App::launch_with_clock(
        ColorSupport::None,
        Language::English,
        &command_line(None, false),
        &shipped_content(),
        folder.save_file(),
        Box::new(ManualClock::new()),
    )
    .unwrap();
    let steps: [&[KeyCode]; 4] = [
        &[],
        &[KeyCode::Down, KeyCode::Enter],
        &[KeyCode::Esc, KeyCode::Down, KeyCode::Enter],
        &[KeyCode::Esc, KeyCode::Down, KeyCode::Enter],
    ];
    for step in steps {
        keys(&mut app, step);
        let buffer = render(&app, 80, 24).backend().buffer().clone();
        for cell in buffer.content() {
            assert_eq!(
                (cell.fg, cell.bg),
                (Color::Reset, Color::Reset),
                "{:?}",
                app.state()
            );
        }
    }
}

// --- The title screen offers every entry ------------------------

#[test]
fn the_title_lists_new_game_seed_daily_options_and_quit_in_that_order_without_a_save() {
    let folder = ScratchFolder::new("title-empty");
    let app = launch_with_arguments(&folder, &[]).unwrap();

    for (width, height) in SIZES {
        let screen = rows(&app, width, height);
        let at: Vec<Option<usize>> = ["New game", "Seed", "Daily", "Options", "Quit"]
            .iter()
            .map(|label| row_of(&screen, label))
            .collect();
        assert!(at.iter().all(Option::is_some), "{width}x{height}: {at:?}");
        assert!(at.windows(2).all(|pair| pair[0] < pair[1]), "{at:?}");
        assert_eq!(row_of(&screen, "Continue"), None, "no save, no Continue");
    }
    assert_eq!(
        chosen(&app),
        TitleItem::NewGame,
        "the first entry is chosen"
    );
    assert!(!app.has_saved_game());
}

#[test]
fn with_a_save_continue_comes_first_is_chosen_and_says_where_the_game_stands() {
    let folder = ScratchFolder::new("title-save");
    save_game(&folder, 42, 4);
    let app = launch_with_arguments(&folder, &[]).unwrap();

    for (width, height) in SIZES {
        let screen = rows(&app, width, height);
        let at: Vec<Option<usize>> = ["Continue", "New game", "Seed", "Daily", "Options", "Quit"]
            .iter()
            .map(|label| row_of(&screen, label))
            .collect();
        assert!(at.iter().all(Option::is_some), "{width}x{height}: {at:?}");
        assert!(at.windows(2).all(|pair| pair[0] < pair[1]), "{at:?}");
        let continue_row = &screen[at[0].unwrap()];
        assert!(continue_row.contains("depth 1"), "{continue_row}");
        assert!(
            continue_row.contains("turn 5"),
            "4 waits: the turn shown is the fifth: {continue_row}"
        );
    }
    assert_eq!(chosen(&app), TitleItem::Continue);
}

#[test]
fn the_menu_wraps_around_and_every_entry_does_what_it_says() {
    let folder = ScratchFolder::new("title-keys");
    let mut app = launch_with_arguments(&folder, &[]).unwrap();

    app.handle_key(press(KeyCode::Up));
    assert_eq!(
        chosen(&app),
        TitleItem::Quit,
        "Up from the first wraps to Quit"
    );
    app.handle_key(press(KeyCode::Down));
    assert_eq!(chosen(&app), TitleItem::NewGame, "Down from Quit wraps");
    app.handle_key(press(KeyCode::End));
    assert_eq!(chosen(&app), TitleItem::Quit);
    app.handle_key(press(KeyCode::Home));
    assert_eq!(chosen(&app), TitleItem::NewGame);
    app.handle_key(press(KeyCode::Char('j')));
    assert_eq!(chosen(&app), TitleItem::Seed);
    app.handle_key(press(KeyCode::Char('k')));
    assert_eq!(chosen(&app), TitleItem::NewGame);
    app.handle_key(press(KeyCode::Esc));
    assert_eq!(chosen(&app), TitleItem::NewGame, "Escape does nothing here");

    go_to(&mut app, TitleItem::Seed);
    app.handle_key(press(KeyCode::Enter));
    assert!(matches!(app.state(), State::SeedEntry(_)));
    app.handle_key(press(KeyCode::Esc));
    assert_eq!(
        chosen(&app),
        TitleItem::Seed,
        "Escape goes back to the menu"
    );

    go_to(&mut app, TitleItem::Quit);
    app.handle_key(press(KeyCode::Enter));
    assert_eq!(app.state(), State::Quit);

    let mut app = launch_with_arguments(&folder, &[]).unwrap();
    app.handle_key(press(KeyCode::Char('q')));
    assert_eq!(app.state(), State::Quit, "q quits from the title");
    let mut app = launch_with_arguments(&folder, &[]).unwrap();
    app.handle_key(ctrl_c());
    assert_eq!(app.state(), State::Quit, "Ctrl+C quits from the title");
    assert_eq!(
        files(&folder),
        Vec::<String>::new(),
        "quitting the title writes nothing"
    );

    let mut app = launch_with_arguments(&folder, &[]).unwrap();
    app.handle_key(press(KeyCode::Enter));
    assert_eq!(app.state(), State::Playing, "New game starts a game");
}

// --- The seed entry ---------------------------------------------------------

/// Types `entry` on the seed entry of the title screen and presses Enter.
fn submit_seed(app: &mut App, entry: &str) {
    go_to(app, TitleItem::Seed);
    app.handle_key(press(KeyCode::Enter));
    type_text(app, entry);
    app.handle_key(press(KeyCode::Enter));
}

#[test]
fn a_valid_seed_starts_the_game_of_that_seed_whatever_form_it_is_typed_in() {
    let accepted: [(&str, u64); 8] = [
        ("42", 42),
        ("0", 0),
        ("7F3A-91C2-0B5E-D4A8", 0x7F3A_91C2_0B5E_D4A8),
        ("7f3a-91c2-0b5e-d4a8", 0x7F3A_91C2_0B5E_D4A8),
        ("FFFF-FFFF-FFFF-FFFF", u64::MAX),
        ("18446744073709551615", u64::MAX),
        ("000042", 42),
        ("42 ", 42),
    ];
    for (typed, value) in accepted {
        let folder = ScratchFolder::new("seed-valid");
        let mut app = launch_with_arguments(&folder, &[]).unwrap();
        submit_seed(&mut app, typed);
        assert_eq!(app.state(), State::Playing, "{typed:?}");
        assert_eq!(app.game().seed(), Seed::from(value), "{typed:?}");
        assert!(
            rows(&app, 80, 24)[0].contains(&Seed::from(value).to_string()),
            "{typed:?}: the status line shows the seed"
        );
    }
    // The seed typed is the seed of --seed.
    let typed_folder = ScratchFolder::new("seed-typed");
    let mut typed = launch_with_arguments(&typed_folder, &[]).unwrap();
    submit_seed(&mut typed, "7F3A-91C2-0B5E-D4A8");
    let flag_folder = ScratchFolder::new("seed-flag");
    let flagged = launch_with_arguments(&flag_folder, &["--seed", "7F3A-91C2-0B5E-D4A8"]).unwrap();
    assert_eq!(rows(&typed, 80, 24), rows(&flagged, 80, 24));
    assert_eq!(rows(&typed, 120, 40), rows(&flagged, 120, 40));
}

#[test]
fn an_invalid_seed_is_refused_with_a_reason_and_the_entry_stays_open() {
    let long = "9".repeat(300);
    let refused: [(&str, &str); 11] = [
        ("abc", "\"abc\" is not a seed"),
        ("-1", "\"-1\" is not a seed"),
        ("18446744073709551616", "is not a seed"),
        ("7F3A-91C2-0B5E-D4A", "is not a seed"),
        ("4 2", "\"4 2\" is not a seed"),
        ("!@#$%^&*()", "is not a seed"),
        ("0x2A", "is not a seed"),
        ("12e3", "is not a seed"),
        ("", "Type a seed first"),
        ("   ", "Type a seed first"),
        (&long, "is not a seed"),
    ];
    for (typed, reason) in refused {
        let folder = ScratchFolder::new("seed-invalid");
        let mut app = launch_with_arguments(&folder, &[]).unwrap();
        submit_seed(&mut app, typed);
        assert!(
            matches!(app.state(), State::SeedEntry(_)),
            "{typed:?} must not start a game"
        );
        for (width, height) in SIZES {
            let screen = rows(&app, width, height);
            assert!(
                screen.iter().any(|row| row.contains(reason)),
                "{typed:?}: a reason ({reason}):\n{}",
                screen.join("\n")
            );
            assert!(
                screen
                    .iter()
                    .all(|row| row.chars().count() <= usize::from(width))
            );
        }
        assert_eq!(
            files(&folder),
            Vec::<String>::new(),
            "{typed:?}: nothing written"
        );
        // Typing goes on after a refusal, and a valid seed then starts.
        app.handle_key(press(KeyCode::Esc));
        submit_seed(&mut app, "5");
        assert_eq!(app.state(), State::Playing, "{typed:?}: the next try works");
    }
}

#[test]
fn the_seed_field_edits_like_a_text_field_and_a_q_is_a_letter() {
    let folder = ScratchFolder::new("seed-edit");
    let mut app = launch_with_arguments(&folder, &[]).unwrap();
    go_to(&mut app, TitleItem::Seed);
    app.handle_key(press(KeyCode::Enter));
    type_text(&mut app, "q4x");
    assert!(
        matches!(app.state(), State::SeedEntry(_)),
        "q does not quit here"
    );
    assert!(text(&app).contains("Seed  q4x_"), "{}", text(&app));
    app.handle_key(press(KeyCode::Backspace));
    assert!(text(&app).contains("Seed  q4_"));
    for _ in 0..6 {
        app.handle_key(press(KeyCode::Backspace));
    }
    assert!(
        text(&app).contains("Seed  _"),
        "more Backspaces than letters are harmless"
    );
    type_text(&mut app, &"1".repeat(100));
    let field = rows(&app, 80, 24)
        .into_iter()
        .find(|row| row.contains("Seed  1"))
        .unwrap();
    assert!(
        field.trim().chars().count() <= 40,
        "the field is bounded: {field}"
    );
    app.handle_key(press(KeyCode::Esc));
    assert_eq!(chosen(&app), TitleItem::Seed);
    app.handle_key(press(KeyCode::Enter));
    assert!(text(&app).contains("Seed  _"), "a new entry starts empty");
    app.handle_key(ctrl_c());
    assert_eq!(app.state(), State::Quit);
}

// --- The confirmation before a saved game is abandoned ------------------------

#[test]
fn abandoning_a_saved_game_asks_first_and_only_the_second_answer_deletes_it() {
    let folder = ScratchFolder::new("confirm");
    save_game(&folder, 42, 4);
    let saved = fs::read(folder.save_file().path()).unwrap();
    let mut app = launch_with_arguments(&folder, &[]).unwrap();

    go_to(&mut app, TitleItem::NewGame);
    app.handle_key(press(KeyCode::Enter));
    assert!(
        matches!(app.state(), State::Confirm(_)),
        "{:?}",
        app.state()
    );
    let screen = rows(&app, 80, 24);
    for part in [
        "Start a new game?",
        "abandoned",
        "deleted for good",
        "Keep the game in progress",
        "Abandon it and start anew",
        "depth 1",
    ] {
        assert!(
            screen.iter().any(|row| row.contains(part)),
            "{part}:\n{}",
            screen.join("\n")
        );
    }
    let keep = row_of(&screen, "Keep the game in progress").unwrap();
    let abandon = row_of(&screen, "Abandon it and start anew").unwrap();
    assert!(keep < abandon, "Keep comes first");
    assert!(
        screen[keep].contains('›') || screen[keep].contains('>'),
        "and is chosen: {}",
        screen[keep]
    );
    assert_eq!(
        fs::read(folder.save_file().path()).unwrap(),
        saved,
        "untouched while asking"
    );

    app.handle_key(press(KeyCode::Char('q')));
    assert!(
        matches!(app.state(), State::Confirm(_)),
        "q does nothing here"
    );
    app.handle_key(press(KeyCode::Down));
    let on_abandon = rows(&app, 80, 24);
    assert!(
        on_abandon[row_of(&on_abandon, "Abandon it and start anew").unwrap()].contains(['›', '>'])
    );
    app.handle_key(press(KeyCode::Up));
    app.handle_key(press(KeyCode::Esc));
    assert_eq!(
        chosen(&app),
        TitleItem::NewGame,
        "Escape goes back to the menu"
    );
    assert_eq!(fs::read(folder.save_file().path()).unwrap(), saved);

    app.handle_key(press(KeyCode::Enter));
    app.handle_key(press(KeyCode::Enter));
    assert!(
        matches!(app.state(), State::Title(_)),
        "Enter on Keep goes back"
    );
    assert!(app.has_saved_game());
    assert_eq!(
        fs::read(folder.save_file().path()).unwrap(),
        saved,
        "Keep keeps"
    );

    app.handle_key(press(KeyCode::Enter));
    app.handle_key(press(KeyCode::Down));
    app.handle_key(press(KeyCode::Enter));
    assert_eq!(app.state(), State::Playing, "Abandon starts the new game");
    assert!(
        !folder.save_file().path().exists(),
        "and deletes the old save"
    );
    assert!(!app.has_saved_game());
    assert_eq!(app.game().player_turns(), 0, "a brand new game");
    // A second Enter sent at once changes nothing: still the one new game.
    app.handle_key(press(KeyCode::Enter));
    assert_eq!(app.state(), State::Playing);
}

#[test]
fn a_seed_and_the_daily_dungeon_ask_before_abandoning_a_save_too_and_ctrl_c_keeps_it() {
    let folder = ScratchFolder::new("confirm-others");
    save_game(&folder, 42, 2);
    let saved = fs::read(folder.save_file().path()).unwrap();
    let mut app = launch_with_arguments(&folder, &[]).unwrap();

    go_to(&mut app, TitleItem::Daily);
    app.handle_key(press(KeyCode::Enter));
    assert!(matches!(app.state(), State::Confirm(_)));
    app.handle_key(press(KeyCode::Enter));
    assert_eq!(chosen(&app), TitleItem::Daily, "Keep returns on Daily");

    submit_seed(&mut app, "5");
    assert!(
        matches!(app.state(), State::Confirm(_)),
        "a valid seed asks first"
    );
    app.handle_key(press(KeyCode::Esc));
    assert_eq!(chosen(&app), TitleItem::Seed, "Escape returns on Seed");

    submit_seed(&mut app, "5");
    app.handle_key(ctrl_c());
    assert_eq!(app.state(), State::Quit);
    assert_eq!(
        fs::read(folder.save_file().path()).unwrap(),
        saved,
        "the save survived"
    );

    let mut next = launch_with_arguments(&folder, &[]).unwrap();
    next.handle_key(press(KeyCode::Enter));
    assert_eq!(next.state(), State::Playing, "Continue resumes it");
    assert_eq!(next.game().player_turns(), 2);
}
