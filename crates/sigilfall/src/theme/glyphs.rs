//! The characters the map and the interface are drawn with, in Unicode and
//! in their ASCII fallback.
//!
//! Exposes [`GlyphSet`] and [`Glyphs`]. The values come from section
//! "Glyphes" of `docs/design.md`; every Unicode glyph there is present in
//! Cascadia Mono, the default font of Windows Terminal. Only the glyphs of
//! elements the game already has are listed: the others (arches, stairs,
//! engravings, creatures, items, sigil grid, frames) join this table with
//! the tasks that bring them to the screen.

/// Which character set to draw with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlyphSet {
    /// Unicode glyphs (the default): stone walls, middle dots, arrows.
    Unicode,
    /// Plain ASCII, for fonts that lack the Unicode glyphs and for
    /// terminals that draw ambiguous-width characters on two cells (CJK
    /// locales). Selected by the `--ascii` flag.
    Ascii,
}

/// The character for each element of the screen, in one [`GlyphSet`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyphs {
    /// Floor in view: `·`, or `.` in ASCII.
    pub floor: char,
    /// Wall: `▒`, or `#` in ASCII.
    pub wall: char,
    /// The player: `@` in both sets.
    pub player: char,
    /// Left arrow of the key help: `←`, or `<` in ASCII.
    pub arrow_left: char,
    /// Up arrow of the key help: `↑`, or `^` in ASCII.
    pub arrow_up: char,
    /// Right arrow of the key help: `→`, or `>` in ASCII.
    pub arrow_right: char,
    /// Down arrow of the key help: `↓`, or `v` in ASCII.
    pub arrow_down: char,
    /// Sign between the two terms of a size, as in "80 × 24": `×`, or `x`
    /// in ASCII.
    pub dimension: char,
}

impl Glyphs {
    /// Returns the glyphs of `set`.
    pub const fn new(set: GlyphSet) -> Self {
        match set {
            GlyphSet::Unicode => Self {
                floor: '·',
                wall: '▒',
                player: '@',
                arrow_left: '←',
                arrow_up: '↑',
                arrow_right: '→',
                arrow_down: '↓',
                dimension: '×',
            },
            GlyphSet::Ascii => Self {
                floor: '.',
                wall: '#',
                player: '@',
                arrow_left: '<',
                arrow_up: '^',
                arrow_right: '>',
                arrow_down: 'v',
                dimension: 'x',
            },
        }
    }

    /// Returns the four movement arrows as one key name, in the order
    /// left, up, right, down (`←↑→↓`), as the key help shows them.
    pub fn arrows(&self) -> String {
        [
            self.arrow_left,
            self.arrow_up,
            self.arrow_right,
            self.arrow_down,
        ]
        .iter()
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every glyph of `glyphs`, with its field name for failure messages.
    fn all(glyphs: Glyphs) -> [(&'static str, char); 8] {
        [
            ("floor", glyphs.floor),
            ("wall", glyphs.wall),
            ("player", glyphs.player),
            ("arrow_left", glyphs.arrow_left),
            ("arrow_up", glyphs.arrow_up),
            ("arrow_right", glyphs.arrow_right),
            ("arrow_down", glyphs.arrow_down),
            ("dimension", glyphs.dimension),
        ]
    }

    #[test]
    fn unicode_glyphs_are_the_code_points_of_the_design() {
        let glyphs = Glyphs::new(GlyphSet::Unicode);

        let expected = [
            ("floor", '\u{00B7}'),
            ("wall", '\u{2592}'),
            ("player", '\u{0040}'),
            ("arrow_left", '\u{2190}'),
            ("arrow_up", '\u{2191}'),
            ("arrow_right", '\u{2192}'),
            ("arrow_down", '\u{2193}'),
            ("dimension", '\u{00D7}'),
        ];
        assert_eq!(all(glyphs), expected);
    }

    #[test]
    fn ascii_fallback_is_printable_ascii_only() {
        for (name, glyph) in all(Glyphs::new(GlyphSet::Ascii)) {
            assert!(glyph.is_ascii_graphic(), "{name}: {glyph:?}");
        }
    }

    #[test]
    fn map_glyphs_are_distinct_in_both_sets() {
        for set in [GlyphSet::Unicode, GlyphSet::Ascii] {
            let glyphs = Glyphs::new(set);
            let map = [glyphs.floor, glyphs.wall, glyphs.player];
            for (i, a) in map.iter().enumerate() {
                assert!(!map[i + 1..].contains(a), "{set:?}: {a:?} used twice");
            }
        }
    }

    #[test]
    fn arrows_read_left_up_right_down() {
        assert_eq!(Glyphs::new(GlyphSet::Unicode).arrows(), "←↑→↓");
        assert_eq!(Glyphs::new(GlyphSet::Ascii).arrows(), "<^>v");
    }
}
