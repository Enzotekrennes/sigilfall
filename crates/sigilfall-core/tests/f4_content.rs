//! Acceptance tests of F4, scenario 3: a monster file that is not valid is
//! refused with an error that names the monster and the field in cause.
//!
//! Written against the public API only (`Content::parse`, `Content::load`).
//! Each case starts from a valid file and breaks exactly one thing, so the
//! error under test is the only possible one.
//!
//! The "unknown reference" case of the scenario cannot be tested yet: the
//! monster format holds no field that refers to another entry (journal T13,
//! postponed to T20 and T22). `the_format_has_no_reference_field_yet` records
//! that fact, so the day a reference appears this file shows it.

use sigilfall_core::content::{Content, ContentError};

/// One valid entry, written field by field like `content/monsters.ron`.
/// `{id}`, `{hp}` and `{speed}` are replaced by the cases.
const ENTRY: &str = "    Monster(
        id: \"{id}\",
        name: \"test creature\",
        family: Vermin,
        elite: false,
        speed: {speed},
        hp: {hp},
        attack: 2,
        defense: 1,
        min_depth: 1,
    ),
";

/// Builds an entry from `ENTRY` with the given values.
fn entry(id: &str, speed: &str, hp: &str) -> String {
    ENTRY
        .replace("{id}", id)
        .replace("{speed}", speed)
        .replace("{hp}", hp)
}

/// Wraps entries into a file.
fn file(entries: &[String]) -> String {
    format!("[\n{}]\n", entries.concat())
}

/// Parses `text` and returns the message of the error it must produce.
fn refusal(text: &str) -> String {
    match Content::parse(text) {
        Ok(_) => panic!("the file was accepted, an error was expected:\n{text}"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn the_shipped_file_is_valid() {
    let content = Content::load().expect("shipped content must load");
    assert!(!content.monsters().is_empty());
    assert!(content.monsters_at(1).count() > 0);
}

#[test]
fn negative_hit_points_name_the_monster_and_the_field() {
    let text = file(&[
        entry("good_rat", "100", "5"),
        entry("sick_rat", "100", "-4"),
    ]);

    let message = refusal(&text);

    assert!(message.contains("sick_rat"), "monster not named: {message}");
    assert!(message.contains("`hp`"), "field not named: {message}");
    assert!(message.contains("-4"), "value not shown: {message}");
    assert!(
        !message.contains("good_rat"),
        "the valid monster is blamed: {message}"
    );
}

#[test]
fn the_error_is_typed_and_carries_the_entry_and_the_field() {
    let text = file(&[
        entry("good_rat", "100", "5"),
        entry("sick_rat", "100", "-4"),
    ]);

    match Content::parse(&text) {
        Err(ContentError::InvalidField { entry, field, .. }) => {
            assert!(entry.contains("sick_rat"), "{entry}");
            assert!(entry.contains("entry 2"), "position missing: {entry}");
            assert_eq!(field, "hp");
        }
        other => panic!("expected InvalidField, got {other:?}"),
    }
}

#[test]
fn zero_hit_points_and_zero_or_negative_speed_are_refused() {
    for (speed, hp, field) in [
        ("100", "0", "`hp`"),
        ("0", "5", "`speed`"),
        ("-100", "5", "`speed`"),
    ] {
        let message = refusal(&file(&[entry("odd_rat", speed, hp)]));
        assert!(message.contains("odd_rat"), "{message}");
        assert!(message.contains(field), "{message}");
    }
}

#[test]
fn absurd_values_are_refused_and_the_field_is_named() {
    for (speed, hp, field) in [("5000", "5", "`speed`"), ("100", "100000", "`hp`")] {
        let message = refusal(&file(&[entry("odd_rat", speed, hp)]));
        assert!(message.contains("odd_rat"), "{message}");
        assert!(message.contains(field), "{message}");
    }
}

#[test]
fn a_repeated_identifier_names_both_entries() {
    let text = file(&[entry("twin", "100", "5"), entry("twin", "100", "5")]);

    let message = refusal(&text);

    assert!(message.contains("twin"), "{message}");
    assert!(message.contains("`id`"), "{message}");
}

#[test]
fn a_missing_field_is_named() {
    let text = file(&[entry("lone", "100", "5")]).replace("        hp: 5,\n", "");

    let message = refusal(&text);

    assert!(message.contains("lone"), "{message}");
    assert!(message.contains("`hp`"), "{message}");
    assert!(message.contains("missing"), "{message}");
}

#[test]
fn a_value_of_the_wrong_type_gives_a_line_and_the_field() {
    let text = file(&[entry("typo_rat", "100", "\"five\"")]);

    let message = refusal(&text);

    assert!(message.contains("typo_rat"), "{message}");
    assert!(message.contains("`hp`"), "{message}");
    assert!(message.contains("line"), "no position: {message}");
}

#[test]
fn an_unknown_family_is_refused_with_the_entry() {
    let text = file(&[entry("dragon", "100", "5")]).replace("Vermin", "Dragon");

    let message = refusal(&text);

    assert!(message.contains("dragon"), "{message}");
    assert!(message.contains("family"), "{message}");
}

#[test]
fn garbage_and_empty_files_are_refused() {
    for text in ["", "[]", "not ron at all", "[Monster(", "(1, 2"] {
        assert!(Content::parse(text).is_err(), "accepted: {text:?}");
    }
}

#[test]
fn a_file_without_a_monster_for_the_first_floor_is_refused() {
    let text = file(&[entry("deep_rat", "100", "5")]).replace("min_depth: 1", "min_depth: 4");

    let message = refusal(&text);

    assert!(message.contains("deep_rat"), "{message}");
    assert!(message.contains("min_depth"), "{message}");
}

#[test]
fn the_format_has_no_reference_field_yet() {
    // A monster that points at something else (a drop, a spawn table) is
    // exactly what scenario 3 calls an "unknown reference". Such a field does
    // not exist in the format, so it is rejected as an unknown field, and no
    // check of reference exists to test.
    for field in ["drops", "loot", "summons", "spawns"] {
        let text = file(&[entry("rat", "100", "5")]).replace(
            "        min_depth: 1,\n",
            &format!("        min_depth: 1,\n        {field}: \"ghost_item\",\n"),
        );
        let message = refusal(&text);
        assert!(message.contains(field), "{field} not named: {message}");
    }
}
