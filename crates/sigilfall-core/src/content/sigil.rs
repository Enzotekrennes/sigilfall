//! Sigil definitions and the rules a sigil file must follow.
//!
//! Exposes [`SigilDef`], the validated description of one sigil, its
//! [`Effect`] and [`Targeting`], and the bound [`MAX_POWER`]. [`validate`]
//! turns the raw entries read from the sigil file into definitions, or
//! reports the first fault with the entry and field in cause: a pattern the
//! player cannot draw as written, a pattern too close to another one, a
//! targeting that cannot aim the effect, a repeated identifier, a missing
//! field, a value out of bounds, or no sigil to find on the first floor.
//!
//! This module describes sigils; it neither recognises traces (see the
//! `sigil` module of the crate) nor applies effects.

use serde::Deserialize;

use super::SIGILS_FILE;
use super::check::{EntryCheck, UniqueIds};
use super::entry_file::RawEntry;
use super::error::{ContentError, FieldProblem};
use super::sigil_file::SigilEntry;
use crate::generation::FIRST_DEPTH;
use crate::sigil::{MIN_PATTERN_DISTANCE, Trace, edit_distance};

/// Highest accepted power of a sigil, and highest strength of an item's heal
/// or steadying (`ItemEffect`).
///
/// Like the combat bounds of monsters, it only catches typos such as an extra
/// digit: two digits are far above what a balanced sigil or item needs.
pub const MAX_POWER: i32 = 99;

/// What a sigil does when cast. Its strength is the sigil's power, whose
/// meaning depends on the effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
pub enum Effect {
    /// Wounds the target: power is the hit points removed.
    Damage,
    /// Mends the target: power is the hit points restored.
    Heal,
    /// Moves the caster in a straight line: power is the number of cells.
    Blink,
    /// Holds the target in place: power is the number of turns it loses.
    Hold,
}

impl Effect {
    /// Returns the targetings that can aim this effect; a sigil file pairing
    /// the effect with any other is refused.
    ///
    /// - [`Effect::Damage`] and [`Effect::Hold`] work against an enemy:
    ///   [`Targeting::NearestEnemy`] or [`Targeting::Direction`] (the first
    ///   creature on the line). Aimed at the caster they would only hurt or
    ///   stall the player.
    /// - [`Effect::Heal`] mends the caster: [`Targeting::Caster`] only, since
    ///   healing an enemy is never wanted.
    /// - [`Effect::Blink`] moves the caster along a line, which needs a
    ///   direction: [`Targeting::Direction`] only.
    pub const fn targetings(self) -> &'static [Targeting] {
        match self {
            Self::Damage | Self::Hold => &[Targeting::NearestEnemy, Targeting::Direction],
            Self::Heal => &[Targeting::Caster],
            Self::Blink => &[Targeting::Direction],
        }
    }
}

/// Whom or where a sigil's effect applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
pub enum Targeting {
    /// The caster.
    Caster,
    /// A direction the player chooses once the trace is finished.
    Direction,
    /// The nearest enemy the caster can see.
    NearestEnemy,
}

/// One sigil, as described in the sigil file and validated.
///
/// Its pattern is a trace the player can draw exactly as written, it is at
/// least [`MIN_PATTERN_DISTANCE`] edits away from the pattern of every other
/// sigil of the file, and its targeting is one of `Effect::targetings` of its
/// effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigilDef {
    /// Position of the sigil in its file, counted from 0: the order of the
    /// grimoire and of recognition.
    index: usize,
    /// Stable key used by saves and the grimoire.
    id: String,
    /// Name shown to the player.
    name: String,
    /// Nodes to link, in order.
    pattern: Trace,
    /// What the sigil does.
    effect: Effect,
    /// Strength of the effect, from 1 to [`MAX_POWER`].
    power: i32,
    /// Whom or where the effect applies.
    targeting: Targeting,
    /// Shallowest floor where its glyph can be found.
    min_depth: u32,
}

impl SigilDef {
    /// Returns the position of the sigil in the sigil file, counted from 0:
    /// `Content::sigils()[i].index()` is `i`.
    ///
    /// The grimoire lists the sigils it knows in this order, whatever the
    /// order they were learnt in, and `sigil::recognize` expects it.
    pub const fn index(&self) -> usize {
        self.index
    }

    /// Returns the stable identifier: lower-case letters, digits and
    /// underscores, unique among sigils.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the name shown to the player, in English.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the pattern: the nodes to link, in order, at least
    /// `sigil::MIN_PATTERN_NODES` of them.
    pub const fn pattern(&self) -> &Trace {
        &self.pattern
    }

    /// Returns what the sigil does.
    pub const fn effect(&self) -> Effect {
        self.effect
    }

    /// Returns the strength of the effect, from 1 to [`MAX_POWER`]; its unit
    /// depends on the [`Effect`].
    pub const fn power(&self) -> i32 {
        self.power
    }

    /// Returns whom or where the effect applies.
    pub const fn targeting(&self) -> Targeting {
        self.targeting
    }

    /// Returns the shallowest floor where the sigil's glyph can be found;
    /// from [`FIRST_DEPTH`] to `content::MAX_DEPTH`.
    pub const fn min_depth(&self) -> u32 {
        self.min_depth
    }

    /// Returns whether the sigil's glyph may be engraved on floor `depth`:
    /// `depth` is at least [`SigilDef::min_depth`].
    pub const fn can_appear_at(&self, depth: u32) -> bool {
        depth >= self.min_depth
    }

    /// Builds a sigil identified by `id` with the pattern `digits`, without
    /// any of the file's checks: a damage of power 1 aimed at the nearest
    /// enemy, first in file order.
    ///
    /// Only tests use it, to build sigil lists a content file would refuse,
    /// such as two patterns too close to each other.
    ///
    /// # Panics
    ///
    /// Panics if `digits` is not a pattern the player can draw.
    #[cfg(test)]
    pub(crate) fn unchecked(id: &str, digits: &[u8]) -> Self {
        Self {
            index: 0,
            id: id.to_owned(),
            name: id.to_owned(),
            pattern: Trace::from_pattern(digits).expect("a drawable test pattern"),
            effect: Effect::Damage,
            power: 1,
            targeting: Targeting::NearestEnemy,
            min_depth: FIRST_DEPTH,
        }
    }
}

/// Turns the entries of the sigil file into definitions, in file order.
///
/// Each entry is checked in turn (identifier form, presence of every field,
/// pattern, bounds, targeting suited to the effect), then against the
/// entries before it (identifier not reused, pattern at least
/// [`MIN_PATTERN_DISTANCE`] edits away from theirs). The list must hold at
/// least one sigil whose glyph can be found on floor [`FIRST_DEPTH`], so a
/// new run always has something to learn.
///
/// # Errors
///
/// Returns the first fault found: [`ContentError::InvalidField`] naming the
/// entry and the field (for a pattern too close to another, the later entry,
/// with the earlier one in [`FieldProblem::TooClose`]),
/// [`ContentError::NoSigil`] for an empty list, or
/// [`ContentError::NoSigilAtFirstDepth`].
pub(super) fn validate(entries: Vec<SigilEntry>) -> Result<Vec<SigilDef>, ContentError> {
    if entries.is_empty() {
        return Err(ContentError::NoSigil { file: SIGILS_FILE });
    }
    let mut sigils: Vec<SigilDef> = Vec::with_capacity(entries.len());
    let mut ids = UniqueIds::new(SigilEntry::KIND);
    for (index, entry) in entries.into_iter().enumerate() {
        let position = index + 1;
        let sigil = validate_entry(index, entry)?;
        ids.claim(position, &sigil.id)?;
        check_distinct_pattern(position, &sigil, &sigils)?;
        sigils.push(sigil);
    }
    check_first_depth(&sigils)?;
    Ok(sigils)
}

/// Refuses a non-empty sigil list that leaves floor [`FIRST_DEPTH`] without
/// any glyph to find, naming the entry whose `min_depth` is the lowest.
fn check_first_depth(sigils: &[SigilDef]) -> Result<(), ContentError> {
    if sigils.iter().any(|sigil| sigil.can_appear_at(FIRST_DEPTH)) {
        return Ok(());
    }
    let Some(shallowest) = sigils.iter().min_by_key(|sigil| sigil.min_depth) else {
        return Ok(());
    };
    Err(ContentError::NoSigilAtFirstDepth {
        file: SIGILS_FILE,
        first_depth: FIRST_DEPTH,
        entry: SigilEntry::KIND.label(shallowest.index + 1, Some(&shallowest.id)),
        min_depth: shallowest.min_depth,
    })
}

/// Validates the entry at `index` (counted from 0) on its own, without
/// looking at the other entries.
fn validate_entry(index: usize, entry: SigilEntry) -> Result<SigilDef, ContentError> {
    let kind = SigilEntry::KIND;
    let position = index + 1;
    let id = EntryCheck::new(&kind, position, None).identifier(entry.id)?;
    let check = EntryCheck::new(&kind, position, Some(&id));
    let name = check.name(entry.name)?;
    let pattern = pattern(&check, entry.pattern)?;
    let effect = check.required(entry.effect, "effect")?;
    let power = check.in_range(entry.power, "power", 1, MAX_POWER)?;
    let targeting = suited_targeting(&check, effect, entry.targeting)?;
    Ok(SigilDef {
        index,
        name,
        pattern,
        effect,
        power,
        targeting,
        min_depth: check.depth(entry.min_depth, "min_depth")?,
        id,
    })
}

/// Returns `targeting`, the field `targeting` of the entry `check`
/// designates, refusing a missing one or one that cannot aim `effect` (see
/// [`Effect::targetings`]).
fn suited_targeting(
    check: &EntryCheck,
    effect: Effect,
    targeting: Option<Targeting>,
) -> Result<Targeting, ContentError> {
    let targeting = check.required(targeting, "targeting")?;
    let allowed = effect.targetings();
    if !allowed.contains(&targeting) {
        return Err(check.fail(
            "targeting",
            FieldProblem::UnsuitedTargeting {
                effect,
                targeting,
                allowed,
            },
        ));
    }
    Ok(targeting)
}

/// Returns the trace described by `digits`, the field `pattern` of the entry
/// `check` designates, refusing a missing pattern or one the player cannot
/// draw exactly as written (see `Trace::from_pattern`).
fn pattern(check: &EntryCheck, digits: Option<Vec<u8>>) -> Result<Trace, ContentError> {
    let digits = check.required(digits, "pattern")?;
    Trace::from_pattern(&digits)
        .map_err(|problem| check.fail("pattern", FieldProblem::BadPattern(problem)))
}

/// Refuses `sigil`, the entry at `position`, if its pattern is fewer than
/// [`MIN_PATTERN_DISTANCE`] edits away from the pattern of one of `earlier`,
/// the sigils before it in the file; the first such sigil is named.
fn check_distinct_pattern(
    position: usize,
    sigil: &SigilDef,
    earlier: &[SigilDef],
) -> Result<(), ContentError> {
    let kind = SigilEntry::KIND;
    for (index, other) in earlier.iter().enumerate() {
        let distance = edit_distance(sigil.pattern.nodes(), other.pattern.nodes());
        if distance < MIN_PATTERN_DISTANCE {
            let check = EntryCheck::new(&kind, position, Some(&sigil.id));
            return Err(check.fail(
                "pattern",
                FieldProblem::TooClose {
                    other: kind.label(index + 1, Some(&other.id)),
                    distance,
                    min: MIN_PATTERN_DISTANCE,
                },
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::{Content, MAX_NAME_LENGTH};
    use crate::sigil::{MIN_PATTERN_NODES, Node, PatternError, TraceError};

    /// The monster file shipped with the game, valid, for parsing sigils.
    const MONSTERS: &str = include_str!("../../content/monsters.ron");

    /// Fields of a valid test sigil identified by `id` with `pattern`, in
    /// file order.
    fn fields(id: &str, pattern: &str) -> Vec<(&'static str, String)> {
        [
            ("id", format!("\"{id}\"")),
            ("name", "\"test sigil\"".to_owned()),
            ("pattern", pattern.to_owned()),
            ("effect", "Damage".to_owned()),
            ("power", "5".to_owned()),
            ("targeting", "NearestEnemy".to_owned()),
            ("min_depth", "1".to_owned()),
        ]
        .into()
    }

    /// Writes one `Sigil(...)` entry, one field per line.
    fn write_entry(fields: &[(&'static str, String)]) -> String {
        let body: String = fields
            .iter()
            .map(|(name, value)| format!("        {name}: {value},\n"))
            .collect();
        format!("    Sigil(\n{body}    )")
    }

    /// A valid entry identified by `id` with `pattern`.
    fn valid(id: &str, pattern: &str) -> String {
        write_entry(&fields(id, pattern))
    }

    /// A valid entry identified by `id`, except that `field` holds `value`.
    fn with(id: &str, field: &str, value: &str) -> String {
        let mut fields = fields(id, "[7, 5, 3]");
        for (name, current) in &mut fields {
            if *name == field {
                *current = value.to_owned();
            }
        }
        write_entry(&fields)
    }

    /// A valid entry identified by `id`, except that `field` is left out.
    fn without(id: &str, field: &str) -> String {
        let mut fields = fields(id, "[7, 5, 3]");
        fields.retain(|(name, _)| *name != field);
        write_entry(&fields)
    }

    /// A sigil file listing `entries`.
    fn file(entries: &[String]) -> String {
        format!("[\n{}\n]\n", entries.join(",\n"))
    }

    /// Parses the sigil file `source` with the shipped monsters.
    fn parse(source: &str) -> Result<Content, ContentError> {
        Content::parse_files(MONSTERS, source)
    }

    /// Parses `source`, expecting it to be refused, and returns the error and
    /// its message.
    fn refused(source: &str) -> (ContentError, String) {
        let error = parse(source).expect_err("the sigil file should be refused");
        let message = error.to_string();
        (error, message)
    }

    /// Asserts that `message` names the sigil file, `entry` and `field`.
    fn assert_names(message: &str, entry: &str, field: &str) {
        assert!(message.contains(SIGILS_FILE), "no file in: {message}");
        assert!(message.contains(entry), "no `{entry}` in: {message}");
        assert!(
            message.contains(&format!("`{field}`")),
            "no field `{field}` in: {message}"
        );
    }

    /// The node numbered `digit`, which must be from 1 to 9.
    fn node(digit: u8) -> Node {
        Node::new(digit).expect("a digit from 1 to 9")
    }

    /// Asserts that `error` refuses the field `pattern` with `problem`.
    fn assert_pattern_problem(error: &ContentError, problem: &FieldProblem) {
        assert!(
            matches!(
                error,
                ContentError::InvalidField { field: "pattern", problem: p, .. } if p == problem
            ),
            "{error:?}"
        );
    }

    #[test]
    fn shipped_sigils_are_valid_and_far_apart() {
        let content = Content::load().expect("the shipped content files are valid");
        let sigils = content.sigils();

        assert!((4..=6).contains(&sigils.len()), "{} sigils", sigils.len());
        for (index, sigil) in sigils.iter().enumerate() {
            let digits: Vec<u8> = sigil.pattern().nodes().iter().map(|n| n.digit()).collect();
            assert_eq!(Trace::from_pattern(&digits).as_ref(), Ok(sigil.pattern()));
            for other in &sigils[index + 1..] {
                let distance = edit_distance(sigil.pattern().nodes(), other.pattern().nodes());
                assert!(
                    distance >= MIN_PATTERN_DISTANCE,
                    "{} and {} are {distance} edit(s) apart",
                    sigil.id(),
                    other.id()
                );
            }
        }
        assert!(
            sigils.iter().any(|sigil| sigil.min_depth() == FIRST_DEPTH),
            "no sigil to discover on the first floor"
        );
    }

    #[test]
    fn valid_file_is_read_in_order_with_its_values() {
        let source = file(&[
            valid("ember", "[7, 5, 3]"),
            write_entry(&[
                ("id", "\"recall\"".to_owned()),
                ("name", "\"Recall\"".to_owned()),
                ("pattern", "[8, 5, 2, 1]".to_owned()),
                ("effect", "Heal".to_owned()),
                ("power", "8".to_owned()),
                ("targeting", "Caster".to_owned()),
                ("min_depth", "2".to_owned()),
            ]),
        ]);

        let content = parse(&source).expect("valid sigils");

        let ids: Vec<&str> = content.sigils().iter().map(SigilDef::id).collect();
        assert_eq!(ids, ["ember", "recall"]);
        let recall = content.sigil("recall").expect("defined");
        assert_eq!(recall.name(), "Recall");
        let pattern: Vec<u8> = recall.pattern().nodes().iter().map(|n| n.digit()).collect();
        assert_eq!(pattern, [8, 5, 2, 1]);
        assert_eq!(recall.effect(), Effect::Heal);
        assert_eq!(recall.power(), 8);
        assert_eq!(recall.targeting(), Targeting::Caster);
        assert_eq!(recall.min_depth(), 2);
        assert_eq!(recall.index(), 1);
        assert!(!recall.can_appear_at(1) && recall.can_appear_at(2));
        assert!(content.sigil("unknown").is_none());
        assert!(!content.monsters().is_empty(), "monsters are still read");
    }

    #[test]
    fn each_effect_accepts_only_the_targetings_that_can_aim_it() {
        let effects = [
            ("Damage", Effect::Damage),
            ("Heal", Effect::Heal),
            ("Blink", Effect::Blink),
            ("Hold", Effect::Hold),
        ];
        let targetings = [
            ("Caster", Targeting::Caster),
            ("Direction", Targeting::Direction),
            ("NearestEnemy", Targeting::NearestEnemy),
        ];
        let suited = [
            (Effect::Damage, Targeting::NearestEnemy),
            (Effect::Damage, Targeting::Direction),
            (Effect::Heal, Targeting::Caster),
            (Effect::Blink, Targeting::Direction),
            (Effect::Hold, Targeting::NearestEnemy),
            (Effect::Hold, Targeting::Direction),
        ];
        for (effect_name, effect) in effects {
            for (targeting_name, targeting) in targetings {
                let entry =
                    with("any", "effect", effect_name).replace("NearestEnemy", targeting_name);
                let result = parse(&file(&[entry]));
                if suited.contains(&(effect, targeting)) {
                    let content = result.expect("suited targeting");
                    let sigil = &content.sigils()[0];
                    assert_eq!((sigil.effect(), sigil.targeting()), (effect, targeting));
                    continue;
                }
                let error = result.expect_err("unsuited targeting");
                let message = error.to_string();
                assert_names(&message, "sigil `any` (entry 1)", "targeting");
                assert!(
                    matches!(
                        error,
                        ContentError::InvalidField {
                            field: "targeting",
                            problem: FieldProblem::UnsuitedTargeting { effect: e, targeting: t, .. },
                            ..
                        } if (e, t) == (effect, targeting)
                    ),
                    "{effect:?} with {targeting:?}: {message}"
                );
            }
        }
    }

    #[test]
    fn unsuited_targeting_names_the_accepted_ones() {
        let entry = with("leap", "effect", "Blink").replace("NearestEnemy", "Caster");
        let (_, message) = refused(&file(&[entry]));
        assert!(
            message.contains("Caster cannot aim the effect Blink, expected Direction"),
            "{message}"
        );

        let entry = with("smite", "effect", "Damage").replace("NearestEnemy", "Caster");
        let (_, message) = refused(&file(&[entry]));
        assert!(
            message.contains("expected NearestEnemy or Direction"),
            "{message}"
        );
    }

    #[test]
    fn pattern_that_skips_an_unused_middle_node_is_refused() {
        let source = file(&[valid("ok", "[8, 5, 2, 1]"), valid("leap", "[4, 7, 9]")]);

        let (error, message) = refused(&source);

        assert_names(&message, "sigil `leap` (entry 2)", "pattern");
        assert!(message.contains("passes over node 8"), "{message}");
        assert_pattern_problem(
            &error,
            &FieldProblem::BadPattern(PatternError::SkipsMiddle {
                from: node(7),
                to: node(9),
                middle: node(8),
            }),
        );
    }

    #[test]
    fn pattern_with_a_repeated_node_is_refused() {
        let (error, message) = refused(&file(&[valid("loop", "[1, 2, 5, 1]")]));

        assert_names(&message, "sigil `loop` (entry 1)", "pattern");
        assert!(message.contains("node 1 is already part"), "{message}");
        assert_pattern_problem(
            &error,
            &FieldProblem::BadPattern(PatternError::Trace(TraceError::Repeated { node: node(1) })),
        );
    }

    #[test]
    fn pattern_off_the_grid_or_too_short_is_refused() {
        let (error, message) = refused(&file(&[valid("far", "[1, 2, 10]")]));
        assert_names(&message, "sigil `far` (entry 1)", "pattern");
        assert_pattern_problem(
            &error,
            &FieldProblem::BadPattern(PatternError::NotANode { digit: 10 }),
        );

        let (error, message) = refused(&file(&[valid("stub", "[1, 2]")]));
        assert_names(&message, "sigil `stub` (entry 1)", "pattern");
        assert_pattern_problem(
            &error,
            &FieldProblem::BadPattern(PatternError::TooShort {
                nodes: 2,
                min: MIN_PATTERN_NODES,
            }),
        );

        let (_, message) = refused(&file(&[valid("void", "[]")]));
        assert_names(&message, "sigil `void` (entry 1)", "pattern");
    }

    #[test]
    fn pattern_one_edit_from_an_earlier_one_is_refused() {
        // 7-5-2 is 7-5-3 with one node replaced.
        let source = file(&[
            valid("ember", "[7, 5, 3]"),
            valid("recall", "[8, 5, 2, 1]"),
            valid("spark", "[7, 5, 2]"),
        ]);

        let (error, message) = refused(&source);

        assert_names(&message, "sigil `spark` (entry 3)", "pattern");
        assert!(message.contains("sigil `ember` (entry 1)"), "{message}");
        assert_pattern_problem(
            &error,
            &FieldProblem::TooClose {
                other: "sigil `ember` (entry 1)".to_owned(),
                distance: 1,
                min: MIN_PATTERN_DISTANCE,
            },
        );
    }

    #[test]
    fn same_pattern_twice_is_refused() {
        let source = file(&[valid("ember", "[7, 5, 3]"), valid("twin", "[7, 5, 3]")]);

        let (error, message) = refused(&source);

        assert_names(&message, "sigil `twin` (entry 2)", "pattern");
        assert!(
            matches!(
                error,
                ContentError::InvalidField {
                    problem: FieldProblem::TooClose { distance: 0, .. },
                    ..
                }
            ),
            "{error:?}"
        );
    }

    #[test]
    fn pattern_two_edits_from_an_earlier_one_is_refused() {
        // 9-5-1 is 7-5-3 with two nodes replaced: 7-5-1 would be one edit
        // from both.
        let source = file(&[valid("ember", "[7, 5, 3]"), valid("frost", "[9, 5, 1]")]);

        let (error, message) = refused(&source);

        assert_names(&message, "sigil `frost` (entry 2)", "pattern");
        assert!(message.contains("expected at least 3"), "{message}");
        assert_pattern_problem(
            &error,
            &FieldProblem::TooClose {
                other: "sigil `ember` (entry 1)".to_owned(),
                distance: 2,
                min: MIN_PATTERN_DISTANCE,
            },
        );
    }

    #[test]
    fn patterns_three_edits_apart_are_accepted() {
        // 9-5-1-4 is 7-5-3 with two nodes replaced and one added.
        let source = file(&[valid("ember", "[7, 5, 3]"), valid("frost", "[9, 5, 1, 4]")]);

        let content = parse(&source).expect("patterns far enough apart");

        assert_eq!(MIN_PATTERN_DISTANCE, 3);
        let indexes: Vec<usize> = content.sigils().iter().map(SigilDef::index).collect();
        assert_eq!(indexes, [0, 1]);
    }

    #[test]
    fn duplicate_identifier_is_refused() {
        let source = file(&[
            valid("ember", "[7, 5, 3]"),
            valid("recall", "[8, 5, 2, 1]"),
            valid("ember", "[1, 5, 9, 6]"),
        ]);

        let (error, message) = refused(&source);

        assert_names(&message, "sigil `ember` (entry 3)", "id");
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
    fn missing_field_is_refused_with_its_entry() {
        for field in [
            "name",
            "pattern",
            "effect",
            "power",
            "targeting",
            "min_depth",
        ] {
            let source = file(&[valid("ok", "[8, 5, 2, 1]"), without("hollow", field)]);

            let (error, message) = refused(&source);

            assert_names(&message, "sigil `hollow` (entry 2)", field);
            assert!(
                matches!(
                    error,
                    ContentError::InvalidField { field: f, problem: FieldProblem::Missing, .. }
                        if f == field
                ),
                "{error:?}"
            );
        }

        let (_, message) = refused(&file(&[without("ignored", "id")]));
        assert_names(&message, "sigil entry 1", "id");
    }

    #[test]
    fn power_out_of_bounds_and_depth_zero_are_refused() {
        let too_strong = (MAX_POWER + 1).to_string();
        for power in ["0", "-2", too_strong.as_str()] {
            let (_, message) = refused(&file(&[with("odd", "power", power)]));
            assert_names(&message, "sigil `odd` (entry 1)", "power");
        }
        let strongest = MAX_POWER.to_string();
        assert!(parse(&file(&[with("odd", "power", &strongest)])).is_ok());

        let (_, message) = refused(&file(&[with("surface", "min_depth", "0")]));
        assert_names(&message, "sigil `surface` (entry 1)", "min_depth");
    }

    #[test]
    fn malformed_identifier_or_blank_name_is_refused() {
        let (_, message) = refused(&file(&[with("ignored", "id", "\"Ember\"")]));
        assert_names(&message, "sigil entry 1", "id");

        let (_, message) = refused(&file(&[with("nameless", "name", "\"\"")]));
        assert_names(&message, "sigil `nameless` (entry 1)", "name");
    }

    #[test]
    fn unknown_effect_reports_line_entry_and_field() {
        // Line 1 is `[`, the entry opens on line 2 and `effect` is its fourth
        // field.
        let (error, message) = refused(&file(&[with("odd", "effect", "Teleport")]));

        assert!(
            matches!(error, ContentError::Syntax { line: 6, .. }),
            "{error:?}"
        );
        assert_names(&message, "sigil `odd` (entry 1), field", "effect");
        assert!(message.contains("Teleport"), "{message}");
    }

    #[test]
    fn node_that_is_not_a_small_number_is_a_syntax_error() {
        let (error, message) = refused(&file(&[valid("odd", "[7, 5, 300]")]));

        assert!(
            matches!(error, ContentError::Syntax { line: 5, .. }),
            "{error:?}"
        );
        assert_names(&message, "sigil `odd` (entry 1), field", "pattern");
    }

    #[test]
    fn unknown_field_and_errors_outside_the_entries_are_located() {
        let source = file(&[valid("ok", "[7, 5, 3]")]).replace("power: 5", "might: 5");
        let (_, message) = refused(&source);
        assert_names(&message, "sigil `ok` (entry 1)", "might");

        let (_, message) = refused("Sigil(id: \"lonely\")");
        assert!(message.contains("before the first sigil"), "{message}");

        let (_, message) = refused(&format!("{} trailing", file(&[valid("ok", "[7, 5, 3]")])));
        assert!(message.contains("after the last sigil"), "{message}");
    }

    #[test]
    fn a_file_without_a_sigil_on_the_first_floor_is_refused() {
        let source = file(&[
            with("deep", "min_depth", "3"),
            with("shallowest", "min_depth", "2").replace("[7, 5, 3]", "[8, 5, 2, 1]"),
        ]);

        let (error, message) = refused(&source);

        assert_eq!(
            error,
            ContentError::NoSigilAtFirstDepth {
                file: SIGILS_FILE,
                first_depth: FIRST_DEPTH,
                entry: "sigil `shallowest` (entry 2)".to_owned(),
                min_depth: 2,
            }
        );
        assert!(message.contains(SIGILS_FILE), "{message}");
        assert!(
            message.contains("sigil `shallowest` (entry 2)"),
            "{message}"
        );
        let one_at_first = source.replacen("min_depth: 3", "min_depth: 1", 1);
        assert!(parse(&one_at_first).is_ok());
    }

    #[test]
    fn a_name_longer_than_the_screens_can_show_is_refused() {
        let longest = "x".repeat(MAX_NAME_LENGTH);
        let entry = with("wordy", "name", &format!("\"{longest}\""));
        let content = parse(&file(&[entry])).expect("a name at the limit");
        assert_eq!(content.sigils()[0].name(), longest);

        let too_long = format!("\"{longest}y\"");
        let (error, message) = refused(&file(&[with("wordy", "name", &too_long)]));

        assert_names(&message, "sigil `wordy` (entry 1)", "name");
        assert_eq!(
            error,
            ContentError::InvalidField {
                file: SIGILS_FILE,
                entry: "sigil `wordy` (entry 1)".to_owned(),
                field: "name",
                problem: FieldProblem::TooLong {
                    length: MAX_NAME_LENGTH + 1,
                    max: MAX_NAME_LENGTH,
                },
            }
        );
    }

    #[test]
    fn empty_file_is_refused() {
        let (error, message) = refused("[]");

        assert_eq!(error, ContentError::NoSigil { file: SIGILS_FILE });
        assert!(message.contains(SIGILS_FILE), "{message}");
    }
}
