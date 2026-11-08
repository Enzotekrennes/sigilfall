//! The death summary (T25), driven by simulated keys and read from the
//! rendered screen: after a real death on a generated floor every count
//! shown equals `Game::stats`; the list of creatures slain is capped, most
//! slain first; the seed shown replays the run; the summary fits 80 × 24
//! and centres on larger screens; it is drawn with the theme's tokens in
//! the four rendering modes, and in plain ASCII with the ASCII glyphs.

use clap::Parser as _;
use ratatui::style::{Color, Modifier, Style};
use sigilfall_core::game::Status;
use sigilfall_core::map::Tile;
use sigilfall_core::path::find_path;

use super::*;
use crate::cli::Cli;
use crate::ui::death::MAX_KILL_ROWS;

/// Creatures of known strength: six kinds that die from any blow of the
/// player without hurting it much, and the executioner, whose blow kills
/// the player's 30 hit points at once.
const ARENA_MONSTERS: &str = r#"[
    Monster(id: "moth", name: "wax moth", family: Vermin, elite: false,
        speed: 100, hp: 1, attack: 0, defense: 0, min_depth: 1),
    Monster(id: "mite", name: "dust mite", family: Vermin, elite: false,
        speed: 100, hp: 1, attack: 0, defense: 0, min_depth: 1),
    Monster(id: "gnat", name: "cave gnat", family: Vermin, elite: false,
        speed: 100, hp: 1, attack: 0, defense: 0, min_depth: 1),
    Monster(id: "tick", name: "bone tick", family: Vermin, elite: false,
        speed: 100, hp: 1, attack: 0, defense: 0, min_depth: 1),
    Monster(id: "wisp", name: "ash wisp", family: Spectre, elite: false,
        speed: 100, hp: 1, attack: 0, defense: 0, min_depth: 1),
    Monster(id: "imp", name: "ink imp", family: Homunculus, elite: false,
        speed: 100, hp: 1, attack: 0, defense: 0, min_depth: 1),
    Monster(id: "executioner", name: "executioner", family: Acolyte, elite: true,
        speed: 100, hp: 999, attack: 99, defense: 0, min_depth: 1),
]"#;

/// Seeds tried for a real death on generated floors; the reckless player
/// dies on most of them.
const REAL_SEEDS: [u64; 6] = [0, 3, 42, 99, 1234, 7];

/// Most player turns a real game is played for before the test gives up
/// on its seed.
const MAX_REAL_TURNS: u64 = 3000;

/// The four rendering modes of `docs/design.md`.
const MODES: [ColorMode; 4] = [
    ColorMode::TrueColorDark,
    ColorMode::TrueColorLight,
    ColorMode::Ansi16,
    ColorMode::Monochrome,
];

/// The default theme of Windows Terminal (see [`new_app`]).
fn dark() -> Theme {
    Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode)
}

/// The text of every row of `app` drawn on a `width` × `height` screen.
fn screen_rows(app: &App, width: u16, height: u16) -> Vec<String> {
    let terminal = render(app, width, height);
    (0..height)
        .map(|y| row_text(terminal.backend().buffer(), y))
        .collect()
}

/// The value written after the leader dots of the count `label`, and the
/// detail after it (empty when there is none), as read on the screen.
fn count(rows: &[String], label: &str) -> (String, String) {
    let row = rows
        .iter()
        .find(|row| row.trim_start().starts_with(&format!("{label} .")))
        .unwrap_or_else(|| panic!("a `{label}` row:\n{}", rows.join("\n")));
    let after = row.trim_start()[label.len()..]
        .trim_start()
        .trim_start_matches('.')
        .trim();
    let (value, detail) = after.split_once("   ").unwrap_or((after, ""));
    (value.trim().to_owned(), detail.trim().to_owned())
}

/// The rows under the `Creatures slain` row that continue its detail.
fn continuation_rows(rows: &[String]) -> Vec<String> {
    let first = rows
        .iter()
        .position(|row| row.contains(messages::KILLS_LABEL))
        .expect("a creatures row");
    rows[first + 1..]
        .iter()
        .take_while(|row| !row.contains(messages::GLYPHS_LABEL))
        .map(|row| row.trim().to_owned())
        .collect()
}

/// The fixed-room game of [`SEED`] after the player killed, east of its
/// start, the creatures of `kills` (`(definition, how many)`, in order),
/// then died to the executioner.
fn slain(kills: &[(&str, u32)]) -> App {
    let content = Content::parse(ARENA_MONSTERS).expect("valid test monsters");
    let mut game = Game::in_fixed_room(SEED);
    let east = PLAYER_START.step(Direction::East);
    for &(kind, times) in kills {
        for _ in 0..times {
            let before = game.stats().total_kills();
            game.spawn_monster(east, content.monster(kind).unwrap());
            game.submit(Action::Move(Direction::East));
            assert_eq!(game.stats().total_kills(), before + 1, "{kind} dies");
        }
    }
    game.spawn_monster(east, content.monster("executioner").unwrap());
    for _ in 0..10 {
        if game.is_over() {
            break;
        }
        game.submit(Action::Wait);
    }
    assert!(game.is_over(), "the executioner kills");
    let app = App::with_game(dark(), game, content);
    assert_eq!(app.state(), State::GameOver);
    app
}

/// The key that plays the reckless player's next action in `app`: charge
/// the nearest visible monster, go down the stairs, or walk to them.
fn reckless_key(app: &App) -> KeyCode {
    let game = app.game();
    let map = game.map();
    let me = player_position(app);
    let monsters: Vec<Position> = game
        .actors()
        .filter(|(_, actor)| !actor.is_player())
        .map(|(_, actor)| actor.position())
        .collect();
    let occupied = |cell: Position| monsters.contains(&cell);
    let charge = game
        .visible_monsters()
        .filter_map(|monster| find_path(map, me, monster.position, occupied))
        .min_by_key(Vec::len);
    if let Some(&direction) = charge.as_ref().and_then(|path| path.first()) {
        return vi_key(direction);
    }
    if map.tile(me) == Some(Tile::StairsDown) {
        return KeyCode::Char('>');
    }
    let width = i32::try_from(map.width()).unwrap();
    let height = i32::try_from(map.height()).unwrap();
    (0..height)
        .flat_map(|y| (0..width).map(move |x| Position::new(x, y)))
        .find(|&cell| map.tile(cell) == Some(Tile::StairsDown))
        .and_then(|stairs| find_path(map, me, stairs, |_| false))
        .and_then(|path| path.first().copied())
        .map_or(KeyCode::Char('.'), vi_key)
}

/// Plays a real game of `seed` (generated floors, shipped content) with
/// keys until the player dies: the reckless player, with a trace cast
/// every few turns (ember once its glyph is read, otherwise a fizzle).
/// Returns `None` if the player survives [`MAX_REAL_TURNS`].
fn real_death(seed: Seed) -> Option<App> {
    let mut app = App::new(dark(), seed, &shipped_content());
    let mut last_cast = None;
    while app.game().player_turns() < MAX_REAL_TURNS && app.state() != State::GameOver {
        let turns = app.game().player_turns();
        if turns % 9 == 4 && last_cast != Some(turns) {
            last_cast = Some(turns);
            press_all(
                &mut app,
                &[
                    KeyCode::Char('t'),
                    KeyCode::Char('7'),
                    KeyCode::Char('5'),
                    KeyCode::Char('3'),
                    KeyCode::Enter,
                ],
            );
            // Close a verdict or a refusal, at no cost.
            if matches!(app.state(), State::Casting(_)) {
                app.handle_key(press(KeyCode::Esc));
            }
            if matches!(app.state(), State::Casting(_)) {
                app.handle_key(press(KeyCode::Esc));
            }
            continue;
        }
        app.handle_key(press(reckless_key(&app)));
        if app.game().player_turns() == turns && app.state() == State::Playing {
            app.handle_key(press(KeyCode::Char('.')));
        }
    }
    (app.state() == State::GameOver).then_some(app)
}

#[test]
fn after_a_real_death_every_count_shown_is_the_one_the_game_kept() {
    let mut checked = 0;
    for seed in REAL_SEEDS.map(Seed::new) {
        let Some(app) = real_death(seed) else {
            continue;
        };
        let stats = app.game().stats();
        let rows = screen_rows(&app, 80, 24);
        let text = rows.join("\n");

        assert_eq!(
            count(&rows, messages::DEPTH_REACHED_LABEL).0,
            stats.deepest().to_string(),
            "{text}"
        );
        assert_eq!(stats.deepest(), app.game().depth());
        assert_eq!(
            count(&rows, messages::TURNS_LABEL).0,
            stats.turns().to_string()
        );
        let (kills, first_kind) = count(&rows, messages::KILLS_LABEL);
        assert_eq!(kills, stats.total_kills().to_string(), "{text}");
        // The first kind listed is one of the most slain, with its count.
        let most = stats.kills().map(|(_, count)| count).max().unwrap_or(0);
        assert!(most > 0, "seed {seed}: the reckless player killed");
        let (shown_count, shown_name) = first_kind.split_once(' ').expect("count and name");
        assert_eq!(shown_count, most.to_string(), "{text}");
        assert!(
            stats.kills().any(|(id, count)| count == most
                && messages::monster_name(&shipped_content(), id) == shown_name),
            "the most slain kind comes first: {first_kind}\n{text}"
        );
        assert_eq!(
            count(&rows, messages::GLYPHS_LABEL).0,
            stats.glyphs_learnt().to_string()
        );
        let (casts, breakdown) = count(&rows, messages::CASTS_LABEL);
        assert_eq!(casts, stats.casts().total().to_string(), "{text}");
        assert!(stats.casts().total() > 0, "seed {seed}: casts were made");
        assert_eq!(breakdown, messages::cast_breakdown(stats.casts(), '·'));
        assert_eq!(
            count(&rows, messages::ITEMS_USED_LABEL).0,
            stats.items_used().to_string()
        );
        let Status::GameOver { killed_by } = app.game().status() else {
            panic!("the game is over");
        };
        let cause = messages::cause_of_death(
            killed_by,
            app.game().depth(),
            app.game().player_turns(),
            &shipped_content(),
        );
        assert!(text.contains(&cause), "{cause}\n{text}");
        assert!(text.contains(&format!("SEED   {seed}")), "{text}");
        checked += 1;
    }
    assert!(checked >= 2, "only {checked} real deaths");
}

#[test]
fn the_creatures_slain_are_listed_most_first_and_capped() {
    let app = slain(&[
        ("moth", 2),
        ("mite", 1),
        ("gnat", 3),
        ("tick", 1),
        ("wisp", 4),
        ("imp", 1),
    ]);
    let rows = screen_rows(&app, 80, 24);

    let (total, first) = count(&rows, messages::KILLS_LABEL);
    assert_eq!(total, "12");
    assert_eq!(first, "4 ash wisp");
    assert_eq!(
        continuation_rows(&rows),
        ["3 cave gnat", "2 wax moth", "and 3 more"]
    );
    assert_eq!(1 + continuation_rows(&rows).len(), MAX_KILL_ROWS);
    assert_snapshot!(rows.join("\n"));
}

#[test]
fn up_to_four_kinds_are_listed_whole_ties_by_name() {
    let app = slain(&[("moth", 1), ("mite", 1), ("gnat", 12), ("tick", 1)]);
    let rows = screen_rows(&app, 80, 24);

    assert_eq!(count(&rows, messages::KILLS_LABEL).1, "12 cave gnat");
    assert_eq!(
        continuation_rows(&rows),
        [" 1 bone tick", " 1 dust mite", " 1 wax moth"].map(str::trim)
    );
    let text = rows.join("\n");
    assert!(!text.contains("more"), "{text}");
    // The counts are right-aligned: the units line up under "12".
    let column = |row: &str, needle: &str| row.find(needle).unwrap();
    let kills_row = rows.iter().find(|r| r.contains("12 cave gnat")).unwrap();
    let tick_row = rows.iter().find(|r| r.contains("1 bone tick")).unwrap();
    assert_eq!(column(kills_row, "2 cave"), column(tick_row, "1 bone"));
}

#[test]
fn a_run_without_kills_or_casts_shows_zeros_and_no_detail() {
    let app = slain(&[]);
    let rows = screen_rows(&app, 80, 24);

    assert_eq!(
        count(&rows, messages::KILLS_LABEL),
        ("0".to_owned(), String::new())
    );
    assert!(continuation_rows(&rows).is_empty());
    assert_eq!(
        count(&rows, messages::CASTS_LABEL),
        ("0".to_owned(), String::new())
    );
    assert_eq!(count(&rows, messages::DEPTH_REACHED_LABEL).0, "1");
}

#[test]
fn the_seed_shown_is_the_run_seed_and_replays_it() {
    let app = slain(&[("moth", 1)]);
    let rows = screen_rows(&app, 80, 24);
    let text = rows.join("\n");

    let framed = rows
        .iter()
        .find_map(|row| row.split_once("SEED"))
        .map(|(_, rest)| rest.trim().trim_end_matches('│').trim().to_owned())
        .expect("the framed seed");
    let command = text
        .lines()
        .find_map(|row| row.trim().strip_prefix("Replay this seed: "))
        .expect("the replay command");
    assert_eq!(framed, "7F3A-91C2-0B5E-D4A8");
    assert_eq!(command, format!("sigilfall --seed {framed}"));

    // The command, as typed, starts the same run.
    let arguments: Vec<&str> = command.split(' ').collect();
    let cli = Cli::try_parse_from(arguments).expect("the command parses");
    assert_eq!(cli.seed, Some(app.game().seed()));
    let replay = Game::new(cli.seed.expect("a seed"), &shipped_content());
    let original = Game::new(SEED, &shipped_content());
    assert_eq!(
        sigilfall_core::save::to_string(&replay).unwrap(),
        sigilfall_core::save::to_string(&original).unwrap(),
        "the same first floor"
    );
    // The seed also picks the epitaph, the same on every replay.
    assert!(text.contains(messages::epitaph(SEED)), "{text}");
}

#[test]
fn the_summary_fits_80_by_24_and_centres_on_larger_screens() {
    let app = slain(&[
        ("moth", 2),
        ("mite", 1),
        ("gnat", 3),
        ("tick", 1),
        ("wisp", 4),
    ]);
    let small = screen_rows(&app, 80, 24);
    let small_text = small.join("\n");
    for needle in [
        messages::DEATH_TITLE,
        "Killed by an executioner at depth 1, on turn ",
        messages::ITEMS_USED_LABEL,
        "SEED   7F3A-91C2-0B5E-D4A8",
        "Replay this seed: sigilfall --seed 7F3A-91C2-0B5E-D4A8",
    ] {
        assert!(small_text.contains(needle), "{needle}:\n{small_text}");
    }
    assert!(
        small[23].starts_with(" Enter title screen   q quit"),
        "{small_text}"
    );
    assert!(small[22].trim().is_empty(), "a margin above the help");
    assert!(small[0].trim().is_empty(), "a margin above the title");

    let large = screen_rows(&app, 120, 40);
    let large_text = large.join("\n");
    let title_row = large
        .iter()
        .position(|row| row.contains(messages::DEATH_TITLE))
        .unwrap();
    let replay_row = large
        .iter()
        .position(|row| row.contains("Replay this seed"))
        .unwrap();
    let above = title_row;
    let below = 39 - replay_row - 1;
    assert!(above.abs_diff(below) <= 1, "centred down:\n{large_text}");
    let title = &large[title_row];
    let left = title.len() - title.trim_start().len();
    let right = title.len() - title.trim_end().len();
    assert!(left.abs_diff(right) <= 1, "centred across:\n{large_text}");
    // The counts stay one block, aligned on their left edge.
    let count_rows = large.iter().filter(|row| row.contains(" ..")).count();
    assert_eq!(count_rows, 6, "{large_text}");
    let starts: Vec<usize> = large
        .iter()
        .filter(|row| row.contains(" .."))
        .map(|row| row.len() - row.trim_start().len())
        .collect();
    assert!(starts.windows(2).all(|w| w[0] == w[1]), "{large_text}");
    assert!(
        large[39].starts_with(" Enter title screen   q quit"),
        "{large_text}"
    );
    assert_snapshot!(large_text);
}

#[test]
fn the_summary_is_drawn_with_the_theme_tokens_in_every_mode() {
    for mode in MODES {
        let theme = Theme::new(mode, GlyphSet::Unicode);
        let mut app = slain(&[("moth", 1)]);
        app.theme = theme.clone();
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
        assert_eq!(style_of("7F3A-"), token(theme.text_title), "{mode:?}");
        assert_eq!(style_of("SEED"), token(theme.text_muted), "{mode:?}");
        assert_eq!(style_of("╭"), token(theme.border), "{mode:?}");
        assert_eq!(style_of("Killed by"), token(theme.text), "{mode:?}");
        assert_eq!(style_of("Turns"), token(theme.text_muted), "{mode:?}");
        assert_eq!(style_of("Replay"), token(theme.text_muted), "{mode:?}");
        if mode == ColorMode::Monochrome {
            for cell in buffer.content() {
                assert_eq!((cell.fg, cell.bg), (Color::Reset, Color::Reset));
            }
            assert!(
                buffer[find("7F3A-")].modifier.contains(Modifier::BOLD),
                "the seed stands out without colour"
            );
        } else {
            assert_eq!(Some(buffer[(0, 0)].bg), theme.bg.bg, "{mode:?}");
        }
    }
}

#[test]
fn with_ascii_glyphs_the_summary_is_plain_ascii_in_every_mode() {
    for mode in MODES {
        let mut app = slain(&[
            ("wisp", 2),
            ("moth", 1),
            ("mite", 1),
            ("gnat", 1),
            ("imp", 1),
        ]);
        app.theme = Theme::new(mode, GlyphSet::Ascii);
        let rows = screen_rows(&app, 80, 24);
        let text = rows.join("\n");

        assert!(text.is_ascii(), "{mode:?}:\n{text}");
        assert!(text.contains("+------"), "the frame in ASCII:\n{text}");
        assert!(
            text.contains("|    SEED   7F3A-91C2-0B5E-D4A8    |"),
            "{text}"
        );
        if mode == ColorMode::Monochrome {
            assert_snapshot!(text);
        }
    }
}
