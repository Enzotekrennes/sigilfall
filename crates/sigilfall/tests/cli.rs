//! Command-line tests of the real `sigilfall` executable.
//!
//! Only invocations that end before the game takes over the terminal are
//! run here (invalid arguments, `--help`, `--version`), so the tests need no
//! terminal and leave none in a changed state.

use std::process::{Command, Output};

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
fn help_lists_the_seed_and_ascii_options() {
    let output = sigilfall(&["--help"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "{stdout}");
    assert!(stdout.contains("--seed <SEED>"), "{stdout}");
    assert!(stdout.contains("--ascii"), "{stdout}");
}
