//! The side panel right of the map: the player's health, in figures and as
//! a bar, then the list of what the player sees.
//!
//! Exposes [`render`], the health thresholds ([`HP_HIGH_ABOVE_PERCENT`],
//! [`HP_LOW_BELOW_PERCENT`]), the bar's width ([`HP_BAR_WIDTH`]) and the
//! [`HealthLevel`] they decide. The layout follows the game screen of
//! `docs/design.md`: a border down the left, "HP" with its value, the bar,
//! a blank row, then "IN VIEW" with the player and one line per kind of
//! monster in view. The sigils block of the mock-up arrives with the sigils
//! (T19), between the two.
//!
//! Health is never told by colour alone: the value is written out, the bar
//! is drawn with two glyphs, and below a quarter the value is preceded by
//! `!`.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget as _;
use sigilfall_core::content::Content;
use sigilfall_core::game::Game;

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
/// column, then the health block and the "in view" list for `game`, with
/// monster names from `content`.
///
/// The list takes every row left under the health block. When more kinds
/// of monsters are in view than rows, the last row says how many more there
/// are. Text wider than the panel is cut. An empty `area` draws nothing.
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
    let mut lines = health_block(game, content_area.width, theme);
    lines.push(Line::default());
    let list_rows = usize::from(content_area.height).saturating_sub(lines.len());
    lines.extend(in_view_block(
        game,
        content,
        list_rows,
        content_area.width,
        theme,
    ));
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
        format!("{} {hp}/{max_hp}", messages::LOW_HP_MARK)
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

/// One line of the "in view" list: a glyph in its own style, then a name.
struct Entry {
    /// The glyph the thing is drawn with on the map.
    glyph: char,
    /// Style of that glyph, the same as on the map.
    glyph_style: Style,
    /// Name, possibly followed by a count.
    name: String,
}

/// The "in view" block in at most `rows` rows of `width` columns: its
/// title, the player, then one entry per kind of monster in view, in the
/// order of `Game::visible_monsters`, with "×2" after the name when several
/// of a kind are in view.
fn in_view_block(
    game: &Game,
    content: &Content,
    rows: usize,
    width: u16,
    theme: &Theme,
) -> Vec<Line<'static>> {
    if rows == 0 {
        return Vec::new();
    }
    let mut entries = vec![Entry {
        glyph: theme.glyphs.player,
        glyph_style: theme.player,
        name: messages::PLAYER_IN_VIEW.to_owned(),
    }];
    let mut kinds: Vec<(&str, char, Style, usize)> = Vec::new();
    for monster in game.visible_monsters() {
        if let Some(kind) = kinds.iter_mut().find(|kind| kind.0 == monster.definition) {
            kind.3 += 1;
        } else {
            let glyph = theme.glyphs.creature(monster.family, monster.elite);
            kinds.push((monster.definition, glyph, theme.creature(monster.family), 1));
        }
    }
    let name_width = usize::from(width).saturating_sub(2);
    entries.extend(kinds.into_iter().map(|(definition, glyph, style, count)| {
        let name = messages::monster_name(content, definition);
        Entry {
            glyph,
            glyph_style: style,
            name: counted_name(name, count, name_width, theme.glyphs.dimension),
        }
    }));

    let mut lines = vec![Line::styled(messages::IN_VIEW_TITLE, theme.text_muted)];
    // Rows under the title; when the entries overflow them, the last one
    // tells how many entries it replaces.
    let room = rows - 1;
    let overflows = entries.len() > room;
    let shown = if overflows {
        room.saturating_sub(1)
    } else {
        entries.len()
    };
    let hidden = entries.len() - shown;
    lines.extend(entries.into_iter().take(shown).map(|entry| {
        Line::from(vec![
            Span::styled(entry.glyph.to_string(), entry.glyph_style),
            Span::styled(format!(" {}", entry.name), theme.text),
        ])
    }));
    if overflows && room > 0 {
        lines.push(Line::styled(
            messages::more_in_view(hidden),
            theme.text_muted,
        ));
    }
    lines
}

/// `name` followed by `×count` when `count` is above 1, cut so the whole
/// fits in `width` characters; the count is kept and the name shortened.
fn counted_name(name: &str, count: usize, width: usize, times: char) -> String {
    let suffix = if count > 1 {
        format!(" {times}{count}")
    } else {
        String::new()
    };
    let room = width.saturating_sub(suffix.chars().count());
    let short: String = name.chars().take(room).collect();
    format!("{}{suffix}", short.trim_end())
}

#[cfg(test)]
mod tests {
    use sigilfall_core::game::PLAYER_START;
    use sigilfall_core::geometry::Position;
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
            ["IN VIEW", "@ you", "v rat ×2", "e eye", "s shade"]
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
            ["IN VIEW", "@ you", "v rat x2", "and 2 more"]
        );
        assert_eq!(
            texts(&in_view_block(&game, &content, 1, 19, &theme)),
            ["IN VIEW"]
        );
        assert!(in_view_block(&game, &content, 0, 19, &theme).is_empty());
    }
}
