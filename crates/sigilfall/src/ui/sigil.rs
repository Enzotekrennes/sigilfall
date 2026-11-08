//! The sigil overlay, drawn over the map while the player casts: the 3 × 3
//! grid with the trace stroke by stroke, the grimoire, the time the trace
//! will take and the essence, then the direction prompt and the verdict of
//! an imperfect cast.
//!
//! Exposes [`render`] and the overlay's size ([`OVERLAY_WIDTH`],
//! [`OVERLAY_HEIGHT`]). The layout follows the "Surcouche de tracé de
//! sigil" mock-up of `docs/design.md`: a `bg_panel` frame over the map (the
//! side panel and the log stay in view), the grid on the left laid out like
//! a numeric keypad, 6 columns and 3 rows between two nodes so diagonals
//! fall at 45°, and the grimoire on the right (`ui::grimoire`). Under them,
//! the trace so far, its cost in time, the essence and a notice. A verdict
//! replaces the grimoire with the outcome: the word UNSTABLE or FAILED, the
//! pattern expected and the one traced, and the faulty node drawn `×`.
//!
//! Every state is readable without colour: free, linked, pen and faulty
//! nodes have four glyphs, and verdicts and warnings are written out.

use std::collections::BTreeMap;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget as _;
use sigilfall_core::content::SigilDef;
use sigilfall_core::game::Game;
use sigilfall_core::sigil::{CastQuality, ESSENCE_COST, Node, Trace};

use super::grimoire;
use crate::casting::{Casting, Notice, Verdict};
use crate::messages;
use crate::theme::Theme;

/// Width of the overlay, frame included.
///
/// The map's width on an 80-column screen (58 columns) but one, so the
/// frame never touches the side panel's border: the mock-up's 50 columns
/// plus room for the effect of each sigil in the grimoire, which decision
/// D12 asks to show with its name and pattern. Wider terminals show the
/// map around it.
pub const OVERLAY_WIDTH: u16 = 57;

/// Height of the overlay, frame included: the grid's 7 rows, the 4 lines
/// of trace, time, essence and notice, and the blank rows around them,
/// which fits the 16 rows of the map on a 24-row screen.
pub const OVERLAY_HEIGHT: u16 = 15;

/// Columns between two nodes of a row of the grid, as in the mock-up.
const NODE_STEP_X: u16 = 6;

/// Rows between two nodes of a column of the grid, half of
/// [`NODE_STEP_X`] since a terminal cell is about twice as tall as wide:
/// diagonals then fall at 45°.
const NODE_STEP_Y: u16 = 3;

/// Nodes on one side of the grid.
const GRID_SIDE: u16 = 3;

/// Columns of the grid, from the left node to the right one.
const GRID_WIDTH: u16 = NODE_STEP_X * (GRID_SIDE - 1) + 1;

/// Rows of the grid, from the top node to the bottom one.
const GRID_HEIGHT: u16 = NODE_STEP_Y * (GRID_SIDE - 1) + 1;

/// Column of the grid's left nodes inside the frame, as in the mock-up.
const GRID_LEFT: u16 = 2;

/// Row of the grid's top nodes inside the frame, under one blank row.
const GRID_TOP: u16 = 1;

/// Column of the grimoire and of the verdict inside the frame: three blank
/// columns right of the grid (one less than the mock-up, for the effects
/// of the grimoire).
const COLUMN_LEFT: u16 = GRID_LEFT + GRID_WIDTH + 3;

/// Row of the first line under the grid (trace, time, essence, notice),
/// one blank row below it.
const INFO_TOP: u16 = GRID_TOP + GRID_HEIGHT + 1;

/// Blank columns kept inside the right edge of the frame.
const RIGHT_MARGIN: u16 = 1;

/// Columns a label takes before its value under the grid ("Essence" and
/// two spaces).
const LABEL_WIDTH: usize = 9;

/// Columns a label takes before its value in a verdict ("expected" and two
/// spaces).
const VERDICT_LABEL_WIDTH: usize = 10;

/// What a grid cell between two nodes shows: a piece of stroke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stroke {
    /// Between two nodes of a row.
    Horizontal,
    /// Between two nodes of a column.
    Vertical,
    /// Going up to the right.
    Rising,
    /// Going down to the right.
    Falling,
    /// A rising and a falling stroke on the same cell.
    Cross,
}

/// How the nodes and strokes of a trace are drawn on the grid.
#[derive(Debug, Clone, Copy)]
struct GridLook {
    /// Style of the strokes and of the linked nodes.
    stroke: Style,
    /// Whether the last node is the pen (`◉`), as while tracing.
    pen: bool,
    /// Node drawn `×` in the stroke style, on a verdict.
    fault: Option<Node>,
}

/// Draws the overlay of `casting` over the map `area` of `buffer`, for the
/// player of `game`, with `theme`.
///
/// The overlay is [`OVERLAY_WIDTH`] × [`OVERLAY_HEIGHT`], centred in `area`
/// and cut to it; on an area too small for a frame, nothing is drawn. The
/// sigils it names are those of the player's grimoire.
pub fn render(buffer: &mut Buffer, area: Rect, casting: &Casting, game: &Game, theme: &Theme) {
    let frame = overlay_area(area);
    if frame.width < 2 || frame.height < 2 {
        return;
    }
    let title = match casting {
        Casting::Drawing(_) => messages::TRACE_TITLE.to_owned(),
        Casting::Aiming { sigil, .. } => messages::aim_title(sigil_name(game, *sigil)),
        Casting::Verdict(verdict) if verdict.quality == CastQuality::Fizzled => {
            messages::FAILED_TITLE.to_owned()
        }
        Casting::Verdict(_) => messages::UNSTABLE_TITLE.to_owned(),
    };
    draw_frame(buffer, area, frame, &title, theme);
    let inner = Rect {
        x: frame.x + 1,
        y: frame.y + 1,
        width: frame.width - 2,
        height: frame.height - 2,
    };
    match casting {
        Casting::Drawing(drawing) => {
            let notice = drawing_notice(drawing.notice(), game, theme);
            draw_tracing(buffer, inner, game, &drawing.trace(), notice, theme);
        }
        Casting::Aiming { trace, sigil } => {
            let prompt = messages::aim_prompt(sigil_name(game, *sigil));
            let notice = Line::styled(prompt, theme.text);
            draw_tracing(buffer, inner, game, trace, Some(notice), theme);
        }
        Casting::Verdict(verdict) => draw_verdict(buffer, inner, game, verdict, theme),
    }
}

/// Returns the overlay's rectangle in the map `area`: [`OVERLAY_WIDTH`] ×
/// [`OVERLAY_HEIGHT`], centred, and cut to `area` when it is smaller.
pub fn overlay_area(area: Rect) -> Rect {
    let width = OVERLAY_WIDTH.min(area.width);
    let height = OVERLAY_HEIGHT.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

/// Returns the sigil of the player's grimoire at position `index` of the
/// sigil file (`SigilDef::index`), if the player knows it.
fn known_sigil(game: &Game, index: usize) -> Option<&SigilDef> {
    game.grimoire()
        .sigils()
        .iter()
        .find(|sigil| sigil.index() == index)
}

/// Returns the name of the sigil at position `index` of the sigil file, or
/// an empty name if the player does not know it (which the application
/// never asks).
fn sigil_name(game: &Game, index: usize) -> &str {
    known_sigil(game, index).map_or("", SigilDef::name)
}

/// Blanks to the screen background (`bg`) the cells of the map `area` that
/// touch `frame`, sides and corners: a one-cell gutter, cut to `area`.
///
/// On an 80 × 24 screen the 57 × 15 frame leaves one map column between it
/// and the side panel's border, and one map row under it. Left as they
/// were, they showed a sliver of the map, "│▒│ Essence", which reads as a
/// drawing fault; blank, they match the mock-ups of `docs/design.md`. On a
/// wider screen the map stays in view beyond this gutter.
fn clear_gutter(buffer: &mut Buffer, area: Rect, frame: Rect, theme: &Theme) {
    let (left, top) = (
        frame.left().saturating_sub(1),
        frame.top().saturating_sub(1),
    );
    let (right, bottom) = (
        frame.right().saturating_add(1),
        frame.bottom().saturating_add(1),
    );
    let around = Rect::new(left, top, right - left, bottom - top).intersection(area);
    for position in around.positions() {
        if frame.contains(position) {
            continue;
        }
        if let Some(cell) = buffer.cell_mut(position) {
            cell.reset();
            cell.set_style(theme.bg);
        }
    }
}

/// Clears `frame` to the panel background and draws its rounded border
/// (`border`), with `title` (`text_title`) on the top edge after one
/// stroke, as in the mock-up: "╭─ Trace a sigil ──╮". The inventory
/// (`ui::inventory`) draws its frame the same way.
///
/// The cells of the map `area` that touch the frame are blanked first (see
/// [`clear_gutter`]), so no stray map glyph sits against it.
pub(super) fn draw_frame(buffer: &mut Buffer, area: Rect, frame: Rect, title: &str, theme: &Theme) {
    clear_gutter(buffer, area, frame, theme);
    let glyphs = &theme.glyphs;
    for y in frame.top()..frame.bottom() {
        for x in frame.left()..frame.right() {
            if let Some(cell) = buffer.cell_mut((x, y)) {
                cell.reset();
                cell.set_style(theme.bg_panel);
            }
        }
    }
    let (left, right) = (frame.left(), frame.right() - 1);
    let (top, bottom) = (frame.top(), frame.bottom() - 1);
    for x in left + 1..right {
        put(buffer, (x, top), glyphs.frame_horizontal, theme.border);
        put(buffer, (x, bottom), glyphs.frame_horizontal, theme.border);
    }
    for y in top + 1..bottom {
        put(buffer, (left, y), glyphs.frame_vertical, theme.border);
        put(buffer, (right, y), glyphs.frame_vertical, theme.border);
    }
    put(buffer, (left, top), glyphs.frame_top_left, theme.border);
    put(buffer, (right, top), glyphs.frame_top_right, theme.border);
    put(
        buffer,
        (left, bottom),
        glyphs.frame_bottom_left,
        theme.border,
    );
    put(
        buffer,
        (right, bottom),
        glyphs.frame_bottom_right,
        theme.border,
    );
    let title_area = Rect {
        x: left + 2,
        y: top,
        width: frame.width.saturating_sub(4),
        height: 1,
    };
    Line::styled(format!(" {title} "), theme.text_title).render(title_area, buffer);
}

/// Draws the tracing view in the frame's `inner` area: the grid with
/// `trace` and its pen, the grimoire of the player of `game` on the right,
/// and under the grid the trace, its time, the essence and `notice`.
fn draw_tracing(
    buffer: &mut Buffer,
    inner: Rect,
    game: &Game,
    trace: &Trace,
    notice: Option<Line<'static>>,
    theme: &Theme,
) {
    let look = GridLook {
        stroke: theme.sigil_stroke,
        pen: true,
        fault: None,
    };
    draw_grid(buffer, inner, trace, look, theme);
    let column = column_area(inner);
    let rows = usize::from(GRID_HEIGHT.min(column.height));
    let grimoire = grimoire::lines(game.grimoire(), rows, column.width, theme);
    draw_lines(buffer, column, grimoire);

    let separator = theme.glyphs.separator;
    let player = game.player_actor();
    let essence = player.essence();
    let mut info = vec![
        labelled(
            messages::TRACE_LABEL,
            LABEL_WIDTH,
            Span::styled(
                messages::pattern_text(trace.nodes(), separator),
                theme.sigil_stroke,
            ),
            theme,
        ),
        labelled(
            messages::TIME_LABEL,
            LABEL_WIDTH,
            Span::styled(messages::cast_time(trace.len(), separator), theme.text),
            theme,
        ),
        Line::from(vec![
            Span::styled(
                format!("{:LABEL_WIDTH$}", messages::ESSENCE_LABEL),
                theme.text_muted,
            ),
            Span::styled(
                format!("{essence}/{}", player.max_essence()),
                theme.sigil_stroke,
            ),
            Span::styled(
                format!(" {separator} {ESSENCE_COST} a cast"),
                theme.text_muted,
            ),
        ]),
    ];
    info.extend(notice);
    draw_lines(buffer, info_area(inner), info);
}

/// The notice under the grid while drawing: what the last key did wrong,
/// or else a warning when the player of `game` lacks the essence of a
/// cast. Each starts with the warning mark, in `sigil_failed`.
fn drawing_notice(notice: Option<Notice>, game: &Game, theme: &Theme) -> Option<Line<'static>> {
    let essence = game.player_actor().essence();
    let text = match notice {
        Some(Notice::Refused(node)) => messages::node_refused(node),
        Some(Notice::Empty) => messages::EMPTY_TRACE.to_owned(),
        None if essence < ESSENCE_COST => messages::essence_too_low(essence, ESSENCE_COST),
        None => return None,
    };
    Some(Line::styled(
        format!("{} {text}", messages::WARNING_MARK),
        theme.sigil_failed,
    ))
}

/// Draws the verdict of an unstable or fizzled cast in the frame's `inner`
/// area: the grid with the trace in the verdict's colour and its faulty
/// node `×`, and on the right the verdict word, why, the patterns and the
/// outcome.
fn draw_verdict(buffer: &mut Buffer, inner: Rect, game: &Game, verdict: &Verdict, theme: &Theme) {
    let sigil = verdict.sigil.and_then(|index| known_sigil(game, index));
    let unstable = verdict.quality != CastQuality::Fizzled;
    let style = if unstable {
        theme.sigil_unstable
    } else {
        theme.sigil_failed
    };
    let look = GridLook {
        stroke: style,
        pen: false,
        fault: verdict.fault(sigil.map(SigilDef::pattern)),
    };
    draw_grid(buffer, inner, &verdict.trace, look, theme);

    let separator = theme.glyphs.separator;
    let traced = labelled(
        messages::TRACED_LABEL,
        VERDICT_LABEL_WIDTH,
        Span::styled(
            messages::pattern_text(verdict.trace.nodes(), separator),
            style,
        ),
        theme,
    );
    let mut lines = Vec::new();
    if unstable {
        let name = sigil.map_or("", SigilDef::name);
        let expected = sigil.map_or_else(String::new, |sigil| {
            messages::pattern_text(sigil.pattern().nodes(), separator)
        });
        lines.push(Line::styled(messages::UNSTABLE_WORD, style));
        lines.push(Line::styled(
            format!("{name}, {}", messages::ONE_NODE_OFF),
            theme.text,
        ));
        lines.push(labelled(
            messages::EXPECTED_LABEL,
            VERDICT_LABEL_WIDTH,
            Span::styled(expected, theme.sigil_stroke),
            theme,
        ));
        lines.push(traced);
    } else {
        lines.push(Line::styled(messages::FAILED_WORD, style));
        lines.push(Line::styled(messages::NO_KNOWN_PATTERN, theme.text_muted));
        lines.push(traced);
    }
    let outcome = if unstable {
        messages::UNSTABLE_OUTCOME
    } else {
        messages::FAILED_OUTCOME
    };
    // The outcome sits level with the grid's bottom row, as in the mock-up.
    let bottom_row = usize::from(GRID_HEIGHT - 1);
    lines.resize(bottom_row, Line::default());
    lines.push(Line::styled(outcome, theme.text));
    draw_lines(buffer, column_area(inner), lines);
}

/// A line made of `label`, padded to `width` columns in `text_muted`, then
/// `value`.
fn labelled(label: &str, width: usize, value: Span<'static>, theme: &Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:width$}"), theme.text_muted),
        value,
    ])
}

/// The area right of the grid, from its top row to the bottom of `inner`,
/// where the grimoire or the verdict is written.
fn column_area(inner: Rect) -> Rect {
    let x = inner.x + COLUMN_LEFT.min(inner.width);
    let right = inner.right().saturating_sub(RIGHT_MARGIN).max(x);
    let y = inner.y + GRID_TOP.min(inner.height);
    Rect {
        x,
        y,
        width: right - x,
        height: inner.bottom() - y,
    }
}

/// The area under the grid, from [`INFO_TOP`] to the bottom of `inner`,
/// across the whole width but the margins.
fn info_area(inner: Rect) -> Rect {
    let x = inner.x + GRID_LEFT.min(inner.width);
    let right = inner.right().saturating_sub(RIGHT_MARGIN).max(x);
    let y = inner.y + INFO_TOP.min(inner.height);
    Rect {
        x,
        y,
        width: right - x,
        height: inner.bottom() - y,
    }
}

/// Writes `lines` one per row of `area`, from its top, as far as it goes.
pub(super) fn draw_lines(buffer: &mut Buffer, area: Rect, lines: Vec<Line<'static>>) {
    for (offset, line) in (0..area.height).zip(lines) {
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

/// Draws the grid with `trace` in the frame's `inner` area, as `look`
/// says: strokes first, then the nine nodes over them.
///
/// A node is free (`○`, `sigil_node`), linked (`●`, the stroke style), the
/// pen (`◉`, `sigil_cursor`) when it is the last one and `look.pen` is set,
/// or faulty (`×`, the stroke style) when it is `look.fault`.
fn draw_grid(buffer: &mut Buffer, inner: Rect, trace: &Trace, look: GridLook, theme: &Theme) {
    let glyphs = &theme.glyphs;
    let origin = (inner.x + GRID_LEFT, inner.y + GRID_TOP);
    let inside = |(x, y): (u16, u16)| {
        let cell = (origin.0 + x, origin.1 + y);
        (cell.0 < inner.right() && cell.1 < inner.bottom()).then_some(cell)
    };
    for (cell, stroke) in stroke_cells(trace) {
        let glyph = match stroke {
            Stroke::Horizontal => glyphs.stroke_horizontal,
            Stroke::Vertical => glyphs.stroke_vertical,
            Stroke::Rising => glyphs.stroke_rising,
            Stroke::Falling => glyphs.stroke_falling,
            Stroke::Cross => glyphs.stroke_cross,
        };
        if let Some(cell) = inside(cell) {
            put(buffer, cell, glyph, look.stroke);
        }
    }
    let pen = trace.last().filter(|_| look.pen);
    for digit in 1..=9 {
        let Some(node) = Node::new(digit) else {
            continue;
        };
        let (glyph, style) = if Some(node) == look.fault {
            (glyphs.sigil_fault, look.stroke)
        } else if Some(node) == pen {
            (glyphs.sigil_pen, theme.sigil_cursor)
        } else if trace.contains(node) {
            (glyphs.sigil_linked, look.stroke)
        } else {
            (glyphs.sigil_node, theme.sigil_node)
        };
        if let Some(cell) = inside(node_cell(node)) {
            put(buffer, cell, glyph, style);
        }
    }
}

/// The grid cell of `node`, from the top-left node: columns left to right,
/// rows top to bottom, so 7 is at (0, 0) and 3 at (12, 6).
const fn node_cell(node: Node) -> (u16, u16) {
    let column = node.column() as u16;
    let row_from_top = (GRID_SIDE - 1) - node.row() as u16;
    (column * NODE_STEP_X, row_from_top * NODE_STEP_Y)
}

/// The cells between the nodes of `trace` that its strokes cross, each
/// with the piece of stroke it shows, in grid coordinates (see
/// [`node_cell`]).
///
/// Each pair of consecutive nodes is joined by a straight line: along a row
/// or a column, every cell between them; otherwise, one cell per row
/// between them, on the line. Cells of nodes are left out (nodes are drawn
/// over strokes), and a cell crossed by a rising and a falling stroke shows
/// a [`Stroke::Cross`].
fn stroke_cells(trace: &Trace) -> BTreeMap<(u16, u16), Stroke> {
    let mut cells = BTreeMap::new();
    for pair in trace.nodes().windows(2) {
        for (cell, stroke) in segment(pair[0], pair[1]) {
            cells
                .entry(cell)
                .and_modify(|existing: &mut Stroke| {
                    if *existing != stroke {
                        *existing = Stroke::Cross;
                    }
                })
                .or_insert(stroke);
        }
    }
    cells
}

/// The cells a straight stroke from `from` to `to` crosses, node cells
/// excluded, each with its piece of stroke (see [`stroke_cells`]).
fn segment(from: Node, to: Node) -> Vec<((u16, u16), Stroke)> {
    let (x0, y0) = node_cell(from);
    let (x1, y1) = node_cell(to);
    let is_node = |(x, y): (u16, u16)| x % NODE_STEP_X == 0 && y % NODE_STEP_Y == 0;
    let cells: Vec<((u16, u16), Stroke)> = if y0 == y1 {
        (x0.min(x1) + 1..x0.max(x1))
            .map(|x| ((x, y0), Stroke::Horizontal))
            .collect()
    } else if x0 == x1 {
        (y0.min(y1) + 1..y0.max(y1))
            .map(|y| ((x0, y), Stroke::Vertical))
            .collect()
    } else {
        // Going down the screen, a stroke that also goes right falls.
        let falling = (x1 > x0) == (y1 > y0);
        let stroke = if falling {
            Stroke::Falling
        } else {
            Stroke::Rising
        };
        let (dx, dy) = (i32::from(x1) - i32::from(x0), i32::from(y1) - i32::from(y0));
        (y0.min(y1) + 1..y0.max(y1))
            .map(|y| {
                // Node steps make this division exact: 6 or 12 columns over
                // 3 or 6 rows.
                let x = i32::from(x0) + dx * (i32::from(y) - i32::from(y0)) / dy;
                let x = u16::try_from(x).unwrap_or(0);
                ((x, y), stroke)
            })
            .collect()
    };
    cells
        .into_iter()
        .filter(|(cell, _)| !is_node(*cell))
        .collect()
}

/// Writes `glyph` with `style` at the cell `(x, y)` of `buffer`, if it
/// exists, over whatever was drawn there; the panel background is kept.
fn put(buffer: &mut Buffer, (x, y): (u16, u16), glyph: char, style: Style) {
    if let Some(cell) = buffer.cell_mut((x, y)) {
        cell.fg = Color::Reset;
        cell.modifier = Modifier::empty();
        cell.set_char(glyph).set_style(style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The trace drawn by pressing `digits` in order.
    fn pressed(digits: &[u8]) -> Trace {
        let mut trace = Trace::new();
        for &digit in digits {
            trace
                .push(Node::new(digit).expect("a digit from 1 to 9"))
                .expect("a node not linked yet");
        }
        trace
    }

    /// The grid of `trace` drawn as text: nodes as their digit, strokes as
    /// ASCII, blanks as spaces.
    fn sketch(trace: &Trace) -> Vec<String> {
        let mut rows = vec![vec![' '; usize::from(GRID_WIDTH)]; usize::from(GRID_HEIGHT)];
        for ((x, y), stroke) in stroke_cells(trace) {
            rows[usize::from(y)][usize::from(x)] = match stroke {
                Stroke::Horizontal => '-',
                Stroke::Vertical => '|',
                Stroke::Rising => '/',
                Stroke::Falling => '\\',
                Stroke::Cross => 'X',
            };
        }
        for digit in 1..=9 {
            let node = Node::new(digit).unwrap();
            let (x, y) = node_cell(node);
            rows[usize::from(y)][usize::from(x)] = char::from(b'0' + digit);
        }
        rows.into_iter()
            .map(|row| row.into_iter().collect::<String>().trim_end().to_owned())
            .collect()
    }

    #[test]
    fn nodes_sit_like_a_keypad_six_columns_and_three_rows_apart() {
        assert_eq!(
            sketch(&Trace::new()),
            [
                "7     8     9",
                "",
                "",
                "4     5     6",
                "",
                "",
                "1     2     3"
            ]
        );
    }

    #[test]
    fn rows_and_columns_are_joined_by_straight_lines() {
        assert_eq!(
            sketch(&pressed(&[7, 8, 5])),
            [
                "7-----8     9",
                "      |",
                "      |",
                "4     5     6",
                "",
                "",
                "1     2     3"
            ]
        );
    }

    #[test]
    fn diagonals_fall_at_45_degrees_as_in_the_mock_up() {
        assert_eq!(
            sketch(&pressed(&[4, 8, 6])),
            [
                "7     8     9",
                "    /   \\",
                "  /       \\",
                "4     5     6",
                "",
                "",
                "1     2     3"
            ]
        );
    }

    #[test]
    fn a_stroke_over_a_linked_node_is_drawn_through_it() {
        // 2 then 8 links 5 on the way; 4 then 6 crosses the linked 5.
        assert_eq!(
            sketch(&pressed(&[2, 8, 4, 6])),
            [
                "7     8     9",
                "    / |",
                "  /   |",
                "4-----5-----6",
                "      |",
                "      |",
                "1     2     3"
            ]
        );
    }

    #[test]
    fn nodes_that_are_not_aligned_are_joined_by_a_straight_line() {
        assert_eq!(
            sketch(&pressed(&[1, 6])),
            [
                "7     8     9",
                "",
                "",
                "4     5     6",
                "        /",
                "    /",
                "1     2     3"
            ]
        );
    }

    #[test]
    fn a_rising_and_a_falling_stroke_on_one_cell_make_a_cross() {
        // 7 to 2 falls and 8 to 1 rises; both pass through (3, 3).
        let trace = pressed(&[7, 2, 8, 1]);

        let crosses: Vec<_> = stroke_cells(&trace)
            .into_iter()
            .filter(|(_, stroke)| *stroke == Stroke::Cross)
            .map(|(cell, _)| cell)
            .collect();

        assert_eq!(crosses, [(3, 3)]);
    }

    #[test]
    fn the_overlay_is_centred_in_the_map_and_cut_to_it() {
        assert_eq!(
            overlay_area(Rect::new(0, 1, 58, 16)),
            Rect::new(0, 1, 57, 15)
        );
        assert_eq!(
            overlay_area(Rect::new(0, 1, 98, 32)),
            Rect::new(20, 9, 57, 15)
        );
        assert_eq!(overlay_area(Rect::new(0, 1, 10, 5)), Rect::new(0, 1, 10, 5));
    }
}
