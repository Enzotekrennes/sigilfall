//! The sigil overlay's own state: the trace being drawn, the direction
//! prompt and the verdict shown after an imperfect cast.
//!
//! Exposes [`Casting`], its steps [`Drawing`] and [`Verdict`], and the
//! [`Notice`] a drawing shows. A trace lives here, node by node, until the
//! player validates it: only then does the application submit it whole to
//! the game as `Action::Cast` (decision D19). Nothing in this module spends
//! time or essence, so cancelling at any step costs nothing. The drawing
//! rules themselves (no node twice, a stroke over an unused middle node
//! links it) are `Trace`'s; this module never re-implements them.

use sigilfall_core::sigil::{CastQuality, Node, Trace, TraceError};

/// Where the player stands in casting a sigil, from the first node to the
/// verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Casting {
    /// The player links nodes on the grid.
    Drawing(Drawing),
    /// The trace is finished and casts a sigil aimed in a direction: the
    /// direction is asked before the cast is submitted.
    Aiming {
        /// The finished trace, never empty.
        trace: Trace,
        /// Position of the sigil the trace casts in the content's sigil
        /// list (`SigilDef::index`), to name it.
        sigil: usize,
    },
    /// An unstable or fizzled cast was submitted: its verdict stays on
    /// screen until the next key.
    Verdict(Verdict),
}

impl Casting {
    /// Returns the first step of a cast: drawing, with an empty trace.
    pub const fn start() -> Self {
        Self::Drawing(Drawing::new())
    }
}

/// A trace being drawn, and what the overlay tells about the last key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Drawing {
    /// Nodes linked so far, following the rules of `Trace`.
    trace: Trace,
    /// What the last key did wrong, if anything; cleared by the next key
    /// that changes the trace.
    notice: Option<Notice>,
}

/// Why the last key of a drawing changed nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    /// The node is already part of the trace and was not linked again.
    Refused(Node),
    /// Enter was pressed on an empty trace, which is never cast.
    Empty,
}

impl Drawing {
    /// Returns a drawing with no node linked and nothing to tell.
    pub const fn new() -> Self {
        Self {
            trace: Trace::new(),
            notice: None,
        }
    }

    /// Returns a drawing of `trace`, as when the player comes back from the
    /// direction prompt.
    pub const fn of(trace: Trace) -> Self {
        Self {
            trace,
            notice: None,
        }
    }

    /// Returns the nodes linked so far.
    pub const fn trace(&self) -> Trace {
        self.trace
    }

    /// Returns what the last key did wrong, if anything.
    pub const fn notice(&self) -> Option<Notice> {
        self.notice
    }

    /// Links `node` to the trace, by `Trace::push`'s rules: a stroke over
    /// an unused middle node links it on the way.
    ///
    /// A node already linked is refused: the trace is unchanged and the
    /// drawing's notice says so ([`Notice::Refused`]). Otherwise the notice
    /// is cleared.
    pub fn link(&mut self, node: Node) {
        self.notice = match self.trace.push(node) {
            Ok(()) => None,
            Err(TraceError::Repeated { node }) => Some(Notice::Refused(node)),
        };
    }

    /// Removes the last node linked and clears the notice; does nothing
    /// to an empty trace but clear the notice.
    ///
    /// Only the last node goes, even when it was reached by a stroke that
    /// linked a middle node on the way: pressing 7 then 3 links 7, 5 and 3,
    /// and undoing leaves 7 and 5. The shorter trace is rebuilt by linking
    /// the remaining nodes again in order, which gives back exactly those
    /// nodes: a middle node a stroke passed over always comes before the
    /// stroke's end, so it is already linked when that end is linked again.
    pub fn undo(&mut self) {
        self.notice = None;
        let nodes = self.trace.nodes();
        let Some((_, kept)) = nodes.split_last() else {
            return;
        };
        let mut trace = Trace::new();
        for &node in kept {
            // Each node of a trace is new to the nodes before it, so linking
            // the prefix again cannot be refused.
            let linked = trace.push(node);
            debug_assert!(linked.is_ok(), "{node} relinked in {kept:?}");
        }
        self.trace = trace;
    }

    /// Notes that Enter was pressed on an empty trace ([`Notice::Empty`]).
    pub const fn refuse_empty(&mut self) {
        self.notice = Some(Notice::Empty);
    }
}

/// The verdict of a cast that was not pure, as the overlay shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verdict {
    /// The trace that was cast.
    pub trace: Trace,
    /// How well it matched: unstable or fizzled.
    pub quality: CastQuality,
    /// Position of the sigil an unstable trace cast in the content's sigil
    /// list (`SigilDef::index`); `None` for a fizzle.
    pub sigil: Option<usize>,
    /// Whether the fizzled trace is a pattern the grimoire does not hold
    /// (`Event::UnknownPattern`) rather than a near miss of a known sigil;
    /// `false` for an unstable trace.
    pub unknown: bool,
}

impl Verdict {
    /// Returns the node where the trace went wrong, drawn `×` on the
    /// verdict, given the pattern of the sigil it was meant to be.
    ///
    /// For an unstable trace (`pattern` given), it is the first node of the
    /// trace that differs from the pattern at the same place: the replaced
    /// node, or the extra one. A trace that only misses its last node
    /// strays nowhere and has none. For a fizzle (`pattern` is `None`), it
    /// is the last node, where the trace ended without matching anything.
    pub fn fault(&self, pattern: Option<&Trace>) -> Option<Node> {
        let nodes = self.trace.nodes();
        let Some(pattern) = pattern else {
            return nodes.last().copied();
        };
        nodes
            .iter()
            .zip(
                pattern
                    .nodes()
                    .iter()
                    .map(Some)
                    .chain(std::iter::repeat(None)),
            )
            .find(|(traced, expected)| Some(*traced) != *expected)
            .map(|(traced, _)| *traced)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The node numbered `digit`, which must be from 1 to 9.
    fn node(digit: u8) -> Node {
        Node::new(digit).expect("a digit from 1 to 9")
    }

    /// The digits of the nodes of `drawing`'s trace, in order.
    fn digits(drawing: &Drawing) -> Vec<u8> {
        drawing.trace().nodes().iter().map(|n| n.digit()).collect()
    }

    /// A drawing after pressing `keys` in order.
    fn drawn(keys: &[u8]) -> Drawing {
        let mut drawing = Drawing::new();
        for &digit in keys {
            drawing.link(node(digit));
        }
        drawing
    }

    #[test]
    fn linking_follows_the_rules_of_the_trace() {
        let drawing = drawn(&[7, 3, 6]);

        assert_eq!(digits(&drawing), [7, 5, 3, 6], "7 to 3 links 5 on the way");
        assert_eq!(drawing.notice(), None);
    }

    #[test]
    fn a_node_linked_twice_is_refused_and_told_and_the_next_key_clears_it() {
        let mut drawing = drawn(&[7, 5]);

        drawing.link(node(5));
        assert_eq!(digits(&drawing), [7, 5], "the trace is unchanged");
        assert_eq!(drawing.notice(), Some(Notice::Refused(node(5))));

        drawing.link(node(3));
        assert_eq!(digits(&drawing), [7, 5, 3]);
        assert_eq!(drawing.notice(), None);
    }

    #[test]
    fn undo_removes_only_the_last_node() {
        let mut drawing = drawn(&[8, 5, 2, 1]);

        drawing.undo();
        assert_eq!(digits(&drawing), [8, 5, 2]);
        drawing.undo();
        drawing.undo();
        drawing.undo();
        assert_eq!(digits(&drawing), [] as [u8; 0]);
        drawing.undo();
        assert!(
            drawing.trace().is_empty(),
            "undo on an empty trace is harmless"
        );
    }

    #[test]
    fn undo_keeps_a_middle_node_linked_on_the_way() {
        let mut drawing = drawn(&[7, 3]);

        drawing.undo();

        assert_eq!(digits(&drawing), [7, 5]);
    }

    #[test]
    fn undo_keeps_strokes_that_cross_a_linked_node() {
        // 2 then 8 links 5; 4 then 6 crosses the linked 5 without it.
        let mut drawing = drawn(&[2, 8, 4, 6, 9]);
        assert_eq!(digits(&drawing), [2, 5, 8, 4, 6, 9]);

        drawing.undo();

        assert_eq!(digits(&drawing), [2, 5, 8, 4, 6]);
    }

    #[test]
    fn undo_and_an_empty_cast_update_the_notice() {
        let mut drawing = drawn(&[7, 7]);
        drawing.undo();
        assert_eq!(drawing.notice(), None);

        drawing.refuse_empty();
        assert_eq!(drawing.notice(), Some(Notice::Empty));
    }

    /// A verdict of the trace drawn by pressing `keys`.
    fn verdict(keys: &[u8], quality: CastQuality) -> Verdict {
        Verdict {
            trace: drawn(keys).trace(),
            quality,
            sigil: None,
            unknown: false,
        }
    }

    #[test]
    fn the_fault_of_an_unstable_trace_is_where_it_leaves_the_pattern() {
        let ember = Trace::from_pattern(&[7, 5, 3]).unwrap();
        let unstable = |keys: &[u8]| verdict(keys, CastQuality::Unstable).fault(Some(&ember));

        assert_eq!(unstable(&[7, 5, 2]), Some(node(2)), "a node replaced");
        assert_eq!(unstable(&[7, 4, 5, 3]), Some(node(4)), "a node added");
        assert_eq!(unstable(&[7, 5, 3, 6]), Some(node(6)), "a node added last");
        assert_eq!(unstable(&[7, 5]), None, "the last node missing");
    }

    #[test]
    fn the_fault_of_a_fizzle_is_its_last_node() {
        let fizzled = verdict(&[4, 8, 6], CastQuality::Fizzled);

        assert_eq!(fizzled.fault(None), Some(node(6)));
    }
}
