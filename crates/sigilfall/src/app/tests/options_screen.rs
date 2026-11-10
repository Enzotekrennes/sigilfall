//! The options screen and the colour modes, driven by simulated keys on scratch
//! folders only: each option changed in place and applied to the screen at
//! once, kept in `options.ron` and found again at the next launch, `--ascii`
//! winning over the glyph option for its session, an unreadable or unwritable
//! options file handled with a notice and never a crash, the data folder
//! created by the first option kept, the monochrome and `NO_COLOR` modes on
//! every screen, and the styles of the options screen in every rendering mode.

use super::player_view::{
    SIZES, background_brightness, buffer, change_option, enter_options, files, go_to, go_to_row,
    launch_with_arguments, launch_with_terminal, options_text, paint, row_of, rows, save_game,
    shipped, text, type_text,
};
use super::*;
use crate::app::{App, State};
use crate::cli::Cli;
use crate::clock::ManualClock;
use crate::persist::{LoadedOptions, OptionsFile, SaveFile, ScratchFolder};
use crate::theme::{Appearance, ColorChoice, ColorSupport, GlyphSet, Options, Settings};
use crate::ui::options::{OptionRow, OptionsMenu};
use crate::ui::title::{TitleItem, TitleMenu};
use ratatui::crossterm::event::KeyCode;
use ratatui::style::{Color, Modifier};
use sigilfall_core::game::Game;
use sigilfall_core::rng::Seed;
use std::fs;
use std::path::PathBuf;

/// Launches the application as `main` does on `folder`, with `--ascii` if
/// `ascii`, on a terminal `detected` as such.
fn launch(folder: &ScratchFolder, detected: ColorSupport, ascii: bool) -> App {
    App::launch_with_clock(
        detected,
        Language::English,
        &Cli {
            seed: None,
            daily: false,
            ascii,
            lang: None,
        },
        &shipped_content(),
        folder.save_file(),
        Box::new(ManualClock::new()),
    )
    .expect("the launch succeeds")
}

/// Launches the application on `folder` on a 24-bit terminal and opens the
/// options screen from the title menu ("Options" is the entry above the
/// last, "Quit").
fn open_options(folder: &ScratchFolder) -> App {
    let mut app = launch(folder, ColorSupport::TrueColor, false);
    press_all(&mut app, &[KeyCode::End, KeyCode::Up, KeyCode::Enter]);
    assert_eq!(app.state(), State::Options(OptionsMenu::open()));
    app
}

/// The text of `app` drawn on an 80 × 24 screen.
fn screen_text(app: &App) -> String {
    render(app, 80, 24).backend().to_string()
}

/// The options saved in `folder`, which must be readable.
fn saved_options(folder: &ScratchFolder) -> Options {
    match folder.options_file().load(Language::English) {
        LoadedOptions::Found(options) => options,
        other => panic!("the options are saved: {other:?}"),
    }
}

#[test]
fn the_options_screen_lists_the_three_options_and_back() {
    let folder = ScratchFolder::new("options-screen");

    let app = open_options(&folder);

    let text = screen_text(&app);
    for needle in [
        "O P T I O N S",
        "› Glyphs",
        "Unicode",
        "Colours",
        "Auto · true colour",
        "Theme",
        "Dark",
        "Back",
        "Sample",
        "Unicode draws stone walls and sigils; ASCII suits any font.",
        " ↑↓ choose   ←→ change   Esc back",
    ] {
        assert!(text.contains(needle), "{needle}:\n{text}");
    }
    assert!(
        !folder.options_file().path().exists(),
        "opening the screen writes nothing"
    );
    assert_snapshot!("options_screen", text);
}

#[test]
fn each_option_applies_at_once_and_is_kept_after_a_relaunch() {
    let folder = ScratchFolder::new("options-kept");
    let mut app = open_options(&folder);

    // Glyphs: ASCII at once, on this very screen.
    press_all(&mut app, &[KeyCode::Right]);
    assert_eq!(app.options().glyphs, GlyphSet::Ascii);
    let text = screen_text(&app);
    assert!(text.is_ascii(), "{text}");
    assert!(text.contains("> Glyphs       ASCII"), "{text}");
    assert_eq!(
        saved_options(&folder).glyphs,
        GlyphSet::Ascii,
        "written at once"
    );

    // Colours: 16 colours, then the theme: light, drawn in monochrome
    // with 16 colours, which the screen explains.
    press_all(&mut app, &[KeyCode::Down, KeyCode::Right]);
    assert_eq!(app.options().colors, ColorChoice::Ansi16);
    assert_eq!(app.theme().text.fg, Some(Color::White));
    press_all(&mut app, &[KeyCode::Down, KeyCode::Right]);
    assert_eq!(app.options().appearance, Appearance::Light);
    assert_eq!(app.theme().text.fg, None, "monochrome");
    let text = screen_text(&app);
    assert!(
        text.contains("Light needs true colour: drawn in monochrome here."),
        "{text}"
    );
    let expected = Options {
        glyphs: GlyphSet::Ascii,
        colors: ColorChoice::Ansi16,
        appearance: Appearance::Light,
        language: Language::English,
    };
    assert_eq!(saved_options(&folder), expected);

    // Escape goes back to the title, on Options, in the new look.
    press_all(&mut app, &[KeyCode::Esc]);
    assert_eq!(
        app.state(),
        State::Title(TitleMenu::on(TitleItem::Options, false))
    );
    assert!(screen_text(&app).contains("> Options"));

    // The next launch finds them, and draws with them from the start.
    let relaunched = launch(&folder, ColorSupport::TrueColor, false);
    assert_eq!(relaunched.options(), expected);
    assert!(screen_text(&relaunched).is_ascii());
    assert_eq!(relaunched.theme().text.fg, None, "monochrome");
}

#[test]
fn the_light_theme_on_true_colour_is_the_parchment() {
    let folder = ScratchFolder::new("options-light");
    let mut app = open_options(&folder);

    press_all(&mut app, &[KeyCode::Down, KeyCode::Down, KeyCode::Enter]);

    assert_eq!(app.options().appearance, Appearance::Light);
    let buffer = render(&app, 80, 24).backend().buffer().clone();
    assert_eq!(buffer[(0, 0)].bg, Color::Rgb(0xF2, 0xEB, 0xDC));
    assert!(!screen_text(&app).contains("monochrome here"));
}

#[test]
fn values_cycle_both_ways_and_enter_on_back_returns_to_the_title() {
    let folder = ScratchFolder::new("options-cycle");
    let mut app = open_options(&folder);
    press_all(&mut app, &[KeyCode::Down]);

    let mut seen = Vec::new();
    for _ in 0..3 {
        press_all(&mut app, &[KeyCode::Right]);
        seen.push(app.options().colors);
    }
    assert_eq!(
        seen,
        [
            ColorChoice::Ansi16,
            ColorChoice::Monochrome,
            ColorChoice::Auto
        ]
    );
    press_all(&mut app, &[KeyCode::Left]);
    assert_eq!(app.options().colors, ColorChoice::Monochrome);
    press_all(&mut app, &[KeyCode::Char('h'), KeyCode::Char('l')]);
    assert_eq!(app.options().colors, ColorChoice::Monochrome);
    press_all(&mut app, &[KeyCode::Char(' ')]);
    assert_eq!(app.options().colors, ColorChoice::Auto, "Space too");

    // q does not quit here; Back is the last entry, reached by wrapping.
    press_all(&mut app, &[KeyCode::Char('q'), KeyCode::Up, KeyCode::Up]);
    assert_eq!(app.state(), State::Options(OptionsMenu::last()));
    press_all(&mut app, &[KeyCode::Right]);
    assert_eq!(
        app.state(),
        State::Options(OptionsMenu::last()),
        "Back has no value"
    );
    press_all(&mut app, &[KeyCode::Enter]);
    assert_eq!(
        app.state(),
        State::Title(TitleMenu::on(TitleItem::Options, false))
    );
}

#[test]
fn ascii_on_the_command_line_wins_for_the_session_without_changing_the_option() {
    let folder = ScratchFolder::new("options-cli-ascii");
    let mut app = launch(&folder, ColorSupport::TrueColor, true);
    press_all(&mut app, &[KeyCode::End, KeyCode::Up, KeyCode::Enter]);

    let text = screen_text(&app);
    assert!(text.is_ascii(), "{text}");
    assert!(
        text.contains("> Glyphs       Unicode"),
        "the option: {text}"
    );
    assert!(
        text.contains("--ascii is on: ASCII glyphs for this session."),
        "{text}"
    );

    // Changing the option keeps ASCII for this session, and is saved.
    press_all(&mut app, &[KeyCode::Right, KeyCode::Right]);
    assert!(screen_text(&app).is_ascii());
    assert_eq!(saved_options(&folder).glyphs, GlyphSet::Unicode);

    // Without --ascii, the next launch follows the option again.
    let plain = launch(&folder, ColorSupport::TrueColor, false);
    assert!(screen_text(&plain).contains('›'));
}

#[test]
fn the_options_also_apply_to_a_game_started_with_seed() {
    let folder = ScratchFolder::new("options-seed");
    let ascii = Options {
        glyphs: GlyphSet::Ascii,
        ..Options::default()
    };
    folder.options_file().write(&ascii).unwrap();

    let app = App::launch_with_clock(
        ColorSupport::TrueColor,
        Language::English,
        &Cli {
            seed: Some(SEED),
            daily: false,
            ascii: false,
            lang: None,
        },
        &shipped_content(),
        folder.save_file(),
        Box::new(ManualClock::new()),
    )
    .unwrap();

    assert_eq!(app.state(), State::Playing);
    assert!(screen_text(&app).is_ascii());
}

#[test]
fn a_corrupt_options_file_gives_the_defaults_and_a_notice_without_crashing() {
    for (label, content) in [
        ("garbage", b"this is not RON at all".to_vec()),
        ("not-text", vec![0xFF, 0xFE, 0x00]),
        ("empty", Vec::new()),
        (
            "unknown-value",
            b"(format_version: 1, glyphs: Emoji, colors: Auto, theme: Dark)".to_vec(),
        ),
        (
            "other-version",
            b"(format_version: 9, glyphs: Ascii, colors: Auto, theme: Dark)".to_vec(),
        ),
    ] {
        let folder = ScratchFolder::new(label);
        fs::write(folder.options_file().path(), &content).unwrap();

        let app = launch(&folder, ColorSupport::TrueColor, false);

        assert_eq!(app.options(), Options::default(), "{label}");
        assert_eq!(app.state(), State::Title(TitleMenu::open()), "{label}");
        let text = screen_text(&app);
        assert!(
            text.contains("! The options file could not be read"),
            "{label}:\n{text}"
        );
        assert_eq!(
            fs::read(folder.options_file().path()).unwrap(),
            content,
            "{label}: left as it was until an option changes"
        );
    }
}

#[test]
fn an_options_file_of_another_version_says_so() {
    let folder = ScratchFolder::new("options-version");
    fs::write(
        folder.options_file().path(),
        "(format_version: 9, glyphs: Ascii, colors: Auto, theme: Dark)",
    )
    .unwrap();

    let app = launch(&folder, ColorSupport::TrueColor, false);

    let text = screen_text(&app);
    assert!(text.contains("format version 9"), "{text}");
    assert_snapshot!("title_with_an_unreadable_options_file", text);
}

#[test]
fn changing_an_option_replaces_a_corrupt_file_and_clears_the_notice() {
    let folder = ScratchFolder::new("options-repaired");
    fs::write(folder.options_file().path(), "oops").unwrap();
    let mut app = launch(&folder, ColorSupport::TrueColor, false);

    press_all(
        &mut app,
        &[
            KeyCode::End,
            KeyCode::Up,
            KeyCode::Enter,
            KeyCode::Right,
            KeyCode::Esc,
        ],
    );

    assert_eq!(saved_options(&folder).glyphs, GlyphSet::Ascii);
    assert!(!screen_text(&app).contains("could not be read"));
}

#[test]
fn in_a_long_data_folder_the_notice_still_says_the_options_apply_until_quitting() {
    // As long as the temporary folders of some systems or macOS's
    // `~/Library/Application Support/Sigilfall`, and longer than a line.
    let folder = ScratchFolder::new("options-long-folder");
    let deep = folder
        .path()
        .join("RUNNER~1-AppData-Local-Temp-of-a-continuous-integration-runner")
        .join("var-folders-zz-zyxvpxvq6csfxvn_n0000000000000-T")
        .join("Library")
        .join("Application Support")
        .join("Sigilfall");
    fs::create_dir_all(deep.join("options.ron.tmp")).unwrap();
    assert!(deep.display().to_string().chars().count() > 160);
    for (language, explanation) in [
        (Language::English, "The options apply until you quit"),
        (
            Language::French,
            "Elles s'appliquent jusqu'à ce que vous quittiez",
        ),
    ] {
        let mut app = App::launch_with_clock(
            ColorSupport::TrueColor,
            language,
            &Cli {
                seed: None,
                daily: false,
                ascii: false,
                lang: None,
            },
            &shipped_content(),
            SaveFile::in_folder(&deep),
            Box::new(ManualClock::new()),
        )
        .expect("the launch succeeds");
        press_all(&mut app, &[KeyCode::End, KeyCode::Up, KeyCode::Enter]);
        press_all(&mut app, &[KeyCode::Right]);

        let text = screen_text(&app);
        let words: Vec<&str> = text
            .split(|c: char| c.is_whitespace() || c == '"')
            .filter(|word| !word.is_empty())
            .collect();
        let words = words.join(" ");
        assert!(words.contains(explanation), "{language:?}: {text}");
        assert!(words.contains("options.ron"), "{language:?}: {text}");
    }
}

#[test]
fn options_that_cannot_be_written_still_apply_and_the_screen_says_why() {
    let folder = ScratchFolder::new("options-unwritable");
    // A folder named like the temporary file makes every write fail.
    fs::create_dir(folder.path().join("options.ron.tmp")).unwrap();
    let mut app = open_options(&folder);

    press_all(&mut app, &[KeyCode::Right]);

    assert_eq!(app.options().glyphs, GlyphSet::Ascii, "applied anyway");
    let text = screen_text(&app);
    assert!(
        text.contains("! the options could not be saved ("),
        "{text}"
    );
    // The notice wraps: its words are read whole across rows.
    let words: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || c == '"')
        .filter(|word| !word.is_empty())
        .collect();
    assert!(
        words
            .join(" ")
            .contains("The options apply until you quit;"),
        "{text}"
    );
    assert!(!folder.options_file().path().exists());
}

#[test]
fn the_options_screen_reads_without_colour_in_every_mode() {
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
        (Options::default(), ColorSupport::Ansi16, "ansi16"),
        (
            Options {
                colors: ColorChoice::Monochrome,
                ..Options::default()
            },
            ColorSupport::TrueColor,
            "monochrome",
        ),
    ];
    for (options, detected, label) in modes {
        let folder = ScratchFolder::new(label);
        folder.options_file().write(&options).unwrap();
        let mut app = launch(&folder, detected, false);
        press_all(
            &mut app,
            &[KeyCode::End, KeyCode::Up, KeyCode::Enter, KeyCode::Down],
        );
        let buffer = render(&app, 80, 24).backend().buffer().clone();
        let theme = app.theme().clone();
        let reset = Some(Color::Reset);

        let (y, row) = (0..24)
            .map(|y| (y, row_text(&buffer, y)))
            .find(|(_, row)| row.contains("› Colours"))
            .expect("Colours is chosen");
        let x = u16::try_from(row.chars().position(|c| c == '›').unwrap()).unwrap();
        let label_cell = &buffer[(x + 2, y)];
        let value_cell = &buffer[(x + 15, y)];
        match theme.bg_selection.bg {
            Some(selection) => assert_eq!(value_cell.bg, selection, "{label}"),
            None => assert!(value_cell.modifier.contains(Modifier::REVERSED), "{label}"),
        }
        assert_eq!(
            Some(label_cell.fg),
            theme.text_muted.fg.or(reset),
            "{label}"
        );
        assert_eq!(Some(value_cell.fg), theme.text.fg.or(reset), "{label}");
        let title_y = (0..24)
            .find(|&y| row_text(&buffer, y).contains("O P T I O N S"))
            .unwrap();
        let title_x = u16::try_from(
            row_text(&buffer, title_y)
                .chars()
                .position(|c| c == 'O')
                .unwrap(),
        )
        .unwrap();
        let title = &buffer[(title_x, title_y)];
        assert_eq!(Some(title.fg), theme.text_title.fg.or(reset), "{label}");
        assert!(title.modifier.contains(Modifier::BOLD), "{label}");
        // The sample's player glyph in the player token.
        let sample_y = (0..24)
            .find(|&y| row_text(&buffer, y).contains("Sample"))
            .unwrap();
        let sample = row_text(&buffer, sample_y);
        let at = u16::try_from(sample.chars().position(|c| c == '@').unwrap()).unwrap();
        assert_eq!(
            Some(buffer[(at, sample_y)].fg),
            theme.player.fg.or(reset),
            "{label}"
        );
        assert_eq!(Some(buffer[(0, 0)].bg), theme.bg.bg, "{label}");
    }
}

// --- The options apply at once and are kept ---------------------

#[test]
fn the_glyph_option_redraws_at_once_and_the_choice_is_kept_for_the_next_session() {
    let folder = ScratchFolder::new("glyphs");
    let mut app = launch_with_arguments(&folder, &[]).unwrap();
    assert_eq!(
        app.options().glyphs,
        GlyphSet::Unicode,
        "Unicode by default"
    );
    assert!(
        !text(&app).is_ascii(),
        "the Unicode title uses Unicode glyphs"
    );

    enter_options(&mut app);
    let before = rows(&app, 80, 24);
    assert!(
        before
            .iter()
            .any(|row| row.contains("Glyphs") && row.contains("Unicode")),
        "{before:#?}"
    );
    let sample = row_of(&before, "Sample").expect("a sample line");
    assert!(!before[sample].is_ascii(), "{}", before[sample]);

    app.handle_key(press(KeyCode::Enter));
    let after = rows(&app, 80, 24);
    assert_eq!(app.options().glyphs, GlyphSet::Ascii, "the option changed");
    assert!(
        after
            .iter()
            .any(|row| row.contains("Glyphs") && row.contains("ASCII")),
        "the value is shown at once"
    );
    assert!(
        after[sample].is_ascii(),
        "the sample is redrawn in ASCII: {}",
        after[sample]
    );

    let file = options_text(&folder);
    assert!(
        file.contains("format_version: 1") && file.contains("glyphs: Ascii"),
        "{file}"
    );
    assert_eq!(
        files(&folder),
        ["options.ron"],
        "written whole, no temporary file left"
    );

    app.handle_key(press(KeyCode::Esc));
    for (width, height) in SIZES {
        assert!(
            rows(&app, width, height)
                .iter()
                .all(|row| row.chars().all(|c| c.is_ascii() || matches!(c, '↑' | '↓'))),
            "the title is plain ASCII now (key help arrows excepted)"
        );
    }

    // A new session keeps the choice, on the title and in the game.
    let mut next = launch_with_arguments(&folder, &[]).unwrap();
    assert_eq!(next.options().glyphs, GlyphSet::Ascii);
    next.handle_key(press(KeyCode::Enter));
    assert_eq!(next.state(), State::Playing);
    let map_rows = &rows(&next, 80, 24)[..23];
    assert!(
        map_rows.iter().all(|row| row.is_ascii()),
        "the game screen is plain ASCII:\n{}",
        map_rows.join("\n")
    );

    // And back to Unicode, also kept.
    let mut again = launch_with_arguments(&folder, &[]).unwrap();
    enter_options(&mut again);
    again.handle_key(press(KeyCode::Left));
    assert_eq!(again.options().glyphs, GlyphSet::Unicode, "Left goes back");
    assert!(options_text(&folder).contains("glyphs: Unicode"));
    again.handle_key(press(KeyCode::Char(' ')));
    assert_eq!(
        again.options().glyphs,
        GlyphSet::Ascii,
        "Space changes the value"
    );
}

#[test]
fn the_colour_options_apply_at_once_and_are_kept() {
    let folder = ScratchFolder::new("colours");
    let mut app = launch_with_arguments(&folder, &[]).unwrap();
    let auto = paint(&buffer(&app, 80, 24));
    assert!(auto.rgb && !auto.named, "true colour by default: {auto:?}");

    enter_options(&mut app);
    go_to_row(&mut app, OptionRow::Colors);
    app.handle_key(press(KeyCode::Enter));
    assert_eq!(app.options().colors, ColorChoice::Ansi16);
    let sixteen = paint(&buffer(&app, 80, 24));
    assert!(
        sixteen.named && !sixteen.rgb && !sixteen.indexed,
        "16 colours: only the named ANSI colours, at once: {sixteen:?}"
    );
    assert!(
        rows(&app, 80, 24)
            .iter()
            .any(|row| row.contains("Colours") && row.contains("16 colours"))
    );
    assert!(options_text(&folder).contains("colors: Ansi16"));

    app.handle_key(press(KeyCode::Enter));
    assert_eq!(app.options().colors, ColorChoice::Monochrome);
    let mono = paint(&buffer(&app, 80, 24));
    assert!(
        !mono.rgb && !mono.indexed && !mono.named && mono.emphasis,
        "none: no colour at all, hierarchy by emphasis: {mono:?}"
    );
    assert!(options_text(&folder).contains("colors: Monochrome"));

    app.handle_key(press(KeyCode::Enter));
    assert_eq!(
        app.options().colors,
        ColorChoice::Auto,
        "the cycle comes back to Auto"
    );
    assert!(paint(&buffer(&app, 80, 24)).rgb);
    app.handle_key(press(KeyCode::Left));
    assert_eq!(
        app.options().colors,
        ColorChoice::Monochrome,
        "Left goes backward"
    );

    // Kept: a new session starts monochrome, on every screen.
    let mut next = launch_with_arguments(&folder, &[]).unwrap();
    assert_eq!(next.options().colors, ColorChoice::Monochrome);
    let title = paint(&buffer(&next, 80, 24));
    assert!(!title.rgb && !title.named && !title.indexed);
    next.handle_key(press(KeyCode::Enter));
    let game = paint(&buffer(&next, 80, 24));
    assert!(
        !game.rgb && !game.named && !game.indexed && game.emphasis,
        "{game:?}"
    );
}

#[test]
fn the_light_theme_paints_a_light_page_at_once_is_kept_and_needs_true_colour() {
    let folder = ScratchFolder::new("light");
    let mut app = launch_with_arguments(&folder, &[]).unwrap();
    let dark = background_brightness(&buffer(&app, 80, 24)).expect("a 24-bit background");
    assert!(dark < 60, "the dark theme paints ink: {dark}");

    enter_options(&mut app);
    go_to_row(&mut app, OptionRow::Theme);
    app.handle_key(press(KeyCode::Enter));
    assert_eq!(app.options().appearance, Appearance::Light);
    let light = background_brightness(&buffer(&app, 80, 24)).expect("a 24-bit background");
    assert!(light > 180, "the light theme paints parchment: {light}");
    assert!(options_text(&folder).contains("theme: Light"));
    app.handle_key(press(KeyCode::Esc));
    let title = background_brightness(&buffer(&app, 80, 24)).unwrap();
    assert!(title > 180, "the title is light too");

    let mut next = launch_with_arguments(&folder, &[]).unwrap();
    assert_eq!(next.options().appearance, Appearance::Light);
    next.handle_key(press(KeyCode::Enter));
    let game_light = background_brightness(&buffer(&next, 80, 24)).unwrap();
    assert!(game_light > 180, "the game is light: {game_light}");

    // README: "light for 24-bit colour terminals". With 16 colours it is
    // drawn without colour instead, and still readable.
    let sixteen =
        launch_with_terminal(&folder, &[], ColorSupport::Ansi16, &ManualClock::new()).unwrap();
    let painted = paint(&buffer(&sixteen, 80, 24));
    assert!(
        !painted.rgb && !painted.named && !painted.indexed && painted.emphasis,
        "{painted:?}"
    );
    assert!(
        rows(&sixteen, 80, 24)
            .iter()
            .any(|row| row.contains("New game")),
        "the title is still there"
    );
}

#[test]
fn no_color_and_the_terminal_report_decide_the_colours_as_the_readme_says() {
    use std::ffi::OsString;
    let support = |pairs: &[(&str, &str)]| {
        ColorSupport::from_env(|name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| OsString::from(value))
        })
    };
    assert_eq!(support(&[("NO_COLOR", "1")]), ColorSupport::None);
    assert_eq!(support(&[("NO_COLOR", "anything")]), ColorSupport::None);
    assert_eq!(
        support(&[
            ("NO_COLOR", "1"),
            ("COLORTERM", "truecolor"),
            ("WT_SESSION", "x")
        ]),
        ColorSupport::None,
        "NO_COLOR beats every announcement of colour"
    );
    assert_eq!(
        support(&[("NO_COLOR", ""), ("COLORTERM", "truecolor")]),
        ColorSupport::TrueColor,
        "an empty NO_COLOR counts as unset"
    );
    assert_eq!(
        support(&[("COLORTERM", "truecolor")]),
        ColorSupport::TrueColor
    );
    assert_eq!(support(&[("COLORTERM", "24bit")]), ColorSupport::TrueColor);
    assert_eq!(support(&[("COLORTERM", "24BIT")]), ColorSupport::TrueColor);
    assert_eq!(support(&[("WT_SESSION", "any")]), ColorSupport::TrueColor);
    assert_eq!(support(&[("COLORTERM", "yes")]), ColorSupport::Ansi16);
    assert_eq!(
        support(&[]),
        ColorSupport::Ansi16,
        "otherwise the 16 ANSI colours"
    );
}

/// Every screen of the application on a terminal that supports `support`,
/// with `options`: the title, the options, the seed entry, the
/// confirmation, the game, the trace and the inventory.
fn every_screen(support: ColorSupport, options: Options) -> Vec<(&'static str, App)> {
    let content = shipped();
    let settings = Settings {
        options,
        detected: support,
        forced_ascii: false,
        forced_language: None,
    };
    let folder = ScratchFolder::new("screens");
    save_game(&folder, 7, 3);
    // The options reach the screens through the file, as a player's do.
    folder.options_file().write(&options).unwrap();
    let fresh = || launch_with_terminal(&folder, &[], support, &ManualClock::new()).unwrap();

    let mut screens = vec![("title", fresh())];
    let mut on_options = fresh();
    enter_options(&mut on_options);
    screens.push(("options", on_options));
    let mut on_seed = fresh();
    go_to(&mut on_seed, TitleItem::Seed);
    on_seed.handle_key(press(KeyCode::Enter));
    type_text(&mut on_seed, "zz");
    on_seed.handle_key(press(KeyCode::Enter));
    screens.push(("seed entry with an error", on_seed));
    let mut on_confirm = fresh();
    go_to(&mut on_confirm, TitleItem::NewGame);
    on_confirm.handle_key(press(KeyCode::Enter));
    assert!(matches!(on_confirm.state(), State::Confirm(_)));
    screens.push(("confirmation", on_confirm));

    let game = || Game::new(Seed::from(7), &content);
    let mut playing = App::with_game(settings.theme(), game(), content.clone());
    type_text(&mut playing, "..");
    screens.push(("game", playing));
    let mut trace = App::with_game(settings.theme(), game(), content.clone());
    type_text(&mut trace, "t75");
    screens.push(("trace", trace));
    let mut pack = App::with_game(settings.theme(), game(), content.clone());
    pack.handle_key(press(KeyCode::Char('i')));
    screens.push(("inventory", pack));
    screens
}

#[test]
fn with_no_colour_every_screen_is_drawn_without_colour_but_not_without_hierarchy() {
    let none = every_screen(ColorSupport::None, Options::default());
    assert_eq!(none.len(), 7);
    for (name, app) in &none {
        for (width, height) in SIZES {
            let painted = paint(&buffer(app, width, height));
            assert!(
                !painted.rgb && !painted.indexed && !painted.named,
                "{name} at {width}x{height} is coloured: {painted:?}"
            );
        }
    }
    let with_emphasis = none
        .iter()
        .filter(|(_, app)| paint(&buffer(app, 80, 24)).emphasis)
        .count();
    assert!(
        with_emphasis >= 6,
        "bold, reversed or dim carry the hierarchy: {with_emphasis}/{}",
        none.len()
    );

    // The same screens do have colour when colour is available: the test
    // above is not passing because nothing is ever painted.
    for support in [ColorSupport::TrueColor, ColorSupport::Ansi16] {
        let coloured = every_screen(support, Options::default());
        for (name, app) in &coloured {
            let painted = paint(&buffer(app, 80, 24));
            assert!(
                painted.rgb || painted.named,
                "{name} with {support:?}: {painted:?}"
            );
            assert_eq!(
                painted.rgb,
                support == ColorSupport::TrueColor,
                "{name}: {painted:?}"
            );
            assert!(!painted.indexed, "{name}: no 256-colour palette is used");
        }
    }
}

#[test]
fn the_explicit_ascii_and_monochrome_choices_hold_on_every_screen() {
    let options = Options {
        glyphs: GlyphSet::Ascii,
        colors: ColorChoice::Monochrome,
        appearance: Appearance::Dark,
        language: Language::English,
    };
    for (name, app) in every_screen(ColorSupport::TrueColor, options) {
        let painted = paint(&buffer(&app, 80, 24));
        assert!(
            !painted.rgb && !painted.named && !painted.indexed,
            "{name}: {painted:?}"
        );
        let screen = rows(&app, 80, 24);
        // Only the arrows of the key help are left outside ASCII.
        let strange: Vec<&String> = screen
            .iter()
            .filter(|row| {
                !row.chars()
                    .all(|c| c.is_ascii() || matches!(c, '↑' | '↓' | '←' | '→'))
            })
            .collect();
        assert!(strange.is_empty(), "{name}: {strange:#?}");
    }
}

// --- --ascii wins over the option ------------------------------------------

#[test]
fn ascii_on_the_command_line_beats_the_saved_option_for_the_session_only() {
    let folder = ScratchFolder::new("ascii-flag");
    folder.options_file().write(&Options::default()).unwrap();
    let saved = options_text(&folder);
    assert!(saved.contains("glyphs: Unicode"));

    let mut app = launch_with_arguments(&folder, &["--ascii"]).unwrap();
    assert_eq!(
        app.options().glyphs,
        GlyphSet::Unicode,
        "the option is untouched"
    );
    assert!(
        rows(&app, 80, 24)
            .iter()
            .all(|row| row.chars().all(|c| c.is_ascii() || matches!(c, '↑' | '↓'))),
        "the title is drawn in ASCII"
    );
    enter_options(&mut app);
    assert!(
        text(&app).contains("--ascii is on"),
        "the options screen says the flag is in force:\n{}",
        text(&app)
    );
    // Changing another option writes the file, with the glyph option as the
    // player had it, not as the flag forces it.
    go_to_row(&mut app, OptionRow::Colors);
    app.handle_key(press(KeyCode::Enter));
    let file = options_text(&folder);
    assert!(
        file.contains("glyphs: Unicode") && file.contains("colors: Ansi16"),
        "{file}"
    );
    app.handle_key(press(KeyCode::Esc));
    app.handle_key(press(KeyCode::Enter));
    assert!(
        rows(&app, 80, 24)[..23].iter().all(|row| row.is_ascii()),
        "the game too is ASCII"
    );

    // Without the flag, Unicode is back.
    let plain = launch_with_arguments(&folder, &[]).unwrap();
    assert!(!text(&plain).is_ascii());

    // And an option saved as ASCII needs no flag.
    let other = ScratchFolder::new("ascii-option");
    other
        .options_file()
        .write(&Options {
            glyphs: GlyphSet::Ascii,
            ..Options::default()
        })
        .unwrap();
    let by_option = launch_with_arguments(&other, &[]).unwrap();
    assert!(by_option.options().glyphs == GlyphSet::Ascii);
    assert!(
        rows(&by_option, 80, 24)
            .iter()
            .all(|row| row.chars().all(|c| c.is_ascii() || matches!(c, '↑' | '↓')))
    );
}

// --- An options file that cannot be read ------------------------------------

#[test]
fn an_options_file_that_cannot_be_read_gives_the_defaults_a_notice_and_is_replaced_by_the_first_change()
 {
    let huge = format!("(format_version: 1, glyphs: {})", "U".repeat(20_000));
    let cases: [(&str, Vec<u8>); 9] = [
        ("garbage", b"this is not ron {{{".to_vec()),
        ("empty", Vec::new()),
        ("binary", vec![0xff, 0xfe, 0x00, 0x80]),
        (
            "another version",
            b"(format_version: 99, glyphs: Ascii, colors: Auto, theme: Dark)".to_vec(),
        ),
        (
            "unknown value",
            b"(format_version: 1, glyphs: Braille, colors: Auto, theme: Dark)".to_vec(),
        ),
        (
            "missing field",
            b"(format_version: 1, glyphs: Ascii)".to_vec(),
        ),
        ("truncated", b"(format_version: 1, glyphs: As".to_vec()),
        ("huge", huge.into_bytes()),
        ("only a number", b"42".to_vec()),
    ];
    for (name, bytes) in cases {
        let folder = ScratchFolder::new("corrupt-options");
        fs::write(folder.options_file().path(), &bytes).unwrap();

        let mut app =
            launch_with_arguments(&folder, &[]).unwrap_or_else(|error| panic!("{name}: {error:#}"));
        assert_eq!(
            app.options(),
            Options::default(),
            "{name}: the defaults apply"
        );
        for (width, height) in SIZES {
            let screen = rows(&app, width, height);
            assert!(
                screen
                    .iter()
                    .any(|row| row.contains("options file could not be read")),
                "{name} at {width}x{height}: a notice says so:\n{}",
                screen.join("\n")
            );
            assert!(
                row_of(&screen, "New game").is_some(),
                "{name}: the menu is there"
            );
        }
        let notice_rows: Vec<String> = rows(&app, 80, 24)
            .into_iter()
            .skip_while(|row| !row.contains("options file could not be read"))
            .take_while(|row| !row.contains("choose"))
            .collect();
        assert!(
            notice_rows.len() <= 6,
            "{name}: the notice stays short: {notice_rows:#?}"
        );
        assert!(
            notice_rows.iter().all(|row| row.chars().count() <= 80),
            "{name}: and fits the screen"
        );
        assert!(
            notice_rows
                .join(" ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .contains("changing one rewrites the file"),
            "{name}: it says what to do: {notice_rows:#?}"
        );

        // A plain session leaves the file as it found it.
        app.save_on_exit().unwrap();
        assert_eq!(
            fs::read(folder.options_file().path()).unwrap(),
            bytes,
            "{name}"
        );

        // The first change replaces it with a valid file.
        change_option(&mut app, OptionRow::Glyphs);
        let file = options_text(&folder);
        assert!(
            file.contains("format_version: 1") && file.contains("glyphs: Ascii"),
            "{name}: {file}"
        );
        assert!(
            !text(&app).contains("options file could not be read"),
            "{name}: the notice is gone once the file is rewritten"
        );
        assert!(matches!(
            OptionsFile::in_folder(folder.path()).load(Language::English),
            LoadedOptions::Found(_)
        ));
        let next = launch_with_arguments(&folder, &[]).unwrap();
        assert!(!text(&next).contains("could not be read"), "{name}");
        assert_eq!(next.options().glyphs, GlyphSet::Ascii, "{name}");
    }
}

#[test]
fn an_options_file_that_is_a_folder_or_cannot_be_written_never_crashes_the_game() {
    let folder = ScratchFolder::new("options-folder");
    fs::create_dir(folder.options_file().path()).unwrap();
    let mut app = launch_with_arguments(&folder, &[]).unwrap();
    assert!(text(&app).contains("options file could not be read"));

    enter_options(&mut app);
    app.handle_key(press(KeyCode::Enter));
    assert_eq!(
        app.options().glyphs,
        GlyphSet::Ascii,
        "the option applies for the session"
    );
    let screen = text(&app);
    assert!(
        screen.contains("could not be saved"),
        "the screen says why:\n{screen}"
    );
    assert!(
        screen.contains("until you quit"),
        "and for how long:\n{screen}"
    );
    assert!(
        folder.options_file().path().is_dir(),
        "the folder is left alone"
    );
    // The game still starts.
    app.handle_key(press(KeyCode::Esc));
    app.handle_key(press(KeyCode::Home));
    app.handle_key(press(KeyCode::Enter));
    assert_eq!(app.state(), State::Playing);
}

#[test]
fn the_data_folder_is_created_when_the_first_option_is_kept() {
    let folder = ScratchFolder::new("created");
    let nested: PathBuf = folder.path().join("a").join("b");
    let options = OptionsFile::in_folder(&nested);
    assert!(!nested.exists());
    options.write(&Options::default()).unwrap();
    assert!(
        nested.join("options.ron").is_file(),
        "README: created if missing"
    );
}
