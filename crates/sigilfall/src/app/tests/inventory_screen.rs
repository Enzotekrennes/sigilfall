//! Items driven by simulated keys (T21): picking up with `g` or `,`, the
//! inventory opened with `i`, choosing an item, using and dropping it, the
//! refusals of the game, closing at no cost, the empty state, the items on
//! the map and in the side panel, and the styles of the inventory in the
//! four rendering modes.

use ratatui::style::{Color, Modifier, Style};
use sigilfall_core::combat::PLAYER_MAX_HP;
use sigilfall_core::content::ItemEffect;
use sigilfall_core::item::{INVENTORY_CAPACITY, ItemRefusal};

use super::*;
use crate::message_log::Tone;

/// A monster that wounds the player one hit point per blow and dies at
/// the first blow it takes: the nibbler deals the minimum damage of a blow
/// (attack 0) and has 1 hit point.
const NIBBLER: &str = r#"[
    Monster(id: "nibbler", name: "nibbler", family: Vermin, elite: false,
        speed: 100, hp: 1, attack: 0, defense: 0, min_depth: 1),
]"#;

/// The four rendering modes of `docs/design.md`.
const MODES: [ColorMode; 4] = [
    ColorMode::TrueColorDark,
    ColorMode::TrueColorLight,
    ColorMode::Ansi16,
    ColorMode::Monochrome,
];

/// Healing of the shipped Healing Draught (`content/items.ron`), checked
/// by [`the_shipped_draught_heals_ten`].
const DRAUGHT_HEAL: i32 = 10;

/// Screen row of the first item of the inventory on an 80 × 24 screen: the
/// overlay's frame starts on row 1 like the map, and the list starts one
/// blank row inside it.
const FIRST_ITEM_ROW: u16 = 3;

/// Screen column of the selection marker on an 80 × 24 screen: the frame
/// starts on column 0 and the list one blank column inside it.
const MARKER_COLUMN: u16 = 2;

/// Screen rows of a refusal notice of two lines: the last two rows inside
/// the frame, whose 15 rows end on row 15.
const NOTICE_ROWS: [u16; 2] = [13, 14];

/// Screen column right of the chosen line's highlight: the list ends one
/// blank column inside the frame's right border, on column 56.
const LIST_END_COLUMN: u16 = 55;

/// The nibbler, with the shipped sigils and items.
fn content() -> Content {
    Content::parse(NIBBLER).expect("valid test monsters")
}

/// The default theme of Windows Terminal (see [`new_app`]).
fn dark() -> Theme {
    Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode)
}

/// Lays an item of each shipped definition `ids` on the player's cell of
/// `game` and picks it up, in order, so the inventory lists them so.
fn pick_up(game: &mut Game, ids: &[&str]) {
    let content = content();
    for id in ids {
        let cell = game.player_actor().position();
        game.place_item(cell, content.item(id).expect("a shipped item"));
        let events = game.submit(Action::PickUp);
        assert!(
            matches!(events.first(), Some(GameEvent::PickedUp { .. })),
            "{id}: {events:?}"
        );
    }
}

/// A fixed-room game of [`SEED`] where the player carries the shipped
/// items `ids`, in that order, and stands on the start cell.
fn carrying(ids: &[&str]) -> Game {
    let mut game = Game::in_fixed_room(SEED);
    pick_up(&mut game, ids);
    game
}

/// An application on `game`, with the nibbler and the shipped content,
/// drawn with `theme`.
fn app_on(game: Game, theme: Theme) -> App {
    App::with_game(theme, game, content())
}

/// A fixed-room game of [`SEED`] where a nibbler has wounded the player
/// down to `hp` hit points, then died of the player's blow; the player
/// stands on the start cell.
fn wounded_to(hp: i32) -> Game {
    let mut game = Game::in_fixed_room(SEED);
    let east = PLAYER_START.step(Direction::East);
    game.spawn_monster(east, content().monster("nibbler").unwrap());
    // Each blow takes one hit point; 30 waits are more than enough.
    for _ in 0..30 {
        if game.player_actor().hp() <= hp {
            break;
        }
        game.submit(Action::Wait);
    }
    // The nibbler strikes at most once more before the player's blow.
    game.submit(Action::Move(Direction::East));
    assert!(
        game.actors().all(|(_, actor)| actor.is_player()),
        "the nibbler died"
    );
    assert_eq!(game.player_actor().position(), PLAYER_START);
    game
}

/// What the game shows of time and of the player, to check that nothing
/// happened: ticks, turns, hit points, position, and the items carried.
fn clock(app: &App) -> (u64, u64, i32, Position, Vec<String>) {
    let game = app.game();
    (
        game.ticks(),
        game.player_turns(),
        game.player_actor().hp(),
        game.player_actor().position(),
        carried(app),
    )
}

/// The identifiers of the items the player of `app` carries, in order.
fn carried(app: &App) -> Vec<String> {
    app.game()
        .inventory()
        .map(|(_, item)| item.definition().id().to_owned())
        .collect()
}

/// The whole screen of `app` at 80 × 24.
fn screen(app: &App) -> Buffer {
    render(app, 80, 24).backend().buffer().clone()
}

/// The screen of `app` at 80 × 24, as lines of text.
fn text(app: &App) -> String {
    render(app, 80, 24).backend().to_string()
}

/// The newest line of the log.
fn last_log(app: &App) -> (Tone, String) {
    let message = app.log().newest_first().next().expect("the log has lines");
    (message.tone, message.text.clone())
}

/// Position of the item chosen in the inventory, or `None` when the
/// inventory is closed.
fn chosen(app: &App) -> Option<usize> {
    match app.state() {
        State::Inventory(screen) => Some(screen.selected(app.game().inventory().count())),
        _ => None,
    }
}

/// The screen column where `needle` starts in `row`, a row of
/// [`row_text`], or `None` when it is not there. Columns count characters,
/// not bytes: frames and glyphs take several bytes.
fn column_of(row: &str, needle: &str) -> Option<u16> {
    let byte = row.find(needle)?;
    u16::try_from(row[..byte].chars().count()).ok()
}

/// The screen row showing the item in `slot` of the inventory.
fn item_row(slot: usize) -> u16 {
    FIRST_ITEM_ROW + u16::try_from(slot).unwrap()
}

/// Asserts that the cell `(x, y)` of `buffer` shows `glyph` with exactly
/// the colour of `style` and at least its emphasis.
fn assert_cell(buffer: &Buffer, (x, y): (u16, u16), glyph: char, style: Style, what: &str) {
    let cell = &buffer[(x, y)];
    assert_eq!(cell.symbol(), glyph.to_string(), "{what}");
    assert_eq!(cell.fg, style.fg.unwrap_or(Color::Reset), "{what}");
    assert!(cell.modifier.contains(style.add_modifier), "{what}");
}

#[test]
fn the_shipped_draught_heals_ten() {
    let content = content();
    let draught = content.item("healing_draught").unwrap();

    assert_eq!(draught.effect(), &ItemEffect::Heal(DRAUGHT_HEAL));
}

#[test]
fn g_picks_up_the_item_underfoot_and_time_passes() {
    let mut game = Game::in_fixed_room(SEED);
    let east = PLAYER_START.step(Direction::East);
    game.place_item(east, content().item("healing_draught").unwrap());
    let mut app = app_on(game, dark());
    app.handle_key(press(KeyCode::Right));
    let turns = app.game().player_turns();

    app.handle_key(press(KeyCode::Char('g')));

    assert_eq!(carried(&app), ["healing_draught"]);
    assert_eq!(
        app.game().player_turns(),
        turns + 1,
        "picking up takes a turn"
    );
    assert_eq!(
        last_log(&app),
        (Tone::Success, "You pick up the Healing Draught.".to_owned())
    );
    assert_eq!(
        app.game().visible_items().count(),
        0,
        "no longer on the floor"
    );
    assert_eq!(app.state(), State::Playing);
}

#[test]
fn comma_picks_up_too() {
    let mut game = Game::in_fixed_room(SEED);
    game.place_item(PLAYER_START, content().item("steadying_ink").unwrap());
    let mut app = app_on(game, dark());

    app.handle_key(press(KeyCode::Char(',')));

    assert_eq!(carried(&app), ["steadying_ink"]);
}

#[test]
fn picking_up_where_nothing_lies_takes_no_time() {
    let mut app = app_on(Game::in_fixed_room(SEED), dark());
    let before = clock(&app);

    app.handle_key(press(KeyCode::Char('g')));

    assert_eq!(clock(&app), before);
    assert_eq!(
        last_log(&app),
        (Tone::Info, "There is nothing here to pick up.".to_owned())
    );
}

#[test]
fn a_full_inventory_leaves_the_item_on_the_floor_and_says_why() {
    let ten = ["healing_draught"; INVENTORY_CAPACITY];
    let mut game = carrying(&ten);
    game.place_item(PLAYER_START, content().item("essence_phial").unwrap());
    let mut app = app_on(game, dark());
    let before = clock(&app);

    app.handle_key(press(KeyCode::Char('g')));

    assert_eq!(clock(&app), before, "no time passes and nothing is carried");
    assert_eq!(
        last_log(&app),
        (
            Tone::Info,
            "Your pack is full (10/10): the Essence Phial stays on the floor.".to_owned()
        )
    );
    let left = app.game().visible_items().map(|item| item.definition);
    assert_eq!(left.collect::<Vec<_>>(), ["essence_phial"]);

    app.handle_key(press(KeyCode::Char('i')));
    let text = text(&app);
    assert!(text.contains(" 10/10 "), "{text}");
    let slots: Vec<String> = (0..INVENTORY_CAPACITY)
        .map(|slot| {
            let row = row_text(&screen(&app), item_row(slot));
            row.chars().nth(4).map(String::from).unwrap_or_default()
        })
        .collect();
    assert_eq!(slots, ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"]);
}

#[test]
fn drinking_a_draught_heals_up_to_the_maximum_and_the_panel_shows_it() {
    let mut game = wounded_to(27);
    let hp = game.player_actor().hp();
    assert!(
        hp > PLAYER_MAX_HP - DRAUGHT_HEAL && hp < PLAYER_MAX_HP,
        "a wound smaller than the heal: {hp}"
    );
    pick_up(&mut game, &["healing_draught"]);
    let mut app = app_on(game, dark());
    let turns = app.game().player_turns();

    press_all(&mut app, &[KeyCode::Char('i'), KeyCode::Enter]);

    assert_eq!(
        app.state(),
        State::Playing,
        "using an item closes the inventory"
    );
    assert_eq!(app.game().player_actor().hp(), PLAYER_MAX_HP, "capped");
    assert_eq!(app.game().player_turns(), turns + 1);
    assert!(carried(&app).is_empty(), "the draught is used up");
    let restored = PLAYER_MAX_HP - hp;
    assert_eq!(
        last_log(&app),
        (
            Tone::Success,
            format!("You drink the Healing Draught and recover {restored} HP (30/30).")
        )
    );
    assert!(row_text(&screen(&app), 1).ends_with("HP            30/30 "));
}

#[test]
fn drinking_a_draught_when_badly_hurt_restores_its_whole_strength() {
    let mut game = wounded_to(15);
    let hp = game.player_actor().hp();
    assert!(hp + DRAUGHT_HEAL < PLAYER_MAX_HP, "a deep wound: {hp}");
    pick_up(&mut game, &["healing_draught"]);
    let mut app = app_on(game, dark());

    press_all(&mut app, &[KeyCode::Char('i'), KeyCode::Char('u')]);

    let healed = hp + DRAUGHT_HEAL;
    assert_eq!(app.game().player_actor().hp(), healed);
    assert_eq!(
        last_log(&app).1,
        format!("You drink the Healing Draught and recover 10 HP ({healed}/30).")
    );
    assert!(
        row_text(&screen(&app), 1).ends_with(&format!("{healed}/30 ")),
        "the panel follows"
    );
}

#[test]
fn drinking_a_draught_unhurt_says_so_instead_of_mending() {
    let mut app = app_on(carrying(&["healing_draught"]), dark());

    press_all(&mut app, &[KeyCode::Char('i'), KeyCode::Enter]);

    assert_eq!(app.game().player_actor().hp(), PLAYER_MAX_HP);
    assert_eq!(
        last_log(&app).1,
        "You drink the Healing Draught, but you were not hurt (30/30)."
    );
    assert!(app.log().newest_first().all(|m| !m.text.contains("mend")));
}

#[test]
fn dropping_lays_the_chosen_item_underfoot_and_closes_the_inventory() {
    let mut app = app_on(carrying(&["healing_draught", "steadying_ink"]), dark());
    let turns = app.game().player_turns();

    press_all(
        &mut app,
        &[KeyCode::Char('i'), KeyCode::Down, KeyCode::Char('d')],
    );

    assert_eq!(app.state(), State::Playing);
    assert_eq!(carried(&app), ["healing_draught"]);
    assert_eq!(app.game().player_turns(), turns + 1);
    let dropped: Vec<_> = app
        .game()
        .visible_items()
        .map(|item| (item.definition, item.position))
        .collect();
    assert_eq!(dropped, [("steadying_ink", PLAYER_START)]);
    assert_eq!(
        last_log(&app),
        (Tone::Info, "You drop the Steadying Ink.".to_owned())
    );
    let text = text(&app);
    assert!(text.contains("♦ Steadying Ink"), "listed in view: {text}");
}

#[test]
fn a_refused_drop_takes_no_time_and_keeps_the_inventory_open() {
    let mut game = carrying(&["healing_draught"]);
    game.place_item(PLAYER_START, content().item("steadying_ink").unwrap());
    let mut app = app_on(game, dark());
    let before = clock(&app);

    press_all(&mut app, &[KeyCode::Char('i'), KeyCode::Char('d')]);

    assert_eq!(clock(&app), before);
    let State::Inventory(inventory) = app.state() else {
        panic!("the inventory stays open: {:?}", app.state());
    };
    assert_eq!(
        inventory.refusal().map(|(_, reason)| reason),
        Some(ItemRefusal::CellTaken)
    );
    let refusal = "Something already lies here: drop the Healing Draught elsewhere.";
    assert_eq!(last_log(&app), (Tone::Info, refusal.to_owned()));
    let notice = |app: &App| {
        let screen = screen(app);
        NOTICE_ROWS.map(|row| row_text(&screen, row))
    };
    let [first, second] = notice(&app);
    assert!(
        first.starts_with("│ ! Something already lies here: drop the Healing "),
        "{first}"
    );
    assert!(second.starts_with("│   Draught elsewhere. "), "{second}");

    app.handle_key(press(KeyCode::Down));
    let [first, second] = notice(&app);
    assert!(
        !first.contains("Something") && !second.contains("Draught"),
        "a new choice clears it: {first}{second}"
    );
}

#[test]
fn a_page_whose_sigil_is_known_is_refused_without_cost() {
    let content = content();
    let mut game = Game::in_fixed_room(SEED);
    let east = PLAYER_START.step(Direction::East);
    game.engrave(east, content.sigil("veil").unwrap());
    game.submit(Action::Move(Direction::East));
    game.submit(Action::Move(Direction::West));
    assert!(game.grimoire().knows("veil"));
    pick_up(&mut game, &["veil_page"]);
    let mut app = app_on(game, dark());
    let before = clock(&app);

    app.handle_key(press(KeyCode::Char('i')));
    assert!(row_text(&screen(&app), item_row(0)).contains("teach Veil · known"));
    app.handle_key(press(KeyCode::Enter));

    assert_eq!(clock(&app), before, "the page is kept and no time passes");
    assert!(matches!(app.state(), State::Inventory(_)));
    assert_eq!(
        last_log(&app).1,
        "You already know Veil: the Page of the Veil has nothing to teach."
    );
    assert_snapshot!(text(&app));
}

#[test]
fn reading_an_unknown_page_teaches_its_sigil() {
    let mut app = app_on(carrying(&["veil_page"]), dark());

    press_all(&mut app, &[KeyCode::Char('i'), KeyCode::Enter]);

    assert!(app.game().grimoire().knows("veil"));
    assert_eq!(
        last_log(&app),
        (
            Tone::Success,
            "You read the Page of the Veil: Veil joins your grimoire.".to_owned()
        )
    );
}

#[test]
fn escape_closes_the_inventory_at_no_cost() {
    let mut app = app_on(carrying(&["healing_draught", "essence_phial"]), dark());
    let before = clock(&app);
    let log = app.log().clone();

    press_all(
        &mut app,
        &[
            KeyCode::Char('i'),
            KeyCode::Down,
            KeyCode::Char('2'),
            KeyCode::End,
            KeyCode::Esc,
        ],
    );

    assert_eq!(app.state(), State::Playing);
    assert_eq!(
        clock(&app),
        before,
        "opening, choosing and closing are free"
    );
    assert_eq!(*app.log(), log);
    app.handle_key(press(KeyCode::Char('i')));
    assert_eq!(
        chosen(&app),
        Some(0),
        "a new inventory starts on the first item"
    );
}

#[test]
fn arrows_vi_keys_and_digits_move_the_choice() {
    let ids = ["healing_draught", "essence_phial", "steadying_ink"];
    let mut app = app_on(carrying(&ids), dark());
    app.handle_key(press(KeyCode::Char('i')));

    let mut seen = vec![chosen(&app)];
    for code in [
        KeyCode::Down,
        KeyCode::Char('j'),
        KeyCode::Down,
        KeyCode::Up,
        KeyCode::Char('k'),
        KeyCode::Char('3'),
        KeyCode::Char('9'),
        KeyCode::Home,
        KeyCode::End,
    ] {
        app.handle_key(press(code));
        app.handle_key(release(code));
        seen.push(chosen(&app));
    }

    let expected = [0, 1, 2, 0, 2, 1, 2, 2, 0, 2].map(Some);
    assert_eq!(seen, expected, "a release never moves the choice");
}

#[test]
fn the_chosen_line_is_marked_and_highlighted_in_every_mode() {
    for mode in MODES {
        for glyph_set in [GlyphSet::Unicode, GlyphSet::Ascii] {
            let theme = Theme::new(mode, glyph_set);
            let mut app = app_on(carrying(&["healing_draught", "steadying_ink"]), theme);
            press_all(&mut app, &[KeyCode::Char('i'), KeyCode::Down]);
            let theme = Theme::new(mode, glyph_set);
            let screen = screen(&app);
            let what = format!("{mode:?}, {glyph_set:?}");

            let (marker_cell, other_cell) =
                ((MARKER_COLUMN, item_row(1)), (MARKER_COLUMN, item_row(0)));
            let marker = &screen[marker_cell];
            assert_eq!(
                marker.symbol(),
                theme.glyphs.selection.to_string(),
                "{what}"
            );
            assert_eq!(screen[other_cell].symbol(), " ", "{what}");
            let selection = theme.bg_selection;
            for x in MARKER_COLUMN..LIST_END_COLUMN {
                let chosen = &screen[(x, item_row(1))];
                let other = &screen[(x, item_row(0))];
                if let Some(bg) = selection.bg {
                    assert_eq!(chosen.bg, bg, "{what}, column {x}");
                    assert_eq!(Some(other.bg), theme.bg_panel.bg, "{what}, column {x}");
                } else {
                    assert!(
                        chosen.modifier.contains(Modifier::REVERSED),
                        "{what}, column {x}"
                    );
                    assert!(
                        !other.modifier.contains(Modifier::REVERSED),
                        "{what}, column {x}"
                    );
                }
            }
        }
    }
}

#[test]
fn the_inventory_draws_with_its_tokens_in_every_mode() {
    for mode in MODES {
        let theme = Theme::new(mode, GlyphSet::Unicode);
        let mut app = app_on(carrying(&["healing_draught", "steadying_ink"]), theme);
        app.handle_key(press(KeyCode::Char('i')));
        let theme = Theme::new(mode, GlyphSet::Unicode);
        let screen = screen(&app);
        let what = |part: &str| format!("{mode:?}: {part}");

        assert_cell(&screen, (0, 1), '╭', theme.border, &what("frame"));
        assert_cell(&screen, (3, 1), 'I', theme.text_title, &what("title"));
        let capacity = column_of(&row_text(&screen, 1), "2/10").expect("capacity shown");
        assert_cell(&screen, (capacity, 1), '2', theme.text, &what("capacity"));
        // Second line, not chosen: digit, glyph, name and effect.
        let row = item_row(1);
        assert_cell(&screen, (4, row), '2', theme.text_muted, &what("slot"));
        assert_cell(&screen, (7, row), '♦', theme.item, &what("glyph"));
        assert_cell(&screen, (9, row), 'S', theme.text, &what("name"));
        let effect = column_of(&row_text(&screen, row), "steady").expect("effect shown");
        assert_cell(
            &screen,
            (effect, row),
            's',
            theme.text_muted,
            &what("effect"),
        );
        assert_eq!(
            Some(screen[(30, 8)].bg),
            theme.bg_panel.bg,
            "{}",
            what("panel")
        );
    }
}

#[test]
fn an_empty_inventory_explains_how_to_pick_up_and_its_keys_do_nothing() {
    let mut app = new_app();
    let before = clock(&app);
    let log = app.log().clone();

    press_all(
        &mut app,
        &[
            KeyCode::Char('i'),
            KeyCode::Enter,
            KeyCode::Char('d'),
            KeyCode::Down,
        ],
    );

    assert_eq!(chosen(&app), Some(0));
    assert_eq!(clock(&app), before);
    assert_eq!(*app.log(), log);
    let text = text(&app);
    assert!(text.contains("You carry nothing yet."), "{text}");
    assert!(text.contains(" 0/10 "), "{text}");
    assert_snapshot!(text);
}

#[test]
fn a_filled_inventory_with_the_third_item_chosen() {
    let ids = [
        "healing_draught",
        "essence_phial",
        "steadying_ink",
        "veil_page",
        "healing_draught",
    ];
    let mut app = app_on(carrying(&ids), dark());

    press_all(
        &mut app,
        &[KeyCode::Char('i'), KeyCode::Down, KeyCode::Down],
    );

    assert_eq!(chosen(&app), Some(2));
    assert_snapshot!(text(&app));
}

#[test]
fn a_map_with_items_in_view_lists_them_in_the_panel() {
    let content = content();
    let mut game = Game::in_fixed_room(SEED);
    for (id, cell) in [
        ("healing_draught", Position::new(8, 3)),
        ("steadying_ink", Position::new(10, 6)),
        ("veil_page", Position::new(3, 8)),
        ("healing_draught", Position::new(7, 7)),
    ] {
        game.place_item(cell, content.item(id).unwrap());
    }
    let app = app_on(game, dark());

    let text = text(&app);

    for line in [
        "¡ Healing Draugh ×2",
        "♦ Steadying Ink",
        "¶ Page of the Veil",
    ] {
        assert!(text.contains(line), "{line}: {text}");
    }
    assert_snapshot!(text);
}

#[test]
fn the_inventory_is_plain_ascii_with_ascii_glyphs() {
    let theme = Theme::new(ColorMode::Monochrome, GlyphSet::Ascii);
    let mut app = app_on(carrying(&["veil_page", "healing_draught"]), theme);

    press_all(&mut app, &[KeyCode::Char('i'), KeyCode::Down]);

    let text = text(&app);
    assert!(text.is_ascii(), "{text}");
    assert!(text.contains("> 2  ! Healing Draught"), "{text}");
    assert!(text.contains("^v/1-0 choose"), "{text}");
}

#[test]
fn the_key_help_names_the_item_keys_on_every_screen() {
    let mut app = app_on(carrying(&["healing_draught"]), dark());
    let help = |app: &App| row_text(&screen(app), 23);

    assert!(help(&app).contains("g pick up   i inventory   > descend   q quit"));
    app.handle_key(press(KeyCode::Char('i')));
    assert_eq!(
        help(&app).trim_end(),
        " ↑↓/1-0 choose   Enter/u use   d drop   Esc close"
    );
    let mut empty = new_app();
    empty.handle_key(press(KeyCode::Char('i')));
    assert_eq!(help(&empty).trim_end(), " Esc close");
}

#[test]
fn inventory_keys_do_nothing_below_80_by_24_but_ctrl_c_quits() {
    let mut app = app_on(carrying(&["healing_draught"]), dark());
    app.handle_key(press(KeyCode::Char('i')));
    render(&app, 60, 20);
    let before = clock(&app);

    press_all(
        &mut app,
        &[KeyCode::Enter, KeyCode::Char('d'), KeyCode::Esc],
    );

    assert_eq!(clock(&app), before);
    assert!(matches!(app.state(), State::Inventory(_)));
    app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert_eq!(app.state(), State::Quit);
}

#[test]
fn q_does_not_quit_from_the_inventory_and_i_does_not_open_it_after_death() {
    let mut app = app_on(carrying(&["healing_draught"]), dark());
    press_all(&mut app, &[KeyCode::Char('i'), KeyCode::Char('q')]);
    assert!(matches!(app.state(), State::Inventory(_)));

    let content = Content::parse(
        r#"[Monster(id: "executioner", name: "executioner", family: Acolyte, elite: true,
            speed: 100, hp: 999, attack: 99, defense: 0, min_depth: 1)]"#,
    )
    .unwrap();
    let mut game = carrying(&["healing_draught"]);
    let east = PLAYER_START.step(Direction::East);
    game.spawn_monster(east, content.monster("executioner").unwrap());
    let mut app = App::with_game(dark(), game, content);
    // A draught does not save the player from a blow of 99.
    for _ in 0..3 {
        press_all(&mut app, &[KeyCode::Char('.')]);
    }
    assert_eq!(app.state(), State::GameOver);
    app.handle_key(press(KeyCode::Char('i')));
    assert_eq!(app.state(), State::GameOver);
}
