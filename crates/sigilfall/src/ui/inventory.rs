//! The inventory, drawn over the map while the player looks at what they
//! carry: one line per item with its slot, glyph, name and effect, the
//! chosen one marked, and how full the pack is.
//!
//! Exposes [`InventoryScreen`], the screen's own state (which item is
//! chosen, and why the game last refused an action on one), and
//! [`render`]. The overlay takes the place and the size of the sigil
//! overlay (`ui::sigil`): a `bg_panel` frame over the map, the side panel
//! and the log left in view. Items come in the order they were picked up,
//! numbered from 1 to 0 like the digit keys that choose them. The chosen
//! line starts with the `›` marker and sits on `bg_selection`, which is
//! reversed in 16 colours and in monochrome, so the choice reads without
//! colour. An empty pack explains how to pick an item up.
//!
//! Nothing here acts on the game: the application submits `Action::Use`
//! and `Action::Drop` for the chosen item and tells this state about a
//! refusal.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget as _;
use sigilfall_core::content::ItemEffect;
use sigilfall_core::game::Game;
use sigilfall_core::item::{Item, ItemId, ItemRefusal};

use super::log::wrap;
use super::sigil::{draw_frame, draw_lines, overlay_area};
use crate::messages;
use crate::theme::Theme;

/// Columns kept blank inside each side of the frame, so the list and the
/// highlight of the chosen line never touch the border.
const SIDE_MARGIN: u16 = 1;

/// Row of the first item inside the frame, under one blank row.
const LIST_TOP: u16 = 1;

/// Blank columns between the name column and the effects.
const NAME_GAP: usize = 2;

/// Slots numbered by the digit keys: `1` to `9`, then `0` for the tenth.
const DIGITS: usize = 10;

/// Most rows a refusal takes at the bottom of the frame: the blank row
/// under a full list of `item::INVENTORY_CAPACITY` items and the last row.
/// Two rows of the list's 53 columns hold the longest refusal, with an item
/// and a sigil name of 24 characters each.
const NOTICE_ROWS: usize = 2;

/// Columns the continuation of a notice is indented by, so it lines up
/// under the text after the warning mark ("! ").
const NOTICE_INDENT: usize = 2;

/// The inventory's own state: which item is chosen and what the game last
/// refused. Opening the inventory always starts on the first item with
/// nothing to tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InventoryScreen {
    /// Position of the chosen item in the inventory, counted from 0. Kept
    /// below the number of items carried by every move; drawing clamps it
    /// as well.
    selected: usize,
    /// The item and the reason of the last action the game refused, shown
    /// under the list until the choice changes.
    refusal: Option<(ItemId, ItemRefusal)>,
}

impl InventoryScreen {
    /// Returns the state of a newly opened inventory: first item chosen,
    /// nothing refused.
    pub const fn open() -> Self {
        Self {
            selected: 0,
            refusal: None,
        }
    }

    /// Returns the position of the chosen item, counted from 0, out of
    /// `carried` items: never past the last one, and 0 for an empty pack.
    pub fn selected(&self, carried: usize) -> usize {
        self.selected.min(carried.saturating_sub(1))
    }

    /// Returns the item and the reason of the last action the game
    /// refused, if the choice has not changed since.
    pub const fn refusal(&self) -> Option<(ItemId, ItemRefusal)> {
        self.refusal
    }

    /// Returns this state with the item above the chosen one chosen, out of
    /// `carried` items; from the first, the last. Clears the refusal.
    pub fn previous(self, carried: usize) -> Self {
        let selected = self.selected(carried);
        let above = if selected == 0 {
            carried.saturating_sub(1)
        } else {
            selected - 1
        };
        Self::choosing(above)
    }

    /// Returns this state with the item below the chosen one chosen, out of
    /// `carried` items; from the last, the first. Clears the refusal.
    pub fn next(self, carried: usize) -> Self {
        let below = self.selected(carried) + 1;
        Self::choosing(if below >= carried { 0 } else { below })
    }

    /// Returns this state with the item in `slot` chosen, counted from 0,
    /// if one of the `carried` items is there; otherwise the state is kept
    /// as it is, refusal included, since nothing changed.
    pub const fn slot(self, slot: usize, carried: usize) -> Self {
        if slot < carried {
            Self::choosing(slot)
        } else {
            self
        }
    }

    /// Returns this state with the last of `carried` items chosen. Clears
    /// the refusal.
    pub const fn last(carried: usize) -> Self {
        Self::choosing(carried.saturating_sub(1))
    }

    /// Returns this state with the game's refusal of `reason` for `item`
    /// to show, the choice kept.
    pub const fn refused(self, item: ItemId, reason: ItemRefusal) -> Self {
        Self {
            refusal: Some((item, reason)),
            ..self
        }
    }

    /// A state with the item at `selected` chosen and nothing refused.
    const fn choosing(selected: usize) -> Self {
        Self {
            selected,
            refusal: None,
        }
    }
}

/// Draws the inventory of the player of `game`, in the state `screen`,
/// over the map `area` of `buffer`, with `theme`.
///
/// The overlay is the sigil overlay's rectangle (`ui::sigil::overlay_area`):
/// a frame titled "Inventory" with the count carried out of the capacity,
/// "3/10", on its top edge. Inside, under a blank row, one line per item:
/// the `›` marker on the chosen one, the slot's digit, the item's glyph in
/// `item`, its name in `text` and what it does in `text_muted`, names and
/// effects in aligned columns. The chosen line sits on `bg_selection`. An
/// empty pack shows how to pick an item up instead. The last rows tell the
/// refusal of `screen`, if any, after the warning mark, wrapped on at most
/// [`NOTICE_ROWS`] rows. On an area too small for a frame, nothing is
/// drawn; text that does not fit is cut.
pub fn render(
    buffer: &mut Buffer,
    area: Rect,
    screen: &InventoryScreen,
    game: &Game,
    theme: &Theme,
) {
    let frame = overlay_area(area);
    if frame.width < 2 || frame.height < 2 {
        return;
    }
    let items: Vec<(ItemId, &Item)> = game.inventory().collect();
    draw_frame(buffer, frame, messages::INVENTORY_TITLE, theme);
    draw_capacity(buffer, frame, items.len(), theme);
    let inner = Rect {
        x: frame.x + 1,
        y: frame.y + 1,
        width: frame.width - 2,
        height: frame.height - 2,
    };
    let list = list_area(inner);
    if items.is_empty() {
        let lines = messages::empty_inventory()
            .iter()
            .map(|text| empty_state_line(text, theme))
            .collect();
        draw_lines(buffer, list, lines);
    } else {
        let selected = screen.selected(items.len());
        draw_lines(buffer, list, item_lines(&items, selected, game, theme));
        let row = list.y + u16::try_from(selected).unwrap_or(u16::MAX);
        if row < list.bottom() {
            buffer.set_style(
                Rect {
                    y: row,
                    height: 1,
                    ..list
                },
                theme.bg_selection,
            );
        }
    }
    if let Some((item, reason)) = screen.refusal() {
        let text = messages::item_refusal(game, item, reason);
        draw_notice(buffer, list, &text, theme);
    }
}

/// Writes `text` at the bottom of the `list` area in `text`, after the
/// warning mark, wrapped to the list's width with its continuation
/// indented by [`NOTICE_INDENT`], on at most [`NOTICE_ROWS`] rows.
fn draw_notice(buffer: &mut Buffer, list: Rect, text: &str, theme: &Theme) {
    let width = usize::from(list.width).saturating_sub(NOTICE_INDENT);
    let lines: Vec<Line<'static>> = wrap(text, width)
        .into_iter()
        .take(NOTICE_ROWS)
        .enumerate()
        .map(|(index, line)| {
            let lead = if index == 0 {
                format!("{} ", messages::WARNING_MARK)
            } else {
                " ".repeat(NOTICE_INDENT)
            };
            Line::styled(format!("{lead}{line}"), theme.text)
        })
        .collect();
    let rows = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .min(list.height);
    let area = Rect {
        y: list.bottom() - rows,
        height: rows,
        ..list
    };
    draw_lines(buffer, area, lines);
}

/// Writes the count carried out of the capacity, " 3/10 ", in `text` on
/// the top edge of `frame`, ending two columns before its right corner as
/// the title starts two columns after its left one.
fn draw_capacity(buffer: &mut Buffer, frame: Rect, carried: usize, theme: &Theme) {
    let tag = format!(" {} ", messages::capacity(carried));
    let width = u16::try_from(tag.chars().count()).unwrap_or(u16::MAX);
    let Some(x) = frame.right().checked_sub(2 + width) else {
        return;
    };
    if x < frame.x + 2 {
        return;
    }
    Line::styled(tag, theme.text).render(
        Rect {
            x,
            y: frame.y,
            width,
            height: 1,
        },
        buffer,
    );
}

/// The area of the list inside the frame's `inner` area: from [`LIST_TOP`]
/// down, between the side margins.
fn list_area(inner: Rect) -> Rect {
    let x = inner.x + SIDE_MARGIN.min(inner.width);
    let right = inner.right().saturating_sub(SIDE_MARGIN).max(x);
    let y = inner.y + LIST_TOP.min(inner.height);
    Rect {
        x,
        y,
        width: right - x,
        height: inner.bottom() - y,
    }
}

/// One line per carried item of `items`, the one at `selected` marked:
/// marker, slot digit, glyph, name padded to the longest name carried,
/// then what the item does (see [`render`]).
fn item_lines(
    items: &[(ItemId, &Item)],
    selected: usize,
    game: &Game,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let glyphs = &theme.glyphs;
    let name_width = items
        .iter()
        .map(|(_, item)| item.definition().name().chars().count())
        .max()
        .unwrap_or(0);
    items
        .iter()
        .enumerate()
        .map(|(index, (_, item))| {
            let definition = item.definition();
            let known = match definition.effect() {
                ItemEffect::Teach(sigil) => game.grimoire().knows(sigil.id()),
                _ => false,
            };
            let marker = if index == selected {
                glyphs.selection
            } else {
                ' '
            };
            let name = definition.name();
            let padding = name_width - name.chars().count() + NAME_GAP;
            Line::from(vec![
                Span::styled(format!("{marker} "), theme.text),
                Span::styled(format!("{} ", (index + 1) % DIGITS), theme.text_muted),
                Span::styled(format!(" {}", glyphs.item(definition.family())), theme.item),
                Span::styled(format!(" {name}{}", " ".repeat(padding)), theme.text),
                Span::styled(
                    messages::item_summary(definition, known, glyphs.separator),
                    theme.text_muted,
                ),
            ])
        })
        .collect()
}

/// One line of the empty state, in `text_muted`, with the `{glyph}` mark
/// replaced by the map glyph of a vial in `item`.
fn empty_state_line(text: &str, theme: &Theme) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, part) in text.split("{glyph}").enumerate() {
        if index > 0 {
            spans.push(Span::styled(theme.glyphs.vial.to_string(), theme.item));
        }
        spans.push(Span::styled(part.to_owned(), theme.text_muted));
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn up_and_down_wrap_around_the_list() {
        let screen = InventoryScreen::open();

        assert_eq!(screen.selected(3), 0);
        assert_eq!(
            screen.previous(3).selected(3),
            2,
            "from the first, the last"
        );
        assert_eq!(screen.next(3).selected(3), 1);
        assert_eq!(screen.next(3).next(3).next(3).selected(3), 0);
        assert_eq!(InventoryScreen::last(3).selected(3), 2);
    }

    #[test]
    fn a_slot_beyond_the_items_carried_changes_nothing() {
        let item = ItemId::default();
        let screen = InventoryScreen::open().refused(item, ItemRefusal::CellTaken);

        assert_eq!(screen.slot(5, 3), screen, "slot 6 of 3 items");
        let chosen = screen.slot(2, 3);
        assert_eq!(chosen.selected(3), 2);
        assert_eq!(chosen.refusal(), None, "a new choice clears the notice");
    }

    #[test]
    fn the_choice_never_points_past_the_last_item() {
        let screen = InventoryScreen::open().slot(4, 5);

        assert_eq!(screen.selected(5), 4);
        assert_eq!(screen.selected(2), 1, "two items left");
        assert_eq!(screen.selected(0), 0, "empty pack");
        assert_eq!(InventoryScreen::open().previous(0).selected(0), 0);
        assert_eq!(InventoryScreen::open().next(0).selected(0), 0);
    }

    #[test]
    fn a_refusal_keeps_the_choice() {
        let item = ItemId::default();
        let screen = InventoryScreen::open()
            .slot(1, 3)
            .refused(item, ItemRefusal::NothingToLearn);

        assert_eq!(screen.selected(3), 1);
        assert_eq!(screen.refusal(), Some((item, ItemRefusal::NothingToLearn)));
        assert_eq!(screen.next(3).refusal(), None);
    }
}
