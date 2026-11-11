//! The first minutes of a run read right in both languages and on both
//! layouts: the log opens with the sigil the player knows and the key that
//! traces it; each node of the trace grid shows its digit, and on AZERTY the
//! key that links it; the key help names a key for the stairs typed without
//! Shift; a movement letter typed in upper case says once that Caps Lock may
//! be on; item names are written in lower case inside sentences; and the
//! refusal of the longest seed reads whole at 80 columns.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use sigilfall_core::action::Action;
use sigilfall_core::game::{Game, PLAYER_START};

use super::player_view::{go_to, press, rows, type_text};
use super::{SEED, shipped_content};
use crate::app::{App, State};
use crate::cli::Cli;
use crate::clock::ManualClock;
use crate::input::{self, Command};
use crate::keyboard::Keyboard;
use crate::language::Language;
use crate::message_log::{Message, Tone};
use crate::persist::ScratchFolder;
use crate::theme::{ColorMode, ColorSupport, Defaults, GlyphSet, Theme};
use crate::ui::title::{SEED_CAPACITY, TitleItem};

/// The four combinations of a language and a layout.
const COMBINATIONS: [(Language, Keyboard); 4] = [
    (Language::English, Keyboard::Qwerty),
    (Language::English, Keyboard::Azerty),
    (Language::French, Keyboard::Qwerty),
    (Language::French, Keyboard::Azerty),
];

/// No-break space, which French writes before `:` and `?`.
const NBSP: char = '\u{a0}';

/// Launches the application as `main` does on `folder`, on a system in
/// `language` with a `keyboard`, with `--seed` set to `seed` when given.
fn launch(folder: &ScratchFolder, language: Language, keyboard: Keyboard, seed: bool) -> App {
    let cli = Cli {
        seed: seed.then_some(SEED),
        daily: false,
        ascii: false,
        lang: None,
        keyboard: None,
    };
    App::launch_with_clock(
        ColorSupport::TrueColor,
        Defaults { language, keyboard },
        &cli,
        &shipped_content(),
        folder.save_file(),
        Box::new(ManualClock::new()),
    )
    .expect("the launch succeeds")
}

/// The default theme of Windows Terminal with `glyphs`, in `language`, on
/// `keyboard`.
fn theme(language: Language, keyboard: Keyboard, glyphs: GlyphSet) -> Theme {
    Theme::new(ColorMode::TrueColorDark, glyphs)
        .in_language(language)
        .on_keyboard(keyboard)
}

/// A game in the fixed room of [`SEED`], whose walls and start never change,
/// in `language`, on `keyboard`, drawn with `glyphs`.
fn fixed_room(language: Language, keyboard: Keyboard, glyphs: GlyphSet) -> App {
    App::with_game(
        theme(language, keyboard, glyphs),
        Game::in_fixed_room(SEED),
        shipped_content(),
    )
}

/// A new game of [`SEED`], knowing Ember as every run does, in `language`,
/// on `keyboard`, drawn with `glyphs`.
fn new_game(language: Language, keyboard: Keyboard, glyphs: GlyphSet) -> App {
    let content = shipped_content();
    App::with_game(
        theme(language, keyboard, glyphs),
        Game::new(SEED, &content),
        content,
    )
}

/// The texts of the log of `app`, oldest first.
fn log_texts(app: &App) -> Vec<String> {
    let mut texts: Vec<String> = app
        .log()
        .newest_first()
        .map(|line| line.text.clone())
        .collect();
    texts.reverse();
    texts
}

/// A press of the character `typed`, with Shift when `shift` is set, as
/// crossterm reports a capital typed with Shift rather than Caps Lock.
fn char_press(typed: char, shift: bool) -> KeyEvent {
    let modifiers = if shift {
        KeyModifiers::SHIFT
    } else {
        KeyModifiers::NONE
    };
    KeyEvent::new(KeyCode::Char(typed), modifiers)
}

// --- The opening line -------------------------------------------------------

#[test]
fn a_new_run_opens_with_the_sigil_known_and_the_key_that_traces_it() {
    for (language, wanted) in [
        (
            Language::English,
            "You enter the dungeon knowing the sigil Ember: press t to trace it.".to_owned(),
        ),
        (
            Language::French,
            format!("Vous entrez dans le donjon avec le sigille Braise{NBSP}: t pour le tracer."),
        ),
    ] {
        for keyboard in Keyboard::ALL {
            let folder = ScratchFolder::new("first-steps-opening");
            let app = launch(&folder, language, keyboard, true);
            assert_eq!(app.state(), State::Playing);
            assert_eq!(
                app.log().newest_first().cloned().collect::<Vec<Message>>(),
                [Message::new(Tone::Sigil, wanted.clone())],
                "{language:?} {keyboard:?}"
            );
            // One row of the log, under the map.
            let screen = rows(&app, 80, 24);
            assert!(screen[18].ends_with(&wanted), "{}", screen.join("\n"));
            assert_eq!(screen[19], "", "{}", screen.join("\n"));
            // `t` opens the grid on both layouts, as the line says.
            let mut app = app;
            app.handle_key(press(KeyCode::Char('t')));
            assert!(matches!(app.state(), State::Casting(_)));
        }
    }
}

// --- The labels of the trace grid -------------------------------------------

#[test]
fn every_node_of_the_grid_shows_its_digit_and_on_azerty_the_key_that_links_it() {
    let cases = [(GlyphSet::Unicode, '○'), (GlyphSet::Ascii, 'o')];
    for (language, keyboard) in COMBINATIONS {
        for (glyphs, free) in cases {
            let mut app = new_game(language, keyboard, glyphs);
            app.handle_key(press(KeyCode::Char('t')));
            let screen = rows(&app, 80, 24);
            let grid_rows = [("789", "è_ç"), ("456", "'(-"), ("123", "&é\"")];
            for (digits, keys) in grid_rows {
                let nodes: Vec<String> = digits.chars().map(|d| format!("{free}{d}")).collect();
                let nodes = nodes.join("    ");
                let at = screen
                    .iter()
                    .position(|row| row.starts_with(&format!("{}  {nodes}", frame(glyphs))))
                    .unwrap_or_else(|| panic!("{nodes} missing:\n{}", screen.join("\n")));
                let under: String = screen[at + 1].chars().skip(1).take(16).collect();
                let wanted = match keyboard {
                    Keyboard::Qwerty => String::new(),
                    Keyboard::Azerty => {
                        let keys: Vec<String> = keys.chars().map(String::from).collect();
                        format!("   {}", keys.join("     "))
                    }
                };
                assert_eq!(
                    under.trim_end(),
                    wanted,
                    "{language:?} {keyboard:?} {glyphs:?}:\n{}",
                    screen.join("\n")
                );
            }
        }
    }
}

/// The left side of the overlay's frame in `glyphs`.
const fn frame(glyphs: GlyphSet) -> char {
    match glyphs {
        GlyphSet::Unicode => '│',
        GlyphSet::Ascii => '|',
    }
}

#[test]
fn the_keys_written_under_the_nodes_are_the_keys_that_link_them() {
    // Ember, 7 5 3, typed as the grid writes it on AZERTY: è ( ".
    let mut app = new_game(Language::French, Keyboard::Azerty, GlyphSet::Unicode);
    app.handle_key(press(KeyCode::Char('t')));
    for typed in ['è', '(', '"'] {
        app.handle_key(press(KeyCode::Char(typed)));
    }
    let screen = rows(&app, 80, 24).join("\n");
    assert!(screen.contains("Tracé    7·5·3"), "{screen}");
    // The grimoire writes the pattern in digits, as the grid's labels.
    assert!(screen.contains("Braise  7·5·3"), "{screen}");
}

// --- The key of the stairs ----------------------------------------------------

#[test]
fn the_key_help_names_a_key_for_the_stairs_typed_without_shift() {
    for (language, keyboard) in COMBINATIONS {
        let app = fixed_room(language, keyboard, GlyphSet::Unicode);
        let help = rows(&app, 80, 24).pop().unwrap();
        let (key, label) = match (keyboard, language) {
            (Keyboard::Qwerty, Language::English) => ('>', "descend"),
            (Keyboard::Qwerty, Language::French) => ('>', "descente"),
            (Keyboard::Azerty, Language::English) => ('<', "descend"),
            (Keyboard::Azerty, Language::French) => ('<', "descente"),
        };
        assert!(
            help.contains(&format!(" {key} {label} ")),
            "{language:?} {keyboard:?}: {help}"
        );
        // The key named goes down the stairs on that layout.
        assert_eq!(
            input::command_for(keyboard, press(KeyCode::Char(key))),
            Some(Command::Act(Action::Descend)),
            "{keyboard:?}"
        );
    }
    // On QWERTY `<` stays free: only AZERTY has it without Shift.
    assert_eq!(
        input::command_for(Keyboard::Qwerty, press(KeyCode::Char('<'))),
        None
    );
}

// --- Caps Lock ----------------------------------------------------------------

#[test]
fn a_capital_movement_letter_does_nothing_and_says_once_that_caps_lock_may_be_on() {
    let mut app = fixed_room(Language::French, Keyboard::Azerty, GlyphSet::Unicode);
    let (start, turns) = (
        app.game().player_actor().position(),
        app.game().player_turns(),
    );
    let before = log_texts(&app).len();

    // Caps Lock on: Z, then Q and D, as a player tries the other ways.
    app.handle_key(char_press('Z', false));
    app.handle_key(char_press('Q', false));
    app.handle_key(char_press('D', true));
    let log = log_texts(&app);
    assert_eq!(
        log[before..],
        [format!("Z ne fait rien{NBSP}: Verr Maj activé{NBSP}?")],
        "once for the run of ignored capitals"
    );
    assert_eq!(app.game().player_actor().position(), start, "nothing moved");
    assert_eq!(app.game().player_turns(), turns, "no time passed");

    // A key that does something starts a new run: the next capital says so
    // again, with its own letter.
    app.handle_key(press(KeyCode::Char('d')));
    assert_eq!(
        app.game().player_actor().position(),
        start.step(sigilfall_core::geometry::Direction::East)
    );
    app.handle_key(char_press('S', false));
    app.handle_key(char_press('S', false));
    let log = log_texts(&app);
    let hints: Vec<&String> = log
        .iter()
        .filter(|line| line.contains("Verr Maj"))
        .collect();
    assert_eq!(hints.len(), 2, "{log:?}");
    assert_eq!(
        hints[1],
        &format!("S ne fait rien{NBSP}: Verr Maj activé{NBSP}?")
    );
}

#[test]
fn caps_lock_is_hinted_for_the_movement_letters_of_the_layout_only() {
    for keyboard in Keyboard::ALL {
        let letters: &[char] = match keyboard {
            Keyboard::Qwerty => &['W', 'A', 'S', 'D', 'H', 'J', 'K', 'L', 'Y', 'U', 'B', 'N'],
            Keyboard::Azerty => &['Z', 'Q', 'S', 'D', 'H', 'J', 'K', 'L', 'Y', 'U', 'B', 'N'],
        };
        for &letter in letters {
            let mut app = fixed_room(Language::English, keyboard, GlyphSet::Unicode);
            app.handle_key(char_press(letter, false));
            assert_eq!(
                log_texts(&app).last().map(String::as_str),
                Some(format!("{letter} does nothing: Caps Lock on?").as_str()),
                "{keyboard:?} {letter}"
            );
        }
        // Capitals that are not movement letters of the layout say nothing:
        // `E` goes down the stairs (refused off them), `T` and `G` are not
        // movement letters, and `Z` or `W` move on the other layout only.
        let other = match keyboard {
            Keyboard::Qwerty => 'Z',
            Keyboard::Azerty => 'W',
        };
        for letter in ['E', 'T', 'G', other] {
            let mut app = fixed_room(Language::English, keyboard, GlyphSet::Unicode);
            app.handle_key(char_press(letter, false));
            assert!(
                !log_texts(&app)
                    .iter()
                    .any(|line| line.contains("Caps Lock")),
                "{keyboard:?} {letter}: {:?}",
                log_texts(&app)
            );
        }
    }
}

// --- Item names in sentences --------------------------------------------------

#[test]
fn every_item_name_of_the_content_is_written_in_lower_case() {
    for item in shipped_content().items() {
        for name in [item.name(), item.french_name().noun()] {
            let first = name.chars().next().expect("a name");
            assert!(first.is_lowercase(), "{name}");
        }
    }
}

#[test]
fn item_names_are_lower_case_in_sentences_and_capitalised_at_the_start_of_a_line_of_the_pack() {
    let content = shipped_content();
    let cases = [
        (
            Language::French,
            "Vous ramassez l'encre d'aplomb.",
            "♦ Encre d'aplomb",
        ),
        (
            Language::English,
            "You pick up the steadying ink.",
            "♦ Steadying ink",
        ),
    ];
    for (language, picked, listed) in cases {
        let mut game = Game::in_fixed_room(SEED);
        game.place_item(PLAYER_START, content.item("steadying_ink").unwrap());
        let mut app = App::with_game(
            theme(language, Keyboard::Qwerty, GlyphSet::Unicode),
            game,
            content.clone(),
        );
        // In view, the side panel names it in lower case, as the creatures.
        let panel = rows(&app, 80, 24).join("\n");
        let in_view = listed.to_lowercase();
        assert!(panel.contains(&in_view), "{panel}");

        app.handle_key(press(KeyCode::Char('g')));
        assert_eq!(log_texts(&app).last().map(String::as_str), Some(picked));
        app.handle_key(press(KeyCode::Char('i')));
        let pack = rows(&app, 80, 24).join("\n");
        assert!(pack.contains(listed), "{pack}");
    }
}

// --- The refusal of a seed ---------------------------------------------------

#[test]
fn the_refusal_of_a_full_seed_field_reads_whole_at_80_columns() {
    let typed = "9".repeat(SEED_CAPACITY);
    for (language, wanted) in [
        (
            Language::English,
            format!("! \"{typed}\" is not a seed: use one of the forms above."),
        ),
        (
            Language::French,
            format!("! «{NBSP}{typed}{NBSP}» n'est pas une graine{NBSP}: voir ci-dessus."),
        ),
    ] {
        let folder = ScratchFolder::new("first-steps-seed");
        let mut app = launch(&folder, language, Keyboard::Qwerty, false);
        go_to(&mut app, TitleItem::Seed);
        app.handle_key(press(KeyCode::Enter));
        type_text(&mut app, &typed);
        app.handle_key(press(KeyCode::Enter));
        let screen = rows(&app, 80, 24);
        assert!(
            screen.iter().any(|row| row.trim() == wanted),
            "{language:?}:\n{}",
            screen.join("\n")
        );
        assert!(wanted.chars().count() <= 80);
    }
}
