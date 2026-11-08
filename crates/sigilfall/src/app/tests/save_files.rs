//! The save file across launches (T24), on scratch folders only: the game
//! saved on quitting, resumed exactly with a "Welcome back" through
//! "Continue" on the title screen (T26), deleted the moment the player dies
//! and never brought back from the end screen; a corrupt save or one of
//! another version set aside with a message before a new game; a leftover
//! temporary file never read; a failed write reported with a non-zero exit;
//! and the refusal of `--seed` while a game is saved.

use std::fs;

use sigilfall_core::save;

use super::*;
use crate::message_log::Tone;
use crate::persist::{Loaded, SaveFile, ScratchFolder};
use crate::theme::ColorSupport;
use crate::ui::title::TitleMenu;

/// A monster that kills the player in one blow, so a death takes one key.
const EXECUTIONER: &str = r#"[
    Monster(id: "executioner", name: "executioner", family: Acolyte, elite: true,
        speed: 100, hp: 999, attack: 99, defense: 0, min_depth: 1),
]"#;

/// A seed other than [`SEED`], for the second game of a test.
const OTHER_SEED: Seed = Seed::new(42);

/// Keys of a walk on a generated floor that tries every direction and
/// waits, long enough for monsters to come and fight: each key is played
/// once, in order.
pub(super) const WALK: &[char] = &[
    'l', 'l', 'j', 'j', 'h', 'k', 'y', 'u', 'b', 'n', '.', '.', 'l', 'l', 'l', 'j', 'j', 'j', 'h',
    'h', 'k', 'k', '.', '.', '.', 'n', 'n', 'u', 'u', 'b', 'y', '.', 'l', 'j', 'h', 'k',
];

/// The default theme of Windows Terminal.
pub(super) fn dark() -> Theme {
    Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode)
}

/// The command line `sigilfall`, with `--seed` if `seed` is given.
const fn command_line(seed: Option<Seed>) -> Cli {
    Cli {
        seed,
        daily: false,
        ascii: false,
    }
}

/// Launches the application as `main` does, on the save file of `folder`,
/// on a 24-bit terminal: the default options give the dark theme.
pub(super) fn launch(folder: &ScratchFolder, seed: Option<Seed>) -> anyhow::Result<App> {
    App::launch_with_clock(
        ColorSupport::TrueColor,
        &command_line(seed),
        &shipped_content(),
        folder.save_file(),
        Box::new(ManualClock::new()),
    )
}

/// Launches the application without `--seed`, as `sigilfall` alone does,
/// checks that the title screen offers "Continue" and chooses it, which
/// is the entry chosen at first.
fn resume(folder: &ScratchFolder) -> App {
    let mut app = launch(folder, None).unwrap();
    assert_eq!(app.state(), State::Title(TitleMenu::open()));
    assert!(app.has_saved_game(), "Continue is offered");
    app.handle_key(press(KeyCode::Enter));
    assert_eq!(app.state(), State::Playing, "Continue resumes the game");
    app
}

/// Launches the application without `--seed` on a folder whose save
/// cannot be continued, checks that the title screen does not offer
/// "Continue", and chooses "New game", the entry chosen at first.
fn launch_new_game(folder: &ScratchFolder) -> App {
    let mut app = launch(folder, None).unwrap();
    assert_eq!(app.state(), State::Title(TitleMenu::open()));
    assert!(!app.has_saved_game(), "Continue is not offered");
    app.handle_key(press(KeyCode::Enter));
    assert_eq!(app.state(), State::Playing, "New game starts a game");
    app
}

/// Plays each key of `keys` once, as presses.
pub(super) fn type_keys(app: &mut App, keys: &[char]) {
    for &key in keys {
        app.handle_key(press(KeyCode::Char(key)));
    }
}

/// The log of `app`, oldest message first.
pub(super) fn log_lines(app: &App) -> Vec<Message> {
    let mut lines: Vec<Message> = app.log().newest_first().cloned().collect();
    lines.reverse();
    lines
}

/// The save text of `game`, which holds its whole state, random stream
/// included: two games with the same text play on identically.
pub(super) fn state_of(game: &Game) -> String {
    save::to_string(game).expect("a game can be written")
}

/// Reads the save of `folder` back, which must hold a game.
fn saved_game(folder: &ScratchFolder) -> Game {
    match folder.save_file().load(&shipped_content()) {
        Ok(Loaded::Game(game)) => *game,
        other => panic!("a game is saved: {other:?}"),
    }
}

/// An application in the fixed room of [`SEED`], with an executioner east
/// of the player, saving to `save`.
pub(super) fn doomed_app(save: SaveFile) -> App {
    let content = Content::parse(EXECUTIONER).expect("valid test monster");
    let mut game = Game::in_fixed_room(SEED);
    game.spawn_monster(
        PLAYER_START.step(Direction::East),
        content.monster("executioner").unwrap(),
    );
    App::with_game(dark(), game, content).saving_to(save)
}

/// Waits until the executioner has killed the player of `app`.
pub(super) fn wait_for_death(app: &mut App) {
    for _ in 0..10 {
        if app.state() == State::GameOver {
            return;
        }
        app.handle_key(press(KeyCode::Char('.')));
    }
    panic!("the executioner kills the player");
}

#[test]
fn q_quits_and_the_game_in_progress_is_saved() {
    let folder = ScratchFolder::new("quit-q");
    let mut app = launch(&folder, Some(SEED)).unwrap();
    type_keys(&mut app, &WALK[..10]);
    assert!(
        !folder.save_file().path().exists(),
        "nothing is written while playing"
    );

    app.handle_key(press(KeyCode::Char('q')));
    assert_eq!(app.state(), State::Quit);
    app.save_on_exit().unwrap();

    assert_eq!(state_of(&saved_game(&folder)), state_of(app.game()));
}

#[test]
fn ctrl_c_in_an_overlay_quits_and_saves_too() {
    let folder = ScratchFolder::new("quit-ctrl-c");
    let mut app = launch(&folder, Some(SEED)).unwrap();
    type_keys(&mut app, &['l', 'j', 'i']);
    assert!(matches!(app.state(), State::Inventory(_)));

    app.handle_key(KeyEvent::new(
        KeyCode::Char('c'),
        ratatui::crossterm::event::KeyModifiers::CONTROL,
    ));
    assert_eq!(app.state(), State::Quit);
    app.save_on_exit().unwrap();

    assert_eq!(state_of(&saved_game(&folder)), state_of(app.game()));
}

#[test]
fn a_resumed_game_goes_on_exactly_as_if_never_left() {
    let folder = ScratchFolder::new("resume");
    let mut first = launch(&folder, Some(SEED)).unwrap();
    type_keys(&mut first, WALK);
    let never_left = first.game().clone();
    type_keys(&mut first, &['q']);
    first.save_on_exit().unwrap();

    let mut resumed = resume(&folder);

    assert_eq!(resumed.game().seed(), SEED);
    assert_eq!(state_of(resumed.game()), state_of(&never_left));
    let opening = log_lines(&resumed);
    assert_eq!(opening.len(), 1, "{opening:?}");
    assert_eq!(opening[0].tone, Tone::Info);
    assert!(opening[0].text.starts_with("Welcome back"), "{opening:?}");

    // The same actions give the same events, to the end of the walk...
    let mut twin = never_left.clone();
    let mut copy = resumed.game().clone();
    for key in WALK.iter().chain(WALK) {
        let Some(Command::Act(action)) = input::command_for(press(KeyCode::Char(*key))) else {
            panic!("{key} is a game action");
        };
        assert_eq!(copy.submit(action), twin.submit(action), "{key}");
    }
    // ...and the resumed application logs what the one never left would.
    let mut never_left_app = App::with_game(dark(), never_left, shipped_content());
    type_keys(&mut resumed, WALK);
    type_keys(&mut never_left_app, WALK);
    assert_eq!(
        log_lines(&resumed)[1..],
        log_lines(&never_left_app)[1..],
        "same messages after the opening one"
    );
    assert_eq!(state_of(resumed.game()), state_of(never_left_app.game()));
}

#[test]
fn quitting_a_resumed_game_saves_it_again() {
    let folder = ScratchFolder::new("resume-twice");
    let mut first = launch(&folder, Some(SEED)).unwrap();
    type_keys(&mut first, &WALK[..5]);
    first.save_on_exit().unwrap();

    let mut second = resume(&folder);
    type_keys(&mut second, &WALK[5..12]);
    type_keys(&mut second, &['q']);
    second.save_on_exit().unwrap();

    let third = resume(&folder);
    assert_eq!(state_of(third.game()), state_of(second.game()));
}

#[test]
fn the_save_is_deleted_when_the_player_dies_before_the_end_screen() {
    let folder = ScratchFolder::new("death");
    let mut app = doomed_app(folder.save_file());
    app.save_on_exit().unwrap();
    assert!(folder.save_file().path().is_file(), "a game was saved");

    wait_for_death(&mut app);

    // Nothing has been drawn since the killing blow.
    assert!(
        !folder.save_file().path().exists(),
        "deleted at the death itself"
    );
    let text = render(&app, 80, 24).backend().to_string();
    assert!(text.contains("Killed by"), "{text}");
}

#[test]
fn quitting_from_the_end_screen_brings_no_save_back() {
    let folder = ScratchFolder::new("death-quit");
    let clock = ManualClock::new();
    let mut app = doomed_app(folder.save_file()).with_clock(clock.clone());
    app.save_on_exit().unwrap();
    wait_for_death(&mut app);
    clock.advance(END_SCREEN_GRACE);

    app.handle_key(press(KeyCode::Char('q')));
    assert_eq!(app.state(), State::Quit);
    app.save_on_exit().unwrap();

    assert!(!folder.save_file().path().exists());
    assert!(!folder.path().join("save.ron.tmp").exists());
    let next = launch_new_game(&folder);
    assert_eq!(log_lines(&next), vec![messages::welcome()], "a new game");
}

#[test]
fn a_save_that_cannot_be_deleted_at_death_is_logged_and_reported_on_leaving() {
    let folder = ScratchFolder::new("death-undeletable");
    // A folder named like the save cannot be deleted as a file.
    fs::create_dir(folder.save_file().path()).unwrap();
    let mut app = doomed_app(folder.save_file());

    wait_for_death(&mut app);

    let newest = app.log().newest_first().next().unwrap().clone();
    assert_eq!(newest, messages::save_not_deleted());
    let error = app.save_on_exit().unwrap_err();
    assert!(format!("{error:#}").contains("cannot delete"), "{error:#}");
}

#[test]
fn a_corrupt_save_is_set_aside_with_a_message_and_a_new_game_starts() {
    let folder = ScratchFolder::new("corrupt");
    let save = folder.save_file();
    fs::write(save.path(), "(format_version: 1, seed: oops").unwrap();

    let mut app = launch(&folder, None).unwrap();
    assert!(!app.has_saved_game(), "nothing to continue");
    let screen = render(&app, 80, 24).backend().to_string();
    assert!(
        screen.contains("! Your saved game could not be read:"),
        "the title says so:\n{screen}"
    );
    app.handle_key(press(KeyCode::Enter));

    assert_eq!(app.state(), State::Playing, "New game was chosen");
    assert_eq!(app.game().player_turns(), 0, "a new game");
    let opening = log_lines(&app);
    assert_eq!(opening.len(), 2, "{opening:?}");
    assert_eq!(opening[0].tone, Tone::Danger);
    assert!(
        opening[0]
            .text
            .starts_with("Your saved game could not be read: "),
        "{opening:?}"
    );
    let moved_to = folder.path().join("save.ron.bad");
    assert!(
        opening[0].text.contains(&moved_to.display().to_string()),
        "{opening:?}"
    );
    assert_eq!(opening[1], messages::welcome());
    assert_eq!(
        fs::read_to_string(&moved_to).unwrap(),
        "(format_version: 1, seed: oops",
        "the old save is kept whole"
    );
    assert!(!save.path().exists());
}

#[test]
fn a_corrupt_save_is_never_overwritten_by_the_new_game() {
    let folder = ScratchFolder::new("corrupt-then-quit");
    fs::write(folder.save_file().path(), [0xFF, 0x00, 0xFE]).unwrap();
    let mut app = launch_new_game(&folder);

    type_keys(&mut app, &['l', 'q']);
    app.save_on_exit().unwrap();

    assert_eq!(
        fs::read(folder.path().join("save.ron.bad")).unwrap(),
        [0xFF, 0x00, 0xFE]
    );
    assert_eq!(state_of(&saved_game(&folder)), state_of(app.game()));
}

#[test]
fn a_save_of_another_version_is_named_as_such_and_set_aside() {
    let folder = ScratchFolder::new("other-version");
    let mut first = launch(&folder, Some(SEED)).unwrap();
    type_keys(&mut first, &WALK[..4]);
    first.save_on_exit().unwrap();
    let path = folder.save_file().path().to_path_buf();
    let text = fs::read_to_string(&path).unwrap();
    let current = format!("format_version: {},", save::FORMAT_VERSION);
    let newer = save::FORMAT_VERSION + 1;
    let text = text.replacen(&current, &format!("format_version: {newer},"), 1);
    fs::write(&path, &text).unwrap();

    let app = launch_new_game(&folder);

    let opening = log_lines(&app);
    assert_eq!(opening[0].tone, Tone::Danger);
    assert!(
        opening[0].text.contains(&format!(
            "format version {newer}, but this game reads version {}",
            save::FORMAT_VERSION
        )),
        "{opening:?}"
    );
    assert_eq!(app.game().player_turns(), 0, "a new game");
    assert_eq!(
        fs::read_to_string(folder.path().join("save.ron.bad")).unwrap(),
        text
    );
}

#[test]
fn a_leftover_temporary_file_is_never_resumed() {
    let folder = ScratchFolder::new("temporary");
    let mut first = launch(&folder, Some(SEED)).unwrap();
    type_keys(&mut first, &WALK[..6]);
    let temporary = folder.path().join("save.ron.tmp");
    fs::write(&temporary, state_of(first.game())).unwrap();

    // No save, only the leftover: neither resumed nor in the way of --seed.
    let app = launch(&folder, Some(OTHER_SEED)).unwrap();

    assert_eq!(app.game().seed(), OTHER_SEED);
    assert_eq!(log_lines(&app), vec![messages::welcome()]);
}

#[test]
fn a_leftover_temporary_file_does_not_hide_the_save() {
    let folder = ScratchFolder::new("temporary-and-save");
    let mut first = launch(&folder, Some(SEED)).unwrap();
    type_keys(&mut first, &WALK[..6]);
    first.save_on_exit().unwrap();
    // What an interrupted write leaves next to the save.
    fs::write(folder.path().join("save.ron.tmp"), "(format_version: 1, se").unwrap();

    let app = resume(&folder);

    assert_eq!(state_of(app.game()), state_of(first.game()));
    assert!(log_lines(&app)[0].text.starts_with("Welcome back"));
}

#[test]
fn a_seed_given_while_a_game_is_saved_is_refused_and_the_save_kept() {
    let folder = ScratchFolder::new("seed-refused");
    let mut first = launch(&folder, Some(SEED)).unwrap();
    type_keys(&mut first, &WALK[..6]);
    first.save_on_exit().unwrap();
    let path = folder.save_file().path().to_path_buf();
    let before = fs::read(&path).unwrap();

    for seed in [OTHER_SEED, SEED] {
        let error = launch(&folder, Some(seed)).unwrap_err();

        let message = format!("{error:#}");
        assert!(message.contains(&path.display().to_string()), "{message}");
        assert!(message.contains("without --seed"), "{message}");
        assert!(message.contains("title screen"), "{message}");
        assert!(message.contains("delete that file"), "{message}");
        assert_eq!(fs::read(&path).unwrap(), before, "the save is untouched");
    }
    let resumed = resume(&folder);
    assert_eq!(state_of(resumed.game()), state_of(first.game()));
}

#[test]
fn a_seed_given_with_a_corrupt_save_starts_that_seed() {
    let folder = ScratchFolder::new("seed-corrupt");
    fs::write(folder.save_file().path(), "not a save").unwrap();

    let app = launch(&folder, Some(OTHER_SEED)).unwrap();

    assert_eq!(app.game().seed(), OTHER_SEED);
    assert_eq!(log_lines(&app)[0].tone, Tone::Danger);
    assert!(folder.path().join("save.ron.bad").is_file());
}

#[test]
fn a_seed_given_without_a_save_starts_that_seed() {
    let folder = ScratchFolder::new("seed-fresh");

    let app = launch(&folder, Some(OTHER_SEED)).unwrap();

    assert_eq!(app.game().seed(), OTHER_SEED);
    assert_eq!(log_lines(&app), vec![messages::welcome()]);
}

#[test]
fn a_saved_finished_game_is_deleted_and_a_new_game_starts() {
    let folder = ScratchFolder::new("finished");
    let content = Content::parse(EXECUTIONER).expect("valid test monster");
    let mut dead = doomed_app(SaveFile::in_folder(folder.path().join("unused")));
    wait_for_death(&mut dead);
    fs::write(folder.save_file().path(), state_of(dead.game())).unwrap();

    let app = App::launch_with_clock(
        ColorSupport::TrueColor,
        &command_line(Some(OTHER_SEED)),
        &content,
        folder.save_file(),
        Box::new(ManualClock::new()),
    )
    .unwrap();

    assert_eq!(app.state(), State::Playing);
    assert_eq!(app.game().seed(), OTHER_SEED);
    assert!(!folder.save_file().path().exists());
}

#[test]
fn a_failed_save_on_leaving_is_an_error_naming_the_file() {
    let folder = ScratchFolder::new("write-fails");
    let blocker = folder.path().join("a-file");
    fs::write(&blocker, "").unwrap();
    // The data folder would have to be inside a file: it cannot be created.
    let save = SaveFile::in_folder(blocker.join("data"));
    let mut app = App::with_game(dark(), Game::in_fixed_room(SEED), shipped_content())
        .saving_to(save.clone());
    type_keys(&mut app, &['l', 'q']);

    let error = app.save_on_exit().unwrap_err();

    let message = format!("{error:#}");
    assert!(message.contains("could not be saved"), "{message}");
    assert!(
        message.contains(&save.path().display().to_string()),
        "{message}"
    );
    // `main` returns it, which ends the process with exit code 1 and
    // prints it on standard error, even after an earlier error.
    let reported = crate::conclude([Ok(()), Err(error), Ok(())]).unwrap_err();
    assert!(format!("{reported:#}").contains("could not be saved"));
    let first = crate::conclude([
        Err(anyhow::anyhow!("drawing failed")),
        Err(anyhow::anyhow!("not saved")),
        Ok(()),
    ])
    .unwrap_err();
    assert_eq!(first.to_string(), "drawing failed");
    assert!(crate::conclude([Ok(()), Ok(()), Ok(())]).is_ok());
}

#[test]
fn an_application_without_a_save_file_writes_nothing() {
    let mut app = new_app();
    type_keys(&mut app, &['l', 'q']);

    assert!(app.save_on_exit().is_ok());
}
