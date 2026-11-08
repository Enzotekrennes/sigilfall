//! The message log under the map: a titled separator, then the most recent
//! messages, oldest at the top, each with the prefix and colour of its tone.
//!
//! Exposes [`render`] and the wrapping width of a message
//! ([`TEXT_WIDTH`]). The messages come from the application's
//! [`MessageLog`]; this module only lays them out. It scrolls by itself:
//! the newest messages always show, and those that no longer fit scroll out
//! at the top, whole, so every line shown belongs to a message whose prefix
//! is visible.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget as _;

use crate::message_log::{Message, MessageLog, Tone};
use crate::messages;
use crate::theme::Theme;

/// Columns a message's text is wrapped to.
///
/// `docs/design.md` caps a log line at 78 characters on an 80-column
/// screen: a margin, the prefix and a space take three columns, which
/// leaves 76 for the text and keeps the right margin free. The width stays
/// the same on wider terminals, so lines remain easy to read.
pub const TEXT_WIDTH: usize = 76;

/// Columns before a message's text: the left margin, the prefix and a
/// space. Continuation lines are indented by as much, so the prefix column
/// stays clear.
const INDENT: &str = "   ";

/// Draws the log in `area` of `buffer`: the separator on its first row,
/// with the side panel's border meeting it at column `junction` when there
/// is a panel above, then as many of the newest messages of `log` as the
/// remaining rows hold.
///
/// Messages are wrapped to [`TEXT_WIDTH`] columns and shown whole, oldest
/// first, from the second row down. A message longer than the whole log
/// area (not possible with the game's sentences) shows its first lines. An
/// empty `area` draws nothing.
pub fn render(
    buffer: &mut Buffer,
    area: Rect,
    junction: Option<u16>,
    log: &MessageLog,
    theme: &Theme,
) {
    if area.is_empty() {
        return;
    }
    separator(area.width, area.x, junction, theme).render(Rect { height: 1, ..area }, buffer);
    let rows = usize::from(area.height - 1);
    for (offset, line) in (1..).zip(visible_lines(log, rows, theme)) {
        line.render(
            Rect {
                y: area.y + offset,
                height: 1,
                ..area
            },
            buffer,
        );
    }
}

/// Builds the separator above the log, `width` columns starting at column
/// `left`: a line in the border style with the title "Log" near its left
/// end and, when `junction` is given, a junction at that column, where the
/// side panel's border comes down, as in the game screen mock-up of
/// `docs/design.md`.
fn separator(width: u16, left: u16, junction: Option<u16>, theme: &Theme) -> Line<'static> {
    let glyphs = &theme.glyphs;
    let title = format!(" {} ", messages::LOG_TITLE);
    let title_end = 1 + title.chars().count();
    let junction = junction.map(|x| usize::from(x.saturating_sub(left)));
    let rule: String = (title_end..usize::from(width))
        .map(|column| {
            if Some(column) == junction {
                glyphs.frame_junction_up
            } else {
                glyphs.frame_horizontal
            }
        })
        .collect();
    Line::from(vec![
        Span::styled(glyphs.frame_horizontal.to_string(), theme.border),
        Span::styled(title, theme.text_muted),
        Span::styled(rule, theme.border),
    ])
}

/// The lines of the newest messages of `log` that fit in `rows` rows,
/// oldest first.
fn visible_lines(log: &MessageLog, rows: usize, theme: &Theme) -> Vec<Line<'static>> {
    let mut shown: Vec<Vec<Line<'static>>> = Vec::new();
    let mut used = 0;
    for message in log.newest_first() {
        let lines = message_lines(message, theme);
        if used + lines.len() > rows {
            if shown.is_empty() {
                // Too long for the whole area: keep its beginning.
                shown.push(lines.into_iter().take(rows).collect());
            }
            break;
        }
        used += lines.len();
        shown.push(lines);
    }
    shown.into_iter().rev().flatten().collect()
}

/// The screen lines of `message`: its prefix and the start of its text on
/// the first, the rest of the text wrapped to [`TEXT_WIDTH`] and indented
/// under it, all in the style of its tone.
fn message_lines(message: &Message, theme: &Theme) -> Vec<Line<'static>> {
    let (prefix, style) = tone_look(message.tone, theme);
    wrap(&message.text, TEXT_WIDTH)
        .into_iter()
        .enumerate()
        .map(|(index, text)| {
            let lead = if index == 0 {
                Span::styled(format!(" {prefix} "), style)
            } else {
                Span::raw(INDENT)
            };
            Line::from(vec![lead, Span::styled(text, style)])
        })
        .collect()
}

/// The prefix and style of a message of `tone` (`docs/design.md`,
/// "Préfixes du journal"): the prefix carries the tone without colour.
const fn tone_look(tone: Tone, theme: &Theme) -> (char, Style) {
    match tone {
        Tone::Info => (' ', theme.log_info),
        Tone::Success => ('+', theme.log_success),
        Tone::Danger => ('!', theme.log_danger),
        Tone::Sigil => ('~', theme.log_sigil),
    }
}

/// Splits `text` into lines of at most `width` characters, breaking
/// between words; a word longer than `width` is cut. Runs of spaces count
/// as one break. Returns one empty line for an empty text, and no line is
/// ever empty otherwise. The inventory wraps its notices with it too.
pub(super) fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let mut word: Vec<char> = word.chars().collect();
        while word.len() > width {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            let rest = word.split_off(width);
            lines.push(word.into_iter().collect());
            word = rest;
        }
        let needed = if current.is_empty() {
            word.len()
        } else {
            current.chars().count() + 1 + word.len()
        };
        if needed > width {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.extend(word);
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapping_breaks_between_words_within_the_width() {
        assert_eq!(wrap("a bb ccc dd", 6), ["a bb", "ccc dd"]);
        assert_eq!(wrap("exactly six", 11), ["exactly six"]);
        assert_eq!(wrap("", 10), [""]);
    }

    #[test]
    fn a_word_longer_than_the_width_is_cut() {
        assert_eq!(wrap("ab abcdefgh c", 4), ["ab", "abcd", "efgh", "c"]);
    }

    #[test]
    fn every_wrapped_line_fits_and_no_word_is_lost() {
        let text = "The lead homunculus hits you (6). ".repeat(9);
        let lines = wrap(&text, TEXT_WIDTH);

        assert!(lines.len() > 1);
        for line in &lines {
            assert!(line.chars().count() <= TEXT_WIDTH, "{line:?}");
            assert!(!line.is_empty());
        }
        assert_eq!(
            lines.join(" ").split_whitespace().collect::<Vec<_>>(),
            text.split_whitespace().collect::<Vec<_>>()
        );
    }
}
