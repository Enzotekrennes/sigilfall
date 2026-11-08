//! The options screen (T26), driven by simulated keys on scratch folders
//! only: each option changed in place and applied to the screen at once,
//! kept in `options.ron` and found again at the next launch, `--ascii`
//! winning over the glyph option for its session, an unreadable or
//! unwritable options file handled with a notice and never a crash, and the
//! styles of the screen in every rendering mode.

use std::fs;

use ratatui::style::{Color, Modifier};

use super::*;
use crate::persist::{LoadedOptions, ScratchFolder};
use crate::theme::{Appearance, ColorChoice, ColorSupport, Options};
use crate::ui::options::OptionsMenu;
use crate::ui::title::{TitleItem, TitleMenu};

/// Launches the application as `main` does on `folder`, with `--ascii` if
/// `ascii`, on a terminal `detected` as such.
fn launch(folder: &ScratchFolder, detected: ColorSupport, ascii: bool) -> App {
    App::launch(
        detected,
        &Cli { seed: None, ascii },
        &shipped_content(),
        folder.save_file(),
    )
    .expect("the launch succeeds")
}

/// Launches the application on `folder` on a 24-bit terminal and opens the
/// options screen from the title menu (no save: "Options" is the third
/// entry).
fn open_options(folder: &ScratchFolder) -> App {
    let mut app = launch(folder, ColorSupport::TrueColor, false);
    press_all(&mut app, &[KeyCode::Down, KeyCode::Down, KeyCode::Enter]);
    assert_eq!(app.state(), State::Options(OptionsMenu::open()));
    app
}

/// The text of `app` drawn on an 80 × 24 screen.
fn screen_text(app: &App) -> String {
    render(app, 80, 24).backend().to_string()
}

/// The options saved in `folder`, which must be readable.
fn saved_options(folder: &ScratchFolder) -> Options {
    match folder.options_file().load() {
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
    press_all(&mut app, &[KeyCode::Down, KeyCode::Down, KeyCode::Enter]);

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

    let app = App::launch(
        ColorSupport::TrueColor,
        &Cli {
            seed: Some(SEED),
            ascii: false,
        },
        &shipped_content(),
        folder.save_file(),
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
            KeyCode::Down,
            KeyCode::Down,
            KeyCode::Enter,
            KeyCode::Right,
            KeyCode::Esc,
        ],
    );

    assert_eq!(saved_options(&folder).glyphs, GlyphSet::Ascii);
    assert!(!screen_text(&app).contains("could not be read"));
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
        text.contains("! the options could not be saved to"),
        "{text}"
    );
    assert!(text.contains("The options apply until you quit."), "{text}");
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
            &[KeyCode::Down, KeyCode::Down, KeyCode::Enter, KeyCode::Down],
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
