//! Acceptance tests of F9 (save, permadeath, summary), binary half.
//!
//! They drive the application as `main` does, on scratch folders only, and
//! read what the player reads: the rendered screens and the files on disk.
//! Written independently of the unit tests of `persist` and `app`: the
//! numbers of the death summary are parsed back from the drawn screen and
//! compared with the core's statistics, a resumed run is compared with a run
//! that was never interrupted (fights included, so damage rolls prove the
//! random stream), the save is read by a second thread while it is being
//! rewritten, and every kind of unreadable or leftover file is tried.
//!
//! The only place that depends on how `main` builds the application is
//! [`launch`].

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::Path;

use clap::Parser as _;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use sigilfall_core::action::Action;
use sigilfall_core::content::Content;
use sigilfall_core::game::Game;
use sigilfall_core::geometry::{Direction, Position};
use sigilfall_core::map::Tile;
use sigilfall_core::rng::Seed;
use sigilfall_core::save;

use crate::app::{App, END_SCREEN_GRACE, State};
use crate::cli::Cli;
use crate::clock::ManualClock;
use crate::persist::{Loaded, SaveFile, ScratchFolder};
use crate::theme::{ColorMode, ColorSupport, GlyphSet, Theme};

/// Keys of a walk that tries every direction and waits, long enough for
/// monsters to come and fight.
const WALK: &str = "lljjhkyub n..lllj jjhhkk...nnuubyb.ljhk.lljjhkyubn..";

/// The seeds of the fights: the player starts beside two monsters.
const FIGHT_SEEDS: [u64; 6] = [3, 11, 21, 33, 58, 64];

/// Terminal sizes the screens are compared at.
const SIZES: [(u16, u16); 2] = [(80, 24), (120, 40)];

/// Rows of an 80 × 24 screen above the message log: the status line, the
/// map and the side panel. The log below starts empty after a launch.
const ROWS_ABOVE_THE_LOG: usize = 17;

/// The default theme of Windows Terminal.
fn dark() -> Theme {
    Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode)
}

/// The game's own content.
fn shipped() -> Content {
    Content::load().expect("the shipped content is valid")
}

/// The command line `sigilfall` followed by `arguments`.
fn command_line(arguments: &[&str]) -> Cli {
    Cli::try_parse_from(std::iter::once("sigilfall").chain(arguments.iter().copied()))
        .expect("valid arguments")
}

/// Launches the application as `main` does, on the save file of `folder`,
/// with `arguments` on the command line, and stops on the title screen when
/// there is one.
fn open(folder: &ScratchFolder, arguments: &[&str]) -> anyhow::Result<App> {
    App::launch(
        ColorSupport::TrueColor,
        &command_line(arguments),
        &shipped(),
        folder.save_file(),
    )
}

/// Launches the application and, when it opens on the title screen, presses
/// Enter on the entry chosen at start ("Continue" when a game is saved,
/// "New game" otherwise), so the game is in progress.
fn launch(folder: &ScratchFolder, arguments: &[&str]) -> anyhow::Result<App> {
    let mut app = open(folder, arguments)?;
    if matches!(app.state(), State::Title(_)) {
        app.handle_key(press(KeyCode::Enter));
    }
    Ok(app)
}

/// A key press of `code`.
fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

/// Ctrl+C.
fn ctrl_c() -> KeyEvent {
    KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
}

/// Plays each character of `keys` as a key press; a space is skipped.
fn type_keys(app: &mut App, keys: &str) {
    for key in keys.chars().filter(|key| *key != ' ') {
        app.handle_key(press(KeyCode::Char(key)));
    }
}

/// The text of `app` drawn on a `width` × `height` screen, one string per
/// row.
fn screen(app: &App, width: u16, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect()
}

/// The whole text of the screen, rows joined.
fn screen_text(app: &App, width: u16, height: u16) -> String {
    screen(app, width, height).join("\n")
}

/// The rows of `a` and `b` that differ among the first `rows`.
fn differences(a: &[String], b: &[String], rows: usize) -> Vec<(usize, String, String)> {
    (0..rows.min(a.len()).min(b.len()))
        .filter(|&row| a[row] != b[row])
        .map(|row| (row, a[row].clone(), b[row].clone()))
        .collect()
}

/// The save text of `game`: its whole state, random stream included.
fn state_of(game: &Game) -> String {
    save::to_string(game).expect("a game can be written")
}

/// The names of the files in `folder`, sorted.
fn files(folder: &ScratchFolder) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(folder.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The bytes of the file `name` in `folder`.
fn bytes(folder: &ScratchFolder, name: &str) -> Vec<u8> {
    fs::read(folder.path().join(name)).unwrap_or_else(|error| panic!("{name}: {error}"))
}

/// Quits `app` with `q` as the player does, saves, and returns.
fn quit_and_save(app: &mut App) {
    app.handle_key(press(KeyCode::Char('q')));
    assert_eq!(app.state(), State::Quit);
    app.save_on_exit().expect("the game is saved");
}

/// A game of `seed` with a rat and a beetle standing beside the player, as
/// the pseudo-terminal pilot sets up fights.
fn fighting_game(seed: u64) -> Game {
    crowded_game(seed, &["ossuary_rat", "wax_beetle"])
}

/// A game of `seed` with one monster of each of `kinds` beside the player.
fn crowded_game(seed: u64, kinds: &[&str]) -> Game {
    let content = shipped();
    let mut game = Game::new(Seed::from(seed), &content);
    let me = game.player_actor().position();
    let mut kinds = kinds.iter().copied();
    for direction in Direction::ALL {
        let cell = me.step(direction);
        let free = game.map().can_step(me, direction)
            && game.actors().all(|(_, actor)| actor.position() != cell);
        if free && let Some(kind) = kinds.next() {
            game.spawn_monster(cell, content.monster(kind).unwrap());
        }
    }
    game
}

/// A save in `folder` holding `game`, written as the application writes it.
fn save_game(folder: &ScratchFolder, game: &Game) {
    folder.save_file().write(game).expect("the save is written");
}

// --- Scenario 1: quit, relaunch, the same state, random stream included ---

#[test]
fn a_fight_resumed_after_quitting_ends_as_the_fight_never_interrupted() {
    let mut fought = 0;
    for value in FIGHT_SEEDS {
        let (before, after) = WALK.split_at(24);
        // The run that is quit and resumed...
        let folder = ScratchFolder::new("f9-fight-quit");
        save_game(&folder, &fighting_game(value));
        let mut quit = launch(&folder, &[]).unwrap();
        type_keys(&mut quit, before);
        let at_quit = screen(&quit, 80, 24);
        let mid_state = state_of(quit.game());
        quit_and_save(&mut quit);
        let mut resumed = launch(&folder, &[]).unwrap();
        // ...is, on its screen and in its state, the run at the moment of
        // quitting...
        assert_eq!(state_of(resumed.game()), mid_state, "seed {value}");
        for (width, height) in SIZES {
            let was = screen(&quit, width, height);
            let is = screen(&resumed, width, height);
            let rows = if (width, height) == (80, 24) {
                ROWS_ABOVE_THE_LOG
            } else {
                usize::from(height) - 6
            };
            assert_eq!(
                differences(&was, &is, rows),
                vec![],
                "seed {value} at {width} x {height}"
            );
        }
        assert_eq!(
            differences(&at_quit, &screen(&resumed, 80, 24), ROWS_ABOVE_THE_LOG),
            vec![]
        );
        // ...and the one that never stopped.
        let never = ScratchFolder::new("f9-fight-never");
        save_game(&never, &fighting_game(value));
        let mut control = launch(&never, &[]).unwrap();
        type_keys(&mut control, before);
        type_keys(&mut control, after);
        type_keys(&mut resumed, after);
        assert_eq!(
            state_of(resumed.game()),
            state_of(control.game()),
            "seed {value}: whole state, random stream included"
        );
        assert_eq!(
            differences(
                &screen(&resumed, 80, 24),
                &screen(&control, 80, 24),
                ROWS_ABOVE_THE_LOG
            ),
            vec![],
            "seed {value}"
        );
        let player = control.game().player_actor();
        fought += usize::from(player.hp() < player.max_hp());
    }
    assert!(fought >= 3, "only {fought} fights hurt the player");
}

/// Plays a tourist on the shipped content of `seed` (picks items up,
/// strikes what blocks the way, takes the stairs) until it stands on the
/// third floor or deeper, alive, carrying something and knowing a sigil,
/// with no monster in view. `None` if this seed gives no such game.
fn deep_game(seed: u64) -> Option<Game> {
    let content = shipped();
    let mut game = Game::new(Seed::from(seed), &content);
    let start = game.player_actor().position();
    for (direction, id) in [(Direction::East, "ember"), (Direction::West, "recall")] {
        let cell = start.step(direction);
        if game.map().can_step(start, direction) && game.engraving_at(cell).is_none() {
            game.engrave(cell, content.sigil(id).unwrap());
        }
    }
    for _ in 0..1500 {
        if game.is_over() {
            return None;
        }
        let here = game.player_actor().position();
        let stairs = stairs_of(&game);
        let rich = game.depth() >= 3
            && game.inventory().next().is_some()
            && !game.grimoire().is_empty()
            && game.visible_monsters().next().is_none();
        if rich && here != stairs {
            return Some(game);
        }
        if game.visible_items().any(|item| item.position == here) && game.inventory().count() < 10 {
            game.submit(Action::PickUp);
            continue;
        }
        let blocker = Direction::ALL.into_iter().find(|&direction| {
            game.actors().any(|(_, actor)| {
                actor.as_monster().is_some() && actor.position() == here.step(direction)
            })
        });
        let action = if let Some(direction) = blocker {
            Action::Move(direction)
        } else if here == stairs {
            Action::Descend
        } else {
            first_step(&game, stairs).map_or(Action::Wait, Action::Move)
        };
        game.submit(action);
    }
    None
}

/// The stairs of the current floor of `game`.
fn stairs_of(game: &Game) -> Position {
    let map = game.map();
    let (width, height) = (
        i32::try_from(map.width()).unwrap(),
        i32::try_from(map.height()).unwrap(),
    );
    (0..height)
        .flat_map(|y| (0..width).map(move |x| Position::new(x, y)))
        .find(|&cell| map.tile(cell) == Some(Tile::StairsDown))
        .expect("every floor has stairs")
}

/// First step of a shortest walk to `to`, ignoring creatures.
fn first_step(game: &Game, to: Position) -> Option<Direction> {
    let from = game.player_actor().position();
    let map = game.map();
    let mut first: HashMap<Position, Direction> = HashMap::new();
    let mut seen = HashSet::from([from]);
    let mut queue = VecDeque::from([from]);
    while let Some(cell) = queue.pop_front() {
        for direction in Direction::ALL {
            let next = cell.step(direction);
            if !map.can_step(cell, direction) || !seen.insert(next) {
                continue;
            }
            first.insert(
                next,
                if cell == from {
                    direction
                } else {
                    first[&cell]
                },
            );
            if next == to {
                return Some(first[&next]);
            }
            queue.push_back(next);
        }
    }
    None
}

#[test]
fn a_deep_run_with_a_pack_and_a_grimoire_resumes_with_the_same_screens() {
    let game = (0..40)
        .find_map(deep_game)
        .expect("a seed gives a deep, equipped run");
    assert!(game.depth() >= 3);
    let folder = ScratchFolder::new("f9-deep");
    save_game(&folder, &game);

    let mut first = launch(&folder, &[]).unwrap();
    assert_eq!(state_of(first.game()), state_of(&game), "same state");
    assert_eq!(first.game().depth(), game.depth());
    // The pack and the grimoire, as the player reads them.
    first.handle_key(press(KeyCode::Char('i')));
    let pack_screen = screen_text(&first, 80, 24);
    for (_, item) in game.inventory() {
        assert!(
            pack_screen.contains(item.definition().name()),
            "the pack shows {}:\n{pack_screen}",
            item.definition().name()
        );
    }
    first.handle_key(press(KeyCode::Esc));
    first.handle_key(press(KeyCode::Char('t')));
    let grimoire_screen = screen_text(&first, 80, 24);
    for sigil in game.grimoire().sigils() {
        assert!(
            grimoire_screen.contains(sigil.name()),
            "the grimoire shows {}:\n{grimoire_screen}",
            sigil.name()
        );
    }
    first.handle_key(press(KeyCode::Esc));
    // Play on, quit, resume: nothing was lost at the second save either.
    type_keys(&mut first, WALK);
    let state = state_of(first.game());
    quit_and_save(&mut first);
    let second = launch(&folder, &[]).unwrap();
    assert_eq!(state_of(second.game()), state);
    assert_eq!(second.game().depth(), first.game().depth());
    assert_eq!(
        differences(
            &screen(&first, 80, 24),
            &screen(&second, 80, 24),
            ROWS_ABOVE_THE_LOG
        ),
        vec![]
    );
    // And the file does not drift: saving a resumed run unchanged writes
    // the same bytes.
    let written = bytes(&folder, "save.ron");
    second.save_on_exit().unwrap();
    assert_eq!(bytes(&folder, "save.ron"), written);
    assert_eq!(files(&folder), ["save.ron"]);
}

#[test]
fn quitting_with_ctrl_c_from_any_overlay_saves_and_q_from_an_overlay_does_not_quit() {
    for opening in ["i", "t", "t7", "t75"] {
        // Ctrl+C saves.
        let folder = ScratchFolder::new("f9-overlay-ctrl-c");
        let mut app = launch(&folder, &["--seed", "42"]).unwrap();
        type_keys(&mut app, &WALK[..12]);
        type_keys(&mut app, opening);
        assert!(
            matches!(app.state(), State::Inventory(_) | State::Casting(_)),
            "{opening}: an overlay is open"
        );
        let state = state_of(app.game());
        app.handle_key(ctrl_c());
        assert_eq!(app.state(), State::Quit, "{opening}");
        app.save_on_exit().unwrap();
        assert_eq!(files(&folder), ["save.ron"], "{opening}");
        let back = launch(&folder, &[]).unwrap();
        assert_eq!(state_of(back.game()), state, "{opening}");
        assert_eq!(back.state(), State::Playing, "{opening}: no overlay");

        // `q` does not quit from an overlay (docs/design.md), so it cannot
        // lose the run; leaving the overlay with Esc, `q` then saves.
        let folder = ScratchFolder::new("f9-overlay-q");
        let mut app = launch(&folder, &["--seed", "42"]).unwrap();
        type_keys(&mut app, &WALK[..12]);
        type_keys(&mut app, opening);
        app.handle_key(press(KeyCode::Char('q')));
        assert_ne!(
            app.state(),
            State::Quit,
            "{opening}: q is not a quit key here"
        );
        assert_eq!(
            files(&folder),
            Vec::<String>::new(),
            "{opening}: nothing written"
        );
        for _ in 0..3 {
            app.handle_key(press(KeyCode::Esc));
        }
        assert_eq!(app.state(), State::Playing, "{opening}");
        quit_and_save(&mut app);
        assert_eq!(files(&folder), ["save.ron"]);
    }
}

// --- Scenario 2: death, summary, the save is deleted ---

/// The value written after the leader dots of the row `label` of the
/// summary, and the detail after it.
fn summary_row(screen: &[String], label: &str) -> (String, String) {
    let row = screen
        .iter()
        .find(|row| row.trim_start().starts_with(&format!("{label} .")))
        .unwrap_or_else(|| panic!("a `{label}` row in:\n{}", screen.join("\n")));
    let after = row.trim_start()[label.len()..]
        .trim_start()
        .trim_start_matches('.')
        .trim();
    let (value, detail) = after.split_once("   ").unwrap_or((after, ""));
    (value.trim().to_owned(), detail.trim().to_owned())
}

/// A run that kills, casts and then dies: the application on a game of
/// `seed` in a folder, the player next to a rat and a beetle with the
/// glyph of Ember under its feet, and a lead homunculus on its way.
fn doomed_run(folder: &ScratchFolder, seed: u64) -> App {
    let content = shipped();
    let mut game = crowded_game(seed, &["ossuary_rat", "wax_beetle", "lead_homunculus"]);
    let me = game.player_actor().position();
    let free = Direction::ALL
        .into_iter()
        .find(|&d| {
            let cell = me.step(d);
            game.map().can_step(me, d)
                && game.engraving_at(cell).is_none()
                && game.actors().all(|(_, a)| a.position() != cell)
        })
        .expect("a free neighbour");
    game.engrave(me.step(free), content.sigil("ember").unwrap());
    save_game(folder, &game);
    let mut app = launch(folder, &[]).unwrap();
    // Read the glyph, fight whatever is beside, cast three times.
    app.handle_key(press(KeyCode::Char(direction_key(free))));
    for direction in Direction::ALL {
        for _ in 0..4 {
            if app.state() == State::GameOver {
                return app;
            }
            app.handle_key(press(KeyCode::Char(direction_key(direction))));
        }
    }
    for trace in ["753", "75", "159"] {
        if app.state() == State::GameOver {
            return app;
        }
        app.handle_key(press(KeyCode::Char('t')));
        type_keys(&mut app, trace);
        app.handle_key(press(KeyCode::Enter));
        if matches!(app.state(), State::Casting(_)) {
            app.handle_key(press(KeyCode::Char('x')));
        }
    }
    app
}

/// The key that moves in `direction`.
const fn direction_key(direction: Direction) -> char {
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

#[test]
fn the_death_summary_shows_what_the_core_counted_and_the_save_is_gone() {
    let content = shipped();
    let mut summaries = 0;
    for value in [3_u64, 11, 21, 33, 58, 64, 99, 123] {
        let folder = ScratchFolder::new("f9-death");
        let mut app = doomed_run(&folder, value);
        // Wait for the end: the lead homunculus is not there, so the rat
        // and the beetle must do it; bounded, and skipped if they cannot.
        for _ in 0..400 {
            if app.state() == State::GameOver {
                break;
            }
            app.handle_key(press(KeyCode::Char('.')));
        }
        if app.state() != State::GameOver {
            continue;
        }
        summaries += 1;

        assert!(
            !folder.save_file().path().exists(),
            "seed {value}: the save is deleted at the death"
        );
        let game = app.game();
        let stats = game.stats();
        for (width, height) in SIZES {
            let rows = screen(&app, width, height);
            let text = rows.join("\n");
            assert!(text.contains("T H E   D E S C E N T   E N D S"), "{text}");
            // The seed, in the form the player types back.
            assert!(
                text.contains(&game.seed().to_string()),
                "seed {value}: the summary shows the seed:\n{text}"
            );
            assert!(
                text.contains(&format!("--seed {}", game.seed())),
                "the replay command:\n{text}"
            );
            // Depth, turns, cause.
            assert_eq!(
                summary_row(&rows, "Depth reached").0,
                game.stats().deepest().to_string(),
                "seed {value}"
            );
            assert_eq!(
                summary_row(&rows, "Turns").0,
                game.player_turns().to_string()
            );
            let sigilfall_core::game::Status::GameOver { killed_by } = game.status() else {
                panic!("the game is over");
            };
            let killer = match killed_by {
                sigilfall_core::combat::Combatant::Monster { definition } => {
                    content.monster(definition).unwrap().name().to_owned()
                }
                sigilfall_core::combat::Combatant::Player => "own magic".to_owned(),
            };
            assert!(
                text.contains(&format!("{killer} at depth {}", game.depth())),
                "seed {value}: the cause names {killer}:\n{text}"
            );
            // Creatures slain, sigils cast, glyphs, items.
            let (kills, detail) = summary_row(&rows, "Creatures slain");
            assert_eq!(kills, stats.total_kills().to_string(), "seed {value}");
            if stats.total_kills() > 0 {
                let first = stats.kills().next().unwrap();
                let name = content.monster(first.0).unwrap().name();
                assert!(
                    text.contains(name) || detail.contains(name),
                    "seed {value}: {name} is listed:\n{text}"
                );
            }
            let (casts, quality) = summary_row(&rows, "Sigils cast");
            assert_eq!(casts, stats.casts().total().to_string(), "seed {value}");
            if stats.casts().total() > 0 {
                let counts = stats.casts();
                assert_eq!(
                    quality,
                    format!(
                        "pure {} · unstable {} · fizzled {}",
                        counts.pure, counts.unstable, counts.fizzled
                    ),
                    "seed {value}"
                );
            }
            assert_eq!(
                summary_row(&rows, "Glyphs learnt").0,
                stats.glyphs_learnt().to_string()
            );
            assert_eq!(
                summary_row(&rows, "Items used").0,
                stats.items_used().to_string()
            );
        }

        // The seed shown gives the first floor back.
        let shown = screen_text(&app, 80, 24);
        let typed = shown
            .split("--seed ")
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
            .expect("a replay command");
        let seed: Seed = typed.parse().expect("the shown seed parses");
        assert_eq!(seed, game.seed());
        let again = Game::new(seed, &content);
        let new_run = Game::new(game.seed(), &content);
        assert_eq!(state_of(&again), state_of(&new_run));

        // Once the end screen has settled (B1), keys do nothing on it; `q`
        // leaves and recreates nothing, neither a save nor a leftover.
        let clock = ManualClock::new();
        let mut app = app.with_clock(clock.clone());
        clock.advance(END_SCREEN_GRACE);
        let frozen = screen_text(&app, 80, 24);
        type_keys(&mut app, WALK);
        assert_eq!(screen_text(&app, 80, 24), frozen, "seed {value}");
        quit_and_save(&mut app);
        assert_eq!(files(&folder), Vec::<String>::new(), "seed {value}");
        // The next launch is a brand new game, not the dead character.
        let next = launch(&folder, &[]).unwrap();
        assert_eq!(next.state(), State::Playing);
        assert_eq!(next.game().player_turns(), 0);
    }
    assert!(summaries >= 3, "only {summaries} runs reached the summary");
}

#[test]
fn a_death_while_the_save_is_held_by_a_folder_is_reported_on_leaving() {
    // The save cannot be deleted (a folder has its name): the death says so
    // in the log, and leaving reports it, with an error naming the file.
    let folder = ScratchFolder::new("f9-undeletable");
    let mut app = doomed_run(&folder, 3);
    for _ in 0..400 {
        if app.state() == State::GameOver {
            break;
        }
        app.handle_key(press(KeyCode::Char('.')));
    }
    if app.state() != State::GameOver {
        return;
    }
    // Put a folder where the deleted save was, then die again: not possible
    // after death, so check the leaving path directly.
    fs::create_dir(folder.save_file().path()).unwrap();
    let error = app
        .save_on_exit()
        .expect_err("a folder cannot be deleted as a save");
    assert!(
        format!("{error:#}").contains("save.ron"),
        "the error names the file: {error:#}"
    );
}

// --- Scenario 3: corrupt or foreign saves ---

/// A real save, for the corruptions to start from.
fn a_good_save() -> String {
    let mut game = Game::new(Seed::from(42), &shipped());
    let mut folder_app = App::with_game(dark(), game.clone(), shipped());
    type_keys(&mut folder_app, &WALK[..10]);
    game = folder_app.game().clone();
    state_of(&game)
}

#[test]
fn every_unreadable_save_is_set_aside_with_a_message_and_a_new_game_starts() {
    let good = a_good_save();
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", Vec::new()),
        ("garbage", b"this is not a save {{{".to_vec()),
        ("not utf-8", vec![0xff, 0xfe, 0x00, 0x80]),
        ("truncated", good.as_bytes()[..good.len() / 2].to_vec()),
        (
            "version 1",
            good.replacen(
                &format!("format_version: {}", save::FORMAT_VERSION),
                "format_version: 1",
                1,
            )
            .into_bytes(),
        ),
        (
            "version 99",
            good.replacen(
                &format!("format_version: {}", save::FORMAT_VERSION),
                "format_version: 99",
                1,
            )
            .into_bytes(),
        ),
        (
            "unknown monster",
            good.replace("ossuary_rat", "dragon")
                .replace("wax_beetle", "dragon")
                .replace("acolyte", "dragon")
                .replace("homunculus", "dragon")
                .into_bytes(),
        ),
        ("a folder's worth of nul bytes", vec![0; 4096]),
        (
            "crlf and a bom",
            format!(
                "\u{feff}{}",
                good.replace('\n', "\r\n").replacen(
                    &format!("format_version: {}", save::FORMAT_VERSION),
                    "format_version: 7",
                    1
                )
            )
            .into_bytes(),
        ),
    ];
    for (name, content) in cases {
        let folder = ScratchFolder::new("f9-corrupt");
        fs::write(folder.path().join("save.ron"), &content).unwrap();

        let title = open(&folder, &[]).unwrap_or_else(|error| panic!("{name}: {error:#}"));
        // The player is told, on the title screen, before anything starts.
        let text = screen_text(&title, 80, 24);
        assert!(
            text.contains("could not be read"),
            "{name}: not told:
{text}"
        );
        // (The title may cut the path of a long data folder; the file's
        // place is checked on disk below.)
        assert!(!title.has_saved_game(), "{name}: nothing to continue");
        // New game: a new game, in progress, from turn zero.
        let mut app = title;
        app.handle_key(press(KeyCode::Enter));
        assert_eq!(app.state(), State::Playing, "{name}");
        assert_eq!(app.game().player_turns(), 0, "{name}");
        assert_eq!(app.game().depth(), 1, "{name}");
        // The unreadable save was moved, byte for byte, not deleted.
        assert_eq!(files(&folder), ["save.ron.bad"], "{name}");
        assert_eq!(bytes(&folder, "save.ron.bad"), content, "{name}");
        // The new game is saved on quitting, beside the kept file.
        type_keys(&mut app, &WALK[..6]);
        quit_and_save(&mut app);
        assert_eq!(files(&folder), ["save.ron", "save.ron.bad"], "{name}");
        assert_eq!(bytes(&folder, "save.ron.bad"), content, "{name}: kept");
        // The next launch resumes the new game without any complaint.
        let again = launch(&folder, &[]).unwrap();
        let quiet = screen_text(&again, 80, 24);
        assert!(quiet.contains("Welcome back"), "{name}:\n{quiet}");
        assert!(!quiet.contains("could not be read"), "{name}");
        assert_eq!(
            bytes(&folder, "save.ron.bad"),
            content,
            "{name}: still kept"
        );
    }
}

#[test]
fn a_newer_unreadable_save_replaces_the_older_one_kept_aside() {
    let folder = ScratchFolder::new("f9-bad-replaced");
    fs::write(folder.path().join("save.ron.bad"), b"OLDER").unwrap();
    fs::write(folder.path().join("save.ron"), b"NEWER").unwrap();

    launch(&folder, &[]).unwrap();

    assert_eq!(files(&folder), ["save.ron.bad"]);
    assert_eq!(bytes(&folder, "save.ron.bad"), b"NEWER");
}

#[test]
fn a_save_that_is_a_folder_stops_the_launch_with_an_error_and_loses_nothing() {
    let folder = ScratchFolder::new("f9-save-is-a-folder");
    fs::create_dir(folder.path().join("save.ron")).unwrap();
    fs::write(folder.path().join("save.ron").join("inside"), b"x").unwrap();

    let error = launch(&folder, &[]).expect_err("a folder is not a save");

    let message = format!("{error:#}");
    assert!(message.contains("save.ron"), "{message}");
    assert!(folder.path().join("save.ron").join("inside").exists());
}

// --- The temporary file is never read ---

#[test]
fn a_leftover_temporary_file_is_never_read_whatever_it_holds() {
    let other = state_of(&Game::new(Seed::from(777), &shipped()));
    let half = &other.as_bytes()[..other.len() / 3];
    for (name, leftover) in [
        ("a complete game", other.as_bytes()),
        ("half a game", half),
        ("garbage", b"\0\0\0"),
    ] {
        // Alone: no save, so a new game of the seed asked for, not an error
        // and not the leftover.
        let folder = ScratchFolder::new("f9-tmp-alone");
        fs::write(folder.path().join("save.ron.tmp"), leftover).unwrap();
        let mut app = launch(&folder, &["--seed", "5"]).unwrap_or_else(|e| panic!("{name}: {e:#}"));
        assert_eq!(app.game().seed(), Seed::from(5), "{name}");
        assert_eq!(
            bytes(&folder, "save.ron.tmp"),
            leftover,
            "{name}: untouched at launch"
        );
        quit_and_save(&mut app);
        // The write replaced the leftover with the real save.
        assert_eq!(files(&folder), ["save.ron"], "{name}");

        // Beside a save: the save is resumed.
        let folder = ScratchFolder::new("f9-tmp-beside");
        let mut first = launch(&folder, &["--seed", "5"]).unwrap();
        type_keys(&mut first, &WALK[..8]);
        quit_and_save(&mut first);
        fs::write(folder.path().join("save.ron.tmp"), leftover).unwrap();
        let resumed = launch(&folder, &[]).unwrap_or_else(|e| panic!("{name}: {e:#}"));
        assert_eq!(state_of(resumed.game()), state_of(first.game()), "{name}");
        assert_eq!(bytes(&folder, "save.ron.tmp"), leftover, "{name}");
    }
}

#[test]
fn the_save_is_replaced_whole_while_another_thread_keeps_reading_it() {
    // Atomic write: whatever moment the reader picks, it finds the old save
    // or the new one, whole, never half of one and never nothing after the
    // first write.
    let content = shipped();
    let folder = ScratchFolder::new("f9-atomic");
    let games: Vec<Game> = (0..2_u64)
        .map(|seed| {
            let mut app = App::with_game(
                dark(),
                Game::new(Seed::from(seed + 100), &content),
                content.clone(),
            );
            type_keys(&mut app, &WALK[..10]);
            app.game().clone()
        })
        .collect();
    let texts: Vec<String> = games.iter().map(state_of).collect();
    let file = folder.save_file();
    file.write(&games[0]).unwrap();
    let stop = std::sync::atomic::AtomicBool::new(false);

    let reads = std::thread::scope(|scope| {
        let reader = scope.spawn(|| {
            let mut reads = 0_u32;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                match file.load(&content) {
                    Ok(Loaded::Game(game)) => {
                        let text = state_of(&game);
                        assert!(texts.contains(&text), "a game that was never saved");
                    }
                    other => panic!("a torn read: {other:?}"),
                }
                reads += 1;
            }
            reads
        });
        for round in 0..150 {
            file.write(&games[round % 2]).expect("rewriting the save");
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        reader.join().expect("the reader never saw a torn save")
    });

    assert!(reads > 10, "the reader read the save {reads} times");
    assert_eq!(files(&folder), ["save.ron"], "no leftover temporary file");
}

// --- --seed while a game is saved ---

#[test]
fn a_seed_given_while_a_game_is_saved_is_refused_and_the_save_is_untouched() {
    let folder = ScratchFolder::new("f9-seed-refused");
    let mut first = launch(&folder, &["--seed", "42"]).unwrap();
    type_keys(&mut first, &WALK[..14]);
    quit_and_save(&mut first);
    let before = bytes(&folder, "save.ron");

    for arguments in [
        vec!["--seed", "5"],
        vec!["--seed", "42"],
        vec!["--seed", "7F3A-91C2-0B5E-D4A8"],
        vec!["--ascii", "--seed", "1"],
    ] {
        let error = launch(&folder, &arguments).expect_err("the launch is refused");
        let message = format!("{error:#}");
        assert!(message.contains("--seed"), "{arguments:?}: {message}");
        assert!(
            message.contains("save.ron"),
            "{arguments:?}: names the file: {message}"
        );
        assert_eq!(bytes(&folder, "save.ron"), before, "{arguments:?}");
        assert_eq!(files(&folder), ["save.ron"], "{arguments:?}");
    }
    // Without a seed, the game is resumed.
    let resumed = launch(&folder, &[]).unwrap();
    assert_eq!(resumed.game().seed(), Seed::from(42));
    // With an unreadable save the seed is accepted: nothing is lost.
    fs::write(folder.path().join("save.ron"), b"broken").unwrap();
    let new_game = launch(&folder, &["--seed", "9"]).unwrap();
    assert_eq!(new_game.game().seed(), Seed::from(9));
}

// --- A failed write ---

#[test]
fn a_write_that_fails_is_an_error_naming_the_file_and_the_previous_save_survives() {
    let folder = ScratchFolder::new("f9-write-fails");
    let mut first = launch(&folder, &["--seed", "42"]).unwrap();
    type_keys(&mut first, &WALK[..10]);
    quit_and_save(&mut first);
    let kept = bytes(&folder, "save.ron");

    let mut second = launch(&folder, &[]).unwrap();
    type_keys(&mut second, &WALK[10..30]);
    // A folder where the temporary file goes makes the write fail.
    fs::create_dir(folder.path().join("save.ron.tmp")).unwrap();
    second.handle_key(press(KeyCode::Char('q')));
    let error = second.save_on_exit().expect_err("the write fails");

    let message = format!("{error:#}");
    assert!(message.contains("could not be saved"), "{message}");
    assert!(message.contains("save.ron"), "{message}");
    assert_eq!(
        bytes(&folder, "save.ron"),
        kept,
        "the previous save survives"
    );
    // A fresh game cannot be saved either when the folder cannot be made.
    let blocked = ScratchFolder::new("f9-write-fails-folder");
    let file = blocked.path().join("not-a-folder");
    fs::write(&file, b"x").unwrap();
    let impossible = SaveFile::in_folder(Path::new(&file));
    let error = impossible
        .write(&Game::new(Seed::from(1), &shipped()))
        .expect_err("a file cannot hold a save");
    assert!(format!("{error:#}").contains("could not be saved"));
}

// --- The title screen in front of a save ---

#[test]
fn the_title_offers_continue_only_with_a_save_and_says_when_a_save_was_set_aside() {
    let folder = ScratchFolder::new("f9-title");
    let fresh = open(&folder, &[]).unwrap();
    assert!(!fresh.has_saved_game());
    assert!(!screen_text(&fresh, 80, 24).contains("Continue"));

    let mut played = launch(&folder, &["--seed", "42"]).unwrap();
    type_keys(&mut played, &WALK[..10]);
    quit_and_save(&mut played);
    let title = open(&folder, &[]).unwrap();
    assert!(title.has_saved_game());
    assert!(screen_text(&title, 80, 24).contains("Continue"));
    // Leaving the title without playing keeps the save as it was.
    let before = bytes(&folder, "save.ron");
    title.save_on_exit().unwrap();
    assert_eq!(bytes(&folder, "save.ron"), before);

    fs::write(folder.path().join("save.ron"), b"broken").unwrap();
    let set_aside = open(&folder, &[]).unwrap();
    assert!(!set_aside.has_saved_game());
    let text = screen_text(&set_aside, 80, 24);
    assert!(text.contains("could not be read"), "{text}");
    assert_eq!(bytes(&folder, "save.ron.bad"), b"broken");
}

/// A folder inside `folder` whose name is long enough for the path of the
/// save kept aside, `…/save.ron.bad`, to have `length` characters.
fn folder_for_a_bad_save_path_of(folder: &ScratchFolder, length: usize) -> std::path::PathBuf {
    let base = folder
        .path()
        .join("save.ron.bad")
        .to_string_lossy()
        .chars()
        .count();
    let name = "long-folder-name-".repeat(10);
    let name: String = name.chars().take(length - base - 1).collect();
    let deep = folder.path().join(name);
    fs::create_dir_all(&deep).unwrap();
    assert_eq!(
        deep.join("save.ron.bad").to_string_lossy().chars().count(),
        length
    );
    deep
}

/// The rows of `screen` joined, without the margins and the indents of
/// wrapped lines.
fn joined(screen: &[String]) -> String {
    screen
        .iter()
        .map(|row| row.trim())
        .filter(|row| !row.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn a_20_kb_error_and_a_120_character_path_show_whole_on_the_title_and_in_the_log() {
    let folder = ScratchFolder::new("f9-b2");
    let deep = folder_for_a_bad_save_path_of(&folder, 120);
    let save = SaveFile::in_folder(&deep);
    fs::write(save.path(), "x".repeat(20_000)).unwrap();
    let bad = deep.join("save.ron.bad").display().to_string();
    // The path does not fit on a line of 76: its start gives way to `…`,
    // and its end, with the file name, stays.
    let tail: String = bad.chars().skip(120 - 74).collect();

    let mut app = App::launch(
        ColorSupport::TrueColor,
        &command_line(&[]),
        &shipped(),
        save.clone(),
    )
    .unwrap();
    let title = screen(&app, 80, 24);
    let on_title = joined(&title);
    assert!(
        on_title.contains("! Your saved game could not be read: the save is unreadable"),
        "{}",
        title.join(
            "
"
        )
    );
    assert!(
        on_title.contains(&format!("It was kept as …{tail}.")),
        "the file kept aside is named on the title:
{}",
        title.join(
            "
"
        )
    );
    assert!(
        on_title.contains("…. It was kept as"),
        "the error is cut: {on_title}"
    );
    assert!(title.iter().all(|row| row.chars().count() <= 80));
    // The menu and its help are still there.
    assert!(on_title.contains("New game"), "{on_title}");
    assert!(
        title[23].contains("q quit"),
        "{}",
        title.join(
            "
"
        )
    );

    app.handle_key(press(KeyCode::Enter));
    assert_eq!(app.state(), State::Playing);
    let game = screen(&app, 80, 24);
    let log = joined(&game[ROWS_ABOVE_THE_LOG + 1..23]);
    assert!(
        log.starts_with("! Your saved game could not be read: the save is unreadable"),
        "the message opens the log:
{}",
        game.join(
            "
"
        )
    );
    assert!(
        log.contains(&format!(
            "the old save was kept as …{tail}. You enter the dungeon."
        )),
        "the whole message, then the welcome line:
{}",
        game.join(
            "
"
        )
    );
    assert!(
        fs::metadata(&bad).unwrap().len() == 20_000,
        "kept as it was"
    );
}

#[test]
fn a_very_long_error_text_still_tells_the_player_the_save_was_set_aside() {
    let folder = ScratchFolder::new("f9-long-notice");
    fs::write(folder.path().join("save.ron"), "x".repeat(20_000)).unwrap();

    let title = open(&folder, &[]).unwrap();
    let on_title = screen_text(&title, 80, 24);
    let game = launch(&folder, &[]).unwrap();
    assert_eq!(game.state(), State::Playing);

    assert!(
        on_title.contains("could not be read"),
        "title screen:
{on_title}"
    );
}

// --- Enter after a deadly cast ---

/// The monster that kills the player in one blow, in a shipped-compatible
/// content (the save is not involved).
const EXECUTIONER: &str = r#"[
    Monster(id: "executioner", name: "executioner", family: Acolyte, elite: true,
        speed: 100, hp: 999, attack: 99, defense: 0, min_depth: 1),
]"#;

/// An application on the fixed room of seed 5, with the executioner
/// `distance` cells east of the player, saving to `folder`, reading the
/// time from `clock`.
fn executioner_app(folder: &ScratchFolder, distance: usize, clock: &ManualClock) -> App {
    let content = Content::parse(EXECUTIONER).expect("valid test monster");
    let mut game = Game::in_fixed_room(Seed::from(5));
    let cell = (0..distance).fold(sigilfall_core::game::PLAYER_START, |cell, _| {
        cell.step(Direction::East)
    });
    game.spawn_monster(cell, content.monster("executioner").unwrap());
    App::with_game(dark(), game, content)
        .saving_to(folder.save_file())
        .with_clock(clock.clone())
}

/// Bumps into the executioner east of the player of `app` until it deals
/// its deadly blow, as a player hitting the same key does.
fn fight_the_executioner(app: &mut App) {
    for _ in 0..10 {
        if app.state() == State::GameOver {
            return;
        }
        app.handle_key(press(KeyCode::Char('l')));
    }
    panic!("the executioner kills the player");
}

/// A millisecond, the smallest step the grace period is checked at.
const MILLISECOND: std::time::Duration = std::time::Duration::from_millis(1);

#[test]
fn an_extra_enter_right_after_a_deadly_cast_does_not_skip_the_end_screen() {
    let mut deadly = 0;
    for trace in ["159", "75", "753"] {
        let content = Content::parse(EXECUTIONER).expect("valid test monster");
        let mut game = Game::in_fixed_room(Seed::from(5));
        game.spawn_monster(
            sigilfall_core::game::PLAYER_START.step(Direction::East),
            content.monster("executioner").unwrap(),
        );
        let folder = ScratchFolder::new("f9-deadly-cast");
        let mut app = App::with_game(dark(), game, content).saving_to(folder.save_file());
        app.handle_key(press(KeyCode::Char('t')));
        type_keys(&mut app, trace);
        app.handle_key(press(KeyCode::Enter));
        let after_cast = app.state();
        // The player's finger was still on Enter.
        app.handle_key(press(KeyCode::Enter));
        let after_extra = app.state();

        assert!(
            matches!(after_cast, State::GameOver | State::Casting(_)),
            "{trace}: after the cast: {after_cast:?}"
        );
        if after_cast == State::GameOver {
            deadly += 1;
            assert_eq!(
                after_extra,
                State::GameOver,
                "{trace}: the extra Enter left the end screen unseen"
            );
        } else {
            // The player survived the cast: its verdict shows until the
            // next key, as designed, and that Enter closes it.
            assert_eq!(after_extra, State::Playing, "{trace}");
        }
    }
    assert!(deadly > 0, "at least one cast lets the deadly blow fall");
}

#[test]
fn once_the_grace_is_over_enter_after_a_deadly_cast_goes_back_to_the_title() {
    let folder = ScratchFolder::new("f9-cast-then-title");
    let clock = ManualClock::new();
    let mut app = executioner_app(&folder, 1, &clock);
    app.handle_key(press(KeyCode::Char('t')));
    type_keys(&mut app, "159");
    app.handle_key(press(KeyCode::Enter));
    assert_eq!(app.state(), State::GameOver, "the cast lets the blow fall");
    assert!(!folder.save_file().path().exists(), "the save is gone");

    // Every key typed during the grace is thrown away, Enter and `q` too.
    for code in [KeyCode::Enter, KeyCode::Char('q'), KeyCode::Enter] {
        app.handle_key(press(code));
        assert_eq!(app.state(), State::GameOver, "{code:?} during the grace");
    }
    clock.advance(END_SCREEN_GRACE - MILLISECOND);
    app.handle_key(press(KeyCode::Enter));
    assert_eq!(app.state(), State::GameOver, "a millisecond before the end");
    clock.advance(MILLISECOND);
    // Nothing typed during the grace was kept for later.
    assert_eq!(app.state(), State::GameOver, "no key was queued");
    assert!(screen_text(&app, 80, 24).contains("T H E   D E S C E N T   E N D S"));

    app.handle_key(press(KeyCode::Enter));
    assert!(matches!(app.state(), State::Title(_)), "{:?}", app.state());
    let title = screen_text(&app, 80, 24);
    assert!(title.contains("New game"), "{title}");
    assert!(!title.contains("Continue"), "{title}");
}

#[test]
fn the_grace_also_protects_a_death_in_melee() {
    let folder = ScratchFolder::new("f9-melee-grace");
    let clock = ManualClock::new();
    let mut app = executioner_app(&folder, 1, &clock);
    // Blows at the executioner, until it answers with a deadly one.
    fight_the_executioner(&mut app);

    app.handle_key(press(KeyCode::Enter));
    app.handle_key(press(KeyCode::Char('q')));
    assert_eq!(app.state(), State::GameOver, "held back by the grace");
    clock.advance(END_SCREEN_GRACE);
    app.handle_key(press(KeyCode::Enter));
    assert!(matches!(app.state(), State::Title(_)), "{:?}", app.state());
}

#[test]
fn the_grace_also_protects_a_death_while_a_movement_key_is_held() {
    let folder = ScratchFolder::new("f9-held-key-grace");
    let clock = ManualClock::new();
    let mut app = executioner_app(&folder, 3, &clock);
    // A held key: the terminal keeps sending presses, then repeats, well
    // after the walk ended in the executioner's blow.
    for _ in 0..30 {
        app.handle_key(press(KeyCode::Char('l')));
    }
    assert_eq!(app.state(), State::GameOver, "the walk ends in death");
    for _ in 0..30 {
        app.handle_key(KeyEvent::new_with_kind(
            KeyCode::Char('l'),
            KeyModifiers::NONE,
            ratatui::crossterm::event::KeyEventKind::Repeat,
        ));
    }
    // The finger moves from the arrow to Enter, half a second later.
    clock.advance(END_SCREEN_GRACE / 2);
    app.handle_key(press(KeyCode::Enter));
    assert_eq!(app.state(), State::GameOver, "held back by the grace");

    clock.advance(END_SCREEN_GRACE / 2);
    app.handle_key(press(KeyCode::Enter));
    assert!(matches!(app.state(), State::Title(_)), "{:?}", app.state());
}

#[test]
fn ctrl_c_quits_at_once_during_the_grace() {
    let folder = ScratchFolder::new("f9-grace-ctrl-c");
    let clock = ManualClock::new();
    let mut app = executioner_app(&folder, 1, &clock);
    fight_the_executioner(&mut app);

    app.handle_key(ctrl_c());
    assert_eq!(app.state(), State::Quit);
    app.save_on_exit().expect("nothing to save");
    assert!(!folder.save_file().path().exists());
}

#[test]
fn keys_typed_during_the_grace_are_not_kept_for_later_and_q_quits_only_after_it() {
    let folder = ScratchFolder::new("f9-grace-nothing-kept");
    let clock = ManualClock::new();
    let mut app = executioner_app(&folder, 1, &clock);
    fight_the_executioner(&mut app);

    // Enter pressed at the very start of the grace must not be remembered:
    // once the grace is over the end screen is still there, waiting for a
    // key pressed on purpose.
    app.handle_key(press(KeyCode::Enter));
    clock.advance(END_SCREEN_GRACE);
    assert_eq!(app.state(), State::GameOver, "no key was queued");
    assert!(screen_text(&app, 80, 24).contains("T H E   D E S C E N T   E N D S"));

    // A fresh end screen, to try `q` on the two sides of the delay.
    let folder = ScratchFolder::new("f9-grace-q");
    let clock = ManualClock::new();
    let mut app = executioner_app(&folder, 1, &clock);
    fight_the_executioner(&mut app);
    clock.advance(END_SCREEN_GRACE - MILLISECOND);
    app.handle_key(press(KeyCode::Char('q')));
    assert_eq!(app.state(), State::GameOver, "q a millisecond too early");
    clock.advance(MILLISECOND);
    app.handle_key(press(KeyCode::Char('q')));
    assert_eq!(app.state(), State::Quit, "q once the grace is over quits");
}
