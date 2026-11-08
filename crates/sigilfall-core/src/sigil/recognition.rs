//! Recognition of a finished trace among the known sigils (decision D7).
//!
//! Exposes [`recognize`] and its result [`Recognition`], [`edit_distance`],
//! the measure of how far a trace is from a pattern, the thresholds
//! [`UNSTABLE_DISTANCE`], [`MIN_PATTERN_DISTANCE`] and
//! [`NEAR_MISS_DISTANCE`], and [`is_unknown_pattern`]. A trace equal to a
//! pattern casts that sigil; a trace one edit away casts it unstably;
//! anything else fizzles, either as a near miss of a known sigil or as a
//! pattern the grimoire does not hold at all. What success, instability or
//! a fizzle do in the game is not decided here.

use super::{Node, Trace};
use crate::content::SigilDef;

/// Largest edit distance between a trace and a pattern for which the sigil
/// is still cast, unstably.
///
/// One wrong, missing or extra node: the "one mistake" of decision D7.
pub const UNSTABLE_DISTANCE: usize = 1;

/// Smallest edit distance two sigil patterns may have from each other
/// (decision D18).
///
/// Twice [`UNSTABLE_DISTANCE`] plus one, so that a near miss always points
/// to a single sigil: the edit distance obeys the triangle inequality, so a
/// trace within [`UNSTABLE_DISTANCE`] of two patterns would put them at most
/// twice that apart. A trace one edit away from a pattern is therefore never
/// another pattern, nor one edit away from another. [`recognize`] still
/// breaks such ties by a fixed rule, as a safeguard for sigil lists that do
/// not come from a validated content file.
pub const MIN_PATTERN_DISTANCE: usize = 2 * UNSTABLE_DISTANCE + 1;

/// Largest edit distance between a fizzled trace and a known pattern for
/// which the trace still reads as a botched attempt at that sigil, rather
/// than as a pattern the grimoire does not hold: 2.
///
/// One less than [`MIN_PATTERN_DISTANCE`]: validated patterns are at least
/// that far apart, so the exact pattern of a sigil not learnt yet is never
/// a near miss of a known one. Traced, it is always told apart as unknown
/// (see [`is_unknown_pattern`]), which points the player to the glyphs.
pub const NEAR_MISS_DISTANCE: usize = MIN_PATTERN_DISTANCE - 1;

/// What a finished trace casts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recognition<'a> {
    /// The trace is exactly the pattern of this sigil.
    Success(&'a SigilDef),
    /// The trace is [`UNSTABLE_DISTANCE`] edit away from the pattern of this
    /// sigil, and exactly matches none.
    Unstable(&'a SigilDef),
    /// The trace is further from every known pattern.
    Fizzle,
}

/// Returns the edit distance between the node sequences `a` and `b`.
///
/// It is the Levenshtein distance on nodes: the fewest single-node edits
/// that turn one sequence into the other, an edit being inserting a node,
/// removing one, or replacing one by another. So 7-5-3 is one edit from
/// 7-5-2 (a node replaced), from 7-5 (one removed) and from 7-5-3-6 (one
/// added). Swapping two nodes counts as two edits. The distance is
/// symmetric, and 0 exactly when both sequences are equal.
pub fn edit_distance(a: &[Node], b: &[Node]) -> usize {
    // Classic dynamic programming, one row at a time: after processing the
    // first `i` nodes of `a`, `row[j]` is the distance between them and the
    // first `j` nodes of `b`.
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, &from) in a.iter().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, &to) in b.iter().enumerate() {
            let replaced = diagonal + usize::from(from != to);
            let removed = row[j + 1] + 1;
            let inserted = row[j] + 1;
            diagonal = row[j + 1];
            row[j + 1] = replaced.min(removed).min(inserted);
        }
    }
    row[b.len()]
}

/// Compares the finished `trace` to the patterns of `sigils` and returns
/// what it casts.
///
/// `sigils` are the sigils the trace may cast, normally those the player
/// knows, in the order of the content file. A sigil whose pattern equals
/// the trace is a [`Recognition::Success`]. Otherwise, the first sigil in
/// that order whose pattern is [`UNSTABLE_DISTANCE`] edit away is a
/// [`Recognition::Unstable`]. Validated sigils are [`MIN_PATTERN_DISTANCE`]
/// apart, so at most one of them can be that close; should two be (sigils
/// that skipped validation), the one listed first wins, so the result never
/// depends on anything but the trace and that order.
/// Otherwise, including for an empty trace or no sigil, the trace is a
/// [`Recognition::Fizzle`].
pub fn recognize<'a>(
    trace: &Trace,
    sigils: impl IntoIterator<Item = &'a SigilDef>,
) -> Recognition<'a> {
    let mut unstable = None;
    for sigil in sigils {
        let distance = edit_distance(trace.nodes(), sigil.pattern().nodes());
        if distance == 0 {
            return Recognition::Success(sigil);
        }
        if distance <= UNSTABLE_DISTANCE && unstable.is_none() {
            unstable = Some(sigil);
        }
    }
    unstable.map_or(Recognition::Fizzle, Recognition::Unstable)
}

/// Returns whether `trace` is further than [`NEAR_MISS_DISTANCE`] edits
/// from the pattern of every sigil of `sigils`: a pattern the grimoire does
/// not hold, not a slip of the hand on a known sigil.
///
/// `sigils` are the sigils the trace may cast, normally those the player
/// knows; with none, every trace is unknown. Such a trace always fizzles
/// ([`recognize`] gives [`Recognition::Fizzle`]); the converse is not true,
/// since a fizzle two edits from a known pattern is a near miss.
pub fn is_unknown_pattern<'a>(
    trace: &Trace,
    sigils: impl IntoIterator<Item = &'a SigilDef>,
) -> bool {
    sigils
        .into_iter()
        .all(|sigil| edit_distance(trace.nodes(), sigil.pattern().nodes()) > NEAR_MISS_DISTANCE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::Content;

    /// The node numbered `digit`, which must be from 1 to 9.
    fn node(digit: u8) -> Node {
        Node::new(digit).expect("a digit from 1 to 9")
    }

    /// The nodes numbered `digits`, without the drawing rules of a trace.
    fn nodes(digits: &[u8]) -> Vec<Node> {
        digits.iter().map(|&digit| node(digit)).collect()
    }

    /// The trace obtained by pressing `digits` in order, each accepted.
    fn pressed(digits: &[u8]) -> Trace {
        let mut trace = Trace::new();
        for &digit in digits {
            trace.push(node(digit)).expect("a node not linked yet");
        }
        trace
    }

    /// A content whose sigils have the given identifiers and patterns, in
    /// that order, with the shipped monsters.
    fn content_with(sigils: &[(&str, &str)]) -> Content {
        let entries: Vec<String> = sigils
            .iter()
            .map(|(id, pattern)| {
                format!(
                    "Sigil(id: \"{id}\", name: \"{id}\", pattern: {pattern}, effect: Damage, \
                     power: 1, targeting: NearestEnemy, min_depth: 1, known_at_start: false)"
                )
            })
            .collect();
        let monsters = include_str!("../../content/monsters.ron");
        Content::parse_files(monsters, &format!("[{}]", entries.join(",")))
            .expect("valid test sigils")
    }

    /// The identifier of the sigil `recognition` casts, with its quality.
    fn verdict(recognition: Recognition<'_>) -> (&'static str, Option<&str>) {
        match recognition {
            Recognition::Success(sigil) => ("success", Some(sigil.id())),
            Recognition::Unstable(sigil) => ("unstable", Some(sigil.id())),
            Recognition::Fizzle => ("fizzle", None),
        }
    }

    #[test]
    fn edit_distance_counts_replaced_removed_and_added_nodes() {
        let pattern = nodes(&[7, 5, 3]);
        let cases: [(&[u8], usize); 9] = [
            (&[7, 5, 3], 0),
            (&[7, 5, 2], 1),
            (&[7, 5], 1),
            (&[5, 3], 1),
            (&[7, 5, 3, 6], 1),
            (&[7, 8, 5, 3], 1),
            (&[3, 5, 7], 2),
            (&[1, 2], 3),
            (&[], 3),
        ];
        for (other, expected) in cases {
            let other = nodes(other);
            assert_eq!(edit_distance(&pattern, &other), expected, "{other:?}");
            assert_eq!(edit_distance(&other, &pattern), expected, "{other:?}");
        }
    }

    #[test]
    fn an_exact_trace_succeeds() {
        let content = content_with(&[("ember", "[7, 5, 3]"), ("recall", "[8, 5, 2, 1]")]);

        let cast = |digits: &[u8]| verdict(recognize(&pressed(digits), content.sigils()));

        assert_eq!(cast(&[8, 5, 2, 1]), ("success", Some("recall")));
        // The stroke 7 to 3 links 5 on the way: the trace is 7-5-3.
        assert_eq!(cast(&[7, 3]), ("success", Some("ember")));
    }

    #[test]
    fn a_trace_one_edit_away_is_unstable() {
        let content = content_with(&[("ember", "[7, 5, 3]"), ("recall", "[8, 5, 2, 1]")]);

        let cast = |digits: &[u8]| verdict(recognize(&pressed(digits), content.sigils()));

        assert_eq!(cast(&[7, 5, 2]), ("unstable", Some("ember")));
        assert_eq!(cast(&[7, 5, 3, 6]), ("unstable", Some("ember")));
        assert_eq!(cast(&[8, 5, 2]), ("unstable", Some("recall")));
        assert_eq!(cast(&[9, 5, 2, 1]), ("unstable", Some("recall")));
    }

    #[test]
    fn an_unknown_or_empty_trace_fizzles() {
        let content = content_with(&[("ember", "[7, 5, 3]"), ("recall", "[8, 5, 2, 1]")]);

        let cast = |digits: &[u8]| verdict(recognize(&pressed(digits), content.sigils()));

        assert_eq!(cast(&[4, 8, 6]), ("fizzle", None));
        assert_eq!(cast(&[3, 5, 7]), ("fizzle", None), "reversed: two edits");
        assert_eq!(cast(&[]), ("fizzle", None));
        let nothing_known = recognize(&pressed(&[7, 5, 3]), []);
        assert_eq!(verdict(nothing_known), ("fizzle", None));
    }

    #[test]
    fn a_fizzle_two_edits_from_a_known_pattern_is_a_near_miss_not_unknown() {
        let content = content_with(&[("ember", "[7, 5, 3]"), ("recall", "[8, 5, 2, 1]")]);
        let unknown = |digits: &[u8]| is_unknown_pattern(&pressed(digits), content.sigils());

        // Two edits from ember: it fizzles, yet reads as a botched ember.
        assert_eq!(
            verdict(recognize(&pressed(&[3, 5, 7]), content.sigils())),
            ("fizzle", None)
        );
        assert!(!unknown(&[3, 5, 7]));
        assert!(!unknown(&[7, 5, 3]), "a known pattern is never unknown");
        assert!(!unknown(&[7, 5, 2]), "nor is a trace one edit off");
        // Three edits or more from every known pattern.
        assert!(unknown(&[4, 8, 6]));
        assert!(unknown(&[1, 5, 9, 6]), "lance, not learnt");
        assert!(unknown(&[]));
        assert!(
            is_unknown_pattern(&pressed(&[7, 5, 3]), []),
            "nothing known"
        );
        assert_eq!(NEAR_MISS_DISTANCE, 2);
    }

    #[test]
    fn the_pattern_of_a_sigil_not_learnt_is_always_unknown() {
        let content = Content::load().expect("the shipped content is valid");
        for learnt in content.sigils() {
            for other in content.sigils().iter().filter(|s| s.id() != learnt.id()) {
                assert!(
                    is_unknown_pattern(other.pattern(), [learnt]),
                    "{} traced while knowing only {}",
                    other.id(),
                    learnt.id()
                );
            }
        }
    }

    #[test]
    fn only_the_given_sigils_are_recognised() {
        let content = content_with(&[("ember", "[7, 5, 3]"), ("recall", "[8, 5, 2, 1]")]);
        let known = content.sigil("recall");

        let recognition = recognize(&pressed(&[7, 5, 3]), known);

        assert_eq!(verdict(recognition), ("fizzle", None));
    }

    #[test]
    fn a_trace_one_edit_from_two_patterns_casts_the_one_listed_first() {
        // 7-5-3 and 9-5-1 are two edits apart, too close for a content file,
        // and 7-5-1 is one edit from both: the safeguard decides.
        let ember = SigilDef::unchecked("ember", &[7, 5, 3]);
        let frost = SigilDef::unchecked("frost", &[9, 5, 1]);
        let trace = pressed(&[7, 5, 1]);

        assert_eq!(
            verdict(recognize(&trace, [&ember, &frost])),
            ("unstable", Some("ember"))
        );
        assert_eq!(
            verdict(recognize(&trace, [&frost, &ember])),
            ("unstable", Some("frost"))
        );
    }

    #[test]
    fn shipped_near_misses_point_to_a_single_sigil() {
        // Every trace one edit from a shipped pattern is neither another
        // shipped pattern nor one edit from one.
        let content = Content::load().expect("the shipped content is valid");
        for (index, sigil) in content.sigils().iter().enumerate() {
            for other in &content.sigils()[index + 1..] {
                let distance = edit_distance(sigil.pattern().nodes(), other.pattern().nodes());
                assert!(
                    distance > 2 * UNSTABLE_DISTANCE,
                    "{} and {} are {distance} edit(s) apart",
                    sigil.id(),
                    other.id()
                );
            }
        }
    }
}
