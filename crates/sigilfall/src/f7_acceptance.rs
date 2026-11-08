//! Acceptance tests of F7 (items and inventory) on the whole application.
//!
//! They drive [`App`] with simulated keys and render it into ratatui's
//! `TestBackend`, as a player would use it, and check the three acceptance
//! scenarios of F7 on the screen and in the log: an item picked up appears in
//! the inventory, a healing draught drunk restores hit points without passing
//! the maximum, and a full pack leaves the item on the floor and says why.
//! Beyond them they check dropping, the refusals that cost no time, the
//! essence phial, the ink that steadies the next two unstable casts, the page
//! that teaches Veil, items out of sight, the inventory screen at 80 × 24 in
//! every colour mode (the chosen line must read without colour), Escape, the
//! keys that must not act, and the replay of a series of keys.
//!
//! Expected values are written out here (costs of 100 to pick up and use and
//! 50 to drop, ten slots, wording of the log and of the screen) instead of
//! being read from the production modules, so that a silent change of a rule
//! or of a text is noticed. Every loop is bounded.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::style::Style;
use sigilfall_core::action::Action;
use sigilfall_core::content::{Content, SigilDef};
use sigilfall_core::game::{Game, PLAYER_START};
use sigilfall_core::geometry::{Direction, Position};
use sigilfall_core::rng::Seed;

use crate::app::{App, State};
use crate::theme::{ColorMode, GlyphSet, Theme};

/// The monsters of the tests: the gnat never hurts; the biter strikes hard
/// and dies in a couple of blows, to wound the player on purpose.
const MONSTERS: &str = r#"[
    Monster(id: "gnat", name: "cave gnat", family: Vermin, elite: false,
        speed: 100, hp: 999, attack: 0, defense: 0, min_depth: 1),
    Monster(id: "biter", name: "biter", family: Vermin, elite: false,
        speed: 100, hp: 5, attack: 6, defense: 0, min_depth: 1),
    Monster(id: "pest", name: "pest", family: Vermin, elite: false,
        speed: 100, hp: 1, attack: 2, defense: 0, min_depth: 1),
]"#;

/// Where a monster stands to be in sight, not adjacent: 4 east, 2 up.
const FOE_CELL: Position = Position::new(9, 3);

/// A cell of the fixed room out of sight from the start.
const FAR_CELL: Position = Position::new(25, 3);

/// Slots of the pack, hit points and essence at most.
const CAPACITY: usize = 10;
const MAX_HP: i32 = 30;

/// Energy per pick up or use, and per drop; per tick gained.
const PICK_UP: i32 = 100;
const USE: i32 = 100;
const DROP: i32 = 50;

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

/// Presses `code` once.
fn key(app: &mut App, code: KeyCode) {
    app.handle_key(press(code));
}

/// A fixed-room game of `seed`, the player alone at the start.
fn room(seed: Seed) -> Game {
    Game::in_fixed_room(seed)
}

/// Places the shipped item `id` on `cell` of `game`.
fn lay(game: &mut Game, cell: Position, id: &str) {
    game.place_item(cell, content().item(id).expect("a shipped item"));
}

/// An application on `game` with `theme`.
fn app_with(theme: Theme, game: Game) -> App {
    App::with_game(theme, game, content())
}

/// An application on `game` with the default theme.
fn app_on(game: Game) -> App {
    app_with(dark(), game)
}

/// An application whose player has picked up `ids`, one per cell of row 5
/// from the start eastwards with the `g` key, and now stands on the next
/// cell, where the shipped item `under` lies if given. The log holds the
/// pick-ups.
fn carrying_with(seed: Seed, ids: &[&str], under: Option<&str>) -> App {
    let mut game = room(seed);
    for (x, id) in ids.iter().chain(under.iter()).enumerate() {
        lay(
            &mut game,
            Position::new(5 + i32::try_from(x).unwrap(), 5),
            id,
        );
    }
    let mut app = app_on(game);
    for _ in ids {
        keys(&mut app, "g");
        keys(&mut app, "l");
    }
    assert_eq!(app.game().inventory().count(), ids.len(), "all picked up");
    app
}

/// An application whose player carries `ids` and stands on an empty cell.
fn carrying(seed: Seed, ids: &[&str]) -> App {
    carrying_with(seed, ids, None)
}

/// Whether the application shows the inventory.
fn in_inventory(app: &App) -> bool {
    matches!(app.state(), State::Inventory(_))
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

/// Clock, turns, energy, essence, hit points, position and items carried.
type Snapshot = (u64, u64, i32, i32, i32, Position, usize);

/// What the game shows of the player and the clock, to check that nothing
/// happened (the log length excluded: a refusal is written there).
fn world(app: &App) -> Snapshot {
    let player = app.game().player_actor();
    (
        app.game().ticks(),
        app.game().player_turns(),
        player.energy(),
        player.essence(),
        player.hp(),
        player.position(),
        app.game().inventory().count(),
    )
}

/// The names of the items carried, in order.
fn carried(app: &App) -> Vec<String> {
    app.game()
        .inventory()
        .map(|(_, item)| item.definition().id().to_owned())
        .collect()
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

/// A game in which the player was wounded by `striker`, a creature of the
/// test content that is then dead.
/// Bounded: the loops stop after a fixed number of turns.
fn wounded_game(seed: Seed, deficit: i32, striker: &str) -> Game {
    let content = content();
    let mut game = room(seed);
    let biter = game.spawn_monster(
        PLAYER_START.step(Direction::East),
        content.monster(striker).expect("a test monster"),
    );
    for _ in 0..40 {
        if MAX_HP - game.player_actor().hp() >= deficit {
            break;
        }
        game.submit(Action::Wait);
    }
    assert!(MAX_HP - game.player_actor().hp() >= deficit, "wounded");
    for _ in 0..40 {
        if game.actor(biter).is_none() || game.is_over() {
            break;
        }
        game.submit(Action::Move(Direction::East));
    }
    assert!(!game.is_over() && game.actor(biter).is_none());
    game
}

/// A game in which the player knows the shipped sigil `id` (glyph engraved
/// east of the start, read, walked back from), with a gnat in sight.
fn knowing_game(seed: Seed, id: &str) -> Game {
    let mut game = room(seed);
    game.engrave(PLAYER_START.step(Direction::East), &sigil(id));
    game.submit(Action::Move(Direction::East));
    game.submit(Action::Move(Direction::West));
    assert!(game.grimoire().knows(id));
    game.spawn_monster(FOE_CELL, content().monster("gnat").expect("a test monster"));
    game
}

// --- Scenario 1: an item on the floor, picked up, is in the inventory -------

#[test]
fn scenario_1_a_picked_up_item_appears_in_the_inventory_and_the_log_says_so() {
    for seed in seeds() {
        for def in content().items() {
            for pick in ['g', ','].map(|k| k.to_string()).iter().map(String::as_str) {
                let mut game = room(seed);
                lay(&mut game, PLAYER_START, def.id());
                let mut app = app_on(game);
                let before = clock(&app);

                keys(&mut app, pick);

                assert_eq!(
                    last_log(&app),
                    format!("You pick up the {}.", def.name()),
                    "seed {seed}, key {pick}"
                );
                assert_eq!(carried(&app), vec![def.id().to_owned()]);
                assert_eq!(spent(&app, before), PICK_UP);

                keys(&mut app, "i");
                let screen = text(&app);
                assert!(screen.contains("Inventory"), "{screen}");
                assert!(screen.contains("1/10"), "{screen}");
                assert!(screen.contains(def.name()), "{screen}");
                assert!(screen.contains("› 1"), "the only item is chosen: {screen}");
            }
        }
    }
}

#[test]
fn scenario_1_the_item_is_drawn_on_the_map_until_picked_up() {
    let mut game = room(Seed::new(42));
    lay(
        &mut game,
        PLAYER_START.step(Direction::East),
        "healing_draught",
    );
    lay(&mut game, PLAYER_START.step(Direction::South), "veil_page");
    lay(
        &mut game,
        PLAYER_START.step(Direction::West),
        "steadying_ink",
    );
    let mut app = app_on(game);

    let before = text(&app);
    assert!(before.contains('¡'), "the draught is drawn: {before}");
    assert!(before.contains('¶'), "the page is drawn: {before}");
    assert!(before.contains('♦'), "the ink is drawn: {before}");
    assert!(
        before.contains("Healing Draught"),
        "IN VIEW names it: {before}"
    );

    keys(&mut app, "l");
    keys(&mut app, "g");

    let after = text(&app);
    assert!(after.contains('¶') && after.contains('♦'));
    assert!(
        !after.contains("¡ Healing Draught"),
        "the picked draught left the floor legend: {after}"
    );
    assert!(
        !after.contains('¡'),
        "the picked draught left the map: {after}"
    );
}

#[test]
fn scenario_1_the_inventory_lists_items_in_the_order_picked_up_with_their_effect() {
    let mut app = carrying(
        Seed::new(42),
        &[
            "steadying_ink",
            "healing_draught",
            "veil_page",
            "essence_phial",
        ],
    );

    keys(&mut app, "i");
    let screen = rows(&app);
    let wanted = [
        ("1", "Steadying Ink", "steady 2 casts"),
        ("2", "Healing Draught", "heal 10 HP"),
        ("3", "Page of the Veil", "teach Veil"),
        ("4", "Essence Phial", "restore 4 essence"),
    ];
    let mut last_row = 0;
    for (slot, name, effect) in wanted {
        let row = screen
            .iter()
            .position(|r| {
                r.contains(name) && r.contains(effect) && r.contains(&format!(" {slot} "))
            })
            .unwrap_or_else(|| panic!("{name} / {effect} missing:\n{}", screen.join("\n")));
        assert!(row > last_row, "{name} out of order");
        last_row = row;
    }
    assert!(text(&app).contains("4/10"));
}

// --- Scenario 2: a healing draught restores HP, never above the maximum -----

#[test]
fn scenario_2_a_draught_restores_hit_points_and_the_panel_shows_them() {
    for seed in seeds() {
        let mut game = wounded_game(seed, 11, "biter");
        let before_hp = game.player_actor().hp();
        lay(&mut game, PLAYER_START, "healing_draught");
        let mut app = app_on(game);
        keys(&mut app, "g");
        assert_eq!(
            panel_value(&app, "HP"),
            Some(format!("{before_hp}/30")),
            "the panel shows the wound"
        );
        let before = clock(&app);

        keys(&mut app, "i");
        key(&mut app, KeyCode::Enter);

        let after_hp = before_hp + 10;
        assert_eq!(
            last_log(&app),
            format!("You drink the Healing Draught and recover 10 HP ({after_hp}/30)."),
            "seed {seed}"
        );
        assert_eq!(app.game().player_actor().hp(), after_hp);
        assert_eq!(panel_value(&app, "HP"), Some(format!("{after_hp}/30")));
        assert_eq!(app.state(), State::Playing, "the inventory closes");
        assert_eq!(spent(&app, before), USE);
        assert!(carried(&app).is_empty(), "the draught is spent");
    }
}

#[test]
fn scenario_2_a_small_wound_is_healed_to_the_maximum_and_no_further() {
    for seed in seeds() {
        for use_key in ['u', '\n'] {
            let mut game = wounded_game(seed, 1, "pest");
            let missing = MAX_HP - game.player_actor().hp();
            assert!((1..10).contains(&missing), "a small wound: {missing}");
            lay(&mut game, PLAYER_START, "healing_draught");
            let mut app = app_on(game);
            keys(&mut app, "gi");

            if use_key == 'u' {
                keys(&mut app, "u");
            } else {
                key(&mut app, KeyCode::Enter);
            }

            assert_eq!(
                last_log(&app),
                format!("You drink the Healing Draught and recover {missing} HP (30/30)."),
                "seed {seed}"
            );
            assert_eq!(app.game().player_actor().hp(), MAX_HP);
            assert_eq!(panel_value(&app, "HP"), Some("30/30".to_owned()));
            assert!(!text(&app).contains("31/30"));
        }
    }
}

#[test]
fn scenario_2_a_draught_drunk_unhurt_says_so_and_stays_at_the_maximum() {
    let mut game = room(Seed::new(42));
    lay(&mut game, PLAYER_START, "healing_draught");
    let mut app = app_on(game);

    keys(&mut app, "gi");
    key(&mut app, KeyCode::Enter);

    assert_eq!(
        last_log(&app),
        "You drink the Healing Draught, but you were not hurt (30/30)."
    );
    assert_eq!(app.game().player_actor().hp(), MAX_HP);
    assert!(
        carried(&app).is_empty(),
        "the draught is spent all the same"
    );
}

#[test]
fn scenario_2_two_enters_in_a_row_drink_one_draught_only() {
    let mut game = wounded_game(Seed::new(42), 1, "pest");
    lay(&mut game, PLAYER_START, "healing_draught");
    lay(
        &mut game,
        PLAYER_START.step(Direction::East),
        "healing_draught",
    );
    let mut app = app_on(game);
    keys(&mut app, "g");
    keys(&mut app, "l");
    keys(&mut app, "g");
    keys(&mut app, "i");
    let turns = app.game().player_turns();

    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Enter);

    assert_eq!(app.game().player_turns(), turns + 1, "one drink, one turn");
    assert_eq!(carried(&app).len(), 1, "one draught is left");
    assert_eq!(app.state(), State::Playing);
}

// --- Scenario 3: a full pack leaves the item on the floor -------------------

#[test]
fn scenario_3_a_full_pack_leaves_the_item_on_the_floor_and_the_log_says_why() {
    for seed in seeds() {
        let mut app = carrying_with(seed, &["healing_draught"; CAPACITY], Some("essence_phial"));
        assert_eq!(carried(&app).len(), CAPACITY);
        let before = clock(&app);
        let still = world(&app);

        keys(&mut app, "g");

        assert_eq!(
            last_log(&app),
            "Your pack is full (10/10): the Essence Phial stays on the floor.",
            "seed {seed}"
        );
        assert_eq!(world(&app), still, "nothing was paid");
        assert_eq!(clock(&app), before);
        assert_eq!(carried(&app).len(), CAPACITY);
        assert!(
            app.game()
                .visible_items()
                .any(|item| item.definition == "essence_phial"),
            "the phial still lies on the floor"
        );
        assert!(text(&app).contains("Essence Phial"), "still in the legend");
    }
}

#[test]
fn scenario_3_a_full_pack_is_readable_in_80_by_24() {
    let mut app = carrying(Seed::new(42), &["healing_draught"; CAPACITY]);
    keys(&mut app, "i");

    let screen = rows(&app);
    assert!(screen.iter().all(|row| row.chars().count() == 80));
    assert_eq!(
        screen
            .iter()
            .filter(|r| r.contains("¡ Healing Draught"))
            .count(),
        CAPACITY,
        "ten lines:\n{}",
        screen.join("\n")
    );
    assert!(screen.iter().any(|r| r.contains("10/10")));
    let digits = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"];
    for digit in digits {
        assert!(
            screen
                .iter()
                .any(|r| r.contains(&format!(" {digit}  ¡ Healing Draught"))),
            "slot {digit}:\n{}",
            screen.join("\n")
        );
    }
    // The frame, the side panel and the log are still all there.
    assert!(screen.iter().any(|r| r.contains("HP")));
    assert!(screen.iter().any(|r| r.contains("Log")));
    assert!(screen.last().unwrap().contains("Esc close"));
}

#[test]
fn scenario_3_a_refusal_with_a_full_pack_fits_under_ten_lines() {
    // A full pack, standing on an item: dropping is refused, which writes the
    // refusal under the ten lines of the list.
    let mut app = carrying_with(
        Seed::new(42),
        &["healing_draught"; CAPACITY],
        Some("steadying_ink"),
    );
    keys(&mut app, "i");
    keys(&mut app, "d");

    let screen = rows(&app);
    let wanted = "Something already lies here: drop the Healing Draught elsewhere.";
    let joined = screen.join("\n");
    assert!(
        joined.contains("Something already lies here") && joined.contains("elsewhere."),
        "{joined}"
    );
    assert_eq!(last_log(&app), wanted);
    assert_eq!(
        screen
            .iter()
            .filter(|r| r.contains("Healing Draught") && r.contains("heal 10 HP"))
            .count(),
        CAPACITY,
        "{joined}"
    );
    assert!(in_inventory(&app));
}

// --- Dropping ---------------------------------------------------------------

#[test]
fn an_item_dropped_from_the_inventory_lies_under_the_player_for_half_a_turn() {
    for seed in seeds() {
        let mut app = carrying(seed, &["essence_phial", "healing_draught"]);
        let before = clock(&app);

        keys(&mut app, "i");
        key(&mut app, KeyCode::Down);
        keys(&mut app, "d");

        assert_eq!(
            last_log(&app),
            "You drop the Healing Draught.",
            "seed {seed}"
        );
        assert_eq!(carried(&app), vec!["essence_phial".to_owned()]);
        assert_eq!(spent(&app, before), DROP);
        assert_eq!(app.state(), State::Playing, "the inventory closes");
        assert!(
            app.game()
                .visible_items()
                .any(|item| item.position == app.game().player_actor().position()
                    && item.definition == "healing_draught")
        );

        // And it can be picked up again.
        keys(&mut app, "g");
        assert_eq!(last_log(&app), "You pick up the Healing Draught.");
        assert_eq!(carried(&app).len(), 2);
    }
}

#[test]
fn dropping_where_an_item_already_lies_keeps_the_inventory_open_and_costs_nothing() {
    let mut app = carrying_with(
        Seed::new(42),
        &["essence_phial", "veil_page"],
        Some("healing_draught"),
    );
    keys(&mut app, "i");
    let still = world(&app);
    let clock_before = clock(&app);

    keys(&mut app, "d");

    assert_eq!(
        last_log(&app),
        "Something already lies here: drop the Essence Phial elsewhere."
    );
    assert_eq!(world(&app), still);
    assert_eq!(clock(&app), clock_before);
    assert!(in_inventory(&app), "the inventory stays open");
    let screen = text(&app);
    assert!(
        screen.contains("! Something already lies here"),
        "the reason is under the list: {screen}"
    );
    assert_eq!(carried(&app).len(), 2);

    // Choosing another item clears the notice.
    key(&mut app, KeyCode::Down);
    assert!(!text(&app).contains("! Something already lies"));
}

// --- Refusals that cost no time ----------------------------------------------

#[test]
fn picking_up_with_nothing_there_is_refused_in_the_log_and_costs_nothing() {
    for seed in seeds() {
        let mut game = room(seed);
        game.spawn_monster(FOE_CELL, content().monster("biter").unwrap());
        let mut app = app_on(game);
        let still = world(&app);
        let lines = app.log().len();

        for pick in ["g", ","] {
            keys(&mut app, pick);
            assert_eq!(last_log(&app), "There is nothing here to pick up.");
            assert_eq!(world(&app), still, "seed {seed}, key {pick}");
        }
        assert_eq!(app.log().len(), lines + 2, "one line per refusal");
    }
}

#[test]
fn a_page_of_a_known_sigil_is_refused_without_cost_and_stays_in_the_pack() {
    for seed in seeds() {
        let mut game = knowing_game(seed, "veil");
        lay(&mut game, PLAYER_START, "veil_page");
        let mut app = app_on(game);
        keys(&mut app, "g");
        keys(&mut app, "i");
        assert!(
            text(&app).contains("teach Veil · known"),
            "the page says it is known: {}",
            text(&app)
        );
        let still = world(&app);

        key(&mut app, KeyCode::Enter);

        assert_eq!(
            last_log(&app),
            "You already know Veil: the Page of the Veil has nothing to teach.",
            "seed {seed}"
        );
        assert_eq!(world(&app), still);
        assert!(in_inventory(&app));
        assert!(
            text(&app).contains("! You already know Veil"),
            "{}",
            text(&app)
        );
        assert_eq!(carried(&app), vec!["veil_page".to_owned()]);
    }
}

// --- The essence phial --------------------------------------------------------

#[test]
fn the_essence_phial_restores_essence_up_to_the_maximum_and_the_panel_agrees() {
    for seed in seeds() {
        let mut game = knowing_game(seed, "ember");
        // Spend six essence on three casts, then pick the phial up.
        for _ in 0..3 {
            game.submit(Action::Cast {
                trace: trace(&[7, 5, 3]),
                direction: None,
            });
        }
        lay(&mut game, PLAYER_START, "essence_phial");
        let mut app = app_on(game);
        keys(&mut app, "g");
        let essence = app.game().player_actor().essence();
        assert!(essence < 6, "the reserve is lacking: {essence}");
        let restored = (6 - essence).min(4);

        keys(&mut app, "i");
        key(&mut app, KeyCode::Enter);
        let expected = format!(
            "You drink the Essence Phial and recover {restored} essence ({}/6).",
            essence + restored
        );
        assert!(
            log(&app).contains(&expected),
            "seed {seed}: {:?}",
            log(&app)
        );
        // The reserve may have recovered a point since, never past six.
        let shown = app.game().player_actor().essence();
        assert!((essence + restored..=6).contains(&shown));
        assert_eq!(panel_value(&app, "Essence"), Some(format!("{shown}/6")));
        assert!(app.game().player_actor().essence() <= 6);
    }
}

/// The trace pressed as `digits`.
fn trace(digits: &[u8]) -> sigilfall_core::sigil::Trace {
    let mut trace = sigilfall_core::sigil::Trace::new();
    for &digit in digits {
        trace
            .push(sigilfall_core::sigil::Node::new(digit).expect("a digit from 1 to 9"))
            .expect("a node not linked yet");
    }
    trace
}

#[test]
fn a_phial_drunk_with_a_full_reserve_never_shows_more_than_six() {
    let mut game = room(Seed::new(42));
    lay(&mut game, PLAYER_START, "essence_phial");
    let mut app = app_on(game);
    keys(&mut app, "gi");
    key(&mut app, KeyCode::Enter);

    let line = last_log(&app);
    assert!(
        line.contains("Essence Phial") && line.contains("(6/6)"),
        "{line}"
    );
    assert_eq!(panel_value(&app, "Essence"), Some("6/6".to_owned()));
}

// --- The ink --------------------------------------------------------------------

/// Presses `t`, the digits and Enter: casts the trace at the nearest enemy.
fn cast(app: &mut App, digits: &str) {
    keys(app, "t");
    keys(app, digits);
    key(app, KeyCode::Enter);
}

#[test]
fn the_ink_steadies_the_next_two_unstable_casts_and_the_log_says_so() {
    for seed in seeds() {
        let mut game = knowing_game(seed, "ember");
        lay(&mut game, PLAYER_START, "steadying_ink");
        let mut app = app_on(game);
        keys(&mut app, "gi");
        key(&mut app, KeyCode::Enter);
        assert_eq!(
            last_log(&app),
            "You use the Steadying Ink: your next 2 unstable casts will be cast pure.",
            "seed {seed}"
        );
        assert_eq!(app.game().steady_casts(), 2);

        // The first unstable cast: one node off 7-5-3.
        cast(&mut app, "752");
        let first = log(&app);
        assert!(
            first
                .iter()
                .any(|l| l.contains("Ember, steadied trace (1 left)")),
            "seed {seed}: {first:?}"
        );
        // A verdict box may be waiting for a key: any key closes it.
        key(&mut app, KeyCode::Enter);
        cast(&mut app, "783");
        let second = log(&app);
        assert!(
            second
                .iter()
                .any(|l| l.contains("Ember, steadied trace (0 left)")),
            "seed {seed}: {second:?}"
        );
        key(&mut app, KeyCode::Enter);

        // The third is on its own.
        let lines_before = log(&app).len();
        cast(&mut app, "453");
        let third = log(&app)[lines_before..].join("\n");
        assert!(!third.contains("steadied"), "seed {seed}: {third}");
        assert!(
            third.contains("unstable"),
            "the third cast is unstable: {third}"
        );
        assert_eq!(app.game().steady_casts(), 0);
    }
}

// --- The page that teaches Veil ---------------------------------------------

#[test]
fn the_page_of_the_veil_teaches_veil_and_the_panel_lists_it() {
    for seed in seeds() {
        let mut game = room(seed);
        lay(&mut game, PLAYER_START, "veil_page");
        let mut app = app_on(game);
        keys(&mut app, "gi");
        assert!(text(&app).contains("teach Veil"), "{}", text(&app));
        assert!(!text(&app).contains("known"));
        let before = clock(&app);

        key(&mut app, KeyCode::Enter);

        assert_eq!(
            last_log(&app),
            "You read the Page of the Veil: Veil joins your grimoire.",
            "seed {seed}"
        );
        assert!(app.game().grimoire().knows("veil"));
        assert_eq!(spent(&app, before), USE);
        assert!(text(&app).contains("Veil"), "the panel lists it");
        assert!(
            text(&app).contains("SIGILS") && text(&app).contains("1/5"),
            "{}",
            text(&app)
        );
    }
}

// --- Items out of sight -----------------------------------------------------

#[test]
fn an_item_out_of_sight_is_not_drawn_nor_listed_until_the_player_nears_it() {
    for seed in seeds() {
        let mut game = room(seed);
        lay(&mut game, FAR_CELL, "healing_draught");
        let mut app = app_on(game);

        let far = text(&app);
        assert!(!far.contains('¡'), "seed {seed}: drawn out of sight\n{far}");
        assert!(
            !far.contains("Healing Draught"),
            "listed out of sight\n{far}"
        );

        // Walk east until it shows, bounded.
        let mut shown = false;
        for _ in 0..30 {
            if text(&app).contains('¡') {
                shown = true;
                break;
            }
            keys(&mut app, "l");
        }
        assert!(shown, "seed {seed}: the item never came into view");
        assert!(text(&app).contains("Healing Draught"), "listed in view");

        // Walk back west until the cell is out of sight again.
        for _ in 0..30 {
            if !text(&app).contains('¡') {
                break;
            }
            keys(&mut app, "h");
        }
        assert!(
            !text(&app).contains('¡'),
            "seed {seed}: remembered cell still shows the item\n{}",
            text(&app)
        );
        assert!(!text(&app).contains("Healing Draught"));
    }
}

// --- The inventory screen -------------------------------------------------------

#[test]
fn an_empty_pack_explains_how_to_pick_something_up() {
    let mut app = app_on(room(Seed::new(42)));
    let still = world(&app);
    let lines = app.log().len();

    keys(&mut app, "i");

    let screen = rows(&app);
    let joined = screen.join("\n");
    assert!(
        joined.contains("Inventory") && joined.contains("0/10"),
        "{joined}"
    );
    assert!(joined.contains("You carry nothing yet."), "{joined}");
    assert!(
        joined.contains("Stand on an item, ¡ on the map, and press"),
        "{joined}"
    );
    assert!(joined.contains("g to pick it up."), "{joined}");
    assert!(screen.last().unwrap().contains("Esc close"));
    assert!(!screen.last().unwrap().contains("Enter/u use"));
    assert_eq!(world(&app), still);

    // Use, drop and the navigation keys do nothing on an empty pack.
    for pressed in ["u", "d", "j", "k", "5", "0"] {
        keys(&mut app, pressed);
    }
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::End);
    assert_eq!(world(&app), still);
    assert!(in_inventory(&app));
    assert_eq!(app.log().len(), lines, "nothing is written for nothing");
}

#[test]
fn the_keys_move_the_choice_in_a_loop_and_by_digit() {
    let mut app = carrying(
        Seed::new(42),
        &[
            "steadying_ink",
            "healing_draught",
            "veil_page",
            "essence_phial",
        ],
    );
    keys(&mut app, "i");
    let chosen = |app: &App| {
        rows(app)
            .into_iter()
            .find(|r| r.contains('›') && r.contains('│'))
            .unwrap_or_default()
    };
    assert!(chosen(&app).contains("Steadying Ink"), "{}", chosen(&app));
    let turns = app.game().player_turns();

    key(&mut app, KeyCode::Down);
    assert!(chosen(&app).contains("Healing Draught"));
    keys(&mut app, "j");
    assert!(chosen(&app).contains("Page of the Veil"));
    keys(&mut app, "k");
    assert!(chosen(&app).contains("Healing Draught"));
    key(&mut app, KeyCode::Up);
    key(&mut app, KeyCode::Up);
    assert!(chosen(&app).contains("Essence Phial"), "wraps to the last");
    key(&mut app, KeyCode::Down);
    assert!(chosen(&app).contains("Steadying Ink"), "wraps to the first");
    keys(&mut app, "3");
    assert!(chosen(&app).contains("Page of the Veil"));
    key(&mut app, KeyCode::End);
    assert!(chosen(&app).contains("Essence Phial"));
    key(&mut app, KeyCode::Home);
    assert!(chosen(&app).contains("Steadying Ink"));
    keys(&mut app, "9");
    assert!(
        !chosen(&app).is_empty() && chosen(&app).matches('›').count() == 1,
        "a digit past the last item still leaves exactly one choice: {}",
        text(&app)
    );
    assert_eq!(app.game().player_turns(), turns, "choosing took no time");
}

#[test]
fn the_zero_key_chooses_the_tenth_slot() {
    let mut ids = vec!["healing_draught"; CAPACITY - 1];
    ids.push("steadying_ink");
    let mut app = carrying(Seed::new(42), &ids);
    keys(&mut app, "i");
    keys(&mut app, "0");
    assert!(
        rows(&app)
            .iter()
            .any(|r| r.contains('›') && r.contains("Steadying Ink")),
        "{}",
        text(&app)
    );
}

#[test]
fn the_chosen_line_reads_without_colour_in_every_mode_and_both_glyph_sets() {
    let modes = [
        ColorMode::TrueColorDark,
        ColorMode::TrueColorLight,
        ColorMode::Ansi16,
        ColorMode::Monochrome,
    ];
    for mode in modes {
        for glyphs in [GlyphSet::Unicode, GlyphSet::Ascii] {
            let mut game = room(Seed::new(42));
            for (x, id) in ["healing_draught", "essence_phial", "veil_page"]
                .iter()
                .enumerate()
            {
                lay(
                    &mut game,
                    Position::new(5 + i32::try_from(x).unwrap(), 5),
                    id,
                );
            }
            let mut app = app_with(Theme::new(mode, glyphs), game);
            keys(&mut app, "g");
            keys(&mut app, "l");
            keys(&mut app, "g");
            keys(&mut app, "l");
            keys(&mut app, "g");
            keys(&mut app, "i");
            key(&mut app, KeyCode::Down);

            let buffer = buffer_at(&app, 80, 24);
            let screen = rows_of(&buffer);
            let marker = if glyphs == GlyphSet::Unicode {
                '›'
            } else {
                '>'
            };
            let marked: Vec<usize> = screen
                .iter()
                .enumerate()
                .filter(|(_, r)| r.contains(&format!("{marker} 2")))
                .map(|(y, _)| y)
                .collect();
            assert_eq!(
                marked.len(),
                1,
                "{mode:?} {glyphs:?}\n{}",
                screen.join("\n")
            );
            let chosen = marked[0];
            let other = screen
                .iter()
                .position(|r| r.contains("  1  ") && r.contains("Healing Draught"))
                .unwrap_or_else(|| panic!("{mode:?}: first line\n{}", screen.join("\n")));

            // Compare the style of the name's cell on the chosen line and on
            // an unchosen one: background or modifiers must differ, so that
            // the choice does not depend on the marker alone.
            let name_x = |y: usize| {
                let row = &screen[y];
                let at = row
                    .find(if y == chosen {
                        "Essence Phial"
                    } else {
                        "Healing Draught"
                    })
                    .unwrap();
                u16::try_from(row[..at].chars().count()).unwrap()
            };
            let style =
                |y: usize| -> Style { buffer[(name_x(y), u16::try_from(y).unwrap())].style() };
            let (a, b) = (style(chosen), style(other));
            assert!(
                a.bg != b.bg
                    || a.add_modifier != b.add_modifier
                    || a.sub_modifier != b.sub_modifier,
                "{mode:?} {glyphs:?}: the chosen line looks like the others: {a:?} vs {b:?}"
            );
        }
    }
}

#[test]
fn escape_closes_the_inventory_for_free_and_never_quits() {
    let mut app = carrying(Seed::new(42), &["healing_draught", "veil_page"]);
    keys(&mut app, "i");
    let still = world(&app);
    let lines = app.log().len();

    key(&mut app, KeyCode::Esc);

    assert_eq!(app.state(), State::Playing);
    assert_eq!(world(&app), still);
    assert_eq!(app.log().len(), lines, "nothing is written");
    assert!(!text(&app).contains("Inventory"));

    // A second Escape on the map does nothing either.
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.state(), State::Playing);
    assert_eq!(world(&app), still);

    // Escape after a refusal also closes without cost.
    let mut app = carrying_with(Seed::new(42), &["veil_page"], Some("steadying_ink"));
    let still = world(&app);
    keys(&mut app, "id");
    assert!(in_inventory(&app));
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.state(), State::Playing);
    assert_eq!(world(&app), still);
}

#[test]
fn q_does_not_quit_from_the_inventory_but_ctrl_c_does() {
    let mut app = carrying(Seed::new(42), &["healing_draught"]);
    keys(&mut app, "iq");
    assert!(in_inventory(&app), "q is not a quit key here");

    app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert_eq!(app.state(), State::Quit);
}

#[test]
fn keys_that_do_not_belong_to_the_inventory_change_nothing() {
    let mut app = carrying(Seed::new(42), &["healing_draught", "veil_page"]);
    keys(&mut app, "i");
    let still = world(&app);
    let lines = app.log().len();

    // Moves, waiting, tracing and picking up do nothing while it is open.
    for pressed in ["h", "l", "y", ".", "t", "g", ",", "i", "x", "?"] {
        keys(&mut app, pressed);
    }
    key(&mut app, KeyCode::Left);
    key(&mut app, KeyCode::Tab);
    key(&mut app, KeyCode::Backspace);

    assert!(in_inventory(&app));
    assert_eq!(world(&app), still);
    assert_eq!(app.log().len(), lines);
}

#[test]
fn key_releases_and_repeats_do_not_act_twice() {
    let mut game = room(Seed::new(42));
    lay(&mut game, PLAYER_START, "healing_draught");
    let mut app = app_on(game);

    let mut release = KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE);
    release.kind = KeyEventKind::Release;
    app.handle_key(release);
    assert!(carried(&app).is_empty(), "a release is not a press");

    keys(&mut app, "g");
    keys(&mut app, "i");
    let mut enter = press(KeyCode::Enter);
    enter.kind = KeyEventKind::Release;
    app.handle_key(enter);
    assert!(in_inventory(&app), "a release does not use the item");
    assert_eq!(carried(&app).len(), 1);
}

#[test]
fn the_inventory_cannot_be_opened_or_used_below_80_by_24() {
    let mut app = carrying(Seed::new(42), &["healing_draught"]);
    keys(&mut app, "i");
    // A smaller terminal: the resize message replaces the screen and the
    // keys that act are ignored.
    let _ = buffer_at(&app, 79, 24);
    let still = world(&app);
    key(&mut app, KeyCode::Enter);
    keys(&mut app, "d");
    assert_eq!(world(&app), still, "no item used or dropped when too small");
    assert_eq!(carried(&app).len(), 1);

    let small = rows_of(&buffer_at(&app, 79, 24)).join("\n");
    assert!(!small.contains("Healing Draught"), "{small}");
    assert!(
        small.to_lowercase().contains("terminal") || small.contains("80"),
        "{small}"
    );
}

#[test]
fn the_inventory_screen_does_not_hide_the_side_panel_or_the_log() {
    let mut app = carrying(Seed::new(42), &["healing_draught", "essence_phial"]);
    keys(&mut app, "i");
    let screen = text(&app);
    for expected in [
        "HP",
        "Essence",
        "SIGILS",
        "IN VIEW",
        "Log",
        "You pick up the Essence Phial.",
    ] {
        assert!(screen.contains(expected), "{expected} missing:\n{screen}");
    }
    assert!(screen.contains("↑↓/1-0 choose   Enter/u use   d drop   Esc close"));
}

#[test]
fn the_screen_has_no_item_text_clipped_with_the_longest_names() {
    // A 24-character item name and a 24-character sigil name: the longest
    // refusal must fit in the two rows kept for it.
    let sigils = include_str!("../../sigilfall-core/content/sigils.ron")
        .replace("name: \"Veil\"", "name: \"Veil of the Long Nights.\"");
    let items = include_str!("../../sigilfall-core/content/items.ron").replace(
        "name: \"Page of the Veil\"",
        "name: \"Page of the Veil Beyond\"",
    );
    let Ok(content) = Content::parse_all(MONSTERS, &sigils, &items) else {
        // The bound on names (24 characters) refuses these: use the limit.
        let sigils = include_str!("../../sigilfall-core/content/sigils.ron")
            .replace("name: \"Veil\"", "name: \"Veil of the Long Night\"");
        let items = include_str!("../../sigilfall-core/content/items.ron").replace(
            "name: \"Page of the Veil\"",
            "name: \"Page of the Veil Beyond\"",
        );
        let content = Content::parse_all(MONSTERS, &sigils, &items).expect("names at the limit");
        long_names_screen(content);
        return;
    };
    long_names_screen(content);
}

/// Checks the refusal of a page of a known sigil with long names.
fn long_names_screen(content: Content) {
    let mut game = room(Seed::new(42));
    game.engrave(
        PLAYER_START.step(Direction::East),
        content.sigil("veil").expect("veil"),
    );
    game.submit(Action::Move(Direction::East));
    game.submit(Action::Move(Direction::West));
    let page = content.item("veil_page").expect("the page").clone();
    game.place_item(PLAYER_START, &page);
    let mut app = App::with_game(dark(), game, content);
    keys(&mut app, "gi");
    key(&mut app, KeyCode::Enter);

    let screen = rows(&app);
    let notice: Vec<&String> = screen[1..17]
        .iter()
        .filter(|r| r.contains("already know"))
        .collect();
    assert_eq!(notice.len(), 1, "{}", screen.join("\n"));
    assert!(
        screen.iter().any(|r| r.contains("to teach.")),
        "the end of the sentence is not cut:\n{}",
        screen.join("\n")
    );
    assert!(screen.iter().all(|r| r.chars().count() == 80));
}

// --- After the death -----------------------------------------------------------

#[test]
fn nothing_is_picked_up_nor_opened_after_the_death() {
    // A biter with a mighty attack kills a player standing still.
    let killer = r#"[
        Monster(id: "killer", name: "killer", family: Vermin, elite: false,
            speed: 400, hp: 999, attack: 99, defense: 0, min_depth: 1),
    ]"#;
    let sigils = include_str!("../../sigilfall-core/content/sigils.ron");
    let items = include_str!("../../sigilfall-core/content/items.ron");
    let content = Content::parse_all(killer, sigils, items).expect("valid");
    let mut game = room(Seed::new(42));
    game.spawn_monster(
        PLAYER_START.step(Direction::East),
        content.monster("killer").unwrap(),
    );
    lay(&mut game, PLAYER_START, "healing_draught");
    let mut app = App::with_game(dark(), game, content);

    for _ in 0..10 {
        if app.state() == State::GameOver {
            break;
        }
        keys(&mut app, ".");
    }
    assert_eq!(app.state(), State::GameOver);
    let still = world(&app);

    keys(&mut app, "g");
    keys(&mut app, "i");

    assert_eq!(app.state(), State::GameOver);
    assert_eq!(world(&app), still);
    assert!(carried(&app).is_empty());
}

// --- Deterministic replay ---------------------------------------------------------

/// Plays a fixed series of keys on a fresh application of the real game of
/// `seed` (monsters and items of the generated floor) and returns the log
/// and the screen at the end.
fn replay(seed: Seed, theme: Theme) -> (Vec<String>, String, Snapshot) {
    let mut app = App::new(theme, seed, &Content::load().expect("shipped content"));
    let script = "g,ilkgjdu.hjklyubn.g,iuiduggdd.lkjhyubn..uigiu";
    for pressed in script.chars().cycle().take(300) {
        keys(&mut app, &pressed.to_string());
        if app.state() == State::Quit {
            break;
        }
        if in_inventory(&app) && pressed == 'u' {
            key(&mut app, KeyCode::Esc);
        }
    }
    (log(&app), text(&app), world(&app))
}

#[test]
fn the_same_seed_and_keys_replay_the_same_log_and_screen() {
    for seed in seeds() {
        let first = replay(seed, dark());
        let second = replay(seed, dark());
        assert!(!first.0.is_empty(), "something was logged");
        assert_eq!(first, second, "seed {seed}");
    }
}

#[test]
fn a_scripted_game_with_items_replays_identically_in_a_fixed_room() {
    let run = || {
        let mut game = room(Seed::new(7));
        for (x, id) in [
            "healing_draught",
            "essence_phial",
            "steadying_ink",
            "veil_page",
        ]
        .iter()
        .enumerate()
        {
            lay(
                &mut game,
                Position::new(6 + i32::try_from(x).unwrap(), 5),
                id,
            );
        }
        game.spawn_monster(FOE_CELL, content().monster("biter").unwrap());
        let mut app = app_on(game);
        for pressed in "lglglgl gi\njuiddi\x1bggi1ukd".chars() {
            keys(&mut app, &pressed.to_string());
        }
        (log(&app), text(&app), world(&app))
    };
    assert_eq!(run(), run());
}

// --- Dying while using, sizes, known pages ---------------------------------------

#[test]
fn dying_right_after_drinking_ends_on_the_end_screen_not_in_the_inventory() {
    let killer = r#"[
        Monster(id: "killer", name: "killer", family: Vermin, elite: false,
            speed: 400, hp: 999, attack: 99, defense: 0, min_depth: 1),
    ]"#;
    let sigils = include_str!("../../sigilfall-core/content/sigils.ron");
    let items = include_str!("../../sigilfall-core/content/items.ron");
    let content = Content::parse_all(killer, sigils, items).expect("valid");
    let mut game = room(Seed::new(42));
    game.spawn_monster(
        PLAYER_START.step(Direction::East),
        content.monster("killer").unwrap(),
    );
    lay(&mut game, PLAYER_START, "healing_draught");
    let mut app = App::with_game(dark(), game, content);

    keys(&mut app, "gi");
    if in_inventory(&app) {
        key(&mut app, KeyCode::Enter);
    }

    assert_eq!(app.state(), State::GameOver);
    assert!(text(&app).contains("Killed by"), "{}", text(&app));
}

#[test]
fn the_overlay_stays_whole_on_larger_terminals() {
    let mut app = carrying(Seed::new(42), &["healing_draught", "veil_page"]);
    keys(&mut app, "i");
    for (width, height) in [(80, 24), (81, 25), (100, 30), (120, 40), (200, 60)] {
        let screen = rows_of(&buffer_at(&app, width, height)).join("\n");
        assert!(screen.contains("Inventory"), "{width}x{height}");
        assert!(screen.contains("Healing Draught"), "{width}x{height}");
        assert!(screen.contains("Page of the Veil"), "{width}x{height}");
        assert!(screen.contains("2/10"), "{width}x{height}");
        assert!(screen.contains("Esc close"), "{width}x{height}");
    }
}

#[test]
fn a_second_page_shows_as_known_once_the_first_is_read() {
    let mut app = carrying(Seed::new(42), &["veil_page", "veil_page"]);
    keys(&mut app, "i");
    assert!(!text(&app).contains("known"));

    key(&mut app, KeyCode::Enter);
    keys(&mut app, "i");

    assert!(text(&app).contains("teach Veil · known"), "{}", text(&app));
    assert_eq!(carried(&app), vec!["veil_page".to_owned()]);
}
