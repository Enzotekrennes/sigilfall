//! The run recorded for the README's GIF: the keys `demo.tape` types once
//! the game has started, replayed through the application.
//!
//! VHS, which plays `demo.tape`, only runs where a real terminal and
//! ffmpeg are available, and the GIF is recorded by CI long after the
//! keys were chosen. These tests read the tape the way VHS does, launch the
//! application the way `main` does with the command line the tape types,
//! on an empty data folder and an 80 × 24 screen as the tape sets up, and
//! play every key. They check that the run shows what the tape promises: a
//! glyph read, a sigil traced and cast, a fight, the stairs taken, and the
//! game left with `q`. A change to the game that makes the recorded run
//! drift (a new random draw, a moved glyph) fails here, not in the GIF.

use super::player_view::launch_with_arguments;
use super::readme::README;
use super::save_files::launch;
use super::*;
use crate::app::{App, State};
use crate::cli::Cli;
use crate::persist::ScratchFolder;
use clap::Parser as _;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use sigilfall_core::game::Game;
use sigilfall_core::rng::Seed;

/// The VHS script, from the root of the repository.
const TAPE: &str = include_str!("../../../../../demo.tape");

/// Seed the tape types on the seed entry of the title screen.
const DEMO_SEED: Seed = Seed::new(9940);

/// Width and height of the terminal the tape sets with `stty`: the
/// smallest the game supports.
const SCREEN: (u16, u16) = (80, 24);

/// One command of a VHS tape, as far as these tests need to know it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TapeCommand {
    /// `Type "text"`: each character typed in turn.
    Type(String),
    /// A key command such as `Enter` or `Down 3`: the key, pressed that
    /// many times.
    Key(KeyCode, usize),
    /// `Ctrl+C`.
    CtrlC,
    /// `Sleep`: time passes, no key is sent.
    Sleep,
    /// Any other command (`Set`, `Hide`, `Output`…), by name: it sends no
    /// key to the game.
    Other(String),
}

/// Reads `tape` into its commands, in order; blank lines and `#` comments
/// are skipped.
///
/// A command may carry a typing speed (`Type@150ms`, `Down@1s`), which is
/// ignored, and a key command a repeat count (`Down 3`).
///
/// # Panics
///
/// Panics on a `Type` whose text is not between double quotes, or on a
/// repeat count that is not a number, naming the line.
fn parse_tape(tape: &str) -> Vec<TapeCommand> {
    tape.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(parse_line)
        .collect()
}

/// Reads one non-blank, non-comment line of a tape (see [`parse_tape`]).
fn parse_line(line: &str) -> TapeCommand {
    let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
    let name = word.split('@').next().unwrap_or(word);
    if name == "Type" {
        let text = rest
            .trim()
            .strip_prefix('"')
            .and_then(|text| text.strip_suffix('"'))
            .unwrap_or_else(|| panic!("the text of `{line}` is between double quotes"));
        return TapeCommand::Type(text.to_owned());
    }
    let key = match name {
        "Enter" => KeyCode::Enter,
        "Escape" => KeyCode::Esc,
        "Backspace" => KeyCode::Backspace,
        "Space" => KeyCode::Char(' '),
        "Tab" => KeyCode::Tab,
        "Up" => KeyCode::Up,
        "Down" => KeyCode::Down,
        "Left" => KeyCode::Left,
        "Right" => KeyCode::Right,
        "Ctrl+C" => return TapeCommand::CtrlC,
        "Sleep" => return TapeCommand::Sleep,
        other => return TapeCommand::Other(other.to_owned()),
    };
    let count = match rest.trim() {
        "" => 1,
        count => count
            .parse()
            .unwrap_or_else(|_| panic!("the repeat count of `{line}` is a number")),
    };
    TapeCommand::Key(key, count)
}

/// What the tape does: the shell lines it types and enters before the
/// game starts, the command line that starts the game, and the key presses
/// sent to the game afterwards.
#[derive(Debug)]
struct Demo {
    /// Every line typed and entered before the game, in order.
    setup: Vec<String>,
    /// The words of the line that starts the game, `sigilfall` first.
    command_line: Vec<String>,
    /// Every key sent to the game, in order.
    keys: Vec<KeyEvent>,
}

/// Splits the commands of a tape into a [`Demo`]: the game starts with the
/// first line typed whose first word is `sigilfall`, once Enter is pressed.
///
/// # Panics
///
/// Panics if no line starts the game, or if a command after the start would
/// do something these tests cannot replay (anything but keys, text and
/// `Sleep`), so the tape and its check never drift apart in silence.
fn split_demo(commands: &[TapeCommand]) -> Demo {
    let mut setup = Vec::new();
    let mut typed = String::new();
    let mut commands = commands.iter();
    let command_line = loop {
        match commands.next() {
            None => panic!("no line of the tape starts `sigilfall`"),
            Some(TapeCommand::Type(text)) => typed.push_str(text),
            Some(TapeCommand::Key(KeyCode::Enter, 1)) => {
                let line = std::mem::take(&mut typed);
                if line.split_whitespace().next() == Some("sigilfall") {
                    break line.split_whitespace().map(str::to_owned).collect();
                }
                setup.push(line);
            }
            Some(_) => {}
        }
    };
    let mut keys = Vec::new();
    for command in commands {
        match command {
            TapeCommand::Type(text) => keys.extend(text.chars().map(typed_key)),
            TapeCommand::Key(code, count) => keys.extend((0..*count).map(|_| press(*code))),
            TapeCommand::CtrlC => {
                keys.push(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
            }
            TapeCommand::Sleep => {}
            TapeCommand::Other(name) => {
                panic!("`{name}` after the game starts cannot be replayed by these tests")
            }
        }
    }
    Demo {
        setup,
        command_line,
        keys,
    }
}

/// The key press a terminal reports for `typed`: Shift is held for an
/// upper-case letter.
fn typed_key(typed: char) -> KeyEvent {
    let modifiers = if typed.is_ascii_uppercase() {
        KeyModifiers::SHIFT
    } else {
        KeyModifiers::NONE
    };
    KeyEvent::new(KeyCode::Char(typed), modifiers)
}

/// The game in progress in `app`, if any: kept after each key, so the run
/// can still be checked once `q` has ended it.
fn game_in_progress(app: &App) -> Option<Game> {
    app.run.as_ref().map(|run| run.game.clone())
}

#[test]
fn the_tape_reader_reads_text_counts_speeds_and_comments() {
    let commands = parse_tape(
        "# a comment\n\nType@150ms \"9a\"\nDown 3\nEnter@1s\nSleep 2s\nCtrl+C\nSet FontSize 20\n",
    );

    assert_eq!(
        commands,
        [
            TapeCommand::Type("9a".to_owned()),
            TapeCommand::Key(KeyCode::Down, 3),
            TapeCommand::Key(KeyCode::Enter, 1),
            TapeCommand::Sleep,
            TapeCommand::CtrlC,
            TapeCommand::Other("Set".to_owned()),
        ]
    );
}

#[test]
fn the_game_starts_at_the_line_that_runs_sigilfall_and_only_keys_follow() {
    let demo = split_demo(&parse_tape(
        "Type \"cd /tmp\"\nEnter\nType \"sigilfall --ascii\"\nEnter\nType \"jJ\"\nDown 2\n",
    ));

    assert_eq!(demo.setup, ["cd /tmp"]);
    assert_eq!(demo.command_line, ["sigilfall", "--ascii"]);
    assert_eq!(
        demo.keys,
        [
            press(KeyCode::Char('j')),
            KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT),
            press(KeyCode::Down),
            press(KeyCode::Down),
        ]
    );
}

#[test]
#[should_panic(expected = "cannot be replayed")]
fn a_command_the_replay_cannot_follow_after_the_start_is_refused() {
    split_demo(&parse_tape("Type \"sigilfall\"\nEnter\nHide\n"));
}

#[test]
fn the_demo_starts_on_an_empty_data_folder_in_an_80_by_24_terminal() {
    let demo = split_demo(&parse_tape(TAPE));
    let setup = demo.setup.join("\n");

    // The replay below launches on an empty folder and draws 80 × 24: the
    // recording must do the same for the two to match.
    assert!(setup.contains("SIGILFALL_DATA_DIR=$(mktemp -d)"), "{setup}");
    assert!(setup.contains("stty cols 80 rows 24"), "{setup}");
    assert_eq!(demo.command_line, ["sigilfall"]);
}

#[test]
fn the_demo_reads_a_glyph_casts_ember_fights_and_goes_down_the_stairs() {
    let demo = split_demo(&parse_tape(TAPE));
    let cli = Cli::try_parse_from(&demo.command_line).expect("the tape's command line is valid");
    assert!(!cli.ascii, "the replay launches without --ascii");
    let folder = ScratchFolder::new("demo-tape");
    let mut app = launch(&folder, cli.seed).expect("the launch on an empty folder succeeds");
    let (width, height) = SCREEN;

    let mut last_game = None;
    for (index, key) in demo.keys.iter().enumerate() {
        assert_ne!(
            app.state(),
            State::Quit,
            "key {index} ({key:?}) comes after the game has quit"
        );
        // The event loop draws before it reads each key.
        render(&app, width, height);
        app.handle_key(*key);
        last_game = game_in_progress(&app).or(last_game);
    }

    assert_eq!(app.state(), State::Quit, "the tape ends by quitting");
    let game = last_game.expect("the tape starts a game");
    assert_eq!(game.seed(), DEMO_SEED);
    assert!(!game.is_over(), "the player survives the demo");
    assert_eq!(game.depth(), 2, "the demo ends on the second floor");
    assert!(game.grimoire().knows("ember"), "the glyph of Ember is read");
    let stats = game.stats();
    assert!(stats.glyphs_learnt() >= 1);
    assert_eq!(stats.casts().pure, 1, "Ember is cast once, pure");
    assert_eq!(stats.casts().total(), 1);
    assert!(stats.total_kills() >= 1, "a monster dies on screen");
    let log: Vec<&str> = app
        .log()
        .newest_first()
        .map(|message| message.text.as_str())
        .collect();
    assert!(
        log.iter()
            .any(|text| text.starts_with("You hit the") || text.starts_with("You kill the")),
        "the player strikes in melee: {log:?}"
    );
}

// --- The recorded demo replays ------------------------------------------------

/// One key press per character of each `Type` line of `demo.tape` after the
/// line that starts the game, with `Down`, `Enter`, `Escape` as keys. A
/// second, independent reader of the tape.
fn keys_of_the_tape(tape: &str) -> Vec<KeyEvent> {
    let mut keys = Vec::new();
    let mut started = false;
    let mut typed_line = String::new();
    for line in tape
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let mut parts = line.splitn(2, ' ');
        let word = parts.next().unwrap();
        let rest = parts.next().unwrap_or("").trim();
        let word = word.split('@').next().unwrap();
        let command: Vec<KeyEvent> = match word {
            "Type" => {
                let text = rest.trim_matches('"');
                if !started {
                    typed_line.push_str(text);
                    continue;
                }
                text.chars().map(|c| press(KeyCode::Char(c))).collect()
            }
            "Enter" => {
                if !started {
                    started = typed_line.trim() == "sigilfall";
                    typed_line.clear();
                    continue;
                }
                vec![press(KeyCode::Enter)]
            }
            "Down" if started => vec![press(KeyCode::Down)],
            "Up" if started => vec![press(KeyCode::Up)],
            "Escape" if started => vec![press(KeyCode::Esc)],
            "Backspace" if started => vec![press(KeyCode::Backspace)],
            _ => continue,
        };
        keys.extend(command);
    }
    keys
}

/// What the recorded run shows, played from the title of a fresh folder.
struct DemoRun {
    /// The game at the end (kept before the final `q`).
    depth: u32,
    /// Sigils cast.
    casts: u32,
    /// Creatures slain.
    kills: u32,
    /// Glyphs read.
    glyphs: u32,
    /// The seed of the run.
    seed: Seed,
    /// Whether the last key quit the game.
    quit: bool,
}

/// Replays `keys` from the title screen of a fresh folder.
fn replay(keys: &[KeyEvent]) -> DemoRun {
    let folder = ScratchFolder::new("f10-demo");
    let mut app = launch_with_arguments(&folder, &[]).unwrap();
    let mut last = None;
    for key in keys {
        if matches!(app.state(), State::Quit) {
            break;
        }
        if app.state() == State::Playing || matches!(app.state(), State::Casting(_)) {
            last = Some(app.game().clone());
        }
        app.handle_key(*key);
    }
    let game = last.expect("the run reached a game");
    let stats = game.stats();
    DemoRun {
        depth: game.depth().max(stats.deepest()),
        casts: stats.casts().total(),
        kills: stats.total_kills(),
        glyphs: stats.glyphs_learnt(),
        seed: game.seed(),
        quit: app.state() == State::Quit,
    }
}

#[test]
fn the_recorded_demo_reads_a_glyph_casts_fights_goes_down_and_quits() {
    let tape = include_str!("../../../../../demo.tape");
    let keys = keys_of_the_tape(tape);
    assert!(keys.len() > 50, "the tape sends many keys: {}", keys.len());

    let run = replay(&keys);
    assert_eq!(run.seed, Seed::from(9940), "the seed typed on the title");
    assert!(run.glyphs >= 1, "a glyph is read");
    assert!(run.casts >= 1, "a sigil is cast");
    assert!(run.kills >= 1, "a creature is slain");
    assert!(run.depth >= 2, "the stairs are taken: depth {}", run.depth);
    assert!(run.quit, "the run ends with q");

    // The checks can fail: take the tape apart and the run no longer
    // satisfies them (no walk to the glyph, no cast, no descent).
    for (needle, what) in [
        ("Type@110ms \"bbbbyybbyyyyykk\"", "the walk to the glyph"),
        ("Type \"t\"", "opening the grid"),
        ("Type \">\"", "going down"),
    ] {
        assert!(tape.contains(needle), "the tape still contains {what}");
        let broken = tape.replace(needle, "Sleep 1s");
        let broken_run = replay(&keys_of_the_tape(&broken));
        let still_whole = broken_run.glyphs >= 1
            && broken_run.casts >= 1
            && broken_run.kills >= 1
            && broken_run.depth >= 2;
        assert!(
            !still_whole,
            "without {what} the run must lose something: {:?}",
            (
                broken_run.glyphs,
                broken_run.casts,
                broken_run.kills,
                broken_run.depth
            )
        );
    }
}

#[test]
fn the_demo_tape_uses_only_commands_vhs_has_and_the_replay_can_play() {
    let tape = include_str!("../../../../../demo.tape");
    let allowed = [
        "Output",
        "Require",
        "Set",
        "Hide",
        "Show",
        "Type",
        "Enter",
        "Down",
        "Up",
        "Escape",
        "Backspace",
        "Space",
        "Tab",
        "Left",
        "Right",
        "Sleep",
        "Ctrl+C",
    ];
    for line in tape
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let word = line.split([' ', '@']).next().unwrap();
        assert!(
            allowed.contains(&word),
            "`{line}`: {word} is not a command the replay understands"
        );
    }
    assert!(
        tape.contains("Output assets/demo.gif"),
        "the GIF the README shows"
    );
    assert!(tape.contains("Require sigilfall"));
    assert!(
        tape.contains("stty cols 80 rows 24"),
        "80 x 24, the smallest supported size"
    );
    assert!(
        README.contains("![A run of seed 9940"),
        "the README shows the GIF of seed 9940"
    );
}
