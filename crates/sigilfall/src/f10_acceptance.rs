//! Acceptance tests of F10 (finish), binary half.
//!
//! They drive the application as `main` does, on scratch folders only, and
//! read what the player reads: the rendered screens, the colours and
//! emphasis of every cell, and the files on disk. They check the title menu
//! and its order, the options that apply at once and are kept, the
//! monochrome and `NO_COLOR` modes, `--ascii` over the saved option, an
//! options file that cannot be read, the seed entry with valid and invalid
//! seeds, the confirmation before a saved game is abandoned, the key help in
//! 80 columns on every screen, and the README, whose keys, options, paths
//! and files are checked against the program. The release files are only
//! checked for static consistency (targets, trigger, installers): that a tag
//! really produces the three binaries is proved in phase 6, on GitHub.
//!
//! Written independently of the unit tests of `app`, `ui` and `persist`:
//! screens are read from rendered buffers, colours from the cells, and
//! expected seeds and key bindings are written out here instead of being read
//! from the production code. The only place that depends on how `main` builds
//! the application is [`launch_with`].

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use clap::{CommandFactory as _, Parser as _};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Color, Modifier};
use sigilfall_core::action::Action;
use sigilfall_core::content::Content;
use sigilfall_core::game::Game;
use sigilfall_core::geometry::Direction;
use sigilfall_core::rng::Seed;

use crate::app::{App, State};
use crate::cli::Cli;
use crate::clock::ManualClock;
use crate::input::{self, Command};
use crate::persist::{DATA_DIR_VAR, LoadedOptions, OptionsFile, ScratchFolder};
use crate::theme::{Appearance, ColorChoice, ColorSupport, GlyphSet, Options, Settings};
use crate::ui::options::OptionRow;
use crate::ui::title::TitleItem;

/// The two sizes screens are compared at: the smallest the game supports
/// and a roomy one.
pub(crate) const SIZES: [(u16, u16); 2] = [(80, 24), (120, 40)];

/// The game's own content.
pub(crate) fn shipped() -> Content {
    Content::load().expect("the shipped content is valid")
}

/// The command line `sigilfall` followed by `arguments`.
pub(crate) fn command_line(arguments: &[&str]) -> Cli {
    Cli::try_parse_from(std::iter::once("sigilfall").chain(arguments.iter().copied()))
        .expect("valid arguments")
}

/// Launches the application as `main` does, on the save file of `folder`,
/// for a terminal detected as `support`, with `arguments` on the command
/// line, reading the date from `clock`. Stops on the title screen when there
/// is one.
pub(crate) fn launch_with(
    folder: &ScratchFolder,
    arguments: &[&str],
    support: ColorSupport,
    clock: &ManualClock,
) -> anyhow::Result<App> {
    App::launch_with_clock(
        support,
        &command_line(arguments),
        &shipped(),
        folder.save_file(),
        Box::new(clock.clone()),
    )
}

/// [`launch_with`] on a true-colour terminal and the default clock.
pub(crate) fn launch(folder: &ScratchFolder, arguments: &[&str]) -> anyhow::Result<App> {
    launch_with(
        folder,
        arguments,
        ColorSupport::TrueColor,
        &ManualClock::new(),
    )
}

/// A key press of `code`.
pub(crate) fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

/// Ctrl+C.
pub(crate) fn ctrl_c() -> KeyEvent {
    KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
}

/// Plays each character of `keys` as a key press.
pub(crate) fn type_text(app: &mut App, keys: &str) {
    for key in keys.chars() {
        app.handle_key(press(KeyCode::Char(key)));
    }
}

/// The buffer of `app` drawn on a `width` × `height` screen.
pub(crate) fn buffer(app: &App, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
    terminal.backend().buffer().clone()
}

/// The text of `app` drawn on a `width` × `height` screen, one string per
/// row, trailing blanks cut.
pub(crate) fn rows(app: &App, width: u16, height: u16) -> Vec<String> {
    let buffer = buffer(app, width, height);
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect()
}

/// The whole text of the 80 × 24 screen.
pub(crate) fn text(app: &App) -> String {
    rows(app, 80, 24).join("\n")
}

/// The index of the first row of `rows` that contains `needle`.
fn row_of(rows: &[String], needle: &str) -> Option<usize> {
    rows.iter().position(|row| row.contains(needle))
}

/// The entry of the title menu that is chosen.
fn chosen(app: &App) -> TitleItem {
    let State::Title(menu) = app.state() else {
        panic!("on the title menu, not {:?}", app.state());
    };
    menu.chosen(app.has_saved_game())
}

/// Moves the title menu onto `item`.
pub(crate) fn go_to(app: &mut App, item: TitleItem) {
    for _ in 0..8 {
        if chosen(app) == item {
            return;
        }
        app.handle_key(press(KeyCode::Down));
    }
    panic!("the title menu has no {item:?}");
}

/// Opens the options screen from the title menu.
fn open_options(app: &mut App) {
    go_to(app, TitleItem::Options);
    app.handle_key(press(KeyCode::Enter));
    assert!(
        matches!(app.state(), State::Options(_)),
        "{:?}",
        app.state()
    );
}

/// Moves the options screen onto `row`.
fn go_to_row(app: &mut App, row: OptionRow) {
    for _ in 0..6 {
        let State::Options(menu) = app.state() else {
            panic!("on the options screen, not {:?}", app.state());
        };
        if menu.chosen() == row {
            return;
        }
        app.handle_key(press(KeyCode::Down));
    }
    panic!("the options screen has no {row:?}");
}

/// Changes the option of `row` once, forward, from the title menu, and goes
/// back to the title.
fn change_option(app: &mut App, row: OptionRow) {
    open_options(app);
    go_to_row(app, row);
    app.handle_key(press(KeyCode::Enter));
    app.handle_key(press(KeyCode::Esc));
}

/// What the cells of a screen are painted with.
#[derive(Debug, Default, PartialEq, Eq)]
struct Paint {
    /// Some cell has a 24-bit colour.
    rgb: bool,
    /// Some cell has a colour of the 256-colour palette.
    indexed: bool,
    /// Some cell has one of the 16 named colours.
    named: bool,
    /// Some cell is bold, reversed or dim.
    emphasis: bool,
}

/// Reads how `buffer` is painted.
fn paint(buffer: &Buffer) -> Paint {
    let mut paint = Paint::default();
    for cell in buffer.content() {
        for color in [cell.fg, cell.bg] {
            match color {
                Color::Rgb(..) => paint.rgb = true,
                Color::Indexed(_) => paint.indexed = true,
                Color::Reset => {}
                _ => paint.named = true,
            }
        }
        if cell
            .modifier
            .intersects(Modifier::BOLD | Modifier::REVERSED | Modifier::DIM)
        {
            paint.emphasis = true;
        }
    }
    paint
}

/// The brightness (0 to 255) of the background of the top-left cell, or
/// `None` when it is not a 24-bit colour.
fn background_brightness(buffer: &Buffer) -> Option<u32> {
    match buffer[(0, 0)].bg {
        Color::Rgb(r, g, b) => {
            Some((u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114) / 1000)
        }
        _ => None,
    }
}

/// A save in `folder` holding a fresh game of `seed` played for `waits`
/// turns, written as the application writes it.
fn save_game(folder: &ScratchFolder, seed: u64, waits: usize) {
    let mut game = Game::new(Seed::from(seed), &shipped());
    for _ in 0..waits {
        game.submit(Action::Wait);
    }
    folder
        .save_file()
        .write(&game)
        .expect("the save is written");
}

/// The names of the files in `folder`, sorted.
fn files(folder: &ScratchFolder) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(folder.path())
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// The options file of `folder`, as text, or an empty string.
fn options_text(folder: &ScratchFolder) -> String {
    fs::read_to_string(folder.options_file().path()).unwrap_or_default()
}

// --- Scenario 1: the title screen offers every entry ------------------------

#[test]
fn the_title_lists_new_game_seed_daily_options_and_quit_in_that_order_without_a_save() {
    let folder = ScratchFolder::new("f10-title-empty");
    let app = launch(&folder, &[]).unwrap();

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
    let folder = ScratchFolder::new("f10-title-save");
    save_game(&folder, 42, 4);
    let app = launch(&folder, &[]).unwrap();

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
    let folder = ScratchFolder::new("f10-title-keys");
    let mut app = launch(&folder, &[]).unwrap();

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

    let mut app = launch(&folder, &[]).unwrap();
    app.handle_key(press(KeyCode::Char('q')));
    assert_eq!(app.state(), State::Quit, "q quits from the title");
    let mut app = launch(&folder, &[]).unwrap();
    app.handle_key(ctrl_c());
    assert_eq!(app.state(), State::Quit, "Ctrl+C quits from the title");
    assert_eq!(
        files(&folder),
        Vec::<String>::new(),
        "quitting the title writes nothing"
    );

    let mut app = launch(&folder, &[]).unwrap();
    app.handle_key(press(KeyCode::Enter));
    assert_eq!(app.state(), State::Playing, "New game starts a game");
}

// --- Scenario 2: the options apply at once and are kept ---------------------

#[test]
fn the_glyph_option_redraws_at_once_and_the_choice_is_kept_for_the_next_session() {
    let folder = ScratchFolder::new("f10-glyphs");
    let mut app = launch(&folder, &[]).unwrap();
    assert_eq!(
        app.options().glyphs,
        GlyphSet::Unicode,
        "Unicode by default"
    );
    assert!(
        !text(&app).is_ascii(),
        "the Unicode title uses Unicode glyphs"
    );

    open_options(&mut app);
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
    let mut next = launch(&folder, &[]).unwrap();
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
    let mut again = launch(&folder, &[]).unwrap();
    open_options(&mut again);
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
    let folder = ScratchFolder::new("f10-colours");
    let mut app = launch(&folder, &[]).unwrap();
    let auto = paint(&buffer(&app, 80, 24));
    assert!(auto.rgb && !auto.named, "true colour by default: {auto:?}");

    open_options(&mut app);
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
    let mut next = launch(&folder, &[]).unwrap();
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
    let folder = ScratchFolder::new("f10-light");
    let mut app = launch(&folder, &[]).unwrap();
    let dark = background_brightness(&buffer(&app, 80, 24)).expect("a 24-bit background");
    assert!(dark < 60, "the dark theme paints ink: {dark}");

    open_options(&mut app);
    go_to_row(&mut app, OptionRow::Theme);
    app.handle_key(press(KeyCode::Enter));
    assert_eq!(app.options().appearance, Appearance::Light);
    let light = background_brightness(&buffer(&app, 80, 24)).expect("a 24-bit background");
    assert!(light > 180, "the light theme paints parchment: {light}");
    assert!(options_text(&folder).contains("theme: Light"));
    app.handle_key(press(KeyCode::Esc));
    let title = background_brightness(&buffer(&app, 80, 24)).unwrap();
    assert!(title > 180, "the title is light too");

    let mut next = launch(&folder, &[]).unwrap();
    assert_eq!(next.options().appearance, Appearance::Light);
    next.handle_key(press(KeyCode::Enter));
    let game_light = background_brightness(&buffer(&next, 80, 24)).unwrap();
    assert!(game_light > 180, "the game is light: {game_light}");

    // README: "light for 24-bit colour terminals". With 16 colours it is
    // drawn without colour instead, and still readable.
    let sixteen = launch_with(&folder, &[], ColorSupport::Ansi16, &ManualClock::new()).unwrap();
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
    };
    let folder = ScratchFolder::new("f10-screens");
    save_game(&folder, 7, 3);
    // The options reach the screens through the file, as a player's do.
    folder.options_file().write(&options).unwrap();
    let fresh = || launch_with(&folder, &[], support, &ManualClock::new()).unwrap();

    let mut screens = vec![("title", fresh())];
    let mut on_options = fresh();
    open_options(&mut on_options);
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
    let folder = ScratchFolder::new("f10-ascii-flag");
    folder.options_file().write(&Options::default()).unwrap();
    let saved = options_text(&folder);
    assert!(saved.contains("glyphs: Unicode"));

    let mut app = launch(&folder, &["--ascii"]).unwrap();
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
    open_options(&mut app);
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
    let plain = launch(&folder, &[]).unwrap();
    assert!(!text(&plain).is_ascii());

    // And an option saved as ASCII needs no flag.
    let other = ScratchFolder::new("f10-ascii-option");
    other
        .options_file()
        .write(&Options {
            glyphs: GlyphSet::Ascii,
            ..Options::default()
        })
        .unwrap();
    let by_option = launch(&other, &[]).unwrap();
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
        let folder = ScratchFolder::new("f10-corrupt-options");
        fs::write(folder.options_file().path(), &bytes).unwrap();

        let mut app = launch(&folder, &[]).unwrap_or_else(|error| panic!("{name}: {error:#}"));
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
            OptionsFile::in_folder(folder.path()).load(),
            LoadedOptions::Found(_)
        ));
        let next = launch(&folder, &[]).unwrap();
        assert!(!text(&next).contains("could not be read"), "{name}");
        assert_eq!(next.options().glyphs, GlyphSet::Ascii, "{name}");
    }
}

#[test]
fn an_options_file_that_is_a_folder_or_cannot_be_written_never_crashes_the_game() {
    let folder = ScratchFolder::new("f10-options-folder");
    fs::create_dir(folder.options_file().path()).unwrap();
    let mut app = launch(&folder, &[]).unwrap();
    assert!(text(&app).contains("options file could not be read"));

    open_options(&mut app);
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
    let folder = ScratchFolder::new("f10-created");
    let nested: PathBuf = folder.path().join("a").join("b");
    let options = OptionsFile::in_folder(&nested);
    assert!(!nested.exists());
    options.write(&Options::default()).unwrap();
    assert!(
        nested.join("options.ron").is_file(),
        "README: created if missing"
    );
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
        let folder = ScratchFolder::new("f10-seed-valid");
        let mut app = launch(&folder, &[]).unwrap();
        submit_seed(&mut app, typed);
        assert_eq!(app.state(), State::Playing, "{typed:?}");
        assert_eq!(app.game().seed(), Seed::from(value), "{typed:?}");
        assert!(
            rows(&app, 80, 24)[0].contains(&Seed::from(value).to_string()),
            "{typed:?}: the status line shows the seed"
        );
    }
    // The seed typed is the seed of --seed.
    let typed_folder = ScratchFolder::new("f10-seed-typed");
    let mut typed = launch(&typed_folder, &[]).unwrap();
    submit_seed(&mut typed, "7F3A-91C2-0B5E-D4A8");
    let flag_folder = ScratchFolder::new("f10-seed-flag");
    let flagged = launch(&flag_folder, &["--seed", "7F3A-91C2-0B5E-D4A8"]).unwrap();
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
        let folder = ScratchFolder::new("f10-seed-invalid");
        let mut app = launch(&folder, &[]).unwrap();
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
    let folder = ScratchFolder::new("f10-seed-edit");
    let mut app = launch(&folder, &[]).unwrap();
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
    let folder = ScratchFolder::new("f10-confirm");
    save_game(&folder, 42, 4);
    let saved = fs::read(folder.save_file().path()).unwrap();
    let mut app = launch(&folder, &[]).unwrap();

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
    let folder = ScratchFolder::new("f10-confirm-others");
    save_game(&folder, 42, 2);
    let saved = fs::read(folder.save_file().path()).unwrap();
    let mut app = launch(&folder, &[]).unwrap();

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

    let mut next = launch(&folder, &[]).unwrap();
    next.handle_key(press(KeyCode::Enter));
    assert_eq!(next.state(), State::Playing, "Continue resumes it");
    assert_eq!(next.game().player_turns(), 2);
}

// --- The key help fits in 80 columns on every screen --------------------------

#[test]
fn the_key_help_of_every_screen_fits_in_80_columns_whole() {
    let folder = ScratchFolder::new("f10-help");
    save_game(&folder, 7, 1);

    let mut screens: Vec<(&str, App, Vec<&str>)> = Vec::new();
    let title = launch(&folder, &[]).unwrap();
    screens.push((
        "title",
        title,
        vec!["↑↓ choose", "Enter confirm", "q quit", "v0."],
    ));
    let mut seed = launch(&folder, &[]).unwrap();
    go_to(&mut seed, TitleItem::Seed);
    seed.handle_key(press(KeyCode::Enter));
    screens.push((
        "seed entry",
        seed,
        vec!["Enter start", "Backspace erase", "Esc back"],
    ));
    let mut confirm = launch(&folder, &[]).unwrap();
    go_to(&mut confirm, TitleItem::NewGame);
    confirm.handle_key(press(KeyCode::Enter));
    screens.push(("confirmation", confirm, vec!["Enter", "Esc"]));
    let mut options = launch(&folder, &[]).unwrap();
    open_options(&mut options);
    screens.push((
        "options",
        options,
        vec!["↑↓ choose", "←→ change", "Esc back"],
    ));

    let mut game = launch(&folder, &[]).unwrap();
    game.handle_key(press(KeyCode::Enter));
    let mut trace = launch(&folder, &[]).unwrap();
    trace.handle_key(press(KeyCode::Enter));
    trace.handle_key(press(KeyCode::Char('t')));
    let mut pack = launch(&folder, &[]).unwrap();
    pack.handle_key(press(KeyCode::Enter));
    pack.handle_key(press(KeyCode::Char('i')));
    screens.push((
        "game",
        game,
        vec![
            "←↑→↓/1-9 move",
            ". wait",
            "t trace",
            "g take",
            "i pack",
            "> descend",
            "q save & quit",
        ],
    ));
    screens.push((
        "trace",
        trace,
        vec![
            "1-9 link a node",
            "Backspace undo",
            "Enter cast",
            "Esc cancel",
        ],
    ));
    screens.push(("inventory", pack, vec!["Esc close"]));

    for (name, app, parts) in &screens {
        let screen = rows(app, 80, 24);
        let help = screen.last().unwrap();
        assert!(help.chars().count() <= 80, "{name}: {help:?}");
        for part in parts {
            assert!(
                help.contains(part),
                "{name}: `{part}` is missing from {help:?}"
            );
        }
    }
    // The ASCII game help is the same words with plain arrows.
    let ascii = ScratchFolder::new("f10-help-ascii");
    let mut app = launch(&ascii, &["--ascii", "--seed", "42"]).unwrap();
    app.handle_key(press(KeyCode::Char('.')));
    let help = rows(&app, 80, 24).pop().unwrap();
    assert!(help.is_ascii() && help.chars().count() <= 80, "{help:?}");
    assert!(
        help.contains("move") && help.contains("save & quit"),
        "{help:?}"
    );

    // Below 80 x 24 the game says so instead of drawing a cut screen.
    for (width, height) in [(79, 24), (80, 23), (60, 15), (1, 1)] {
        let small = rows(&screens[5].1, width, height);
        let joined = small.join(" ");
        assert!(
            joined.contains("Enlarge the terminal to 80") || width < 20,
            "{width}x{height}: {joined:?}"
        );
    }
}

// --- The README tells the truth ----------------------------------------------

/// The README, from the root of the repository.
const README: &str = include_str!("../../../README.md");

/// A key press of `code`.
fn key(code: KeyCode) -> KeyEvent {
    press(code)
}

/// The command bound to `code` on the game screen.
fn on_map(code: KeyCode) -> Option<Command> {
    input::command_for(key(code))
}

#[test]
fn every_key_the_readme_gives_for_the_map_does_what_it_says() {
    let moves: [(KeyCode, Direction); 8 + 4 + 8 + 4] = [
        (KeyCode::Up, Direction::North),
        (KeyCode::Down, Direction::South),
        (KeyCode::Left, Direction::West),
        (KeyCode::Right, Direction::East),
        (KeyCode::Char('h'), Direction::West),
        (KeyCode::Char('j'), Direction::South),
        (KeyCode::Char('k'), Direction::North),
        (KeyCode::Char('l'), Direction::East),
        (KeyCode::Char('y'), Direction::NorthWest),
        (KeyCode::Char('u'), Direction::NorthEast),
        (KeyCode::Char('b'), Direction::SouthWest),
        (KeyCode::Char('n'), Direction::SouthEast),
        (KeyCode::Char('1'), Direction::SouthWest),
        (KeyCode::Char('2'), Direction::South),
        (KeyCode::Char('3'), Direction::SouthEast),
        (KeyCode::Char('4'), Direction::West),
        (KeyCode::Char('6'), Direction::East),
        (KeyCode::Char('7'), Direction::NorthWest),
        (KeyCode::Char('8'), Direction::North),
        (KeyCode::Char('9'), Direction::NorthEast),
        (KeyCode::Home, Direction::NorthWest),
        (KeyCode::PageUp, Direction::NorthEast),
        (KeyCode::End, Direction::SouthWest),
        (KeyCode::PageDown, Direction::SouthEast),
    ];
    for (code, direction) in moves {
        assert_eq!(
            on_map(code),
            Some(Command::Act(Action::Move(direction))),
            "{code:?}"
        );
    }
    for code in [KeyCode::Char('.'), KeyCode::Char('5'), KeyCode::KeypadBegin] {
        assert_eq!(on_map(code), Some(Command::Act(Action::Wait)), "{code:?}");
    }
    assert_eq!(on_map(KeyCode::Char('t')), Some(Command::Trace));
    assert_eq!(
        on_map(KeyCode::Char('g')),
        Some(Command::Act(Action::PickUp))
    );
    assert_eq!(
        on_map(KeyCode::Char(',')),
        Some(Command::Act(Action::PickUp))
    );
    assert_eq!(on_map(KeyCode::Char('i')), Some(Command::Inventory));
    assert_eq!(
        on_map(KeyCode::Char('>')),
        Some(Command::Act(Action::Descend))
    );
    assert_eq!(
        on_map(KeyCode::Char('D')),
        Some(Command::Act(Action::Descend))
    );
    assert_eq!(on_map(KeyCode::Char('q')), Some(Command::Quit));
    assert_eq!(input::command_for(ctrl_c()), Some(Command::Quit));
    assert_eq!(
        on_map(KeyCode::Esc),
        None,
        "Escape never quits and does nothing on the map"
    );
    assert_eq!(
        on_map(KeyCode::Char('d')),
        None,
        "lower-case d is not the stairs"
    );
    assert_eq!(
        on_map(KeyCode::Char('z')),
        None,
        "no key the README does not give"
    );

    // The README lists these keys in its table.
    for fragment in [
        "Arrows",
        "`1`-`9`",
        "`h j k l y u b n`",
        "Home, Page Up, End, Page Down",
        "`.`, `5`",
        "`t`",
        "`g`, `,`",
        "`i`",
        "`>`, `D`",
        "`q`",
        "Ctrl+C",
    ] {
        assert!(README.contains(fragment), "the README names {fragment}");
    }
}

#[test]
fn every_key_the_readme_gives_for_the_other_screens_does_what_it_says() {
    use input::{
        EndCommand, InventoryCommand, MenuCommand, OptionsCommand, SeedCommand, TraceCommand,
    };
    // The trace grid.
    for digit in '1'..='9' {
        assert!(
            matches!(
                input::trace_command_for(key(KeyCode::Char(digit))),
                Some(TraceCommand::Link(_))
            ),
            "{digit}"
        );
    }
    assert_eq!(
        input::trace_command_for(key(KeyCode::Backspace)),
        Some(TraceCommand::Undo)
    );
    assert_eq!(
        input::trace_command_for(key(KeyCode::Enter)),
        Some(TraceCommand::Cast)
    );
    assert_eq!(
        input::trace_command_for(key(KeyCode::Esc)),
        Some(TraceCommand::Cancel)
    );
    assert_eq!(
        input::trace_command_for(key(KeyCode::Char('q'))),
        None,
        "q does not quit the grid"
    );

    // The inventory: Up and Down or k and j, Home and End, digits, Enter or u, d, Escape.
    assert_eq!(
        input::inventory_command_for(key(KeyCode::Up)),
        Some(InventoryCommand::Previous)
    );
    assert_eq!(
        input::inventory_command_for(key(KeyCode::Char('k'))),
        Some(InventoryCommand::Previous)
    );
    assert_eq!(
        input::inventory_command_for(key(KeyCode::Down)),
        Some(InventoryCommand::Next)
    );
    assert_eq!(
        input::inventory_command_for(key(KeyCode::Char('j'))),
        Some(InventoryCommand::Next)
    );
    assert_eq!(
        input::inventory_command_for(key(KeyCode::Home)),
        Some(InventoryCommand::First)
    );
    assert_eq!(
        input::inventory_command_for(key(KeyCode::End)),
        Some(InventoryCommand::Last)
    );
    for (digit, slot) in ('1'..='9').zip(0..9).chain([('0', 9)]) {
        assert_eq!(
            input::inventory_command_for(key(KeyCode::Char(digit))),
            Some(InventoryCommand::Slot(slot)),
            "{digit}"
        );
    }
    assert_eq!(
        input::inventory_command_for(key(KeyCode::Enter)),
        Some(InventoryCommand::Use)
    );
    assert_eq!(
        input::inventory_command_for(key(KeyCode::Char('u'))),
        Some(InventoryCommand::Use)
    );
    assert_eq!(
        input::inventory_command_for(key(KeyCode::Char('d'))),
        Some(InventoryCommand::Drop)
    );
    assert_eq!(
        input::inventory_command_for(key(KeyCode::Esc)),
        Some(InventoryCommand::Close)
    );

    // The title and the confirmation.
    for (code, command) in [
        (KeyCode::Up, MenuCommand::Previous),
        (KeyCode::Char('k'), MenuCommand::Previous),
        (KeyCode::Down, MenuCommand::Next),
        (KeyCode::Char('j'), MenuCommand::Next),
        (KeyCode::Home, MenuCommand::First),
        (KeyCode::End, MenuCommand::Last),
        (KeyCode::Enter, MenuCommand::Confirm),
    ] {
        assert_eq!(
            input::title_command_for(key(code)),
            Some(command),
            "title {code:?}"
        );
        assert_eq!(
            input::confirm_command_for(key(code)),
            Some(command),
            "confirm {code:?}"
        );
    }
    assert_eq!(
        input::title_command_for(key(KeyCode::Char('q'))),
        Some(MenuCommand::Quit)
    );
    assert_eq!(input::title_command_for(key(KeyCode::Esc)), None);
    assert_eq!(
        input::confirm_command_for(key(KeyCode::Esc)),
        Some(MenuCommand::Back)
    );
    assert_eq!(input::confirm_command_for(key(KeyCode::Char('q'))), None);

    // The seed entry.
    assert_eq!(
        input::seed_command_for(key(KeyCode::Backspace)),
        Some(SeedCommand::Erase)
    );
    assert_eq!(
        input::seed_command_for(key(KeyCode::Enter)),
        Some(SeedCommand::Submit)
    );
    assert_eq!(
        input::seed_command_for(key(KeyCode::Esc)),
        Some(SeedCommand::Back)
    );

    // The options screen.
    for (code, command) in [
        (KeyCode::Up, OptionsCommand::Previous),
        (KeyCode::Char('k'), OptionsCommand::Previous),
        (KeyCode::Down, OptionsCommand::Next),
        (KeyCode::Char('j'), OptionsCommand::Next),
        (KeyCode::Left, OptionsCommand::PreviousValue),
        (KeyCode::Char('h'), OptionsCommand::PreviousValue),
        (KeyCode::Right, OptionsCommand::NextValue),
        (KeyCode::Char('l'), OptionsCommand::NextValue),
        (KeyCode::Enter, OptionsCommand::Confirm),
        (KeyCode::Char(' '), OptionsCommand::Confirm),
        (KeyCode::Esc, OptionsCommand::Back),
    ] {
        assert_eq!(
            input::options_command_for(key(code)),
            Some(command),
            "{code:?}"
        );
    }
    assert_eq!(input::options_command_for(key(KeyCode::Char('q'))), None);

    // The end screen.
    assert_eq!(
        input::end_command_for(key(KeyCode::Enter)),
        Some(EndCommand::Title)
    );
    assert_eq!(
        input::end_command_for(key(KeyCode::Char('q'))),
        Some(EndCommand::Quit)
    );
    assert_eq!(input::end_command_for(key(KeyCode::Esc)), None);
    assert_eq!(
        input::end_command_for(ctrl_c()),
        Some(EndCommand::Interrupt)
    );
}

#[test]
fn the_command_line_options_of_the_readme_are_exactly_the_ones_of_the_program() {
    let table = README
        .split("### Command line")
        .nth(1)
        .and_then(|rest| rest.split("### Keys").next())
        .expect("a command line section");
    let documented: BTreeSet<String> = table
        .lines()
        .filter(|line| line.starts_with("| `--"))
        .flat_map(|line| {
            line.split('|')
                .nth(1)
                .unwrap_or("")
                .split(',')
                .filter_map(|cell| {
                    cell.trim()
                        .trim_matches('`')
                        .split_whitespace()
                        .next()
                        .map(str::to_owned)
                })
                .collect::<Vec<_>>()
        })
        .collect();
    let real: BTreeSet<String> = Cli::command()
        .get_arguments()
        .filter_map(|argument| argument.get_long().map(|long| format!("--{long}")))
        .chain(["--help".to_owned(), "--version".to_owned()])
        .collect();
    assert_eq!(documented, real, "the README table and the program agree");

    // Each of them is accepted, and `--help` and `--version` are real.
    assert!(Cli::try_parse_from(["sigilfall", "--seed", "42"]).is_ok());
    assert!(Cli::try_parse_from(["sigilfall", "--daily"]).is_ok());
    assert!(Cli::try_parse_from(["sigilfall", "--ascii"]).is_ok());
    for flag in ["--help", "--version"] {
        let error = Cli::try_parse_from(["sigilfall", flag]).unwrap_err();
        assert_eq!(error.exit_code(), 0, "{flag} prints and exits 0");
    }
    assert!(Cli::try_parse_from(["sigilfall", "--nope"]).is_err());
}

#[test]
fn the_environment_variables_and_files_of_the_readme_are_the_real_ones() {
    assert_eq!(DATA_DIR_VAR, "SIGILFALL_DATA_DIR");
    for variable in ["SIGILFALL_DATA_DIR", "NO_COLOR", "COLORTERM", "WT_SESSION"] {
        assert!(README.contains(variable), "the README names {variable}");
    }

    // The program reads no environment variable the README does not name.
    let mut read = BTreeSet::new();
    let sources = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut pending = vec![sources];
    while let Some(folder) = pending.pop() {
        for entry in fs::read_dir(folder).unwrap().filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs")
                && !path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().contains("acceptance"))
                && !path.components().any(|part| part.as_os_str() == "tests")
            {
                let source = fs::read_to_string(&path).unwrap();
                for call in ["var(\"", "var_os(\""] {
                    for piece in source.split(call).skip(1) {
                        if let Some(name) = piece.split('"').next() {
                            read.insert(name.to_owned());
                        }
                    }
                }
            }
        }
    }
    read.retain(|name| name.chars().all(|c| c.is_ascii_uppercase() || c == '_'));
    for name in &read {
        assert!(
            README.contains(name.as_str()),
            "the program reads {name}, which the README does not name"
        );
    }
    for name in ["NO_COLOR", "COLORTERM", "WT_SESSION"] {
        assert!(
            read.contains(name),
            "{name} is really read, not only documented: {read:?}"
        );
    }

    // The files: save.ron and options.ron beside each other, save.ron.bad.
    let folder = ScratchFolder::new("f10-readme-files");
    assert_eq!(folder.save_file().path().file_name().unwrap(), "save.ron");
    assert_eq!(
        folder.options_file().path().file_name().unwrap(),
        "options.ron"
    );
    assert_eq!(
        folder.save_file().path().parent(),
        folder.options_file().path().parent()
    );
    fs::write(folder.save_file().path(), "not a save").unwrap();
    let app = launch(&folder, &[]).unwrap();
    assert!(!app.has_saved_game());
    assert!(
        folder.path().join("save.ron.bad").is_file(),
        "an unreadable save is renamed save.ron.bad: {:?}",
        files(&folder)
    );
    assert!(README.contains("save.ron.bad") && README.contains("`options.ron`"));
}

#[test]
fn the_data_folder_the_readme_names_for_each_system_is_the_one_the_program_asks_the_system_for() {
    // The program asks the `directories` crate for the data folder of the
    // application "Sigilfall" (read from the source, because the constant is
    // private); that crate's rules give the three paths of the README.
    let persist = include_str!("persist.rs");
    assert!(persist.contains("const APPLICATION: &str = \"Sigilfall\";"));
    assert!(persist.contains("ProjectDirs::from(\"\", \"\", APPLICATION)"));
    for row in [
        "`%APPDATA%\\Sigilfall\\data`",
        "`$XDG_DATA_HOME/sigilfall`",
        "`~/.local/share/sigilfall`",
        "`~/Library/Application Support/Sigilfall`",
    ] {
        assert!(README.contains(row), "the README gives {row}");
    }
    // Nothing is created by asking: only the path is computed.
    if let Some(dirs) = directories::ProjectDirs::from("", "", "Sigilfall") {
        let path = dirs.data_dir().to_path_buf();
        if cfg!(windows) {
            let appdata = std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .expect("APPDATA on Windows");
            assert_eq!(path, appdata.join("Sigilfall").join("data"));
        } else if cfg!(target_os = "macos") {
            assert!(
                path.ends_with("Library/Application Support/Sigilfall"),
                "{path:?}"
            );
        } else {
            assert!(path.ends_with("sigilfall"), "{path:?}");
        }
    }
}

#[test]
fn the_files_the_readme_points_to_exist_and_the_placeholders_are_the_known_ones() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for file in [
        "ARCHITECTURE.md",
        "LICENSE-APACHE",
        "LICENSE-MIT",
        "demo.tape",
        "dist-workspace.toml",
        "rust-toolchain.toml",
        "Cargo.lock",
    ] {
        assert!(root.join(file).is_file(), "{file} exists");
    }
    for link in [
        "(ARCHITECTURE.md)",
        "(LICENSE-APACHE)",
        "(LICENSE-MIT)",
        "(demo.tape)",
    ] {
        assert!(README.contains(link), "the README links {link}");
    }
    // Placeholders to fill in phase 6, and nothing else of that kind.
    assert!(README.contains("github.com/OWNER/sigilfall"));
    assert!(README.contains("assets/demo.gif"));
    assert!(
        !root.join("assets/demo.gif").exists(),
        "assets/demo.gif is still to be recorded by CI (phase 6): when it exists, delete this check"
    );
    let toolchain = fs::read_to_string(root.join("rust-toolchain.toml")).unwrap();
    assert!(
        toolchain.contains("channel = \"stable\""),
        "README: the toolchain is pinned to stable"
    );
    let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert!(
        manifest.contains("rust-version = \"1.88\""),
        "README: minimum supported version 1.88"
    );
    assert!(README.contains("Rust 1.88"));
    // The test the README names exists.
    assert!(README.contains("`app::tests::demo_tape`"));
    assert!(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/app/tests/demo_tape.rs")
            .is_file(),
        "the module the README names exists"
    );
}

// --- Release files: static consistency only -----------------------------------

#[test]
fn the_release_files_agree_on_targets_trigger_installers_and_tool_version() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dist = fs::read_to_string(root.join("dist-workspace.toml")).unwrap();
    let workflow = fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();

    // One binary per system the brief names: Windows, Linux, macOS.
    let targets = [
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "x86_64-unknown-linux-gnu",
        "x86_64-pc-windows-msvc",
    ];
    let listed: BTreeSet<&str> = dist
        .split("targets = [")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .unwrap()
        .split('"')
        .skip(1)
        .step_by(2)
        .collect();
    assert_eq!(listed, targets.into_iter().collect::<BTreeSet<_>>());
    assert!(dist.contains("ci = \"github\""));
    assert!(dist.contains("installers = [\"shell\", \"powershell\"]"));
    assert!(dist.contains("[workspace]") && dist.contains("members = [\"cargo:.\"]"));

    // The workflow runs on a version tag, the version of dist is the one of
    // the configuration, and the jobs the README describes exist.
    let dist_version = dist
        .lines()
        .find_map(|line| line.strip_prefix("cargo-dist-version = \""))
        .and_then(|rest| rest.split('"').next())
        .unwrap();
    assert!(
        workflow.contains(&format!("cargo-dist/releases/download/v{dist_version}/")),
        "the workflow installs dist {dist_version}"
    );
    assert!(workflow.contains("tags:\n      - \"v[0-9]+.[0-9]+.[0-9]+*\""));
    assert!(workflow.contains("contents: write"));
    for job in [
        "plan:",
        "build-local-artifacts:",
        "build-global-artifacts:",
        "host:",
    ] {
        assert!(workflow.contains(&format!("\n  {job}")), "{job}");
    }
    assert!(workflow.contains("gh release create"));
    assert!(
        workflow.contains("artifacts_matrix"),
        "the matrix comes from the plan of dist"
    );
    // The runner overrides name targets of the list, nothing else.
    for line in dist
        .lines()
        .skip_while(|l| !l.starts_with("[dist.github-custom-runners]"))
        .skip(1)
    {
        if let Some((target, _)) = line.split_once(" = ") {
            assert!(targets.contains(&target.trim()), "{target} is a target");
        }
    }
    // A tag `v<version>` matches the glob for the version of the crate.
    let version = fs::read_to_string(root.join("Cargo.toml"))
        .unwrap()
        .lines()
        .find_map(|line| {
            line.strip_prefix("version = \"")
                .map(|rest| rest.trim_end_matches('"').to_owned())
        })
        .unwrap();
    let tag = format!("v{version}");
    let digits: Vec<&str> = tag[1..].split('.').collect();
    assert!(
        tag.starts_with('v')
            && digits.len() == 3
            && digits
                .iter()
                .all(|d| d.chars().next().is_some_and(|c| c.is_ascii_digit())),
        "{tag}"
    );
    // Allow-dirty is declared: dist generate has not been run (phase 6).
    assert!(dist.contains("allow-dirty = [\"ci\"]"));
}

// --- The recorded demo replays ------------------------------------------------

/// One key press per character of each `Type` line of `demo.tape` after the
/// line that starts the game, with `Down`, `Enter`, `Escape` as keys. A
/// second, independent reader of the tape.
fn keys_of_the_tape(tape: &str) -> Vec<KeyEvent> {
    let mut keys = Vec::new();
    let mut started = false;
    let mut typed_line = String::new();
    for line in tape
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let mut parts = line.splitn(2, ' ');
        let word = parts.next().unwrap();
        let rest = parts.next().unwrap_or("").trim();
        let word = word.split('@').next().unwrap();
        let command: Vec<KeyEvent> = match word {
            "Type" => {
                let text = rest.trim_matches('"');
                if !started {
                    typed_line.push_str(text);
                    continue;
                }
                text.chars().map(|c| press(KeyCode::Char(c))).collect()
            }
            "Enter" => {
                if !started {
                    started = typed_line.trim() == "sigilfall";
                    typed_line.clear();
                    continue;
                }
                vec![press(KeyCode::Enter)]
            }
            "Down" if started => vec![press(KeyCode::Down)],
            "Up" if started => vec![press(KeyCode::Up)],
            "Escape" if started => vec![press(KeyCode::Esc)],
            "Backspace" if started => vec![press(KeyCode::Backspace)],
            _ => continue,
        };
        keys.extend(command);
    }
    keys
}

/// What the recorded run shows, played from the title of a fresh folder.
struct DemoRun {
    /// The game at the end (kept before the final `q`).
    depth: u32,
    /// Sigils cast.
    casts: u32,
    /// Creatures slain.
    kills: u32,
    /// Glyphs read.
    glyphs: u32,
    /// The seed of the run.
    seed: Seed,
    /// Whether the last key quit the game.
    quit: bool,
}

/// Replays `keys` from the title screen of a fresh folder.
fn replay(keys: &[KeyEvent]) -> DemoRun {
    let folder = ScratchFolder::new("f10-demo");
    let mut app = launch(&folder, &[]).unwrap();
    let mut last = None;
    for key in keys {
        if matches!(app.state(), State::Quit) {
            break;
        }
        if app.state() == State::Playing || matches!(app.state(), State::Casting(_)) {
            last = Some(app.game().clone());
        }
        app.handle_key(*key);
    }
    let game = last.expect("the run reached a game");
    let stats = game.stats();
    DemoRun {
        depth: game.depth().max(stats.deepest()),
        casts: stats.casts().total(),
        kills: stats.total_kills(),
        glyphs: stats.glyphs_learnt(),
        seed: game.seed(),
        quit: app.state() == State::Quit,
    }
}

#[test]
fn the_recorded_demo_reads_a_glyph_casts_fights_goes_down_and_quits() {
    let tape = include_str!("../../../demo.tape");
    let keys = keys_of_the_tape(tape);
    assert!(keys.len() > 50, "the tape sends many keys: {}", keys.len());

    let run = replay(&keys);
    assert_eq!(run.seed, Seed::from(9940), "the seed typed on the title");
    assert!(run.glyphs >= 1, "a glyph is read");
    assert!(run.casts >= 1, "a sigil is cast");
    assert!(run.kills >= 1, "a creature is slain");
    assert!(run.depth >= 2, "the stairs are taken: depth {}", run.depth);
    assert!(run.quit, "the run ends with q");

    // The checks can fail: take the tape apart and the run no longer
    // satisfies them (no walk to the glyph, no cast, no descent).
    for (needle, what) in [
        ("Type@110ms \"bbbbyybbyyyyykk\"", "the walk to the glyph"),
        ("Type \"t\"", "opening the grid"),
        ("Type \">\"", "going down"),
    ] {
        assert!(tape.contains(needle), "the tape still contains {what}");
        let broken = tape.replace(needle, "Sleep 1s");
        let broken_run = replay(&keys_of_the_tape(&broken));
        let still_whole = broken_run.glyphs >= 1
            && broken_run.casts >= 1
            && broken_run.kills >= 1
            && broken_run.depth >= 2;
        assert!(
            !still_whole,
            "without {what} the run must lose something: {:?}",
            (
                broken_run.glyphs,
                broken_run.casts,
                broken_run.kills,
                broken_run.depth
            )
        );
    }
}

#[test]
fn the_demo_tape_uses_only_commands_vhs_has_and_the_replay_can_play() {
    let tape = include_str!("../../../demo.tape");
    let allowed = [
        "Output",
        "Require",
        "Set",
        "Hide",
        "Show",
        "Type",
        "Enter",
        "Down",
        "Up",
        "Escape",
        "Backspace",
        "Space",
        "Tab",
        "Left",
        "Right",
        "Sleep",
        "Ctrl+C",
    ];
    for line in tape
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let word = line.split([' ', '@']).next().unwrap();
        assert!(
            allowed.contains(&word),
            "`{line}`: {word} is not a command the replay understands"
        );
    }
    assert!(
        tape.contains("Output assets/demo.gif"),
        "the GIF the README shows"
    );
    assert!(tape.contains("Require sigilfall"));
    assert!(
        tape.contains("stty cols 80 rows 24"),
        "80 x 24, the smallest supported size"
    );
    assert!(
        README.contains("![A run of seed 9940"),
        "the README shows the GIF of seed 9940"
    );
}
