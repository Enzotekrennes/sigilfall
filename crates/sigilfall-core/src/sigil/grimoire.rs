//! The grimoire: the sigils the player knows from the start of the run or
//! has learnt from engraved glyphs.
//!
//! Exposes [`Grimoire`]. A trace is only ever recognised against the sigils
//! of the grimoire, so a perfectly drawn sigil the player has not learnt yet
//! fizzles. Where glyphs lie and when they are read is the game's job.

use crate::content::SigilDef;

/// The sigils the player knows, in the order of the sigil file.
///
/// The order never depends on the order the sigils were learnt in: it is
/// [`SigilDef::index`], the order `sigil::recognize` expects and the one the
/// grimoire is shown in. A sigil is known at most once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Grimoire {
    /// Known sigils, sorted by [`SigilDef::index`], each identifier once.
    sigils: Vec<SigilDef>,
}

impl Grimoire {
    /// Returns an empty grimoire.
    pub const fn new() -> Self {
        Self { sigils: Vec::new() }
    }

    /// Returns the grimoire a new run starts with: the sigils of `sigils`
    /// (normally `Content::sigils`) that are `SigilDef::known_at_start`, in
    /// file order; empty when none is.
    pub fn for_new_run(sigils: &[SigilDef]) -> Self {
        let mut grimoire = Self::new();
        for sigil in sigils.iter().filter(|sigil| sigil.known_at_start()) {
            grimoire.learn(sigil);
        }
        grimoire
    }

    /// Adds `sigil` to the grimoire and returns whether it was new.
    ///
    /// A sigil already known (same identifier) leaves the grimoire unchanged
    /// and gives `false`. A new one takes its place in file order.
    pub fn learn(&mut self, sigil: &SigilDef) -> bool {
        if self.knows(sigil.id()) {
            return false;
        }
        let place = self
            .sigils
            .partition_point(|known| known.index() < sigil.index());
        self.sigils.insert(place, sigil.clone());
        true
    }

    /// Returns whether the sigil identified by `id` is known.
    pub fn knows(&self, id: &str) -> bool {
        self.sigils.iter().any(|known| known.id() == id)
    }

    /// Returns the known sigils, in the order of the sigil file.
    pub fn sigils(&self) -> &[SigilDef] {
        &self.sigils
    }

    /// Returns the number of known sigils.
    pub fn len(&self) -> usize {
        self.sigils.len()
    }

    /// Returns whether no sigil is known yet.
    pub fn is_empty(&self) -> bool {
        self.sigils.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::Content;

    /// Identifiers of the sigils of `grimoire`, in order.
    fn ids(grimoire: &Grimoire) -> Vec<&str> {
        grimoire.sigils().iter().map(SigilDef::id).collect()
    }

    #[test]
    fn a_new_grimoire_knows_nothing() {
        let grimoire = Grimoire::new();

        assert!(grimoire.is_empty());
        assert_eq!(grimoire.len(), 0);
        assert!(!grimoire.knows("ember"));
    }

    #[test]
    fn a_new_run_knows_only_the_sigils_known_at_start() {
        let content = Content::load().expect("the shipped content is valid");

        let grimoire = Grimoire::for_new_run(content.sigils());

        assert_eq!(ids(&grimoire), ["ember"]);
        assert!(!grimoire.knows("recall"));
        assert!(Grimoire::for_new_run(&[]).is_empty());
    }

    #[test]
    fn sigils_follow_the_file_order_whatever_the_learning_order() {
        let content = Content::load().expect("the shipped content is valid");
        let file_order: Vec<&str> = content.sigils().iter().map(SigilDef::id).collect();
        let mut grimoire = Grimoire::new();

        for sigil in content.sigils().iter().rev() {
            assert!(grimoire.learn(sigil), "{} is new", sigil.id());
        }

        assert_eq!(ids(&grimoire), file_order);
        assert_eq!(grimoire.len(), content.sigils().len());
    }

    #[test]
    fn learning_a_known_sigil_changes_nothing() {
        let content = Content::load().expect("the shipped content is valid");
        let ember = content.sigil("ember").expect("shipped sigil");
        let mut grimoire = Grimoire::new();

        assert!(grimoire.learn(ember));
        let before = grimoire.clone();

        assert!(!grimoire.learn(ember));
        assert_eq!(grimoire, before);
        assert!(grimoire.knows("ember"));
        assert_eq!(ids(&grimoire), ["ember"]);
    }
}
