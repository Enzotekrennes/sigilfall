//! Command-line tests of the real `sigilfall` executable.
//!
//! Only invocations that end before the game takes over the terminal are
//! run here (invalid or conflicting arguments, `--help`, `--version`, a
//! redirected output), so the tests need no terminal and leave none in a
//! changed state.

use std::io::Read as _;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// How long the game may take to refuse a redirected output. Refusing takes
/// milliseconds; the margin absorbs a slow, loaded CI machine, while a game
/// stuck waiting would never end.
const REFUSAL_DEADLINE: Duration = Duration::from_secs(10);

/// Pause between two checks of whether the game has ended.
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Runs the built `sigilfall` executable with `arguments` and returns what
/// it printed and how it ended.
fn sigilfall(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sigilfall"))
        .args(arguments)
        .output()
        .expect("start the sigilfall executable")
}

#[test]
fn an_invalid_seed_fails_with_the_expected_format() {
    let output = sigilfall(&["--seed", "abc"]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(output.stdout.is_empty(), "nothing drawn on the terminal");
    assert!(stderr.contains("'abc'"), "{stderr}");
    assert!(stderr.contains("--seed"), "{stderr}");
    assert!(stderr.contains("such as 42"), "{stderr}");
    assert!(stderr.contains("7F3A-91C2-0B5E-D4A8"), "{stderr}");
}

#[test]
fn daily_with_a_seed_fails_before_the_game_starts() {
    let output = sigilfall(&["--daily", "--seed", "42"]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(output.stdout.is_empty(), "nothing drawn on the terminal");
    assert!(
        stderr.contains("the argument '--daily' cannot be used with '--seed <SEED>'"),
        "{stderr}"
    );
}

#[test]
fn help_lists_the_seed_daily_and_ascii_options() {
    let output = sigilfall(&["--help"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "{stdout}");
    assert!(stdout.contains("--seed <SEED>"), "{stdout}");
    assert!(stdout.contains("--daily"), "{stdout}");
    assert!(stdout.contains("--ascii"), "{stdout}");
}

#[test]
fn help_is_wrapped_to_the_width_in_columns() {
    // Under `cargo test` the output is a pipe, not a terminal, so the width
    // comes from COLUMNS, as in a shell that exports it.
    let output = Command::new(env!("CARGO_BIN_EXE_sigilfall"))
        .arg("--help")
        .env("COLUMNS", "80")
        .output()
        .expect("start the sigilfall executable");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "{stdout}");
    assert!(stdout.contains("--seed <SEED>"), "{stdout}");
    for line in stdout.lines() {
        assert!(line.chars().count() <= 80, "{line:?} in\n{stdout}");
    }
}

#[test]
fn a_redirected_output_is_refused_at_once_with_a_clear_message() {
    // Under `cargo test`, the pipes set up below are not terminals, just
    // like `sigilfall --seed 42 > out.txt`.
    let mut child = Command::new(env!("CARGO_BIN_EXE_sigilfall"))
        .args(["--seed", "42"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start the sigilfall executable");

    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("check the sigilfall process") {
            break status;
        }
        if started.elapsed() > REFUSAL_DEADLINE {
            // Kill it so a regression fails the test instead of hanging it.
            child.kill().expect("stop the stuck sigilfall process");
            child.wait().expect("reap the stuck sigilfall process");
            panic!("sigilfall still running after {REFUSAL_DEADLINE:?} with a redirected output");
        }
        thread::sleep(POLL_INTERVAL);
    };
    let mut stdout = Vec::new();
    let mut stderr = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_end(&mut stdout)
        .unwrap();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();

    assert!(!status.success(), "{status:?}: {stderr}");
    assert!(stdout.is_empty(), "nothing drawn on the redirected output");
    assert!(!stderr.contains(''), "no terminal escape: {stderr:?}");
    assert!(!stderr.contains("panicked"), "{stderr}");
    assert!(
        stderr.contains("standard output is not a terminal"),
        "{stderr}"
    );
}
