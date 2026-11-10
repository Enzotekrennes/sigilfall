//! The death summary, shown once the player has died: how the run ended,
//! what the player did, the seed to replay it, and a line to share.
//!
//! Exposes [`render`] and [`MAX_KILL_ROWS`], and to the victory screen
//! (`ui::victory`) the part both end screens share, [`render_summary`]:
//! everything below the lines that say how the run ended. From top to bottom,
//! the screen shows the title, an epitaph chosen from the seed, the cause of
//! death with the depth and the turn, the counts of the run (deepest floor,
//! turns, creatures slain by kind, glyphs learnt, sigils cast by quality,
//! items used), the seed in a frame, the command that replays it, the line to
//! share (`messages::share_line`: the daily date or the seed, then the depth,
//! or "victory" for a won run, turns, kills and casts), and the key help.
//! Every count comes from `Game::stats`, which the game keeps from its
//! events; this module only lays them out.
//!
//! The summary is centred as a block, with the counts aligned on their
//! dots, and fits an 80 × 24 screen; larger screens centre it with more
//! margin. It is drawn with the theme's tokens and glyphs only, so the ASCII
//! glyph set gives a screen of plain ASCII, and colour never carries
//! meaning: in monochrome every line still reads the same. The message log
//! is not shown: the cause line says who dealt the last blow.

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use sigilfall_core::content::Content;
use sigilfall_core::game::{Game, Status};
use sigilfall_core::stats::Stats;

use crate::input;
use crate::language::Language;
use crate::messages;
use crate::theme::Theme;

/// Most rows the list of creatures slain takes, its first row included.
///
/// Four rows keep the whole summary within an 80 × 24 screen with a row of
/// margin above and below. Up to four kinds are listed whole; beyond, the
/// three most slain are listed and the last row says how many kinds are
/// left out ("and 5 more"), which is never less than two.
pub const MAX_KILL_ROWS: usize = 4;

/// Width of a count's label with its leader dots, as in "Turns ........ ",
/// so the values line up in one column.
///
/// The longest label, "Creatures slain", is 15 characters: 22 leaves it at
/// least a space and five dots before its value.
const LABEL_WIDTH: usize = 22;

/// Blank columns between a value and its detail (the creatures slain by
/// kind, the casts by quality).
const DETAIL_GAP: usize = 3;

/// Blank columns on each side of the seed inside its frame.
const SEED_PADDING: usize = 4;

/// Blank columns between the seed's label and the seed inside its frame.
const SEED_LABEL_GAP: usize = 3;

/// Draws the death summary of `game` over `area`: the summary centred in
/// the rows above the last one, and the key help on the last row. Monster
/// names come from `content`.
///
/// The cause line is only written when the player died (`Status::GameOver`);
/// called on a game still in progress or won (a caller's mistake:
/// `ui::render` sends a won game to `ui::victory`), the screen still shows
/// the counts and the seed. The background is expected to be painted already,
/// and an area too small for the text is clipped without panicking.
pub fn render(frame: &mut Frame, area: Rect, game: &Game, content: &Content, theme: &Theme) {
    let mut head = vec![
        Line::styled(theme.labels().death_title, theme.text_title),
        Line::styled(
            messages::epitaph(theme.language, game.seed()),
            theme.text_muted,
        ),
        Line::default(),
    ];
    if let Status::GameOver { killed_by } = game.status() {
        head.push(Line::styled(
            messages::cause_of_death(
                theme.language,
                killed_by,
                game.depth(),
                game.player_turns(),
                content,
            ),
            theme.text,
        ));
        head.push(Line::default());
    }
    render_summary(frame, area, head, game, content, theme);
}

/// Draws an end screen of `game` over `area`: the `head` lines, which say
/// how the run ended, then the summary both end screens share (see
/// [`summary`]), centred as one block in the rows above the last one, and
/// the key help ("Enter title screen   q quit") on the last row. Monster
/// names come from `content`.
///
/// The background is expected to be painted already, and an area too small
/// for the text is clipped without panicking.
pub(super) fn render_summary(
    frame: &mut Frame,
    area: Rect,
    head: Vec<Line<'static>>,
    game: &Game,
    content: &Content,
    theme: &Theme,
) {
    let summary_area = Rect {
        height: area.height.saturating_sub(1),
        ..area
    };
    let mut lines = head;
    lines.extend(summary(game, content, theme));
    let text_height = u16::try_from(lines.len()).unwrap_or(u16::MAX);
    let top = summary_area.height.saturating_sub(text_height) / 2;
    let text_area = Rect {
        y: summary_area.y + top,
        height: summary_area.height - top,
        ..summary_area
    };
    frame.render_widget(
        Paragraph::new(lines).alignment(Alignment::Center),
        text_area,
    );

    let help = super::key_help(
        vec![
            (
                theme.labels().enter_key.to_owned(),
                theme.labels().title_label,
            ),
            (
                input::quit_key(theme.keyboard).to_string(),
                theme.labels().quit_label,
            ),
        ],
        theme,
    );
    let help_area = Rect {
        y: area.bottom().saturating_sub(1),
        height: area.height.min(1),
        ..area
    };
    frame.render_widget(help, help_area);
}

/// The centred lines of the summary shared by both end screens, top to
/// bottom: the counts, the seed in its frame, the replay command and the
/// line to share. Lines of one block (the counts, the frame of the seed)
/// are padded to the same width, so that centring each line keeps the
/// block aligned on its left edge.
fn summary(game: &Game, content: &Content, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = padded(counts(game.stats(), content, theme));
    lines.push(Line::default());
    lines.extend(seed_frame(game, theme));
    lines.push(Line::default());
    lines.push(Line::styled(
        messages::replay_hint(theme.language, game.seed()),
        theme.text_muted,
    ));
    // Right under the replay command, without a blank row, so the summary
    // keeps a row of margin above and below on 80 × 24 with the longest
    // list of kills; in the text style, as the line meant to be copied.
    lines.push(Line::styled(
        messages::share_line(game.daily_date(), game.seed(), game.stats(), game.status()),
        theme.text,
    ));
    lines
}

/// One row of the counts: a label, its value and an optional detail.
struct CountRow {
    /// What is counted; empty on the rows that continue the detail of the
    /// row above.
    label: &'static str,
    /// The count; empty on continuation rows.
    value: String,
    /// What the count is made of, in the muted style; may be empty.
    detail: String,
}

impl CountRow {
    /// A row with a label and a value, and no detail yet.
    fn new(label: &'static str, value: impl ToString) -> Self {
        Self {
            label,
            value: value.to_string(),
            detail: String::new(),
        }
    }

    /// A row that only continues the detail of the row above.
    const fn continuation(detail: String) -> Self {
        Self {
            label: "",
            value: String::new(),
            detail,
        }
    }

    /// Returns the row as a line: the label and its leader dots muted, the
    /// value in the text style, left-aligned in a column `value_width`
    /// characters wide, the detail muted.
    fn line(self, value_width: usize, theme: &Theme) -> Line<'static> {
        let lead = if self.label.is_empty() {
            " ".repeat(LABEL_WIDTH)
        } else {
            let dots = LABEL_WIDTH.saturating_sub(self.label.chars().count() + 2);
            format!("{} {} ", self.label, ".".repeat(dots))
        };
        let mut spans = vec![
            Span::styled(lead, theme.text_muted),
            Span::styled(format!("{:<value_width$}", self.value), theme.text),
        ];
        if !self.detail.is_empty() {
            spans.push(Span::raw(" ".repeat(DETAIL_GAP)));
            spans.push(Span::styled(self.detail, theme.text_muted));
        }
        Line::from(spans)
    }
}

/// The rows of the run's counts: deepest floor, turns, creatures slain with
/// the kinds listed beside and below (see [`kill_details`]), glyphs learnt,
/// sigils cast with their qualities, items used.
fn counts(stats: &Stats, content: &Content, theme: &Theme) -> Vec<Line<'static>> {
    let mut rows = vec![
        CountRow::new(theme.labels().depth_reached_label, stats.deepest()),
        CountRow::new(theme.labels().turns_label, stats.turns()),
    ];
    let mut kills = kill_details(stats, content, theme.language).into_iter();
    rows.push(CountRow {
        detail: kills.next().unwrap_or_default(),
        ..CountRow::new(theme.labels().kills_label, stats.total_kills())
    });
    rows.extend(kills.map(CountRow::continuation));
    rows.push(CountRow::new(
        theme.labels().glyphs_label,
        stats.glyphs_learnt(),
    ));
    let casts = stats.casts();
    rows.push(CountRow {
        detail: if casts.total() == 0 {
            String::new()
        } else {
            messages::cast_breakdown(theme.language, casts, theme.glyphs.separator)
        },
        ..CountRow::new(theme.labels().casts_label, casts.total())
    });
    rows.push(CountRow::new(
        theme.labels().items_used_label,
        stats.items_used(),
    ));
    // The details start in one column, just past the widest value.
    let value_width = rows
        .iter()
        .map(|row| row.value.chars().count())
        .max()
        .unwrap_or(0);
    rows.into_iter()
        .map(|row| row.line(value_width, theme))
        .collect()
}

/// The creatures slain, one kind per row, most slain first (ties by name),
/// as "12 ossuary rat" with the counts right-aligned; at most
/// [`MAX_KILL_ROWS`] rows, the last one saying how many kinds are left out
/// when they do not all fit, named in `language`. Empty when nothing was
/// slain.
fn kill_details(stats: &Stats, content: &Content, language: Language) -> Vec<String> {
    let mut kinds: Vec<(u32, &str)> = stats
        .kills()
        .map(|(definition, count)| {
            let name = messages::monster_count_name(language, content, definition, count);
            (count, name)
        })
        .collect();
    kinds.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    let shown = if kinds.len() > MAX_KILL_ROWS {
        MAX_KILL_ROWS - 1
    } else {
        kinds.len()
    };
    let count_width = kinds
        .iter()
        .take(shown)
        .map(|(count, _)| count.to_string().len())
        .max()
        .unwrap_or(0);
    let mut rows: Vec<String> = kinds
        .iter()
        .take(shown)
        .map(|(count, name)| format!("{count:>count_width$} {name}"))
        .collect();
    if kinds.len() > shown {
        rows.push(messages::more_in_view(language, kinds.len() - shown));
    }
    rows
}

/// The seed in a frame of the theme's corners and lines, in the title
/// style after its muted label: three lines of the same width.
fn seed_frame(game: &Game, theme: &Theme) -> Vec<Line<'static>> {
    let glyphs = theme.glyphs;
    let seed = game.seed().to_string();
    let inner = SEED_PADDING * 2
        + theme.labels().seed_label.chars().count()
        + SEED_LABEL_GAP
        + seed.chars().count();
    let horizontal = glyphs.frame_horizontal.to_string().repeat(inner);
    let vertical = glyphs.frame_vertical.to_string();
    let padding = " ".repeat(SEED_PADDING);
    vec![
        Line::styled(
            format!(
                "{}{horizontal}{}",
                glyphs.frame_top_left, glyphs.frame_top_right
            ),
            theme.border,
        ),
        Line::from(vec![
            Span::styled(vertical.clone(), theme.border),
            Span::raw(padding.clone()),
            Span::styled(theme.labels().seed_label, theme.text_muted),
            Span::raw(" ".repeat(SEED_LABEL_GAP)),
            Span::styled(seed, theme.text_title),
            Span::raw(padding),
            Span::styled(vertical, theme.border),
        ]),
        Line::styled(
            format!(
                "{}{horizontal}{}",
                glyphs.frame_bottom_left, glyphs.frame_bottom_right
            ),
            theme.border,
        ),
    ]
}

/// Pads every line of `lines` with blanks to the width of the widest, so
/// centring them one by one keeps their left edges aligned.
fn padded(lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    let width = lines.iter().map(Line::width).max().unwrap_or(0);
    lines
        .into_iter()
        .map(|mut line| {
            let missing = width - line.width();
            if missing > 0 {
                line.spans
                    .push(Span::styled(" ".repeat(missing), Style::default()));
            }
            line
        })
        .collect()
}
