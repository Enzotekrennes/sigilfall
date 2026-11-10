//! The keys follow the keyboard layout, QWERTY or AZERTY: guessed from the
//! system until the player chooses one in the options, which keeps it, or
//! given for a session by `--keyboard`. On AZERTY the player moves with ZQSD
//! and traces with the top row without Shift; every screen's key help names
//! the keys of the layout, in each language, within 80 × 24.

use std::fs;

use insta::assert_snapshot;
use ratatui::crossterm::event::KeyCode;
use sigilfall_core::game::{Game, PLAYER_START};
use sigilfall_core::geometry::Direction;

use super::player_view::{enter_options, go_to_row, launch_with_arguments, press, rows};
use super::{SEED, shipped_content};
use crate::app::{App, State};
use crate::cli::Cli;
use crate::clock::ManualClock;
use crate::keyboard::Keyboard;
use crate::language::Language;
use crate::persist::ScratchFolder;
use crate::theme::{ColorMode, ColorSupport, Defaults, GlyphSet, Theme};
use crate::ui::options::OptionRow;

/// The four combinations of a language and a layout, with the name of
/// their snapshots.
const COMBINATIONS: [(Language, Keyboard, &str); 4] = [
    (Language::English, Keyboard::Qwerty, "english_qwerty"),
    (Language::English, Keyboard::Azerty, "english_azerty"),
    (Language::French, Keyboard::Qwerty, "french_qwerty"),
    (Language::French, Keyboard::Azerty, "french_azerty"),
];

/// The default mode of Windows Terminal, in `language`, on `keyboard`.
fn theme(language: Language, keyboard: Keyboard) -> Theme {
    Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode)
        .in_language(language)
        .on_keyboard(keyboard)
}

/// The text of `app` drawn on an 80 × 24 screen, one line per row.
fn screen(app: &App) -> String {
    rows(app, 80, 24).join("\n")
}

/// The key help of `app` on an 80 × 24 screen: its last row, without the
/// blank columns after it.
fn help(app: &App) -> String {
    rows(app, 80, 24).pop().unwrap().trim_end().to_owned()
}

/// A new game in the fixed room of [`SEED`], in `language`, on `keyboard`.
fn fixed_room(language: Language, keyboard: Keyboard) -> App {
    App::with_game(
        theme(language, keyboard),
        Game::in_fixed_room(SEED),
        shipped_content(),
    )
}

/// Launches the application as `main` does on `folder`, on a system whose
/// language and usual layout are `language` and `keyboard`, with
/// `arguments`.
fn launch_on(folder: &ScratchFolder, language: Language, keyboard: Keyboard, cli: &Cli) -> App {
    App::launch_with_clock(
        ColorSupport::TrueColor,
        Defaults { language, keyboard },
        cli,
        &shipped_content(),
        folder.save_file(),
        Box::new(ManualClock::new()),
    )
    .expect("the launch succeeds")
}

/// The command line without arguments.
const fn no_arguments() -> Cli {
    Cli {
        seed: None,
        daily: false,
        ascii: false,
        lang: None,
        keyboard: None,
    }
}

// --- Playing on AZERTY ------------------------------------------------------

#[test]
fn on_azerty_zqsd_moves_and_the_top_row_without_shift_traces() {
    let mut app = fixed_room(Language::French, Keyboard::Azerty);
    let start = app.game().player_actor().position();
    assert_eq!(start, PLAYER_START);

    for (typed, direction) in [
        ('d', Direction::East),
        ('q', Direction::West),
        ('s', Direction::South),
        ('z', Direction::North),
    ] {
        let before = app.game().player_actor().position();
        app.handle_key(press(KeyCode::Char(typed)));
        assert_eq!(
            app.game().player_actor().position(),
            before.step(direction),
            "{typed}"
        );
    }

    // Ember, 7 5 3, typed `è ( "` without Shift.
    app.handle_key(press(KeyCode::Char('t')));
    for typed in ['è', '(', '"'] {
        app.handle_key(press(KeyCode::Char(typed)));
    }
    let text = screen(&app);
    assert!(text.contains("Tracé    7·5·3"), "{text}");
    assert!(matches!(app.state(), State::Casting(_)));

    // `p` quits on AZERTY; `q` moved west above and never quits.
    app.handle_key(press(KeyCode::Esc));
    app.handle_key(press(KeyCode::Char('p')));
    assert_eq!(app.state(), State::Quit);
}

#[test]
fn on_qwerty_wasd_moves_q_quits_and_the_top_row_symbols_do_nothing() {
    let mut app = fixed_room(Language::English, Keyboard::Qwerty);
    for (typed, direction) in [
        ('d', Direction::East),
        ('a', Direction::West),
        ('s', Direction::South),
        ('w', Direction::North),
    ] {
        let before = app.game().player_actor().position();
        app.handle_key(press(KeyCode::Char(typed)));
        assert_eq!(
            app.game().player_actor().position(),
            before.step(direction),
            "{typed}"
        );
    }
    let turns = app.game().player_turns();
    for typed in ['&', 'é', '(', 'è', 'z', 'p'] {
        app.handle_key(press(KeyCode::Char(typed)));
    }
    assert_eq!(app.game().player_turns(), turns, "no time passed");
    app.handle_key(press(KeyCode::Char('q')));
    assert_eq!(app.state(), State::Quit);
}

// --- The option -------------------------------------------------------------

#[test]
fn the_system_gives_the_layout_until_the_player_chooses_one_which_is_kept() {
    let folder = ScratchFolder::new("keyboard-option");
    let app = launch_on(&folder, Language::French, Keyboard::Azerty, &no_arguments());
    assert_eq!(app.options().keyboard, Keyboard::Azerty);
    assert!(help(&app).contains(" p quitter "), "{}", help(&app));
    assert!(
        !folder.options_file().path().exists(),
        "nothing is written until an option changes"
    );

    let mut app = launch_on(
        &folder,
        Language::English,
        Keyboard::Azerty,
        &no_arguments(),
    );
    enter_options(&mut app);
    go_to_row(&mut app, OptionRow::Keyboard);
    // On AZERTY `d` changes the value, as Right does.
    app.handle_key(press(KeyCode::Char('d')));
    assert_eq!(app.options().keyboard, Keyboard::Qwerty);
    let text = screen(&app);
    assert!(text.contains("Keyboard     QWERTY"), "{text}");
    let written = fs::read_to_string(folder.options_file().path()).unwrap();
    assert!(written.contains("keyboard: Qwerty"), "{written}");

    // Kept whatever the system says afterwards.
    let again = launch_on(
        &folder,
        Language::English,
        Keyboard::Azerty,
        &no_arguments(),
    );
    assert_eq!(again.options().keyboard, Keyboard::Qwerty);
    assert!(help(&again).contains(" q quit "), "{}", help(&again));
}

#[test]
fn keyboard_on_the_command_line_wins_for_the_session_without_changing_the_option() {
    let folder = ScratchFolder::new("keyboard-forced");
    let mut app = launch_with_arguments(&folder, &["--keyboard", "azerty", "--seed", "42"])
        .expect("the launch succeeds");
    assert!(help(&app).starts_with(" zqsd move"), "{}", help(&app));
    assert!(help(&app).ends_with("p save & quit"), "{}", help(&app));
    app.handle_key(press(KeyCode::Char('p')));
    app.save_on_exit().unwrap();

    let mut title = launch_with_arguments(&folder, &["--keyboard", "azerty"]).unwrap();
    enter_options(&mut title);
    let text = screen(&title);
    assert!(
        text.contains("--keyboard is on: AZERTY for this session."),
        "{text}"
    );
    assert!(text.contains("Keyboard     QWERTY"), "{text}");
    assert_eq!(title.options().keyboard, Keyboard::Qwerty);
}

// --- The key help of each language and layout -----------------------------------

#[test]
fn every_key_help_names_the_keys_of_the_layout_in_each_language_within_80_columns() {
    for (language, keyboard, name) in COMBINATIONS {
        let (movement, nodes, quit) = match keyboard {
            Keyboard::Qwerty => ("wasd", "1-9", 'q'),
            Keyboard::Azerty => ("zqsd", "&…ç", 'p'),
        };
        let mut app = fixed_room(language, keyboard);
        let mut helps = vec![("game", help(&app))];
        app.handle_key(press(KeyCode::Char('t')));
        helps.push(("trace", help(&app)));
        app.handle_key(press(KeyCode::Esc));
        app.handle_key(press(KeyCode::Char('i')));
        helps.push(("inventory", help(&app)));

        let game = &helps[0].1;
        assert!(game.starts_with(&format!(" {movement} ")), "{name}: {game}");
        assert!(game.contains(&format!(" {quit} ")), "{name}: {game}");
        let trace = &helps[1].1;
        assert!(trace.starts_with(&format!(" {nodes} ")), "{name}: {trace}");
        for (screen, help) in &helps {
            assert!(help.chars().count() <= 80, "{name} {screen}: {help:?}");
        }
    }
}

#[test]
fn the_game_screen_in_each_language_and_layout() {
    for (language, keyboard, name) in COMBINATIONS {
        let content = shipped_content();
        let mut app = App::with_game(
            theme(language, keyboard),
            Game::new(SEED, &content),
            content,
        );
        app.handle_key(press(KeyCode::Char('.')));
        assert_snapshot!(format!("game_screen_{name}"), screen(&app));
    }
}

#[test]
fn the_trace_grid_in_each_language_and_layout() {
    for (language, keyboard, name) in COMBINATIONS {
        let content = shipped_content();
        let mut app = App::with_game(
            theme(language, keyboard),
            Game::new(SEED, &content),
            content,
        );
        // Ember's first two nodes, 7 and 5, with the keys of the layout.
        let keys = match keyboard {
            Keyboard::Qwerty => ['t', '7', '5'],
            Keyboard::Azerty => ['t', 'è', '('],
        };
        for typed in keys {
            app.handle_key(press(KeyCode::Char(typed)));
        }
        assert_snapshot!(format!("trace_grid_{name}"), screen(&app));
    }
}

#[test]
fn the_options_screen_in_each_language_and_layout() {
    for (language, keyboard, name) in COMBINATIONS {
        let folder = ScratchFolder::new("keyboard-options-screen");
        let mut app = launch_on(&folder, language, keyboard, &no_arguments());
        enter_options(&mut app);
        go_to_row(&mut app, OptionRow::Keyboard);
        assert_snapshot!(format!("options_screen_{name}"), screen(&app));
    }
}
