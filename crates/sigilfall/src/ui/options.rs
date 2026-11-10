//! The options screen, opened from the title menu: glyphs, colours, theme,
//! language and keyboard layout, each changed in place and shown at once in
//! the screen's own colours, glyphs, words and key help.
//!
//! Exposes [`OptionRow`], the entries of the screen; [`OptionsMenu`], the
//! screen's own state (which entry is chosen); [`change`], which gives an
//! option its next or previous value; and [`render`]. The list follows the
//! title menu: the chosen line starts with the `›` marker and sits on
//! `bg_selection`, so the choice reads without colour. Under the list, a
//! sample line draws a few map glyphs in their tokens, so the effect of a
//! change is seen on the screen itself, then what the chosen entry does,
//! the notes that explain why the screen may differ from the options
//! (`--ascii`, `--lang` or `--keyboard` given, the light theme without true
//! colour) and the notice of a failed write. A language is always named in
//! itself ("English", "Français"), so a player who cannot read the language
//! in use still finds their own; a layout by its own name ("QWERTY",
//! "AZERTY"), the same in every language.
//!
//! Nothing here writes a file: the application applies and saves the
//! options. Every word comes from `messages`.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget as _;

use super::log::wrap;
use super::{draw_centred_rows, key_help, up_down};
use crate::messages::{self, Labels};
use crate::theme::{Appearance, ColorChoice, ColorSupport, GlyphSet, Options, Settings, Theme};

/// An entry of the options screen, top to bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionRow {
    /// Unicode or ASCII glyphs.
    Glyphs,
    /// Automatic colours, 16 colours or Monochrome.
    Colors,
    /// Dark or light theme.
    Theme,
    /// English or French.
    Language,
    /// QWERTY or AZERTY.
    Keyboard,
    /// Back to the title menu.
    Back,
}

/// The entries, top to bottom.
const ROWS: [OptionRow; 6] = [
    OptionRow::Glyphs,
    OptionRow::Colors,
    OptionRow::Theme,
    OptionRow::Language,
    OptionRow::Keyboard,
    OptionRow::Back,
];

/// Columns of a line taken by an option's label, before its value: the
/// longest label, "Language" or "Keyboard" (or "Couleurs"), and five blank
/// columns.
const LABEL_WIDTH: usize = 13;

/// Columns the notice of a failed write is wrapped to: the 80 columns of
/// the smallest screen less the mark "! " and a margin of one column on
/// each side. Lines are padded to it, so the notice stays aligned on its
/// left edge once centred.
const NOTICE_WIDTH: usize = 76;

/// Most notes the screen shows under the description: `--ascii` given,
/// `--lang` given, `--keyboard` given, and the light theme drawn in
/// monochrome (see [`notes`]).
const NOTES: usize = 4;

/// The options screen's own state: which entry is chosen, as a position
/// in [`ROWS`]. It opens on the first option.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OptionsMenu {
    /// Position of the chosen entry in [`ROWS`], counted from 0, always
    /// below its length.
    selected: usize,
}

impl OptionsMenu {
    /// Returns the screen as it opens, on the glyph option.
    pub const fn open() -> Self {
        Self { selected: 0 }
    }

    /// Returns the chosen entry.
    pub const fn chosen(self) -> OptionRow {
        ROWS[self.selected]
    }

    /// Returns the screen with the entry above chosen; from the first, the
    /// last.
    pub const fn previous(self) -> Self {
        Self {
            selected: (self.selected + ROWS.len() - 1) % ROWS.len(),
        }
    }

    /// Returns the screen with the entry below chosen; from the last, the
    /// first.
    pub const fn next(self) -> Self {
        Self {
            selected: (self.selected + 1) % ROWS.len(),
        }
    }

    /// Returns the screen with its last entry, "Back", chosen.
    pub const fn last() -> Self {
        Self {
            selected: ROWS.len() - 1,
        }
    }
}

/// Returns `options` with the option of `row` given its next value
/// (`forward` true) or its previous one, wrapping around: Unicode and
/// ASCII; Auto, 16 colours and monochrome; dark and light; English and
/// French; QWERTY and AZERTY. "Back" changes nothing.
pub const fn change(options: Options, row: OptionRow, forward: bool) -> Options {
    match row {
        OptionRow::Glyphs => Options {
            glyphs: match options.glyphs {
                GlyphSet::Unicode => GlyphSet::Ascii,
                GlyphSet::Ascii => GlyphSet::Unicode,
            },
            ..options
        },
        OptionRow::Colors => Options {
            colors: match (options.colors, forward) {
                (ColorChoice::Auto, true) | (ColorChoice::Monochrome, false) => ColorChoice::Ansi16,
                (ColorChoice::Ansi16, true) | (ColorChoice::Auto, false) => ColorChoice::Monochrome,
                (ColorChoice::Monochrome, true) | (ColorChoice::Ansi16, false) => ColorChoice::Auto,
            },
            ..options
        },
        OptionRow::Theme => Options {
            appearance: match options.appearance {
                Appearance::Dark => Appearance::Light,
                Appearance::Light => Appearance::Dark,
            },
            ..options
        },
        OptionRow::Language => Options {
            language: if forward {
                options.language.next()
            } else {
                options.language.previous()
            },
            ..options
        },
        OptionRow::Keyboard => Options {
            keyboard: if forward {
                options.keyboard.next()
            } else {
                options.keyboard.previous()
            },
            ..options
        },
        OptionRow::Back => options,
    }
}

/// Draws the options screen in the state `menu` over `area` of `buffer`,
/// for `settings`, with `theme` (the theme of those settings, so a change
/// shows at once): the title, one line per entry with its value, the
/// sample line, what the chosen entry does, the notes on `--ascii`, on
/// `--lang`, on `--keyboard` and on the light theme without true colour
/// when they apply,
/// and `notice`, the
/// reason the options could not be written, if any; the key help on the
/// last row.
///
/// The background is expected to be painted already; an area too small
/// for the screen clips it without panicking.
pub fn render(
    buffer: &mut Buffer,
    area: Rect,
    menu: &OptionsMenu,
    settings: &Settings,
    notice: Option<&str>,
    theme: &Theme,
) {
    let mut rows = vec![
        (
            Line::styled(theme.labels().options_title, theme.text_title),
            false,
        ),
        (Line::default(), false),
    ];
    let (entries, width) = entry_lines(*menu, settings, theme);
    rows.extend(entries);
    rows.push((Line::default(), false));
    rows.push((sample_line(width, theme), false));
    rows.push((Line::default(), false));
    rows.push((
        Line::styled(help_of(menu.chosen(), theme.labels()), theme.text_muted),
        false,
    ));
    // Centred as if both notes were shown, so the list does not move when
    // one appears.
    let steady_rows = rows.len() + NOTES;
    for note in notes(settings, theme) {
        rows.push((Line::styled(note, theme.text_muted), false));
    }
    if let Some(notice) = notice {
        rows.push((Line::default(), false));
        for (index, text) in wrap(notice, NOTICE_WIDTH).into_iter().enumerate() {
            let mark = if index == 0 {
                messages::WARNING_MARK
            } else {
                " "
            };
            rows.push((
                Line::from(vec![
                    Span::styled(format!("{mark} "), theme.log_danger),
                    Span::styled(format!("{text:<NOTICE_WIDTH$}"), theme.text),
                ]),
                false,
            ));
        }
    }
    let body = Rect {
        height: area.height.saturating_sub(1),
        ..area
    };
    draw_centred_rows(buffer, body, rows, steady_rows, theme.bg_selection);

    let help = key_help(
        vec![
            (up_down(theme), theme.labels().choose_label),
            (
                format!("{}{}", theme.glyphs.arrow_left, theme.glyphs.arrow_right),
                theme.labels().change_label,
            ),
            (
                theme.labels().escape_key.to_owned(),
                theme.labels().menu_back_label,
            ),
        ],
        theme,
    );
    let help_row = Rect {
        y: area.bottom().saturating_sub(1),
        height: area.height.min(1),
        ..area
    };
    help.render(help_row, buffer);
}

/// The lines of the entries, and their common width: the `›` marker on the
/// chosen one, the label in `text_muted`, the value in `text`; "Back"
/// alone, in `text`. Lines are padded to the same width so the list stays
/// aligned once centred; the chosen one is marked to sit on
/// `bg_selection`.
fn entry_lines(
    menu: OptionsMenu,
    settings: &Settings,
    theme: &Theme,
) -> (Vec<(Line<'static>, bool)>, usize) {
    let (separator, labels) = (theme.glyphs.separator, theme.labels());
    let entries: Vec<(&str, String)> = ROWS
        .iter()
        .map(|&row| match row {
            OptionRow::Glyphs => (
                labels.glyphs_option,
                glyphs_value(settings.options.glyphs, labels),
            ),
            OptionRow::Colors => (
                labels.colors_option,
                colors_value(settings.options.colors, settings.detected, separator, theme),
            ),
            OptionRow::Theme => (
                labels.theme_option,
                theme_value(settings.options.appearance, labels),
            ),
            OptionRow::Language => (
                labels.language_option,
                settings.options.language.endonym().to_owned(),
            ),
            OptionRow::Keyboard => (
                labels.keyboard_option,
                settings.options.keyboard.name().to_owned(),
            ),
            OptionRow::Back => (labels.back_item, String::new()),
        })
        .collect();
    let width = entries
        .iter()
        .map(|(_, value)| 2 + LABEL_WIDTH + value.chars().count())
        .max()
        .unwrap_or(0);
    let lines = entries
        .into_iter()
        .enumerate()
        .map(|(index, (label, value))| {
            let chosen = index == menu.selected;
            let marker = if chosen { theme.glyphs.selection } else { ' ' };
            let label_style = if value.is_empty() {
                theme.text
            } else {
                theme.text_muted
            };
            let pad = width - 2 - LABEL_WIDTH - value.chars().count();
            let line = Line::from(vec![
                Span::styled(format!("{marker} "), theme.text),
                Span::styled(format!("{label:<LABEL_WIDTH$}"), label_style),
                Span::styled(format!("{value}{}", " ".repeat(pad)), theme.text),
            ]);
            (line, chosen)
        })
        .collect();
    (lines, width)
}

/// The value of the glyph option, as the screen shows it in the words of
/// `labels`.
fn glyphs_value(glyphs: GlyphSet, labels: &Labels) -> String {
    match glyphs {
        GlyphSet::Unicode => labels.unicode_value,
        GlyphSet::Ascii => labels.ascii_value,
    }
    .to_owned()
}

/// The value of the colour option, as the screen shows it in the language
/// of `theme`; "Auto" says what was `detected`.
fn colors_value(
    colors: ColorChoice,
    detected: ColorSupport,
    separator: char,
    theme: &Theme,
) -> String {
    match colors {
        ColorChoice::Auto => messages::auto_colors(theme.language, detected, separator),
        ColorChoice::Ansi16 => theme.labels().ansi16_value.to_owned(),
        ColorChoice::Monochrome => theme.labels().monochrome_value.to_owned(),
    }
}

/// The value of the theme option, as the screen shows it in the words of
/// `labels`.
fn theme_value(appearance: Appearance, labels: &Labels) -> String {
    match appearance {
        Appearance::Dark => labels.dark_value,
        Appearance::Light => labels.light_value,
    }
    .to_owned()
}

/// What the entry `row` does, in the words of `labels`, shown under the
/// list while it is chosen.
const fn help_of(row: OptionRow, labels: &'static Labels) -> &'static str {
    let index = match row {
        OptionRow::Glyphs => 0,
        OptionRow::Colors => 1,
        OptionRow::Theme => 2,
        OptionRow::Language => 3,
        OptionRow::Keyboard => 4,
        OptionRow::Back => 5,
    };
    labels.option_help[index]
}

/// The notes that explain why the screen differs from the options, in the
/// language of `theme`: ASCII glyphs forced by `--ascii`, the language
/// forced by `--lang`, the layout forced by `--keyboard`, and the light
/// theme drawn in monochrome because the colours are not 24-bit.
fn notes(settings: &Settings, theme: &Theme) -> Vec<String> {
    let mut notes = Vec::new();
    if settings.forced_ascii {
        notes.push(theme.labels().ascii_forced.to_owned());
    }
    if let Some(forced) = settings.forced_language {
        notes.push(messages::language_forced(theme.language, forced));
    }
    if let Some(forced) = settings.forced_keyboard {
        notes.push(messages::keyboard_forced(theme.language, forced));
    }
    let light_in_monochrome = settings.options.appearance == Appearance::Light
        && settings.options.colors.support(settings.detected) == ColorSupport::Ansi16;
    if light_in_monochrome {
        notes.push(theme.labels().light_in_monochrome.to_owned());
    }
    notes
}

/// The sample line, padded to `width` so it lines up with the entries: the
/// label, then the player, two creatures, an item, a carved glyph, the
/// stairs, a wall and the floor, each in its token and with the theme's
/// glyphs, so a change of option shows at once.
fn sample_line(width: usize, theme: &Theme) -> Line<'static> {
    let glyphs = &theme.glyphs;
    let wall: String = std::iter::repeat_n(glyphs.wall, 3).collect();
    let floor: String = std::iter::repeat_n(glyphs.floor, 3).collect();
    let mut line = Line::from(vec![
        Span::styled(
            format!("  {:<LABEL_WIDTH$}", theme.labels().sample_label),
            theme.text_muted,
        ),
        Span::styled(glyphs.player.to_string(), theme.player),
        Span::raw(" "),
        Span::styled(glyphs.vermin.to_string(), theme.monster_hostile),
        Span::raw(" "),
        Span::styled(glyphs.spectre.to_string(), theme.monster_arcane),
        Span::raw(" "),
        Span::styled(glyphs.vial.to_string(), theme.item),
        Span::raw(" "),
        Span::styled(glyphs.engraving_unread.to_string(), theme.engraving_unread),
        Span::raw(" "),
        Span::styled(glyphs.stairs.to_string(), theme.stairs),
        Span::raw(" "),
        Span::styled(wall, theme.wall_seen),
        Span::styled(floor, theme.floor),
    ]);
    let pad = width.saturating_sub(line.width());
    line.push_span(Span::raw(" ".repeat(pad)));
    line
}
