//! Command-line tests of the `--lang` and `--keyboard` options of the real
//! `sigilfall` executable.
//!
//! Only invocations that end before the game takes over the terminal are
//! run here: a value that is not one of the choices, a missing value, an
//! option given twice and `--help`. Each refusal must name the option, the
//! value it received and the values it accepts, leave the standard output
//! empty, and end with the exit code clap uses for a usage error.

use std::process::{Command, Output};

/// Exit code of a command-line usage error.
const USAGE_ERROR: i32 = 2;

/// How clap lists the languages the `--lang` option accepts.
const LANGUAGES: &str = "[possible values: en, fr]";

/// How clap lists the layouts the `--keyboard` option accepts.
const LAYOUTS: &str = "[possible values: qwerty, azerty]";

/// Runs the built `sigilfall` executable with `arguments` and returns what
/// it printed and how it ended.
fn sigilfall(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sigilfall"))
        .args(arguments)
        .output()
        .expect("start the sigilfall executable")
}

/// Asserts that `output` is a clean usage error whose message contains each
/// of `expected`.
fn assert_refused(output: &Output, expected: &[&str]) {
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(output.status.code(), Some(USAGE_ERROR), "{stderr}");
    assert!(output.stdout.is_empty(), "nothing drawn on the terminal");
    for fragment in expected {
        assert!(
            stderr.contains(fragment),
            "missing {fragment:?} in {stderr}"
        );
    }
}

#[test]
fn an_unknown_language_is_refused_with_the_languages_accepted() {
    let output = sigilfall(&["--lang", "xx"]);

    assert_refused(&output, &["'xx'", "--lang", LANGUAGES]);
}

#[test]
fn an_unknown_keyboard_is_refused_with_the_layouts_accepted() {
    let output = sigilfall(&["--keyboard", "xx"]);

    assert_refused(&output, &["'xx'", "--keyboard", LAYOUTS]);
}

#[test]
fn the_equals_form_is_refused_like_the_spaced_form() {
    assert_refused(
        &sigilfall(&["--lang=klingon"]),
        &["'klingon'", "--lang", LANGUAGES],
    );
    assert_refused(
        &sigilfall(&["--keyboard=dvorak"]),
        &["'dvorak'", "--keyboard", LAYOUTS],
    );
}

#[test]
fn an_empty_value_is_refused() {
    assert_refused(&sigilfall(&["--lang", ""]), &["--lang", LANGUAGES]);
    assert_refused(&sigilfall(&["--keyboard", ""]), &["--keyboard", LAYOUTS]);
}

#[test]
fn a_missing_value_is_refused_and_names_the_choices() {
    assert_refused(&sigilfall(&["--lang"]), &["--lang", LANGUAGES]);
    assert_refused(&sigilfall(&["--keyboard"]), &["--keyboard", LAYOUTS]);
}

#[test]
fn a_valid_option_does_not_excuse_an_invalid_one() {
    let output = sigilfall(&["--lang", "fr", "--keyboard", "xx"]);

    assert_refused(&output, &["'xx'", "--keyboard", LAYOUTS]);
}

#[test]
fn giving_an_option_twice_is_refused() {
    assert_refused(
        &sigilfall(&["--lang", "fr", "--lang", "en"]),
        &["--lang", "cannot be used multiple times"],
    );
    assert_refused(
        &sigilfall(&["--keyboard", "azerty", "--keyboard", "qwerty"]),
        &["--keyboard", "cannot be used multiple times"],
    );
}

#[test]
fn help_lists_the_language_and_keyboard_options_with_their_values() {
    let output = sigilfall(&["--help"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "{stdout}");
    for fragment in [
        "--lang <LANG>",
        "- en: English",
        "- fr: French",
        "--keyboard <LAYOUT>",
        "- qwerty: QWERTY",
        "- azerty: AZERTY",
    ] {
        assert!(
            stdout.contains(fragment),
            "missing {fragment:?} in {stdout}"
        );
    }
}
