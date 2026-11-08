//! Automatic exploration driven by simulated keys (T29): `x` takes a first
//! step at once, the walk goes on step by step and stops on a monster, an
//! item or an unread glyph coming into view, on a key press, at its step
//! limit or when nothing is left to explore, with a line in the log each
//! time; a monster already in view keeps the player still, and generated
//! floors are explored in a bounded number of presses.

use sigilfall_core::path::flood_fill;

use super::*;
use crate::app::exploring::MAX_EXPLORE_STEPS;
use crate::input::EXPLORE_KEY;
use crate::message_log::Tone;

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
