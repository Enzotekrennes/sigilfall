//! The grimoire as the sigil overlay shows it: every sigil the player
//! knows, with its name, its pattern and what it does, or how to learn one
//! while it is empty.
//!
//! Exposes [`lines`]. Decision D12 keeps the grimoire in view while the
//! player traces, so the difficulty lies in choosing a sigil and drawing it
//! without a slip, not in remembering it. The sigils come in the order of
//! the sigil file, as `Grimoire` keeps them. This module only builds text
//! lines; `ui::sigil` places them in the overlay.

use ratatui::text::{Line, Span};
use sigilfall_core::sigil::Grimoire;

use crate::messages;
use crate::theme::Theme;

/// Columns a sigil's name may take in an entry; a longer name is cut.
///
/// The longest shipped name is 7 characters ("Passage"); content names may
/// reach 24 (`content::MAX_NAME_LENGTH`), which would leave no room for the
/// pattern and the effect in the overlay's 36 columns.
const NAME_COLUMNS: usize = 10;

/// Blank columns between the name, the pattern and the effect of an entry.
const GAP: usize = 2;

/// Builds the grimoire in at most `rows` lines of `width` columns: the
/// title, then one entry per sigil of `grimoire`, or the empty state.
///
/// An entry is the sigil's name (`text`), its pattern as the player presses
/// it ("7·5·3", `sigil_stroke`) and what it does ("harm 6 · nearest",
/// `text_muted`), in columns aligned on the longest name and pattern of the
/// grimoire; text that does not fit `width` is cut. When the sigils
/// outnumber the rows, the last row says how many are not shown. An empty
/// grimoire explains, under the title, that sigils are learnt by stepping
/// on the glyphs carved in the floor, with the map glyph of an unread one.
/// `rows` of 0 gives no line.
pub fn lines(grimoire: &Grimoire, rows: usize, width: u16, theme: &Theme) -> Vec<Line<'static>> {
    if rows == 0 {
        return Vec::new();
    }
    let mut lines = vec![Line::styled(messages::GRIMOIRE_TITLE, theme.text_muted)];
    let room = rows - 1;
    if grimoire.is_empty() {
        lines.extend(
            messages::EMPTY_GRIMOIRE
                .iter()
                .take(room)
                .map(|text| empty_state_line(text, theme)),
        );
        return lines;
    }

    let separator = theme.glyphs.separator;
    let entries: Vec<(String, String, String)> = grimoire
        .sigils()
        .iter()
        .map(|sigil| {
            let name: String = sigil.name().chars().take(NAME_COLUMNS).collect();
            let pattern = messages::pattern_text(sigil.pattern().nodes(), separator);
            (name, pattern, messages::effect_summary(sigil, separator))
        })
        .collect();
    let name_width = column_width(entries.iter().map(|entry| &entry.0));
    let pattern_width = column_width(entries.iter().map(|entry| &entry.1));
    let overflows = entries.len() > room;
    let shown = if overflows {
        room.saturating_sub(1)
    } else {
        entries.len()
    };
    let hidden = entries.len() - shown;
    lines.extend(
        entries
            .into_iter()
            .take(shown)
            .map(|(name, pattern, effect)| {
                cut(
                    Line::from(vec![
                        Span::styled(padded(&name, name_width), theme.text),
                        Span::styled(padded(&pattern, pattern_width), theme.sigil_stroke),
                        Span::styled(effect, theme.text_muted),
                    ]),
                    width,
                )
            }),
    );
    if overflows && room > 0 {
        lines.push(Line::styled(
            messages::more_in_view(hidden),
            theme.text_muted,
        ));
    }
    lines
}

/// One line of the empty state, in `text_muted`, with the `{glyph}` mark
/// replaced by the map glyph of an unread engraving in its own style. The
/// verdict of an unknown pattern (`ui::sigil`) tells how to learn sigils
/// with it too.
pub(super) fn empty_state_line(text: &str, theme: &Theme) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, part) in text.split("{glyph}").enumerate() {
        if index > 0 {
            spans.push(Span::styled(
                theme.glyphs.engraving_unread.to_string(),
                theme.engraving_unread,
            ));
        }
        spans.push(Span::styled(part.to_owned(), theme.text_muted));
    }
    Line::from(spans)
}

/// Width of a column of `texts`: the longest, in characters.
fn column_width<'text>(texts: impl Iterator<Item = &'text String>) -> usize {
    texts.map(|text| text.chars().count()).max().unwrap_or(0)
}

/// `text` followed by spaces up to `width` characters, then [`GAP`] more.
fn padded(text: &str, width: usize) -> String {
    let fill = width.saturating_sub(text.chars().count()) + GAP;
    format!("{text}{}", " ".repeat(fill))
}

/// `line` cut to `width` characters, span by span, styles kept.
fn cut(line: Line<'static>, width: u16) -> Line<'static> {
    let mut room = usize::from(width);
    let spans = line
        .spans
        .into_iter()
        .filter_map(|span| {
            if room == 0 {
                return None;
            }
            let text: String = span.content.chars().take(room).collect();
            room -= text.chars().count();
            Some(Span::styled(text, span.style))
        })
        .collect::<Vec<_>>();
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use sigilfall_core::content::Content;

    use super::*;
    use crate::theme::{ColorMode, GlyphSet};

    /// The dark theme with `set` glyphs.
    fn theme(set: GlyphSet) -> Theme {
        Theme::new(ColorMode::TrueColorDark, set)
    }

    /// A grimoire that knows every shipped sigil whose identifier is in
    /// `ids`.
    fn knowing(ids: &[&str]) -> Grimoire {
        let content = Content::load().expect("the shipped content is valid");
        let mut grimoire = Grimoire::new();
        for id in ids {
            grimoire.learn(content.sigil(id).expect("a shipped sigil"));
        }
        grimoire
    }

    /// The text of `lines`, one string per line.
    fn texts(lines: &[Line<'_>]) -> Vec<String> {
        lines.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn each_known_sigil_shows_its_name_pattern_and_effect_in_file_order() {
        let lines = lines(
            &knowing(&["recall", "ember"]),
            7,
            36,
            &theme(GlyphSet::Unicode),
        );

        assert_eq!(
            texts(&lines),
            [
                "GRIMOIRE",
                "Ember   7·5·3    harm 6 · nearest",
                "Recall  8·5·2·1  heal 8 · self",
            ]
        );
    }

    #[test]
    fn the_pattern_is_in_the_stroke_colour_and_the_effect_muted() {
        let theme = theme(GlyphSet::Unicode);
        let lines = lines(&knowing(&["ember"]), 7, 35, &theme);

        let entry = &lines[1].spans;
        assert_eq!(entry[0].style, theme.text);
        assert_eq!(entry[1].style, theme.sigil_stroke);
        assert_eq!(entry[2].style, theme.text_muted);
    }

    #[test]
    fn an_empty_grimoire_tells_how_to_learn_a_sigil() {
        let theme = theme(GlyphSet::Ascii);
        let lines = lines(&Grimoire::new(), 7, 35, &theme);

        assert_eq!(
            texts(&lines),
            [
                "GRIMOIRE",
                "No sigil known yet.",
                "Step on a glyph carved",
                "in the floor, & on the",
                "map, to learn its sigil.",
            ]
        );
        assert_eq!(lines[3].spans[1].style, theme.engraving_unread);
    }

    #[test]
    fn sigils_beyond_the_rows_are_counted_on_the_last_row() {
        let all = knowing(&["ember", "recall", "veil", "passage", "lance"]);

        let lines = lines(&all, 4, 35, &theme(GlyphSet::Unicode));

        let texts = texts(&lines);
        assert_eq!(texts.len(), 4);
        assert_eq!(texts[3], "and 3 more");
    }

    #[test]
    fn entries_are_cut_to_the_width() {
        let all = knowing(&["ember", "recall", "veil", "passage", "lance"]);

        for width in [0, 5, 20, 35] {
            for line in lines(&all, 7, width, &theme(GlyphSet::Unicode))
                .iter()
                .skip(1)
            {
                assert!(line.width() <= usize::from(width), "{width}: {line}");
            }
        }
    }
}
