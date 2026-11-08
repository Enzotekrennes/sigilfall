//! Spawn tables: how often each creature and each item is drawn, floor by
//! floor, and the rules a spawn file must follow.
//!
//! Exposes to the `content` module [`SpawnTable`], the validated rules with
//! the weight of an entry on a floor, [`validate`], which turns the raw
//! rules read from the spawn file into a table or reports the first fault
//! with the rule and field in cause, and [`MAX_WEIGHT`]. A table built
//! without a spawn file ([`SpawnTable::uniform`]) gives every entry the same
//! weight from its `min_depth` on, the behaviour of content parsed for
//! tests.
//!
//! A rule places one creature or item on a band of floors with a weight:
//! on a floor, each entry is drawn with a chance proportional to its weight
//! among the entries of its kind covering that floor. Floors deeper than
//! [`MAX_DEPTH`] use the weights of [`MAX_DEPTH`]. Drawing is done by the
//! generation, not here.

use super::check::EntryCheck;
use super::entry_file::RawEntry;
use super::error::{ContentError, FieldProblem};
use super::item_file::ItemEntry;
use super::monster_file::MonsterEntry;
use super::spawn_file::{SpawnEntry, TargetEntry};
use super::{ITEMS_FILE, ItemDef, MAX_DEPTH, MONSTERS_FILE, MonsterDef, SPAWNS_FILE};
use crate::generation::FIRST_DEPTH;

/// Highest accepted weight of a rule.
///
/// Weights are relative within a floor, so two digits already express any
/// useful ratio (1 in 99), and the sum over every entry stays far from
/// overflowing; a larger value is a typo.
pub const MAX_WEIGHT: i32 = 99;

/// What a rule places: an index in the monster or item list of the content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    /// The monster at this index of the monster list.
    Monster(usize),
    /// The item at this index of the item list.
    Item(usize),
}

/// One validated rule: `target` is drawn with `weight` on every floor from
/// `from_depth` to `to_depth` included.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SpawnRule {
    /// The creature or item placed.
    target: Target,
    /// First floor covered, from the target's `min_depth` to [`MAX_DEPTH`].
    from_depth: u32,
    /// Last floor covered, from `from_depth` to [`MAX_DEPTH`].
    to_depth: u32,
    /// Relative chance of being drawn, from 1 to [`MAX_WEIGHT`].
    weight: u32,
}

impl SpawnRule {
    /// Returns whether the rule covers floor `depth`.
    const fn covers(&self, depth: u32) -> bool {
        self.from_depth <= depth && depth <= self.to_depth
    }

    /// Returns whether the rule shares at least one floor with `other`.
    const fn overlaps(&self, other: &Self) -> bool {
        self.from_depth <= other.to_depth && other.from_depth <= self.to_depth
    }
}

/// Validated spawn rules. Two rules of the same target never cover a common
/// floor, every monster and item has at least one rule, and every floor
/// from [`FIRST_DEPTH`] to [`MAX_DEPTH`] has at least one monster.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SpawnTable {
    /// The rules, in file order.
    rules: Vec<SpawnRule>,
}

impl SpawnTable {
    /// Returns the table of content parsed without a spawn file: one rule of
    /// weight 1 per monster and per item, from its `min_depth` to
    /// [`MAX_DEPTH`], so every entry allowed on a floor is equally likely.
    ///
    /// The monsters and items are already validated: their `min_depth` lies
    /// within [`FIRST_DEPTH`] and [`MAX_DEPTH`], and one monster covers the
    /// first floor, hence every floor.
    pub(super) fn uniform(monsters: &[MonsterDef], items: &[ItemDef]) -> Self {
        let rule = |target, from_depth| SpawnRule {
            target,
            from_depth,
            to_depth: MAX_DEPTH,
            weight: 1,
        };
        let monsters = monsters
            .iter()
            .enumerate()
            .map(|(index, monster)| rule(Target::Monster(index), monster.min_depth()));
        let items = items
            .iter()
            .enumerate()
            .map(|(index, item)| rule(Target::Item(index), item.min_depth()));
        Self {
            rules: monsters.chain(items).collect(),
        }
    }

    /// Returns the weight of the monster at `index` of the monster list on
    /// floor `depth`: 0 when no rule places it there. Floors deeper than
    /// [`MAX_DEPTH`] count as [`MAX_DEPTH`].
    pub(super) fn monster_weight(&self, index: usize, depth: u32) -> u32 {
        self.weight(Target::Monster(index), depth)
    }

    /// Returns the weight of the item at `index` of the item list on floor
    /// `depth`, like [`SpawnTable::monster_weight`].
    pub(super) fn item_weight(&self, index: usize, depth: u32) -> u32 {
        self.weight(Target::Item(index), depth)
    }

    /// Returns the weight of `target` on floor `depth`, deeper floors
    /// counting as [`MAX_DEPTH`]; 0 when no rule covers it.
    fn weight(&self, target: Target, depth: u32) -> u32 {
        let depth = depth.min(MAX_DEPTH);
        self.rules
            .iter()
            .find(|rule| rule.target == target && rule.covers(depth))
            .map_or(0, |rule| rule.weight)
    }
}

/// Turns the rules of the spawn file into a table, checking them against
/// `monsters` and `items`, the validated entries they refer to.
///
/// Each rule is checked in turn (target defined, floors within
/// [`FIRST_DEPTH`] and [`MAX_DEPTH`], not above the target's `min_depth`,
/// `to_depth` not above `from_depth`, weight within bounds), then against the
/// rules before it (no common floor with a rule of the same target). Then
/// every monster and item must have a rule, and every floor up to
/// [`MAX_DEPTH`] at least one monster.
///
/// # Errors
///
/// Returns the first fault found: [`ContentError::InvalidField`] naming the
/// rule (by the identifier it places and its position) and the field, with
/// [`FieldProblem::UnknownReference`] for an undefined target,
/// [`FieldProblem::ShallowerThanReference`] for a band starting above the
/// target's `min_depth` and [`FieldProblem::Overlap`] for two rules of the
/// same target sharing a floor; [`ContentError::NoSpawnRule`] naming an
/// entry no rule places; or [`ContentError::NoMonsterAtDepth`].
pub(super) fn validate(
    entries: Vec<SpawnEntry>,
    monsters: &[MonsterDef],
    items: &[ItemDef],
) -> Result<SpawnTable, ContentError> {
    let mut rules: Vec<SpawnRule> = Vec::with_capacity(entries.len());
    for (index, entry) in entries.into_iter().enumerate() {
        let position = index + 1;
        let rule = validate_entry(position, entry, monsters, items)?;
        check_no_overlap(position, &rule, &rules, monsters, items)?;
        rules.push(rule);
    }
    let table = SpawnTable { rules };
    check_every_entry_placed(&table, monsters, items)?;
    check_every_depth_has_monsters(&table, monsters)?;
    Ok(table)
}

/// Validates the rule at `position` (counted from 1) on its own, without
/// looking at the other rules.
fn validate_entry(
    position: usize,
    entry: SpawnEntry,
    monsters: &[MonsterDef],
    items: &[ItemDef],
) -> Result<SpawnRule, ContentError> {
    let check = EntryCheck::new(
        &SpawnEntry::KIND,
        position,
        entry.of.as_ref().map(TargetEntry::id),
    );
    let (target, reference, min_depth) = target(&check, entry.of, monsters, items)?;
    let from_depth = check.depth(entry.from_depth, "from_depth")?;
    if from_depth < min_depth {
        return Err(check.fail(
            "from_depth",
            FieldProblem::ShallowerThanReference {
                value: from_depth,
                reference,
                min_depth,
            },
        ));
    }
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
    Ok(SpawnRule {
        target,
        from_depth,
        to_depth,
        weight: weight.unsigned_abs(),
    })
}

/// Returns what `entry`, the field `of` of the rule `check` designates,
/// places, how errors name it (such as "monster `acolyte`") and its
/// `min_depth`, refusing a missing field or an identifier the monster or
/// item list does not define.
fn target(
    check: &EntryCheck,
    entry: Option<TargetEntry>,
    monsters: &[MonsterDef],
    items: &[ItemDef],
) -> Result<(Target, String, u32), ContentError> {
    const FIELD: &str = "of";
    let unknown = |noun, id: &str, file| {
        check.fail(
            FIELD,
            FieldProblem::UnknownReference {
                noun,
                id: id.to_owned(),
                file,
            },
        )
    };
    match check.required(entry, FIELD)? {
        TargetEntry::Monster(id) => monsters
            .iter()
            .position(|monster| monster.id() == id)
            .map(|index| {
                let reference = format!("monster `{id}`");
                (
                    Target::Monster(index),
                    reference,
                    monsters[index].min_depth(),
                )
            })
            .ok_or_else(|| unknown("monster", &id, MONSTERS_FILE)),
        TargetEntry::Item(id) => items
            .iter()
            .position(|item| item.id() == id)
            .map(|index| {
                let reference = format!("item `{id}`");
                (Target::Item(index), reference, items[index].min_depth())
            })
            .ok_or_else(|| unknown("item", &id, ITEMS_FILE)),
    }
}

/// Refuses `rule`, the rule at `position`, if it shares a floor with one of
/// `earlier`, the rules before it, of the same target; the first such rule
/// is named.
fn check_no_overlap(
    position: usize,
    rule: &SpawnRule,
    earlier: &[SpawnRule],
    monsters: &[MonsterDef],
    items: &[ItemDef],
) -> Result<(), ContentError> {
    let kind = SpawnEntry::KIND;
    let Some(index) = earlier
        .iter()
        .position(|other| other.target == rule.target && other.overlaps(rule))
    else {
        return Ok(());
    };
    let id = target_id(rule.target, monsters, items);
    Err(EntryCheck::new(&kind, position, Some(id)).fail(
        "from_depth",
        FieldProblem::Overlap {
            from_depth: rule.from_depth,
            to_depth: rule.to_depth,
            other: kind.label(index + 1, Some(id)),
        },
    ))
}

/// Returns the identifier of the monster or item `target` designates.
fn target_id<'a>(target: Target, monsters: &'a [MonsterDef], items: &'a [ItemDef]) -> &'a str {
    match target {
        Target::Monster(index) => monsters[index].id(),
        Target::Item(index) => items[index].id(),
    }
}

/// Refuses a table in which a monster or an item has no rule, naming the
/// first such entry, monsters first.
fn check_every_entry_placed(
    table: &SpawnTable,
    monsters: &[MonsterDef],
    items: &[ItemDef],
) -> Result<(), ContentError> {
    let placed = |target| table.rules.iter().any(|rule| rule.target == target);
    let unplaced_monster = (0..monsters.len())
        .find(|&index| !placed(Target::Monster(index)))
        .map(|index| MonsterEntry::KIND.label(index + 1, Some(monsters[index].id())));
    let unplaced_item = || {
        (0..items.len())
            .find(|&index| !placed(Target::Item(index)))
            .map(|index| ItemEntry::KIND.label(index + 1, Some(items[index].id())))
    };
    match unplaced_monster.or_else(unplaced_item) {
        Some(entry) => Err(ContentError::NoSpawnRule {
            file: SPAWNS_FILE,
            entry,
        }),
        None => Ok(()),
    }
}

/// Refuses a table that leaves a floor from [`FIRST_DEPTH`] to [`MAX_DEPTH`]
/// without any monster, naming the shallowest such floor.
fn check_every_depth_has_monsters(
    table: &SpawnTable,
    monsters: &[MonsterDef],
) -> Result<(), ContentError> {
    let empty = (FIRST_DEPTH..=MAX_DEPTH)
        .find(|&depth| (0..monsters.len()).all(|index| table.monster_weight(index, depth) == 0));
    match empty {
        Some(depth) => Err(ContentError::NoMonsterAtDepth {
            file: SPAWNS_FILE,
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

    /// The monster file shipped with the game.
    const MONSTERS: &str = include_str!("../../content/monsters.ron");

    /// The sigil file shipped with the game.
    const SIGILS: &str = include_str!("../../content/sigils.ron");

    /// The item file shipped with the game.
    const ITEMS: &str = include_str!("../../content/items.ron");

    /// The spawn file shipped with the game, one rule per line.
    const SPAWNS: &str = include_str!("../../content/spawns.ron");

    /// Number of rules of the shipped spawn file.
    fn shipped_rules() -> usize {
        SPAWNS
            .lines()
            .filter(|line| line.trim_start().starts_with("Spawn("))
            .count()
    }

    /// Adds `rules` at the end of the spawn file `spawns`, after its own.
    fn append(spawns: &str, rules: &[&str]) -> String {
        let end = spawns.rfind(']').expect("a spawn file is a list");
        let added: String = rules.iter().map(|rule| format!("    {rule},\n")).collect();
        format!("{}{added}]\n", &spawns[..end])
    }

    /// The shipped spawn file with `rules` added at its end.
    fn with_rules(rules: &[&str]) -> String {
        append(SPAWNS, rules)
    }

    /// The shipped spawn file without the rules whose line contains `text`.
    fn without_rules(text: &str) -> String {
        SPAWNS
            .lines()
            .filter(|line| !line.contains(text))
            .map(|line| format!("{line}\n"))
            .collect()
    }

    /// Parses the shipped content with `spawns` as the spawn file.
    fn parse(spawns: &str) -> Result<Content, ContentError> {
        Content::parse_with_spawns(MONSTERS, SIGILS, ITEMS, spawns)
    }

    /// Parses `spawns`, expecting it to be refused, and returns the error
    /// and its message.
    fn refused(spawns: &str) -> (ContentError, String) {
        let error = parse(spawns).expect_err("the spawn file should be refused");
        let message = error.to_string();
        (error, message)
    }

    /// How errors name the rule at `position` (counted from 1) placing
    /// `id`.
    fn rule_label(id: &str, position: usize) -> String {
        format!("spawn `{id}` (entry {position})")
    }

    /// Asserts that `error` is about `field` of the rule `entry` of the
    /// spawn file, with `problem`, and that its message names the file, the
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
            (SPAWNS_FILE, entry, field, problem)
        );
        let message = error.to_string();
        assert!(message.contains(SPAWNS_FILE), "{message}");
        assert!(message.contains(entry), "{message}");
        assert!(message.contains(&format!("`{field}`")), "{message}");
    }

    #[test]
    fn the_shipped_tables_are_valid_and_cover_every_floor() {
        let content = Content::load().expect("the shipped content is valid");

        for depth in FIRST_DEPTH..=MAX_DEPTH {
            assert!(content.monster_weights(depth).next().is_some(), "{depth}");
            for (monster, weight) in content.monster_weights(depth) {
                assert!(monster.can_appear_at(depth), "{} {depth}", monster.id());
                assert!((1..=MAX_WEIGHT.unsigned_abs()).contains(&weight));
            }
            for (item, _) in content.item_weights(depth) {
                assert!(item.can_appear_at(depth), "{} {depth}", item.id());
            }
        }
        // Floors deeper than the content is written for reuse the weights of
        // the deepest one.
        let weights = |depth| -> Vec<(String, u32)> {
            content
                .monster_weights(depth)
                .map(|(monster, weight)| (monster.id().to_owned(), weight))
                .collect()
        };
        assert_eq!(weights(MAX_DEPTH + 1), weights(MAX_DEPTH));
        assert_eq!(weights(u32::MAX), weights(MAX_DEPTH));
    }

    #[test]
    fn the_first_floor_draws_its_entries_with_equal_weights() {
        // Equal weights on the first floor make its draws those of a uniform
        // choice, so first floors stay as they were before spawn tables.
        let content = Content::load().expect("the shipped content is valid");

        let monsters: Vec<u32> = content
            .monster_weights(FIRST_DEPTH)
            .map(|(_, weight)| weight)
            .collect();
        let items: Vec<u32> = content
            .item_weights(FIRST_DEPTH)
            .map(|(_, weight)| weight)
            .collect();

        assert!(monsters.iter().all(|&weight| weight == 1), "{monsters:?}");
        assert!(items.iter().all(|&weight| weight == 1), "{items:?}");
    }

    #[test]
    fn content_without_a_spawn_file_draws_every_allowed_entry_equally() {
        let content = Content::parse_all(MONSTERS, SIGILS, ITEMS).expect("the shipped files");

        for depth in FIRST_DEPTH..=MAX_DEPTH + 1 {
            let monsters: Vec<(&str, u32)> = content
                .monster_weights(depth)
                .map(|(monster, weight)| (monster.id(), weight))
                .collect();
            let expected: Vec<(&str, u32)> = content
                .monsters()
                .iter()
                .filter(|monster| monster.can_appear_at(depth))
                .map(|monster| (monster.id(), 1))
                .collect();
            assert_eq!(monsters, expected, "depth {depth}");
            let items: Vec<(&str, u32)> = content
                .item_weights(depth)
                .map(|(item, weight)| (item.id(), weight))
                .collect();
            let allowed: Vec<(&str, u32)> = content
                .items()
                .iter()
                .filter(|item| item.can_appear_at(depth))
                .map(|item| (item.id(), 1))
                .collect();
            assert_eq!(items, allowed, "depth {depth}");
        }
    }

    #[test]
    fn a_rule_placing_an_unknown_monster_or_item_is_refused() {
        let added = shipped_rules() + 1;
        let rule = r#"Spawn(of: Monster("ghoul"), from_depth: 1, to_depth: 2, weight: 1)"#;
        let (error, message) = refused(&with_rules(&[rule]));
        let problem = FieldProblem::UnknownReference {
            noun: "monster",
            id: "ghoul".to_owned(),
            file: MONSTERS_FILE,
        };
        assert_field(&error, &rule_label("ghoul", added), "of", &problem);
        assert!(message.contains(MONSTERS_FILE), "{message}");

        let rule = r#"Spawn(of: Item("acolyte"), from_depth: 2, to_depth: 2, weight: 1)"#;
        let (error, _) = refused(&with_rules(&[rule]));
        let problem = FieldProblem::UnknownReference {
            noun: "item",
            id: "acolyte".to_owned(),
            file: ITEMS_FILE,
        };
        assert_field(&error, &rule_label("acolyte", added), "of", &problem);
    }

    #[test]
    fn a_band_above_the_min_depth_of_what_it_places_is_refused() {
        let spawns = append(
            &without_rules("hierophant"),
            &[r#"Spawn(of: Monster("hierophant"), from_depth: 1, to_depth: 3, weight: 1)"#],
        );
        let position = shipped_rules() - SPAWNS.matches(r#"Monster("hierophant")"#).count() + 1;

        let (error, message) = refused(&spawns);

        let problem = FieldProblem::ShallowerThanReference {
            value: 1,
            reference: "monster `hierophant`".to_owned(),
            min_depth: 4,
        };
        assert_field(
            &error,
            &rule_label("hierophant", position),
            "from_depth",
            &problem,
        );
        assert!(message.contains("`min_depth` is 4"), "{message}");
    }

    #[test]
    fn floors_out_of_bounds_are_refused() {
        let base = without_rules("Monster(\"vigil_eye\")");
        let position = shipped_rules();
        let cases = [
            (
                0,
                4,
                "from_depth",
                FieldProblem::TooLow { value: 0, min: 1 },
            ),
            (
                11,
                11,
                "from_depth",
                FieldProblem::TooHigh { value: 11, max: 10 },
            ),
            (
                9,
                11,
                "to_depth",
                FieldProblem::TooHigh { value: 11, max: 10 },
            ),
            (
                9,
                8,
                "to_depth",
                FieldProblem::OutOfRange {
                    value: 8,
                    min: 9,
                    max: 10,
                },
            ),
        ];
        for (from, to, field, problem) in cases {
            let rule = format!(
                r#"Spawn(of: Monster("vigil_eye"), from_depth: {from}, to_depth: {to}, weight: 1)"#
            );
            let (error, _) = refused(&append(&base, &[&rule]));

            assert_field(&error, &rule_label("vigil_eye", position), field, &problem);
        }
        // The deepest band accepted.
        let rule = r#"Spawn(of: Monster("vigil_eye"), from_depth: 10, to_depth: 10, weight: 1)"#;
        assert!(parse(&append(&base, &[rule])).is_ok());
    }

    #[test]
    fn weights_out_of_bounds_are_refused() {
        let base = without_rules("steadying_ink");
        let position = shipped_rules() - SPAWNS.matches("steadying_ink").count() + 1;
        for weight in [0, -1, MAX_WEIGHT + 1] {
            let rule = format!(
                r#"Spawn(of: Item("steadying_ink"), from_depth: 1, to_depth: 10, weight: {weight})"#
            );

            let (error, _) = refused(&append(&base, &[&rule]));

            let problem = FieldProblem::OutOfRange {
                value: weight.into(),
                min: 1,
                max: MAX_WEIGHT.into(),
            };
            assert_field(
                &error,
                &rule_label("steadying_ink", position),
                "weight",
                &problem,
            );
        }
    }

    #[test]
    fn two_rules_of_the_same_entry_on_a_common_floor_are_refused() {
        let rule = r#"Spawn(of: Monster("ossuary_rat"), from_depth: 3, to_depth: 5, weight: 2)"#;

        let (error, message) = refused(&with_rules(&[rule]));

        let problem = FieldProblem::Overlap {
            from_depth: 3,
            to_depth: 5,
            other: rule_label("ossuary_rat", 2),
        };
        assert_field(
            &error,
            &rule_label("ossuary_rat", shipped_rules() + 1),
            "from_depth",
            &problem,
        );
        assert!(message.contains("overlap"), "{message}");
    }

    #[test]
    fn a_rule_missing_a_field_is_refused_with_its_entry() {
        let added = shipped_rules() + 1;
        let rule = r#"Spawn(of: Monster("shade"), from_depth: 3, weight: 1)"#;
        let (error, _) = refused(&with_rules(&[rule]));
        assert_field(
            &error,
            &rule_label("shade", added),
            "to_depth",
            &FieldProblem::Missing,
        );

        let rule = "Spawn(from_depth: 2, to_depth: 3, weight: 1)";
        let (error, _) = refused(&with_rules(&[rule]));
        let entry = format!("spawn entry {added}");
        assert_field(&error, &entry, "of", &FieldProblem::Missing);
    }

    #[test]
    fn a_monster_or_an_item_without_rule_is_refused() {
        let (error, message) = refused(&without_rules("lead_homunculus"));
        assert_eq!(
            error,
            ContentError::NoSpawnRule {
                file: SPAWNS_FILE,
                entry: "monster `lead_homunculus` (entry 8)".to_owned(),
            }
        );
        assert!(message.contains("never appear"), "{message}");

        let (error, _) = refused(&without_rules("veil_page"));
        assert_eq!(
            error,
            ContentError::NoSpawnRule {
                file: SPAWNS_FILE,
                entry: "item `veil_page` (entry 4)".to_owned(),
            }
        );
    }

    /// A spawn file placing every shipped entry once, whose monsters leave
    /// floor 4 empty.
    const GAP_AT_FOUR: &str = r#"[
        Spawn(of: Monster("ossuary_rat"), from_depth: 1, to_depth: 3, weight: 1),
        Spawn(of: Monster("wax_beetle"), from_depth: 1, to_depth: 1, weight: 1),
        Spawn(of: Monster("acolyte"), from_depth: 5, to_depth: 10, weight: 1),
        Spawn(of: Monster("homunculus"), from_depth: 2, to_depth: 2, weight: 1),
        Spawn(of: Monster("vigil_eye"), from_depth: 3, to_depth: 3, weight: 1),
        Spawn(of: Monster("shade"), from_depth: 3, to_depth: 3, weight: 1),
        Spawn(of: Monster("hierophant"), from_depth: 5, to_depth: 10, weight: 1),
        Spawn(of: Monster("lead_homunculus"), from_depth: 5, to_depth: 10, weight: 1),
        Spawn(of: Item("healing_draught"), from_depth: 1, to_depth: 10, weight: 1),
        Spawn(of: Item("essence_phial"), from_depth: 1, to_depth: 10, weight: 1),
        Spawn(of: Item("steadying_ink"), from_depth: 1, to_depth: 10, weight: 1),
        Spawn(of: Item("veil_page"), from_depth: 2, to_depth: 10, weight: 1),
    ]"#;

    #[test]
    fn a_floor_without_any_monster_is_refused() {
        let (error, message) = refused(GAP_AT_FOUR);

        assert_eq!(
            error,
            ContentError::NoMonsterAtDepth {
                file: SPAWNS_FILE,
                depth: 4,
                first_depth: FIRST_DEPTH,
                max_depth: MAX_DEPTH,
            }
        );
        assert!(message.contains("floor 4"), "{message}");
        let filled = GAP_AT_FOUR.replace(
            r#"Monster("acolyte"), from_depth: 5"#,
            r#"Monster("acolyte"), from_depth: 4"#,
        );
        assert!(parse(&filled).is_ok());
    }

    #[test]
    fn a_syntax_error_names_the_spawn_file_and_the_rule() {
        let rule = r#"Spawn(of: Monster("shade"), from_depth: 3, to_depth: 4, wieght: 1)"#;

        let error = parse(&with_rules(&[rule])).expect_err("an unknown field");

        assert!(
            matches!(
                error,
                ContentError::Syntax {
                    file: SPAWNS_FILE,
                    ..
                }
            ),
            "{error:?}"
        );
        let message = error.to_string();
        assert!(
            message.contains(&rule_label("shade", shipped_rules() + 1)),
            "{message}"
        );
        assert!(message.contains("wieght"), "{message}");
    }
}
