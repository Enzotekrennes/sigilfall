//! The characters the map and the interface are drawn with, in Unicode and
//! in their ASCII fallback.
//!
//! Exposes [`GlyphSet`] and [`Glyphs`]. The values come from section
//! "Glyphes" of `docs/design.md`; every Unicode glyph there is present in
//! Cascadia Mono, the default font of Windows Terminal. Only the glyphs of
//! elements the game already has are listed: the others (arches,
//! engravings, items, sigil grid, rounded frames) join this table with the
//! tasks that bring them to the screen.
//!
//! Creatures are drawn with the letter of their family, lower case for the
//! common form and upper case for the elite one ([`Glyphs::creature`]).
//! These letters are plain ASCII, so they are the same in both sets, and
//! they alone tell the families apart when the terminal shows no colour.

use sigilfall_core::content::Family;

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
    /// Stairs down: `▼`, or `>` in ASCII.
    pub stairs: char,
    /// Common vermin: `v` in both sets; `V` for the elite form.
    pub vermin: char,
    /// Common acolyte: `a` in both sets; `A` for the elite form.
    pub acolyte: char,
    /// Common homunculus: `h` in both sets; `H` for the elite form.
    pub homunculus: char,
    /// Common eye: `e` in both sets; `E` for the elite form.
    pub eye: char,
    /// Common spectre: `s` in both sets; `S` for the elite form.
    pub spectre: char,
    /// Left arrow of the key help: `←`, or `<` in ASCII.
    pub arrow_left: char,
    /// Up arrow of the key help: `↑`, or `^` in ASCII.
    pub arrow_up: char,
    /// Right arrow of the key help: `→`, or `>` in ASCII.
    pub arrow_right: char,
    /// Down arrow of the key help: `↓`, or `v` in ASCII.
    pub arrow_down: char,
    /// Sign between the two terms of a size, as in "80 × 24", and before a
    /// count, as in "×2": `×`, or `x` in ASCII.
    pub dimension: char,
    /// Horizontal line of frames and separators: `─`, or `-` in ASCII.
    pub frame_horizontal: char,
    /// Vertical line of frames and separators: `│`, or `|` in ASCII.
    pub frame_vertical: char,
    /// Junction where a vertical line meets a horizontal one from above:
    /// `┴`, or `+` in ASCII.
    pub frame_junction_up: char,
    /// Filled cell of the health bar: `█`, or `=` in ASCII.
    pub bar_full: char,
    /// Empty cell of the health bar: `░`, or `-` in ASCII.
    pub bar_empty: char,
}

impl Glyphs {
    /// Returns the glyphs of `set`.
    pub const fn new(set: GlyphSet) -> Self {
        match set {
            GlyphSet::Unicode => Self {
                floor: '·',
                wall: '▒',
                player: '@',
                stairs: '▼',
                vermin: 'v',
                acolyte: 'a',
                homunculus: 'h',
                eye: 'e',
                spectre: 's',
                arrow_left: '←',
                arrow_up: '↑',
                arrow_right: '→',
                arrow_down: '↓',
                dimension: '×',
                frame_horizontal: '─',
                frame_vertical: '│',
                frame_junction_up: '┴',
                bar_full: '█',
                bar_empty: '░',
            },
            GlyphSet::Ascii => Self {
                floor: '.',
                wall: '#',
                player: '@',
                stairs: '>',
                vermin: 'v',
                acolyte: 'a',
                homunculus: 'h',
                eye: 'e',
                spectre: 's',
                arrow_left: '<',
                arrow_up: '^',
                arrow_right: '>',
                arrow_down: 'v',
                dimension: 'x',
                frame_horizontal: '-',
                frame_vertical: '|',
                frame_junction_up: '+',
                bar_full: '=',
                bar_empty: '-',
            },
        }
    }

    /// Returns the letter a creature of `family` is drawn with: the
    /// family's lower-case letter for the common form, the same letter in
    /// upper case when `elite` is true (`docs/design.md`, "Créatures").
    pub const fn creature(&self, family: Family, elite: bool) -> char {
        let common = match family {
            Family::Vermin => self.vermin,
            Family::Acolyte => self.acolyte,
            Family::Homunculus => self.homunculus,
            Family::Eye => self.eye,
            Family::Spectre => self.spectre,
        };
        if elite {
            common.to_ascii_uppercase()
        } else {
            common
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
    fn all(glyphs: Glyphs) -> [(&'static str, char); 19] {
        [
            ("floor", glyphs.floor),
            ("wall", glyphs.wall),
            ("player", glyphs.player),
            ("stairs", glyphs.stairs),
            ("vermin", glyphs.vermin),
            ("acolyte", glyphs.acolyte),
            ("homunculus", glyphs.homunculus),
            ("eye", glyphs.eye),
            ("spectre", glyphs.spectre),
            ("arrow_left", glyphs.arrow_left),
            ("arrow_up", glyphs.arrow_up),
            ("arrow_right", glyphs.arrow_right),
            ("arrow_down", glyphs.arrow_down),
            ("dimension", glyphs.dimension),
            ("frame_horizontal", glyphs.frame_horizontal),
            ("frame_vertical", glyphs.frame_vertical),
            ("frame_junction_up", glyphs.frame_junction_up),
            ("bar_full", glyphs.bar_full),
            ("bar_empty", glyphs.bar_empty),
        ]
    }

    #[test]
    fn unicode_glyphs_are_the_code_points_of_the_design() {
        let glyphs = Glyphs::new(GlyphSet::Unicode);

        let expected = [
            ("floor", '\u{00B7}'),
            ("wall", '\u{2592}'),
            ("player", '\u{0040}'),
            ("stairs", '\u{25BC}'),
            ("vermin", 'v'),
            ("acolyte", 'a'),
            ("homunculus", 'h'),
            ("eye", 'e'),
            ("spectre", 's'),
            ("arrow_left", '\u{2190}'),
            ("arrow_up", '\u{2191}'),
            ("arrow_right", '\u{2192}'),
            ("arrow_down", '\u{2193}'),
            ("dimension", '\u{00D7}'),
            ("frame_horizontal", '\u{2500}'),
            ("frame_vertical", '\u{2502}'),
            ("frame_junction_up", '\u{2534}'),
            ("bar_full", '\u{2588}'),
            ("bar_empty", '\u{2591}'),
        ];
        assert_eq!(all(glyphs), expected);
    }

    #[test]
    fn ascii_fallback_is_printable_ascii_only() {
        for (name, glyph) in all(Glyphs::new(GlyphSet::Ascii)) {
            assert!(glyph.is_ascii_graphic(), "{name}: {glyph:?}");
        }
    }

    /// The five creature families, in the order of `docs/design.md`.
    const FAMILIES: [Family; 5] = [
        Family::Vermin,
        Family::Acolyte,
        Family::Homunculus,
        Family::Eye,
        Family::Spectre,
    ];

    #[test]
    fn map_glyphs_are_distinct_in_both_sets() {
        for set in [GlyphSet::Unicode, GlyphSet::Ascii] {
            let glyphs = Glyphs::new(set);
            let mut map = vec![glyphs.floor, glyphs.wall, glyphs.player, glyphs.stairs];
            for family in FAMILIES {
                map.push(glyphs.creature(family, false));
                map.push(glyphs.creature(family, true));
            }
            for (i, a) in map.iter().enumerate() {
                assert!(!map[i + 1..].contains(a), "{set:?}: {a:?} used twice");
            }
        }
    }

    #[test]
    fn creatures_take_their_family_letter_upper_case_when_elite() {
        let expected = [
            (Family::Vermin, 'v', 'V'),
            (Family::Acolyte, 'a', 'A'),
            (Family::Homunculus, 'h', 'H'),
            (Family::Eye, 'e', 'E'),
            (Family::Spectre, 's', 'S'),
        ];
        for set in [GlyphSet::Unicode, GlyphSet::Ascii] {
            let glyphs = Glyphs::new(set);
            for (family, common, elite) in expected {
                assert_eq!(glyphs.creature(family, false), common, "{set:?}");
                assert_eq!(glyphs.creature(family, true), elite, "{set:?}");
            }
        }
    }

    #[test]
    fn arrows_read_left_up_right_down() {
        assert_eq!(Glyphs::new(GlyphSet::Unicode).arrows(), "←↑→↓");
        assert_eq!(Glyphs::new(GlyphSet::Ascii).arrows(), "<^>v");
    }
}
