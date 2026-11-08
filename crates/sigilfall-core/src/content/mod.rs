//! The game's data: creature definitions read from RON files and validated.
//!
//! Exposes [`Content`], with [`Content::load`] for the files built into the
//! game and [`Content::parse`] for any text, [`MonsterDef`] and [`Family`],
//! the bounds of a monster definition, and the errors [`ContentError`] and
//! [`FieldProblem`]. The files are embedded in the binary at compile time
//! (`include_str!`), so an installed game needs no asset folder.
//!
//! This module only reads and checks data. Spawning monsters on a floor is
//! not done here.

mod error;
mod monster;
mod monster_file;

pub use error::{ContentError, FieldProblem};
pub use monster::{Family, MAX_ATTACK, MAX_DEFENSE, MAX_HP, MAX_SPEED, MonsterDef};

/// Path of the monster file relative to the `sigilfall-core` crate, as named
/// in error messages.
pub const MONSTERS_FILE: &str = "content/monsters.ron";

/// The monster file shipped with the game.
const MONSTERS_RON: &str = include_str!("../../content/monsters.ron");

/// Every piece of validated game data.
///
/// A `Content` value always satisfies the rules of the content files: unique
/// identifiers, values within bounds, and at least one monster for the first
/// floor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Content {
    /// Monster definitions, in file order.
    monsters: Vec<MonsterDef>,
}

impl Content {
    /// Parses and validates the content files built into the game.
    ///
    /// Called once at start-up. A test checks that the shipped files are
    /// valid, so an error here means the data was edited without running the
    /// tests.
    ///
    /// # Errors
    ///
    /// Same as [`Content::parse`].
    pub fn load() -> Result<Self, ContentError> {
        Self::parse(MONSTERS_RON)
    }

    /// Parses and validates `monsters`, the text of a monster file.
    ///
    /// The text is a RON list of `Monster(...)` entries; see
    /// `content/monsters.ron` for the fields and their meaning.
    ///
    /// # Errors
    ///
    /// - [`ContentError::Syntax`] with the line, the column, the entry and the
    ///   field when the text is not valid RON, uses an unknown or repeated
    ///   field, or a value of the wrong type;
    /// - [`ContentError::InvalidField`] naming the entry and the field when a
    ///   field is missing, out of bounds, or an identifier is malformed or
    ///   repeated;
    /// - [`ContentError::NoMonster`] or [`ContentError::NoMonsterAtFirstDepth`]
    ///   when the first floor would have no creature.
    pub fn parse(monsters: &str) -> Result<Self, ContentError> {
        let entries = monster_file::read(monsters)?;
        Ok(Self {
            monsters: monster::validate(entries)?,
        })
    }

    /// Returns every monster definition, in file order.
    pub fn monsters(&self) -> &[MonsterDef] {
        &self.monsters
    }

    /// Returns the monster definition identified by `id`, if any.
    pub fn monster(&self, id: &str) -> Option<&MonsterDef> {
        self.monsters.iter().find(|monster| monster.id() == id)
    }

    /// Returns the monsters that may appear on floor `depth`, in file order.
    ///
    /// Never empty for the first floor (`generation::FIRST_DEPTH`), nor for
    /// any deeper one.
    pub fn monsters_at(&self, depth: u32) -> impl Iterator<Item = &MonsterDef> {
        self.monsters
            .iter()
            .filter(move |monster| monster.can_appear_at(depth))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generation::FIRST_DEPTH;

    /// Fields of a valid test monster identified by `id`, in file order.
    fn fields(id: &str) -> Vec<(&'static str, String)> {
        [
            ("id", format!("\"{id}\"")),
            ("name", "\"test creature\"".to_owned()),
            ("family", "Vermin".to_owned()),
            ("elite", "false".to_owned()),
            ("speed", "100".to_owned()),
            ("hp", "5".to_owned()),
            ("attack", "1".to_owned()),
            ("defense", "0".to_owned()),
            ("min_depth", "1".to_owned()),
        ]
        .into()
    }

    /// Writes one `Monster(...)` entry, one field per line.
    fn write_entry(fields: &[(&'static str, String)]) -> String {
        let body: String = fields
            .iter()
            .map(|(name, value)| format!("        {name}: {value},\n"))
            .collect();
        format!("    Monster(\n{body}    )")
    }

    /// A valid entry identified by `id`.
    fn valid(id: &str) -> String {
        write_entry(&fields(id))
    }

    /// A valid entry identified by `id`, except that `field` holds `value`.
    fn with(id: &str, field: &str, value: &str) -> String {
        let mut fields = fields(id);
        for (name, current) in &mut fields {
            if *name == field {
                *current = value.to_owned();
            }
        }
        write_entry(&fields)
    }

    /// A valid entry identified by `id`, except that `field` is left out.
    fn without(id: &str, field: &str) -> String {
        let mut fields = fields(id);
        fields.retain(|(name, _)| *name != field);
        write_entry(&fields)
    }

    /// A monster file listing `entries`.
    fn file(entries: &[String]) -> String {
        format!("[\n{}\n]\n", entries.join(",\n"))
    }

    /// Parses `source`, expecting it to be refused, and returns the error and
    /// its message.
    fn refused(source: &str) -> (ContentError, String) {
        let error = Content::parse(source).expect_err("the content should be refused");
        let message = error.to_string();
        (error, message)
    }

    /// Asserts that `message` names the monster file, `entry` and `field`.
    fn assert_names(message: &str, entry: &str, field: &str) {
        assert!(message.contains(MONSTERS_FILE), "no file in: {message}");
        assert!(message.contains(entry), "no `{entry}` in: {message}");
        assert!(
            message.contains(&format!("`{field}`")),
            "no field `{field}` in: {message}"
        );
    }

    #[test]
    fn shipped_content_is_valid() {
        let content = Content::load().expect("the shipped content files are valid");

        let count = content.monsters().len();
        assert!((5..=8).contains(&count), "{count} monsters shipped");
        assert!(content.monsters_at(FIRST_DEPTH).next().is_some());
    }

    #[test]
    fn valid_file_is_read_in_order_with_its_values() {
        let source = file(&[
            valid("bone_rat"),
            with("deep_thing", "min_depth", "3")
                .replace("Vermin", "Spectre")
                .replace("false", "true"),
        ]);

        let content = Content::parse(&source).expect("valid content");

        let ids: Vec<&str> = content.monsters().iter().map(MonsterDef::id).collect();
        assert_eq!(ids, ["bone_rat", "deep_thing"]);
        let deep = content.monster("deep_thing").expect("defined");
        assert_eq!(deep.name(), "test creature");
        assert_eq!(deep.family(), Family::Spectre);
        assert!(deep.elite());
        assert_eq!(
            (deep.speed(), deep.hp(), deep.attack(), deep.defense()),
            (100, 5, 1, 0)
        );
        assert_eq!(deep.min_depth(), 3);
        assert!(content.monster("unknown").is_none());
    }

    #[test]
    fn monsters_at_keeps_those_allowed_at_that_depth() {
        let source = file(&[
            with("first", "min_depth", "1"),
            with("third", "min_depth", "3"),
            with("second", "min_depth", "2"),
        ]);
        let content = Content::parse(&source).expect("valid content");

        let at = |depth| -> Vec<&str> { content.monsters_at(depth).map(MonsterDef::id).collect() };

        assert_eq!(at(1), ["first"]);
        assert_eq!(at(2), ["first", "second"]);
        assert_eq!(at(3), ["first", "third", "second"]);
    }

    #[test]
    fn duplicate_identifier_is_refused() {
        let source = file(&[valid("shade"), valid("acolyte"), valid("shade")]);

        let (error, message) = refused(&source);

        assert_names(&message, "monster `shade` (entry 3)", "id");
        assert!(message.contains("entry 1"), "first use missing: {message}");
        assert!(matches!(
            error,
            ContentError::InvalidField {
                problem: FieldProblem::Duplicate { first_entry: 1, .. },
                ..
            }
        ));
    }

    #[test]
    fn zero_speed_is_refused() {
        let source = file(&[valid("ok"), with("frozen", "speed", "0")]);

        let (error, message) = refused(&source);

        assert_names(&message, "monster `frozen` (entry 2)", "speed");
        assert!(matches!(
            error,
            ContentError::InvalidField {
                field: "speed",
                problem: FieldProblem::OutOfRange { value: 0, .. },
                ..
            }
        ));
    }

    #[test]
    fn negative_or_excessive_speed_is_refused() {
        let (_, message) = refused(&file(&[with("backward", "speed", "-100")]));
        assert_names(&message, "monster `backward`", "speed");

        let too_fast = (MAX_SPEED + 1).to_string();
        let (_, message) = refused(&file(&[with("blur", "speed", &too_fast)]));
        assert_names(&message, "monster `blur`", "speed");
        assert!(message.contains(&MAX_SPEED.to_string()), "{message}");

        let fastest = MAX_SPEED.to_string();
        assert!(Content::parse(&file(&[with("blur", "speed", &fastest)])).is_ok());
    }

    #[test]
    fn zero_or_negative_hit_points_are_refused() {
        for hp in ["0", "-3"] {
            let (error, message) = refused(&file(&[with("husk", "hp", hp)]));

            assert_names(&message, "monster `husk` (entry 1)", "hp");
            assert!(message.contains(hp), "value {hp} missing: {message}");
            assert!(matches!(
                error,
                ContentError::InvalidField { field: "hp", .. }
            ));
        }
    }

    #[test]
    fn combat_values_out_of_bounds_are_refused() {
        let cases = [
            ("hp", MAX_HP + 1),
            ("attack", -1),
            ("attack", MAX_ATTACK + 1),
            ("defense", -1),
            ("defense", MAX_DEFENSE + 1),
        ];
        for (field, value) in cases {
            let (_, message) = refused(&file(&[with("brute", field, &value.to_string())]));

            assert_names(&message, "monster `brute`", field);
        }
    }

    #[test]
    fn depth_zero_is_refused() {
        let source = file(&[valid("ok"), with("surface", "min_depth", "0")]);

        let (error, message) = refused(&source);

        assert_names(&message, "monster `surface` (entry 2)", "min_depth");
        assert!(matches!(
            error,
            ContentError::InvalidField {
                problem: FieldProblem::TooLow { value: 0, min: 1 },
                ..
            }
        ));
    }

    #[test]
    fn first_floor_without_monster_is_refused() {
        let source = file(&[
            with("deep", "min_depth", "4"),
            with("shallowest", "min_depth", "2"),
            with("also_shallow", "min_depth", "2"),
        ]);

        let (error, message) = refused(&source);

        assert_names(&message, "monster `shallowest` (entry 2)", "min_depth");
        assert_eq!(
            error,
            ContentError::NoMonsterAtFirstDepth {
                file: MONSTERS_FILE,
                first_depth: FIRST_DEPTH,
                entry: "monster `shallowest` (entry 2)".to_owned(),
                min_depth: 2,
            }
        );
    }

    #[test]
    fn empty_file_is_refused() {
        let (error, message) = refused("[]");

        assert_eq!(
            error,
            ContentError::NoMonster {
                file: MONSTERS_FILE
            }
        );
        assert!(message.contains(MONSTERS_FILE), "{message}");
    }

    #[test]
    fn missing_field_is_refused_with_its_entry() {
        let source = file(&[valid("ok"), without("hollow", "hp")]);

        let (error, message) = refused(&source);

        assert_names(&message, "monster `hollow` (entry 2)", "hp");
        assert!(matches!(
            error,
            ContentError::InvalidField {
                field: "hp",
                problem: FieldProblem::Missing,
                ..
            }
        ));
    }

    #[test]
    fn missing_identifier_is_refused_with_the_entry_position() {
        let source = file(&[valid("ok"), without("ignored", "id")]);

        let (_, message) = refused(&source);

        assert_names(&message, "monster entry 2", "id");
        assert!(message.contains("missing"), "{message}");
    }

    #[test]
    fn malformed_identifier_or_blank_name_is_refused() {
        for id in ["", "Shade", "9lives", "two words", "dash-ed"] {
            let (_, message) = refused(&file(&[with("ignored", "id", &format!("\"{id}\""))]));
            assert_names(&message, "monster entry 1", "id");
        }

        let (_, message) = refused(&file(&[with("nameless", "name", "\"  \"")]));
        assert_names(&message, "monster `nameless` (entry 1)", "name");
    }

    /// Asserts that `error` is a syntax error on `line`, and returns its
    /// message.
    fn syntax_error_on(error: &ContentError, line: usize) -> String {
        assert!(
            matches!(error, ContentError::Syntax { line: l, .. } if *l == line),
            "{error:?}"
        );
        let message = error.to_string();
        assert!(
            message.starts_with(&format!("{MONSTERS_FILE}, line {line},")),
            "{message}"
        );
        message
    }

    #[test]
    fn value_of_the_wrong_type_reports_line_entry_and_field() {
        // Line 1 is `[` and each entry takes 11 lines, so the second entry
        // opens on line 13 and its fifth field, `speed`, is on line 18.
        let source = file(&[valid("ok"), with("swift", "speed", "\"fast\"")]);
        assert!(source.lines().nth(17).unwrap().contains("speed: \"fast\""));

        let (error, _) = refused(&source);

        let message = syntax_error_on(&error, 18);
        assert_names(&message, "monster `swift` (entry 2), field", "speed");
    }

    #[test]
    fn broken_syntax_reports_line_entry_and_field() {
        let source = file(&[with("broken", "speed", "1OO")]);
        assert!(source.lines().nth(6).unwrap().contains("speed: 1OO"));

        let (error, _) = refused(&source);

        let message = syntax_error_on(&error, 7);
        assert_names(&message, "monster `broken` (entry 1), after field", "speed");
    }

    #[test]
    fn syntax_error_before_the_identifier_names_the_entry_position() {
        let mut fields = fields("late_id");
        fields.rotate_left(1);
        // `id` now comes last, so the parser meets `family` (line 15) first.
        let source = file(&[valid("ok"), write_entry(&fields).replace("Vermin", "3")]);

        let (error, _) = refused(&source);

        let message = syntax_error_on(&error, 15);
        assert_names(&message, "monster entry 2, field", "family");
    }

    #[test]
    fn unknown_field_reports_its_name_and_entry() {
        let source = file(&[valid("ok")]).replace("hp: 5", "hpp: 5");

        let (error, _) = refused(&source);

        let message = syntax_error_on(&error, 8);
        assert_names(&message, "monster `ok` (entry 1)", "hpp");
    }

    #[test]
    fn repeated_field_is_refused() {
        let source = file(&[valid("twice")]).replace("hp: 5,", "hp: 5,\n        hp: 6,");

        let (error, _) = refused(&source);

        let message = syntax_error_on(&error, 9);
        assert_names(&message, "monster `twice` (entry 1), field", "hp");
    }

    #[test]
    fn unknown_family_reports_its_name_and_entry() {
        let (error, _) = refused(&file(&[with("odd", "family", "Goblin")]));

        let message = syntax_error_on(&error, 5);
        assert_names(&message, "monster `odd` (entry 1), field", "family");
        assert!(message.contains("Goblin"), "{message}");
    }

    #[test]
    fn error_outside_the_entries_says_so() {
        let (error, _) = refused("Monster(id: \"lonely\")");
        let message = syntax_error_on(&error, 1);
        assert!(message.contains("before the first monster"), "{message}");

        let (error, _) = refused(&format!("{} trailing", file(&[valid("ok")])));
        let message = syntax_error_on(&error, 13);
        assert!(message.contains("after the last monster"), "{message}");
    }
}
