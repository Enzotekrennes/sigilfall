//! The title screen, shown at launch and after a death: the emblem, the
//! game's name, the menu, and the two steps the menu can open on the same
//! screen, the seed entry and the confirmation before a saved game is
//! abandoned.
//!
//! Exposes the states of those three steps, each [`Copy`] so the
//! application can hold them in its `State`: [`TitleMenu`] (which entry of
//! the menu is chosen, out of [`TitleItem`]s), [`SeedEntry`] (the seed
//! being typed and why it was refused, a [`SeedProblem`]) and
//! [`Confirmation`] (the new game waiting for the player's answer); the
//! [`TitleStep`] to draw; and [`render`]. The screen follows the "Écran
//! titre" mock-up of `docs/design.md`: a traced sigil as emblem, the name
//! in `text_title`, the tagline, then the menu, whose chosen line starts
//! with the `›` marker and sits on `bg_selection` (reversed in 16 colours
//! and in monochrome), so the choice reads without colour. "Continue" is
//! listed only while a saved game can be resumed, with its depth and turn.
//!
//! Nothing here starts a game or touches a file: the application acts on
//! the entry chosen. Every word comes from `messages`.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget as _;
use sigilfall_core::rng::Seed;

use super::log::wrap;
use super::{draw_centred_rows, key_help, up_down};
use crate::messages;
use crate::theme::Theme;

/// An entry of the title menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleItem {
    /// Resume the saved game; listed only while there is one.
    Continue,
    /// Start a new game on a random seed.
    NewGame,
    /// Type a seed, then start a new game on it.
    Seed,
    /// Open the options screen.
    Options,
    /// Leave the game.
    Quit,
}

impl TitleItem {
    /// Returns the entry's label, as the menu shows it.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Continue => messages::CONTINUE_ITEM,
            Self::NewGame => messages::NEW_GAME_ITEM,
            Self::Seed => messages::SEED_ITEM,
            Self::Options => messages::OPTIONS_ITEM,
            Self::Quit => messages::QUIT_ITEM,
        }
    }
}

/// The menu while a saved game can be resumed.
const ITEMS_WITH_SAVE: [TitleItem; 5] = [
    TitleItem::Continue,
    TitleItem::NewGame,
    TitleItem::Seed,
    TitleItem::Options,
    TitleItem::Quit,
];

/// The menu while there is no saved game: "Continue" is left out rather
/// than greyed out, so every line listed does something.
const ITEMS_WITHOUT_SAVE: [TitleItem; 4] = [
    TitleItem::NewGame,
    TitleItem::Seed,
    TitleItem::Options,
    TitleItem::Quit,
];

/// Returns the entries of the menu, top to bottom: with "Continue" first
/// when `has_save` is true, without it otherwise.
pub const fn items(has_save: bool) -> &'static [TitleItem] {
    if has_save {
        &ITEMS_WITH_SAVE
    } else {
        &ITEMS_WITHOUT_SAVE
    }
}

/// The title menu's own state: which entry is chosen, as a position in
/// [`items`]. Every method takes `has_save`, since "Continue" comes and
/// goes with the save; a position past the end is clamped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TitleMenu {
    /// Position of the chosen entry in [`items`], counted from 0.
    selected: usize,
}

impl TitleMenu {
    /// Returns the menu as it opens: its first entry chosen, which is
    /// "Continue" when a game is saved and "New game" otherwise.
    pub const fn open() -> Self {
        Self { selected: 0 }
    }

    /// Returns the menu with `item` chosen, or the first entry if `item`
    /// is not listed (as "Continue" without a save).
    pub fn on(item: TitleItem, has_save: bool) -> Self {
        let selected = items(has_save)
            .iter()
            .position(|&listed| listed == item)
            .unwrap_or(0);
        Self { selected }
    }

    /// Returns the position of the chosen entry, never past the last one.
    pub fn selected(self, has_save: bool) -> usize {
        self.selected.min(items(has_save).len() - 1)
    }

    /// Returns the chosen entry.
    pub fn chosen(self, has_save: bool) -> TitleItem {
        items(has_save)[self.selected(has_save)]
    }

    /// Returns the menu with the entry above chosen; from the first, the
    /// last.
    pub fn previous(self, has_save: bool) -> Self {
        let count = items(has_save).len();
        Self {
            selected: (self.selected(has_save) + count - 1) % count,
        }
    }

    /// Returns the menu with the entry below chosen; from the last, the
    /// first.
    pub fn next(self, has_save: bool) -> Self {
        let count = items(has_save).len();
        Self {
            selected: (self.selected(has_save) + 1) % count,
        }
    }

    /// Returns the menu with its last entry, "Quit", chosen.
    pub const fn last(has_save: bool) -> Self {
        Self {
            selected: items(has_save).len() - 1,
        }
    }
}

/// Most characters the seed field holds.
///
/// The longest seed is 20 characters (18446744073709551615, or 19 in the
/// shared form); 24 leaves room to see a stray character before Enter
/// reports it.
pub const SEED_CAPACITY: usize = 24;

/// Why the seed typed was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedProblem {
    /// Enter was pressed on an empty field.
    Empty,
    /// The text is neither a number nor the shared form of a seed.
    Invalid,
}

/// The seed entry's own state: what has been typed, and why the last
/// Enter was refused, shown until the text changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeedEntry {
    /// The characters typed, the first `length` of them.
    typed: [char; SEED_CAPACITY],
    /// How many characters have been typed.
    length: usize,
    /// Why the last Enter was refused, if the text has not changed since.
    problem: Option<SeedProblem>,
}

impl SeedEntry {
    /// Returns an empty field with nothing to report.
    pub const fn open() -> Self {
        Self {
            typed: [' '; SEED_CAPACITY],
            length: 0,
            problem: None,
        }
    }

    /// Returns the text typed so far.
    pub fn text(&self) -> String {
        self.typed[..self.length].iter().collect()
    }

    /// Returns why the last Enter was refused, if the text has not changed
    /// since.
    pub const fn problem(&self) -> Option<SeedProblem> {
        self.problem
    }

    /// Returns the field with `typed` added at the end, the problem
    /// cleared. A control character, or any character once the field holds
    /// [`SEED_CAPACITY`], is not added.
    pub fn push(mut self, typed: char) -> Self {
        if typed.is_control() || self.length == SEED_CAPACITY {
            return self;
        }
        self.typed[self.length] = typed;
        self.length += 1;
        self.problem = None;
        self
    }

    /// Returns the field without its last character, the problem cleared.
    pub const fn erase(mut self) -> Self {
        self.length = self.length.saturating_sub(1);
        self.problem = None;
        self
    }

    /// Reads the seed typed, as `--seed` reads it (`Seed`'s `FromStr`), the
    /// spaces around it ignored.
    ///
    /// # Errors
    ///
    /// Returns the field with the reason to show, [`SeedProblem::Empty`] or
    /// [`SeedProblem::Invalid`], when the text is not a seed; the text is
    /// kept so the player can correct it.
    pub fn submit(self) -> Result<Seed, Self> {
        let text = self.text();
        let text = text.trim();
        let problem = if text.is_empty() {
            SeedProblem::Empty
        } else if let Ok(seed) = text.parse() {
            return Ok(seed);
        } else {
            SeedProblem::Invalid
        };
        Err(Self {
            problem: Some(problem),
            ..self
        })
    }
}

/// The confirmation's own state: the new game the player asked for, and
/// whether "Abandon" or "Keep" is chosen. It opens on "Keep", so an Enter
/// pressed by habit loses nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Confirmation {
    /// Seed of the new game: the one typed, or `None` for a random seed
    /// ("New game").
    seed: Option<Seed>,
    /// Whether "Abandon it and start anew" is chosen rather than "Keep".
    abandon: bool,
}

impl Confirmation {
    /// Returns the confirmation of a new game on `seed` (`None` for a
    /// random one), "Keep" chosen.
    pub const fn new(seed: Option<Seed>) -> Self {
        Self {
            seed,
            abandon: false,
        }
    }

    /// Returns the seed of the new game, or `None` for a random one.
    pub const fn seed(self) -> Option<Seed> {
        self.seed
    }

    /// Returns whether "Abandon it and start anew" is chosen.
    pub const fn abandons(self) -> bool {
        self.abandon
    }

    /// Returns the confirmation with the other answer chosen: with two
    /// answers, up and down both switch.
    pub const fn switch(self) -> Self {
        Self {
            abandon: !self.abandon,
            ..self
        }
    }

    /// Returns the confirmation with "Keep" chosen (`abandon` false) or
    /// "Abandon" chosen (`abandon` true).
    pub const fn choosing(self, abandon: bool) -> Self {
        Self { abandon, ..self }
    }

    /// Returns the menu entry that asked for this new game, to go back to
    /// when the player keeps the saved game: "Seed" for a typed seed, "New
    /// game" otherwise.
    pub const fn asked_from(self) -> TitleItem {
        if self.seed.is_some() {
            TitleItem::Seed
        } else {
            TitleItem::NewGame
        }
    }
}

/// What the title screen shows under the name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleStep<'state> {
    /// The menu, in this state.
    Menu(&'state TitleMenu),
    /// The seed entry, in this state.
    Seed(&'state SeedEntry),
    /// The confirmation before the saved game is abandoned.
    Confirm(&'state Confirmation),
}

/// The emblem: a sigil traced from 7 across the top, down the diagonal
/// through 5 to 1, and across the bottom, on the 3 × 3 grid of the sigil
/// overlay (6 columns and 3 rows between two nodes). `N` is a linked node,
/// `o` a free one, `-` a horizontal stroke and `/` a stroke rising to the
/// right; spaces stay blank.
const EMBLEM: [&str; 7] = [
    "N-----N-----N",
    "          /  ",
    "        /    ",
    "o     N     o",
    "    /        ",
    "  /          ",
    "N-----N-----N",
];

/// Columns of a menu line taken by an entry's label, before the detail
/// beside it ("Continue" and its depth and turn): the longest label,
/// "Continue" or "New game", and four blank columns.
const LABEL_WIDTH: usize = 12;

/// Narrowest a menu is drawn, so its left edge stays in one column (22 on
/// 80 columns, as in the mock-up) whether "Continue" and its detail are
/// listed or not, and on the confirmation's answers.
const MENU_MIN_WIDTH: usize = 36;

/// Rows of the emblem, the name, the tagline and the blank rows around
/// them.
const HEADER_ROWS: usize = EMBLEM.len() + 4;

/// Rows kept under the header for the menu, the seed entry or the
/// confirmation, the tallest of which takes six: the screen is centred as
/// if this many were drawn, so the emblem and the name never move from one
/// step to the next.
const BODY_ROWS: usize = 6;

/// Most rows the notices take, under the menu and a blank row. The emblem,
/// the name and a menu of five entries take 16 of the 23 rows above the
/// key help on an 80 × 24 screen; five rows of 76 columns hold a notice
/// with a long path and its system error, and leave one row of margin.
const NOTICE_ROWS: usize = 5;

/// Columns a notice is wrapped to: the 80 columns of the smallest screen
/// less the mark "! " and a margin of one column on each side.
const NOTICE_WIDTH: usize = 76;

/// A line of the screen, centred on its own width, and whether it is the
/// chosen line of a menu, to be drawn on `bg_selection`.
struct Row {
    /// What the line shows.
    line: Line<'static>,
    /// Whether the line is the chosen entry of a menu.
    chosen: bool,
}

impl Row {
    /// A line that is not chosen.
    const fn plain(line: Line<'static>) -> Self {
        Self {
            line,
            chosen: false,
        }
    }

    /// A blank line.
    fn blank() -> Self {
        Self::plain(Line::default())
    }
}

/// Draws the title screen over `area` of `buffer` with `theme`: the
/// emblem, the name and the tagline, then `step` (the menu, the seed entry
/// or the confirmation), then the `notices`, each after the warning mark,
/// wrapped on at most [`NOTICE_ROWS`] rows together, all centred in the
/// rows above the last (as if [`BODY_ROWS`] were drawn under the header,
/// so nothing moves from one step to the next, and raised when notices
/// need the room); the key help of `step` on the last row, with the
/// version at its right end on the menu.
///
/// `saved` describes the saved game (see `messages::saved_run`), beside
/// "Continue" and in the confirmation's warning; `None` when no game is
/// saved, which leaves "Continue" out of the menu.
///
/// The background is expected to be painted already; an area too small
/// for the screen clips it without panicking.
pub fn render(
    buffer: &mut Buffer,
    area: Rect,
    step: TitleStep<'_>,
    saved: Option<&str>,
    notices: &[String],
    theme: &Theme,
) {
    let mut rows = emblem(theme);
    rows.extend([
        Row::blank(),
        Row::plain(Line::styled(messages::GAME_TITLE, theme.text_title)),
        Row::plain(Line::styled(messages::TAGLINE, theme.text_muted)),
        Row::blank(),
    ]);
    let help = match step {
        TitleStep::Menu(menu) => {
            rows.extend(menu_rows(*menu, saved, theme));
            menu_help(theme)
        }
        TitleStep::Seed(entry) => {
            rows.extend(seed_rows(entry, theme));
            seed_help(theme)
        }
        TitleStep::Confirm(confirmation) => {
            rows.extend(confirmation_rows(*confirmation, saved, theme));
            confirmation_help(theme)
        }
    };
    let notice_rows = notice_rows(notices, theme);
    if !notice_rows.is_empty() {
        rows.push(Row::blank());
        rows.extend(notice_rows);
    }
    let body = Rect {
        height: area.height.saturating_sub(1),
        ..area
    };
    draw_centred(buffer, body, rows, theme);

    let help_row = Rect {
        y: area.bottom().saturating_sub(1),
        height: area.height.min(1),
        ..area
    };
    help.render(help_row, buffer);
    if matches!(step, TitleStep::Menu(_)) {
        // Drawn in its own cells only: a line drawn over the whole row
        // would restyle the key help.
        let version = Line::styled(format!("{} ", messages::version()), theme.text_muted);
        let width = u16::try_from(version.width())
            .unwrap_or(u16::MAX)
            .min(help_row.width);
        let version_area = Rect {
            x: help_row.right() - width,
            width,
            ..help_row
        };
        version.render(version_area, buffer);
    }
}

/// The rows of the emblem ([`EMBLEM`]), linked nodes and strokes in
/// `sigil_stroke`, free nodes in `sigil_node`, with the theme's glyphs.
fn emblem(theme: &Theme) -> Vec<Row> {
    let glyphs = &theme.glyphs;
    EMBLEM
        .iter()
        .map(|pattern| {
            let spans: Vec<Span<'static>> = pattern
                .chars()
                .map(|cell| match cell {
                    'N' => Span::styled(glyphs.sigil_linked.to_string(), theme.sigil_stroke),
                    'o' => Span::styled(glyphs.sigil_node.to_string(), theme.sigil_node),
                    '-' => Span::styled(glyphs.stroke_horizontal.to_string(), theme.sigil_stroke),
                    '/' => Span::styled(glyphs.stroke_rising.to_string(), theme.sigil_stroke),
                    _ => Span::raw(" "),
                })
                .collect();
            Row::plain(Line::from(spans))
        })
        .collect()
}

/// The lines of a menu, one per entry: the `›` marker on the chosen one,
/// the label in `text`, and its `detail` in `text_muted` (empty for most
/// entries). Lines are padded to the same width, at least
/// [`MENU_MIN_WIDTH`], so the menu stays aligned on its left edge once
/// centred, in the same column from one menu to the next.
fn menu_lines(entries: &[(&'static str, String)], chosen: usize, theme: &Theme) -> Vec<Row> {
    let width = entries
        .iter()
        .map(|(label, detail)| 2 + LABEL_WIDTH.max(label.chars().count()) + detail.chars().count())
        .max()
        .unwrap_or(0)
        .max(MENU_MIN_WIDTH);
    entries
        .iter()
        .enumerate()
        .map(|(index, (label, detail))| {
            let marker = if index == chosen {
                theme.glyphs.selection
            } else {
                ' '
            };
            let used = 2 + LABEL_WIDTH.max(label.chars().count()) + detail.chars().count();
            let line = Line::from(vec![
                Span::styled(format!("{marker} {label:<LABEL_WIDTH$}"), theme.text),
                Span::styled(
                    format!("{detail}{}", " ".repeat(width - used)),
                    theme.text_muted,
                ),
            ]);
            Row {
                line,
                chosen: index == chosen,
            }
        })
        .collect()
}

/// The lines of the title menu in the state `menu`; `saved` is written
/// beside "Continue".
fn menu_rows(menu: TitleMenu, saved: Option<&str>, theme: &Theme) -> Vec<Row> {
    let has_save = saved.is_some();
    let entries: Vec<(&'static str, String)> = items(has_save)
        .iter()
        .map(|&item| {
            let detail = match (item, saved) {
                (TitleItem::Continue, Some(saved)) => saved.to_owned(),
                _ => String::new(),
            };
            (item.label(), detail)
        })
        .collect();
    menu_lines(&entries, menu.selected(has_save), theme)
}

/// The lines of the seed entry: the prompt, the field with what is typed
/// and a `_` cursor on `bg_selection` (reversed without colour, so the
/// field reads as one), the accepted forms, and the reason of a refusal,
/// whose row is kept blank otherwise so nothing moves when it appears.
fn seed_rows(entry: &SeedEntry, theme: &Theme) -> Vec<Row> {
    let typed = entry.text();
    let blank = SEED_CAPACITY - typed.chars().count();
    let field = format!("{typed}_{}", " ".repeat(blank));
    let problem = match entry.problem() {
        None => Line::default(),
        Some(problem) => {
            let text = match problem {
                SeedProblem::Empty => messages::EMPTY_SEED.to_owned(),
                SeedProblem::Invalid => messages::invalid_seed(typed.trim()),
            };
            warning(text, theme)
        }
    };
    vec![
        Row::plain(Line::styled(messages::SEED_PROMPT, theme.text_muted)),
        Row::blank(),
        Row::plain(Line::from(vec![
            Span::styled(
                format!("{}  ", messages::SEED_FIELD_LABEL),
                theme.text_muted,
            ),
            Span::styled(field, theme.text.patch(theme.bg_selection)),
        ])),
        Row::blank(),
        Row::plain(Line::styled(messages::SEED_FORMS, theme.text_muted)),
        Row::plain(problem),
    ]
}

/// The lines of the confirmation: the question in `text_title`, the
/// warning that the game in progress, described by `saved`, will be
/// abandoned and its save deleted, and the two answers as a menu.
fn confirmation_rows(confirmation: Confirmation, saved: Option<&str>, theme: &Theme) -> Vec<Row> {
    let mut rows = vec![Row::plain(Line::styled(
        messages::ABANDON_QUESTION,
        theme.text_title,
    ))];
    rows.extend(
        messages::abandon_warning(saved.unwrap_or_default())
            .into_iter()
            .map(|text| Row::plain(Line::styled(text, theme.text))),
    );
    rows.push(Row::blank());
    let answers = [
        (messages::KEEP_ITEM, String::new()),
        (messages::ABANDON_ITEM, String::new()),
    ];
    rows.extend(menu_lines(
        &answers,
        usize::from(confirmation.abandons()),
        theme,
    ));
    rows
}

/// The lines of the `notices`, each wrapped to [`NOTICE_WIDTH`] after the
/// warning mark, at most [`NOTICE_ROWS`] in all; the rest is cut.
fn notice_rows(notices: &[String], theme: &Theme) -> Vec<Row> {
    notices
        .iter()
        .flat_map(|notice| {
            wrap(notice, NOTICE_WIDTH)
                .into_iter()
                .enumerate()
                .map(|(index, text)| {
                    let mark = if index == 0 {
                        messages::WARNING_MARK
                    } else {
                        " "
                    };
                    // Padded so the lines of a notice stay aligned once
                    // centred.
                    Row::plain(Line::from(vec![
                        Span::styled(format!("{mark} "), theme.log_danger),
                        Span::styled(format!("{text:<NOTICE_WIDTH$}"), theme.text),
                    ]))
                })
        })
        .take(NOTICE_ROWS)
        .collect()
}

/// A line telling a problem: the warning mark in `log_danger`, then `text`
/// in `text`, so it reads without colour.
fn warning(text: String, theme: &Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{} ", messages::WARNING_MARK), theme.log_danger),
        Span::styled(text, theme.text),
    ])
}

/// The key help of the menu: "↑↓ choose   Enter confirm   q quit".
fn menu_help(theme: &Theme) -> Line<'static> {
    key_help(
        vec![
            (up_down(theme), messages::CHOOSE_LABEL),
            (messages::CONFIRM_KEY.to_owned(), messages::CONFIRM_LABEL),
            (messages::QUIT_KEY.to_owned(), messages::QUIT_LABEL),
        ],
        theme,
    )
}

/// The key help of the seed entry: "Enter start   Backspace erase   Esc
/// back".
fn seed_help(theme: &Theme) -> Line<'static> {
    key_help(
        vec![
            (messages::CONFIRM_KEY.to_owned(), messages::START_LABEL),
            (messages::UNDO_KEY.to_owned(), messages::ERASE_LABEL),
            (messages::CANCEL_KEY.to_owned(), messages::MENU_BACK_LABEL),
        ],
        theme,
    )
}

/// The key help of the confirmation: "↑↓ choose   Enter confirm   Esc keep
/// it".
fn confirmation_help(theme: &Theme) -> Line<'static> {
    key_help(
        vec![
            (up_down(theme), messages::CHOOSE_LABEL),
            (messages::CONFIRM_KEY.to_owned(), messages::CONFIRM_LABEL),
            (messages::CANCEL_KEY.to_owned(), messages::KEEP_LABEL),
        ],
        theme,
    )
}

/// Draws `rows` centred in `area` as if the header and [`BODY_ROWS`] were
/// drawn (see `ui::draw_centred_rows`), the chosen line on `bg_selection`.
fn draw_centred(buffer: &mut Buffer, area: Rect, rows: Vec<Row>, theme: &Theme) {
    let rows = rows.into_iter().map(|row| (row.line, row.chosen)).collect();
    draw_centred_rows(
        buffer,
        area,
        rows,
        HEADER_ROWS + BODY_ROWS,
        theme.bg_selection,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continue_is_listed_first_only_with_a_save() {
        assert_eq!(items(true)[0], TitleItem::Continue);
        assert!(!items(false).contains(&TitleItem::Continue));
        assert_eq!(TitleMenu::open().chosen(true), TitleItem::Continue);
        assert_eq!(TitleMenu::open().chosen(false), TitleItem::NewGame);
        assert_eq!(
            TitleMenu::on(TitleItem::Continue, false).chosen(false),
            TitleItem::NewGame,
            "an entry not listed falls back on the first"
        );
    }

    #[test]
    fn the_menu_wraps_both_ways() {
        for has_save in [true, false] {
            let count = items(has_save).len();
            let mut menu = TitleMenu::open();
            for _ in 0..count {
                menu = menu.next(has_save);
            }
            assert_eq!(menu.chosen(has_save), items(has_save)[0]);
            assert_eq!(
                TitleMenu::open().previous(has_save).chosen(has_save),
                TitleItem::Quit
            );
            assert_eq!(TitleMenu::last(has_save).chosen(has_save), TitleItem::Quit);
        }
        // A position kept from a menu with "Continue" is clamped without it.
        assert_eq!(TitleMenu::last(true).chosen(false), TitleItem::Quit);
    }

    #[test]
    fn the_seed_field_keeps_printable_characters_up_to_its_capacity() {
        let mut entry = SeedEntry::open();
        for typed in "7F3A\t\n-91C2".chars() {
            entry = entry.push(typed);
        }
        assert_eq!(entry.text(), "7F3A-91C2", "controls are not typed");

        for _ in 0..40 {
            entry = entry.push('9');
        }
        assert_eq!(entry.text().chars().count(), SEED_CAPACITY);
        assert_eq!(SeedEntry::open().erase(), SeedEntry::open());
    }

    #[test]
    fn a_submitted_seed_is_read_as_the_command_line_reads_it() {
        let typed = |text: &str| text.chars().fold(SeedEntry::open(), SeedEntry::push);

        assert_eq!(typed("42").submit(), Ok(Seed::new(42)));
        assert_eq!(
            typed(" 7f3a-91c2-0b5e-d4a8 ").submit(),
            Ok(Seed::new(0x7F3A_91C2_0B5E_D4A8))
        );
        let refused = typed("7F3A-91C2").submit().unwrap_err();
        assert_eq!(refused.problem(), Some(SeedProblem::Invalid));
        assert_eq!(refused.text(), "7F3A-91C2");
        assert_eq!(
            typed("   ").submit().unwrap_err().problem(),
            Some(SeedProblem::Empty)
        );
        assert_eq!(refused.push('-').problem(), None, "typing clears it");
        assert_eq!(refused.erase().problem(), None, "erasing too");
    }

    #[test]
    fn the_confirmation_opens_on_keep_and_remembers_what_asked() {
        let random = Confirmation::new(None);
        let typed = Confirmation::new(Some(Seed::new(42)));

        assert!(!random.abandons());
        assert!(random.switch().abandons());
        assert!(!random.switch().switch().abandons());
        assert!(random.choosing(true).abandons());
        assert_eq!(random.asked_from(), TitleItem::NewGame);
        assert_eq!(typed.asked_from(), TitleItem::Seed);
        assert_eq!(typed.switch().seed(), Some(Seed::new(42)));
    }
}
