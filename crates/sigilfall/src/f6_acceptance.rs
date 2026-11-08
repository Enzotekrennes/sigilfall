//! Acceptance tests of F6 (sigils) on the whole application.
//!
//! They drive [`App`] with simulated keys and render it into ratatui's
//! `TestBackend`, as a player would use it, and check the five acceptance
//! scenarios of F6 on the screen and in the log: reading a glyph, an exact
//! trace, a trace one node off, an unknown trace and a cancelled one.
//! Beyond them they check that the time shown while tracing is the time the
//! game charges, that the grimoire shown is the game's, that a long cast
//! gives the monsters more turns, that nothing is submitted for an empty
//! trace, that a replay is identical, and that the screens stay readable at
//! 80 × 24. The fixes of the F6 test are checked too: a backlash that takes
//! nothing is never told as a loss of 0, the side panel names the five
//! shipped sigils, and Escape never quits the game.
//!
//! Expected values are written out here (costs of 25 per node and 50 per
//! cast, 2 essence per cast, wording of the log and of the overlay) instead
//! of being read from the production modules, so that a silent change of a
//! rule or of a text is noticed. Every loop is bounded.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use sigilfall_core::action::Action;
use sigilfall_core::content::{Content, Effect, SigilDef, Targeting};
use sigilfall_core::game::{Game, PLAYER_START};
use sigilfall_core::generation::FIRST_DEPTH;
use sigilfall_core::geometry::{Direction, Position};
use sigilfall_core::rng::{Purpose, Seed};
use sigilfall_core::sigil::{Backlash, draw_backlash};

use crate::app::{App, State};
use crate::casting::Casting;
use crate::theme::{ColorMode, GlyphSet, Theme};

/// The monsters of the tests: the gnat has 999 hit points and deals 1 per
/// blow; the sloth acts once per hundred actions of the player.
const MONSTERS: &str = r#"[
    Monster(id: "gnat", name: "cave gnat", family: Vermin, elite: false,
        speed: 100, hp: 999, attack: 0, defense: 0, min_depth: 1),
    Monster(id: "sloth", name: "sloth", family: Vermin, elite: false,
        speed: 1, hp: 999, attack: 0, defense: 0, min_depth: 1),
    Monster(id: "moth", name: "wax moth", family: Vermin, elite: false,
        speed: 100, hp: 1, attack: 0, defense: 0, min_depth: 1),
]"#;

/// Where a monster stands to be in sight, not adjacent: 4 east, 2 up.
const FOE_CELL: Position = Position::new(9, 3);

/// Seed of the tests whose seed does not matter.
const SEED: Seed = Seed::new(42);

/// The five shipped sigils, in file order.
const ALL: [&str; 5] = ["ember", "recall", "veil", "passage", "lance"];

/// Energy per stroke, energy per cast, essence per cast and essence at most.
const STROKE: i32 = 25;
const CAST: i32 = 50;
const ESSENCE_COST: i32 = 2;

/// The seeds named by the brief.
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

/// The shipped sigils and items with the monsters of [`MONSTERS`].
fn content() -> Content {
    let sigils = include_str!("../../sigilfall-core/content/sigils.ron");
    let items = include_str!("../../sigilfall-core/content/items.ron");
    Content::parse_all(MONSTERS, sigils, items).expect("valid test content")
}

/// The shipped sigil `id`.
fn sigil(id: &str) -> SigilDef {
    content().sigil(id).expect("a shipped sigil").clone()
}

/// A key press of `code`.
fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

/// Presses each character of `keys` in order.
fn keys(app: &mut App, keys: &str) {
    for key in keys.chars() {
        app.handle_key(press(KeyCode::Char(key)));
    }
}

/// Opens the overlay and presses the digits of `digits`.
fn trace(app: &mut App, digits: &str) {
    keys(app, "t");
    keys(app, digits);
}

/// Presses Enter.
fn enter(app: &mut App) {
    app.handle_key(press(KeyCode::Enter));
}

/// A game in the fixed room where the player has read the glyphs of the
/// shipped sigils `ids` (engraved in a row east of the start) and walked
/// back to the start; with a monster `foe` ("gnat", ...) at [`FOE_CELL`] if
/// given. Nothing is logged: the reading happened before the application
/// started.
fn game_knowing(seed: Seed, ids: &[&str], foe: Option<&str>) -> Game {
    let content = content();
    let mut game = Game::in_fixed_room(seed);
    let mut cell = PLAYER_START;
    for id in ids {
        cell = cell.step(Direction::East);
        game.engrave(cell, &sigil(id));
    }
    for _ in ids {
        game.submit(Action::Move(Direction::East));
    }
    for _ in ids {
        game.submit(Action::Move(Direction::West));
    }
    assert_eq!(game.grimoire().len(), ids.len(), "every glyph was read");
    if let Some(foe) = foe {
        game.spawn_monster(FOE_CELL, content.monster(foe).expect("a test monster"));
    }
    game
}

/// An application on [`game_knowing`].
fn app_knowing(seed: Seed, ids: &[&str], foe: Option<&str>) -> App {
    App::with_game(dark(), game_knowing(seed, ids, foe), content())
}

/// The whole screen of `app` at `width` × `height`, as text.
fn screen_at(app: &App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
    terminal.backend().to_string()
}

/// The screen of `app` at 80 × 24, without the quotes and line ends of the
/// buffer's debug text, one string per row.
fn rows(app: &App) -> Vec<String> {
    screen_at(app, 80, 24)
        .lines()
        .map(|line| line.trim_matches('"').to_owned())
        .collect()
}

/// The screen of `app` at 80 × 24, rows joined by newlines.
fn text(app: &App) -> String {
    rows(app).join("\n")
}

/// The texts of the log, oldest first.
fn log(app: &App) -> Vec<String> {
    let mut texts: Vec<String> = app.log().newest_first().map(|m| m.text.clone()).collect();
    texts.reverse();
    texts
}

/// The newest text of the log.
fn last_log(app: &App) -> String {
    app.log()
        .newest_first()
        .next()
        .expect("a message")
        .text
        .clone()
}

/// The player's energy and the ticks elapsed, to measure a cost later.
fn clock(app: &App) -> (i32, u64) {
    (app.game().player_actor().energy(), app.game().ticks())
}

/// Energy the player spent since `before` (see [`clock`]).
fn spent(app: &App, before: (i32, u64)) -> i32 {
    let (energy, ticks) = before;
    let gained = i32::try_from(app.game().ticks() - ticks).unwrap() * 100;
    energy + gained - app.game().player_actor().energy()
}

/// What the game shows of the player and the clock, to check that nothing
/// happened.
fn world(app: &App) -> (u64, u64, i32, i32, i32, Position, usize) {
    let player = app.game().player_actor();
    (
        app.game().ticks(),
        app.game().player_turns(),
        player.energy(),
        player.essence(),
        player.hp(),
        player.position(),
        app.log().len(),
    )
}

/// The price of a cast of `strokes` strokes, from decision D19.
const fn price(strokes: i32) -> i32 {
    strokes * STROKE + CAST
}

/// The digits of the trace being drawn, or `None` when the overlay is not
/// on the drawing step.
fn drawn(app: &App) -> Option<Vec<u8>> {
    match app.state() {
        State::Casting(Casting::Drawing(drawing)) => {
            Some(drawing.trace().nodes().iter().map(|n| n.digit()).collect())
        }
        _ => None,
    }
}

/// The essence shown by the side panel, "4" for "Essence    4/6".
fn panel_essence(app: &App) -> Option<i32> {
    rows(app).iter().find_map(|row| {
        let at = row.find("Essence")?;
        let rest = row[at + "Essence".len()..].trim_start();
        let value = rest.split('/').next()?.trim();
        value.parse().ok()
    })
}

/// The time shown by the overlay, "1.25" for "Time     1.25 turns".
fn displayed_turns(app: &App) -> Option<String> {
    rows(app).iter().find_map(|row| {
        let at = row.find("Time ")?;
        let rest = row[at + "Time".len()..].trim_start();
        let value = rest.split(" turns").next()?;
        value
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.')
            .then(|| value.to_owned())
    })
}

/// A value in turns, "1.25", as hundredths of an energy step.
fn energy_of(turns: &str) -> i32 {
    let (whole, hundredths) = turns.split_once('.').expect("two decimals");
    whole.parse::<i32>().unwrap() * 100 + hundredths.parse::<i32>().unwrap()
}

// --- Scenario 1: reading a glyph --------------------------------------------

#[test]
fn reading_a_glyph_announces_it_and_adds_the_sigil_to_the_grimoire() {
    for seed in seeds() {
        let mut game = Game::in_fixed_room(seed);
        game.engrave(PLAYER_START.step(Direction::East), &sigil("ember"));
        let mut app = App::with_game(dark(), game, content());

        let before = text(&app);
        assert!(before.contains("SIGILS"), "{before}");
        assert!(before.contains("none yet"), "{before}");
        assert!(
            before.contains("carved glyph"),
            "the unread glyph is listed"
        );

        app.handle_key(press(KeyCode::Right));

        assert_eq!(
            last_log(&app),
            "You decipher a carved glyph: Ember joins your grimoire.",
            "seed {seed}"
        );
        let after = text(&app);
        assert!(after.contains("1/5"), "the side panel counts it: {after}");
        assert!(after.contains("Ember"), "{after}");
        assert!(after.contains("known glyph"), "{after}");
        assert!(app.game().grimoire().knows("ember"));

        // The overlay lists it with its pattern and its effect.
        keys(&mut app, "t");
        let overlay = text(&app);
        assert!(
            overlay.contains("Ember  7·5·3  harm 6 · nearest"),
            "{overlay}"
        );
    }
}

#[test]
fn reading_the_same_glyph_again_says_nothing_and_changes_nothing() {
    let mut game = Game::in_fixed_room(SEED);
    game.engrave(PLAYER_START.step(Direction::East), &sigil("ember"));
    let mut app = App::with_game(dark(), game, content());
    app.handle_key(press(KeyCode::Right));
    app.handle_key(press(KeyCode::Left));
    let lines = log(&app).len();

    app.handle_key(press(KeyCode::Right));

    let deciphered = log(&app)
        .iter()
        .filter(|line| line.contains("decipher"))
        .count();
    assert_eq!(deciphered, 1, "announced once: {:?}", log(&app));
    assert_eq!(log(&app).len(), lines, "no new line for a known glyph");
    assert_eq!(app.game().grimoire().len(), 1);
}

#[test]
fn reading_works_in_ascii_and_in_every_colour_mode() {
    let modes = [
        ColorMode::TrueColorDark,
        ColorMode::TrueColorLight,
        ColorMode::Ansi16,
        ColorMode::Monochrome,
    ];
    for mode in modes {
        for glyphs in [GlyphSet::Unicode, GlyphSet::Ascii] {
            let mut game = Game::in_fixed_room(SEED);
            game.engrave(PLAYER_START.step(Direction::East), &sigil("ember"));
            let mut app = App::with_game(Theme::new(mode, glyphs), game, content());
            app.handle_key(press(KeyCode::Right));
            let screen = text(&app);
            assert!(screen.contains("Ember"), "{mode:?} {glyphs:?}: {screen}");
            assert!(
                last_log(&app).contains("Ember joins your grimoire"),
                "{mode:?} {glyphs:?}"
            );
            if glyphs == GlyphSet::Ascii {
                assert!(screen.is_ascii(), "{mode:?}: non-ASCII in ASCII mode");
            }
        }
    }
}

// --- Scenario 2: an exact trace succeeds and every stroke costs time -------

#[test]
fn an_exact_trace_casts_the_sigil_logs_it_and_charges_every_stroke() {
    for seed in seeds() {
        let mut app = app_knowing(seed, &["ember"], Some("gnat"));
        let before = clock(&app);
        let turns = app.game().player_turns();

        trace(&mut app, "753");
        assert_eq!(drawn(&app), Some(vec![7, 5, 3]));
        enter(&mut app);

        assert_eq!(
            app.state(),
            State::Playing,
            "a pure cast closes the overlay"
        );
        assert_eq!(
            log(&app)
                .iter()
                .filter(|line| line.starts_with("Ember, pure trace"))
                .count(),
            1,
            "seed {seed}: {:?}",
            log(&app)
        );
        assert!(
            last_log(&app).contains("cave gnat") || log(&app).iter().any(|l| l.contains("seared")),
            "{:?}",
            log(&app)
        );
        assert_eq!(
            spent(&app, before),
            price(3),
            "seed {seed}: 3 strokes + cast"
        );
        assert_eq!(app.game().player_turns(), turns + 1, "one action");
        assert_eq!(panel_essence(&app), Some(4), "the panel shows the essence");
    }
}

#[test]
fn a_pure_cast_is_announced_with_its_effect_in_the_log() {
    let mut app = app_knowing(SEED, &["ember"], Some("gnat"));
    trace(&mut app, "753");
    enter(&mut app);
    let line = log(&app)
        .into_iter()
        .find(|line| line.starts_with("Ember"))
        .expect("the cast is logged");
    assert_eq!(line, "Ember, pure trace: the cave gnat is seared (6).");
}

#[test]
fn the_time_shown_while_tracing_is_the_time_the_game_charges() {
    // (digits pressed, sigils known, direction key if the trace is aimed)
    let cases: [(&str, Option<char>); 9] = [
        ("753", None),        // ember
        ("8521", None),       // recall
        ("47896", None),      // veil
        ("74123", Some('l')), // passage
        ("1596", Some('l')),  // lance
        ("73", None),         // 7-5-3: a middle node linked on the way
        ("139", None),        // 1-2-3-6-9
        ("987", None),        // unknown
        ("123654789", None),  // the nine nodes
    ];
    for (digits, direction) in cases {
        let mut app = app_knowing(SEED, &ALL, Some("sloth"));
        keys(&mut app, "t");
        // After every key, the time shown is that of the strokes linked.
        for digit in digits.chars() {
            keys(&mut app, &digit.to_string());
            let linked = i32::try_from(drawn(&app).expect("still drawing").len()).unwrap();
            let shown = displayed_turns(&app).expect("a time is shown");
            assert_eq!(
                energy_of(&shown),
                price(linked),
                "{digits}: after {linked} linked nodes the overlay says {shown}"
            );
        }
        let shown = energy_of(&displayed_turns(&app).unwrap());
        let before = clock(&app);
        enter(&mut app);
        if let Some(direction) = direction {
            assert!(
                matches!(app.state(), State::Casting(Casting::Aiming { .. })),
                "{digits}: the direction is asked"
            );
            keys(&mut app, &direction.to_string());
        }
        assert_eq!(spent(&app, before), shown, "{digits}: shown vs charged");
    }
}

#[test]
fn the_time_shown_of_an_empty_trace_is_not_a_cost_because_it_is_never_cast() {
    let mut app = app_knowing(SEED, &ALL, Some("sloth"));
    keys(&mut app, "t");
    let before = world(&app);
    enter(&mut app);
    assert_eq!(world(&app), before, "Enter on an empty trace does nothing");
}

#[test]
fn a_long_cast_leaves_the_monsters_more_blows_than_a_short_one() {
    // The gnat deals exactly 1 per blow; three casts of 5 strokes against
    // three of 3, with the cave gnat next to the player.
    let blows = |digits: &str| {
        let mut game = game_knowing(SEED, &[], None);
        game.spawn_monster(
            PLAYER_START.step(Direction::East),
            content().monster("gnat").unwrap(),
        );
        let mut app = App::with_game(dark(), game, content());
        app.handle_key(press(KeyCode::Char('.')));
        let start = app.game().player_actor().hp();
        for _ in 0..3 {
            trace(&mut app, digits);
            enter(&mut app);
            // The verdict of a fizzle stays until a key is pressed.
            app.handle_key(press(KeyCode::Char(' ')));
            assert_eq!(app.state(), State::Playing);
        }
        start - app.game().player_actor().hp()
    };
    let short = blows("987");
    let long = blows("98745");
    assert!(long > short, "5 strokes: {long} blows, 3 strokes: {short}");
}

// --- Scenario 3: one node off is unstable -----------------------------------

/// The three backlash lines of the log, with what each does to the game.
const BACKLASH_LINES: [&str; 3] = [
    "The backlash scorches you (2).",
    "The backlash dazes you: you lose 1 turn.",
    "The backlash drains your essence (4).",
];

#[test]
fn a_trace_one_node_off_is_unstable_and_the_log_gives_effect_and_backlash() {
    let mut seen = std::collections::HashSet::new();
    for value in 0..45 {
        let seed = Seed::new(value);
        let mut app = app_knowing(seed, &["ember"], Some("sloth"));
        let hp = app.game().player_actor().hp();

        trace(&mut app, "752");
        enter(&mut app);

        let State::Casting(Casting::Verdict(_)) = app.state() else {
            panic!("seed {seed}: no verdict shown: {:?}", app.state());
        };
        let screen = text(&app);
        for expected in [
            "Unstable trace",
            "UNSTABLE",
            "Ember, one node off",
            "expected  7·5·3",
            "traced    7·5·2",
            "The magic lashes back.",
        ] {
            assert!(
                screen.contains(expected),
                "seed {seed}: {expected}\n{screen}"
            );
        }
        let lines = log(&app);
        let cast = lines
            .iter()
            .find(|line| line.starts_with("Ember, unstable trace"))
            .unwrap_or_else(|| panic!("seed {seed}: no unstable line in {lines:?}"));
        assert!(cast.contains("seared (6)"), "the effect applied: {cast}");
        let backlashes: Vec<&String> = lines
            .iter()
            .filter(|line| BACKLASH_LINES.contains(&line.as_str()))
            .collect();
        assert_eq!(backlashes.len(), 1, "seed {seed}: {lines:?}");
        let player = app.game().player_actor();
        match backlashes[0].as_str() {
            "The backlash scorches you (2)." => assert_eq!(player.hp(), hp - 2),
            "The backlash drains your essence (4)." => assert_eq!(player.essence(), 0),
            _ => assert!(player.held_turns() <= 1),
        }
        seen.insert(backlashes[0].clone());

        // Any key closes the verdict and costs nothing.
        let before = world(&app);
        app.handle_key(press(KeyCode::Char('x')));
        assert_eq!(app.state(), State::Playing);
        assert_eq!(world(&app), before, "closing the verdict passes no time");
    }
    assert_eq!(seen.len(), 3, "45 seeds show every backlash: {seen:?}");
}

#[test]
fn every_one_off_trace_of_every_sigil_is_unstable_never_failed() {
    // One node replaced, one dropped, one added: for each shipped sigil.
    let near: [(&str, [&str; 3]); 3] = [
        ("recall", ["8529", "852", "85216"]),
        ("veil", ["47893", "4789", "478963"]),
        ("ember", ["752", "75", "7536"]),
    ];
    for (id, traces) in near {
        for digits in traces {
            let mut app = app_knowing(SEED, &[id], Some("sloth"));
            trace(&mut app, digits);
            enter(&mut app);
            let State::Casting(Casting::Verdict(verdict)) = app.state() else {
                panic!("{id} {digits}: no verdict");
            };
            assert_eq!(
                verdict.quality,
                sigilfall_core::sigil::CastQuality::Unstable,
                "{id} {digits}"
            );
            let screen = text(&app);
            assert!(screen.contains("UNSTABLE"), "{id} {digits}\n{screen}");
        }
    }
}

// --- Scenario 4: an unknown trace fizzles -----------------------------------

#[test]
fn an_unknown_trace_fizzles_costs_the_essence_and_the_log_says_so() {
    for seed in seeds() {
        let mut app = app_knowing(seed, &ALL, Some("sloth"));
        let before = clock(&app);
        let foe_hp = app
            .game()
            .actors()
            .filter(|(_, actor)| !actor.is_player())
            .map(|(_, actor)| actor.hp())
            .collect::<Vec<_>>();

        trace(&mut app, "24681");
        enter(&mut app);

        let State::Casting(Casting::Verdict(_)) = app.state() else {
            panic!("seed {seed}: no verdict: {:?}", app.state());
        };
        let screen = text(&app);
        for expected in [
            "Failed trace",
            "FAILED",
            "no known pattern",
            "traced    2·4·5·6·8·1",
            "The spell fizzles.",
        ] {
            assert!(
                screen.contains(expected),
                "seed {seed}: {expected}\n{screen}"
            );
        }
        assert_eq!(
            last_log(&app),
            "The trace matches no sigil you know and fizzles.",
            "seed {seed}"
        );
        assert_eq!(panel_essence(&app), Some(4), "the essence is lost");
        assert_eq!(
            spent(&app, before),
            price(6),
            "the time too: 4 to 6 links 5"
        );
        let after = app
            .game()
            .actors()
            .filter(|(_, actor)| !actor.is_player())
            .map(|(_, actor)| actor.hp())
            .collect::<Vec<_>>();
        assert_eq!(after, foe_hp, "nothing was hurt");
    }
}

#[test]
fn a_valid_sigil_not_learnt_yet_fizzles_instead_of_casting() {
    for seed in seeds() {
        // Only ember is known; the exact pattern of veil is traced.
        let mut app = app_knowing(seed, &["ember"], Some("sloth"));
        trace(&mut app, "47896");
        enter(&mut app);
        let State::Casting(Casting::Verdict(verdict)) = app.state() else {
            panic!("seed {seed}: no verdict");
        };
        assert_eq!(
            verdict.quality,
            sigilfall_core::sigil::CastQuality::Fizzled,
            "seed {seed}"
        );
        assert!(text(&app).contains("FAILED"));
        assert!(!log(&app).iter().any(|line| line.contains("Veil")));

        // Nothing known at all: ember exact and ember one off both fizzle.
        for digits in ["753", "752"] {
            let mut app = app_knowing(seed, &[], Some("sloth"));
            trace(&mut app, digits);
            enter(&mut app);
            assert!(
                matches!(
                    app.state(),
                    State::Casting(Casting::Verdict(v))
                        if v.quality == sigilfall_core::sigil::CastQuality::Fizzled
                ),
                "seed {seed} {digits}"
            );
        }
    }
}

// --- Scenario 5: cancelling costs nothing -----------------------------------

#[test]
fn cancelling_a_trace_in_progress_passes_no_time_at_all() {
    for seed in seeds() {
        let mut app = app_knowing(seed, &ALL, Some("gnat"));
        let before = world(&app);

        trace(&mut app, "7539");
        assert!(matches!(app.state(), State::Casting(_)));
        app.handle_key(press(KeyCode::Esc));

        assert_eq!(
            app.state(),
            State::Playing,
            "seed {seed}: one Escape leaves"
        );
        assert_eq!(world(&app), before, "seed {seed}: nothing changed");
        // The next trace starts empty.
        keys(&mut app, "t");
        assert_eq!(drawn(&app), Some(vec![]), "seed {seed}");
    }
}

#[test]
fn cancelling_at_every_step_costs_nothing() {
    let mut app = app_knowing(SEED, &ALL, Some("gnat"));
    let before = world(&app);

    // Right after opening.
    keys(&mut app, "t");
    app.handle_key(press(KeyCode::Esc));
    assert_eq!(world(&app), before);

    // Back to the trace from the direction prompt, then out.
    trace(&mut app, "74123");
    enter(&mut app);
    assert!(matches!(
        app.state(),
        State::Casting(Casting::Aiming { .. })
    ));
    app.handle_key(press(KeyCode::Esc));
    assert_eq!(drawn(&app), Some(vec![7, 4, 1, 2, 3]), "the trace is kept");
    assert_eq!(world(&app), before);
    app.handle_key(press(KeyCode::Esc));
    assert_eq!(app.state(), State::Playing);
    assert_eq!(world(&app), before);

    // Undone to nothing, then out.
    trace(&mut app, "75");
    app.handle_key(press(KeyCode::Backspace));
    app.handle_key(press(KeyCode::Backspace));
    assert_eq!(drawn(&app), Some(vec![]));
    app.handle_key(press(KeyCode::Esc));
    assert_eq!(world(&app), before);
}

#[test]
fn an_empty_trace_is_never_submitted() {
    for seed in seeds() {
        let mut app = app_knowing(seed, &ALL, Some("gnat"));
        let before = world(&app);

        keys(&mut app, "t");
        enter(&mut app);
        enter(&mut app);

        assert_eq!(drawn(&app), Some(vec![]), "seed {seed}: still drawing");
        assert_eq!(
            world(&app),
            before,
            "seed {seed}: nothing cast, nothing logged"
        );
        assert!(
            text(&app).contains("Link at least one node first."),
            "the overlay explains why"
        );

        // A trace undone to nothing is no different.
        keys(&mut app, "753");
        for _ in 0..3 {
            app.handle_key(press(KeyCode::Backspace));
        }
        enter(&mut app);
        assert_eq!(world(&app), before, "seed {seed}: undone trace");
        assert_eq!(drawn(&app), Some(vec![]));
    }
}

#[test]
fn a_node_pressed_twice_is_refused_with_a_notice_and_costs_nothing() {
    let mut app = app_knowing(SEED, &ALL, Some("gnat"));
    let before = world(&app);
    trace(&mut app, "77");
    assert_eq!(drawn(&app), Some(vec![7]));
    assert!(text(&app).contains("Node 7 is already linked."));
    assert_eq!(world(&app), before);
}

// --- Essence ----------------------------------------------------------------

#[test]
fn without_essence_the_cast_is_refused_in_the_log_and_costs_no_time() {
    for seed in seeds() {
        let mut app = app_knowing(seed, &["ember"], Some("sloth"));
        // Fizzle until the reserve is below a cast.
        for _ in 0..4 {
            if app.game().player_actor().essence() < ESSENCE_COST {
                break;
            }
            trace(&mut app, "987");
            enter(&mut app);
            app.handle_key(press(KeyCode::Char(' ')));
        }
        let essence = app.game().player_actor().essence();
        assert!(essence < ESSENCE_COST, "seed {seed}: essence {essence}");

        // The overlay warns before the trace is finished.
        keys(&mut app, "t");
        keys(&mut app, "753");
        let warning = format!("Not enough essence: {essence} of 2.");
        assert!(text(&app).contains(&warning), "{}", text(&app));

        let before = world(&app);
        enter(&mut app);

        assert_eq!(
            last_log(&app),
            format!("Not enough essence to cast: {essence} of 2."),
            "seed {seed}"
        );
        assert_eq!(app.state(), State::Playing, "no verdict for a refusal");
        let after = world(&app);
        assert_eq!(
            (after.0, after.1, after.2, after.3, after.4, after.5),
            (before.0, before.1, before.2, before.3, before.4, before.5),
            "seed {seed}: no tick, no turn, no energy, no essence"
        );
    }
}

// --- Aimed sigils -----------------------------------------------------------

/// An application where the player stands on (10, 5) knowing `id`.
fn app_in_the_middle(id: &str) -> App {
    let mut game = Game::in_fixed_room(SEED);
    game.engrave(Position::new(10, 5), &sigil(id));
    for _ in 0..5 {
        game.submit(Action::Move(Direction::East));
    }
    assert_eq!(game.player_actor().position(), Position::new(10, 5));
    App::with_game(dark(), game, content())
}

/// The eight movement keys with the direction each points to.
const AIM_KEYS: [(char, Direction); 8] = [
    ('k', Direction::North),
    ('u', Direction::NorthEast),
    ('l', Direction::East),
    ('n', Direction::SouthEast),
    ('j', Direction::South),
    ('b', Direction::SouthWest),
    ('h', Direction::West),
    ('y', Direction::NorthWest),
];

#[test]
fn passage_asks_for_a_direction_and_blinks_four_cells_that_way() {
    for (key, direction) in AIM_KEYS {
        let mut app = app_in_the_middle("passage");
        let before = clock(&app);
        let mut target = Position::new(10, 5);
        for _ in 0..4 {
            target = target.step(direction);
        }

        trace(&mut app, "74123");
        enter(&mut app);

        assert!(
            matches!(app.state(), State::Casting(Casting::Aiming { .. })),
            "{key}: the direction is asked"
        );
        let screen = text(&app);
        assert!(screen.contains("Aim Passage"), "{screen}");
        assert!(screen.contains("Passage is aimed: choose a direction."));
        assert_eq!(spent(&app, before), 0, "nothing is paid before the aim");

        keys(&mut app, &key.to_string());

        assert_eq!(app.state(), State::Playing, "{key}");
        assert_eq!(app.game().player_actor().position(), target, "{key}");
        assert_eq!(
            last_log(&app),
            "Passage, pure trace: you blink 4 cells.",
            "{key}"
        );
        assert_eq!(spent(&app, before), price(5), "{key}");
    }
}

#[test]
fn lance_strikes_the_first_creature_in_the_direction_chosen() {
    for (key, direction) in AIM_KEYS {
        let mut cell = Position::new(10, 5);
        for _ in 0..3 {
            cell = cell.step(direction);
        }
        // Rebuild the game around a gnat on the line.
        let mut game = Game::in_fixed_room(SEED);
        game.engrave(Position::new(10, 5), &sigil("lance"));
        for _ in 0..5 {
            game.submit(Action::Move(Direction::East));
        }
        let gnat = game.spawn_monster(cell, content().monster("sloth").unwrap());
        let mut app = App::with_game(dark(), game, content());

        trace(&mut app, "1596");
        enter(&mut app);
        keys(&mut app, &key.to_string());

        assert_eq!(
            last_log(&app),
            "Lance, pure trace: the sloth is seared (9).",
            "{key}: {:?}",
            log(&app)
        );
        assert_eq!(app.game().actor(gnat).map(|a| a.hp()), Some(990), "{key}");
    }
}

#[test]
fn non_direction_keys_are_ignored_while_the_direction_is_asked() {
    let mut app = app_in_the_middle("passage");
    trace(&mut app, "74123");
    enter(&mut app);
    let before = world(&app);
    for key in ['x', 'z', '5', 't'] {
        keys(&mut app, &key.to_string());
    }
    enter(&mut app);
    assert!(matches!(
        app.state(),
        State::Casting(Casting::Aiming { .. })
    ));
    assert_eq!(world(&app), before);
}

#[test]
fn a_blink_against_a_wall_is_blocked_but_still_paid() {
    let mut app = app_in_the_middle("passage");
    for _ in 0..9 {
        app.handle_key(press(KeyCode::Char('h')));
    }
    assert_eq!(
        app.game().player_actor().position().x,
        1,
        "against the wall"
    );
    let before = clock(&app);
    trace(&mut app, "74123");
    enter(&mut app);
    keys(&mut app, "h");
    assert_eq!(last_log(&app), "Passage, pure trace: the way is blocked.");
    assert_eq!(spent(&app, before), price(5));
}

// --- The grimoire shown is the game's --------------------------------------

/// The pattern and the effect the grimoire writes for `def`, as the player
/// reads them: "7·5·3" and "harm 6 · nearest".
fn grimoire_parts(def: &SigilDef) -> (String, String) {
    let pattern = def
        .pattern()
        .nodes()
        .iter()
        .map(|n| n.digit().to_string())
        .collect::<Vec<_>>()
        .join("·");
    let verb = match def.effect() {
        Effect::Damage => "harm",
        Effect::Heal => "heal",
        Effect::Blink => "blink",
        Effect::Hold => "hold",
    };
    let target = match def.targeting() {
        Targeting::Caster => "self",
        Targeting::Direction => "aimed",
        Targeting::NearestEnemy => "nearest",
    };
    (pattern, format!("{verb} {} · {target}", def.power()))
}

/// Whether `row` shows `def` as a grimoire entry: its name, then its
/// pattern, then its effect, in that order (the columns are padded).
fn row_shows(row: &str, def: &SigilDef) -> bool {
    let (pattern, effect) = grimoire_parts(def);
    let Some(name_at) = row.find(def.name()) else {
        return false;
    };
    let rest = &row[name_at + def.name().len()..];
    let Some(pattern_at) = rest.find(&pattern) else {
        return false;
    };
    rest[pattern_at + pattern.len()..].contains(&effect)
}

#[test]
fn the_grimoire_shown_while_tracing_is_the_grimoire_of_the_game() {
    let orders: [&[&str]; 5] = [
        &[],
        &["ember"],
        &["lance", "recall"],
        &["veil", "passage", "ember"],
        &ALL,
    ];
    for ids in orders {
        let mut app = app_knowing(SEED, ids, None);
        keys(&mut app, "t");
        let rows = rows(&app);

        let known: Vec<SigilDef> = app.game().grimoire().sigils().to_vec();
        assert_eq!(known.len(), ids.len());
        let mut previous_row = 0;
        for def in &known {
            let row = rows
                .iter()
                .position(|row| row_shows(row, def))
                .unwrap_or_else(|| panic!("{ids:?}: {} missing in\n{}", def.id(), rows.join("\n")));
            assert!(row > previous_row, "{ids:?}: file order");
            previous_row = row;
        }
        // Nothing the player does not know is listed.
        for def in content().sigils() {
            if !known.iter().any(|k| k.id() == def.id()) {
                assert!(
                    !rows.iter().any(|row| row_shows(row, def)),
                    "{ids:?}: {} is shown but unknown",
                    def.id()
                );
            }
        }
        if ids.is_empty() {
            assert!(
                rows.iter().any(|row| row.contains("GRIMOIRE")),
                "the empty grimoire still has its title"
            );
        }
        // The side panel counts the same sigils.
        let panel = format!("{}/5", ids.len());
        assert!(rows.iter().any(|row| row.contains(&panel)), "{ids:?}");
    }
}

// --- Backlash wording at the edges -----------------------------------------

/// The first seed whose first gameplay backlash is `wanted`.
fn seed_with_first_backlash(wanted: Backlash) -> Seed {
    (0..200)
        .map(Seed::new)
        .find(|seed| draw_backlash(&mut seed.stream(FIRST_DEPTH, Purpose::Gameplay)) == wanted)
        .expect("a seed draws it first")
}

#[test]
fn a_backlash_that_takes_nothing_is_not_announced_as_a_loss_of_zero() {
    // At exactly one hit point a scorch is spared (0 taken), and a drain
    // after paying a cast from 2 essence finds nothing left (0 lost).
    let seed = seed_with_first_backlash(Backlash::Drain);
    let mut app = app_knowing(seed, &["ember"], None);
    // Fizzle twice: 6 -> 2 essence (no regeneration in so few ticks).
    for _ in 0..2 {
        trace(&mut app, "987");
        enter(&mut app);
        app.handle_key(press(KeyCode::Char(' ')));
    }
    assert_eq!(app.game().player_actor().essence(), 2);
    // The fizzles drew nothing from the gameplay stream.
    trace(&mut app, "752");
    enter(&mut app);
    let lines = log(&app);
    assert!(
        !lines.iter().any(|line| line.contains("(0)")),
        "a drain of nothing is announced as a loss: {lines:?}"
    );
    // The backlash is still named, so the player knows which one came.
    assert!(
        lines
            .iter()
            .any(|line| line == "The backlash finds no essence to drain."),
        "{lines:?}"
    );
    assert_eq!(app.game().player_actor().essence(), 0);
}

#[test]
fn a_scorch_that_is_spared_at_one_hit_point_is_not_announced_as_zero_damage() {
    // The moth's blows use the gameplay stream, so which backlash comes
    // depends on the seed: run seeds until scorches have been seen at one
    // hit point.
    let mut scorched = 0;
    for value in 0..60 {
        let mut game = game_knowing(Seed::new(value), &["ember"], None);
        game.spawn_monster(
            PLAYER_START.step(Direction::North),
            content().monster("moth").unwrap(),
        );
        let mut app = App::with_game(dark(), game, content());
        for _ in 0..60 {
            if app.game().player_actor().hp() <= 1 {
                break;
            }
            app.handle_key(press(KeyCode::Char('.')));
        }
        assert_eq!(app.game().player_actor().hp(), 1, "seed {value}");
        app.handle_key(press(KeyCode::Char('k')));
        assert!(!app.game().is_over());
        trace(&mut app, "752");
        enter(&mut app);
        assert!(
            !app.game().is_over(),
            "seed {value}: a backlash never kills"
        );
        let lines = log(&app);
        if lines.iter().any(|line| line.contains("scorches you")) {
            scorched += 1;
            assert!(
                !lines.iter().any(|line| line.contains("scorches you (0)")),
                "seed {value}: a spared scorch is announced as damage: {lines:?}"
            );
            assert!(
                lines
                    .iter()
                    .any(|line| line == "The backlash scorches you, but you hold on."),
                "seed {value}: the player is told they were spared: {lines:?}"
            );
            assert_eq!(app.game().player_actor().hp(), 1, "seed {value}");
        }
    }
    assert!(scorched > 0, "no scorch at one hit point was exercised");
}

// --- Readability at 80 x 24 -------------------------------------------------

#[test]
fn every_overlay_state_stays_readable_at_80_by_24() {
    let mut app = app_knowing(SEED, &ALL, Some("sloth"));

    // Playing: five sigils in the panel, a monster and a glyph in view.
    let playing = text(&app);
    assert!(playing.contains("SIGILS"), "{playing}");
    assert!(playing.contains("5/5"));
    // The panel names the sigils it has room for and counts the others: none
    // is dropped without a word.
    let panel: Vec<String> = rows(&app)
        .iter()
        .map(|row| row.chars().skip(58).collect())
        .collect();
    let named = content()
        .sigils()
        .iter()
        .filter(|def| panel.iter().any(|row| row.contains(def.name())))
        .count();
    let hidden: usize = panel
        .iter()
        .find_map(|row| {
            let rest = row
                .trim_matches(|c: char| c == '│' || c == ' ')
                .strip_prefix("and ")?;
            rest.strip_suffix(" more")?.parse().ok()
        })
        .unwrap_or(0);
    assert_eq!(
        named + hidden,
        5,
        "named {named}, hidden {hidden}\n{playing}"
    );
    assert!(playing.contains("Ember"), "{playing}");
    assert!(
        playing.contains("IN VIEW"),
        "the monster list survives\n{playing}"
    );
    assert!(playing.contains("sloth"), "{playing}");
    assert!(playing.contains("Essence"));
    assert!(
        playing.contains("t trace"),
        "the footer keeps the trace key"
    );

    // Drawing the nine nodes.
    trace(&mut app, "123654789");
    let nine = text(&app);
    assert!(nine.contains("Trace a sigil"), "{nine}");
    assert!(nine.contains("1·2·3·6·5·4·7·8·9"), "{nine}");
    assert!(nine.contains("2.75 turns"), "{nine}");
    assert!(nine.contains("Essence  6/6 · 2 a cast"), "{nine}");
    assert!(nine.contains("Enter cast"), "the footer explains the keys");
    for name in ["Ember", "Recall", "Veil", "Passage", "Lance"] {
        assert!(nine.contains(name), "{name} hidden by the overlay\n{nine}");
    }
    assert!(nine.contains("Log"), "the log stays in view");

    // Aiming.
    let mut app = app_knowing(SEED, &ALL, Some("sloth"));
    trace(&mut app, "74123");
    enter(&mut app);
    let aim = text(&app);
    assert!(aim.contains("Aim Passage"), "{aim}");
    assert!(aim.contains("Esc"), "{aim}");

    // Unstable and failed verdicts, with five sigils in the panel.
    let mut app = app_knowing(SEED, &ALL, Some("sloth"));
    trace(&mut app, "8529");
    enter(&mut app);
    let unstable = text(&app);
    assert!(unstable.contains("Recall, one node off"), "{unstable}");
    assert!(
        unstable.contains("continue"),
        "the footer says how to go on"
    );
    let mut app = app_knowing(SEED, &ALL, Some("sloth"));
    trace(&mut app, "24681");
    enter(&mut app);
    let failed = text(&app);
    assert!(failed.contains("FAILED"), "{failed}");
    assert!(failed.contains("continue"), "{failed}");

    // The log under the overlay keeps the last lines of the cast.
    assert!(
        failed.contains("fizzles"),
        "the log line of the fizzle is visible\n{failed}"
    );
}

#[test]
fn the_panel_names_the_five_shipped_sigils_at_80_by_24() {
    // B3: with a short "in view" list (the player, a monster and the
    // glyphs), every shipped sigil is named in the side panel, in grimoire
    // order, and none is replaced by "and N more".
    for seed in seeds() {
        let app = app_knowing(seed, &ALL, Some("sloth"));
        let panel: Vec<String> = rows(&app)
            .iter()
            .map(|row| {
                row.chars()
                    .skip(58)
                    .collect::<String>()
                    .trim_matches(|c: char| c == '│' || c == ' ')
                    .to_owned()
            })
            .collect();
        let title = panel
            .iter()
            .position(|row| row.starts_with("SIGILS"))
            .unwrap_or_else(|| panic!("seed {seed}: no SIGILS block in {panel:#?}"));
        assert!(panel[title].ends_with("5/5"), "{panel:#?}");
        assert_eq!(
            panel[title + 1..title + 6],
            ["Ember", "Recall", "Veil", "Passage", "Lance"],
            "seed {seed}: {panel:#?}"
        );
        assert!(
            !panel.iter().any(|row| row.ends_with(" more")),
            "seed {seed}: {panel:#?}"
        );
        // The list under them still names what is in view.
        let in_view = panel.iter().position(|row| row == "IN VIEW").unwrap();
        assert_eq!(panel[in_view + 1], "@ you", "{panel:#?}");
        assert!(panel[in_view..].iter().any(|row| row.contains("sloth")));
        assert!(
            panel[in_view..]
                .iter()
                .any(|row| row.contains("known glyph"))
        );
    }
}

// --- Escape never quits (B4) -------------------------------------------------

#[test]
fn escape_on_the_game_screen_does_nothing_and_passes_no_time() {
    for seed in seeds() {
        let mut app = app_knowing(seed, &ALL, Some("gnat"));
        let before = world(&app);
        let screen = text(&app);

        // A cancelled trace, then the extra presses a player makes out of
        // habit: the game stays, nothing moves.
        trace(&mut app, "75");
        app.handle_key(press(KeyCode::Esc));
        for _ in 0..3 {
            app.handle_key(press(KeyCode::Esc));
        }
        assert_eq!(app.state(), State::Playing, "seed {seed}");
        assert_eq!(world(&app), before, "seed {seed}: no time passes");
        assert_eq!(text(&app), screen, "seed {seed}: the screen is unchanged");

        // After a verdict closed by Escape, the next Escape does nothing
        // either.
        trace(&mut app, "987");
        enter(&mut app);
        assert!(matches!(app.state(), State::Casting(Casting::Verdict(_))));
        app.handle_key(press(KeyCode::Esc));
        assert_eq!(app.state(), State::Playing);
        let after_cast = world(&app);
        app.handle_key(press(KeyCode::Esc));
        assert_eq!(app.state(), State::Playing, "seed {seed}");
        assert_eq!(world(&app), after_cast, "seed {seed}: no time passes");
    }
}

#[test]
fn q_quits_from_the_game_screen_but_not_from_an_overlay() {
    let mut app = app_knowing(SEED, &ALL, None);
    trace(&mut app, "7");
    app.handle_key(press(KeyCode::Char('q')));
    assert!(
        matches!(app.state(), State::Casting(_)),
        "q never quits from the overlay"
    );
    app.handle_key(press(KeyCode::Esc));
    assert_eq!(app.state(), State::Playing);

    app.handle_key(press(KeyCode::Char('q')));
    assert_eq!(app.state(), State::Quit);

    let mut app = app_knowing(SEED, &ALL, None);
    trace(&mut app, "7");
    app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert_eq!(app.state(), State::Quit, "Ctrl+C quits from anywhere");
}

#[test]
fn the_screens_fit_and_the_longest_log_lines_stay_whole_at_80_by_24() {
    // The worst legal line: a sigil and a monster with 24-character names.
    let monsters = r#"[Monster(id: "long", name: "Archivist of Lower Vault", family: Acolyte,
        elite: false, speed: 1, hp: 999, attack: 0, defense: 0, min_depth: 1)]"#;
    let sigils = r#"[Sigil(id: "long", name: "Ember of the Deep Hearth", pattern: [7, 5, 3],
        effect: Damage, power: 6, targeting: NearestEnemy, min_depth: 1)]"#;
    let items = "[]";
    let content = Content::parse_all(monsters, sigils, items).expect("valid long names");
    let mut game = Game::in_fixed_room(SEED);
    game.engrave(
        PLAYER_START.step(Direction::East),
        content.sigil("long").unwrap(),
    );
    game.submit(Action::Move(Direction::East));
    game.submit(Action::Move(Direction::West));
    game.spawn_monster(FOE_CELL, content.monster("long").unwrap());
    let mut app = App::with_game(dark(), game, content);
    trace(&mut app, "752");
    enter(&mut app);
    let screen = text(&app);
    assert!(
        screen.contains("Ember of the Deep Hearth") || screen.contains("Ember of the Deep"),
        "the sigil name is readable somewhere\n{screen}"
    );
    for row in rows(&app) {
        assert!(
            row.chars().count() <= 80,
            "row wider than the screen: {row}"
        );
    }
    // Every log line of the cast is whole: its text can be read back.
    let log_text = text(&app);
    assert!(log_text.contains("unstable trace"), "{log_text}");
}

#[test]
fn below_80_by_24_the_overlay_is_replaced_by_the_resize_message_and_keys_do_nothing() {
    let mut app = app_knowing(SEED, &ALL, Some("sloth"));
    trace(&mut app, "75");
    let small = screen_at(&app, 79, 24);
    assert!(
        small.contains("80"),
        "a message asks for a larger terminal: {small}"
    );
    let before = world(&app);
    app.handle_key(press(KeyCode::Char('3')));
    app.handle_key(press(KeyCode::Enter));
    assert_eq!(
        world(&app),
        before,
        "nothing is traced or cast while hidden"
    );
}

// --- Determinism ------------------------------------------------------------

/// The key script of a replay: waits, moves, exact casts, near misses and
/// unknown traces, with their direction and the key that closes a verdict,
/// drawn from a small generator of this test.
fn key_script(length: usize) -> Vec<Vec<KeyCode>> {
    let traces: [(&str, Option<char>); 9] = [
        ("753", None),
        ("752", None),
        ("8521", None),
        ("852", None),
        ("47896", None),
        ("74123", Some('l')),
        ("1596", Some('l')),
        ("159", Some('l')),
        ("24681", None),
    ];
    let mut state: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = move || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        usize::try_from(state >> 33).unwrap()
    };
    let moves = ['h', 'j', 'k', 'l', 'y', 'u', 'b', 'n'];
    (0..length)
        .map(|_| match next() % 5 {
            0 => vec![KeyCode::Char(moves[next() % 8])],
            1 | 2 => vec![KeyCode::Char('.')],
            _ => {
                let (digits, direction) = traces[next() % traces.len()];
                let mut script = vec![KeyCode::Char('t')];
                script.extend(digits.chars().map(KeyCode::Char));
                script.push(KeyCode::Enter);
                script.extend(direction.map(KeyCode::Char));
                // Closes a verdict if there is one, a plain wait-free key
                // otherwise (an unbound one).
                script.push(KeyCode::Char(' '));
                script
            }
        })
        .collect()
}

/// Plays `script` on `app`, returning the screen and the log after each
/// step.
fn play(mut app: App, script: &[Vec<KeyCode>]) -> Vec<(String, Vec<String>)> {
    script
        .iter()
        .map(|step| {
            for &code in step {
                app.handle_key(press(code));
            }
            (text(&app), log(&app))
        })
        .collect()
}

#[test]
fn the_same_seed_and_keys_replay_the_same_screens_logs_and_backlashes() {
    let script = key_script(220);
    let mut backlashes = std::collections::HashSet::new();
    for seed in seeds() {
        let run = || play(app_knowing(seed, &ALL, Some("sloth")), &script);
        let first = run();
        let second = run();
        assert_eq!(first, second, "seed {seed}: the replay diverged");
        let casts = first
            .last()
            .map(|(_, lines)| lines.iter().filter(|l| l.contains("trace")).count())
            .unwrap_or(0);
        assert!(casts >= 3, "seed {seed}: only {casts} casts logged");
        for (_, lines) in &first {
            for line in lines {
                if BACKLASH_LINES.contains(&line.as_str()) {
                    backlashes.insert(line.clone());
                }
            }
        }
    }
    assert!(!backlashes.is_empty(), "no backlash in the script");
}

#[test]
fn real_floors_replay_the_same_screens_with_casts_in_the_script() {
    let script = key_script(160);
    for seed in seeds() {
        let run = || {
            let content = Content::load().expect("shipped content");
            play(App::new(dark(), seed, &content), &script)
        };
        assert_eq!(run(), run(), "seed {seed}: the replay diverged");
    }
}

// --- More edges --------------------------------------------------------------

#[test]
fn an_unstable_aimed_trace_asks_the_direction_then_shows_the_verdict() {
    for seed in seeds() {
        let mut app = app_in_the_middle("passage");
        let before = clock(&app);

        // 7-4-1-2-3 with its last node replaced: one node off.
        trace(&mut app, "74125");
        enter(&mut app);
        assert!(
            matches!(app.state(), State::Casting(Casting::Aiming { .. })),
            "seed {seed}: the direction is asked for an unstable trace too"
        );
        assert_eq!(spent(&app, before), 0, "nothing is paid before the aim");
        keys(&mut app, "l");

        assert!(
            matches!(app.state(), State::Casting(Casting::Verdict(_))),
            "seed {seed}: {:?}",
            app.state()
        );
        let screen = text(&app);
        assert!(screen.contains("UNSTABLE"), "{screen}");
        assert!(screen.contains("Passage, one node off"), "{screen}");
        let lines = log(&app);
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("Passage, unstable trace")),
            "seed {seed}: {lines:?}"
        );
        assert!(
            lines.iter().any(|l| BACKLASH_LINES.contains(&l.as_str())),
            "seed {seed}: {lines:?}"
        );
    }
}

#[test]
fn pressing_enter_twice_casts_once() {
    let mut app = app_knowing(SEED, &["ember"], Some("sloth"));
    trace(&mut app, "753");
    enter(&mut app);
    let after_first = world(&app);
    enter(&mut app);
    assert_eq!(world(&app), after_first, "the second Enter does nothing");
    assert_eq!(
        log(&app)
            .iter()
            .filter(|line| line.starts_with("Ember"))
            .count(),
        1
    );

    // After a fizzle the second Enter only closes the verdict.
    let mut app = app_knowing(SEED, &["ember"], Some("sloth"));
    trace(&mut app, "987");
    enter(&mut app);
    let after_first = world(&app);
    enter(&mut app);
    assert_eq!(app.state(), State::Playing);
    assert_eq!(
        world(&app),
        after_first,
        "closing the verdict costs nothing"
    );
    assert_eq!(app.game().player_actor().essence(), 4, "one cast was paid");
}

#[test]
fn the_overlay_stays_complete_on_larger_terminals() {
    for (width, height) in [(80, 24), (100, 30), (120, 40), (200, 50)] {
        let mut app = app_knowing(SEED, &ALL, Some("sloth"));
        trace(&mut app, "753");
        let screen = screen_at(&app, width, height);
        for expected in [
            "Trace a sigil",
            "GRIMOIRE",
            "Ember",
            "Recall",
            "Veil",
            "Passage",
            "Lance",
            "1.25 turns",
            "Essence  6/6 · 2 a cast",
            "Enter cast",
        ] {
            assert!(
                screen.contains(expected),
                "{width}x{height}: {expected} missing\n{screen}"
            );
        }
    }
}
