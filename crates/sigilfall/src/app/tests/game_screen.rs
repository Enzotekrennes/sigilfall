//! The game screen and moving around the dungeon, driven by keys like a player
//! and read back from the rendered screen and the game clock: the fixed room
//! and a generated floor drawn around the player, Unicode and ASCII glyphs, the
//! movement keys (arrows, keypad, vi keys) and waiting, walls and the pillar
//! that refuse a move at no cost, key releases and repeats, the turn counter
//! and the seed on the status line, quitting, the resize message below 80 × 24
//! and the camera on larger screens, and the terminal restored after a panic.

use super::*;
use ratatui::crossterm::event::KeyEventState;
use sigilfall_core::map::FIXED_ROOM_PILLAR;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[test]
fn new_game_shows_the_room_and_the_player() {
    let app = new_app();

    assert_eq!(app.state(), State::Playing);
    assert_snapshot!(render(&app, 80, 24).backend());
}

#[test]
fn ascii_glyphs_replace_every_unicode_glyph() {
    let app = fixed_room_app(Theme::new(ColorMode::TrueColorDark, GlyphSet::Ascii), SEED);

    let text = render(&app, 80, 24).backend().to_string();

    assert!(text.is_ascii(), "{text}");
    assert_snapshot!(text);
}

#[test]
fn movement_keys_move_the_player_one_cell_each() {
    let mut app = new_app();
    let ticks = app.game().ticks();

    // Arrow, keypad digit, vi key, then a keypad diagonal without Num Lock.
    press_all(
        &mut app,
        &[
            KeyCode::Right,
            KeyCode::Char('2'),
            KeyCode::Char('l'),
            KeyCode::PageDown,
        ],
    );

    let expected = PLAYER_START
        .step(Direction::East)
        .step(Direction::South)
        .step(Direction::East)
        .step(Direction::SouthEast);
    assert_eq!(player_position(&app), expected);
    assert_eq!(app.game().ticks(), ticks + 4, "each move took one turn");
    assert_snapshot!(render(&app, 80, 24).backend());
}

#[test]
fn waiting_lets_time_pass_without_moving() {
    let mut app = new_app();
    let ticks = app.game().ticks();

    press_all(&mut app, &[KeyCode::Char('.'), KeyCode::Char('5')]);

    assert_eq!(player_position(&app), PLAYER_START);
    assert_eq!(app.game().ticks(), ticks + 2);
}

#[test]
fn wall_stops_the_player_and_no_time_passes() {
    let mut app = new_app();
    // The start is four cells right of the floor next to the left wall.
    press_all(&mut app, &[KeyCode::Char('h'); 4]);
    let at_wall = Position::new(1, PLAYER_START.y);
    assert_eq!(player_position(&app), at_wall);
    let ticks = app.game().ticks();

    press_all(&mut app, &[KeyCode::Left, KeyCode::Home, KeyCode::End]);

    assert_eq!(player_position(&app), at_wall);
    assert_eq!(app.game().ticks(), ticks, "blocked moves take no time");
    assert_eq!(app.state(), State::Playing);
}

#[test]
fn press_then_release_counts_as_one_move() {
    let mut app = new_app();
    let ticks = app.game().ticks();

    app.handle_key(press(KeyCode::Right));
    app.handle_key(release(KeyCode::Right));

    assert_eq!(player_position(&app), PLAYER_START.step(Direction::East));
    assert_eq!(app.game().ticks(), ticks + 1);
}

#[test]
fn q_quits() {
    let mut app = new_app();

    app.handle_key(press(KeyCode::Char('q')));

    assert_eq!(app.state(), State::Quit);
}

#[test]
fn q_then_later_keys_are_ignored() {
    let mut app = new_app();

    press_all(&mut app, &[KeyCode::Char('q'), KeyCode::Right]);

    assert_eq!(app.state(), State::Quit);
    assert_eq!(player_position(&app), PLAYER_START);
}

#[test]
fn escape_on_the_game_screen_does_nothing_and_passes_no_time() {
    let mut app = new_app();
    let ticks = app.game().ticks();
    let turns = app.game().player_turns();
    let messages = app.log().len();
    let before = render(&app, 80, 24).backend().buffer().clone();

    // Twice, as a player closing an overlay then pressing once more.
    press_all(&mut app, &[KeyCode::Esc, KeyCode::Esc]);

    assert_eq!(app.state(), State::Playing);
    assert_eq!(app.game().ticks(), ticks, "no time passes");
    assert_eq!(app.game().player_turns(), turns);
    assert_eq!(player_position(&app), PLAYER_START);
    assert_eq!(app.log().len(), messages, "nothing is logged");
    assert_eq!(render(&app, 80, 24).backend().buffer(), &before);
    // The game still answers.
    app.handle_key(press(KeyCode::Right));
    assert_eq!(player_position(&app), PLAYER_START.step(Direction::East));
}

#[test]
fn releasing_q_does_not_quit() {
    let mut app = new_app();

    app.handle_key(release(KeyCode::Char('q')));

    assert_eq!(app.state(), State::Playing);
}

#[test]
fn terminal_smaller_than_80_by_24_shows_a_resize_message() {
    let app = new_app();

    assert_snapshot!(render(&app, 60, 20).backend());
    for (width, height) in [(79, 24), (80, 23)] {
        let text = render(&app, width, height).backend().to_string();
        assert!(
            text.contains("Enlarge the terminal"),
            "{width} × {height}:\n{text}"
        );
        assert!(!text.contains('@'), "{width} × {height}:\n{text}");
    }
}

#[test]
fn a_generated_floor_is_shown_around_the_player_at_the_start() {
    let app = generated_floor_app();
    let floor = generation::generate(SEED, FIRST_DEPTH);
    assert_eq!(player_position(&app), floor.start());

    let terminal = render(&app, 80, 24);

    let player = camera(&app, 80, 24).screen_position(floor.start());
    assert_eq!(
        player.map(|cell| terminal.backend().buffer()[cell].symbol()),
        Some("@")
    );
    assert_snapshot!(terminal.backend());
}

#[test]
fn walking_across_the_floor_scrolls_the_view_and_keeps_the_player_in_it() {
    // The stairs are the cell farthest from the start, so walking to
    // them crosses the floor. Stop two steps short so they stay visible.
    let mut app = generated_floor_app();
    let floor = generation::generate(SEED, FIRST_DEPTH);
    let to_stairs = DijkstraMap::new(floor.map(), [floor.stairs()]);
    let start_origin = camera(&app, 80, 24).origin();
    let mut origins = vec![start_origin];

    while to_stairs.distance(player_position(&app)) > Some(2) {
        let before = to_stairs.distance(player_position(&app));
        let direction = to_stairs.downhill(player_position(&app)).unwrap();
        app.handle_key(press(vi_key(direction)));
        // Each step must bring the player closer, which also bounds the
        // loop: a refused move fails here instead of looping forever.
        assert!(
            to_stairs.distance(player_position(&app)) < before,
            "the move {direction:?} was refused"
        );
        let buffer = render(&app, 80, 24).backend().buffer().clone();
        let camera = camera(&app, 80, 24);
        let player = camera.screen_position(player_position(&app));
        assert_eq!(
            player.map(|cell| buffer[cell].symbol()),
            Some("@"),
            "{camera:?}"
        );
        origins.push(camera.origin());
    }

    origins.dedup();
    assert!(origins.len() > 1, "the view scrolled: {origins:?}");
    assert_ne!(camera(&app, 80, 24).origin(), start_origin);
    assert_snapshot!(render(&app, 80, 24).backend());
}

#[test]
fn a_120_by_40_terminal_centres_the_floor_across_and_crops_it_down_the_rows() {
    // The 98 × 32 map area is wider than the 80 × 40 floor but shorter.
    let app = generated_floor_app();

    let terminal = render(&app, 120, 40);

    let origin = camera(&app, 120, 40).origin();
    assert_eq!(origin.x, -9, "9 spare columns on each side");
    assert!((0..=40 - 32).contains(&origin.y), "{origin:?}");
    let first_line = row_text(terminal.backend().buffer(), 0);
    assert!(
        first_line.ends_with(" Seed 7F3A-91C2-0B5E-D4A8 "),
        "{first_line:?}"
    );
    assert_snapshot!(terminal.backend());
}

#[test]
fn resize_message_uses_the_ascii_dimension_sign() {
    let app = fixed_room_app(Theme::new(ColorMode::Ansi16, GlyphSet::Ascii), SEED);

    let text = render(&app, 60, 20).backend().to_string();

    assert!(text.contains("80 x 24 (current: 60 x 20)"), "{text}");
}

#[test]
fn resizing_switches_between_message_and_game_without_panicking() {
    let app = new_app();
    let mut terminal = render(&app, 80, 24);

    for (width, height) in [(40, 10), (1, 1), (79, 30), (80, 24), (120, 40)] {
        terminal.backend_mut().resize(width, height);
        terminal.draw(|frame| app.render(frame)).unwrap();
        let text = terminal.backend().to_string();
        let fits = width >= ui::MIN_WIDTH && height >= ui::MIN_HEIGHT;
        assert_eq!(text.contains('@'), fits, "{width} × {height}:\n{text}");
    }
}

/// Environment variable that turns the child-process tests below
/// into the program that panics; absent in a normal test run.
const CHILD_MODE: &str = "SIGILFALL_TEST_PANIC_CHILD";

/// Value of [`CHILD_MODE`] for the child that installs the
/// production panic hook before panicking.
const WITH_HOOK: &str = "with-hook";

/// Value of [`CHILD_MODE`] for the control child that panics without
/// the hook, to show the checks below can fail.
const WITHOUT_HOOK: &str = "without-hook";

/// Terminal sequence that leaves the alternate screen.
const LEAVE_ALTERNATE_SCREEN: &str = "\x1b[?1049l";

/// Terminal sequence that shows the cursor again.
const SHOW_CURSOR: &str = "\x1b[?25h";

/// The screen cell `(column, row)` where the map's cell `position`
/// is drawn on the 80 × 24 screen of `app`, through the camera.
fn screen_cell_of(app: &App, position: Position) -> (u16, u16) {
    let cell = camera(app, 80, 24)
        .screen_position(position)
        .unwrap_or_else(|| panic!("{position:?} is off screen"));
    (cell.x, cell.y)
}

/// Every cell of the map area of an 80 × 24 `buffer` that shows the
/// player glyph. The side panel's "in view" list also shows `@`, as
/// a legend, so it is left out.
fn player_cells(buffer: &Buffer) -> Vec<(u16, u16)> {
    let area = ui::game::map_area(buffer.area);
    (area.top()..area.bottom())
        .flat_map(|y| (area.left()..area.right()).map(move |x| (x, y)))
        .filter(|&(x, y)| buffer[(x, y)].symbol() == "@")
        .collect()
}

/// The game screen of `app` at 80 × 24.
fn screen(app: &App) -> Buffer {
    render(app, 80, 24).backend().buffer().clone()
}

/// The whole screen of `buffer` as text, rows joined by newlines;
/// unlike the backend's own display it accepts a zero-sized screen.
fn screen_text(buffer: &Buffer) -> String {
    (0..buffer.area.height)
        .map(|y| row_text(buffer, y))
        .collect::<Vec<_>>()
        .join(
            "
",
        )
}

/// The number shown after "Turn" on the status line.
fn turn_shown(app: &App) -> u64 {
    let first_line = row_text(&screen(app), 0);
    first_line
        .trim()
        .split_once("Turn ")
        .and_then(|(_, rest)| rest.split_whitespace().next())
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| panic!("no turn counter on: {first_line:?}"))
}

/// Presses `code` `times` times and checks the player ended where
/// `expected` says, so a route to a test position is verified.
fn walk(app: &mut App, steps: &[(KeyCode, usize)], expected: Position) {
    for &(code, times) in steps {
        press_all(app, &vec![code; times]);
    }
    assert_eq!(player_position(app), expected, "route {steps:?}");
}

/// The key press as a keypad digit with Num Lock on: the character
/// is a digit and the key state says it comes from the keypad.
fn keypad(digit: char) -> KeyEvent {
    KeyEvent::new_with_kind_and_state(
        KeyCode::Char(digit),
        KeyModifiers::NONE,
        KeyEventKind::Press,
        KeyEventState::KEYPAD,
    )
}

#[test]
fn every_movement_key_moves_one_cell_in_its_direction() {
    // Direction, arrow or navigation key, digit row / keypad digit
    // and vi key, as listed in the keymap documentation.
    let bindings = [
        (Direction::North, KeyCode::Up, '8', 'k'),
        (Direction::NorthEast, KeyCode::PageUp, '9', 'u'),
        (Direction::East, KeyCode::Right, '6', 'l'),
        (Direction::SouthEast, KeyCode::PageDown, '3', 'n'),
        (Direction::South, KeyCode::Down, '2', 'j'),
        (Direction::SouthWest, KeyCode::End, '1', 'b'),
        (Direction::West, KeyCode::Left, '4', 'h'),
        (Direction::NorthWest, KeyCode::Home, '7', 'y'),
    ];

    for (direction, navigation, digit, vi) in bindings {
        let events = [
            press(navigation),
            press(KeyCode::Char(digit)),
            keypad(digit),
            press(KeyCode::Char(vi)),
        ];
        for key in events {
            let mut app = new_app();
            let ticks = app.game().ticks();

            app.handle_key(key);

            let target = PLAYER_START.step(direction);
            assert_eq!(player_position(&app), target, "{direction:?} by {key:?}");
            assert_eq!(app.game().ticks(), ticks + 1, "{direction:?} by {key:?}");
            let buffer = screen(&app);
            assert_eq!(
                player_cells(&buffer),
                [screen_cell_of(&app, target)],
                "{direction:?} by {key:?}: one @, one cell from the start"
            );
            assert_eq!(turn_shown(&app), 2, "{direction:?} by {key:?}");
        }
    }
}

#[test]
fn moving_leaves_floor_behind_and_draws_a_single_player() {
    let mut app = new_app();
    let start_cell = screen_cell_of(&app, PLAYER_START);

    app.handle_key(press(KeyCode::Char('l')));

    let buffer = screen(&app);
    assert_eq!(
        buffer[start_cell].symbol(),
        "·",
        "the old cell is floor again"
    );
    assert_eq!(player_cells(&buffer).len(), 1);
}

#[test]
fn a_long_walk_follows_every_diagonal_and_returns_to_the_start() {
    let mut app = new_app();
    let there_and_back = [
        KeyCode::Char('u'),
        KeyCode::Char('u'),
        KeyCode::Char('n'),
        KeyCode::Char('n'),
        KeyCode::Char('b'),
        KeyCode::Char('b'),
        KeyCode::Char('y'),
        KeyCode::Char('y'),
    ];

    press_all(&mut app, &there_and_back);

    assert_eq!(player_position(&app), PLAYER_START);
    assert_eq!(turn_shown(&app), 1 + 8);
}

#[test]
fn the_pillar_blocks_a_move_from_each_of_its_eight_sides() {
    // Start (5, 5); the pillar is at (15, 6). Each route reaches a
    // neighbour of the pillar by moves that are themselves legal.
    let right = KeyCode::Char('l');
    let down = KeyCode::Char('j');
    let cases = [
        (
            vec![(right, 9)],
            Position::new(14, 5),
            'n',
            "north-west side",
        ),
        (vec![(right, 10)], Position::new(15, 5), 'j', "north side"),
        (
            vec![(right, 11)],
            Position::new(16, 5),
            'b',
            "north-east side",
        ),
        (
            vec![(right, 9), (down, 1)],
            Position::new(14, 6),
            'l',
            "west side",
        ),
        (
            vec![(right, 11), (down, 1)],
            Position::new(16, 6),
            'h',
            "east side",
        ),
        (
            vec![(right, 9), (down, 2)],
            Position::new(14, 7),
            'u',
            "south-west side",
        ),
        (
            vec![(right, 9), (down, 2), (right, 1)],
            Position::new(15, 7),
            'k',
            "south side",
        ),
        (
            vec![(right, 11), (down, 2)],
            Position::new(16, 7),
            'y',
            "south-east side",
        ),
    ];
    assert_eq!(FIXED_ROOM_PILLAR, Position::new(15, 6));

    for (route, from, towards_pillar, side) in cases {
        let mut app = new_app();
        walk(&mut app, &route, from);
        let ticks = app.game().ticks();
        let before = screen(&app);

        app.handle_key(press(KeyCode::Char(towards_pillar)));

        assert_eq!(player_position(&app), from, "{side}");
        assert_eq!(app.game().ticks(), ticks, "{side}: no time passes");
        assert_eq!(screen(&app), before, "{side}: the screen is unchanged");
        assert_eq!(turn_shown(&app), app.game().player_turns() + 1, "{side}");
    }
}

#[test]
fn the_outer_walls_block_every_move_that_would_enter_them() {
    // (1, 1) is the top-left floor cell, (28, 10) the bottom-right one.
    let corners = [
        (
            [(KeyCode::Char('y'), 4)],
            Position::new(1, 1),
            "kyuhb",
            "top-left corner",
        ),
        (
            [(KeyCode::Char('n'), 5)],
            Position::new(10, 10),
            "jnb",
            "bottom wall",
        ),
    ];

    for (route, from, blocked_keys, place) in corners {
        let mut app = new_app();
        walk(&mut app, &route, from);
        let ticks = app.game().ticks();
        let before = screen(&app);

        for key in blocked_keys.chars() {
            app.handle_key(press(KeyCode::Char(key)));
        }

        assert_eq!(player_position(&app), from, "{place}");
        assert_eq!(app.game().ticks(), ticks, "{place}: no time passes");
        assert_eq!(screen(&app), before, "{place}: the screen is unchanged");
    }
}

#[test]
fn the_bottom_right_corner_is_enclosed_too() {
    let mut app = new_app();
    walk(
        &mut app,
        &[(KeyCode::Char('n'), 5), (KeyCode::Char('l'), 18)],
        Position::new(28, 10),
    );
    let ticks = app.game().ticks();

    for key in ['j', 'l', 'n', 'u', 'b'] {
        app.handle_key(press(KeyCode::Char(key)));
    }
    // The arrows, keypad digits and navigation keys are refused alike.
    for key in [
        KeyCode::Down,
        KeyCode::Right,
        KeyCode::PageDown,
        KeyCode::End,
    ] {
        app.handle_key(press(key));
    }
    app.handle_key(keypad('3'));

    assert_eq!(player_position(&app), Position::new(28, 10));
    assert_eq!(app.game().ticks(), ticks);
}

#[test]
fn a_refused_move_does_not_prevent_the_next_legal_one() {
    let mut app = new_app();
    walk(&mut app, &[(KeyCode::Char('y'), 4)], Position::new(1, 1));
    let ticks = app.game().ticks();

    press_all(&mut app, &[KeyCode::Char('k'), KeyCode::Char('l')]);

    assert_eq!(player_position(&app), Position::new(2, 1));
    assert_eq!(app.game().ticks(), ticks + 1, "only the legal move counts");
}

#[test]
fn a_game_started_in_a_too_small_terminal_asks_for_more_room() {
    let app = new_app();
    let sizes = [
        (0, 0),
        (1, 1),
        (30, 1),
        (79, 24),
        (80, 23),
        (79, 23),
        (60, 20),
        (300, 10),
        (10, 300),
        (79, 200),
        (200, 23),
    ];

    for (width, height) in sizes {
        let text = screen_text(render(&app, width, height).backend().buffer());

        assert!(!text.contains('@'), "{width} × {height}:\n{text}");
        assert!(!text.contains("Turn"), "{width} × {height}:\n{text}");
        if width >= 30 && height >= 2 {
            assert!(text.contains("Enlarge"), "{width} × {height}:\n{text}");
        }
    }
}

#[test]
fn the_resize_message_gives_the_target_and_the_current_size() {
    let app = new_app();

    let text = render(&app, 79, 24).backend().to_string();

    assert!(
        text.contains("Enlarge the terminal to 80 × 24 (current: 79 × 24)."),
        "{text}"
    );
}

#[test]
fn shrinking_and_enlarging_keeps_the_game_exactly_as_it_was() {
    let mut app = new_app();
    press_all(
        &mut app,
        &[KeyCode::Char('l'), KeyCode::Char('n'), KeyCode::Char('.')],
    );
    let before = screen(&app);
    let position = player_position(&app);
    let ticks = app.game().ticks();
    let mut terminal = render(&app, 80, 24);

    for (width, height) in [(40, 10), (1, 1), (0, 0), (79, 24), (80, 24)] {
        terminal.backend_mut().resize(width, height);
        terminal.draw(|frame| app.render(frame)).unwrap();
    }

    assert_eq!(terminal.backend().buffer(), &before);
    assert_eq!(player_position(&app), position);
    assert_eq!(app.game().ticks(), ticks);
}

#[test]
fn key_releases_and_repeats_never_make_time_pass() {
    let mut codes = vec![
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::Home,
        KeyCode::End,
        KeyCode::PageUp,
        KeyCode::PageDown,
        KeyCode::KeypadBegin,
        KeyCode::Enter,
        KeyCode::Tab,
        KeyCode::Esc,
        KeyCode::Backspace,
        KeyCode::Delete,
        KeyCode::Insert,
    ];
    codes.extend((b' '..=b'~').map(|byte| KeyCode::Char(char::from(byte))));
    codes.extend((1..=12).map(KeyCode::F));
    let modifiers = [
        KeyModifiers::NONE,
        KeyModifiers::SHIFT,
        KeyModifiers::CONTROL,
        KeyModifiers::ALT,
    ];
    let mut app = new_app();
    let ticks = app.game().ticks();
    let before = screen(&app);

    for kind in [KeyEventKind::Release, KeyEventKind::Repeat] {
        for &code in &codes {
            for modifiers in modifiers {
                app.handle_key(KeyEvent::new_with_kind(code, modifiers, kind));
            }
        }
    }

    assert_eq!(app.state(), State::Playing, "no release or repeat quits");
    assert_eq!(app.game().ticks(), ticks);
    assert_eq!(player_position(&app), PLAYER_START);
    assert_eq!(screen(&app), before);
}

#[test]
fn drawing_the_screen_again_and_again_never_makes_time_pass() {
    let app = new_app();
    let ticks = app.game().ticks();

    for round in 0..200_u16 {
        let (width, height) = if round % 3 == 0 {
            (80, 24)
        } else {
            (60 + round % 70, 20)
        };
        drop(render(&app, width, height));
    }

    assert_eq!(app.game().ticks(), ticks);
    assert_eq!(turn_shown(&app), 1);
}

#[test]
fn a_new_game_starts_with_the_clock_at_one_and_counts_player_actions() {
    // The status line shows the turn the player is about to play: "Turn 1"
    // before any action, one more per move or wait, and nothing more for a
    // refused move.
    let mut app = new_app();
    assert_eq!(app.game().player_turns(), 0);
    assert_eq!(turn_shown(&app), 1);

    press_all(&mut app, &[KeyCode::Char('l'), KeyCode::Char('.')]);
    assert_eq!(turn_shown(&app), 3);

    walk(&mut app, &[(KeyCode::Char('y'), 4)], Position::new(2, 1));
    assert_eq!(turn_shown(&app), 7);
    press_all(&mut app, &[KeyCode::Char('k'), KeyCode::Char('y')]);
    assert_eq!(turn_shown(&app), 7, "refused moves are not turns");
}

#[test]
fn the_status_line_shows_the_seed_of_the_run_in_its_shareable_form() {
    let mut app = new_app();
    let first_line = row_text(&screen(&app), 0);

    assert!(
        first_line.ends_with(" Seed 7F3A-91C2-0B5E-D4A8 "),
        "{first_line:?}"
    );
    let shown = first_line.trim_end().rsplit(' ').next().unwrap();
    assert_eq!(shown.parse(), Ok(app.game().seed()), "--seed reads it back");

    press_all(&mut app, &[KeyCode::Char('l'), KeyCode::Char('.')]);
    let first_line = row_text(&screen(&app), 0);
    assert!(
        first_line.starts_with(" Depth 1 · Turn 3 "),
        "{first_line:?}"
    );
    assert!(
        first_line.ends_with(" Seed 7F3A-91C2-0B5E-D4A8 "),
        "the seed does not change while playing: {first_line:?}"
    );
}

#[test]
fn the_seed_stays_at_the_right_end_of_a_wider_terminal() {
    let app = fixed_room_app(
        Theme::new(ColorMode::TrueColorDark, GlyphSet::Ascii),
        Seed::new(42),
    );
    let first_line = row_text(render(&app, 120, 30).backend().buffer(), 0);

    assert_eq!(first_line.chars().count(), 120);
    assert!(
        first_line.ends_with(" Seed 0000-0000-0000-002A "),
        "{first_line:?}"
    );
    assert!(
        first_line.starts_with(" Depth 1 - Turn 1 "),
        "{first_line:?}"
    );
}

#[test]
fn game_keys_do_nothing_below_80_by_24_but_quitting_works() {
    // The resize message hides the game, so nothing is played blind; only the
    // quit keys still act.
    for (width, height) in [(79, 24), (80, 23), (40, 10), (0, 0)] {
        let mut app = new_app();
        drop(render(&app, width, height));
        let turns = app.game().player_turns();
        let ticks = app.game().ticks();

        press_all(
            &mut app,
            &[KeyCode::Char('l'), KeyCode::Right, KeyCode::Char('.')],
        );

        let size = format!("{width} × {height}");
        assert_eq!(player_position(&app), PLAYER_START, "{size}");
        assert_eq!(app.game().player_turns(), turns, "{size}");
        assert_eq!(app.game().ticks(), ticks, "{size}: no time passes");
        assert_eq!(app.state(), State::Playing, "{size}");

        app.handle_key(press(KeyCode::Char('q')));

        assert_eq!(app.state(), State::Quit, "{size}");
    }
}

#[test]
fn game_keys_work_again_once_the_terminal_is_enlarged() {
    let mut app = new_app();
    let mut terminal = render(&app, 79, 24);
    app.handle_key(press(KeyCode::Char('l')));
    assert_eq!(player_position(&app), PLAYER_START);

    terminal.backend_mut().resize(80, 24);
    terminal.draw(|frame| app.render(frame)).unwrap();
    app.handle_key(press(KeyCode::Char('l')));

    assert_eq!(player_position(&app), PLAYER_START.step(Direction::East));
    assert_eq!(turn_shown(&app), 2);
}

/// Path of the file that receives the output of a child process,
/// unique per test run and per child mode.
fn child_output_path(mode: &str) -> PathBuf {
    std::env::temp_dir().join(format!("sigilfall-panic-{mode}-{}.log", std::process::id()))
}

/// Runs this test executable again, restricted to the child test, in
/// `mode`, with standard output and error written to one file so the
/// order of what was written is kept. Returns the exit code and the
/// captured output.
fn run_panicking_child(mode: &str) -> (Option<i32>, String) {
    let path = child_output_path(mode);
    let log = std::fs::File::create(&path).expect("create the child's log file");
    let status = Command::new(std::env::current_exe().expect("path of the test binary"))
        .args([
            "--exact",
            "app::tests::game_screen::panicking_child",
            "--nocapture",
        ])
        .env(CHILD_MODE, mode)
        .env("TERM", "xterm-256color")
        .env("RUST_BACKTRACE", "0")
        .stdin(Stdio::null())
        .stdout(log.try_clone().expect("duplicate the log handle"))
        .stderr(log)
        .status()
        .expect("start the child test process");
    let bytes = std::fs::read(&path).expect("read the child's log file");
    drop(std::fs::remove_file(&path));
    (status.code(), String::from_utf8_lossy(&bytes).into_owned())
}

/// The program the two tests below start: panics, with or without
/// the production panic hook installed first, depending on
/// [`CHILD_MODE`]. It does nothing in a normal test run.
#[test]
fn panicking_child() {
    match std::env::var(CHILD_MODE).as_deref() {
        Ok(WITH_HOOK) => {
            crate::install_panic_hook();
            panic!("deliberate panic of the hook test");
        }
        Ok(WITHOUT_HOOK) => panic!("deliberate panic of the control test"),
        _ => {}
    }
}

#[test]
fn a_panic_restores_the_terminal_before_its_message_is_printed() {
    let (code, output) = run_panicking_child(WITH_HOOK);

    assert_eq!(
        code,
        Some(101),
        "the panic still ends the process:\n{output:?}"
    );
    let message = output
        .find("deliberate panic of the hook test")
        .unwrap_or_else(|| panic!("the panic message is printed:\n{output:?}"));
    let leave = output
        .find(LEAVE_ALTERNATE_SCREEN)
        .unwrap_or_else(|| panic!("the alternate screen is left:\n{output:?}"));
    let cursor = output
        .find(SHOW_CURSOR)
        .unwrap_or_else(|| panic!("the cursor is shown again:\n{output:?}"));
    assert!(
        leave < message,
        "the screen is left before the message:\n{output:?}"
    );
    assert!(
        cursor < message,
        "the cursor is shown before the message:\n{output:?}"
    );
}

#[test]
fn without_the_hook_a_panic_leaves_the_terminal_as_it_was() {
    // Control: the same child without the hook emits no restore
    // sequence, so the test above does detect the hook's work.
    let (code, output) = run_panicking_child(WITHOUT_HOOK);

    assert_eq!(code, Some(101), "{output:?}");
    assert!(
        output.contains("deliberate panic of the control test"),
        "{output:?}"
    );
    assert!(!output.contains(LEAVE_ALTERNATE_SCREEN), "{output:?}");
    assert!(!output.contains(SHOW_CURSOR), "{output:?}");
}
