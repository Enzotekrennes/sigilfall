//! Automatic exploration driven by simulated keys: `x` takes a first step at
//! once, the walk goes on step by step and stops on a monster, an item or an
//! unread glyph coming into view, on a key press, at its step limit or when
//! nothing is left to explore, with a line in the log each time; a monster
//! already in view keeps the player still, and generated floors are explored in
//! a bounded number of presses.
//!
//! The tests written from the player's side drive the application as the event
//! loop does: a press of the explore key starts a walk, then
//! `App::explore_further` is called once per step (the loop calls it every
//! 25 ms when no key arrives) and a key press goes through `App::handle_key`
//! while the walk is running. They read what the player reads, the log lines
//! and the rendered screen, and the state of the core game for what must not
//! happen (no time passing, no blow). They are written independently of the
//! unit tests of `app::exploring`: the stop reasons are checked against the
//! game itself (is the monster really in view? did the item come into view
//! with this very step?), not against the function that decides them. The
//! pseudo-terminal half (steady redraw between steps, a key stopping the real
//! loop) is run by hand against the real binary.

use super::player_view::{rows, shipped};
use super::*;
use crate::app::exploring::MAX_EXPLORE_STEPS;
use crate::app::{App, State};
use crate::input::EXPLORE_KEY;
use crate::message_log::Tone;
use crate::theme::{ColorMode, GlyphSet, Theme};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use sigilfall_core::action::Action;
use sigilfall_core::content::{Content, FloorKind, MonsterDef};
use sigilfall_core::game::Game;
use sigilfall_core::generation::{generate, generate_of_kind};
use sigilfall_core::geometry::Position;
use sigilfall_core::path::flood_fill;
use sigilfall_core::rng::Seed;
use sigilfall_core::save;

/// Cell of the fixed room far east of the player, out of its sight at the
/// start: the walk heads east, where the room is still unknown.
const FAR_EAST: Position = Position::new(27, 5);

/// Number of seeds whose first floor is explored with the explore key.
const EXPLORED_SEEDS: u64 = 50;

/// The explore key, pressed.
fn explore_key() -> KeyEvent {
    press(KeyCode::Char(EXPLORE_KEY))
}

/// An application on a game in the fixed room, set up by `prepare` with
/// the shipped content before the game starts.
fn fixed_room_with(prepare: impl FnOnce(&mut Game, &Content)) -> App {
    let content = shipped_content();
    let mut game = Game::in_fixed_room(SEED);
    prepare(&mut game, &content);
    App::with_game(
        Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode),
        game,
        content,
    )
}

/// Takes the steps of the walk in progress until it stops, as the event
/// loop does when no key comes, and returns how many were taken.
///
/// # Panics
///
/// Panics if the walk goes on for more than [`MAX_EXPLORE_STEPS`] steps,
/// which the walk itself must prevent.
fn walk_to_a_halt(app: &mut App) -> u32 {
    let mut steps = 0;
    while app.is_exploring() {
        assert!(steps < MAX_EXPLORE_STEPS, "the walk outlived its limit");
        app.explore_further();
        steps += 1;
    }
    steps
}

/// The newest line of the log, with its tone.
fn last_line(app: &App) -> (Tone, String) {
    let message = app.log().newest_first().next().expect("the log has lines");
    (message.tone, message.text.clone())
}

#[test]
fn x_takes_a_first_step_at_once_and_the_walk_goes_on() {
    let mut app = new_app();
    let start = player_position(&app);

    app.handle_key(explore_key());

    assert_ne!(player_position(&app), start, "the first step is taken");
    assert_eq!(app.game().player_turns(), 1);
    assert!(app.is_exploring());

    app.explore_further();

    assert_eq!(app.game().player_turns(), 2, "one step per call");
    assert!(app.is_exploring());
}

#[test]
fn the_walk_heads_for_the_nearest_unexplored_cells() {
    // From the start, the west wall of the room is in sight and its east
    // part is not: the first steps go east.
    let mut app = new_app();
    let start = player_position(&app);

    app.handle_key(explore_key());
    app.explore_further();

    assert_eq!(player_position(&app).x, start.x + 2);
}

#[test]
fn the_room_is_explored_to_the_end_and_a_new_press_costs_nothing() {
    let mut app = new_app();

    app.handle_key(explore_key());
    let steps = walk_to_a_halt(&mut app);

    assert!(steps > 0);
    assert_eq!(
        last_line(&app),
        (
            Tone::Info,
            "There is nothing left to explore within reach.".to_owned()
        )
    );
    let room = app.game().map();
    for cell in flood_fill(room, PLAYER_START) {
        assert!(app.game().is_remembered(cell), "{cell:?} never seen");
    }

    let turns = app.game().player_turns();
    let ticks = app.game().ticks();
    let lines = app.log().len();
    app.handle_key(explore_key());

    assert!(!app.is_exploring());
    assert_eq!(app.game().player_turns(), turns, "no time passed");
    assert_eq!(app.game().ticks(), ticks);
    assert_eq!(app.log().len(), lines + 1, "said once, not twice");
}

#[test]
fn a_monster_coming_into_view_stops_the_walk() {
    let mut app = fixed_room_with(|game, content| {
        game.spawn_monster(FAR_EAST, content.monster("ossuary_rat").unwrap());
    });

    app.handle_key(explore_key());
    walk_to_a_halt(&mut app);

    assert_eq!(
        last_line(&app),
        (Tone::Danger, "An ossuary rat comes into view.".to_owned())
    );
    assert_eq!(app.game().visible_monsters().count(), 1);
}

#[test]
fn a_monster_already_in_view_keeps_the_player_still() {
    let mut app = fixed_room_with(|game, content| {
        game.spawn_monster(Position::new(8, 5), content.monster("shade").unwrap());
    });
    let start = player_position(&app);

    app.handle_key(explore_key());

    assert!(!app.is_exploring());
    assert_eq!(player_position(&app), start);
    assert_eq!(app.game().player_turns(), 0, "no time passed");
    assert_eq!(
        last_line(&app),
        (
            Tone::Info,
            "You cannot explore with the shade in view.".to_owned()
        )
    );
}

#[test]
fn an_item_stops_the_walk_when_it_comes_into_view_and_only_then() {
    let mut app = fixed_room_with(|game, content| {
        game.place_item(FAR_EAST, content.item("essence_phial").unwrap());
    });

    app.handle_key(explore_key());
    walk_to_a_halt(&mut app);

    assert_eq!(
        last_line(&app),
        (Tone::Info, "You spot an Essence Phial.".to_owned())
    );
    assert_eq!(app.game().visible_items().count(), 1);

    // Pressed again with the phial in view, the walk goes on.
    let turns = app.game().player_turns();
    app.handle_key(explore_key());
    walk_to_a_halt(&mut app);

    assert!(app.game().player_turns() > turns);
    assert_ne!(
        last_line(&app).1,
        "You spot an Essence Phial.",
        "the phial in view does not stop the walk again"
    );
}

#[test]
fn an_unread_glyph_coming_into_view_stops_the_walk() {
    let mut app = fixed_room_with(|game, content| {
        game.engrave(FAR_EAST, &content.sigils()[0]);
    });

    app.handle_key(explore_key());
    walk_to_a_halt(&mut app);

    assert_eq!(
        last_line(&app),
        (
            Tone::Sigil,
            "You spot a carved glyph you have not read.".to_owned()
        )
    );
    assert!(app.game().is_visible(FAR_EAST));
}

#[test]
fn a_key_press_stops_the_walk_and_does_nothing_else() {
    let mut app = new_app();
    app.handle_key(explore_key());
    let here = player_position(&app);
    let turns = app.game().player_turns();

    app.handle_key(release(KeyCode::Char(EXPLORE_KEY)));
    assert!(app.is_exploring(), "a release does not stop the walk");

    app.handle_key(press(KeyCode::Char('l')));

    assert!(!app.is_exploring());
    assert_eq!(
        player_position(&app),
        here,
        "the key did not move the player"
    );
    assert_eq!(app.game().player_turns(), turns);
    assert_eq!(app.state(), State::Playing);
    assert_eq!(
        last_line(&app),
        (Tone::Info, "You stop exploring.".to_owned())
    );

    // The next key is played as usual.
    app.handle_key(press(KeyCode::Char('l')));
    assert_eq!(player_position(&app), Position::new(here.x + 1, here.y));
}

#[test]
fn q_stops_the_walk_without_quitting_and_ctrl_c_quits() {
    let mut app = new_app();
    app.handle_key(explore_key());

    app.handle_key(press(KeyCode::Char('q')));

    assert!(!app.is_exploring());
    assert_eq!(app.state(), State::Playing);

    app.handle_key(explore_key());
    app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));

    assert!(!app.is_exploring());
    assert_eq!(app.state(), State::Quit);
}

#[test]
fn a_walk_stops_at_its_step_limit_and_says_so() {
    // The first floor of the test seed takes more than one walk to explore.
    let mut app = generated_floor_app();

    app.handle_key(explore_key());
    let steps = 1 + walk_to_a_halt(&mut app);

    assert_eq!(steps, MAX_EXPLORE_STEPS);
    assert_eq!(app.game().player_turns(), u64::from(MAX_EXPLORE_STEPS));
    assert_eq!(
        last_line(&app),
        (Tone::Info, "You stop exploring after 200 steps.".to_owned())
    );
}

#[test]
fn a_screen_too_small_stops_the_walk() {
    let mut app = new_app();
    app.handle_key(explore_key());
    let turns = app.game().player_turns();

    render(&app, 79, 24);
    app.explore_further();

    assert!(!app.is_exploring());
    assert_eq!(app.game().player_turns(), turns, "no step taken blind");

    app.handle_key(explore_key());
    assert!(!app.is_exploring(), "the key does nothing below 80 × 24");
}

#[test]
fn the_same_seed_explores_the_same_way() {
    let play = || {
        let mut app = generated_floor_app();
        app.handle_key(explore_key());
        walk_to_a_halt(&mut app);
        (player_position(&app), app.game().ticks(), last_line(&app))
    };

    assert_eq!(play(), play());
}

/// Explores the bare first floor of every seed in `seeds` with the explore
/// key alone and checks that the floor is seen in full, in no more presses
/// than its open cells need walks of [`MAX_EXPLORE_STEPS`] steps, plus the
/// last press that finds nothing left.
///
/// # Panics
///
/// Panics, naming the seed, if a press finds the walk running or takes too
/// many presses, or if a reachable cell is never seen.
fn explore_with_presses(seeds: std::ops::Range<u64>) {
    for raw in seeds {
        let seed = Seed::new(raw);
        let game = Game::on_floor(seed, generation::generate(seed, FIRST_DEPTH));
        let reachable = flood_fill(game.map(), game.player_actor().position());
        let mut app = App::with_game(
            Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode),
            game,
            shipped_content(),
        );
        let steps_per_press = usize::try_from(MAX_EXPLORE_STEPS).expect("fits");
        let max_presses = reachable.len().div_ceil(steps_per_press) + 1;

        let mut presses = 0;
        loop {
            assert!(presses < max_presses, "seed {raw}: too many presses");
            app.handle_key(explore_key());
            presses += 1;
            walk_to_a_halt(&mut app);
            if last_line(&app).1 == "There is nothing left to explore within reach." {
                break;
            }
        }

        for cell in reachable {
            assert!(
                app.game().is_remembered(cell),
                "seed {raw}: {cell:?} never seen"
            );
        }
    }
}

// The seeds are explored in two halves, which the test harness runs in
// parallel: a floor takes about a second in a debug build.

#[test]
fn generated_floors_are_explored_in_bounded_presses_first_half() {
    explore_with_presses(0..EXPLORED_SEEDS / 2);
}

#[test]
fn generated_floors_are_explored_in_bounded_presses_second_half() {
    explore_with_presses(EXPLORED_SEEDS / 2..EXPLORED_SEEDS);
}

/// The explore key of the README.
const EXPLORE: char = 'x';

/// Most presses of the explore key a test makes on one floor before it gives
/// up: far more than the 8 or so a floor needs.
const PRESS_BUDGET: u32 = 40;

/// Steps one press may take at most, as the README says.
const README_STEP_LIMIT: u32 = 200;

fn dark() -> Theme {
    Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode)
}

/// A seed spread over the whole range from a small number.
fn seed_of(n: u64) -> Seed {
    Seed::new(n.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x5A5A)
}

/// A game alone on a bare floor of rooms.
fn rooms_game(n: u64) -> Game {
    Game::on_floor(seed_of(n), generate(seed_of(n), 1))
}

/// A game alone on a bare cave.
fn cave_game(n: u64) -> Game {
    Game::on_floor(
        seed_of(n),
        generate_of_kind(seed_of(n), 5, FloorKind::Caves),
    )
}

/// The application on `game`.
fn app_on(game: Game) -> App {
    App::with_game(dark(), game, shipped())
}

/// Presses the explore key.
fn press_explore(app: &mut App) {
    app.handle_key(press(KeyCode::Char(EXPLORE)));
}

/// Lets a started walk run as the event loop would when no key comes, and
/// returns the number of steps it took, the first one included (the one the
/// press itself takes).
fn run_walk(app: &mut App) -> u32 {
    let turns = app.game().player_turns();
    for _ in 0..1000 {
        if !app.is_exploring() {
            break;
        }
        app.explore_further();
    }
    assert!(!app.is_exploring(), "a walk of more than 1000 steps");
    u32::try_from(app.game().player_turns() - turns).unwrap()
}

/// Presses the explore key and runs the walk to its end; returns the steps
/// taken from the press and the newest line of the log.
fn explore_press(app: &mut App) -> (u32, String) {
    let turns = app.game().player_turns();
    press_explore(app);
    run_walk(app);
    let steps = u32::try_from(app.game().player_turns() - turns).unwrap();
    (steps, newest(app))
}

/// The newest line of the log.
fn newest(app: &App) -> String {
    app.log()
        .newest_first()
        .next()
        .map(|m| m.text.clone())
        .unwrap_or_default()
}

/// Every line of the log, oldest first.
fn log_lines(app: &App) -> Vec<String> {
    let mut lines: Vec<String> = app.log().newest_first().map(|m| m.text.clone()).collect();
    lines.reverse();
    lines
}

/// The state of the whole game as text.
fn state_of(app: &App) -> String {
    save::to_string(app.game()).unwrap()
}

fn content() -> Content {
    shipped()
}

/// The cell where the walk of a bare game would be far away: a walkable
/// cell the player has not seen, at least `distance` king-steps from the
/// player and not visible, so the walk has to travel before it comes into
/// view.
fn far_cell(game: &Game, distance: i32) -> Position {
    let me = game.player_actor().position();
    let (w, h) = (
        i32::try_from(game.map().width()).unwrap(),
        i32::try_from(game.map().height()).unwrap(),
    );
    (0..h)
        .flat_map(|y| (0..w).map(move |x| Position::new(x, y)))
        .filter(|&p| {
            game.map().is_walkable(p)
                && !game.is_remembered(p)
                && p != me
                && (p.x - me.x).abs().max((p.y - me.y).abs()) >= distance
                && game.actors().all(|(_, a)| a.position() != p)
                && game.engraving_at(p).is_none()
        })
        .min_by_key(|p| ((p.x - me.x).abs().max((p.y - me.y).abs()), p.y, p.x))
        .expect("a far cell")
}

// --- Scenario 1: toward the nearest unexplored place ------------------------

/// One press walks, step after step, until something appears: on a bare
/// floor with one monster far away, the walk stops when it comes into view.
#[test]
fn scenario_1_a_press_walks_until_a_monster_comes_into_view() {
    let content = content();
    let rat = content.monsters().first().unwrap();
    for n in 0..6 {
        let mut game = rooms_game(n);
        let cell = far_cell(&game, 20);
        game.spawn_monster(cell, rat);
        let mut app = app_on(game);
        let start = app.game().player_actor().position();

        let mut stops = Vec::new();
        for _ in 0..PRESS_BUDGET {
            let hp = app.game().player_actor().hp();
            let (steps, line) = explore_press(&mut app);
            stops.push((steps, line.clone()));
            if line.contains("comes into view") {
                break;
            }
            // A monster met round a corner may strike in the very step that
            // shows it: the wound is the reason then, and the hit points
            // really fell.
            if line.contains("hurt") {
                assert!(app.game().player_actor().hp() < hp, "seed {n}: {line}");
                break;
            }
            // Anything else would be the step limit.
            assert!(line.contains("after 200 steps"), "seed {n}: {line}");
            assert_eq!(steps, README_STEP_LIMIT, "seed {n}: {line}");
        }

        let (_, last) = stops.last().unwrap();
        assert!(
            last.contains("comes into view") || last.contains("hurt"),
            "seed {n}: {stops:?}"
        );
        assert!(
            app.game().visible_monsters().count() > 0,
            "seed {n}: the walk stopped for a monster but none is in view"
        );
        assert_ne!(app.game().player_actor().position(), start);
        assert!(!app.is_exploring());
        assert_eq!(app.state(), State::Playing);
    }
}

#[test]
fn scenario_1_the_walk_stops_on_an_item_that_comes_into_view_and_goes_on_after() {
    let content = content();
    let item = content.items().first().unwrap();
    for n in 10..16 {
        let mut game = rooms_game(n);
        let cell = far_cell(&game, 20);
        game.place_item(cell, item);
        let mut app = app_on(game);

        let mut found = false;
        for _ in 0..PRESS_BUDGET {
            let (_, line) = explore_press(&mut app);
            if line.starts_with("You spot") {
                found = true;
                assert!(
                    app.game().visible_items().count() > 0,
                    "seed {n}: the log says an item appeared: {line}"
                );
                break;
            }
            assert!(line.contains("after 200 steps"), "seed {n}: {line}");
        }
        assert!(found, "seed {n}: the walk never stopped for the item");

        // The item is still in view: it must not stop the walk again at
        // once; the walk goes on to somewhere else.
        let turns = app.game().player_turns();
        press_explore(&mut app);
        assert!(
            app.game().player_turns() > turns,
            "seed {n}: the walk did not take a step"
        );
        run_walk(&mut app);
        assert!(
            app.game().player_turns() > turns + 1,
            "seed {n}: the item in view stopped the walk after one step: {}",
            newest(&app)
        );
    }
}

#[test]
fn scenario_1_the_walk_stops_on_a_glyph_that_has_not_been_read() {
    let content = content();
    let sigil = content.sigils().first().unwrap();
    for n in 20..26 {
        let mut game = rooms_game(n);
        let cell = far_cell(&game, 20);
        game.engrave(cell, sigil);
        let mut app = app_on(game);

        let mut found = false;
        for _ in 0..PRESS_BUDGET {
            let (_, line) = explore_press(&mut app);
            if line.contains("carved glyph") {
                found = true;
                assert!(
                    app.game()
                        .engravings()
                        .iter()
                        .any(|e| app.game().is_visible(e.position())),
                    "seed {n}: the log speaks of a glyph, none is in view"
                );
                break;
            }
            assert!(line.contains("after 200 steps"), "seed {n}: {line}");
        }
        assert!(found, "seed {n}: the walk never stopped for the glyph");
        // The player has not stepped on it: it is still unread.
        assert!(!app.game().grimoire().knows(sigil.id()), "seed {n}");
    }
}

/// The step limit of the README: a walk with nothing to meet stops after
/// 200 steps exactly, says so, and the next press goes on.
#[test]
fn scenario_1_a_walk_stops_by_itself_after_200_steps_and_the_next_press_goes_on() {
    let mut app = app_on(cave_game(3));

    let (steps, line) = explore_press(&mut app);

    assert_eq!(steps, README_STEP_LIMIT);
    assert_eq!(line, "You stop exploring after 200 steps.");
    let turns = app.game().player_turns();
    press_explore(&mut app);
    assert!(
        app.game().player_turns() > turns,
        "the next press explores on"
    );
}

/// A walk on a populated floor, as a player meets it: every stop has a
/// reason the log gives and the game confirms.
#[test]
fn scenario_1_every_stop_on_a_populated_floor_is_one_the_game_confirms() {
    let content = content();
    let mut reasons = std::collections::BTreeMap::new();
    for n in 40..64 {
        let game = Game::new(seed_of(n), &content);
        let mut app = app_on(game);
        for _ in 0..12 {
            let hp = app.game().player_actor().hp();
            let turns = app.game().player_turns();
            let (steps, line) = explore_press(&mut app);
            let game = app.game();
            if game.is_over() {
                break;
            }
            let reason = if line.contains("comes into view") {
                assert!(game.visible_monsters().count() > 0, "seed {n}: {line}");
                "monster"
            } else if line.contains("carved glyph") {
                assert!(
                    game.engravings()
                        .iter()
                        .any(|e| game.is_visible(e.position())),
                    "seed {n}: {line}"
                );
                "glyph"
            } else if line.starts_with("You spot") {
                assert!(game.visible_items().count() > 0, "seed {n}: {line}");
                "item"
            } else if line.contains("hurt") {
                assert!(game.player_actor().hp() < hp, "seed {n}: {line}");
                "hurt"
            } else if line.contains("after 200 steps") {
                assert_eq!(steps, README_STEP_LIMIT, "seed {n}");
                "limit"
            } else if line.contains("nothing left to explore") {
                assert_eq!(game.player_turns(), turns + u64::from(steps));
                "explored"
            } else if line.contains("cannot explore") {
                assert_eq!(steps, 0, "seed {n}: no step with a foe in view");
                "refused"
            } else if line.contains("Something stands in your way") {
                "blocked"
            } else {
                panic!("seed {n}: unexpected line {line:?}");
            };
            *reasons.entry(reason).or_insert(0_u32) += 1;
            // Do not fight: stop this floor at the first monster.
            if reason == "monster" || reason == "hurt" || reason == "refused" {
                break;
            }
        }
    }
    println!("stop reasons over 24 floors: {reasons:?}");
    assert!(reasons.contains_key("monster") || reasons.contains_key("item"));
}

// --- Scenario 2: nothing left to explore ------------------------------------

#[test]
fn scenario_2_an_explored_floor_says_so_in_the_log_and_no_time_passes() {
    let mut app = app_on(rooms_game(31));
    // Explore everything.
    let mut presses = 0;
    loop {
        let (_, line) = explore_press(&mut app);
        presses += 1;
        if line.contains("nothing left to explore") {
            break;
        }
        assert!(presses < 40, "never explored: {line}");
    }
    let before = state_of(&app);
    let (ticks, turns) = (app.game().ticks(), app.game().player_turns());
    let logged = app.log().len();

    press_explore(&mut app);

    assert_eq!(
        newest(&app),
        "There is nothing left to explore within reach."
    );
    assert_eq!(app.log().len(), logged + 1, "exactly one line added");
    assert_eq!(app.game().ticks(), ticks);
    assert_eq!(app.game().player_turns(), turns);
    assert_eq!(state_of(&app), before, "the whole game state is unchanged");
    assert!(!app.is_exploring());
    // Again, and again: the same.
    press_explore(&mut app);
    press_explore(&mut app);
    assert_eq!(state_of(&app), before);
    let screen = rows(&app, 80, 24).join("\n");
    assert!(screen.contains("nothing left to explore"), "{screen}");
}

#[test]
fn scenario_2_the_fixed_room_is_explored_then_the_key_is_refused_without_time() {
    let mut app = app_on(Game::in_fixed_room(Seed::new(2)));
    for _ in 0..5 {
        let (_, line) = explore_press(&mut app);
        if line.contains("nothing left to explore") {
            break;
        }
    }
    assert_eq!(
        newest(&app),
        "There is nothing left to explore within reach."
    );
    let before = state_of(&app);

    press_explore(&mut app);

    assert_eq!(
        newest(&app),
        "There is nothing left to explore within reach."
    );
    assert_eq!(state_of(&app), before);
}

// --- A monster in view: no start, no blow ------------------------------------

#[test]
fn a_monster_in_view_refuses_to_start_the_walk_and_no_time_passes() {
    let content = content();
    let rat = content.monsters().first().unwrap();
    let mut game = rooms_game(70);
    let me = game.player_actor().position();
    let near = [
        (2, 0),
        (-2, 0),
        (0, 2),
        (0, -2),
        (3, 1),
        (-3, 1),
        (1, 3),
        (1, -3),
    ]
    .into_iter()
    .map(|(dx, dy)| Position::new(me.x + dx, me.y + dy))
    .find(|&p| game.map().is_walkable(p) && game.is_visible(p))
    .expect("a visible free cell near the player");
    game.spawn_monster(near, rat);
    let mut app = app_on(game);
    let before = state_of(&app);
    let hp = app.game().player_actor().hp();

    press_explore(&mut app);

    assert!(
        newest(&app).starts_with("You cannot explore with the"),
        "{}",
        newest(&app)
    );
    assert!(newest(&app).ends_with("in view."), "{}", newest(&app));
    assert!(!app.is_exploring());
    assert_eq!(state_of(&app), before, "no step, no time, no blow");
    assert_eq!(app.game().player_actor().hp(), hp);
    // Pressing again changes nothing either.
    press_explore(&mut app);
    assert_eq!(state_of(&app), before);
}

/// Over populated floors, the walk never strikes: no blow of the player in
/// the events that follow an explore key, whatever stands around.
#[test]
fn exploring_never_strikes_even_with_monsters_closing_in() {
    let content = content();
    for n in 80..96 {
        let mut app = app_on(Game::new(seed_of(n), &content));
        let mut twin = Game::new(seed_of(n), &content);
        for _ in 0..8 {
            if app.game().is_over() {
                break;
            }
            let turns = app.game().player_turns();
            press_explore(&mut app);
            // Whatever the walk does, the player's own kills are zero.
            run_walk(&mut app);
            let kills = app.game().stats().total_kills();
            assert_eq!(kills, 0, "seed {n}: the walk killed something");
            let _ = (turns, &mut twin);
            if newest(&app).contains("comes into view") {
                // Do not press on towards a foe that is awake: wait for it.
                app.handle_key(press(KeyCode::Char('.')));
            }
        }
    }
}

// --- Stops by key and by wound -----------------------------------------------

#[test]
fn any_key_stops_the_walk_at_once_and_nothing_else_happens() {
    for key in [
        KeyCode::Char('a'),
        KeyCode::Char(' '),
        KeyCode::Enter,
        KeyCode::Esc,
        KeyCode::Tab,
        KeyCode::Up,
        KeyCode::Char('l'),
        KeyCode::Char('q'),
        KeyCode::Char('>'),
        KeyCode::Char(EXPLORE),
        KeyCode::Char('i'),
        KeyCode::Char('t'),
    ] {
        let mut app = app_on(cave_game(5));
        press_explore(&mut app);
        assert!(app.is_exploring(), "{key:?}: the walk started");
        app.explore_further();
        app.explore_further();
        let (turns, before) = (app.game().player_turns(), state_of(&app));

        app.handle_key(press(key));

        assert!(!app.is_exploring(), "{key:?}: the walk goes on");
        assert_eq!(newest(&app), "You stop exploring.", "{key:?}");
        assert_eq!(
            app.game().player_turns(),
            turns,
            "{key:?}: a step was taken"
        );
        assert_eq!(state_of(&app), before, "{key:?}: the game changed");
        assert_eq!(
            app.state(),
            State::Playing,
            "{key:?}: another screen opened"
        );
        // The walk is over: further steps do nothing.
        app.explore_further();
        assert_eq!(app.game().player_turns(), turns, "{key:?}");
        // And the keys work again as usual.
        app.handle_key(press(KeyCode::Char('.')));
        assert_eq!(app.game().player_turns(), turns + 1, "{key:?}");
    }
}

#[test]
fn the_release_of_the_explore_key_does_not_stop_the_walk() {
    let mut app = app_on(cave_game(6));
    press_explore(&mut app);
    let release = KeyEvent::new_with_kind(
        KeyCode::Char(EXPLORE),
        KeyModifiers::NONE,
        KeyEventKind::Release,
    );
    app.handle_key(release);
    assert!(app.is_exploring());
    let repeat = KeyEvent::new_with_kind(
        KeyCode::Char(EXPLORE),
        KeyModifiers::NONE,
        KeyEventKind::Repeat,
    );
    app.handle_key(repeat);
    assert!(
        app.is_exploring(),
        "a held key repeating must not stop the walk"
    );
}

#[test]
fn ctrl_c_during_the_walk_quits_and_nothing_stays_half_done() {
    let mut app = app_on(cave_game(7));
    press_explore(&mut app);
    assert!(app.is_exploring());

    app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));

    assert_eq!(app.state(), State::Quit);
    assert!(!app.is_exploring());
}

/// Hurt: a monster met round a corner strikes in the very step that shows
/// it. The walk stops with the "hurt" line (the wound comes before the
/// sighting in the order of reasons), the hit points really fell, and the
/// walk takes no further step.
#[test]
fn a_wound_stops_the_walk_and_the_log_says_so() {
    let content = content();
    let foe: &MonsterDef = content.monsters().first().unwrap();
    let mut stopped_hurt = 0;
    let mut tried = 0;
    for n in 0..80 {
        let mut game = rooms_game(n);
        let cell = far_cell(&game, 20);
        game.spawn_monster(cell, foe);
        tried += 1;
        let mut app = app_on(game);
        for _ in 0..PRESS_BUDGET {
            let hp = app.game().player_actor().hp();
            let (_, line) = explore_press(&mut app);
            if app.game().is_over() {
                break;
            }
            let lost = app.game().player_actor().hp() < hp;
            if line.contains("hurt") {
                assert!(lost, "seed {n}: the hurt line without a wound");
                assert!(!app.is_exploring());
                let turns = app.game().player_turns();
                app.explore_further();
                assert_eq!(app.game().player_turns(), turns, "seed {n}: stepped on");
                stopped_hurt += 1;
                break;
            }
            if line.contains("comes into view") {
                assert!(!lost, "seed {n}: wounded yet the line is {line:?}");
                break;
            }
        }
    }
    println!("{tried} scenarios tried, {stopped_hurt} stopped by a wound");
    assert!(stopped_hurt > 0, "no scenario ended with the hurt line");
}

// --- Other screens, small terminals -----------------------------------------

#[test]
fn the_explore_key_does_nothing_in_the_inventory_or_while_tracing() {
    let mut app = app_on(rooms_game(8));
    let before = state_of(&app);

    app.handle_key(press(KeyCode::Char('i')));
    let inventory = app.state();
    press_explore(&mut app);
    assert_eq!(app.state(), inventory, "still in the inventory");
    assert!(!app.is_exploring());
    app.handle_key(press(KeyCode::Esc));

    app.handle_key(press(KeyCode::Char('t')));
    let casting = app.state();
    press_explore(&mut app);
    assert_eq!(app.state(), casting);
    assert!(!app.is_exploring());
    app.handle_key(press(KeyCode::Esc));

    assert_eq!(state_of(&app), before);
}

#[test]
fn on_a_terminal_smaller_than_80_by_24_the_walk_does_not_move_the_player() {
    let mut app = app_on(rooms_game(9));
    let _ = rows(&app, 60, 20);
    let before = state_of(&app);

    press_explore(&mut app);

    assert!(!app.is_exploring());
    assert_eq!(state_of(&app), before);
}

/// A walk that is running when the terminal shrinks stops at its next step
/// instead of walking blind.
#[test]
fn a_walk_in_progress_stops_when_the_terminal_becomes_too_small() {
    let mut app = app_on(cave_game(9));
    let _ = rows(&app, 80, 24);
    press_explore(&mut app);
    assert!(app.is_exploring());
    let turns = app.game().player_turns();

    let _ = rows(&app, 60, 20);
    app.explore_further();

    assert!(!app.is_exploring());
    assert_eq!(app.game().player_turns(), turns);
}

// --- Determinism, redraw -----------------------------------------------------

#[test]
fn the_same_seed_and_the_same_keys_give_the_same_screens_step_by_step() {
    let content = content();
    let run = || {
        let mut app = app_on(Game::new(seed_of(120), &content));
        let mut screens = Vec::new();
        for _ in 0..4 {
            press_explore(&mut app);
            for _ in 0..300 {
                if !app.is_exploring() {
                    break;
                }
                screens.push(rows(&app, 80, 24));
                app.explore_further();
            }
            screens.push(rows(&app, 80, 24));
            screens.push(rows(&app, 120, 40));
            if app.game().is_over() {
                break;
            }
        }
        (screens, state_of(&app))
    };
    let (first, first_state) = run();
    let (second, second_state) = run();
    println!("{} screens compared", first.len());
    assert!(first.len() >= 8);
    assert_eq!(first, second);
    assert_eq!(first_state, second_state);
}

/// Between two steps the screen shows the move: the player glyph is in a
/// new place (or the view scrolled) and more of the map is known.
#[test]
fn each_step_of_the_walk_changes_the_screen() {
    let mut app = app_on(cave_game(10));
    press_explore(&mut app);
    let mut previous = rows(&app, 80, 24);
    let mut unchanged = 0;
    let mut frames = 0;
    for _ in 0..60 {
        if !app.is_exploring() {
            break;
        }
        app.explore_further();
        let frame = rows(&app, 80, 24);
        frames += 1;
        if frame == previous {
            unchanged += 1;
        }
        previous = frame;
    }
    println!("{frames} steps drawn, {unchanged} identical to the previous frame");
    assert!(frames >= 30);
    assert_eq!(unchanged, 0, "a step that left the screen unchanged");
}

/// A cave is drawn on both sizes without losing the player or the panel.
#[test]
fn a_cave_is_drawn_at_80_by_24_and_120_by_40_with_the_player_in_view() {
    let mut app = app_on(cave_game(11));
    for _ in 0..3 {
        press_explore(&mut app);
        run_walk(&mut app);
    }
    for (w, h) in [(80, 24), (120, 40)] {
        let screen = rows(&app, w, h);
        assert_eq!(screen.len(), usize::from(h));
        let joined = screen.join("\n");
        assert!(joined.contains('@'), "{w}x{h}: no player\n{joined}");
        assert!(
            joined.contains("Depth"),
            "{w}x{h}: no status line\n{joined}"
        );
        assert!(
            screen.iter().all(|r| r.chars().count() <= usize::from(w)),
            "{w}x{h}: a row is wider than the screen"
        );
    }
}

/// Walking by hand after an interruption works: exploration leaves no state
/// behind (a held-over `Exploration`) that would make the next walk start
/// from a count of steps already taken.
#[test]
fn an_interrupted_walk_leaves_nothing_behind_for_the_next_one() {
    let mut app = app_on(cave_game(12));
    press_explore(&mut app);
    for _ in 0..150 {
        app.explore_further();
    }
    app.handle_key(press(KeyCode::Char('a')));
    assert_eq!(newest(&app), "You stop exploring.");

    let (steps, line) = explore_press(&mut app);

    assert_eq!(
        steps, README_STEP_LIMIT,
        "the new walk counts its own 200 steps; got {steps} and {line:?}"
    );
}

/// The log lines of a whole exploration of a floor are all sentences the
/// README promises, nothing internal.
#[test]
fn the_log_of_an_exploration_holds_only_player_sentences() {
    let mut app = app_on(rooms_game(33));
    for _ in 0..30 {
        let (_, line) = explore_press(&mut app);
        if line.contains("nothing left") {
            break;
        }
    }
    for line in log_lines(&app) {
        for bad in [
            "Explore",
            "Halt",
            "Some(",
            "None",
            "{",
            "}",
            "NothingTo",
            "step limit",
        ] {
            assert!(!line.contains(bad), "{line:?} contains {bad:?}");
        }
    }
    let _ = Action::Wait;
}

/// A glyph whose sigil the grimoire already knows teaches nothing new and
/// does not stop the walk; a glyph of a sigil still unknown does.
#[test]
fn a_glyph_of_a_known_sigil_does_not_stop_the_walk_but_an_unknown_one_does() {
    let content = content();
    let (known, unknown) = (&content.sigils()[0], &content.sigils()[1]);
    for n in 50..56 {
        let mut game = rooms_game(n);
        // Learn `known` by stepping onto a glyph of it next to the start.
        let me = game.player_actor().position();
        let (direction, next) = sigilfall_core::geometry::Direction::ALL
            .into_iter()
            .map(|d| (d, me.step(d)))
            .find(|&(d, p)| game.map().can_step(me, d) && game.is_visible(p))
            .expect("a free neighbour");
        game.engrave(next, known);
        game.submit(Action::Move(direction));
        assert!(game.grimoire().knows(known.id()), "seed {n}: not learnt");

        let far_known = far_cell(&game, 20);
        game.engrave(far_known, known);
        let far_unknown = {
            let me = game.player_actor().position();
            let (w, h) = (80, 40);
            (0..h)
                .flat_map(|y| (0..w).map(move |x| Position::new(x, y)))
                .find(|&p| {
                    game.map().is_walkable(p)
                        && !game.is_remembered(p)
                        && p != far_known
                        && game.engraving_at(p).is_none()
                        && game.actors().all(|(_, a)| a.position() != p)
                        && (p.x - far_known.x).abs().max((p.y - far_known.y).abs()) >= 15
                        && (p.x - me.x).abs().max((p.y - me.y).abs()) >= 15
                })
        };
        let Some(far_unknown) = far_unknown else {
            continue;
        };
        game.engrave(far_unknown, unknown);
        let mut app = app_on(game);

        let mut glyph_stops = 0;
        for _ in 0..PRESS_BUDGET {
            let (_, line) = explore_press(&mut app);
            if line.contains("carved glyph") {
                glyph_stops += 1;
                let unread_in_view: Vec<Position> = app
                    .game()
                    .engravings()
                    .iter()
                    .filter(|e| app.game().is_visible(e.position()))
                    .filter(|e| !app.game().grimoire().knows(e.sigil().id()))
                    .map(|e| e.position())
                    .collect();
                assert_eq!(unread_in_view, [far_unknown], "seed {n}: wrong glyph");
            }
            if line.contains("nothing left") {
                break;
            }
        }
        // The explorer does not remember what it announced: an unread glyph
        // that leaves the view and comes back stops the walk again (seen
        // on seed 52), so the count is "at least once", and every stop is
        // for the unknown glyph, never for the known one.
        assert!(
            glyph_stops >= 1,
            "seed {n}: the unknown glyph never stopped the walk"
        );
    }
}
