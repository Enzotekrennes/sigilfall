//! Going down the stairs, driven by keys like a player and read back from the
//! rendered screen and the log.
//!
//! The first tests walk the bare floors of [`SEED`] (see
//! [`generated_floor_app`]), where no monster stands in the way. The others
//! check the descent from the player's side: `>` or `E` on the stairs shows a
//! new floor one level deeper, the same one for the same seed; the floor's
//! creatures come from the table of its depth; off the stairs the keys do
//! nothing and the log says so. Beyond them they check what the player keeps
//! (the panel, the inventory, the log), the status line at every depth up to
//! and past 10 and in ASCII, the key help, the keys that must not descend, the
//! small screens, and the replay of a series of keys across floors.
//!
//! In those, expected values are written out (texts, 80-column help line,
//! depth after each descent) instead of being read from the production
//! modules, so that a silent change of a rule or of a text is noticed. The
//! player is walked to the stairs with the vi keys along a shortest path; the
//! creatures of the tests are slow enough never to act. Every loop is bounded.

use super::*;
use crate::app::{App, State};
use crate::theme::{ColorMode, GlyphSet, Theme};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use sigilfall_core::action::Action;
use sigilfall_core::content::Content;
use sigilfall_core::game::Game;
use sigilfall_core::generation;
use sigilfall_core::geometry::{Direction, Position};
use sigilfall_core::map::{Map, Tile};
use sigilfall_core::rng::Seed;
use sigilfall_core::sigil::{Node, Trace};
use std::collections::{HashMap, HashSet, VecDeque};

/// The cell of the stairs down of the floor `app`'s player is on.
fn stairs(app: &App) -> Position {
    let map = app.game().map();
    let width = i32::try_from(map.width()).unwrap();
    let height = i32::try_from(map.height()).unwrap();
    (0..height)
        .flat_map(|y| (0..width).map(move |x| Position::new(x, y)))
        .find(|&cell| map.tile(cell) == Some(Tile::StairsDown))
        .expect("every generated floor has stairs")
}

/// Walks `app`'s player with the vi keys along a shortest path until
/// it is `distance` steps from the stairs of its floor, checking
/// each step brings it closer (which also bounds the loop).
fn walk_near_stairs(app: &mut App, distance: u32) {
    // The distances borrow a map: a copy leaves `app` free to move.
    let map = app.game().map().clone();
    let to_stairs = DijkstraMap::new(&map, [stairs(app)]);
    while to_stairs.distance(player_position(app)) > Some(distance) {
        let before = to_stairs.distance(player_position(app));
        let direction = to_stairs.downhill(player_position(app)).unwrap();
        app.handle_key(press(vi_key(direction)));
        assert!(
            to_stairs.distance(player_position(app)) < before,
            "the move {direction:?} was refused"
        );
    }
}

/// The first row of `app`'s screen, trimmed: "Depth 2 · Turn 40 …".
fn status(app: &App) -> String {
    row_text(render(app, 80, 24).backend().buffer(), 0)
        .trim()
        .to_owned()
}

/// The texts of `app`'s log, oldest first.
fn log_texts(app: &App) -> Vec<String> {
    let mut texts: Vec<_> = app
        .log()
        .newest_first()
        .map(|message| message.text.clone())
        .collect();
    texts.reverse();
    texts
}

/// What `app` measures of the game: depth, ticks, player turns and
/// the player's cell.
fn clock(app: &App) -> (u32, u64, u64, Position) {
    let game = app.game();
    (
        game.depth(),
        game.ticks(),
        game.player_turns(),
        player_position(app),
    )
}

#[test]
fn the_status_line_starts_with_the_depth_then_the_turn() {
    let app = generated_floor_app();

    let line = status(&app);

    assert!(line.starts_with("Depth 1 · Turn 1 "), "{line:?}");
    assert!(line.ends_with("Seed 7F3A-91C2-0B5E-D4A8"), "{line:?}");
}

#[test]
fn greater_than_on_the_stairs_shows_the_next_floor_and_keeps_the_log() {
    let mut app = generated_floor_app();
    walk_near_stairs(&mut app, 1);
    // One step short, the stairs are in view and named in the panel.
    let before = render(&app, 80, 24).backend().to_string();
    assert!(before.contains("▼ stairs down"), "{before}");
    walk_near_stairs(&mut app, 0);
    let turns = app.game().player_turns();
    let history = log_texts(&app);

    app.handle_key(KeyEvent::new(KeyCode::Char('>'), KeyModifiers::SHIFT));

    assert_eq!(app.state(), State::Playing);
    assert_eq!(app.game().depth(), 2);
    assert_eq!(app.game().player_turns(), turns + 1, "a descent is a turn");
    let next = generation::generate(SEED, 2);
    assert_eq!(player_position(&app), next.start());
    let line = status(&app);
    assert!(
        line.starts_with(&format!("Depth 2 · Turn {} ", turns + 2)),
        "{line:?}"
    );
    assert!(line.ends_with("Seed 7F3A-91C2-0B5E-D4A8"), "{line:?}");
    // The log keeps the floor above and adds one line.
    let after = log_texts(&app);
    assert_eq!(after[..history.len()], history[..]);
    assert_eq!(after[history.len()..], ["You descend to depth 2."]);
    // The screen shows the new floor around the player, and only
    // what the player sees from its start: the floor above is
    // forgotten with its map.
    let terminal = render(&app, 80, 24);
    let buffer = terminal.backend().buffer();
    let camera = camera(&app, 80, 24);
    let player = camera.screen_position(next.start()).expect("in view");
    assert_eq!(buffer[player].symbol(), "@");
    for (screen, cell) in camera.cells() {
        if buffer[screen].symbol() != " " {
            assert!(app.game().is_visible(cell), "{cell:?} drawn unseen");
            assert_eq!(app.game().map().tile(cell), next.map().tile(cell));
        }
    }
    assert_snapshot!(terminal.backend());
}

#[test]
fn a_descent_off_the_stairs_is_refused_at_no_cost_and_logged() {
    let mut app = generated_floor_app();
    walk_near_stairs(&mut app, 1);
    let before = clock(&app);
    let history = log_texts(&app);

    for key in [
        press(KeyCode::Char('>')),
        KeyEvent::new(KeyCode::Char('E'), KeyModifiers::SHIFT),
    ] {
        app.handle_key(key);
    }

    assert_eq!(clock(&app), before, "no depth, time or turn spent");
    assert_eq!(app.state(), State::Playing);
    assert!(status(&app).starts_with("Depth 1 · "), "{}", status(&app));
    let after = log_texts(&app);
    assert_eq!(after[..history.len()], history[..]);
    assert_eq!(
        after[history.len()..],
        ["There are no stairs here.", "There are no stairs here."]
    );
}

#[test]
fn lowercase_d_and_less_than_never_descend() {
    let mut app = generated_floor_app();
    walk_near_stairs(&mut app, 0);
    let before = clock(&app);
    let messages = app.log().len();

    press_all(&mut app, &[KeyCode::Char('<')]);

    assert_eq!(clock(&app), before);
    assert_eq!(app.log().len(), messages, "unbound keys log nothing");

    // On QWERTY `d` is the east key of WASD: one step east, at depth 1.
    let (depth, _, _, position) = before;
    press_all(&mut app, &[KeyCode::Char('d')]);
    let (depth_after, _, _, position_after) = clock(&app);
    assert_eq!(depth_after, depth, "d never descends");
    assert_eq!(position_after, position.step(Direction::East));
}

#[test]
fn descending_again_and_again_counts_each_floor() {
    let mut app = generated_floor_app();
    let mut turn = 1;

    for depth in 2..=4 {
        walk_near_stairs(&mut app, 0);
        // Both keys go down: `>` then the alternative, in turn.
        let key = if depth % 2 == 0 {
            KeyEvent::new(KeyCode::Char('>'), KeyModifiers::SHIFT)
        } else {
            KeyEvent::new(KeyCode::Char('E'), KeyModifiers::SHIFT)
        };
        app.handle_key(key);

        assert_eq!(app.game().depth(), depth);
        assert_eq!(
            player_position(&app),
            generation::generate(SEED, depth).start()
        );
        assert!(app.game().player_turns() + 1 > turn, "time went on");
        turn = app.game().player_turns() + 1;
        let line = status(&app);
        assert!(
            line.starts_with(&format!("Depth {depth} · Turn {turn} ")),
            "{line:?}"
        );
        let newest = app.log().newest_first().next().unwrap();
        assert_eq!(newest.text, format!("You descend to depth {depth}."));
    }
    let descents = log_texts(&app)
        .into_iter()
        .filter(|text| text.starts_with("You descend"))
        .count();
    assert_eq!(descents, 3, "the log keeps every floor");
}

#[test]
fn the_ascii_status_line_and_help_stay_plain_ascii() {
    let app = App::with_game(
        Theme::new(ColorMode::Ansi16, GlyphSet::Ascii),
        Game::on_floor(SEED, generation::generate(SEED, FIRST_DEPTH)),
        shipped_content(),
    );
    let buffer = render(&app, 80, 24).backend().buffer().clone();

    let status = row_text(&buffer, 0);
    let help = row_text(&buffer, 23);

    assert!(status.starts_with(" Depth 1 - Turn 1 "), "{status:?}");
    // Eight help entries fit the 80 columns, two columns apart: nothing cut.
    assert_eq!(
        help.trim_end(),
        " wasd move  . wait  t trace  g take  i pack  > descend  q save & quit  x explore"
    );
    assert!(help.is_ascii(), "{help:?}");
}

/// Creatures that never get to act (speed 1) and die in one blow, in four
/// tiers that [`TIER_RULES`] keeps on disjoint bands of floors.
const MOTES: &str = r#"
    Monster(id: "mote_one", name: "mote one", name_fr: Masculine("le monstre"), plural: "mote ones", plural_fr: "monstres", family: Vermin, elite: false,
        speed: 1, hp: 1, attack: 0, defense: 0, min_depth: 1),
    Monster(id: "mote_two", name: "mote two", name_fr: Masculine("le monstre"), plural: "mote twos", plural_fr: "monstres", family: Vermin, elite: false,
        speed: 1, hp: 1, attack: 0, defense: 0, min_depth: 2),
    Monster(id: "mote_three", name: "mote three", name_fr: Masculine("le monstre"), plural: "mote threes", plural_fr: "monstres", family: Vermin, elite: false,
        speed: 1, hp: 1, attack: 0, defense: 0, min_depth: 4),
    Monster(id: "mote_four", name: "mote four", name_fr: Masculine("le monstre"), plural: "mote fours", plural_fr: "monstres", family: Vermin, elite: false,
        speed: 1, hp: 1, attack: 0, defense: 0, min_depth: 7),
"#;

/// A biter that strikes hard, to wound the player on purpose. It is placed
/// by hand and only has a rule for floor 10, which the tests that use it
/// never reach.
const BITER: &str = r#"
    Monster(id: "biter", name: "biter", name_fr: Masculine("le monstre"), plural: "biters", plural_fr: "monstres", family: Vermin, elite: false,
        speed: 100, hp: 5, attack: 6, defense: 0, min_depth: 1),
"#;

/// The rule of the biter.
const BITER_RULE: &str =
    r#"    Spawn(of: Monster("biter"), from_depth: 10, to_depth: 10, weight: 1),"#;

/// One tier per band of floors, and the items of the shipped game.
const TIER_RULES: &str = r#"
    Spawn(of: Monster("mote_one"), from_depth: 1, to_depth: 1, weight: 1),
    Spawn(of: Monster("mote_two"), from_depth: 2, to_depth: 3, weight: 1),
    Spawn(of: Monster("mote_three"), from_depth: 4, to_depth: 6, weight: 1),
    Spawn(of: Monster("mote_four"), from_depth: 7, to_depth: 10, weight: 1),
    Spawn(of: Item("healing_draught"), from_depth: 1, to_depth: 10, weight: 1),
    Spawn(of: Item("essence_phial"), from_depth: 1, to_depth: 10, weight: 1),
    Spawn(of: Item("steadying_ink"), from_depth: 1, to_depth: 10, weight: 1),
    Spawn(of: Item("veil_page"), from_depth: 2, to_depth: 10, weight: 1),
"#;

/// Most keys spent walking to the stairs of one floor.
const WALK_BUDGET: usize = 1500;

/// The reference seeds of the tests.
fn seeds() -> [Seed; 3] {
    [
        Seed::new(0),
        Seed::new(42),
        "7F3A-91C2-0B5E-D4A8".parse().expect("valid grouped seed"),
    ]
}

/// The default theme of Windows Terminal.
fn dark() -> Theme {
    Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode)
}

/// The harmless creatures with the shipped sigils and items.
fn content() -> Content {
    build(&format!("[{MOTES}]"), &format!("[{TIER_RULES}]"))
}

/// [`content`] and the biter, which only a floor-10 rule places.
fn content_with_biter() -> Content {
    build(
        &format!("[{MOTES}{BITER}]"),
        &format!("[{TIER_RULES}{BITER_RULE}]"),
    )
}

/// The shipped sigils and items with the given monster and spawn files.
fn build(monsters: &str, spawns: &str) -> Content {
    let sigils = include_str!("../../../../sigilfall-core/content/sigils.ron");
    let items = include_str!("../../../../sigilfall-core/content/items.ron");
    Content::parse_with_spawns(monsters, sigils, items, spawns).expect("valid test content")
}

/// A new game of `seed` on the test content, in an application drawn with
/// `theme`.
fn app_with(theme: Theme, seed: Seed) -> App {
    App::new(theme, seed, &content())
}

/// A new game of `seed` on the test content, in the default theme.
fn app(seed: Seed) -> App {
    app_with(dark(), seed)
}

/// A key press of `code` without modifier.
fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

/// Presses each character of `keys` in order.
fn keys(app: &mut App, keys: &str) {
    for key in keys.chars() {
        app.handle_key(press(KeyCode::Char(key)));
    }
}

/// The key that moves in `direction`: the vi keys.
fn vi_char(direction: Direction) -> char {
    match direction {
        Direction::North => 'k',
        Direction::NorthEast => 'u',
        Direction::East => 'l',
        Direction::SouthEast => 'n',
        Direction::South => 'j',
        Direction::SouthWest => 'b',
        Direction::West => 'h',
        Direction::NorthWest => 'y',
    }
}

/// Every cell of `map`, in reading order.
fn cells(map: &Map) -> Vec<Position> {
    let (width, height) = (
        i32::try_from(map.width()).unwrap(),
        i32::try_from(map.height()).unwrap(),
    );
    (0..height)
        .flat_map(|y| (0..width).map(move |x| Position::new(x, y)))
        .collect()
}

/// The cell of the stairs of the floor the player of `app` is on.
fn stairs_of(app: &App) -> Position {
    cells(app.game().map())
        .into_iter()
        .find(|&cell| app.game().map().tile(cell) == Some(Tile::StairsDown))
        .expect("every generated floor has stairs")
}

/// The first step of a shortest walk from `from` to `to`, ignoring
/// creatures; `None` when `to` cannot be reached. Bounded by the number of
/// cells.
fn first_step(map: &Map, from: Position, to: Position) -> Option<Direction> {
    let mut first: HashMap<Position, Direction> = HashMap::new();
    let mut seen = HashSet::from([from]);
    let mut queue = VecDeque::from([from]);
    while let Some(cell) = queue.pop_front() {
        for direction in Direction::ALL {
            let next = cell.step(direction);
            if !map.can_step(cell, direction) || !seen.insert(next) {
                continue;
            }
            let step = if cell == from {
                direction
            } else {
                first[&cell]
            };
            first.insert(next, step);
            if next == to {
                return Some(step);
            }
            queue.push_back(next);
        }
    }
    None
}

/// Presses the vi keys that walk the player to the cell `distance` steps
/// short of the stairs, then returns the number of keys pressed. Bounded.
fn walk_toward_stairs(app: &mut App, stop_short: bool) -> usize {
    let mut pressed = 0;
    for _ in 0..WALK_BUDGET {
        let here = app.game().player_actor().position();
        let stairs = stairs_of(app);
        let Some(direction) = first_step(app.game().map(), here, stairs) else {
            break;
        };
        if here == stairs || (stop_short && here.step(direction) == stairs) {
            break;
        }
        app.handle_key(press(KeyCode::Char(vi_char(direction))));
        pressed += 1;
    }
    pressed
}

/// Walks the player of `app` onto the stairs.
fn walk_to_stairs(app: &mut App) {
    walk_toward_stairs(app, false);
    assert_eq!(
        app.game().player_actor().position(),
        stairs_of(app),
        "the player stands on the stairs"
    );
}

/// The whole screen of `app` at `width` × `height`, as a buffer.
fn buffer_at(app: &App, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
    terminal.backend().buffer().clone()
}

/// The rows of `buffer` as plain text.
fn rows_of(buffer: &Buffer) -> Vec<String> {
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol().to_owned())
                .collect::<String>()
        })
        .collect()
}

/// The screen of `app` at 80 × 24, one string per row.
fn rows(app: &App) -> Vec<String> {
    rows_of(&buffer_at(app, 80, 24))
}

/// The screen of `app` at 80 × 24, rows joined by newlines.
fn text(app: &App) -> String {
    rows(app).join("\n")
}

/// The status line of `app` at 80 × 24: its first row.
fn status_line(app: &App) -> String {
    rows(app).remove(0)
}

/// The texts of the log, oldest first.
fn log(app: &App) -> Vec<String> {
    let mut texts: Vec<String> = app.log().newest_first().map(|m| m.text.clone()).collect();
    texts.reverse();
    texts
}

/// The value shown by the side panel after `label`, as in "HP   25/30".
fn panel_value(app: &App, label: &str) -> Option<String> {
    rows(app).iter().find_map(|row| {
        let at = row.find(label)?;
        let rest = row[at + label.len()..].trim_start();
        let value: String = rest
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '/')
            .collect();
        (!value.is_empty()).then_some(value)
    })
}

/// What the game measures: depth, ticks, player turns, the player's cell,
/// hit points and essence.
type ClockWithVitals = (u32, u64, u64, Position, i32, i32);

/// The clock of `app`'s game (see [`ClockWithVitals`]).
fn clock_with_vitals(app: &App) -> ClockWithVitals {
    let game = app.game();
    let player = game.player_actor();
    (
        game.depth(),
        game.ticks(),
        game.player_turns(),
        player.position(),
        player.hp(),
        player.essence(),
    )
}

/// The identifiers of the kinds of creature on the floor of `app`, sorted,
/// without duplicates.
fn kinds(app: &App) -> Vec<String> {
    let mut kinds: Vec<String> = app
        .game()
        .actors()
        .filter_map(|(_, actor)| Some(actor.as_monster()?.definition().to_owned()))
        .collect();
    kinds.sort();
    kinds.dedup();
    kinds
}

/// The trace obtained by pressing `digits` in order.
fn pressed(digits: &[u8]) -> Trace {
    let mut trace = Trace::new();
    for &digit in digits {
        trace
            .push(Node::new(digit).expect("a digit from 1 to 9"))
            .expect("a node not linked yet");
    }
    trace
}

// --- On the stairs, > or E shows the next floor -----------------

#[test]
fn greater_than_on_the_stairs_shows_depth_two_and_logs_it() {
    for seed in seeds() {
        let mut app = app(seed);
        assert!(
            status_line(&app).starts_with(" Depth 1 · Turn "),
            "{}",
            status_line(&app)
        );
        walk_to_stairs(&mut app);
        let turns = app.game().player_turns();
        let history = log(&app);

        keys(&mut app, ">");

        assert_eq!(app.state(), State::Playing);
        assert_eq!(app.game().depth(), 2, "seed {seed}");
        assert_eq!(app.game().player_turns(), turns + 1, "a descent is a turn");
        assert_eq!(
            app.game().player_actor().position(),
            generation::generate(seed, 2).start(),
            "the player is on the start of the second floor of the seed"
        );
        let line = status_line(&app);
        assert!(
            line.starts_with(&format!(" Depth 2 · Turn {} ", turns + 2)),
            "{line:?}"
        );
        assert!(
            line.trim_end().ends_with(&format!("Seed {seed}")),
            "{line:?}"
        );
        let after = log(&app);
        assert_eq!(
            after[..history.len()],
            history[..],
            "the log keeps its past"
        );
        assert_eq!(
            after.last().map(String::as_str),
            Some("You descend to depth 2.")
        );
        assert_eq!(after.len(), history.len() + 1, "exactly one line added");
        assert!(
            text(&app).contains("You descend to depth 2."),
            "the line is on the screen:\n{}",
            text(&app)
        );
    }
}

#[test]
fn capital_e_descends_like_greater_than_with_or_without_shift() {
    for modifiers in [KeyModifiers::SHIFT, KeyModifiers::NONE] {
        let mut app = app(Seed::new(42));
        walk_to_stairs(&mut app);

        app.handle_key(KeyEvent::new(KeyCode::Char('E'), modifiers));

        assert_eq!(app.game().depth(), 2, "{modifiers:?}");
        assert!(
            status_line(&app).starts_with(" Depth 2 · "),
            "{}",
            status_line(&app)
        );
        assert_eq!(
            log(&app).last().map(String::as_str),
            Some("You descend to depth 2.")
        );
    }
    let mut app = app(Seed::new(42));
    walk_to_stairs(&mut app);
    app.handle_key(KeyEvent::new(KeyCode::Char('>'), KeyModifiers::SHIFT));
    assert_eq!(app.game().depth(), 2, "> typed with Shift");
}

#[test]
fn the_new_floor_is_the_same_for_the_same_seed_screen_included() {
    for seed in seeds() {
        let (mut one, mut two) = (app(seed), app(seed));
        for app in [&mut one, &mut two] {
            walk_to_stairs(app);
            keys(app, ">");
        }

        assert_eq!(text(&one), text(&two), "seed {seed}: the same screen");
        let floor = generation::generate_with_monsters(seed, 2, &content());
        let mut expected: Vec<_> = floor
            .monsters()
            .iter()
            .map(|spawn| (spawn.position(), spawn.definition().id().to_owned()))
            .collect();
        let mut found: Vec<_> = one
            .game()
            .actors()
            .filter_map(|(_, a)| Some((a.position(), a.as_monster()?.definition().to_owned())))
            .collect();
        expected.sort_by_key(|m| (m.0.y, m.0.x));
        found.sort_by_key(|m| (m.0.y, m.0.x));
        assert_eq!(found, expected, "seed {seed}");
    }
    // Another seed, another second floor.
    let (mut a, mut b) = (app(Seed::new(1)), app(Seed::new(2)));
    for app in [&mut a, &mut b] {
        walk_to_stairs(app);
        keys(app, ">");
    }
    assert_ne!(text(&a), text(&b));
}

#[test]
fn only_what_the_player_sees_of_the_new_floor_is_drawn() {
    let mut app = app(Seed::new(42));
    walk_to_stairs(&mut app);
    keys(&mut app, ">");

    let game = app.game();
    let remembered: Vec<Position> = cells(game.map())
        .into_iter()
        .filter(|&cell| game.is_remembered(cell))
        .collect();
    assert!(!remembered.is_empty());
    assert!(
        remembered.iter().all(|&cell| game.is_visible(cell)),
        "nothing of the floor above is remembered on the new one"
    );
    let screen = rows(&app).join("\n");
    assert!(screen.contains('@'), "the player is on the map");
}

#[test]
fn a_descent_takes_one_turn_and_the_turn_counter_runs_on() {
    let mut app = app(Seed::new(42));
    walk_to_stairs(&mut app);
    let line = status_line(&app);
    let turn_before: u64 = line
        .split("Turn ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|number| number.parse().ok())
        .expect("a turn number");

    keys(&mut app, ">");

    let line = status_line(&app);
    let turn_after: u64 = line
        .split("Turn ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|number| number.parse().ok())
        .expect("a turn number");
    assert_eq!(turn_after, turn_before + 1, "{line:?}");
}

// --- The floor's creatures come from the table of its depth ------

#[test]
fn the_creatures_shown_after_each_descent_come_from_the_table_of_the_depth() {
    let mut app = app(Seed::new(42));
    // Down to the tenth floor, the bottom: it has a seal instead of stairs
    // (decision D23), so no floor lies below it.
    for depth in 2..=10_u32 {
        walk_to_stairs(&mut app);
        keys(&mut app, ">");
        let expected = match depth {
            2..=3 => "mote_two",
            4..=6 => "mote_three",
            _ => "mote_four",
        };
        assert_eq!(kinds(&app), [expected], "depth {depth}");
        assert_eq!(app.game().depth(), depth);
    }
}

// --- Off the stairs, nothing happens and the log says so ----------

#[test]
fn off_the_stairs_both_keys_change_nothing_and_the_log_says_so() {
    for seed in seeds() {
        for key in ['>', 'E'] {
            let mut app = app(seed);
            let before = clock_with_vitals(&app);
            let history = log(&app);
            let status_before = status_line(&app);

            keys(&mut app, &key.to_string());

            assert_eq!(
                clock_with_vitals(&app),
                before,
                "seed {seed}, {key}: nothing happened"
            );
            assert_eq!(app.state(), State::Playing);
            let after = log(&app);
            assert_eq!(after[..history.len()], history[..]);
            assert_eq!(
                after.last().map(String::as_str),
                Some("There are no stairs here."),
                "seed {seed}, {key}"
            );
            assert_eq!(after.len(), history.len() + 1);
            assert_eq!(
                status_line(&app),
                status_before,
                "the status line did not move"
            );
            assert!(text(&app).contains("There are no stairs here."));
        }
    }
}

#[test]
fn a_step_short_of_the_stairs_is_still_refused_and_a_repeat_costs_nothing() {
    let mut app = app(Seed::new(42));
    walk_toward_stairs(&mut app, true);
    assert_ne!(app.game().player_actor().position(), stairs_of(&app));
    let before = clock_with_vitals(&app);

    for _ in 0..4 {
        keys(&mut app, ">E");
    }

    assert_eq!(clock_with_vitals(&app), before);
    assert_eq!(app.game().depth(), 1);
    let refusals = log(&app)
        .iter()
        .filter(|line| *line == "There are no stairs here.")
        .count();
    assert_eq!(refusals, 8, "each press is answered");
}

#[test]
fn after_a_refusal_one_more_step_and_the_stairs_work() {
    let mut app = app(Seed::new(42));
    walk_toward_stairs(&mut app, true);
    keys(&mut app, ">");
    assert_eq!(app.game().depth(), 1);

    walk_to_stairs(&mut app);
    keys(&mut app, ">");

    assert_eq!(app.game().depth(), 2);
    assert_eq!(
        log(&app).last().map(String::as_str),
        Some("You descend to depth 2.")
    );
}

#[test]
fn the_refusal_does_not_end_the_game_or_open_anything() {
    let mut app = app(Seed::new(0));
    keys(&mut app, "E");
    assert_eq!(app.state(), State::Playing);
    keys(&mut app, ".");
    assert_eq!(app.game().player_turns(), 1, "the game still takes turns");
}

// --- The keys that must not descend ---------------------------------------------

#[test]
fn other_keys_do_not_descend_even_on_the_stairs() {
    let mut app = app(Seed::new(42));
    walk_to_stairs(&mut app);
    let before = clock_with_vitals(&app);

    for event in [
        press(KeyCode::Char('<')),
        press(KeyCode::Char('e')),
        KeyEvent::new(KeyCode::Char('D'), KeyModifiers::SHIFT),
        KeyEvent::new(KeyCode::Char('D'), KeyModifiers::NONE),
        press(KeyCode::Char('x')),
        press(KeyCode::Enter),
        press(KeyCode::Esc),
        press(KeyCode::Tab),
        KeyEvent::new(KeyCode::Char('>'), KeyModifiers::ALT),
        KeyEvent::new(KeyCode::Char('>'), KeyModifiers::CONTROL),
        KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL),
        KeyEvent::new(KeyCode::Char('D'), KeyModifiers::ALT),
    ] {
        app.handle_key(event);
        assert_eq!(clock_with_vitals(&app), before, "{event:?} did something");
        assert_eq!(app.state(), State::Playing, "{event:?}");
    }
    assert_eq!(app.game().depth(), 1);
}

#[test]
fn the_inventory_overlay_ignores_the_descent_keys_on_the_stairs() {
    let mut app = app(Seed::new(42));
    walk_to_stairs(&mut app);
    keys(&mut app, "i");
    assert!(matches!(app.state(), State::Inventory(_)));

    keys(&mut app, ">E");

    assert_eq!(app.game().depth(), 1, "an open inventory does not descend");
    assert!(matches!(app.state(), State::Inventory(_)));
}

#[test]
fn on_a_screen_under_80_by_24_the_descent_keys_do_nothing() {
    let mut app = app(Seed::new(42));
    walk_to_stairs(&mut app);
    let _ = buffer_at(&app, 79, 24);

    keys(&mut app, ">E");

    assert_eq!(app.game().depth(), 1, "the game screen is not shown");
    let _ = buffer_at(&app, 80, 24);
    keys(&mut app, ">");
    assert_eq!(app.game().depth(), 2, "back at 80 x 24 the key works");
}

// --- What the player keeps --------------------------------------------------------

/// An application whose player stands on the stairs, wounded, short of
/// essence, knowing Ember and carrying a draught, a phial and a steadying
/// ink drunk once (two steadied casts), as set up by core calls: the keys
/// cannot hurt the player, or drink, or cast without the screens in between.
fn prepared(seed: Seed) -> App {
    let content = content_with_biter();
    let mut game = Game::new(seed, &content);
    // Walk to the stairs, striking what blocks the way.
    for _ in 0..WALK_BUDGET {
        let here = game.player_actor().position();
        let stairs = cells(game.map())
            .into_iter()
            .find(|&c| game.map().tile(c) == Some(Tile::StairsDown))
            .unwrap();
        if here == stairs {
            break;
        }
        let direction = first_step(game.map(), here, stairs).expect("reachable");
        game.submit(Action::Move(direction));
    }
    let stairs = game.player_actor().position();
    assert_eq!(game.map().tile(stairs), Some(Tile::StairsDown));

    let neighbour = Direction::ALL
        .into_iter()
        .find(|&d| {
            let cell = stairs.step(d);
            game.map().can_step(stairs, d)
                && game.map().tile(cell) == Some(Tile::Floor)
                && game.actors().all(|(_, a)| a.position() != cell)
        })
        .expect("a free floor cell next to the stairs");
    game.engrave(stairs.step(neighbour), content.sigil("ember").unwrap());
    game.submit(Action::Move(neighbour));
    let back = match neighbour {
        Direction::North => Direction::South,
        Direction::NorthEast => Direction::SouthWest,
        Direction::East => Direction::West,
        Direction::SouthEast => Direction::NorthWest,
        Direction::South => Direction::North,
        Direction::SouthWest => Direction::NorthEast,
        Direction::West => Direction::East,
        Direction::NorthWest => Direction::SouthEast,
    };
    game.submit(Action::Move(back));
    for id in [
        "healing_draught",
        "essence_phial",
        "steadying_ink",
        "steadying_ink",
    ] {
        game.place_item(stairs, content.item(id).unwrap());
        game.submit(Action::PickUp);
    }
    let ink = game
        .inventory()
        .find(|(_, item)| item.definition().id() == "steadying_ink")
        .map(|(id, _)| id)
        .unwrap();
    game.submit(Action::Use(ink));

    let biter = game.spawn_monster(stairs.step(neighbour), content.monster("biter").unwrap());
    for _ in 0..40 {
        if game.player_actor().hp() < game.player_actor().max_hp() {
            break;
        }
        game.submit(Action::Wait);
    }
    for _ in 0..40 {
        if game.actor(biter).is_none() || game.is_over() {
            break;
        }
        game.submit(Action::Move(neighbour));
    }
    let target = cells(game.map())
        .into_iter()
        .find(|&cell| {
            let far = (cell.x - stairs.x).abs().max((cell.y - stairs.y).abs()) >= 3;
            far && game.is_visible(cell)
                && game.map().is_walkable(cell)
                && game.actors().all(|(_, a)| a.position() != cell)
        })
        .expect("a free cell in view");
    game.spawn_monster(target, content.monster("mote_one").unwrap());
    game.submit(Action::Cast {
        trace: pressed(&[7, 5, 3]),
        direction: None,
    });
    assert!(!game.is_over());
    assert_eq!(game.player_actor().position(), stairs);
    App::with_game(dark(), game, content)
}

#[test]
fn the_panel_the_inventory_and_the_grimoire_are_the_same_on_the_next_floor() {
    let mut app = prepared(Seed::new(42));
    let hp = panel_value(&app, "HP").expect("hit points in the panel");
    let essence = panel_value(&app, "Essence").expect("essence in the panel");
    let sigils = panel_value(&app, "SIGILS").expect("the grimoire in the panel");
    assert_ne!(hp, "30/30", "wounded: {hp}");
    assert!(!essence.starts_with('6'), "short of essence: {essence}");
    assert!(sigils.starts_with('1'), "one sigil known: {sigils}");
    keys(&mut app, "i");
    let inventory_before = text(&app);
    app.handle_key(press(KeyCode::Esc));
    assert_eq!(app.state(), State::Playing);
    let steady = app.game().steady_casts();
    assert_eq!(steady, 2);

    keys(&mut app, ">");

    assert_eq!(app.game().depth(), 2);
    assert_eq!(panel_value(&app, "HP"), Some(hp), "hit points");
    let essence_after = panel_value(&app, "Essence").expect("essence");
    assert!(
        essence_after == essence
            || essence_after.starts_with(&format!("{}/", essence_value(&essence) + 1)),
        "essence {essence} then {essence_after}"
    );
    assert_eq!(panel_value(&app, "SIGILS"), Some(sigils), "grimoire");
    assert_eq!(app.game().steady_casts(), steady, "steadied casts");
    keys(&mut app, "i");
    let inventory_after = text(&app);
    for name in ["Healing draught", "Essence phial", "Steadying ink"] {
        assert!(
            inventory_before.contains(name),
            "{name} before:\n{inventory_before}"
        );
        assert!(
            inventory_after.contains(name),
            "{name} after:\n{inventory_after}"
        );
    }
    app.handle_key(press(KeyCode::Esc));
    assert_eq!(app.state(), State::Playing);
}

/// The number before the slash in "4/6".
fn essence_value(value: &str) -> i32 {
    value
        .split('/')
        .next()
        .and_then(|number| number.parse().ok())
        .expect("a number before the slash")
}

#[test]
fn the_log_keeps_every_line_of_the_floors_above() {
    let mut app = app(Seed::new(42));
    let mut history = log(&app);
    assert_eq!(history, ["You enter the dungeon."]);
    for depth in 2..=4 {
        walk_to_stairs(&mut app);
        keys(&mut app, ">");
        let now = log(&app);
        assert_eq!(now[..history.len()], history[..], "depth {depth}");
        assert_eq!(
            now.last().cloned(),
            Some(format!("You descend to depth {depth}."))
        );
        history = now;
    }
    assert_eq!(
        history
            .iter()
            .filter(|line| line.starts_with("You descend"))
            .count(),
        3
    );
}

// --- Status line, key help and small screens --------------------------------------

#[test]
fn the_key_help_names_the_descent_and_still_fits_in_80_columns() {
    let app = app(Seed::new(42));
    let screen = rows(&app);
    let help = screen.last().expect("a last row");
    assert!(help.contains("> descend"), "{help:?}");
    assert!(help.contains("q save & quit"), "{help:?}");
    assert_eq!(help.chars().count(), 80);
    assert!(
        help.trim_end().ends_with("q save & quit  x explore"),
        "{help:?}"
    );
}

#[test]
fn the_status_line_shows_every_depth_up_to_the_tenth_without_losing_the_seed() {
    let seed = Seed::new(42);
    let mut app = app(seed);
    // The tenth floor is the bottom (decision D23): no stairs lead deeper.
    for depth in 2..=10_u32 {
        walk_to_stairs(&mut app);
        keys(&mut app, ">");
        let line = status_line(&app);
        assert!(
            line.starts_with(&format!(" Depth {depth} · Turn ")),
            "{line:?}"
        );
        assert!(
            line.trim_end().ends_with(&format!("Seed {seed}")),
            "{line:?}"
        );
        assert_eq!(line.chars().count(), 80);
    }
}

#[test]
fn in_ascii_the_status_line_and_the_log_are_plain_ascii_on_every_floor() {
    let ascii = Theme::new(ColorMode::TrueColorDark, GlyphSet::Ascii);
    let mut app = app_with(ascii, Seed::new(42));
    for depth in 1..=3_u32 {
        if depth > 1 {
            walk_to_stairs(&mut app);
            keys(&mut app, ">");
        }
        let screen = text(&app);
        assert!(
            screen.chars().all(|c| c.is_ascii() || c == '\n'),
            "depth {depth}: non-ASCII in\n{screen}"
        );
        assert!(
            status_line(&app).starts_with(&format!(" Depth {depth} - Turn ")),
            "{}",
            status_line(&app)
        );
    }
    assert!(text(&app).contains("You descend to depth 3."));
}

#[test]
fn the_stairs_are_named_in_the_panel_once_in_view() {
    let mut app = app(Seed::new(42));
    walk_toward_stairs(&mut app, true);
    assert!(text(&app).contains("stairs down"), "{}", text(&app));
}

#[test]
fn every_floor_draws_at_every_size_without_panicking() {
    let mut app = app(Seed::new(7));
    for depth in 1..=3_u32 {
        if depth > 1 {
            walk_to_stairs(&mut app);
            keys(&mut app, ">");
        }
        for (width, height) in [
            (1, 1),
            (20, 5),
            (79, 24),
            (80, 23),
            (80, 24),
            (81, 25),
            (120, 40),
            (200, 60),
        ] {
            let buffer = buffer_at(&app, width, height);
            assert_eq!((buffer.area.width, buffer.area.height), (width, height));
        }
    }
    assert_eq!(app.game().depth(), 3);
}

// --- Deterministic replay ------------------------------------------------------------

/// Plays a series of keys computed from the game itself (walk to the stairs,
/// descend) for `floors` floors and returns the keys pressed.
fn series(seed: Seed, floors: usize) -> String {
    let mut app = app(seed);
    let mut pressed = String::new();
    for _ in 0..floors {
        for _ in 0..WALK_BUDGET {
            let here = app.game().player_actor().position();
            let stairs = stairs_of(&app);
            if here == stairs {
                break;
            }
            let key = vi_char(first_step(app.game().map(), here, stairs).expect("reachable"));
            pressed.push(key);
            keys(&mut app, &key.to_string());
        }
        pressed.push('>');
        keys(&mut app, ">");
    }
    pressed
}

#[test]
fn the_same_keys_replay_to_the_same_screen_across_four_floors() {
    for seed in seeds() {
        let played = series(seed, 3);
        let (mut one, mut two) = (app(seed), app(seed));
        keys(&mut one, &played);
        keys(&mut two, &played);

        assert_eq!(one.game().depth(), 4, "seed {seed}");
        assert_eq!(text(&one), text(&two), "seed {seed}");
        assert_eq!(log(&one), log(&two));
        assert_eq!(clock_with_vitals(&one), clock_with_vitals(&two));
        // Extra waits and refusals on the last floor stay in step too.
        keys(&mut one, "..>D.");
        keys(&mut two, "..>D.");
        assert_eq!(text(&one), text(&two), "seed {seed}");
    }
}
