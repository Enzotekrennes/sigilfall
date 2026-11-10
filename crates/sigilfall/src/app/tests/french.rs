//! The game in French: the language comes from the system until the player
//! chooses one, the choice is kept, every screen is drawn in French within
//! 80 × 24, and no English text is left on any of them.

use std::fs;

use insta::assert_snapshot;
use ratatui::crossterm::event::KeyCode;
use sigilfall_core::action::Action;
use sigilfall_core::game::{Game, PLAYER_START};
use sigilfall_core::geometry::Direction;

use super::player_view::{
    enter_options, go_to, go_to_row, launch_with_arguments, press, rows, save_game,
};
use super::victory_screen::{harmless, won_game};
use super::{SEED, shipped_content};
use crate::app::{App, State};
use crate::cli::Cli;
use crate::clock::ManualClock;
use crate::language::Language;
use crate::messages;
use crate::persist::ScratchFolder;
use crate::theme::{ColorMode, ColorSupport, GlyphSet, Theme};
use crate::ui::options::OptionRow;
use crate::ui::title::TitleItem;

/// English words a French screen never shows: the words of the labels and
/// sentences of the English catalogue that French does not share. A screen
/// that draws one of them still has a text written outside the catalogue,
/// or taken from the English one.
const ENGLISH_WORDS: &[&str] = &[
    "the",
    "The",
    "you",
    "You",
    "your",
    "Your",
    "and",
    "of",
    "to",
    "is",
    "with",
    "Depth",
    "Turn",
    "Turns",
    "Seed",
    "SEED",
    "Log",
    "HP",
    "SIGILS",
    "VIEW",
    "none",
    "yet",
    "move",
    "wait",
    "trace",
    "Trace",
    "take",
    "pack",
    "descend",
    "unseal",
    "quit",
    "Quit",
    "save",
    "Enter",
    "Esc",
    "Backspace",
    "choose",
    "use",
    "drop",
    "close",
    "Inventory",
    "cast",
    "cancel",
    "continue",
    "Continue",
    "New",
    "game",
    "Daily",
    "Back",
    "Colours",
    "Theme",
    "Dark",
    "Light",
    "Sample",
    "Glyphs",
    "Creatures",
    "slain",
    "learnt",
    "Sigils",
    "Items",
    "used",
    "Replay",
    "turns",
    "node",
    "nothing",
    "carry",
    "enter",
    "dungeon",
    "spell",
    "stairs",
    "seal",
    "glyph",
    "carved",
    "known",
    "expected",
    "traced",
    "FAILED",
    "UNSTABLE",
    "Time",
    "harm",
    "heal",
    "blink",
    "hold",
    "self",
    "aimed",
    "nearest",
    "Language",
    "confirm",
    "start",
    "erase",
    "back",
    "keep",
    "Abandon",
    "Keep",
    "Enlarge",
    "current",
    "Killed",
    "broke",
    "more",
    "link",
    "aim",
    "title",
    "screen",
    "change",
    "Sigil",
    "sigil",
    "Unstable",
    "Failed",
    "magic",
    "fizzles",
    "pure",
    "unstable",
    "fizzled",
    "true",
    "colour",
    "colours",
];

/// The French theme of Windows Terminal's default mode.
fn french() -> Theme {
    Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode).in_language(Language::French)
}

/// The text of `app` drawn on an 80 × 24 screen, one line per row.
fn screen(app: &App) -> String {
    rows(app, 80, 24).join("\n")
}

/// Launches the application as `main` does on `folder`, on a system whose
/// language is `system`, without arguments.
fn launch_on(folder: &ScratchFolder, system: Language) -> App {
    App::launch_with_clock(
        ColorSupport::TrueColor,
        system,
        &Cli {
            seed: None,
            daily: false,
            ascii: false,
            lang: None,
        },
        &shipped_content(),
        folder.save_file(),
        Box::new(ManualClock::new()),
    )
    .expect("the launch succeeds")
}

/// A fixed-room game of [`SEED`] whose player read the glyphs of the
/// shipped sigils `ids` and stands on its start cell.
fn knowing(ids: &[&str]) -> Game {
    let content = shipped_content();
    let mut game = Game::in_fixed_room(SEED);
    let mut cell = PLAYER_START;
    for id in ids {
        cell = cell.step(Direction::East);
        game.engrave(cell, content.sigil(id).expect("a shipped sigil"));
    }
    for _ in ids {
        game.submit(Action::Move(Direction::East));
    }
    for _ in ids {
        game.submit(Action::Move(Direction::West));
    }
    game
}

/// A fixed-room game of [`SEED`] whose player carries the shipped items
/// `ids`, in that order.
fn carrying(ids: &[&str]) -> Game {
    let content = shipped_content();
    let mut game = Game::in_fixed_room(SEED);
    for id in ids {
        let cell = game.player_actor().position();
        game.place_item(cell, content.item(id).expect("a shipped item"));
        game.submit(Action::PickUp);
    }
    assert_eq!(game.inventory().count(), ids.len());
    game
}

/// A fixed-room game of [`SEED`] where the player killed two ossuary rats
/// east of the start, then was killed by a hierophant.
fn killed_by_a_hierophant() -> Game {
    let content = shipped_content();
    let mut game = Game::in_fixed_room(SEED);
    let east = PLAYER_START.step(Direction::East);
    for _ in 0..2 {
        let before = game.stats().total_kills();
        game.spawn_monster(east, content.monster("ossuary_rat").unwrap());
        while game.stats().total_kills() == before {
            game.submit(Action::Move(Direction::East));
        }
    }
    game.spawn_monster(east, content.monster("hierophant").unwrap());
    for _ in 0..200 {
        if game.is_over() {
            break;
        }
        game.submit(Action::Wait);
    }
    assert!(game.is_over(), "the hierophant kills");
    game
}

/// Every screen of the game in French, by name, as their 80 × 24 text:
/// title and its steps, options, game, sigil overlay at each step,
/// inventory, end screens and the resize message.
fn french_screens() -> Vec<(&'static str, String)> {
    let folder = ScratchFolder::new("french-screens");
    let mut screens = Vec::new();
    let mut title = launch_with_arguments(&folder, &["--lang", "fr"]).unwrap();
    screens.push(("title", screen(&title)));
    go_to(&mut title, TitleItem::Seed);
    title.handle_key(press(KeyCode::Enter));
    title.handle_key(press(KeyCode::Char('x')));
    title.handle_key(press(KeyCode::Enter));
    screens.push(("seed entry", screen(&title)));
    let mut options = launch_with_arguments(&folder, &["--lang", "fr", "--ascii"]).unwrap();
    enter_options(&mut options);
    screens.push(("options", screen(&options)));
    go_to_row(&mut options, OptionRow::Language);
    screens.push(("options language", screen(&options)));

    let content = shipped_content();
    let mut game = App::with_game(french(), Game::new(SEED, &content), content.clone());
    for key in ['.', 'x', 'g', '>'] {
        game.handle_key(press(KeyCode::Char(key)));
    }
    screens.push(("game", screen(&game)));
    screens.push(("too small", rows(&game, 60, 20).join("\n")));

    let mut tracing = App::with_game(
        french(),
        knowing(&["recall", "veil", "passage", "lance"]),
        content.clone(),
    );
    for key in ['t', '7', '8', '5'] {
        tracing.handle_key(press(KeyCode::Char(key)));
    }
    screens.push(("trace", screen(&tracing)));
    tracing.handle_key(press(KeyCode::Char('5')));
    screens.push(("node refused", screen(&tracing)));
    tracing.handle_key(press(KeyCode::Enter));
    screens.push(("unknown pattern", screen(&tracing)));
    let mut aiming = App::with_game(french(), knowing(&["passage"]), content.clone());
    for key in ['t', '7', '4', '1', '2', '3'] {
        aiming.handle_key(press(KeyCode::Char(key)));
    }
    aiming.handle_key(press(KeyCode::Enter));
    screens.push(("aim", screen(&aiming)));
    let mut unstable = App::with_game(french(), knowing(&["lance"]), content.clone());
    for key in ['t', '1', '5', '9', '8'] {
        unstable.handle_key(press(KeyCode::Char(key)));
    }
    unstable.handle_key(press(KeyCode::Enter));
    screens.push(("unstable", screen(&unstable)));

    let mut empty = App::with_game(french(), Game::in_fixed_room(SEED), content.clone());
    empty.handle_key(press(KeyCode::Char('i')));
    screens.push(("empty inventory", screen(&empty)));
    let mut pack = App::with_game(
        french(),
        carrying(&[
            "healing_draught",
            "essence_phial",
            "steadying_ink",
            "veil_page",
        ]),
        content.clone(),
    );
    pack.handle_key(press(KeyCode::Char('i')));
    pack.handle_key(press(KeyCode::Down));
    screens.push(("inventory", screen(&pack)));
    pack.handle_key(press(KeyCode::Char('u')));
    screens.push(("used", screen(&pack)));

    let death = App::with_game(french(), killed_by_a_hierophant(), content);
    screens.push(("death", screen(&death)));
    let victory = App::with_game(french(), won_game(SEED), harmless());
    screens.push(("victory", screen(&victory)));
    screens
}

/// The words of `text`: its runs of letters, apostrophes inside a word
/// apart ("l'ombre" gives "l" and "ombre").
fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !c.is_alphabetic())
        .filter(|word| !word.is_empty())
}

#[test]
fn no_english_word_is_left_on_any_french_screen() {
    for (name, text) in french_screens() {
        // The line to share stays in English on purpose: players compare it
        // as text, whatever their language.
        let shown: Vec<&str> = text
            .lines()
            .filter(|row| !row.trim_start().starts_with("Sigilfall "))
            .collect();
        let shown = shown.join("\n");
        let english: Vec<&str> = words(&shown)
            .filter(|word| ENGLISH_WORDS.contains(word))
            .collect();
        assert!(english.is_empty(), "{name}: {english:?} in\n{shown}");
    }
}

#[test]
fn the_witness_words_are_found_on_the_english_screens() {
    // The list catches something: the same screens in English show its
    // words, so a French screen without them has really been translated.
    let folder = ScratchFolder::new("english-screens");
    let title = launch_with_arguments(&folder, &["--lang", "en"]).unwrap();
    let content = shipped_content();
    let game = App::with_game(
        Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode),
        Game::new(SEED, &content),
        content,
    );
    for app in [title, game] {
        let text = screen(&app);
        assert!(
            words(&text).any(|word| ENGLISH_WORDS.contains(&word)),
            "{text}"
        );
    }
}

#[test]
fn every_french_screen_fits_80_by_24_with_its_key_help_whole() {
    for (name, text) in french_screens() {
        let rows: Vec<&str> = text.lines().collect();
        if name == "too small" {
            assert!(text.contains("Agrandissez le terminal à 80"), "{text}");
            continue;
        }
        assert_eq!(rows.len(), 24, "{name}");
        for row in &rows {
            assert!(row.chars().count() <= 80, "{name}: {row:?}");
        }
    }
    let folder = ScratchFolder::new("french-help");
    let mut game = launch_with_arguments(&folder, &["--lang", "fr", "--seed", "42"]).unwrap();
    let help = rows(&game, 80, 24).pop().unwrap();
    for part in [
        "←↑→↓/1-9 aller",
        ". repos",
        "t tracer",
        "g saisir",
        "i sac",
        "> descente",
        "q quitter",
    ] {
        assert!(help.contains(part), "`{part}` is missing from {help:?}");
    }
    game.handle_key(press(KeyCode::Char('t')));
    let help = rows(&game, 80, 24).pop().unwrap();
    for part in [
        "1-9 relier un nœud",
        "Retour arrière effacer",
        "Entrée lancer",
        "Échap annuler",
    ] {
        assert!(help.contains(part), "`{part}` is missing from {help:?}");
    }
}

#[test]
fn a_french_system_opens_the_game_in_french_and_an_english_one_in_english() {
    let folder = ScratchFolder::new("locale-french");
    let french = launch_on(&folder, Language::French);
    assert_eq!(french.options().language, Language::French);
    let text = screen(&french);
    assert!(text.contains("Nouvelle partie"), "{text}");
    assert!(text.contains("Tracez juste. Le donjon écoute."), "{text}");

    let folder = ScratchFolder::new("locale-english");
    let english = launch_on(&folder, Language::English);
    assert!(screen(&english).contains("New game"));
    // Nothing is written until the player changes an option.
    assert!(!folder.options_file().path().exists());
}

#[test]
fn the_language_chosen_is_kept_whatever_the_system_says_afterwards() {
    let folder = ScratchFolder::new("language-kept");
    let mut app = launch_on(&folder, Language::English);
    enter_options(&mut app);
    go_to_row(&mut app, OptionRow::Language);
    app.handle_key(press(KeyCode::Right));

    assert_eq!(app.options().language, Language::French);
    let text = screen(&app);
    assert!(text.contains("Langue       Français"), "{text}");
    assert!(text.contains("O P T I O N S"), "{text}");
    let written = fs::read_to_string(folder.options_file().path()).unwrap();
    assert!(written.contains("language: French"), "{written}");

    let again = launch_on(&folder, Language::English);
    assert_eq!(again.options().language, Language::French);
    assert!(screen(&again).contains("Nouvelle partie"));

    // An options file written before the option existed keeps the system's.
    fs::write(
        folder.options_file().path(),
        "(format_version: 1, glyphs: Unicode, colors: Auto, theme: Dark)",
    )
    .unwrap();
    assert_eq!(
        launch_on(&folder, Language::French).options().language,
        Language::French
    );
    assert_eq!(
        launch_on(&folder, Language::English).options().language,
        Language::English
    );
}

#[test]
fn lang_wins_for_the_session_without_changing_the_option() {
    let folder = ScratchFolder::new("language-forced");
    let mut app = launch_with_arguments(&folder, &["--lang", "fr"]).unwrap();
    assert!(screen(&app).contains("Nouvelle partie"));
    enter_options(&mut app);
    let text = screen(&app);
    assert!(
        text.contains("--lang est actif\u{a0}: Français pour cette session."),
        "{text}"
    );
    assert!(text.contains("Langue       English"), "{text}");
    assert_eq!(app.options().language, Language::English);
}

#[test]
fn the_seed_and_the_line_to_share_do_not_depend_on_the_language() {
    let english = App::with_game(
        Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode),
        won_game(SEED),
        harmless(),
    );
    let french = App::with_game(french(), won_game(SEED), harmless());
    let line = |app: &App| {
        rows(app, 80, 24)
            .into_iter()
            .find(|row| row.trim_start().starts_with("Sigilfall "))
            .expect("the line to share")
    };
    assert_eq!(line(&english), line(&french));
    let seed = SEED.to_string();
    assert!(screen(&english).contains(&seed));
    assert!(screen(&french).contains(&seed));
    // The line under the title is chosen from the seed at the same place in
    // both languages.
    let english_line = messages::victory_line(Language::English, SEED);
    let french_line = messages::victory_line(Language::French, SEED);
    let position = |lines: &[&str], line: &str| lines.iter().position(|l| *l == line);
    assert_eq!(
        position(&messages::ENGLISH.victory_lines, english_line),
        position(&messages::FRENCH.victory_lines, french_line)
    );
}

#[test]
fn french_title_screen() {
    let folder = ScratchFolder::new("french-title");
    save_game(&folder, 7, 3);
    let app = launch_on(&folder, Language::French);
    assert_snapshot!(screen(&app));
}

#[test]
fn french_options_screen() {
    let folder = ScratchFolder::new("french-options");
    let mut app = launch_on(&folder, Language::French);
    enter_options(&mut app);
    go_to_row(&mut app, OptionRow::Language);
    assert!(matches!(app.state(), State::Options(_)));
    assert_snapshot!(screen(&app));
}

#[test]
fn french_game_screen() {
    let content = shipped_content();
    let mut app = App::with_game(french(), Game::new(SEED, &content), content);
    for key in ['.', 'g', '>', 'x'] {
        app.handle_key(press(KeyCode::Char(key)));
    }
    assert_snapshot!(screen(&app));
}

#[test]
fn french_inventory_screen() {
    let mut app = App::with_game(
        french(),
        carrying(&[
            "healing_draught",
            "essence_phial",
            "steadying_ink",
            "veil_page",
        ]),
        shipped_content(),
    );
    for key in [KeyCode::Char('i'), KeyCode::Down, KeyCode::Down] {
        app.handle_key(press(key));
    }
    assert_snapshot!(screen(&app));
}

#[test]
fn french_trace_beside_the_grimoire() {
    let mut app = App::with_game(
        french(),
        knowing(&["recall", "veil", "passage", "lance"]),
        shipped_content(),
    );
    for key in ['t', '7', '8', '5'] {
        app.handle_key(press(KeyCode::Char(key)));
    }
    assert_snapshot!(screen(&app));
}

#[test]
fn french_unstable_verdict() {
    let mut app = App::with_game(french(), knowing(&["lance"]), shipped_content());
    for key in ['t', '1', '5', '9', '8'] {
        app.handle_key(press(KeyCode::Char(key)));
    }
    // Lance is aimed: the direction is asked, then the verdict is shown.
    app.handle_key(press(KeyCode::Enter));
    app.handle_key(press(KeyCode::Char('l')));
    assert!(
        matches!(app.state(), State::Casting(_)),
        "{:?}",
        app.state()
    );
    assert_snapshot!(screen(&app));
}

#[test]
fn french_death_screen() {
    let app = App::with_game(french(), killed_by_a_hierophant(), shipped_content());
    assert_snapshot!(screen(&app));
}

#[test]
fn french_victory_screen() {
    let app = App::with_game(french(), won_game(SEED), harmless());
    assert_snapshot!(screen(&app));
}
