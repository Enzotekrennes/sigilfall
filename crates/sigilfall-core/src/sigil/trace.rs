//! The tracing grid and the rules a trace follows (decision D7).
//!
//! Exposes [`Node`], [`Trace`], their errors [`TraceError`] and
//! [`PatternError`], and [`MIN_PATTERN_NODES`]. The grid is laid out like a
//! numeric keypad, so the digit a player presses is the node it links:
//!
//! ```text
//! 7 8 9
//! 4 5 6
//! 1 2 3
//! ```
//!
//! This module does not know which sigils exist; see `recognition` for
//! comparing a trace to them.

use std::fmt;

use thiserror::Error;

/// Fewest nodes a sigil pattern may have.
///
/// A pattern of one or two nodes is a single stroke: a player would cast it
/// by accident while starting any longer trace, and every two-node trace
/// would be an unstable version of it.
pub const MIN_PATTERN_NODES: usize = 3;

/// Number of nodes on one side of the grid.
const SIDE: u8 = 3;

/// One point of the 3 × 3 grid, numbered 1 to 9 like a numeric keypad: 7 8 9
/// on the top row, 1 2 3 on the bottom one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Node(u8);

impl Node {
    /// Returns the node numbered `digit`, or `None` if `digit` is not from 1
    /// to 9.
    pub const fn new(digit: u8) -> Option<Self> {
        if digit >= 1 && digit <= SIDE * SIDE {
            Some(Self(digit))
        } else {
            None
        }
    }

    /// Returns the node's number, from 1 to 9: the keypad digit that links
    /// it.
    pub const fn digit(self) -> u8 {
        self.0
    }

    /// Returns the node's column, from 0 (left: 7, 4, 1) to 2 (right: 9, 6,
    /// 3).
    pub const fn column(self) -> u8 {
        (self.0 - 1) % SIDE
    }

    /// Returns the node's row, from 0 (bottom: 1, 2, 3) to 2 (top: 7, 8, 9),
    /// as on a keypad.
    pub const fn row(self) -> u8 {
        (self.0 - 1) / SIDE
    }

    /// Returns the node a straight stroke from `self` to `other` passes over,
    /// if any.
    ///
    /// That is the case when the two nodes are aligned (same row, same column
    /// or same diagonal) without being neighbours, such as 1 and 3 (over 2) or
    /// 7 and 3 (over 5). Neighbours, a node and itself, and pairs that are
    /// not aligned (such as 1 and 6) give `None`.
    pub const fn between(self, other: Self) -> Option<Self> {
        let columns = self.column() + other.column();
        let rows = self.row() + other.row();
        // The middle lies on the grid exactly when both sums are even; for
        // two distinct nodes it is then a third node.
        if self.0 == other.0 || !columns.is_multiple_of(2) || !rows.is_multiple_of(2) {
            return None;
        }
        Some(Self(rows / 2 * SIDE + columns / 2 + 1))
    }
}

impl fmt::Display for Node {
    /// Writes the node's digit, as the player presses it.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// Why a node cannot be added to a trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum TraceError {
    /// The node is already part of the trace; a trace never links a node
    /// twice.
    #[error("node {node} is already part of the trace")]
    Repeated {
        /// The node that was linked again.
        node: Node,
    },
}

/// Why a list of digits written in a content file is not a sigil pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PatternError {
    /// A digit is not a node of the grid.
    #[error("{digit} is not a node of the grid, expected 1 to 9")]
    NotANode {
        /// The digit found.
        digit: u8,
    },
    /// The same node is listed twice.
    #[error(transparent)]
    Trace(#[from] TraceError),
    /// Two consecutive nodes are aligned over an unused node that the pattern
    /// does not list between them, so the player would link it anyway: the
    /// written pattern is not the one drawn.
    #[error(
        "the stroke from node {from} to node {to} passes over node {middle}, \
         which must be written between them"
    )]
    SkipsMiddle {
        /// Node the stroke starts from.
        from: Node,
        /// Node the stroke ends on.
        to: Node,
        /// The node it passes over.
        middle: Node,
    },
    /// The pattern has fewer than [`MIN_PATTERN_NODES`] nodes.
    #[error("{nodes} node(s), expected at least {min}")]
    TooShort {
        /// Number of nodes in the pattern.
        nodes: usize,
        /// Fewest nodes accepted, [`MIN_PATTERN_NODES`].
        min: usize,
    },
}

/// The nodes linked so far, in order, following the drawing rules of
/// decision D7.
///
/// A trace never holds the same node twice. A stroke between two aligned
/// nodes that are not neighbours passes over the node between them (see
/// [`Node::between`]): if that node is not linked yet it is linked on the
/// way, as when tracing with a finger; if it is already linked the stroke
/// simply crosses it, since a node cannot be linked twice.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Trace {
    /// Linked nodes, in the order they were linked.
    nodes: Vec<Node>,
}

impl Trace {
    /// Returns an empty trace, before the first node is linked.
    pub const fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    /// Builds the trace a sigil pattern describes, from the digits written in
    /// a content file.
    ///
    /// A pattern lists every node the player links, in order: `[7, 5, 3]` is
    /// valid, but `[7, 3]` is not, because the stroke from 7 to 3 links 5 on
    /// the way. So the pattern shown to the player is exactly what they
    /// draw.
    ///
    /// # Errors
    ///
    /// - [`PatternError::NotANode`] for a digit outside 1 to 9;
    /// - [`PatternError::Trace`] for a node listed twice;
    /// - [`PatternError::SkipsMiddle`] for a stroke over an unused node not
    ///   listed between its ends;
    /// - [`PatternError::TooShort`] for fewer than [`MIN_PATTERN_NODES`]
    ///   nodes.
    pub fn from_pattern(digits: &[u8]) -> Result<Self, PatternError> {
        let mut trace = Self::new();
        for &digit in digits {
            let node = Node::new(digit).ok_or(PatternError::NotANode { digit })?;
            let before = trace.len();
            trace.push(node)?;
            // Growing by two nodes means `push` linked a middle node the
            // pattern does not list.
            if trace.len() > before + 1 {
                return Err(PatternError::SkipsMiddle {
                    from: trace.nodes[before - 1],
                    to: node,
                    middle: trace.nodes[before],
                });
            }
        }
        if trace.len() < MIN_PATTERN_NODES {
            return Err(PatternError::TooShort {
                nodes: trace.len(),
                min: MIN_PATTERN_NODES,
            });
        }
        Ok(trace)
    }

    /// Links `node` at the end of the trace.
    ///
    /// When the stroke from the last node to `node` passes over a node not
    /// linked yet, that node is linked first, so the trace grows by two
    /// nodes; otherwise it grows by one.
    ///
    /// # Errors
    ///
    /// Returns [`TraceError::Repeated`] if `node` is already linked; the
    /// trace is then unchanged.
    pub fn push(&mut self, node: Node) -> Result<(), TraceError> {
        if self.contains(node) {
            return Err(TraceError::Repeated { node });
        }
        if let Some(middle) = self.last().and_then(|last| last.between(node))
            && !self.contains(middle)
        {
            self.nodes.push(middle);
        }
        self.nodes.push(node);
        Ok(())
    }

    /// Returns the linked nodes, in the order they were linked.
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// Returns the number of linked nodes, from 0 to 9.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns whether no node is linked yet.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Returns the node linked last, where the next stroke starts, if any.
    pub fn last(&self) -> Option<Node> {
        self.nodes.last().copied()
    }

    /// Returns whether `node` is already linked.
    pub fn contains(&self, node: Node) -> bool {
        self.nodes.contains(&node)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The node numbered `digit`, which must be from 1 to 9.
    fn node(digit: u8) -> Node {
        Node::new(digit).expect("a digit from 1 to 9")
    }

    /// The trace obtained by pressing `digits` in order, each accepted.
    fn pressed(digits: &[u8]) -> Trace {
        let mut trace = Trace::new();
        for &digit in digits {
            trace.push(node(digit)).expect("a node not linked yet");
        }
        trace
    }

    /// The digits of the nodes of `trace`, in order.
    fn digits(trace: &Trace) -> Vec<u8> {
        trace.nodes().iter().map(|node| node.digit()).collect()
    }

    #[test]
    fn nodes_are_the_digits_one_to_nine_laid_out_like_a_keypad() {
        assert_eq!(Node::new(0), None);
        assert_eq!(Node::new(10), None);
        let layout: Vec<(u8, u8, u8)> = (1..=9)
            .map(|digit| (digit, node(digit).column(), node(digit).row()))
            .collect();
        #[rustfmt::skip]
        let expected = [
            (1, 0, 0), (2, 1, 0), (3, 2, 0),
            (4, 0, 1), (5, 1, 1), (6, 2, 1),
            (7, 0, 2), (8, 1, 2), (9, 2, 2),
        ];
        assert_eq!(layout, expected);
        assert_eq!(node(7).to_string(), "7");
    }

    #[test]
    fn only_aligned_nodes_that_are_not_neighbours_have_a_middle() {
        let aligned = [
            (1, 3, 2),
            (4, 6, 5),
            (7, 9, 8),
            (1, 7, 4),
            (2, 8, 5),
            (3, 9, 6),
            (1, 9, 5),
            (3, 7, 5),
        ];
        let mut with_middle = 0;
        for a in 1..=9 {
            for b in 1..=9 {
                let expected = aligned
                    .iter()
                    .find(|&&(x, y, _)| (x, y) == (a, b) || (y, x) == (a, b))
                    .map(|&(_, _, middle)| node(middle));
                assert_eq!(node(a).between(node(b)), expected, "{a} to {b}");
                with_middle += usize::from(expected.is_some());
            }
        }
        // Eight aligned pairs, each counted in both directions.
        assert_eq!(with_middle, 16);
    }

    #[test]
    fn neighbours_and_unaligned_nodes_are_linked_directly() {
        assert_eq!(digits(&pressed(&[7, 8, 5, 1])), [7, 8, 5, 1]);
        // 1 to 6 is a knight's move: no node lies on the stroke.
        assert_eq!(digits(&pressed(&[1, 6, 7])), [1, 6, 7]);
    }

    #[test]
    fn a_stroke_over_an_unused_middle_node_links_it() {
        assert_eq!(digits(&pressed(&[7, 3])), [7, 5, 3]);
        assert_eq!(digits(&pressed(&[1, 3, 9])), [1, 2, 3, 6, 9]);
    }

    #[test]
    fn a_stroke_over_a_linked_middle_node_crosses_it() {
        // 5 is linked first, so the stroke from 4 to 6 crosses it.
        assert_eq!(digits(&pressed(&[5, 4, 6])), [5, 4, 6]);
        assert_eq!(digits(&pressed(&[2, 1, 3])), [2, 1, 3]);
    }

    #[test]
    fn a_node_cannot_be_linked_twice() {
        let mut trace = pressed(&[7, 8, 9]);

        assert_eq!(
            trace.push(node(8)),
            Err(TraceError::Repeated { node: node(8) })
        );
        assert_eq!(
            trace.push(node(9)),
            Err(TraceError::Repeated { node: node(9) })
        );
        assert_eq!(digits(&trace), [7, 8, 9], "a refused node changes nothing");
        assert_eq!(
            TraceError::Repeated { node: node(8) }.to_string(),
            "node 8 is already part of the trace"
        );
    }

    #[test]
    fn a_trace_can_link_all_nine_nodes() {
        let trace = pressed(&[7, 8, 9, 6, 5, 4, 1, 2, 3]);

        assert_eq!(trace.len(), 9);
        assert!(!trace.is_empty());
        assert_eq!(trace.last(), Some(node(3)));
        assert!(Trace::new().is_empty());
        assert_eq!(Trace::new().last(), None);
    }

    #[test]
    fn a_pattern_is_the_exact_list_of_nodes_drawn() {
        let pattern = Trace::from_pattern(&[7, 5, 3]).expect("valid pattern");
        assert_eq!(pattern, pressed(&[7, 3]));
        // The middle node is linked first, so the strokes 4 to 6 and 7 to 9
        // cross it and the patterns list nothing between their ends.
        assert!(Trace::from_pattern(&[5, 4, 6]).is_ok());
        assert!(Trace::from_pattern(&[8, 7, 9]).is_ok());
    }

    #[test]
    fn a_pattern_that_skips_an_unused_middle_node_is_refused() {
        assert_eq!(
            Trace::from_pattern(&[4, 7, 9]),
            Err(PatternError::SkipsMiddle {
                from: node(7),
                to: node(9),
                middle: node(8),
            })
        );
    }

    #[test]
    fn a_pattern_with_a_repeated_node_or_a_digit_off_the_grid_is_refused() {
        assert_eq!(
            Trace::from_pattern(&[1, 2, 1]),
            Err(PatternError::Trace(TraceError::Repeated { node: node(1) }))
        );
        assert_eq!(
            Trace::from_pattern(&[1, 2, 0]),
            Err(PatternError::NotANode { digit: 0 })
        );
        assert_eq!(
            Trace::from_pattern(&[1, 2, 10]),
            Err(PatternError::NotANode { digit: 10 })
        );
    }

    #[test]
    fn a_pattern_shorter_than_the_minimum_is_refused() {
        for digits in [&[][..], &[5], &[5, 6]] {
            assert_eq!(
                Trace::from_pattern(digits),
                Err(PatternError::TooShort {
                    nodes: digits.len(),
                    min: MIN_PATTERN_NODES,
                })
            );
        }
        assert!(Trace::from_pattern(&[5, 6, 9]).is_ok());
    }
}
