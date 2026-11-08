//! The victory screen, driven by simulated keys on scratch folders and an
//! injected clock only: a run of a seed played with keys down to the bottom
//! floor, saved, resumed with "Continue", walked to the seal, which is drawn
//! and named in the side panel, refused off the seal with the words of the
//! bottom floor, then broken; the victory screen shows the counts the game
//! kept, the seed, the command that replays it and a line to share that says
//! "victory"; the save is deleted at once; the end screen's grace period holds
//! as after a death. A daily run's line names its date. The screen fits
//! 80 × 24, centres on 120 × 40, and is drawn with the theme's tokens in the
//! four modes and in plain ASCII.
//!
//! The tests written from the player's side also read the colours and weights
//! the terminal receives: the seal is looked for on the map, in the side panel
//! and in the key help at every step, in the four colour modes and in ASCII;
//! the summary of the victory screen is parsed back from the screen and
//! compared with the statistics of the game, on a run that did something
//! (kills, a glyph, a cast, an item). Their colours are written out from the
//! design, not read from the theme, so a token changed by mistake is caught.
//!
//! The runs use harmless monsters (speed 1, one hit point, no attack) on the
//! shipped floors, items and sigils, so a player that walks straight to the way
//! down of each floor always reaches the seal.

use super::death_summary::{count, screen_rows};
use super::*;
use crate::app::{App, END_SCREEN_GRACE, State};
use crate::cli::Cli;
use crate::clock::ManualClock;
use crate::persist::ScratchFolder;
use crate::theme::{ColorMode, ColorSupport, GlyphSet, Theme};
use crate::ui::title::TitleMenu;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Color, Modifier, Style};
use sigilfall_core::action::Action;
use sigilfall_core::content::Content;
use sigilfall_core::event::Event;
use sigilfall_core::game::{Game, Status};
use sigilfall_core::generation::is_bottom_floor;
use sigilfall_core::geometry::{Direction, Position};
use sigilfall_core::map::{Map, Tile};
use sigilfall_core::path::find_path;
use sigilfall_core::rng::{DailyDate, Seed};
use sigilfall_core::save;
use sigilfall_core::sigil::{Node, Trace};
use std::collections::{HashMap, VecDeque};
use std::fs;

/// A single kind of monster that never gets to hurt: at speed 1 it gains a
/// hundredth of the energy it needs to act, and any blow kills it.
const HARMLESS: &str = r#"[
    Monster(id: "mote", name: "dust mote", family: Vermin, elite: false,
        speed: 1, hp: 1, attack: 0, defense: 0, min_depth: 1),
]"#;

/// The bottom floor, written out here rather than read from the game.
const BOTTOM: u32 = 10;

/// Most keys spent walking to the way down of one floor; a floor is 80 ×
/// 40 cells, so a few hundred steps are always enough.
const MAX_KEYS_PER_FLOOR: usize = 1500;

/// The four rendering modes of `docs/design.md`.
const MODES: [ColorMode; 4] = [
    ColorMode::TrueColorDark,
    ColorMode::TrueColorLight,
    ColorMode::Ansi16,
    ColorMode::Monochrome,
];

/// The harmless monsters on the shipped floors, items and sigils.
fn harmless() -> Content {
    Content::parse(HARMLESS).expect("valid test monsters")
}

/// Launches the application as `main` does on `folder`, with the harmless
/// monsters, the command line `cli`, and the time of `clock`.
fn launch(folder: &ScratchFolder, cli: &Cli, clock: &ManualClock) -> App {
    App::launch_with_clock(
        ColorSupport::TrueColor,
        cli,
        &harmless(),
        folder.save_file(),
        Box::new(clock.clone()),
    )
    .expect("the launch succeeds")
}

/// The command line `sigilfall --seed SEED`, or `sigilfall --daily`.
const fn command_line(daily: bool) -> Cli {
    Cli {
        seed: if daily { None } else { Some(SEED) },
        daily,
        ascii: false,
    }
}

/// The way down of the floor of `game`: its stairs, or on the bottom floor
/// its seal.
fn way_down(game: &Game) -> Position {
    let map = game.map();
    let width = i32::try_from(map.width()).unwrap();
    let height = i32::try_from(map.height()).unwrap();
    (0..height)
        .flat_map(|y| (0..width).map(move |x| Position::new(x, y)))
        .find(|&cell| matches!(map.tile(cell), Some(Tile::StairsDown | Tile::Seal)))
        .expect("every floor has stairs or a seal")
}

/// The steps left from the player of `game` to the way down of its floor,
/// ignoring creatures.
fn path_down(game: &Game) -> Vec<Direction> {
    let here = game.player_actor().position();
    if here == way_down(game) {
        return Vec::new();
    }
    find_path(game.map(), here, way_down(game), |_| false).expect("the way down is reachable")
}

/// Walks the player of `app` with movement keys until it is `steps` steps
/// from the way down of its floor, striking the motes in the way.
fn walk_until(app: &mut App, steps: usize) {
    for _ in 0..MAX_KEYS_PER_FLOOR {
        let path = path_down(app.game());
        if path.len() <= steps {
            return;
        }
        app.handle_key(press(vi_key(path[0])));
        assert_eq!(app.state(), State::Playing, "the walker plays on");
    }
    panic!("the way down was not reached in {MAX_KEYS_PER_FLOOR} keys");
}

/// Plays `app` with keys down to the bottom floor: walks to the stairs of
/// each floor and presses `>`.
fn descend_to_the_bottom(app: &mut App) {
    while app.game().depth() < BOTTOM {
        let depth = app.game().depth();
        walk_until(app, 0);
        app.handle_key(press(KeyCode::Char('>')));
        assert_eq!(app.game().depth(), depth + 1, "the stairs lead down");
    }
    assert!(is_bottom_floor(app.game().depth()));
}

/// A game of the harmless monsters on `seed`, played to the end without
/// the application: down every staircase, then onto the seal, which is
/// broken. Faster than keys, for the tests of the screen's look.
fn won_game(seed: Seed) -> Game {
    let mut game = Game::new(seed, &harmless());
    loop {
        for _ in 0..MAX_KEYS_PER_FLOOR {
            let Some(&step) = path_down(&game).first() else {
                break;
            };
            game.submit(Action::Move(step));
        }
        game.submit(Action::Descend);
        if game.is_over() {
            break;
        }
    }
    assert_eq!(game.status(), &Status::Won);
    game
}

/// The line of `rows` that starts with "Sigilfall " once trimmed: the line
/// to share.
fn share_row(rows: &[String]) -> String {
    rows.iter()
        .map(|row| row.trim())
        .find(|row| row.starts_with("Sigilfall "))
        .unwrap_or_else(|| panic!("a line to share:\n{}", rows.join("\n")))
        .to_owned()
}

/// The text of the last message of the log of `app`.
fn last_log_line(app: &App) -> String {
    app.log()
        .newest_first()
        .next()
        .expect("the log is never empty")
        .text
        .clone()
}

#[test]
fn breaking_the_seal_with_keys_shows_the_victory_screen_and_deletes_the_save() {
    let folder = ScratchFolder::new("victory");
    let clock = ManualClock::new();
    let mut app = launch(&folder, &command_line(false), &clock);
    descend_to_the_bottom(&mut app);

    // Saved on the bottom floor, then resumed with "Continue".
    app.handle_key(press(KeyCode::Char('q')));
    app.save_on_exit().expect("the game is saved");
    assert!(folder.save_file().path().exists());
    let plain = Cli {
        seed: None,
        daily: false,
        ascii: false,
    };
    let mut app = launch(&folder, &plain, &clock);
    assert!(app.has_saved_game());
    app.handle_key(press(KeyCode::Enter));
    assert_eq!(app.state(), State::Playing);
    assert_eq!(app.game().depth(), BOTTOM);

    // One step short: the seal is drawn, named in the panel, and the help
    // says what the key does here. Off the seal, it is refused at no cost.
    walk_until(&mut app, 1);
    let screen = screen_rows(&app, 80, 24).join("\n");
    assert!(screen.contains("│ ◎ seal"), "{screen}");
    assert!(screen.contains("> unseal"), "{screen}");
    assert!(
        !screen.contains("▼"),
        "no stairs on the bottom floor:\n{screen}"
    );
    assert_snapshot!("seal_in_view_80x24", screen);
    let turns = app.game().player_turns();
    app.handle_key(press(KeyCode::Char('>')));
    assert_eq!(app.state(), State::Playing);
    assert_eq!(app.game().player_turns(), turns, "no time passed");
    assert_eq!(
        last_log_line(&app),
        "No stairs lead deeper: stand on the seal to break it."
    );

    // On the seal, the same key breaks it.
    walk_until(&mut app, 0);
    let turns = app.game().player_turns();
    app.handle_key(press(KeyCode::Char('>')));

    assert_eq!(app.state(), State::GameOver);
    assert_eq!(app.game().status(), &Status::Won);
    assert!(
        !folder.save_file().path().exists(),
        "the save is deleted before the end screen is drawn"
    );
    assert_eq!(
        last_log_line(&app),
        "You break the seal. The dungeon falls silent."
    );
    let game = app.game();
    let stats = game.stats();
    assert_eq!(game.player_turns(), turns + 1, "breaking takes a turn");
    let rows = screen_rows(&app, 80, 24);
    let text = rows.join("\n");
    for needle in [
        messages::VICTORY_TITLE,
        messages::victory_line(SEED),
        "You broke the seal at depth 10, on turn ",
        "SEED   7F3A-91C2-0B5E-D4A8",
        "Replay this seed: sigilfall --seed 7F3A-91C2-0B5E-D4A8",
    ] {
        assert!(text.contains(needle), "{needle}:\n{text}");
    }
    assert!(
        text.contains(&messages::victory_outcome(BOTTOM, turns + 1)),
        "{text}"
    );
    assert!(!text.contains(messages::DEATH_TITLE), "{text}");
    assert!(!text.contains("Killed by"), "{text}");
    assert_eq!(count(&rows, messages::DEPTH_REACHED_LABEL).0, "10");
    assert_eq!(
        count(&rows, messages::TURNS_LABEL).0,
        stats.turns().to_string()
    );
    assert_eq!(stats.turns(), turns + 1);
    assert_eq!(
        count(&rows, messages::KILLS_LABEL).0,
        stats.total_kills().to_string()
    );
    assert_eq!(
        count(&rows, messages::GLYPHS_LABEL).0,
        stats.glyphs_learnt().to_string()
    );
    assert_eq!(
        count(&rows, messages::CASTS_LABEL).0,
        stats.casts().total().to_string()
    );
    assert_eq!(
        count(&rows, messages::ITEMS_USED_LABEL).0,
        stats.items_used().to_string()
    );
    // The line to share, written out by hand from the counts.
    let casts = stats.casts().total();
    assert_eq!(
        share_row(&rows),
        format!(
            "Sigilfall seed 7F3A-91C2-0B5E-D4A8 - victory, turn {}, {} slain, {casts} sigil{}",
            turns + 1,
            stats.total_kills(),
            if casts == 1 { "" } else { "s" }
        )
    );
    assert!(
        rows[23].starts_with(" Enter title screen   q quit"),
        "{text}"
    );
    assert_snapshot!("victory_80x24", text);

    // The grace period of the end screen holds, then Enter leaves it for a
    // title screen with nothing to continue.
    app.handle_key(press(KeyCode::Enter));
    app.handle_key(press(KeyCode::Char('q')));
    assert_eq!(app.state(), State::GameOver, "held back by the grace");
    clock.advance(END_SCREEN_GRACE);
    app.handle_key(press(KeyCode::Enter));
    assert_eq!(app.state(), State::Title(TitleMenu::open()));
    assert!(!app.has_saved_game());
    assert!(!folder.save_file().path().exists());
}

#[test]
fn the_line_to_share_of_a_daily_victory_names_the_date_and_the_victory() {
    let folder = ScratchFolder::new("victory-daily");
    let clock = ManualClock::new();
    let mut app = launch(&folder, &command_line(true), &clock);
    let date = DailyDate::new(2026, 11, 8).expect("a real date");
    assert_eq!(app.game().daily_date(), Some(date));
    descend_to_the_bottom(&mut app);
    walk_until(&mut app, 0);
    app.handle_key(press(KeyCode::Char('>')));

    assert_eq!(app.state(), State::GameOver);
    assert!(!folder.save_file().path().exists());
    let stats = app.game().stats();
    let rows = screen_rows(&app, 80, 24);
    let casts = stats.casts().total();
    assert_eq!(
        share_row(&rows),
        format!(
            "Sigilfall daily 2026-11-08 - victory, turn {}, {} slain, {casts} sigil{}",
            stats.turns(),
            stats.total_kills(),
            if casts == 1 { "" } else { "s" }
        )
    );
    // The frame and the replay command keep the seed of the day.
    let text = rows.join("\n");
    assert!(
        text.contains(&format!("SEED   {}", Seed::daily(date))),
        "{text}"
    );
    assert!(text.contains(&messages::replay_hint(Seed::daily(date))));
}

#[test]
fn the_victory_screen_fits_80_by_24_and_centres_on_larger_screens() {
    let app = App::with_game(
        Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode),
        won_game(SEED),
        harmless(),
    );
    assert_eq!(app.state(), State::GameOver);
    let small = screen_rows(&app, 80, 24);
    assert!(small[0].trim().is_empty(), "a margin above the title");
    assert!(small[22].trim().is_empty(), "a margin above the help");

    let large = screen_rows(&app, 120, 40);
    let large_text = large.join("\n");
    let title_row = large
        .iter()
        .position(|row| row.contains(messages::VICTORY_TITLE))
        .unwrap();
    let share_row = large
        .iter()
        .position(|row| row.trim().starts_with("Sigilfall "))
        .unwrap();
    let below = 39 - share_row - 1;
    assert!(
        title_row.abs_diff(below) <= 1,
        "centred down:\n{large_text}"
    );
    let title = &large[title_row];
    let left = title.len() - title.trim_start().len();
    let right = title.len() - title.trim_end().len();
    assert!(left.abs_diff(right) <= 1, "centred across:\n{large_text}");
    assert!(
        large[39].starts_with(" Enter title screen   q quit"),
        "{large_text}"
    );
    assert_snapshot!("victory_120x40", large_text);
}

#[test]
fn the_victory_screen_is_drawn_with_the_theme_tokens_in_every_mode() {
    let game = won_game(SEED);
    for mode in MODES {
        let theme = Theme::new(mode, GlyphSet::Unicode);
        let app = App::with_game(theme.clone(), game.clone(), harmless());
        let terminal = render(&app, 80, 24);
        let buffer = terminal.backend().buffer();
        let find = |needle: &str| {
            (0..24u16)
                .find_map(|y| {
                    let row = row_text(buffer, y);
                    let x = row.find(needle)?;
                    let column = row[..x].chars().count();
                    Some((u16::try_from(column).unwrap(), y))
                })
                .unwrap_or_else(|| panic!("{needle} on screen"))
        };
        let style_of = |needle: &str| {
            let cell = &buffer[find(needle)];
            (cell.fg, cell.modifier)
        };
        let token = |style: Style| (style.fg.unwrap_or(Color::Reset), style.add_modifier);

        assert_eq!(style_of("T H E"), token(theme.text_title), "{mode:?}");
        let line = messages::victory_line(SEED);
        assert_eq!(style_of(line), token(theme.text_muted), "{mode:?}");
        assert_eq!(style_of("You broke"), token(theme.text), "{mode:?}");
        assert_eq!(style_of("7F3A-"), token(theme.text_title), "{mode:?}");
        assert_eq!(style_of("SEED"), token(theme.text_muted), "{mode:?}");
        assert_eq!(style_of("╭"), token(theme.border), "{mode:?}");
        assert_eq!(style_of("Turns"), token(theme.text_muted), "{mode:?}");
        assert_eq!(style_of("Replay"), token(theme.text_muted), "{mode:?}");
        assert_eq!(style_of("Sigilfall seed"), token(theme.text), "{mode:?}");
        if mode == ColorMode::Monochrome {
            for cell in buffer.content() {
                assert_eq!((cell.fg, cell.bg), (Color::Reset, Color::Reset));
            }
            assert!(
                buffer[find("T H E")].modifier.contains(Modifier::BOLD),
                "the title stands out without colour"
            );
        } else {
            assert_eq!(Some(buffer[(0, 0)].bg), theme.bg.bg, "{mode:?}");
        }
    }
}

#[test]
fn with_ascii_glyphs_the_victory_screen_is_plain_ascii_in_every_mode() {
    let game = won_game(SEED);
    for mode in MODES {
        let app = App::with_game(Theme::new(mode, GlyphSet::Ascii), game.clone(), harmless());
        let text = screen_rows(&app, 80, 24).join("\n");

        assert!(text.is_ascii(), "{mode:?}:\n{text}");
        assert!(text.contains(messages::VICTORY_TITLE), "{text}");
        assert!(
            text.contains("|    SEED   7F3A-91C2-0B5E-D4A8    |"),
            "{text}"
        );
    }
}

#[test]
fn a_death_still_opens_the_death_summary_and_not_the_victory_screen() {
    let content = Content::parse(
        r#"[
            Monster(id: "executioner", name: "executioner", family: Acolyte, elite: true,
                speed: 100, hp: 999, attack: 99, defense: 0, min_depth: 1),
        ]"#,
    )
    .expect("a valid test monster");
    let mut game = Game::in_fixed_room(SEED);
    game.spawn_monster(
        PLAYER_START.step(Direction::East),
        content.monster("executioner").unwrap(),
    );
    for _ in 0..10 {
        game.submit(Action::Wait);
    }
    assert!(game.is_over(), "the executioner kills");
    let app = App::with_game(
        Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode),
        game,
        content,
    );
    let text = screen_rows(&app, 80, 24).join("\n");

    assert!(text.contains(messages::DEATH_TITLE), "{text}");
    assert!(!text.contains(messages::VICTORY_TITLE), "{text}");
    assert!(share_row(&screen_rows(&app, 80, 24)).contains(" - depth 1, "));
}

/// Most keys or actions spent walking to the way down of one floor.
const WALK_BUDGET: usize = 1500;

/// The text the log shows when the key is pressed off the seal.
const REFUSAL: &str = "No stairs lead deeper: stand on the seal to break it.";

/// The text the log shows when the seal breaks.
const BROKEN: &str = "You break the seal. The dungeon falls silent.";

/// The text the log shows when the key is pressed off the stairs, above the
/// bottom floor.
const NO_STAIRS: &str = "There are no stairs here.";

/// One kind of monster that never gets to hurt: at speed 1 it gains a
/// hundredth of the energy it needs to act, and one blow kills it.
const HARMLESS_DUST: &str = r#"[
    Monster(id: "dust", name: "dust", family: Vermin, elite: false,
        speed: 1, hp: 1, attack: 0, defense: 0, min_depth: 1),
]"#;

/// The four colour modes, with the colours the seal must be drawn in when
/// it is in view and when it is only remembered, from `docs/design.md`
/// (dark and light truecolor are hexadecimal, the others are named, and the
/// monochrome mode has none).
const LOOKS: [(ColorMode, Option<Color>, Option<Color>); 4] = [
    (
        ColorMode::TrueColorDark,
        Some(Color::Rgb(0xE3, 0xB2, 0x4F)),
        Some(Color::Rgb(0xA8, 0x87, 0x3F)),
    ),
    (
        ColorMode::TrueColorLight,
        Some(Color::Rgb(0x7A, 0x4F, 0x08)),
        Some(Color::Rgb(0x80, 0x60, 0x2A)),
    ),
    (ColorMode::Ansi16, Some(Color::Yellow), Some(Color::Gray)),
    (ColorMode::Monochrome, None, None),
];

/// The harmless monsters on the shipped floors, items and sigils.
fn harmless_dust() -> Content {
    Content::parse(HARMLESS_DUST).expect("valid test content")
}

/// A key press of `code`.
fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

/// Presses the character `key` on `app`.
fn tap(app: &mut App, key: char) {
    app.handle_key(press(KeyCode::Char(key)));
}

/// The vi key that moves one step in `direction`.
const fn vi_char(direction: Direction) -> char {
    match direction {
        Direction::North => 'k',
        Direction::South => 'j',
        Direction::East => 'l',
        Direction::West => 'h',
        Direction::NorthEast => 'u',
        Direction::NorthWest => 'y',
        Direction::SouthEast => 'n',
        Direction::SouthWest => 'b',
    }
}

/// The application drawn on a `width` × `height` terminal.
fn draw(app: &App, width: u16, height: u16) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
    terminal
}

/// The text of the screen, one string per row, right blanks trimmed.
fn rows(app: &App, width: u16, height: u16) -> Vec<String> {
    let terminal = draw(app, width, height);
    let buffer = terminal.backend().buffer();
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

/// The first cell of the screen showing `symbol` inside the map area (left
/// of the side panel, under the status line, above the log), as its column,
/// its row and its style.
fn find_on_map(
    app: &App,
    width: u16,
    height: u16,
    symbol: &str,
) -> Option<(u16, u16, Color, Modifier)> {
    let terminal = draw(app, width, height);
    let buffer = terminal.backend().buffer();
    let panel = width - 22;
    let log = height - 7;
    for y in 1..log {
        for x in 0..panel {
            let cell = &buffer[(x, y)];
            if cell.symbol() == symbol {
                return Some((x, y, cell.fg, cell.modifier));
            }
        }
    }
    None
}

/// Every cell of `map`, in reading order.
fn cells(map: &Map) -> Vec<Position> {
    let width = i32::try_from(map.width()).unwrap();
    let height = i32::try_from(map.height()).unwrap();
    (0..height)
        .flat_map(|y| (0..width).map(move |x| Position::new(x, y)))
        .collect()
}

/// The cell of the stairs or of the seal of the floor of `game`.
fn way_down_cell(game: &Game) -> Position {
    let map = game.map();
    cells(map)
        .into_iter()
        .find(|&cell| matches!(map.tile(cell), Some(Tile::StairsDown | Tile::Seal)))
        .expect("every floor has stairs or a seal")
}

/// Steps from every walkable cell to `goal`, by breadth-first search over
/// the eight moves.
fn steps_to(map: &Map, goal: Position) -> HashMap<Position, u32> {
    let mut steps = HashMap::from([(goal, 0)]);
    let mut queue = VecDeque::from([goal]);
    while let Some(cell) = queue.pop_front() {
        let here = steps[&cell];
        for direction in Direction::ALL {
            let next = cell.step(direction);
            if map.is_walkable(next) && !steps.contains_key(&next) {
                steps.insert(next, here + 1);
                queue.push_back(next);
            }
        }
    }
    steps
}

/// The direction of the first step of a shortest walk to the way down of
/// the floor of `game`, or `None` when the player stands on it.
fn step_down(game: &Game) -> Option<Direction> {
    let here = game.player_actor().position();
    let goal = way_down_cell(game);
    if here == goal {
        return None;
    }
    let steps = steps_to(game.map(), goal);
    let mine = steps[&here];
    Direction::ALL.into_iter().find(|&direction| {
        let next = here.step(direction);
        game.map().is_walkable(next) && steps.get(&next).is_some_and(|&s| s + 1 == mine)
    })
}

/// Steps left from the player of `game` to the way down of its floor.
fn steps_left(game: &Game) -> u32 {
    steps_to(game.map(), way_down_cell(game))[&game.player_actor().position()]
}

/// Walks the player of `game` with actions until it is `short` steps from
/// the way down of its floor.
fn walk(game: &mut Game, short: u32) {
    for _ in 0..WALK_BUDGET {
        if steps_left(game) <= short {
            return;
        }
        let direction = step_down(game).expect("not there yet");
        game.submit(Action::Move(direction));
        assert!(!game.is_over());
    }
    panic!("the way down was not reached in {WALK_BUDGET} actions");
}

/// Walks the player of `app` with movement keys until it is `short` steps
/// from the way down of its floor, calling `each` with the application
/// before every key.
fn walk_keys(app: &mut App, short: u32, mut each: impl FnMut(&App)) {
    for _ in 0..WALK_BUDGET {
        each(app);
        if steps_left(app.game()) <= short {
            return;
        }
        let direction = step_down(app.game()).expect("not there yet");
        tap(app, vi_char(direction));
        assert_eq!(app.state(), State::Playing, "the walker plays on");
    }
    panic!("the way down was not reached in {WALK_BUDGET} keys");
}

/// Plays `game` down the stairs of every floor to the bottom one.
fn to_the_bottom(game: &mut Game) {
    while game.depth() < BOTTOM {
        walk(game, 0);
        let below = game.depth() + 1;
        assert_eq!(
            game.submit(Action::Descend),
            [Event::Descended { depth: below }]
        );
    }
}

/// Launches the application as `main` does on `folder`, with the harmless
/// dust of [`HARMLESS_DUST`].
fn launch_with_dust(folder: &ScratchFolder, cli: &Cli, clock: &ManualClock) -> App {
    App::launch_with_clock(
        ColorSupport::TrueColor,
        cli,
        &harmless_dust(),
        folder.save_file(),
        Box::new(clock.clone()),
    )
    .expect("the launch succeeds")
}

/// A harmless game of `seed` on the bottom floor, saved in `folder`.
fn saved_on_the_bottom_floor(folder: &ScratchFolder, seed: u64) -> Game {
    let mut game = Game::new(Seed::new(seed), &harmless_dust());
    to_the_bottom(&mut game);
    folder
        .save_file()
        .write(&game)
        .expect("the save is written");
    game
}

/// The command line `sigilfall`, with no option.
const fn no_options() -> Cli {
    Cli {
        seed: None,
        daily: false,
        ascii: false,
    }
}

/// Launches the application and presses Enter on "Continue".
fn resume(folder: &ScratchFolder, clock: &ManualClock) -> App {
    let mut app = launch_with_dust(folder, &no_options(), clock);
    assert!(app.has_saved_game(), "Continue is offered");
    app.handle_key(press(KeyCode::Enter));
    assert_eq!(app.state(), State::Playing);
    app
}

/// Whether `game` shows the seal in its side panel: the cell is in view.
fn seal_in_view(game: &Game) -> bool {
    game.is_visible(way_down_cell(game))
}

/// The side panel part of every row of an 80 × 24 screen.
fn panel_text(screen: &[String]) -> String {
    screen
        .iter()
        .take(17)
        .map(|row| row.chars().skip(58).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The rows under the status line and above the log, to the left of the
/// panel.
fn map_text(screen: &[String]) -> String {
    screen
        .iter()
        .take(17)
        .skip(1)
        .map(|row| row.chars().take(58).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

// --- Scenarios 1 to 3 by keys, from a save made on the tenth floor ----------------------------

#[test]
fn a_run_saved_on_the_tenth_floor_is_resumed_walked_to_the_seal_and_won() {
    let folder = ScratchFolder::new("f14-keys");
    let clock = ManualClock::new();
    let before = saved_on_the_bottom_floor(&folder, 42);
    let seal = way_down_cell(&before);
    assert_eq!(before.map().tile(seal), Some(Tile::Seal));

    // First launch: a few steps, then "q" saves; the second launch resumes.
    let mut app = resume(&folder, &clock);
    assert_eq!(app.game().depth(), BOTTOM);
    for _ in 0..6 {
        let direction = step_down(app.game()).unwrap();
        tap(&mut app, vi_char(direction));
    }
    tap(&mut app, 'q');
    assert_eq!(app.state(), State::Quit);
    app.save_on_exit().expect("the game is saved");
    let mut app = resume(&folder, &clock);
    assert_eq!(app.game().depth(), BOTTOM);
    assert_eq!(way_down_cell(app.game()), seal, "the seal is where it was");
    assert_eq!(app.game().status(), &Status::Playing);

    // Off the seal the key is refused in the words of the bottom floor and
    // takes no time, here and again after a few steps.
    let turns = app.game().player_turns();
    tap(&mut app, '>');
    assert_eq!(app.state(), State::Playing);
    assert_eq!(app.game().player_turns(), turns, "no time passed");
    assert_eq!(last_log_line(&app), REFUSAL);
    assert!(
        folder.save_file().path().exists(),
        "the game is still saved"
    );

    // On the way, at every step: no stairs, "unseal" in the help, and the
    // seal named in the panel exactly when it is in view, drawn on the map
    // when the camera can show it.
    let mut named = 0;
    let mut drawn = 0;
    walk_keys(&mut app, 0, |app| {
        let screen = rows(app, 80, 24);
        let text = screen.join("\n");
        assert!(
            !text.contains('▼'),
            "no stairs on the bottom floor:\n{text}"
        );
        assert!(!text.contains("stairs down"), "{text}");
        assert!(screen[23].contains("> unseal"), "{}", screen[23]);
        assert!(!text.contains("> descend"), "{text}");
        let game = app.game();
        let on_it = game.player_actor().position() == seal;
        let listed = panel_text(&screen).contains("◎ seal");
        assert_eq!(
            listed,
            seal_in_view(game),
            "panel and view disagree:\n{text}"
        );
        named += usize::from(listed);
        if seal_in_view(game) && !on_it {
            let dy = seal.y - game.player_actor().position().y;
            if (-7..=6).contains(&dy) {
                assert!(map_text(&screen).contains('◎'), "{text}");
                drawn += 1;
            }
        }
    });
    assert!(
        named > 0 && drawn > 0,
        "the seal was never in view ({named}, {drawn})"
    );

    // On the seal, the same key breaks it.
    assert_eq!(app.game().player_actor().position(), seal);
    let turns = app.game().player_turns();
    tap(&mut app, '>');

    assert_eq!(app.state(), State::GameOver);
    assert_eq!(app.game().status(), &Status::Won);
    assert!(
        !folder.save_file().path().exists(),
        "the save is deleted as the seal breaks"
    );
    assert_eq!(last_log_line(&app), BROKEN);
    let game = app.game();
    let stats = game.stats();
    assert_eq!(game.player_turns(), turns + 1);
    let screen = rows(&app, 80, 24);
    let text = screen.join("\n");
    assert!(text.contains("T H E   S E A L   B R E A K S"), "{text}");
    assert!(!text.contains("D E S C E N T"), "{text}");
    assert!(!text.contains("Killed by"), "{text}");
    assert!(
        text.contains(&format!(
            "You broke the seal at depth 10, on turn {}.",
            turns + 1
        )),
        "{text}"
    );
    assert!(text.contains("SEED   0000-0000-0000-002A"), "{text}");
    assert!(
        text.contains("Replay this seed: sigilfall --seed 0000-0000-0000-002A"),
        "{text}"
    );
    assert_eq!(count(&screen, "Depth reached").0, "10");
    assert_eq!(count(&screen, "Turns").0, stats.turns().to_string());
    assert_eq!(
        count(&screen, "Creatures slain").0,
        stats.total_kills().to_string()
    );
    assert_eq!(
        count(&screen, "Glyphs learnt").0,
        stats.glyphs_learnt().to_string()
    );
    assert_eq!(
        count(&screen, "Sigils cast").0,
        stats.casts().total().to_string()
    );
    assert_eq!(
        count(&screen, "Items used").0,
        stats.items_used().to_string()
    );
    let casts = stats.casts().total();
    assert_eq!(
        share_row(&screen),
        format!(
            "Sigilfall seed 0000-0000-0000-002A - victory, turn {}, {} slain, {casts} sigil{}",
            stats.turns(),
            stats.total_kills(),
            if casts == 1 { "" } else { "s" }
        )
    );
    assert!(
        screen[23].starts_with(" Enter title screen   q quit"),
        "{text}"
    );

    // Keys typed during the grace delay are thrown away; Ctrl+C is not.
    app.handle_key(press(KeyCode::Enter));
    tap(&mut app, 'q');
    assert_eq!(app.state(), State::GameOver, "held back by the grace");
    clock.advance(END_SCREEN_GRACE / 2);
    app.handle_key(press(KeyCode::Enter));
    assert_eq!(app.state(), State::GameOver, "half a second is not enough");
    clock.advance(END_SCREEN_GRACE / 2);
    app.handle_key(press(KeyCode::Enter));
    assert_eq!(app.state(), State::Title(TitleMenu::open()));

    // The title no longer offers "Continue", and a new launch does not
    // either.
    assert!(!app.has_saved_game());
    let title = rows(&app, 80, 24).join("\n");
    assert!(!title.contains("Continue"), "{title}");
    assert!(title.contains("New game"), "{title}");
    let again = launch_with_dust(&folder, &no_options(), &clock);
    assert!(!again.has_saved_game());
    assert!(!rows(&again, 80, 24).join("\n").contains("Continue"));
}

#[test]
fn ctrl_c_works_at_once_on_the_victory_screen_and_leaves_nothing_saved() {
    let folder = ScratchFolder::new("f14-ctrl-c");
    let clock = ManualClock::new();
    saved_on_the_bottom_floor(&folder, 7);
    let mut app = resume(&folder, &clock);
    walk_keys(&mut app, 0, |_| {});
    tap(&mut app, '>');
    assert_eq!(app.state(), State::GameOver);

    app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));

    assert_eq!(app.state(), State::Quit);
    assert!(!folder.save_file().path().exists());
}

#[test]
fn a_daily_run_won_at_the_bottom_shares_a_line_that_says_victory_and_names_the_day() {
    let folder = ScratchFolder::new("f14-daily");
    let clock = ManualClock::new();
    let date = DailyDate::new(2026, 11, 8).expect("a real date");
    let mut game = Game::daily(date, &harmless_dust());
    to_the_bottom(&mut game);
    folder
        .save_file()
        .write(&game)
        .expect("the save is written");
    let mut app = resume(&folder, &clock);
    assert!(
        rows(&app, 80, 24)[0].contains("Daily 2026-11-08"),
        "the status line names the day"
    );
    walk_keys(&mut app, 0, |_| {});
    tap(&mut app, '>');

    assert_eq!(app.state(), State::GameOver);
    assert!(!folder.save_file().path().exists());
    let stats = app.game().stats();
    let screen = rows(&app, 80, 24);
    let casts = stats.casts().total();
    assert_eq!(
        share_row(&screen),
        format!(
            "Sigilfall daily 2026-11-08 - victory, turn {}, {} slain, {casts} sigil{}",
            stats.turns(),
            stats.total_kills(),
            if casts == 1 { "" } else { "s" }
        )
    );
    let text = screen.join("\n");
    assert!(
        text.contains(&format!("SEED   {}", Seed::daily(date))),
        "{text}"
    );
}

// --- The summary against the statistics of a run that did something -------------------------------

/// The trace of the shipped sigil `ember`, pressed 7, 5, 3.
fn ember_trace() -> Trace {
    let mut trace = Trace::new();
    for digit in [7, 5, 3] {
        trace.push(Node::new(digit).unwrap()).unwrap();
    }
    trace
}

/// A won game whose run did something of each kind the summary counts: it
/// reads a glyph, drinks a potion, casts, kills along the way, and goes
/// down to the seal.
fn busy_victory(content: &Content) -> Game {
    let mut game = Game::new(Seed::new(9940), content);
    let start = game.player_actor().position();
    let mut free = Direction::ALL
        .into_iter()
        .filter(|&d| game.map().can_step(start, d));
    let toward = free.next().expect("a free neighbour");
    let aside = free.next().expect("a second free neighbour");
    let glyph = start.step(toward);
    let back = Direction::ALL
        .into_iter()
        .find(|&d| glyph.step(d) == start)
        .expect("the way back");
    game.engrave(glyph, content.sigil("ember").expect("a shipped sigil"));
    game.submit(Action::Move(toward));
    game.submit(Action::Move(back));
    game.place_item(
        start,
        content.item("healing_draught").expect("a shipped item"),
    );
    game.submit(Action::PickUp);
    let potion = game.inventory().next().map(|(id, _)| id).expect("a potion");
    game.submit(Action::Use(potion));
    game.submit(Action::Cast {
        trace: ember_trace(),
        direction: Some(Direction::East),
    });
    game.submit(Action::Cast {
        trace: ember_trace(),
        direction: Some(Direction::West),
    });
    let dust = content.monster("dust").expect("the test monster");
    game.spawn_monster(start.step(aside), dust);
    for _ in 0..4 {
        game.submit(Action::Move(aside));
    }
    to_the_bottom(&mut game);
    walk(&mut game, 0);
    game.submit(Action::Descend);
    assert_eq!(game.status(), &Status::Won);
    game
}

#[test]
fn the_victory_screen_summary_equals_the_statistics_of_the_game() {
    let content = harmless_dust();
    let game = busy_victory(&content);
    let stats = game.stats().clone();
    assert!(stats.total_kills() >= 1, "the run killed something");
    assert!(stats.glyphs_learnt() >= 1, "the run read a glyph");
    assert!(stats.items_used() >= 1, "the run used an item");
    assert!(stats.casts().total() >= 1, "the run cast a sigil");
    let turns = game.player_turns();
    let app = App::with_game(
        Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode),
        game,
        content,
    );
    assert_eq!(app.state(), State::GameOver);

    for (width, height) in [(80, 24), (120, 40)] {
        let screen = rows(&app, width, height);
        let text = screen.join("\n");
        assert_eq!(count(&screen, "Depth reached").0, "10", "{text}");
        assert_eq!(
            count(&screen, "Turns").0,
            stats.turns().to_string(),
            "{text}"
        );
        assert_eq!(stats.turns(), turns);
        let (kills, detail) = count(&screen, "Creatures slain");
        assert_eq!(kills, stats.total_kills().to_string(), "{text}");
        assert_eq!(detail, format!("{} dust", stats.total_kills()), "{text}");
        assert_eq!(
            count(&screen, "Glyphs learnt").0,
            stats.glyphs_learnt().to_string()
        );
        let (cast, cast_detail) = count(&screen, "Sigils cast");
        assert_eq!(cast, stats.casts().total().to_string(), "{text}");
        let c = stats.casts();
        assert_eq!(
            cast_detail,
            format!(
                "pure {} · unstable {} · fizzled {}",
                c.pure, c.unstable, c.fizzled
            ),
            "{text}"
        );
        assert_eq!(
            count(&screen, "Items used").0,
            stats.items_used().to_string()
        );
        let casts = stats.casts().total();
        assert_eq!(
            share_row(&screen),
            format!(
                "Sigilfall seed {} - victory, turn {turns}, {} slain, {casts} sigil{}",
                Seed::new(9940),
                stats.total_kills(),
                if casts == 1 { "" } else { "s" }
            ),
            "{text}"
        );
    }
}

// --- How the seal looks -------------------------------------------------------------------------------------

/// A game on the bottom floor with its player `short` steps from the seal,
/// the application drawing it with `mode` and `glyphs`.
fn app_beside_the_seal(mode: ColorMode, glyphs: GlyphSet, short: u32) -> App {
    let content = harmless_dust();
    let mut game = Game::new(Seed::new(42), &content);
    to_the_bottom(&mut game);
    walk(&mut game, short);
    App::with_game(Theme::new(mode, glyphs), game, content)
}

/// Walks the player of `game` away from the seal, one best step at a time,
/// until the seal is out of its view; `false` if it never is within the
/// walk budget.
fn walk_out_of_sight(game: &mut Game) -> bool {
    let steps = steps_to(game.map(), way_down_cell(game));
    for _ in 0..WALK_BUDGET {
        if !seal_in_view(game) {
            return true;
        }
        let here = game.player_actor().position();
        let away = Direction::ALL
            .into_iter()
            .filter(|&d| game.map().is_walkable(here.step(d)))
            .max_by_key(|&d| steps[&here.step(d)]);
        match away {
            Some(direction) if steps[&here.step(direction)] > steps[&here] => {
                game.submit(Action::Move(direction));
            }
            _ => return false,
        }
    }
    false
}

#[test]
fn the_seal_is_drawn_in_gold_when_in_view_and_dimmer_when_remembered_in_every_mode() {
    for (mode, visible, remembered) in LOOKS {
        let app = app_beside_the_seal(mode, GlyphSet::Unicode, 1);
        let (_, _, fg, modifier) = find_on_map(&app, 120, 40, "◎").expect("the seal is drawn");
        assert!(
            modifier.contains(Modifier::BOLD),
            "{mode:?}: bold when in view"
        );
        assert!(!modifier.contains(Modifier::DIM), "{mode:?}: never dim");
        if let Some(color) = visible {
            assert_eq!(fg, color, "{mode:?}: colour in view");
        } else {
            assert_eq!(fg, Color::Reset, "{mode:?}: monochrome has no colour");
        }
        let panel = rows(&app, 120, 40).join("\n");
        assert!(panel.contains("◎ seal"), "{mode:?}:\n{panel}");

        // Out of view, the seal stays on the map, in its remembered style.
        let content = harmless_dust();
        let mut game = Game::new(Seed::new(42), &content);
        to_the_bottom(&mut game);
        walk(&mut game, 1);
        assert!(
            walk_out_of_sight(&mut game),
            "{mode:?}: could not leave its sight"
        );
        let app = App::with_game(Theme::new(mode, GlyphSet::Unicode), game, content);
        let (_, _, fg, modifier) =
            find_on_map(&app, 120, 40, "◎").expect("the remembered seal is drawn");
        assert!(
            !modifier.contains(Modifier::BOLD),
            "{mode:?}: not bold when remembered"
        );
        assert!(!modifier.contains(Modifier::DIM), "{mode:?}: never dim");
        if let Some(color) = remembered {
            assert_eq!(fg, color, "{mode:?}: remembered colour");
        } else {
            assert_eq!(fg, Color::Reset);
        }
        let panel = rows(&app, 120, 40).join("\n");
        assert!(
            !panel.contains("◎ seal"),
            "{mode:?}: not listed out of view:\n{panel}"
        );
    }
}

#[test]
fn in_ascii_the_seal_is_a_zero_named_in_the_panel_and_the_victory_screen_is_plain_ascii() {
    for (mode, _, _) in LOOKS {
        let app = app_beside_the_seal(mode, GlyphSet::Ascii, 1);
        let screen = rows(&app, 80, 24);
        let text = screen.join("\n");
        assert!(
            find_on_map(&app, 80, 24, "0").is_some(),
            "{mode:?}:\n{text}"
        );
        assert!(find_on_map(&app, 80, 24, "◎").is_none());
        assert!(panel_text(&screen).contains("0 seal"), "{mode:?}:\n{text}");
        assert!(screen[23].contains("> unseal"), "{mode:?}:\n{text}");

        let content = harmless_dust();
        let mut game = Game::new(Seed::new(42), &content);
        to_the_bottom(&mut game);
        walk(&mut game, 0);
        game.submit(Action::Descend);
        let app = App::with_game(Theme::new(mode, GlyphSet::Ascii), game, content);
        for (width, height) in [(80, 24), (120, 40)] {
            let screen = rows(&app, width, height);
            assert!(
                screen.iter().all(|row| row.is_ascii()),
                "{mode:?}: not plain ASCII at {width}x{height}:\n{}",
                screen.join("\n")
            );
            assert!(screen.join("\n").contains("T H E   S E A L   B R E A K S"));
        }
    }
}

#[test]
fn the_victory_screen_fits_80_by_24_and_centres_on_a_large_terminal() {
    let content = harmless_dust();
    let mut game = Game::new(Seed::new(42), &content);
    to_the_bottom(&mut game);
    walk(&mut game, 0);
    game.submit(Action::Descend);
    for (mode, _, _) in LOOKS {
        let app = App::with_game(
            Theme::new(mode, GlyphSet::Unicode),
            game.clone(),
            content.clone(),
        );
        let small = rows(&app, 80, 24);
        assert!(small.iter().all(|row| row.chars().count() <= 80));
        let title = small
            .iter()
            .position(|r| r.contains("T H E   S E A L"))
            .unwrap();
        assert!(
            title <= 4,
            "the title is near the top at 80x24: row {title}"
        );
        assert!(small[23].contains("Enter title screen"));
        let large = rows(&app, 120, 40);
        let big_title = large
            .iter()
            .position(|r| r.contains("T H E   S E A L"))
            .unwrap();
        assert!(big_title > title, "more margin above on a larger terminal");
        let indent = |rows: &[String], needle: &str| {
            let row = rows.iter().find(|r| r.contains(needle)).unwrap();
            row.chars().take_while(|c| *c == ' ').count()
        };
        assert!(indent(&large, "SEED") > indent(&small, "SEED"));
        let too_small = rows(&app, 79, 23).join("\n");
        assert!(!too_small.contains("SEED"), "{too_small}");
    }
}

// --- Floors above the bottom -------------------------------------------------------------------------------------

#[test]
fn on_floors_one_to_nine_the_stairs_are_named_and_the_key_says_descend() {
    let content = harmless_dust();
    let mut game = Game::new(Seed::new(42), &content);
    for depth in 1..BOTTOM {
        walk(&mut game, 1);
        let app = App::with_game(
            Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode),
            game.clone(),
            content.clone(),
        );
        let screen = rows(&app, 80, 24);
        let text = screen.join("\n");
        assert!(!text.contains('◎'), "depth {depth}:\n{text}");
        assert!(!text.contains("seal"), "depth {depth}:\n{text}");
        assert!(!text.contains("unseal"), "depth {depth}:\n{text}");
        assert!(screen[23].contains("> descend"), "depth {depth}:\n{text}");
        assert!(
            panel_text(&screen).contains("▼ stairs down"),
            "depth {depth}:\n{text}"
        );
        walk(&mut game, 0);
        game.submit(Action::Descend);
    }
}

#[test]
fn off_the_stairs_above_the_bottom_floor_the_key_is_refused_in_the_old_words() {
    let content = harmless_dust();
    let game = Game::new(Seed::new(42), &content);
    let mut app = App::with_game(
        Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode),
        game,
        content,
    );
    let turns = app.game().player_turns();

    tap(&mut app, '>');

    assert_eq!(app.state(), State::Playing);
    assert_eq!(app.game().player_turns(), turns);
    assert_eq!(last_log_line(&app), NO_STAIRS);
}

// --- Saves ---------------------------------------------------------------------------------------------------------

#[test]
fn a_save_of_the_previous_format_is_set_aside_with_a_clear_message_and_a_new_game_starts() {
    let folder = ScratchFolder::new("f14-v3");
    let clock = ManualClock::new();
    let game = Game::new(Seed::new(42), &harmless_dust());
    let current = save::to_string(&game).expect("written");
    let old = current.replacen("format_version: 4", "format_version: 3", 1);
    assert_ne!(old, current);
    fs::write(folder.save_file().path(), old).expect("write the old save");

    let mut app = launch_with_dust(&folder, &no_options(), &clock);

    assert!(!app.has_saved_game(), "an old save cannot be continued");
    let title = rows(&app, 80, 24).join("\n");
    assert!(!title.contains("Continue"), "{title}");
    let flat = title.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        flat.contains("saved game could not be read") && flat.contains("format version 3"),
        "{title}"
    );
    assert!(flat.contains("version 4"), "{title}");
    assert!(
        flat.contains("kept as"),
        "the old file is kept, not deleted:\n{title}"
    );
    assert!(!folder.save_file().path().exists());
    assert!(folder.path().join("save.ron.bad").exists());

    app.handle_key(press(KeyCode::Enter));
    assert_eq!(app.state(), State::Playing, "a new game starts");
    assert_eq!(app.game().depth(), 1);
}

#[test]
fn a_game_saved_by_the_game_on_the_tenth_floor_after_a_descent_reads_back_with_its_seal() {
    let folder = ScratchFolder::new("f14-descent-save");
    let clock = ManualClock::new();
    let content = harmless_dust();
    let mut game = Game::new(Seed::new(42), &content);
    while game.depth() < BOTTOM - 1 {
        walk(&mut game, 0);
        game.submit(Action::Descend);
    }
    walk(&mut game, 1);
    folder
        .save_file()
        .write(&game)
        .expect("the save is written");
    let mut app = resume(&folder, &clock);
    assert_eq!(app.game().depth(), BOTTOM - 1);
    walk_keys(&mut app, 0, |_| {});
    tap(&mut app, '>');
    assert_eq!(app.game().depth(), BOTTOM);
    assert_eq!(
        last_log_line(&app),
        "You descend to depth 10, the last. The seal lies here."
    );
    tap(&mut app, 'q');
    app.save_on_exit().expect("the game is saved");

    let mut app = resume(&folder, &clock);

    assert_eq!(app.game().depth(), BOTTOM);
    assert_eq!(
        app.game().map().tile(way_down_cell(app.game())),
        Some(Tile::Seal)
    );
    walk_keys(&mut app, 0, |_| {});
    tap(&mut app, '>');
    assert_eq!(app.game().status(), &Status::Won);
}
