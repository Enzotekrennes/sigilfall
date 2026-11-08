//! Combat on screen, driven by keys like a player and read back from the
//! rendered screen: the side panel, the message log and the end screen; then
//! combat, life and the journal from the player's side, where only the
//! rendered 80 × 24 screen is read back and the expected log lines are worked
//! out by a twin of the game that is fed the same actions, with the wording of
//! the design written out again here rather than taken from `messages`.

use super::*;
use crate::message_log::Tone;
use crate::ui::hud::{HP_BAR_WIDTH, filled_cells};
use ratatui::style::{Color, Style};
use sigilfall_core::actor::ActorId;
use sigilfall_core::event::Event as GameEvent;
use std::collections::HashMap;

/// Test monsters whose blows are known in advance:
/// - the cave gnat (attack 0) always deals exactly 1 damage to the
///   player (defense 1), and its 999 hit points outlast any fight;
/// - the wax moth dies from any blow of the player;
/// - the executioner (attack 99) kills the player's 30 hit points in
///   one blow.
const TEST_MONSTERS: &str = r#"[
            Monster(id: "gnat", name: "cave gnat", family: Vermin, elite: false,
                speed: 100, hp: 999, attack: 0, defense: 0, min_depth: 1),
            Monster(id: "moth", name: "wax moth", family: Vermin, elite: false,
                speed: 100, hp: 1, attack: 0, defense: 0, min_depth: 1),
            Monster(id: "executioner", name: "executioner", family: Acolyte, elite: true,
                speed: 100, hp: 999, attack: 99, defense: 0, min_depth: 1),
        ]"#;

/// Column of the first character of the side panel's content.
const PANEL_LEFT: u16 = 60;
/// Last column of the side panel's content on an 80-column screen.
const PANEL_RIGHT: u16 = 78;
/// Row of "HP" and its value.
const HP_ROW: u16 = 1;
/// Row of the health bar.
const BAR_ROW: u16 = 2;
/// Row of the "IN VIEW" title, under the essence and the sigils
/// block of an empty grimoire ("SIGILS" and "none yet").
const IN_VIEW_ROW: u16 = 8;
/// Row of the log's separator.
const LOG_SEPARATOR_ROW: u16 = 17;
/// First and last rows of the log's messages.
const LOG_ROWS: std::ops::RangeInclusive<u16> = 18..=22;

/// Picks the health token a level is drawn with out of a theme.
type HealthToken = fn(&Theme) -> Style;

/// The four rendering modes of `docs/design.md`.
const MODES: [ColorMode; 4] = [
    ColorMode::TrueColorDark,
    ColorMode::TrueColorLight,
    ColorMode::Ansi16,
    ColorMode::Monochrome,
];

/// An application in the fixed room of [`SEED`], drawn with
/// `theme`, with a monster of each `(definition, cell)` of
/// [`TEST_MONSTERS`]. Returns the keys of the monsters, in order.
fn arena(theme: Theme, monsters: &[(&str, Position)]) -> (App, Vec<ActorId>) {
    let content = Content::parse(TEST_MONSTERS).expect("valid test monsters");
    let mut game = Game::in_fixed_room(SEED);
    let ids = monsters
        .iter()
        .map(|&(id, cell)| game.spawn_monster(cell, content.monster(id).unwrap()))
        .collect();
    (App::with_game(theme, game, content), ids)
}

/// The cell east of the player's start, where the duels happen.
fn east() -> Position {
    PLAYER_START.step(Direction::East)
}

/// The default theme of Windows Terminal (see [`new_app`]).
fn dark() -> Theme {
    Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode)
}

/// The player's hit points.
fn player_hp(app: &App) -> i32 {
    app.game().player_actor().hp()
}

/// Waits until the cave gnat, one blow of 1 per turn, has brought
/// the player down to `hp`; bounded so a gnat that never strikes
/// fails the test instead of looping.
fn wait_until_hp(app: &mut App, hp: i32) {
    for _ in 0..100 {
        if player_hp(app) <= hp {
            break;
        }
        app.handle_key(press(KeyCode::Char('.')));
    }
    assert_eq!(player_hp(app), hp, "the gnat deals exactly 1 per blow");
}

/// Rows `rows` of `buffer`, as text.
fn rows(buffer: &Buffer, rows: std::ops::RangeInclusive<u16>) -> Vec<String> {
    rows.map(|y| row_text(buffer, y)).collect()
}

/// The text of the side panel's content on row `y`, trimmed.
fn panel_text(buffer: &Buffer, y: u16) -> String {
    (PANEL_LEFT..=PANEL_RIGHT)
        .map(|x| buffer[(x, y)].symbol())
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Asserts that the cell `(x, y)` of `buffer` has exactly the
/// colour and emphasis of `style`.
fn assert_style(buffer: &Buffer, (x, y): (u16, u16), style: Style, what: &str) {
    let cell = &buffer[(x, y)];
    assert_eq!(cell.fg, style.fg.unwrap_or(Color::Reset), "{what}");
    assert_eq!(cell.modifier, style.add_modifier, "{what}");
}

#[test]
fn a_new_game_shows_full_health_and_the_welcome_message() {
    let app = new_app();
    let screen = render(&app, 80, 24).backend().buffer().clone();

    assert_eq!(
        panel_text(&screen, HP_ROW),
        format!("HP{}30/30", " ".repeat(12))
    );
    assert_eq!(panel_text(&screen, BAR_ROW), "█".repeat(18));
    assert_eq!(panel_text(&screen, IN_VIEW_ROW), "IN VIEW");
    assert_eq!(panel_text(&screen, IN_VIEW_ROW + 1), "@ you");
    assert_eq!(
        row_text(&screen, 18).trim_end(),
        "   You enter the dungeon."
    );
    let separator = row_text(&screen, LOG_SEPARATOR_ROW);
    assert!(separator.starts_with("─ Log ──"), "{separator:?}");
    assert_eq!(
        screen[(58, LOG_SEPARATOR_ROW)].symbol(),
        "┴",
        "the panel's border meets the separator"
    );
    assert_eq!(screen[(58, 1)].symbol(), "│");
}

#[test]
fn each_blow_is_logged_with_its_damage_and_the_health_bar_drops() {
    let (mut app, ids) = arena(dark(), &[("gnat", east())]);
    let gnat_hp = |app: &App| app.game().actor(ids[0]).unwrap().hp();

    let before = gnat_hp(&app);
    app.handle_key(press(KeyCode::Char('l')));
    let dealt = before - gnat_hp(&app);
    for _ in 0..3 {
        app.handle_key(press(KeyCode::Char('.')));
    }

    let screen = render(&app, 80, 24).backend().buffer().clone();
    let log = rows(&screen, LOG_ROWS);
    assert!(
        log.iter()
            .any(|line| line.trim_end() == format!("   You hit the cave gnat ({dealt}).")),
        "{log:#?}"
    );
    let taken = 30 - player_hp(&app);
    assert!(taken >= 1, "the gnat struck back");
    let danger = log
        .iter()
        .filter(|line| line.trim_end() == " ! The cave gnat hits you (1).")
        .count();
    assert_eq!(danger, usize::try_from(taken).unwrap(), "{log:#?}");
    let hp = player_hp(&app);
    assert_eq!(
        panel_text(&screen, HP_ROW),
        format!("HP{}{hp}/30", " ".repeat(19 - 2 - format!("{hp}/30").len()))
    );
    let filled = usize::from(filled_cells(hp, 30, HP_BAR_WIDTH));
    assert_eq!(
        panel_text(&screen, BAR_ROW),
        format!("{}{}", "█".repeat(filled), "░".repeat(18 - filled))
    );
    // D25: the gnat's line ends with its own health.
    assert_eq!(
        panel_text(&screen, IN_VIEW_ROW + 2),
        format!("v cave gnat {}/999", gnat_hp(&app))
    );
}

#[test]
fn danger_lines_carry_the_danger_colour_and_the_bang_prefix_in_every_mode() {
    for mode in MODES {
        let theme = Theme::new(mode, GlyphSet::Unicode);
        let (mut app, _) = arena(theme.clone(), &[("gnat", east())]);
        wait_until_hp(&mut app, 29);

        let screen = render(&app, 80, 24).backend().buffer().clone();
        let row = LOG_ROWS
            .clone()
            .find(|&y| row_text(&screen, y).contains("hits you"))
            .unwrap_or_else(|| panic!("{mode:?}: the blow is logged"));
        assert_eq!(screen[(1, row)].symbol(), "!", "{mode:?}");
        assert_style(
            &screen,
            (1, row),
            theme.log_danger,
            &format!("{mode:?}: prefix"),
        );
        assert_style(
            &screen,
            (3, row),
            theme.log_danger,
            &format!("{mode:?}: text"),
        );
        assert_style(
            &screen,
            (3, 18),
            theme.log_info,
            &format!("{mode:?}: welcome"),
        );
    }
}

#[test]
fn a_killing_blow_names_the_victim_and_removes_it_from_the_panel() {
    let (mut app, ids) = arena(dark(), &[("moth", east())]);
    let before = render(&app, 80, 24).backend().buffer().clone();
    assert_eq!(panel_text(&before, IN_VIEW_ROW + 2), "v wax moth      1/1");

    app.handle_key(press(KeyCode::Char('l')));

    assert!(app.game().actor(ids[0]).is_none(), "the moth died");
    let screen = render(&app, 80, 24).backend().buffer().clone();
    let log = rows(&screen, LOG_ROWS);
    let kill = log
        .iter()
        .find(|line| line.starts_with(" + You kill the wax moth ("))
        .unwrap_or_else(|| panic!("{log:#?}"));
    assert!(kill.trim_end().ends_with(")."), "{kill:?}");
    assert!(!log.iter().any(|line| line.contains("dies")), "{log:#?}");
    assert_style(&screen, (1, 19), dark().log_success, "kill line");
    assert_eq!(panel_text(&screen, IN_VIEW_ROW + 2), "");
}

#[test]
fn the_log_scrolls_to_keep_the_five_newest_messages() {
    // A wall bump writes nothing, so alternate blows and waits:
    // each turn adds the player's blow and the gnat's reply.
    let (mut app, _) = arena(dark(), &[("gnat", east())]);
    for _ in 0..4 {
        app.handle_key(press(KeyCode::Char('l')));
    }
    assert!(app.log().len() > 5, "more messages than rows");

    let screen = render(&app, 80, 24).backend().buffer().clone();
    let shown: Vec<_> = rows(&screen, LOG_ROWS)
        .into_iter()
        .map(|line| line[3..].trim_end().to_owned())
        .collect();
    let mut newest: Vec<_> = app
        .log()
        .newest_first()
        .take(5)
        .map(|message| message.text.clone())
        .collect();
    newest.reverse();
    assert_eq!(shown, newest, "oldest at the top, newest at the bottom");
    assert!(!shown.iter().any(|line| line.contains("enter the dungeon")));
}

#[test]
fn walking_into_a_wall_writes_nothing_in_the_log() {
    let mut app = new_app();
    press_all(&mut app, &[KeyCode::Char('h'); 6]);

    assert_eq!(app.log().len(), 1, "only the welcome message");
}

#[test]
fn the_health_value_and_bar_follow_the_thresholds_in_every_mode() {
    // 16/30 is above half, 15/30 exactly half (mid), 8/30 the last
    // mid value, 7/30 under a quarter (low, marked "!").
    let levels: [(i32, &str, HealthToken); 4] = [
        (16, "16/30", |t| t.hp_high),
        (15, "15/30", |t| t.hp_mid),
        (8, "8/30", |t| t.hp_mid),
        (7, "! 7/30", |t| t.hp_low),
    ];
    for mode in MODES {
        for glyph_set in [GlyphSet::Unicode, GlyphSet::Ascii] {
            let theme = Theme::new(mode, glyph_set);
            let (mut app, _) = arena(theme.clone(), &[("gnat", east())]);
            for (hp, value, token) in levels {
                wait_until_hp(&mut app, hp);
                let case = format!("{mode:?}, {glyph_set:?}, {hp} HP");
                let screen = render(&app, 80, 24).backend().buffer().clone();

                let figures = panel_text(&screen, HP_ROW);
                assert!(figures.starts_with("HP "), "{case}: {figures:?}");
                assert!(figures.ends_with(value), "{case}: {figures:?}");
                assert_eq!(
                    row_text(&screen, HP_ROW).chars().nth(78),
                    value.chars().last(),
                    "{case}: right-aligned"
                );
                assert_style(&screen, (PANEL_RIGHT, HP_ROW), token(&theme), &case);
                assert_style(&screen, (PANEL_LEFT, HP_ROW), theme.text_muted, &case);

                let filled = filled_cells(hp, 30, HP_BAR_WIDTH);
                for offset in 0..HP_BAR_WIDTH {
                    let cell = (PANEL_LEFT + offset, BAR_ROW);
                    let (glyph, style) = if offset < filled {
                        (theme.glyphs.bar_full, token(&theme))
                    } else {
                        (theme.glyphs.bar_empty, theme.border)
                    };
                    assert_eq!(
                        screen[cell].symbol(),
                        glyph.to_string(),
                        "{case}: bar cell {offset}"
                    );
                    assert_style(&screen, cell, style, &format!("{case}: bar cell {offset}"));
                }
            }
        }
    }
}

#[test]
fn monochrome_tells_low_health_by_the_mark_alone() {
    let theme = Theme::new(ColorMode::Monochrome, GlyphSet::Ascii);
    let (mut app, _) = arena(theme, &[("gnat", east())]);
    wait_until_hp(&mut app, 8);
    let mid = render(&app, 80, 24).backend().buffer().clone();
    wait_until_hp(&mut app, 7);
    let low = render(&app, 80, 24).backend().buffer().clone();

    assert!(!panel_text(&mid, HP_ROW).contains('!'));
    assert!(panel_text(&low, HP_ROW).contains("! 7/30"));
    assert_eq!(
        panel_text(&low, BAR_ROW),
        format!("{}{}", "=".repeat(5), "-".repeat(13))
    );
}

/// An application whose player was just killed by the executioner,
/// with the turn of the death.
fn killed_app(theme: Theme) -> App {
    let (mut app, _) = arena(theme, &[("executioner", east())]);
    for _ in 0..10 {
        if app.state() == State::GameOver {
            break;
        }
        app.handle_key(press(KeyCode::Char('.')));
    }
    assert_eq!(app.state(), State::GameOver);
    assert!(app.game().is_over());
    app
}

#[test]
fn the_player_death_is_logged_and_the_end_screen_names_the_killer() {
    let app = killed_app(dark());
    let turn = app.game().player_turns();

    let last = app.log().newest_first().next().unwrap();
    assert!(
        last.text.starts_with("The executioner kills you (") && last.text.ends_with(")."),
        "{last:?}"
    );
    let screen = render(&app, 80, 24).backend().buffer().clone();
    let text = rows(&screen, 0..=23).join(
        "
",
    );
    assert!(
        text.contains(&format!(
            "Killed by an executioner at depth 1, on turn {turn}."
        )),
        "{text}"
    );
    assert!(text.contains("SEED   7F3A-91C2-0B5E-D4A8"), "{text}");
    assert!(
        text.contains("Replay this seed: sigilfall --seed 7F3A-91C2-0B5E-D4A8"),
        "{text}"
    );
    assert!(
        row_text(&screen, 23).starts_with(" Enter title screen   q quit"),
        "{text}"
    );
    assert!(!text.contains("Turn "), "no game screen:\n{text}");
    assert_snapshot!(render(&app, 80, 24).backend());
}

#[test]
fn no_game_action_is_played_after_death_but_quitting_works() {
    let clock = ManualClock::new();
    let mut app = killed_app(dark()).with_clock(clock.clone());
    clock.advance(END_SCREEN_GRACE);
    let ticks = app.game().ticks();
    let turns = app.game().player_turns();
    let messages = app.log().len();
    let before = render(&app, 80, 24).backend().buffer().clone();

    press_all(
        &mut app,
        &[
            KeyCode::Char('l'),
            KeyCode::Right,
            KeyCode::Char('.'),
            KeyCode::Char('5'),
            KeyCode::Esc,
        ],
    );

    assert_eq!(app.state(), State::GameOver);
    assert_eq!(app.game().ticks(), ticks, "no time passes");
    assert_eq!(app.game().player_turns(), turns);
    assert_eq!(app.log().len(), messages, "nothing new is logged");
    assert_eq!(
        render(&app, 80, 24).backend().buffer(),
        &before,
        "the end screen stays as it is"
    );

    // Escape never quits, not even here: only `q` and Ctrl+C do.
    app.handle_key(press(KeyCode::Esc));
    assert_eq!(app.state(), State::GameOver);
    app.handle_key(press(KeyCode::Char('q')));
    assert_eq!(app.state(), State::Quit);
}

#[test]
fn the_end_screen_also_asks_for_room_below_80_by_24() {
    let app = killed_app(dark());

    let text = render(&app, 79, 24).backend().to_string();

    assert!(text.contains("Enlarge the terminal"), "{text}");
    assert!(!text.contains("Killed by"), "{text}");
}

#[test]
fn a_fight_on_screen_shows_the_panel_and_the_log_together() {
    let (mut app, _) = arena(dark(), &[("gnat", east()), ("moth", Position::new(6, 7))]);
    press_all(
        &mut app,
        &[KeyCode::Char('l'), KeyCode::Char('l'), KeyCode::Char('.')],
    );

    assert_snapshot!(render(&app, 80, 24).backend());
}

#[test]
fn the_combat_screens_are_plain_ascii_with_ascii_glyphs() {
    let theme = Theme::new(ColorMode::Monochrome, GlyphSet::Ascii);
    let (mut app, _) = arena(theme.clone(), &[("gnat", east())]);
    wait_until_hp(&mut app, 5);
    let playing = render(&app, 80, 24).backend().to_string();
    assert!(playing.is_ascii(), "{playing}");

    let dead = killed_app(theme);
    let end = render(&dead, 80, 24).backend().to_string();
    assert!(end.is_ascii(), "{end}");
    assert_snapshot!(playing);
}

/// Creatures whose blows are known in advance.
/// - the gnat deals exactly 1 to the player and outlasts any fight;
/// - the sloth has 999 hit points and acts once per hundred actions
///   of the player (speed 1);
/// - the moth dies from any blow of the player;
/// - the executioner kills the player in one blow;
/// - the hammer deals 3 to 5 to the player, so the player dies in
///   6 to 10 blows, the exact number depending on the seed;
/// - the brute has 40 hit points and 2 defense.
const MONSTERS: &str = r#"[
            Monster(id: "gnat", name: "cave gnat", family: Vermin, elite: false,
                speed: 100, hp: 999, attack: 0, defense: 0, min_depth: 1),
            Monster(id: "sloth", name: "sloth", family: Vermin, elite: false,
                speed: 1, hp: 999, attack: 0, defense: 0, min_depth: 1),
            Monster(id: "moth", name: "wax moth", family: Vermin, elite: false,
                speed: 100, hp: 1, attack: 0, defense: 0, min_depth: 1),
            Monster(id: "executioner", name: "executioner", family: Acolyte, elite: true,
                speed: 100, hp: 999, attack: 99, defense: 0, min_depth: 1),
            Monster(id: "hammer", name: "iron hammer", family: Homunculus, elite: false,
                speed: 100, hp: 999, attack: 5, defense: 0, min_depth: 1),
            Monster(id: "brute", name: "brute", family: Acolyte, elite: false,
                speed: 100, hp: 40, attack: 0, defense: 2, min_depth: 1),
        ]"#;

/// Messages the log keeps (T16: "100 entries"), written out here so
/// that a change of the limit is noticed.
const HISTORY: usize = 100;

/// The rows [`LOG_ROWS`] of the log of `app` as the game screen
/// draws it at 80 × 24 (separator on row 17), for the fatal blow:
/// the death summary that replaces the game screen (T25) does not
/// show the log.
fn drawn_log(app: &App) -> Vec<String> {
    let mut buffer = Buffer::empty(Rect::new(0, 0, 80, 24));
    let area = Rect::new(0, *LOG_ROWS.start() - 1, 80, 6);
    ui::log::render(&mut buffer, area, None, app.log(), &app.theme);
    rows(&buffer, LOG_ROWS)
}
/// Cells of the health bar.
const BAR_CELLS: usize = 18;

/// The seeds named by the brief.
fn seeds() -> [Seed; 3] {
    [
        Seed::new(0),
        Seed::new(42),
        "7F3A-91C2-0B5E-D4A8".parse().expect("valid grouped seed"),
    ]
}

/// A fight: the application, its twin game, and the name of each
/// monster by key.
struct Fight {
    app: App,
    twin: Game,
    names: HashMap<ActorId, String>,
    ids: Vec<ActorId>,
    /// The messages the log must hold, oldest first, as
    /// `(prefix, text)`.
    expected: Vec<(char, String)>,
}

impl Fight {
    /// A fixed-room game of `seed` with one monster per
    /// `(definition, cell)`, in the application and in its twin.
    fn new(seed: Seed, theme: Theme, monsters: &[(&str, Position)]) -> Self {
        Self::with_content(seed, theme, MONSTERS, monsters)
    }

    /// As [`Fight::new`], with the creatures of `file`.
    fn with_content(seed: Seed, theme: Theme, file: &str, monsters: &[(&str, Position)]) -> Self {
        let content = Content::parse(file).expect("valid test monsters");
        let mut game = Game::in_fixed_room(seed);
        let mut twin = Game::in_fixed_room(seed);
        let mut names = HashMap::new();
        let mut ids = Vec::new();
        for &(definition, cell) in monsters {
            let def = content.monster(definition).expect("known monster");
            let id = game.spawn_monster(cell, def);
            assert_eq!(twin.spawn_monster(cell, def), id, "twin keys agree");
            names.insert(id, def.name().to_owned());
            ids.push(id);
        }
        Self {
            app: App::with_game(theme, game, content),
            twin,
            names,
            ids,
            expected: vec![(' ', "You enter the dungeon.".to_owned())],
        }
    }

    /// Presses `code` in the application and plays `action` in the
    /// twin, which tells what the log must gain. Returns the twin's
    /// events.
    fn play(&mut self, code: KeyCode, action: Action) -> Vec<GameEvent> {
        self.app.handle_key(press(code));
        let events = self.twin.submit(action);
        for event in &events {
            if let GameEvent::Attacked {
                attacker,
                target,
                damage,
                hp_left,
            } = event
            {
                let line = self.blow_line(*attacker, *target, *damage, *hp_left);
                self.expected.push(line);
            }
        }
        events
    }

    /// Presses `l` (east) in the application and the twin.
    fn hit_east(&mut self) -> Vec<GameEvent> {
        self.play(KeyCode::Char('l'), Action::Move(Direction::East))
    }

    /// Presses `.` (wait) in the application and the twin.
    fn wait(&mut self) -> Vec<GameEvent> {
        self.play(KeyCode::Char('.'), Action::Wait)
    }

    /// The line a blow must produce: `You hit the sloth (4).`,
    /// `You kill the wax moth (5).`, `The cave gnat hits you (1).`
    /// or `The executioner kills you (99).`, with the prefix of its
    /// tone.
    fn blow_line(
        &self,
        attacker: ActorId,
        target: ActorId,
        damage: i32,
        hp_left: i32,
    ) -> (char, String) {
        let lethal = hp_left == 0;
        if attacker == self.twin.player() {
            let name = &self.names[&target];
            if lethal {
                ('+', format!("You kill the {name} ({damage})."))
            } else {
                (' ', format!("You hit the {name} ({damage})."))
            }
        } else {
            let name = &self.names[&attacker];
            let verb = if lethal { "kills" } else { "hits" };
            ('!', format!("The {name} {verb} you ({damage})."))
        }
    }

    /// The screen at 80 × 24.
    fn screen(&self) -> Buffer {
        render(&self.app, 80, 24).backend().buffer().clone()
    }

    /// What the log should hold, oldest first, newest last, capped
    /// as the history is.
    fn expected_history(&self) -> &[(char, String)] {
        let skip = self.expected.len().saturating_sub(HISTORY);
        &self.expected[skip..]
    }

    /// Checks the whole log and, while the game screen shows it,
    /// the visible rows against the twin's account. The death
    /// summary does not show the log.
    fn assert_log_is_exact(&self, context: &str) {
        let expected = self.expected_history();
        let mut kept: Vec<(char, String)> = self
            .app
            .log()
            .newest_first()
            .map(|m| (prefix_of(m.tone), m.text.clone()))
            .collect();
        kept.reverse();
        assert_eq!(kept, expected, "{context}: log content");
        if self.app.state() == State::GameOver {
            return;
        }

        let shown: Vec<String> = rows(&self.screen(), LOG_ROWS)
            .into_iter()
            .map(|line| line.trim_end().to_owned())
            .filter(|line| !line.is_empty())
            .collect();
        let wanted: Vec<String> = expected
            .iter()
            .rev()
            .take(5)
            .rev()
            .map(|(prefix, text)| format!(" {prefix} {text}"))
            .collect();
        assert_eq!(shown, wanted, "{context}: visible rows");
    }
}

/// The log prefix of a tone, as `docs/design.md` writes it.
const fn prefix_of(tone: Tone) -> char {
    match tone {
        Tone::Info => ' ',
        Tone::Success => '+',
        Tone::Danger => '!',
        Tone::Sigil => '~',
    }
}

/// The map area of an 80 × 24 `buffer`, as text rows.
fn map_text(buffer: &Buffer) -> Vec<String> {
    let area = ui::game::map_area(buffer.area);
    (area.top()..area.bottom())
        .map(|y| {
            (area.left()..area.right())
                .map(|x| buffer[(x, y)].symbol())
                .collect()
        })
        .collect()
}

/// The glyph drawn where the map cell `cell` is, on `app`'s 80 × 24
/// screen.
fn glyph_at(app: &App, cell: Position) -> String {
    let at = camera(app, 80, 24)
        .screen_position(cell)
        .unwrap_or_else(|| panic!("{cell:?} is off screen"));
    render(app, 80, 24).backend().buffer()[(at.x, at.y)]
        .symbol()
        .to_owned()
}

/// Health read back from the panel: `(hp, max, filled cells, empty
/// cells)`.
fn panel_health(buffer: &Buffer) -> (i32, i32, usize, usize) {
    let value = panel_text(buffer, HP_ROW);
    let figures = value
        .trim_start_matches("HP")
        .trim()
        .trim_start_matches('!')
        .trim();
    let (hp, max) = figures
        .split_once('/')
        .unwrap_or_else(|| panic!("no hp/max on {value:?}"));
    let bar = panel_text(buffer, BAR_ROW);
    let full = bar.chars().filter(|&c| c == '█').count();
    let empty = bar.chars().filter(|&c| c == '░').count();
    (hp.parse().unwrap(), max.parse().unwrap(), full, empty)
}

/// The number after "Turn" on the status line.
fn turn_shown(app: &App) -> u64 {
    let first = row_text(render(app, 80, 24).backend().buffer(), 0);
    first
        .trim()
        .split_once("Turn ")
        .and_then(|(_, rest)| rest.split_whitespace().next())
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| panic!("no turn counter on {first:?}"))
}

// --- Scenario 1 -------------------------------------------------

#[test]
fn s1_moving_into_a_monster_attacks_it_and_the_damage_is_in_the_log() {
    for seed in seeds() {
        let mut fight = Fight::new(seed, dark(), &[("brute", east())]);
        let start = player_position(&fight.app);

        let events = fight.hit_east();

        let GameEvent::Attacked { damage, .. } = events[0] else {
            panic!("seed {seed}: no blow: {events:?}");
        };
        assert!((1..=3).contains(&damage), "4 - 2 +/- 1, got {damage}");
        assert_eq!(player_position(&fight.app), start, "the player stays");
        assert_eq!(
            fight.app.game().actor(fight.ids[0]).unwrap().position(),
            east(),
            "the monster stays"
        );
        fight.assert_log_is_exact(&format!("seed {seed}"));
        let log = rows(&fight.screen(), LOG_ROWS);
        assert!(
            log.iter()
                .any(|l| l.trim_end() == format!("   You hit the brute ({damage}).")),
            "{log:#?}"
        );
        assert_eq!(
            glyph_at(&fight.app, east()),
            "a",
            "the brute is still drawn"
        );
    }
}

#[test]
fn s1_the_player_bar_and_value_follow_the_core_after_every_blow() {
    for seed in seeds() {
        let mut fight = Fight::new(seed, dark(), &[("gnat", east())]);
        let mut last_cells = BAR_CELLS;
        let mut stages = Vec::new();
        for turn in 0..40 {
            fight.hit_east();
            if fight.app.state() != State::Playing {
                break;
            }
            let hp = fight.app.game().player_actor().hp();
            let (shown, max, full, empty) = panel_health(&fight.screen());
            let context = format!("seed {seed} turn {turn}");
            assert_eq!((shown, max), (hp, 30), "{context}: value");
            assert_eq!(full + empty, BAR_CELLS, "{context}: bar width");
            assert!(full >= 1, "{context}: a living player has a cell");
            assert_eq!(
                full == BAR_CELLS,
                hp == 30,
                "{context}: only full health fills"
            );
            assert!(full <= last_cells, "{context}: the bar never grows");
            // Within one cell of the true share, rounded up.
            let share = f64::from(hp) * 18.0 / 30.0;
            assert!(
                (full as f64 - share).abs() < 1.0 + 1e-9,
                "{context}: {full} cells for {hp}/30"
            );
            last_cells = full;
            stages.push((hp, full));
        }
        assert!(
            stages.len() > 20,
            "seed {seed}: the fight was too short to prove anything"
        );
        assert!(
            stages.last().unwrap().1 < BAR_CELLS / 2,
            "seed {seed}: {stages:?}"
        );
    }
}

#[test]
fn s1_the_panel_gives_each_monster_its_current_and_maximum_health() {
    // D25 (2026-11-08): the "in view" line of a monster ends with
    // its hit points, current out of maximum, as figures only.
    let mut fight = Fight::new(Seed::new(42), dark(), &[("brute", east())]);
    fight.hit_east();
    let hp = fight.app.game().actor(fight.ids[0]).unwrap().hp();
    assert!(hp < 40, "the blow wounded the brute");
    let screen = fight.screen();
    // The "in view" list, from its title down: the essence and the
    // sigils count above it are figures of the player.
    let panel: Vec<String> = (3..=16)
        .map(|y| panel_text(&screen, y))
        .skip_while(|line| line != "IN VIEW")
        .collect();
    assert!(!panel.is_empty(), "the panel lists what is in view");

    let line = panel
        .iter()
        .find(|l| l.starts_with("a brute"))
        .unwrap_or_else(|| panic!("{panel:#?}"));
    assert!(line.ends_with(&format!(" {hp}/40")), "{panel:#?}");
    assert!(!panel.iter().any(|l| l.contains('█')), "{panel:#?}");
}

// --- Scenario 2 -------------------------------------------------

#[test]
fn s2_a_dead_monster_disappears_from_the_map_and_the_log_announces_it() {
    for seed in seeds() {
        let mut fight = Fight::new(seed, dark(), &[("moth", east())]);
        assert_eq!(glyph_at(&fight.app, east()), "v", "seed {seed}");

        let events = fight.hit_east();

        assert!(
            events.iter().any(|e| matches!(e, GameEvent::Died { .. })),
            "seed {seed}"
        );
        assert!(fight.app.game().actor(fight.ids[0]).is_none());
        let screen = fight.screen();
        assert!(
            !map_text(&screen).iter().any(|row| row.contains('v')),
            "seed {seed}: the corpse is still drawn"
        );
        assert_eq!(
            glyph_at(&fight.app, east()),
            "·",
            "the cell shows floor again"
        );
        fight.assert_log_is_exact(&format!("seed {seed}"));
        let log = rows(&screen, LOG_ROWS);
        let kills = log.iter().filter(|l| l.contains("wax moth")).count();
        assert_eq!(kills, 1, "announced exactly once: {log:#?}");
        assert!(
            log.iter()
                .any(|l| l.starts_with(" + You kill the wax moth (")),
            "{log:#?}"
        );
        // The freed cell can be entered.
        fight.hit_east();
        assert_eq!(player_position(&fight.app), east(), "seed {seed}");
    }
}

#[test]
fn s2_one_of_two_monsters_dies_and_only_that_one_goes() {
    let west = PLAYER_START.step(Direction::West);
    let mut fight = Fight::new(Seed::new(42), dark(), &[("moth", east()), ("gnat", west)]);
    fight.hit_east();

    let screen = fight.screen();
    assert!(fight.app.game().actor(fight.ids[0]).is_none());
    assert!(fight.app.game().actor(fight.ids[1]).is_some());
    assert_eq!(glyph_at(&fight.app, east()), "·");
    assert_eq!(glyph_at(&fight.app, west), "v", "the gnat is still there");
    fight.assert_log_is_exact("two monsters");
    let panel: Vec<String> = (3..=10).map(|y| panel_text(&screen, y)).collect();
    let gnat_hp = fight.app.game().actor(fight.ids[1]).unwrap().hp();
    let gnat = format!("v cave gnat {gnat_hp}/999");
    assert!(panel.contains(&gnat), "{panel:#?}");
    assert!(!panel.iter().any(|l| l.contains("moth")), "{panel:#?}");
}

#[test]
fn a_killing_blow_is_named_whatever_the_overkill() {
    // The moth has 1 hit point; the blow deals 3 to 5, and the log
    // gives the blow's damage, not the hit points that were left.
    for seed in seeds() {
        let mut fight = Fight::new(seed, dark(), &[("moth", east())]);
        let events = fight.hit_east();
        let GameEvent::Attacked {
            damage, hp_left, ..
        } = events[0]
        else {
            panic!("{events:?}")
        };
        assert_eq!(hp_left, 0);
        assert!(damage >= 3, "the formula gives 3 to 5, got {damage}");
        fight.assert_log_is_exact(&format!("seed {seed}"));
        assert_eq!(
            fight.expected.last().unwrap(),
            &('+', format!("You kill the wax moth ({damage})."))
        );
    }
}

// --- Scenario 3 -------------------------------------------------

/// A fight with the hammer, played until the player dies (bounded).
/// Returns it with the turn shown on the status line when the
/// fatal action was played.
fn fight_to_death(seed: Seed) -> (Fight, u64) {
    let mut fight = Fight::new(seed, dark(), &[("hammer", east())]);
    let mut turn_of_death = 0;
    for _ in 0..60 {
        if fight.app.state() == State::GameOver {
            break;
        }
        turn_of_death = turn_shown(&fight.app);
        fight.wait();
    }
    assert_eq!(fight.app.state(), State::GameOver, "seed {seed}");
    (fight, turn_of_death)
}

#[test]
fn s3_the_death_of_the_player_stops_the_game_and_opens_the_end_screen() {
    for seed in seeds() {
        let (fight, turn) = fight_to_death(seed);

        assert!(fight.app.game().is_over());
        assert_eq!(fight.app.game().player_actor().hp(), 0);
        assert_eq!(fight.app.game().player_turns(), turn);
        fight.assert_log_is_exact(&format!("seed {seed}"));
        let text = rows(&fight.screen(), 0..=23).join("\n");
        assert!(text.contains("T H E   D E S C E N T   E N D S"), "{text}");
        assert!(
            text.contains(&format!(
                "Killed by an iron hammer at depth 1, on turn {turn}."
            )),
            "cause and turn, seed {seed}:\n{text}"
        );
        assert!(text.contains(&format!("SEED   {seed}")), "{text}");
        assert!(!text.contains("Turn "), "no game screen:\n{text}");
        assert!(!text.contains("IN VIEW"), "no side panel:\n{text}");
        let last = fight.expected.last().unwrap();
        assert!(
            last.1.starts_with("The iron hammer kills you ("),
            "{last:?}"
        );
    }
}

#[test]
fn s3_the_seed_on_the_end_screen_is_reusable_as_it_is_written() {
    for seed in seeds() {
        let (fight, _) = fight_to_death(seed);
        let text = rows(&fight.screen(), 0..=23).join("\n");

        // The seed is written in its frame after "SEED" and in the
        // replay command.
        let shown = text
            .lines()
            .find_map(|l| l.split_once("SEED"))
            .map(|(_, rest)| rest.trim().trim_end_matches('│').trim())
            .expect("a seed line");
        let hint = text
            .lines()
            .find_map(|l| l.trim().strip_prefix("Replay this seed: sigilfall --seed "))
            .expect("a replay line")
            .trim();
        assert_eq!(shown, hint);
        let parsed: Seed = shown.parse().expect("the shown seed parses");
        assert_eq!(parsed, seed);

        // Replaying it with the same keys gives the same death.
        let (replay, _) = fight_to_death(parsed);
        assert_eq!(
            rows(&replay.screen(), 0..=23),
            rows(&fight.screen(), 0..=23),
            "seed {seed}: the replay differs"
        );
    }
    // And the seed does drive the blows.
    let ends: Vec<_> = seeds()
        .into_iter()
        .map(|seed| fight_to_death(seed).0.expected)
        .collect();
    assert!(
        ends[0] != ends[1] || ends[1] != ends[2],
        "the seed drives nothing"
    );
}

/// Every key but the quit ones and Enter, which goes back to the
/// title screen since T26, with modifiers that matter.
fn all_non_quit_keys() -> Vec<KeyEvent> {
    let mut keys = Vec::new();
    for c in (' '..='~').filter(|&c| c != 'q') {
        keys.push(press(KeyCode::Char(c)));
        keys.push(KeyEvent::new(KeyCode::Char(c), KeyModifiers::SHIFT));
        keys.push(KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT));
        if c != 'c' {
            keys.push(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
        }
    }
    keys.push(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL));
    keys.push(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::ALT));
    for code in [
        KeyCode::Esc,
        KeyCode::Tab,
        KeyCode::BackTab,
        KeyCode::Backspace,
        KeyCode::Delete,
        KeyCode::Insert,
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::Home,
        KeyCode::End,
        KeyCode::PageUp,
        KeyCode::PageDown,
        KeyCode::KeypadBegin,
    ] {
        keys.push(press(code));
    }
    keys.extend((1..=12).map(|n| press(KeyCode::F(n))));
    // A release or a repeat of a quit key is not a press.
    keys.push(release(KeyCode::Char('q')));
    keys.push(KeyEvent::new_with_kind(
        KeyCode::Char('q'),
        KeyModifiers::NONE,
        KeyEventKind::Repeat,
    ));
    keys
}

#[test]
fn s3_after_death_no_key_but_quit_has_any_effect() {
    for seed in seeds() {
        let (mut fight, _) = fight_to_death(seed);
        let before = fight.screen();
        let game = fight.app.game().clone();
        let log = fight.app.log().clone();

        let keys = all_non_quit_keys();
        assert!(keys.len() > 300, "the key set is broad");
        for key in keys {
            fight.app.handle_key(key);
            assert_eq!(
                fight.app.state(),
                State::GameOver,
                "{key:?} left the end screen"
            );
        }

        assert_eq!(fight.app.game().ticks(), game.ticks());
        assert_eq!(fight.app.game().player_turns(), game.player_turns());
        assert_eq!(fight.app.game().player_actor().hp(), 0);
        assert_eq!(
            fight.app.game().player_actor().position(),
            game.player_actor().position()
        );
        assert_eq!(fight.app.game().status(), game.status());
        assert_eq!(*fight.app.log(), log, "the log is untouched");
        assert_eq!(fight.screen(), before, "the screen is untouched");
    }
}

#[test]
fn s3_every_quit_key_still_works_after_death() {
    let quits = [
        press(KeyCode::Char('q')),
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
    ];
    for key in quits {
        let (fight, _) = fight_to_death(Seed::new(42));
        let clock = ManualClock::new();
        let mut app = fight.app.with_clock(clock.clone());
        clock.advance(END_SCREEN_GRACE);
        app.handle_key(key);
        assert_eq!(app.state(), State::Quit, "{key:?}");
    }
}

#[test]
fn s3_right_after_death_only_ctrl_c_quits() {
    let (fight, _) = fight_to_death(Seed::new(42));
    let mut app = fight.app;
    app.handle_key(press(KeyCode::Char('q')));
    app.handle_key(press(KeyCode::Enter));
    assert_eq!(app.state(), State::GameOver, "held back by the grace");
    app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert_eq!(app.state(), State::Quit);
}

#[test]
fn s3_the_end_screen_survives_a_resize_and_fits_larger_terminals() {
    let (mut fight, _) = fight_to_death(Seed::new(42));
    let big = rows(&fight.screen(), 0..=23);

    render(&fight.app, 60, 20);
    fight.app.handle_key(press(KeyCode::Char('l')));
    assert_eq!(fight.app.state(), State::GameOver);

    assert_eq!(rows(&fight.screen(), 0..=23), big);
    for (width, height) in [(120, 40), (200, 60)] {
        let text = render(&fight.app, width, height).backend().to_string();
        assert!(text.contains("T H E   D E S C E N T   E N D S"), "{text}");
        assert!(text.contains("SEED   0000-0000-0000-002A"), "{text}");
        assert!(
            text.contains("Killed by an iron hammer at depth 1"),
            "{text}"
        );
        assert!(text.contains("q quit"), "{text}");
    }
}

// --- Journal ----------------------------------------------------

#[test]
fn a_long_exchange_scrolls_and_the_log_matches_the_core_blow_for_blow() {
    for seed in seeds() {
        let mut fight = Fight::new(seed, dark(), &[("gnat", east())]);
        let mut turns = 0;
        while fight.app.state() == State::Playing && turns < 60 {
            let before = fight.app.log().len();
            let events = fight.hit_east();
            let blows = events
                .iter()
                .filter(|e| matches!(e, GameEvent::Attacked { .. }))
                .count();
            assert_eq!(
                fight.app.log().len(),
                (before + blows).min(HISTORY),
                "seed {seed} turn {turns}: one line per blow, no more"
            );
            fight.assert_log_is_exact(&format!("seed {seed} turn {turns}"));
            turns += 1;
        }
        assert!(turns >= 25, "seed {seed}: only {turns} turns");
        assert_eq!(
            fight.app.state(),
            State::GameOver,
            "the gnat wins in the end"
        );
        // The fatal blow names the killer, whole, on the last row.
        let log = drawn_log(&fight.app);
        assert_eq!(
            log.last().unwrap().trim_end(),
            " ! The cave gnat kills you (1)."
        );
    }
}

#[test]
fn the_history_keeps_a_hundred_lines_over_a_very_long_fight() {
    let mut fight = Fight::new(Seed::new(42), dark(), &[("sloth", east())]);
    for turn in 0..130 {
        fight.hit_east();
        if turn % 13 == 0 || turn > 120 {
            fight.assert_log_is_exact(&format!("turn {turn}"));
        }
    }

    assert_eq!(fight.app.log().len(), HISTORY);
    let all: Vec<String> = fight
        .app
        .log()
        .newest_first()
        .map(|m| m.text.clone())
        .collect();
    assert!(
        !all.iter().any(|t| t.contains("enter the dungeon")),
        "welcome is gone"
    );
    assert!(
        all.iter()
            .all(|t| t.starts_with("You hit the sloth (") || t.starts_with("The sloth hits you (")),
        "{all:?}"
    );
    assert!(
        fight.app.game().actor(fight.ids[0]).is_some(),
        "still alive"
    );
}

/// A name longer than the content files accept
/// (`content::MAX_NAME_LENGTH`).
const LONG_NAME: &str = "extraordinarily patient executioner of the seventh hollow crypt";

/// A name of exactly `content::MAX_NAME_LENGTH` characters, the
/// longest a creature can have.
const LONGEST_NAME: &str = "eldest crypt executioner";

/// A monster file with one creature named `name` that kills in one
/// blow.
fn long_named_file(name: &str) -> String {
    format!(
        r#"[Monster(id: "long", name: "{name}", family: Acolyte, elite: true,
                    speed: 100, hp: 999, attack: 99, defense: 0, min_depth: 1)]"#
    )
}

/// A fight lost against the creature named [`LONGEST_NAME`].
fn lost_to_the_long_name() -> Fight {
    let file = long_named_file(LONGEST_NAME);
    let mut fight = Fight::with_content(Seed::new(42), dark(), &file, &[("long", east())]);
    for _ in 0..5 {
        if fight.app.state() == State::GameOver {
            break;
        }
        fight.wait();
    }
    assert_eq!(fight.app.state(), State::GameOver);
    fight
}

#[test]
fn the_longest_monster_name_keeps_the_fatal_blow_whole_in_the_log() {
    let fight = lost_to_the_long_name();

    let log: Vec<String> = drawn_log(&fight.app)
        .into_iter()
        .map(|l| l.trim_end().to_owned())
        .collect();
    let fatal = &fight.expected.last().unwrap().1;
    assert!(fatal.contains(LONGEST_NAME), "{fatal}");
    let joined = log
        .iter()
        .skip_while(|l| !l.starts_with(" ! "))
        .filter(|l| !l.is_empty())
        .map(|l| l[3..].to_owned())
        .collect::<Vec<_>>()
        .join(" ");
    assert_eq!(&joined, fatal, "{log:#?}");
    for line in &log {
        assert!(line.chars().count() <= 79, "{line:?}");
    }
}

#[test]
fn a_long_monster_name_is_not_cut_off_in_the_cause_of_death() {
    // A name too long to show whole is refused when loading...
    let error = Content::parse(&long_named_file(LONG_NAME))
        .expect_err("a name longer than the limit is refused");
    let message = error.to_string();
    assert!(
        message.contains("monster `long` (entry 1), field `name`"),
        "{message}"
    );
    assert_eq!(LONGEST_NAME.chars().count(), 24);

    // ...and the longest accepted one is shown whole.
    let fight = lost_to_the_long_name();
    let text = rows(&fight.screen(), 0..=23).join("\n");

    assert!(
        text.contains(&format!("Killed by an {LONGEST_NAME} at depth 1")),
        "the cause of death is cut off on the end screen:\n{text}"
    );
}

// --- Input robustness during a fight ----------------------------

#[test]
fn a_press_with_its_release_and_repeats_strikes_exactly_once() {
    let mut fight = Fight::new(Seed::new(42), dark(), &[("sloth", east())]);

    fight.hit_east();
    fight.app.handle_key(KeyEvent::new_with_kind(
        KeyCode::Char('l'),
        KeyModifiers::NONE,
        KeyEventKind::Repeat,
    ));
    fight.app.handle_key(release(KeyCode::Char('l')));

    assert_eq!(fight.app.game().player_turns(), 1);
    fight.assert_log_is_exact("press, repeat, release");
}

#[test]
fn a_too_small_terminal_blocks_blows_and_the_fight_resumes_when_enlarged() {
    let mut fight = Fight::new(Seed::new(42), dark(), &[("sloth", east())]);
    render(&fight.app, 79, 23);
    fight.app.handle_key(press(KeyCode::Char('l')));
    assert_eq!(fight.app.game().player_turns(), 0, "no blow in the dark");
    assert_eq!(fight.app.log().len(), 1);

    render(&fight.app, 80, 24);
    fight.hit_east();
    fight.assert_log_is_exact("after enlarging");
    assert_eq!(fight.app.game().player_turns(), 1);
}

#[test]
fn drawing_again_and_again_during_a_fight_changes_nothing() {
    let mut fight = Fight::new(Seed::new(42), dark(), &[("gnat", east())]);
    fight.hit_east();
    let first = fight.screen();
    let log = fight.app.log().clone();
    let turns = fight.app.game().player_turns();
    for _ in 0..50 {
        render(&fight.app, 80, 24);
    }
    assert_eq!(fight.screen(), first);
    assert_eq!(*fight.app.log(), log);
    assert_eq!(fight.app.game().player_turns(), turns);
}
