//! The victory screen (T34, F14), driven by simulated keys on scratch
//! folders and an injected clock only: a run of a seed played with keys
//! down to the bottom floor, saved, resumed with "Continue", walked to the
//! seal, which is drawn and named in the side panel, refused off the seal
//! with the words of the bottom floor, then broken; the victory screen
//! shows the counts the game kept, the seed, the command that replays it
//! and a line to share that says "victory"; the save is deleted at once;
//! the end screen's grace period holds as after a death. A daily run's line
//! names its date. The screen fits 80 × 24, centres on 120 × 40, and is
//! drawn with the theme's tokens in the four modes and in plain ASCII.
//!
//! The runs use harmless monsters (speed 1, one hit point, no attack) on
//! the shipped floors, items and sigils, so a player that walks straight
//! to the way down of each floor always reaches the seal.

use ratatui::style::{Color, Modifier, Style};
use sigilfall_core::game::Status;
use sigilfall_core::generation::is_bottom_floor;
use sigilfall_core::map::Tile;
use sigilfall_core::path::find_path;
use sigilfall_core::rng::DailyDate;

use super::death_summary::{count, screen_rows};
use super::*;
use crate::persist::ScratchFolder;
use crate::theme::ColorSupport;
use crate::ui::title::TitleMenu;

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
