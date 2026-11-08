//! The side panel right of the map: the player's health, in figures and as
//! a bar, the essence, the sigils known, then the list of what the player
//! sees.
//!
//! Exposes [`render`], the health thresholds ([`HP_HIGH_ABOVE_PERCENT`],
//! [`HP_LOW_BELOW_PERCENT`]), the bar's width ([`HP_BAR_WIDTH`]), the
//! [`HealthLevel`] they decide and the fewest rows the sigil names keep
//! ([`SIGIL_MIN_ROWS`]). The layout follows the game screen of
//! `docs/design.md`: a border down the left, "HP" with its value, the bar,
//! "Essence" with its value, a blank row, "SIGILS" with the count known out
//! of those that exist and their names, a blank row, then "IN VIEW" with
//! the player, one line per kind of monster, one per kind of item, one per
//! kind of engraved glyph and one for the way down in view: the stairs, or
//! on the bottom floor the seal that ends the run.
//!
//! Each monster entry ends with that monster's hit points, current out of
//! maximum ("4/9"), right-aligned like the player's value above and in the
//! colour of the same [`HealthLevel`]; for a kind seen several times
//! ("×2"), those of the most wounded of them. The figures and the count are
//! never cut, and neither is the name while it fits a row: an entry whose
//! name, count and figures do not share one row takes two, the name alone
//! on the first ("H lead homunculus"), the count and the figures
//! right-aligned on the second ("×2 26/28"). Only a name longer than a whole
//! row is shortened.
//!
//! The rows left under the fixed lines are shared by the sigil names and
//! the "in view" block, in this order of priority: the "in view" block
//! first, since it is the map's legend and names what threatens the player,
//! but never at the cost of the sigil names' first [`SIGIL_MIN_ROWS`] rows;
//! then the sigil names take every row the list leaves free. On a 24-row
//! screen this lists the five shipped sigils whenever the player and at most
//! three other kinds are in view; a block that does not fit counts what it
//! leaves out on its last row ("and 2 more").
//!
//! Health is never told by colour alone: the value is written out, the bar
//! is drawn with two glyphs, and below a quarter the value is preceded by
//! `!`. Essence below the cost of a cast is marked `!` as well. A monster's
//! health is written out too, without `!`: that mark warns of a danger to
//! the player, which a dying monster is not.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget as _;
use sigilfall_core::content::Content;
use sigilfall_core::game::Game;
use sigilfall_core::geometry::Position;
use sigilfall_core::map::Tile;
use sigilfall_core::sigil::ESSENCE_COST;

use crate::messages;
use crate::theme::Theme;

/// Health above this share of the maximum, in percent, is drawn in the
/// `hp_high` colour (`docs/design.md`: "PV au-dessus de 50 %").
pub const HP_HIGH_ABOVE_PERCENT: i32 = 50;

/// Health below this share of the maximum, in percent, is drawn in the
/// `hp_low` colour and marked with `!` (`docs/design.md`: "PV sous 25 %").
/// From this share up to [`HP_HIGH_ABOVE_PERCENT`] included, it is `hp_mid`.
pub const HP_LOW_BELOW_PERCENT: i32 = 25;

/// Cells of the health bar, as `docs/design.md` sets it: 18 of the panel's
/// 19 content columns, each worth one eighteenth of the maximum.
pub const HP_BAR_WIDTH: u16 = 18;

/// Fewest rows the names of the sigils known keep, however long the "in
/// view" list is (fewer only while fewer sigils are known).
///
/// Three rows show two names and "and N more": enough to remind the player
/// that sigils are known and how many, while a crowded list keeps the rest;
/// the overlay's grimoire lists every sigil with its pattern anyway.
pub const SIGIL_MIN_ROWS: usize = 3;

/// Rows of the panel's content that never move: the health figures and bar
/// (2), the essence, a blank row, the sigils' title and the blank row
/// before "IN VIEW". The rest is shared as [`sigil_name_rows`] decides.
const FIXED_ROWS: usize = 6;

/// Columns between the panel's left edge and its content: the border and
/// one blank column.
const CONTENT_LEFT: u16 = 2;

/// Blank columns kept right of the panel's content, so text never touches
/// the terminal's edge.
const CONTENT_RIGHT_MARGIN: u16 = 1;

/// How healthy the player is, which gives the colour of the value and the
/// bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthLevel {
    /// Above [`HP_HIGH_ABOVE_PERCENT`] of the maximum.
    High,
    /// From [`HP_LOW_BELOW_PERCENT`] to [`HP_HIGH_ABOVE_PERCENT`] of the
    /// maximum, both included.
    Mid,
    /// Below [`HP_LOW_BELOW_PERCENT`] of the maximum, down to 0.
    Low,
}

impl HealthLevel {
    /// Returns the level of `hp` hit points out of `max_hp`.
    ///
    /// Shares are compared exactly, in integers: 15 out of 30 is half,
    /// hence [`HealthLevel::Mid`], and 7 out of 30 is under a quarter. A
    /// maximum of 0 or less, which the game never gives, counts as
    /// [`HealthLevel::Low`].
    pub const fn of(hp: i32, max_hp: i32) -> Self {
        if max_hp <= 0 {
            return Self::Low;
        }
        let percent_times_max = hp.saturating_mul(100);
        if percent_times_max > HP_HIGH_ABOVE_PERCENT * max_hp {
            Self::High
        } else if percent_times_max >= HP_LOW_BELOW_PERCENT * max_hp {
            Self::Mid
        } else {
            Self::Low
        }
    }

    /// The style of this level in `theme`: `hp_high`, `hp_mid` or `hp_low`.
    const fn style(self, theme: &Theme) -> Style {
        match self {
            Self::High => theme.hp_high,
            Self::Mid => theme.hp_mid,
            Self::Low => theme.hp_low,
        }
    }
}

/// Returns how many of the bar's `width` cells are filled for `hp` hit
/// points out of `max_hp`: the share rounded up, so a living player always
/// has at least one cell (18/30 fills 11 cells, as in the mock-up), but
/// capped one cell short of full while any hit point is missing, so only
/// full health fills the whole bar. Health of 0 or less, or a maximum of 0
/// or less, give an empty bar.
pub fn filled_cells(hp: i32, max_hp: i32, width: u16) -> u16 {
    if hp <= 0 || max_hp <= 0 {
        return 0;
    }
    if hp >= max_hp {
        return width;
    }
    let rounded_up = (i64::from(hp) * i64::from(width) + i64::from(max_hp) - 1) / i64::from(max_hp);
    u16::try_from(rounded_up).map_or(width, |cells| cells.min(width.saturating_sub(1)))
}

/// Draws the side panel in `area` of `buffer`: the border on its left
/// column, then the health block, the essence, the sigils block and the
/// "in view" list for `game`, with monster names and the number of sigils
/// from `content`.
///
/// The sigil names and the list share the rows under the fixed lines as
/// [`sigil_name_rows`] decides; the list takes every row left under the
/// sigils block. When more things are in view than rows, the last row says
/// how many more there are. Text wider than the panel is cut. An empty
/// `area` draws nothing.
pub fn render(buffer: &mut Buffer, area: Rect, game: &Game, content: &Content, theme: &Theme) {
    if area.is_empty() {
        return;
    }
    for y in area.top()..area.bottom() {
        if let Some(cell) = buffer.cell_mut((area.x, y)) {
            cell.set_char(theme.glyphs.frame_vertical)
                .set_style(theme.border);
        }
    }
    let content_area = Rect {
        x: area.x + CONTENT_LEFT,
        width: area
            .width
            .saturating_sub(CONTENT_LEFT + CONTENT_RIGHT_MARGIN),
        ..area
    };
    let entries = in_view_entries(game, content, theme);
    let shared = usize::from(content_area.height).saturating_sub(FIXED_ROWS);
    let in_view_rows = 1 + entries
        .iter()
        .map(|entry| entry.rows(content_area.width))
        .sum::<usize>();
    let name_rows = sigil_name_rows(game.grimoire().len(), in_view_rows, shared);
    let mut lines = health_block(game, content_area.width, theme);
    lines.push(essence_line(game, content_area.width, theme));
    lines.push(Line::default());
    lines.extend(sigils_block(
        game,
        content,
        name_rows,
        content_area.width,
        theme,
    ));
    lines.push(Line::default());
    let list_rows = usize::from(content_area.height).saturating_sub(lines.len());
    lines.extend(in_view_lines(entries, list_rows, content_area.width, theme));
    for (offset, line) in (0..content_area.height).zip(lines) {
        line.render(
            Rect {
                y: content_area.y + offset,
                height: 1,
                ..content_area
            },
            buffer,
        );
    }
}

/// The health block, `width` columns wide: "HP" and the value right-aligned
/// on one line ("! 5/30" below a quarter), the bar on the next.
fn health_block(game: &Game, width: u16, theme: &Theme) -> Vec<Line<'static>> {
    let player = game.player_actor();
    let (hp, max_hp) = (player.hp().max(0), player.max_hp());
    let level = HealthLevel::of(hp, max_hp);
    let style = level.style(theme);
    let value = if level == HealthLevel::Low {
        format!("{} {hp}/{max_hp}", messages::WARNING_MARK)
    } else {
        format!("{hp}/{max_hp}")
    };
    let gap = usize::from(width)
        .saturating_sub(messages::HP_LABEL.chars().count() + value.chars().count());
    let figures = Line::from(vec![
        Span::styled(messages::HP_LABEL, theme.text_muted),
        Span::raw(" ".repeat(gap)),
        Span::styled(value, style),
    ]);

    let filled = filled_cells(hp, max_hp, HP_BAR_WIDTH);
    let glyphs = &theme.glyphs;
    let bar = Line::from(vec![
        Span::styled(
            glyphs.bar_full.to_string().repeat(usize::from(filled)),
            style,
        ),
        Span::styled(
            glyphs
                .bar_empty
                .to_string()
                .repeat(usize::from(HP_BAR_WIDTH - filled)),
            theme.border,
        ),
    ]);
    vec![figures, bar]
}

/// A line of `width` columns with `label` on the left in `text_muted` and
/// `value` right-aligned in `style`.
fn label_and_value(
    label: &str,
    value: String,
    style: Style,
    width: u16,
    theme: &Theme,
) -> Line<'static> {
    let gap = usize::from(width).saturating_sub(label.chars().count() + value.chars().count());
    Line::from(vec![
        Span::styled(label.to_owned(), theme.text_muted),
        Span::raw(" ".repeat(gap)),
        Span::styled(value, style),
    ])
}

/// The essence line, `width` columns wide: "Essence" and the value
/// right-aligned in `sigil_stroke`, or "! 1/6" in `sigil_failed` while the
/// player has less than a cast costs (`sigil::ESSENCE_COST`).
fn essence_line(game: &Game, width: u16, theme: &Theme) -> Line<'static> {
    let player = game.player_actor();
    let (essence, max) = (player.essence(), player.max_essence());
    let (value, style) = if essence < ESSENCE_COST {
        (
            format!("{} {essence}/{max}", messages::WARNING_MARK),
            theme.sigil_failed,
        )
    } else {
        (format!("{essence}/{max}"), theme.sigil_stroke)
    };
    label_and_value(messages::ESSENCE_LABEL, value, style, width, theme)
}

/// Returns how many rows the sigil names take, out of the `shared` rows
/// that they and the "in view" block divide between them, when `known`
/// sigils are known and the block wants `in_view_rows` rows (its title and
/// the rows of its entries, see [`Entry::rows`]).
///
/// The names take what the block leaves free, but never fewer than
/// [`SIGIL_MIN_ROWS`] and never more than they need: one row per sigil, or
/// one for "none yet". The result never exceeds `shared`; the block gets
/// the rest.
fn sigil_name_rows(known: usize, in_view_rows: usize, shared: usize) -> usize {
    let wanted = known.max(1);
    let left = shared.saturating_sub(in_view_rows);
    left.max(SIGIL_MIN_ROWS).min(wanted).min(shared)
}

/// The sigils block, `width` columns wide: "SIGILS" with the count known
/// out of the sigils of `content`, then the names in grimoire order in at
/// most `rows` rows, the last one counting the others when they do not
/// fit; "none yet" while the grimoire is empty. With `rows` at 0 only the
/// title is drawn.
fn sigils_block(
    game: &Game,
    content: &Content,
    rows: usize,
    width: u16,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let known = game.grimoire().sigils();
    let count = format!("{}/{}", known.len(), content.sigils().len());
    let mut lines = vec![label_and_value(
        messages::SIGILS_TITLE,
        count,
        theme.text,
        width,
        theme,
    )];
    if rows == 0 {
        return lines;
    }
    if known.is_empty() {
        lines.push(Line::styled(messages::NO_SIGIL_YET, theme.text_muted));
        return lines;
    }
    let overflows = known.len() > rows;
    let shown = if overflows { rows - 1 } else { known.len() };
    lines.extend(
        known
            .iter()
            .take(shown)
            .map(|sigil| Line::styled(sigil.name().to_owned(), theme.text)),
    );
    if overflows {
        lines.push(Line::styled(
            messages::more_in_view(known.len() - shown),
            theme.text_muted,
        ));
    }
    lines
}

/// One entry of the "in view" list: a glyph in its own style, then a name
/// and the count of its kind, and for a monster its health right-aligned.
/// It takes one row, or two for a monster whose name, count and health do
/// not fit one ([`Entry::rows`]).
struct Entry {
    /// The glyph the thing is drawn with on the map.
    glyph: char,
    /// Style of that glyph, the same as on the map.
    glyph_style: Style,
    /// Name in full; it is shortened only when drawn, if a row is too
    /// narrow for it.
    name: String,
    /// How many things of this kind are in view, written "×2" after the
    /// name (or before the health) when above 1.
    count: usize,
    /// For a monster, its hit points as "4/9" in the style of their
    /// [`HealthLevel`]; `None` for everything else.
    health: Option<(String, Style)>,
}

impl Entry {
    /// Returns how many rows this entry takes in a list `width` columns
    /// wide: two for a monster whose name, count, one blank column and
    /// health are wider than the row (the glyph and the blank after it
    /// apart), one otherwise.
    fn rows(&self, width: u16) -> usize {
        let Some((health, _)) = &self.health else {
            return 1;
        };
        // The sign of the count is one column in every glyph set.
        let wanted = self.name.chars().count()
            + count_suffix(self.count, 'x').chars().count()
            + 1
            + health.chars().count();
        if wanted <= name_width(width) { 1 } else { 2 }
    }
}

/// One kind of monster in view, while [`in_view_entries`] groups them.
struct MonsterKind<'game> {
    /// Identifier of the definition shared by the monsters of this kind.
    definition: &'game str,
    /// Letter of the kind on the map.
    glyph: char,
    /// Style of that letter, the same as on the map.
    style: Style,
    /// How many monsters of this kind are in view.
    count: usize,
    /// Hit points of the most wounded monster of this kind in view.
    hp: i32,
    /// Maximum hit points of that same monster.
    max_hp: i32,
}

impl MonsterKind<'_> {
    /// Keeps the health of another monster of this kind, with `hp` hit
    /// points out of `max_hp`, if it is more wounded than the one kept so
    /// far: a lower share of its maximum, compared exactly in integers. On a
    /// tie the monster seen first stays.
    fn keep_most_wounded(&mut self, hp: i32, max_hp: i32) {
        let candidate = i64::from(hp) * i64::from(self.max_hp);
        let kept = i64::from(self.hp) * i64::from(max_hp);
        if candidate < kept {
            self.hp = hp;
            self.max_hp = max_hp;
        }
    }
}

/// The entries of the "in view" block: the player, then one entry per kind
/// of monster in view, in the order of `Game::visible_monsters`, then one
/// per kind of item in view, in the order of `Game::visible_items`, then
/// the engraved glyphs in view, unread ones first, then the stairs down or
/// the seal when in view, each with the count of its kind. A monster entry
/// also carries the health of the most wounded monster of its kind in view.
/// Monsters come first since they threaten the player; items, which the
/// player may pick up, come before the glyphs, as in the mock-up. Names are
/// kept whole here: [`entry_lines`] fits them to the panel.
fn in_view_entries(game: &Game, content: &Content, theme: &Theme) -> Vec<Entry> {
    let mut entries = vec![Entry {
        glyph: theme.glyphs.player,
        glyph_style: theme.player,
        name: messages::PLAYER_IN_VIEW.to_owned(),
        count: 1,
        health: None,
    }];
    let mut kinds: Vec<MonsterKind<'_>> = Vec::new();
    for monster in game.visible_monsters() {
        // A monster in view is always in the world; skipping one that is not
        // only drops a line, it never shows wrong figures.
        let Some(actor) = game.actor(monster.id) else {
            continue;
        };
        let (hp, max_hp) = (actor.hp().max(0), actor.max_hp());
        if let Some(kind) = kinds
            .iter_mut()
            .find(|kind| kind.definition == monster.definition)
        {
            kind.count += 1;
            kind.keep_most_wounded(hp, max_hp);
        } else {
            kinds.push(MonsterKind {
                definition: monster.definition,
                glyph: theme.glyphs.creature(monster.family, monster.elite),
                style: theme.creature(monster.family),
                count: 1,
                hp,
                max_hp,
            });
        }
    }
    entries.extend(kinds.into_iter().map(|kind| {
        let level = HealthLevel::of(kind.hp, kind.max_hp);
        Entry {
            glyph: kind.glyph,
            glyph_style: kind.style,
            name: messages::monster_name(content, kind.definition).to_owned(),
            count: kind.count,
            health: Some((format!("{}/{}", kind.hp, kind.max_hp), level.style(theme))),
        }
    }));
    let mut items: Vec<(&str, char, usize)> = Vec::new();
    for item in game.visible_items() {
        if let Some(kind) = items.iter_mut().find(|kind| kind.0 == item.definition) {
            kind.2 += 1;
        } else {
            items.push((item.definition, theme.glyphs.item(item.family), 1));
        }
    }
    entries.extend(items.into_iter().map(|(definition, glyph, count)| Entry {
        glyph,
        glyph_style: theme.item,
        name: messages::item_name(content, definition).to_owned(),
        count,
        health: None,
    }));
    let (read, unread): (Vec<_>, Vec<_>) = game
        .engravings()
        .iter()
        .filter(|engraving| game.is_visible(engraving.position()))
        .partition(|engraving| game.grimoire().knows(engraving.sigil().id()));
    let glyphs = [
        (
            unread.len(),
            theme.glyphs.engraving_unread,
            theme.engraving_unread,
            messages::UNREAD_GLYPH_IN_VIEW,
        ),
        (
            read.len(),
            theme.glyphs.engraving_read,
            theme.engraving_read,
            messages::READ_GLYPH_IN_VIEW,
        ),
    ];
    entries.extend(glyphs.into_iter().filter(|(count, ..)| *count > 0).map(
        |(count, glyph, glyph_style, name)| Entry {
            glyph,
            glyph_style,
            name: name.to_owned(),
            count,
            health: None,
        },
    ));
    let ways_down = [
        (
            Tile::StairsDown,
            theme.glyphs.stairs,
            theme.stairs,
            messages::STAIRS_IN_VIEW,
        ),
        (
            Tile::Seal,
            theme.glyphs.seal,
            theme.seal,
            messages::SEAL_IN_VIEW,
        ),
    ];
    for (tile, glyph, glyph_style, name) in ways_down {
        if tile_in_view(game, tile) {
            entries.push(Entry {
                glyph,
                glyph_style,
                name: name.to_owned(),
                count: 1,
                health: None,
            });
        }
    }
    entries
}

/// Returns whether the player sees a cell holding `tile`: the stairs down,
/// or the seal of the bottom floor. A tile only remembered does not count,
/// like every other entry of the list.
fn tile_in_view(game: &Game, tile: Tile) -> bool {
    let map = game.map();
    // Floors are 80 × 40 (`generation::FLOOR_WIDTH` and `FLOOR_HEIGHT`), so
    // their sides always fit an `i32`.
    let (Ok(width), Ok(height)) = (i32::try_from(map.width()), i32::try_from(map.height())) else {
        return false;
    };
    (0..height)
        .flat_map(|y| (0..width).map(move |x| Position::new(x, y)))
        .any(|cell| map.tile(cell) == Some(tile) && game.is_visible(cell))
}

/// The "in view" block in at most `rows` rows of `width` columns: its
/// title, then `entries` in order, each on its one or two rows
/// ([`entry_lines`]); when they overflow, the last row tells how many
/// entries it replaces, and an entry is shown whole or not at all. Nothing
/// at all when `rows` is 0.
fn in_view_lines(
    entries: Vec<Entry>,
    rows: usize,
    width: u16,
    theme: &Theme,
) -> Vec<Line<'static>> {
    if rows == 0 {
        return Vec::new();
    }
    let mut lines = vec![Line::styled(messages::IN_VIEW_TITLE, theme.text_muted)];
    // Rows under the title; when the entries overflow them, the last one
    // tells how many entries it replaces.
    let room = rows - 1;
    let wanted: usize = entries.iter().map(|entry| entry.rows(width)).sum();
    let overflows = wanted > room;
    let budget = if overflows {
        room.saturating_sub(1)
    } else {
        room
    };
    let total = entries.len();
    let (mut shown, mut used) = (0, 0);
    for entry in entries {
        let entry_rows = entry.rows(width);
        if used + entry_rows > budget {
            break;
        }
        used += entry_rows;
        shown += 1;
        lines.extend(entry_lines(entry, width, theme));
    }
    if overflows && room > 0 {
        lines.push(Line::styled(
            messages::more_in_view(total - shown),
            theme.text_muted,
        ));
    }
    lines
}

/// Columns left for the name, its count and the health on a row of the
/// list `width` columns wide: the glyph and the blank after it take two.
fn name_width(width: u16) -> usize {
    usize::from(width).saturating_sub(2)
}

/// The rows of one entry of the "in view" list, `width` columns wide, as
/// many as [`Entry::rows`] says.
///
/// On one row: the glyph, a blank and the name with its count in `text`,
/// then the health, if any, right-aligned in the style of its level, at
/// least one blank column after the name; a name without health is cut
/// before its count if the row is too narrow. On two rows: the glyph, a
/// blank and the name alone on the first, cut only if wider than the row,
/// then on the second the count, if any, in `text` and the health,
/// right-aligned.
fn entry_lines(entry: Entry, width: u16, theme: &Theme) -> Vec<Line<'static>> {
    let rows = entry.rows(width);
    let times = theme.glyphs.dimension;
    let glyph = Span::styled(entry.glyph.to_string(), entry.glyph_style);
    let Some((health, style)) = entry.health else {
        let name = counted_name(&entry.name, entry.count, name_width(width), times);
        return vec![Line::from(vec![
            glyph,
            Span::styled(format!(" {name}"), theme.text),
        ])];
    };
    let count = count_suffix(entry.count, times);
    if rows == 1 {
        let name = format!(" {}{count}", entry.name);
        // The glyph takes one column.
        let used = 1 + name.chars().count() + health.chars().count();
        let gap = usize::from(width).saturating_sub(used).max(1);
        return vec![Line::from(vec![
            glyph,
            Span::styled(name, theme.text),
            Span::raw(" ".repeat(gap)),
            Span::styled(health, style),
        ])];
    }
    let name = counted_name(&entry.name, 1, name_width(width), times);
    // " ×2" becomes "×2 ", a blank between the count and the figures.
    let count = count.trim_start();
    let count = if count.is_empty() {
        String::new()
    } else {
        format!("{count} ")
    };
    let indent = usize::from(width).saturating_sub(count.chars().count() + health.chars().count());
    vec![
        Line::from(vec![glyph, Span::styled(format!(" {name}"), theme.text)]),
        Line::from(vec![
            Span::raw(" ".repeat(indent)),
            Span::styled(count, theme.text),
            Span::styled(health, style),
        ]),
    ]
}

/// The count written after a name: " ×3" for `count` above 1, with `times`
/// as the sign, nothing for a single thing.
fn count_suffix(count: usize, times: char) -> String {
    if count > 1 {
        format!(" {times}{count}")
    } else {
        String::new()
    }
}

/// `name` followed by `×count` when `count` is above 1, cut so the whole
/// fits in `width` characters; the count is kept and the name shortened.
fn counted_name(name: &str, count: usize, width: usize, times: char) -> String {
    let suffix = count_suffix(count, times);
    let room = width.saturating_sub(suffix.chars().count());
    let short: String = name.chars().take(room).collect();
    format!("{}{suffix}", short.trim_end())
}

#[cfg(test)]
mod tests {
    use sigilfall_core::action::Action;
    use sigilfall_core::game::PLAYER_START;
    use sigilfall_core::geometry::{Direction, Position};
    use sigilfall_core::rng::Seed;

    use super::*;
    use crate::theme::{ColorMode, GlyphSet};

    #[test]
    fn health_levels_follow_the_thresholds_of_the_design() {
        // 30 hit points: above 15 is high, 8 to 15 is mid, 7 and less low.
        assert_eq!(HealthLevel::of(30, 30), HealthLevel::High);
        assert_eq!(HealthLevel::of(16, 30), HealthLevel::High);
        assert_eq!(HealthLevel::of(15, 30), HealthLevel::Mid, "exactly half");
        assert_eq!(HealthLevel::of(8, 30), HealthLevel::Mid);
        assert_eq!(HealthLevel::of(7, 30), HealthLevel::Low);
        assert_eq!(HealthLevel::of(0, 30), HealthLevel::Low);
        // Exactly a quarter is still mid.
        assert_eq!(HealthLevel::of(5, 20), HealthLevel::Mid);
        assert_eq!(HealthLevel::of(4, 20), HealthLevel::Low);
        assert_eq!(HealthLevel::of(1, 0), HealthLevel::Low);
    }

    #[test]
    fn the_bar_rounds_up_and_fills_only_at_full_health() {
        assert_eq!(filled_cells(30, 30, HP_BAR_WIDTH), 18);
        assert_eq!(
            filled_cells(29, 30, HP_BAR_WIDTH),
            17,
            "wounded is never full"
        );
        assert_eq!(filled_cells(18, 30, HP_BAR_WIDTH), 11, "as in the mock-up");
        assert_eq!(filled_cells(1, 30, HP_BAR_WIDTH), 1, "alive shows a cell");
        assert_eq!(filled_cells(0, 30, HP_BAR_WIDTH), 0);
        assert_eq!(filled_cells(-3, 30, HP_BAR_WIDTH), 0);
        assert_eq!(filled_cells(40, 30, HP_BAR_WIDTH), 18, "never overflows");
        assert_eq!(filled_cells(5, 0, HP_BAR_WIDTH), 0);
    }

    #[test]
    fn a_count_is_kept_and_a_long_name_shortened() {
        assert_eq!(counted_name("shade", 1, 17, '×'), "shade");
        assert_eq!(counted_name("shade", 3, 17, '×'), "shade ×3");
        assert_eq!(
            counted_name("lead homunculus", 2, 17, 'x'),
            "lead homunculu x2"
        );
        assert_eq!(
            counted_name("lead homunculus", 1, 17, 'x'),
            "lead homunculus"
        );
    }

    /// Three kinds of monsters, for the "in view" list.
    const THREE_KINDS: &str = r#"[
        Monster(id: "rat", name: "rat", family: Vermin, elite: false,
            speed: 100, hp: 1, attack: 0, defense: 0, min_depth: 1),
        Monster(id: "eye", name: "eye", family: Eye, elite: false,
            speed: 100, hp: 1, attack: 0, defense: 0, min_depth: 1),
        Monster(id: "shade", name: "shade", family: Spectre, elite: false,
            speed: 100, hp: 1, attack: 0, defense: 0, min_depth: 1),
    ]"#;

    /// The fixed room with two rats, an eye and a shade in view of the
    /// player, and the content naming them.
    fn crowded_room() -> (Game, Content) {
        let content = Content::parse(THREE_KINDS).unwrap();
        let mut game = Game::in_fixed_room(Seed::new(42));
        for (id, x) in [("rat", 7), ("eye", 8), ("rat", 9), ("shade", 10)] {
            let cell = Position::new(x, PLAYER_START.y);
            game.spawn_monster(cell, content.monster(id).unwrap());
        }
        (game, content)
    }

    /// The "in view" block of `game` in `rows` rows of `width` columns.
    fn in_view_block(
        game: &Game,
        content: &Content,
        rows: usize,
        width: u16,
        theme: &Theme,
    ) -> Vec<Line<'static>> {
        in_view_lines(in_view_entries(game, content, theme), rows, width, theme)
    }

    /// The text of `lines`.
    fn texts(lines: &[Line<'_>]) -> Vec<String> {
        lines.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn the_in_view_list_groups_a_kind_and_counts_it() {
        let (game, content) = crowded_room();
        let theme = Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode);

        let lines = in_view_block(&game, &content, 10, 19, &theme);

        assert_eq!(
            texts(&lines),
            [
                "IN VIEW",
                "@ you",
                "v rat ×2        1/1",
                "e eye           1/1",
                "s shade         1/1",
            ]
        );
    }

    #[test]
    fn items_in_view_follow_the_monsters_grouped_by_kind() {
        // The test monsters come with the shipped items (`Content::parse`).
        let (mut game, content) = crowded_room();
        let draught = content.item("healing_draught").unwrap();
        let ink = content.item("steadying_ink").unwrap();
        for (definition, x) in [(draught, 3), (ink, 4), (draught, 6)] {
            game.place_item(Position::new(x, PLAYER_START.y + 1), definition);
        }
        let theme = Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode);

        let lines = in_view_block(&game, &content, 10, 19, &theme);

        assert_eq!(
            texts(&lines),
            [
                "IN VIEW",
                "@ you",
                "v rat ×2        1/1",
                "e eye           1/1",
                "s shade         1/1",
                "¡ Healing Draugh ×2",
                "♦ Steadying Ink",
            ]
        );
        let ascii = Theme::new(ColorMode::Monochrome, GlyphSet::Ascii);
        let lines = in_view_block(&game, &content, 10, 19, &ascii);
        assert_eq!(
            texts(&lines)[5..],
            ["! Healing Draugh x2", "* Steadying Ink"]
        );
        assert_eq!(
            lines[5].spans[0].style, ascii.item,
            "glyph in the item token"
        );
    }

    #[test]
    fn entries_beyond_the_rows_are_counted_on_the_last_row() {
        let (game, content) = crowded_room();
        let theme = Theme::new(ColorMode::TrueColorDark, GlyphSet::Ascii);

        // Title and four entries need five rows; with four, the last row
        // replaces two entries.
        let lines = in_view_block(&game, &content, 4, 19, &theme);

        assert_eq!(
            texts(&lines),
            ["IN VIEW", "@ you", "v rat x2        1/1", "and 2 more"]
        );
        assert_eq!(
            texts(&in_view_block(&game, &content, 1, 19, &theme)),
            ["IN VIEW"]
        );
        assert!(in_view_block(&game, &content, 0, 19, &theme).is_empty());
    }

    /// Rats whose defence equals the player's attack, so every blow of the
    /// player takes exactly one hit point (`combat`: at least 1, and the
    /// roll of -1 to +1 never adds more over a difference of 0); too slow
    /// to move or strike more than once in a few turns. A long-named kind
    /// and a shade complete the list.
    const STURDY_KINDS: &str = r#"[
        Monster(id: "rat", name: "rat", family: Vermin, elite: false,
            speed: 1, hp: 8, attack: 0, defense: 4, min_depth: 1),
        Monster(id: "lead", name: "lead homunculus", family: Homunculus,
            elite: true, speed: 1, hp: 28, attack: 0, defense: 4, min_depth: 1),
        Monster(id: "shade", name: "shade", family: Spectre, elite: false,
            speed: 1, hp: 7, attack: 0, defense: 4, min_depth: 1),
    ]"#;

    /// The cell east of the start, where the monster the player strikes
    /// stands.
    const NEXT_TO_START: Position = Position::new(PLAYER_START.x + 1, PLAYER_START.y);

    /// The fixed room with [`STURDY_KINDS`], the monsters `(id, cell)`
    /// spawned in order, after the player struck the one east of the start
    /// `blows` times; returns the game, the content and the struck monster's
    /// hit points, checked to be its maximum less `blows`.
    fn wounded_room(monsters: &[(&str, Position)], blows: i32) -> (Game, Content, i32) {
        let content = Content::parse(STURDY_KINDS).unwrap();
        let mut game = Game::in_fixed_room(Seed::new(42));
        let mut struck = None;
        for &(id, cell) in monsters {
            let actor = game.spawn_monster(cell, content.monster(id).unwrap());
            if cell == NEXT_TO_START {
                struck = Some(actor);
            }
        }
        let struck = struck.expect("a monster stands east of the start");
        for _ in 0..blows {
            game.submit(Action::Move(Direction::East));
        }
        let actor = game.actor(struck).expect("the struck monster lives");
        assert_eq!(actor.hp(), actor.max_hp() - blows, "one hit point a blow");
        let hp = actor.hp();
        (game, content, hp)
    }

    #[test]
    fn a_group_shows_the_health_of_its_most_wounded_member() {
        // The unhurt rat comes first in `visible_monsters`, the struck one
        // second: the group must not just take the first.
        let far = Position::new(PLAYER_START.x + 4, PLAYER_START.y);
        let (game, content, hp) = wounded_room(&[("rat", far), ("rat", NEXT_TO_START)], 3);
        assert_eq!(hp, 5);
        let theme = Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode);

        let lines = in_view_block(&game, &content, 10, 19, &theme);

        assert_eq!(texts(&lines), ["IN VIEW", "@ you", "v rat ×2        5/8"]);
    }

    #[test]
    fn the_most_wounded_is_the_lowest_share_of_its_maximum() {
        let mut kind = MonsterKind {
            definition: "rat",
            glyph: 'v',
            style: Style::default(),
            count: 1,
            hp: 5,
            max_hp: 10,
        };
        kind.keep_most_wounded(3, 4);
        assert_eq!((kind.hp, kind.max_hp), (5, 10), "75 % is less wounded");
        kind.keep_most_wounded(10, 20);
        assert_eq!((kind.hp, kind.max_hp), (5, 10), "a tie keeps the first");
        kind.keep_most_wounded(2, 10);
        assert_eq!((kind.hp, kind.max_hp), (2, 10));
        kind.keep_most_wounded(0, 10);
        assert_eq!((kind.hp, kind.max_hp), (0, 10));
    }

    #[test]
    fn a_long_name_keeps_a_row_and_its_count_and_health_go_below() {
        let far = Position::new(PLAYER_START.x + 4, PLAYER_START.y);
        let (game, content, hp) = wounded_room(&[("lead", far), ("lead", NEXT_TO_START)], 2);
        assert_eq!(hp, 26);
        for (glyph_set, figures) in [
            (GlyphSet::Unicode, "           ×2 26/28"),
            (GlyphSet::Ascii, "           x2 26/28"),
        ] {
            let theme = Theme::new(ColorMode::Monochrome, glyph_set);

            let lines = in_view_block(&game, &content, 10, 19, &theme);

            assert_eq!(
                texts(&lines)[2..],
                ["H lead homunculus", figures],
                "{glyph_set:?}"
            );
            assert_eq!(lines[3].width(), 19, "right-aligned");
        }

        // Alone, the health goes below the whole name as well.
        let (game, content, _) = wounded_room(&[("lead", NEXT_TO_START)], 0);
        let theme = Theme::new(ColorMode::Monochrome, GlyphSet::Ascii);
        let lines = in_view_block(&game, &content, 10, 19, &theme);
        assert_eq!(
            texts(&lines)[2..],
            ["H lead homunculus", "              28/28"]
        );
    }

    #[test]
    fn a_name_that_fits_with_its_count_and_health_keeps_one_row() {
        // "rat ×2", a blank and "5/8" take 10 of the 17 columns.
        let far = Position::new(PLAYER_START.x + 4, PLAYER_START.y);
        let shade = Position::new(PLAYER_START.x + 2, PLAYER_START.y + 2);
        let (game, content, _) =
            wounded_room(&[("rat", far), ("shade", shade), ("rat", NEXT_TO_START)], 3);
        let theme = Theme::new(ColorMode::Monochrome, GlyphSet::Unicode);

        let lines = in_view_block(&game, &content, 10, 19, &theme);

        assert_eq!(
            texts(&lines),
            [
                "IN VIEW",
                "@ you",
                "v rat ×2        5/8",
                "s shade         7/7"
            ]
        );
    }

    #[test]
    fn a_two_row_entry_is_shown_whole_or_counted_on_the_last_row() {
        // The player and a shade take two rows, the lead homunculus two
        // more: with three rows under the title it is replaced by
        // "and 1 more" rather than shown without its health.
        let shade = Position::new(PLAYER_START.x + 2, PLAYER_START.y + 2);
        let (game, content, _) = wounded_room(&[("shade", shade), ("lead", NEXT_TO_START)], 0);
        let theme = Theme::new(ColorMode::Monochrome, GlyphSet::Ascii);

        assert_eq!(
            texts(&in_view_block(&game, &content, 4, 19, &theme)),
            ["IN VIEW", "@ you", "s shade         7/7", "and 1 more"]
        );
        assert_eq!(
            texts(&in_view_block(&game, &content, 5, 19, &theme)),
            [
                "IN VIEW",
                "@ you",
                "s shade         7/7",
                "H lead homunculus",
                "              28/28"
            ]
        );
    }

    #[test]
    fn the_health_takes_the_colour_of_its_level_in_every_mode() {
        // 8/8 is high, 4/8 exactly half hence mid, 1/8 under a quarter.
        let cases = [
            (0, "8/8", HealthLevel::High),
            (4, "4/8", HealthLevel::Mid),
            (7, "1/8", HealthLevel::Low),
        ];
        let modes = [
            ColorMode::TrueColorDark,
            ColorMode::TrueColorLight,
            ColorMode::Ansi16,
            ColorMode::Monochrome,
        ];
        for (blows, figures, level) in cases {
            let (game, content, _) = wounded_room(&[("rat", NEXT_TO_START)], blows);
            for mode in modes {
                for glyph_set in [GlyphSet::Unicode, GlyphSet::Ascii] {
                    let theme = Theme::new(mode, glyph_set);
                    let lines = in_view_block(&game, &content, 10, 19, &theme);
                    let what = format!("{mode:?}, {glyph_set:?}, {figures}");

                    let line = &lines[2];
                    let health = line.spans.last().unwrap();
                    assert_eq!(health.content, figures, "{what}");
                    assert_eq!(health.style, level.style(&theme), "{what}");
                    assert_eq!(line.spans[0].style, theme.monster_hostile, "{what}");
                    assert_eq!(line.width(), 19, "{what}: fills the panel");
                    // The figures say the level without colour: no mark is
                    // added, `!` warns of the player's own danger.
                    assert!(!line.to_string().contains('!'), "{what}");
                }
            }
        }
    }

    /// The side panel of an 80 × `height` game screen for `game`: the
    /// panel's 22 columns at the right of the screen, rows between the
    /// status line and the log, borders and margins kept.
    fn panel_on_screen(game: &Game, content: &Content, height: u16, theme: &Theme) -> String {
        panel_on_screen_of(game, content, 80, height, theme)
    }

    /// [`panel_on_screen`] on a screen `width` columns wide.
    fn panel_on_screen_of(
        game: &Game,
        content: &Content,
        width: u16,
        height: u16,
        theme: &Theme,
    ) -> String {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        use crate::message_log::MessageLog;

        /// Columns of the side panel, its border included.
        const PANEL_WIDTH: u16 = 22;
        /// Rows under the panel: the log's separator, its 5 lines and the
        /// key help.
        const ROWS_BELOW: u16 = 7;
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                crate::ui::render(frame, game, &MessageLog::default(), content, None, theme);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        (1..height - ROWS_BELOW)
            .map(|y| {
                let row: String = (width - PANEL_WIDTH..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect();
                format!("\"{row}\"\n")
            })
            .collect()
    }

    /// Two lead homunculi, one struck twice, and an unhurt shade: two kinds
    /// in view, one grouped, wounded and long-named.
    fn two_kinds_one_grouped() -> (Game, Content) {
        let far = Position::new(PLAYER_START.x + 4, PLAYER_START.y);
        let shade = Position::new(PLAYER_START.x + 2, PLAYER_START.y + 2);
        let (game, content, _) = wounded_room(
            &[("lead", far), ("shade", shade), ("lead", NEXT_TO_START)],
            2,
        );
        (game, content)
    }

    #[test]
    fn monster_health_in_the_panel_of_an_80_by_24_screen() {
        let (game, content) = two_kinds_one_grouped();
        let theme = Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode);

        insta::assert_snapshot!(panel_on_screen(&game, &content, 24, &theme));
    }

    #[test]
    fn monster_health_in_the_panel_of_an_80_by_24_screen_in_ascii() {
        let (game, content) = two_kinds_one_grouped();
        let theme = Theme::new(ColorMode::Monochrome, GlyphSet::Ascii);

        insta::assert_snapshot!(panel_on_screen(&game, &content, 24, &theme));
    }

    #[test]
    fn monster_health_in_the_panel_of_a_120_by_40_screen() {
        let (game, content) = two_kinds_one_grouped();
        let theme = Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode);

        insta::assert_snapshot!(panel_on_screen_of(&game, &content, 120, 40, &theme));
    }
}

#[cfg(test)]
mod sharing_tests {
    use ratatui::buffer::Buffer;
    use sigilfall_core::action::Action;
    use sigilfall_core::game::PLAYER_START;
    use sigilfall_core::geometry::{Direction, Position};
    use sigilfall_core::rng::Seed;

    use super::*;
    use crate::theme::{ColorMode, GlyphSet};

    /// Width and height of the side panel on an 80 × 24 screen.
    const PANEL: Rect = Rect::new(0, 0, 22, 16);

    /// The fixed room where the player has read the glyphs of every
    /// shipped sigil (engraved in a row east of the start) and walked back,
    /// then with `monsters` (id, column) spawned on the start row.
    fn learned_room(monsters: &[(&str, i32)]) -> (Game, Content) {
        let content = Content::parse(
            r#"[
            Monster(id: "rat", name: "rat", family: Vermin, elite: false,
                speed: 1, hp: 1, attack: 0, defense: 0, min_depth: 1),
            Monster(id: "eye", name: "eye", family: Eye, elite: false,
                speed: 1, hp: 1, attack: 0, defense: 0, min_depth: 1),
            Monster(id: "shade", name: "shade", family: Spectre, elite: false,
                speed: 1, hp: 1, attack: 0, defense: 0, min_depth: 1),
        ]"#,
        )
        .unwrap();
        let mut game = Game::in_fixed_room(Seed::new(42));
        let mut cell = PLAYER_START;
        for sigil in content.sigils() {
            cell = cell.step(Direction::East);
            game.engrave(cell, sigil);
        }
        for direction in [Direction::East, Direction::West] {
            for _ in content.sigils() {
                game.submit(Action::Move(direction));
            }
        }
        assert_eq!(game.grimoire().len(), 5, "every glyph was read");
        for &(id, x) in monsters {
            let cell = Position::new(x, PLAYER_START.y + 1);
            game.spawn_monster(cell, content.monster(id).unwrap());
        }
        (game, content)
    }

    /// The panel drawn for `game`, one string per row, border and margins
    /// trimmed.
    fn panel(game: &Game, content: &Content) -> Vec<String> {
        let theme = Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode);
        let mut buffer = Buffer::empty(PANEL);
        render(&mut buffer, PANEL, game, content, &theme);
        (0..PANEL.height)
            .map(|y| {
                let row: String = (PANEL.x..PANEL.right())
                    .map(|x| buffer.cell((x, y)).map_or(" ", |cell| cell.symbol()))
                    .collect();
                row.trim_matches(|c: char| c == '│' || c == ' ').to_owned()
            })
            .collect()
    }

    #[test]
    fn the_names_take_what_the_list_leaves_but_keep_their_minimum() {
        // (known, rows the list wants, shared rows) -> name rows.
        let cases = [
            ((5, 3, 10), 5),  // short list: every name
            ((5, 5, 10), 5),  // player and three kinds: still every name
            ((5, 6, 10), 4),  // one kind more: one name fewer
            ((5, 9, 10), 3),  // crowded list: the minimum
            ((2, 9, 10), 2),  // fewer sigils than the minimum
            ((0, 9, 10), 1),  // "none yet"
            ((12, 3, 10), 7), // more sigils than rows: what the list leaves
            ((12, 3, 20), 12),
            ((5, 3, 2), 2), // never more than the shared rows
        ];
        for ((known, in_view, shared), expected) in cases {
            assert_eq!(
                sigil_name_rows(known, in_view, shared),
                expected,
                "{known} known, list of {in_view}, {shared} shared"
            );
        }
    }

    #[test]
    fn with_a_short_list_the_panel_names_the_five_shipped_sigils() {
        let (game, content) = learned_room(&[("rat", 7)]);

        let rows = panel(&game, &content);

        let at = rows
            .iter()
            .position(|row| row.starts_with("SIGILS"))
            .unwrap();
        assert!(rows[at].ends_with("5/5"), "{rows:#?}");
        assert_eq!(
            rows[at + 1..at + 6],
            ["Ember", "Recall", "Veil", "Passage", "Lance"],
            "{rows:#?}"
        );
        assert!(!rows.iter().any(|row| row.contains("more")), "{rows:#?}");
        // The list keeps every entry under them.
        let list = rows.iter().position(|row| row == "IN VIEW").unwrap();
        assert_eq!(
            rows[list + 1..list + 4],
            ["@ you", "v rat           1/1", "≡ known glyph ×5"]
        );
    }

    #[test]
    fn a_crowded_list_keeps_its_rows_and_the_names_their_minimum() {
        // The player, three kinds of monsters, an item and the glyphs: the
        // list wants 7 rows of the 10 shared, which leaves the names 3.
        let monsters = [("rat", 6), ("eye", 7), ("shade", 8)];
        let (mut game, content) = learned_room(&monsters);
        let draught = content.item("healing_draught").unwrap();
        game.place_item(Position::new(3, PLAYER_START.y + 1), draught);

        let rows = panel(&game, &content);

        let at = rows
            .iter()
            .position(|row| row.starts_with("SIGILS"))
            .unwrap();
        assert_eq!(
            rows[at + 1..at + 4],
            ["Ember", "Recall", "and 3 more"],
            "{rows:#?}"
        );
        let list = rows.iter().position(|row| row == "IN VIEW").unwrap();
        assert_eq!(
            rows[list + 1..],
            [
                "@ you",
                "v rat           1/1",
                "e eye           1/1",
                "s shade         1/1",
                "¡ Healing Draught",
                "≡ known glyph ×5",
            ],
            "{rows:#?}"
        );
    }
}
