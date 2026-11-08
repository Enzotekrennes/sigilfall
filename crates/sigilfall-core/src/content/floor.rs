//! Floor kinds: how each floor is laid out, floor by floor, and the rules a
//! floor file must follow.
//!
//! Exposes [`FloorKind`], the ways the generation can lay out a floor, and
//! to the `content` module [`FloorTable`], the validated rules with the
//! weight of a kind on a floor, and [`validate`], which turns the raw rules
//! read from the floor file into a table or reports the first fault with
//! the rule and field in cause.
//!
//! A rule gives one kind a weight on a band of floors: on a floor, the kind
//! is drawn with a chance proportional to its weight among the kinds
//! covering that floor. Floors deeper than [`MAX_DEPTH`] use the weights of
//! [`MAX_DEPTH`]. Drawing the kind and laying the floor out are done by the
//! generation, not here.

use serde::Deserialize;

use super::check::EntryCheck;
use super::entry_file::RawEntry;
use super::error::{ContentError, FieldProblem};
use super::floor_file::FloorEntry;
use super::{FLOORS_FILE, MAX_DEPTH, MAX_WEIGHT};
use crate::generation::FIRST_DEPTH;

/// How a floor is laid out; written by its name in the floor file
/// (`kind: Caves`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
pub enum FloorKind {
    /// Rectangular rooms joined by corridors, cut by binary space
    /// partitioning.
    Rooms,
    /// One natural cave grown by a cellular automaton.
    Caves,
}

impl FloorKind {
    /// Every kind, in declaration order.
    pub const ALL: [Self; 2] = [Self::Rooms, Self::Caves];

    /// Returns the name of the kind as written in the floor file, such as
    /// `Caves`; error messages name rules by it.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Rooms => "Rooms",
            Self::Caves => "Caves",
        }
    }
}

/// One validated rule: `kind` has `weight` on every floor from `from_depth`
/// to `to_depth` included.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FloorRule {
    /// The kind weighed.
    kind: FloorKind,
    /// First floor covered, from `generation::FIRST_DEPTH` to [`MAX_DEPTH`].
    from_depth: u32,
    /// Last floor covered, from `from_depth` to [`MAX_DEPTH`].
    to_depth: u32,
    /// Relative chance of the kind, from 1 to [`MAX_WEIGHT`].
    weight: u32,
}

impl FloorRule {
    /// Returns whether the rule covers floor `depth`.
    const fn covers(&self, depth: u32) -> bool {
        self.from_depth <= depth && depth <= self.to_depth
    }

    /// Returns whether the rule shares at least one floor with `other`.
    const fn overlaps(&self, other: &Self) -> bool {
        self.from_depth <= other.to_depth && other.from_depth <= self.to_depth
    }
}

/// Validated floor rules. Two rules of the same kind never cover a common
/// floor, and every floor from [`FIRST_DEPTH`] to [`MAX_DEPTH`] has at
/// least one kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FloorTable {
    /// The rules, in file order.
    rules: Vec<FloorRule>,
}

impl FloorTable {
    /// Returns the kinds a floor `depth` may have, in file order of their
    /// rules, each with its weight there. Floors deeper than [`MAX_DEPTH`]
    /// count as [`MAX_DEPTH`], and floors shallower than [`FIRST_DEPTH`] as
    /// [`FIRST_DEPTH`], so the list is never empty.
    pub(super) fn weights(&self, depth: u32) -> impl Iterator<Item = (FloorKind, u32)> {
        let depth = depth.clamp(FIRST_DEPTH, MAX_DEPTH);
        self.rules
            .iter()
            .filter(move |rule| rule.covers(depth))
            .map(|rule| (rule.kind, rule.weight))
    }
}

/// Turns the rules of the floor file into a table.
///
/// Each rule is checked in turn (kind given, floors within [`FIRST_DEPTH`]
/// and [`MAX_DEPTH`], `to_depth` not above `from_depth`, weight within
/// bounds), then against the rules before it (no common floor with a rule
/// of the same kind). Then every floor up to [`MAX_DEPTH`] must have a
/// kind.
///
/// # Errors
///
/// Returns the first fault found: [`ContentError::InvalidField`] naming the
/// rule (by the kind it weighs and its position) and the field, with
/// [`FieldProblem::Overlap`] for two rules of the same kind sharing a
/// floor; or [`ContentError::NoFloorKindAtDepth`] naming the shallowest
/// floor no rule covers.
pub(super) fn validate(entries: Vec<FloorEntry>) -> Result<FloorTable, ContentError> {
    let mut rules: Vec<FloorRule> = Vec::with_capacity(entries.len());
    for (index, entry) in entries.into_iter().enumerate() {
        let position = index + 1;
        let rule = validate_entry(position, entry)?;
        check_no_overlap(position, &rule, &rules)?;
        rules.push(rule);
    }
    let table = FloorTable { rules };
    check_every_depth_has_a_kind(&table)?;
    Ok(table)
}

/// Validates the rule at `position` (counted from 1) on its own, without
/// looking at the other rules.
fn validate_entry(position: usize, entry: FloorEntry) -> Result<FloorRule, ContentError> {
    let check = EntryCheck::new(&FloorEntry::KIND, position, entry.kind.map(FloorKind::name));
    let kind = check.required(entry.kind, "kind")?;
    let from_depth = check.depth(entry.from_depth, "from_depth")?;
    let to_depth = check.depth(entry.to_depth, "to_depth")?;
    if to_depth < from_depth {
        return Err(check.fail(
            "to_depth",
            FieldProblem::OutOfRange {
                value: to_depth.into(),
                min: from_depth.into(),
                max: MAX_DEPTH.into(),
            },
        ));
    }
    let weight = check.in_range(entry.weight, "weight", 1, MAX_WEIGHT)?;
    Ok(FloorRule {
        kind,
        from_depth,
        to_depth,
        weight: weight.unsigned_abs(),
    })
}

/// Refuses `rule`, the rule at `position`, if it shares a floor with one of
/// `earlier`, the rules before it, of the same kind; the first such rule is
/// named.
fn check_no_overlap(
    position: usize,
    rule: &FloorRule,
    earlier: &[FloorRule],
) -> Result<(), ContentError> {
    let entry_kind = FloorEntry::KIND;
    let Some(index) = earlier
        .iter()
        .position(|other| other.kind == rule.kind && other.overlaps(rule))
    else {
        return Ok(());
    };
    let name = rule.kind.name();
    Err(EntryCheck::new(&entry_kind, position, Some(name)).fail(
        "from_depth",
        FieldProblem::Overlap {
            from_depth: rule.from_depth,
            to_depth: rule.to_depth,
            other: entry_kind.label(index + 1, Some(name)),
        },
    ))
}

/// Refuses a table that leaves a floor from [`FIRST_DEPTH`] to [`MAX_DEPTH`]
/// without any kind, naming the shallowest such floor.
fn check_every_depth_has_a_kind(table: &FloorTable) -> Result<(), ContentError> {
    let uncovered =
        (FIRST_DEPTH..=MAX_DEPTH).find(|&depth| !table.rules.iter().any(|rule| rule.covers(depth)));
    match uncovered {
        Some(depth) => Err(ContentError::NoFloorKindAtDepth {
            file: FLOORS_FILE,
            depth,
            first_depth: FIRST_DEPTH,
            max_depth: MAX_DEPTH,
        }),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::Content;
    use crate::content::entry_file;

    /// The floor file shipped with the game, one rule per line.
    const FLOORS: &str = include_str!("../../content/floors.ron");

    /// Number of rules of the shipped floor file.
    fn shipped_rules() -> usize {
        FLOORS
            .lines()
            .filter(|line| line.trim_start().starts_with("Floor("))
            .count()
    }

    /// The shipped floor file with `rules` added at its end.
    fn with_rules(rules: &[&str]) -> String {
        let end = FLOORS.rfind(']').expect("a floor file is a list");
        let added: String = rules.iter().map(|rule| format!("    {rule},\n")).collect();
        format!("{}{added}]\n", &FLOORS[..end])
    }

    /// Reads and validates `floors`, the text of a floor file.
    fn parse(floors: &str) -> Result<FloorTable, ContentError> {
        validate(entry_file::read(floors)?)
    }

    /// Parses `floors`, expecting it to be refused, and returns the error.
    fn refused(floors: &str) -> ContentError {
        parse(floors).expect_err("the floor file should be refused")
    }

    /// How errors name the rule at `position` (counted from 1) weighing
    /// `kind`.
    fn rule_label(kind: FloorKind, position: usize) -> String {
        format!("floor `{}` (entry {position})", kind.name())
    }

    /// Asserts that `error` is about `field` of the rule `entry` of the
    /// floor file, with `problem`, and that its message names the file, the
    /// rule and the field.
    fn assert_field(error: &ContentError, entry: &str, field: &str, problem: &FieldProblem) {
        let ContentError::InvalidField {
            file,
            entry: found_entry,
            field: found_field,
            problem: found_problem,
        } = error
        else {
            panic!("not an error of a field: {error:?}");
        };
        assert_eq!(
            (*file, found_entry.as_str(), *found_field, found_problem),
            (FLOORS_FILE, entry, field, problem)
        );
        let message = error.to_string();
        assert!(message.contains(FLOORS_FILE), "{message}");
        assert!(message.contains(entry), "{message}");
        assert!(message.contains(&format!("`{field}`")), "{message}");
    }

    /// The kinds of floor `depth` in `content`, with their weights.
    fn kinds(content: &Content, depth: u32) -> Vec<(FloorKind, u32)> {
        content.floor_kind_weights(depth).collect()
    }

    #[test]
    fn the_shipped_floor_kinds_cover_every_floor_without_gap() {
        let table = parse(FLOORS).expect("the shipped floor file is valid");

        for depth in FIRST_DEPTH..=MAX_DEPTH {
            let weights: Vec<(FloorKind, u32)> = table.weights(depth).collect();
            assert!(!weights.is_empty(), "floor {depth} has no kind");
            for (kind, weight) in weights {
                assert!(
                    (1..=MAX_WEIGHT.unsigned_abs()).contains(&weight),
                    "floor {depth}: {kind:?} weighs {weight}"
                );
            }
        }
    }

    #[test]
    fn the_shipped_floors_keep_rooms_on_top_and_caves_at_the_bottom() {
        let content = Content::load().expect("the shipped content is valid");

        // The first floors stay rooms, so the first floor of every seed is
        // the one it was before caves existed.
        assert_eq!(kinds(&content, FIRST_DEPTH), [(FloorKind::Rooms, 1)]);
        assert_eq!(kinds(&content, 2), [(FloorKind::Rooms, 1)]);
        // In between, both kinds, caves growing more common.
        assert_eq!(
            kinds(&content, 3),
            [(FloorKind::Rooms, 2), (FloorKind::Caves, 1)]
        );
        assert_eq!(
            kinds(&content, 9),
            [(FloorKind::Rooms, 1), (FloorKind::Caves, 2)]
        );
        // The last floor written for is a cave, and so is every deeper one.
        assert_eq!(kinds(&content, MAX_DEPTH), [(FloorKind::Caves, 1)]);
        assert_eq!(kinds(&content, MAX_DEPTH + 1), kinds(&content, MAX_DEPTH));
        assert_eq!(kinds(&content, u32::MAX), kinds(&content, MAX_DEPTH));
    }

    #[test]
    fn content_parsed_without_a_floor_file_uses_the_shipped_kinds() {
        let shipped = Content::load().expect("the shipped content is valid");
        let monsters = include_str!("../../content/monsters.ron");
        let content = Content::parse(monsters).expect("the shipped monsters");

        for depth in FIRST_DEPTH..=MAX_DEPTH + 1 {
            assert_eq!(kinds(&content, depth), kinds(&shipped, depth), "{depth}");
        }
    }

    #[test]
    fn a_floor_without_any_kind_is_refused() {
        let floors = r"[
            Floor(kind: Rooms, from_depth: 1, to_depth: 3, weight: 1),
            Floor(kind: Caves, from_depth: 5, to_depth: 10, weight: 1),
        ]";

        let error = refused(floors);

        assert_eq!(
            error,
            ContentError::NoFloorKindAtDepth {
                file: FLOORS_FILE,
                depth: 4,
                first_depth: FIRST_DEPTH,
                max_depth: MAX_DEPTH,
            }
        );
        let message = error.to_string();
        assert!(message.contains(FLOORS_FILE), "{message}");
        assert!(message.contains("floor 4"), "{message}");
        assert!(parse(&floors.replace("to_depth: 3", "to_depth: 4")).is_ok());
    }

    #[test]
    fn a_floor_file_short_of_the_last_floor_is_refused() {
        let floors = "[Floor(kind: Rooms, from_depth: 1, to_depth: 9, weight: 1)]";

        assert!(matches!(
            refused(floors),
            ContentError::NoFloorKindAtDepth { depth: 10, .. }
        ));
        assert!(matches!(
            refused("[]"),
            ContentError::NoFloorKindAtDepth { depth: 1, .. }
        ));
    }

    #[test]
    fn two_rules_of_the_same_kind_on_a_common_floor_are_refused() {
        let rule = "Floor(kind: Caves, from_depth: 6, to_depth: 7, weight: 1)";

        let error = refused(&with_rules(&[rule]));

        // The shipped caves of floors 3 to 6 come in third.
        let problem = FieldProblem::Overlap {
            from_depth: 6,
            to_depth: 7,
            other: rule_label(FloorKind::Caves, 3),
        };
        assert_field(
            &error,
            &rule_label(FloorKind::Caves, shipped_rules() + 1),
            "from_depth",
            &problem,
        );
        assert!(error.to_string().contains("overlap"), "{error}");
    }

    #[test]
    fn floors_and_weights_out_of_bounds_are_refused() {
        let position = shipped_rules() + 1;
        let label = rule_label(FloorKind::Rooms, position);
        let cases = [
            (
                0,
                1,
                1,
                "from_depth",
                FieldProblem::TooLow { value: 0, min: 1 },
            ),
            (
                11,
                11,
                1,
                "from_depth",
                FieldProblem::TooHigh { value: 11, max: 10 },
            ),
            (
                10,
                11,
                1,
                "to_depth",
                FieldProblem::TooHigh { value: 11, max: 10 },
            ),
            (
                10,
                9,
                1,
                "to_depth",
                FieldProblem::OutOfRange {
                    value: 9,
                    min: 10,
                    max: 10,
                },
            ),
            (
                10,
                10,
                0,
                "weight",
                FieldProblem::OutOfRange {
                    value: 0,
                    min: 1,
                    max: MAX_WEIGHT.into(),
                },
            ),
            (
                10,
                10,
                MAX_WEIGHT + 1,
                "weight",
                FieldProblem::OutOfRange {
                    value: (MAX_WEIGHT + 1).into(),
                    min: 1,
                    max: MAX_WEIGHT.into(),
                },
            ),
        ];
        for (from, to, weight, field, problem) in cases {
            let rule =
                format!("Floor(kind: Rooms, from_depth: {from}, to_depth: {to}, weight: {weight})");

            assert_field(&refused(&with_rules(&[&rule])), &label, field, &problem);
        }
        // Rooms on the last floor, beside the shipped caves, are accepted.
        let rule = "Floor(kind: Rooms, from_depth: 10, to_depth: 10, weight: 1)";
        assert!(parse(&with_rules(&[rule])).is_ok());
    }

    #[test]
    fn a_rule_missing_a_field_is_refused_with_its_entry() {
        let added = shipped_rules() + 1;
        let rule = "Floor(kind: Caves, from_depth: 10, to_depth: 10)";
        assert_field(
            &refused(&with_rules(&[rule])),
            &rule_label(FloorKind::Caves, added),
            "weight",
            &FieldProblem::Missing,
        );

        let rule = "Floor(from_depth: 10, to_depth: 10, weight: 1)";
        assert_field(
            &refused(&with_rules(&[rule])),
            &format!("floor entry {added}"),
            "kind",
            &FieldProblem::Missing,
        );
    }

    #[test]
    fn an_unknown_kind_is_refused_naming_the_rule_and_the_field() {
        let rule = "Floor(kind: Mazes, from_depth: 4, to_depth: 5, weight: 1)";

        let error = refused(&with_rules(&[rule]));

        assert!(
            matches!(
                error,
                ContentError::Syntax {
                    file: FLOORS_FILE,
                    ..
                }
            ),
            "{error:?}"
        );
        let message = error.to_string();
        assert!(
            message.contains(&format!("floor entry {}", shipped_rules() + 1)),
            "{message}"
        );
        assert!(message.contains("field `kind`"), "{message}");
        assert!(message.contains("Mazes"), "{message}");
    }

    #[test]
    fn a_floor_file_is_checked_by_the_content_loader() {
        let monsters = include_str!("../../content/monsters.ron");
        let sigils = include_str!("../../content/sigils.ron");
        let items = include_str!("../../content/items.ron");
        let spawns = include_str!("../../content/spawns.ron");
        let only_caves = "[Floor(kind: Caves, from_depth: 1, to_depth: 10, weight: 1)]";

        let content = Content::parse_with_floors(monsters, sigils, items, spawns, only_caves)
            .expect("a floor file of caves only is valid");
        assert_eq!(kinds(&content, FIRST_DEPTH), [(FloorKind::Caves, 1)]);

        let error = Content::parse_with_floors(monsters, sigils, items, spawns, "[]")
            .expect_err("a floor file without rules");
        assert!(matches!(error, ContentError::NoFloorKindAtDepth { .. }));
        assert_eq!(
            Content::parse_with_floors(monsters, sigils, items, spawns, FLOORS),
            Content::load()
        );
    }
}
