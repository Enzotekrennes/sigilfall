//! Acceptance tests of F2, scenario 3: `--seed` with a value that is not a
//! seed.
//!
//! Each case starts the real executable with an invalid argument. Such a
//! run must end before the terminal is touched, so these tests need no
//! terminal and never start a game.

use std::process::{Command, Output, Stdio};

/// Runs the built `sigilfall` executable with `arguments`, with no input.
fn sigilfall(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sigilfall"))
        .args(arguments)
        .stdin(Stdio::null())
        .output()
        .expect("start the sigilfall executable")
}

/// Checks that `output` is a clean refusal of the seed `value`.
fn assert_clear_refusal(value: &str, output: &Output) {
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(output.status.code(), Some(2), "{value:?}: {stderr}");
    assert!(output.stdout.is_empty(), "{value:?}: nothing on stdout");
    assert!(
        !stderr.contains('\x1b'),
        "{value:?}: no terminal escape in the message: {stderr:?}"
    );
    assert!(!stderr.contains("panicked"), "{value:?}: {stderr}");
    assert!(stderr.contains("--seed"), "{value:?}: {stderr}");
    assert!(stderr.contains("such as 42"), "{value:?}: {stderr}");
    assert!(
        stderr.contains("7F3A-91C2-0B5E-D4A8"),
        "{value:?}: {stderr}"
    );
}

#[test]
fn seed_abc_is_refused_and_says_which_formats_are_expected() {
    let output = sigilfall(&["--seed", "abc"]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_clear_refusal("abc", &output);
    assert!(
        stderr.contains("'abc'"),
        "the bad value is quoted: {stderr}"
    );
}

#[test]
fn the_equals_form_is_refused_the_same_way() {
    assert_clear_refusal("abc", &sigilfall(&["--seed=abc"]));
}

#[test]
fn other_malformed_seeds_are_refused_the_same_way() {
    for value in [
        "",
        " 42",
        "42 ",
        "4.2",
        "0x2A",
        "18446744073709551616",
        "7F3A-91C2-0B5E",
        "7F3A-91C2-0B5E-D4A8-0000",
        "7F3A-91C2-0B5E-D4AG",
        "7F3A91C20B5ED4A8",
        "'; rm -rf / #",
        &"9".repeat(5000),
    ] {
        assert_clear_refusal(value, &sigilfall(&["--seed", value]));
    }
}

#[test]
fn a_bad_seed_is_refused_even_when_other_options_are_valid() {
    assert_clear_refusal("abc", &sigilfall(&["--ascii", "--seed", "abc"]));
}

#[test]
fn a_seed_option_without_a_value_is_refused() {
    let output = sigilfall(&["--seed"]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(output.stdout.is_empty());
    assert!(stderr.contains("--seed"), "{stderr}");
}

#[test]
fn a_negative_seed_given_with_an_equals_sign_is_refused_with_the_format() {
    assert_clear_refusal("-1", &sigilfall(&["--seed=-1"]));
}

/// Known defect (reported as bug B1 of the F2 test): with a space, clap
/// reads `-1` as an unknown option and the message does not mention the
/// seed formats. Remove the `ignore` once the message names them.
#[test]
#[ignore = "bug B1: `--seed -1` is reported as an unexpected argument, without the seed format"]
fn a_negative_seed_given_after_a_space_is_refused_with_the_format() {
    assert_clear_refusal("-1", &sigilfall(&["--seed", "-1"]));
}
