//! The visual theme: every colour, emphasis and glyph the screens draw with.
//!
//! Exposes [`Theme`], whose fields are the semantic colour tokens (one
//! [`Style`] each) plus the [`Glyphs`] to draw with, the four rendering modes
//! ([`ColorMode`]), terminal colour detection ([`ColorSupport::detect`]) and
//! the player's preferences that feed it ([`Appearance`], [`GlyphSet`],
//! [`ColorChoice`], gathered in the [`Options`] the options screen edits),
//! resolved into a theme by [`Settings`]. This module is the only place where
//! a colour or a glyph is written down: screens ask for a token by name and
//! never pick a colour themselves. Reading and writing the options is left to
//! `persist`.
//!
//! Token styles carry a foreground and emphasis only. The background is the
//! [`Theme::bg`] token, painted once over the whole screen before anything
//! else is drawn, so every cell sits on the theme's background.

mod glyphs;

use std::env;
use std::ffi::OsString;

use ratatui::style::{Color, Modifier, Style};
use serde::{Deserialize, Serialize};
use sigilfall_core::content::Family;

pub use self::glyphs::{GlyphSet, Glyphs};

/// How much colour the terminal can show, as detected at start-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorSupport {
    /// 24-bit colour: the RGB values of the theme are shown as is.
    TrueColor,
    /// The 16 standard ANSI colours, whose exact shades depend on the
    /// terminal's colour scheme.
    Ansi16,
    /// No colour at all, at the user's request (`NO_COLOR`).
    None,
}

impl ColorSupport {
    /// Detects the colour support of the terminal the game runs in, from
    /// the process environment (see [`ColorSupport::from_env`] for the
    /// rules).
    pub fn detect() -> Self {
        Self::from_env(|name| env::var_os(name))
    }

    /// Decides the colour support from environment variables, read through
    /// `var` (which returns a variable's value, or `None` when it is unset).
    ///
    /// Rules, first match wins:
    /// 1. `NO_COLOR` set to a non-empty value → [`ColorSupport::None`], as
    ///    the convention at <https://no-color.org> asks;
    /// 2. `COLORTERM` equal to `truecolor` or `24bit` (any case) →
    ///    [`ColorSupport::TrueColor`]; most terminals with 24-bit colour
    ///    announce it this way;
    /// 3. `WT_SESSION` set → [`ColorSupport::TrueColor`]: Windows Terminal
    ///    supports 24-bit colour but does not set `COLORTERM`, and sets this
    ///    variable in every shell it starts;
    /// 4. anything else → [`ColorSupport::Ansi16`], which every terminal
    ///    supports (macOS Terminal.app, the Linux console, old Windows
    ///    consoles).
    ///
    /// An unreadable value (not valid Unicode) counts as not matching.
    pub fn from_env(var: impl Fn(&str) -> Option<OsString>) -> Self {
        if var("NO_COLOR").is_some_and(|value| !value.is_empty()) {
            return Self::None;
        }
        let colorterm = var("COLORTERM");
        let announces_true_color = colorterm
            .as_deref()
            .and_then(|value| value.to_str())
            .is_some_and(|value| {
                value.eq_ignore_ascii_case("truecolor") || value.eq_ignore_ascii_case("24bit")
            });
        if announces_true_color || var("WT_SESSION").is_some() {
            return Self::TrueColor;
        }
        Self::Ansi16
    }
}

/// The player's choice between the dark theme and the light "parchment"
/// theme. Written as `Dark` or `Light` in the options file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Appearance {
    /// Light text on ink (the default).
    #[default]
    Dark,
    /// Dark text on parchment. Needs 24-bit colour: with 16 colours it is
    /// drawn in monochrome (see [`ColorMode::for_terminal`]).
    Light,
}

/// The player's choice of colours, in the options screen. Written as
/// `Auto`, `Ansi16` or `Monochrome` in the options file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ColorChoice {
    /// What the terminal supports, as detected at start-up
    /// ([`ColorSupport::detect`], which honours `NO_COLOR`). The default.
    #[default]
    Auto,
    /// The 16 ANSI colours, even on a terminal that announces 24-bit
    /// colour, for colour schemes whose shades the player prefers.
    Ansi16,
    /// No colour at all: the terminal's own foreground and background,
    /// hierarchy by emphasis only.
    Monochrome,
}

impl ColorChoice {
    /// Returns the colour support to draw with when the terminal was
    /// detected as `detected`: `detected` itself for [`ColorChoice::Auto`],
    /// otherwise the player's explicit choice, which wins over the
    /// detection (and over `NO_COLOR`, as the convention at
    /// <https://no-color.org> lets a program's own settings do).
    pub const fn support(self, detected: ColorSupport) -> ColorSupport {
        match self {
            Self::Auto => detected,
            Self::Ansi16 => ColorSupport::Ansi16,
            Self::Monochrome => ColorSupport::None,
        }
    }
}

/// Makes the theme the only judge of colour for the rest of the process:
/// every colour it resolves reaches the terminal.
///
/// Crossterm, which writes to the terminal, reads `NO_COLOR` on its own
/// and then drops every colour sequence it is asked to write. That would
/// silently undo an explicit [`ColorChoice::Ansi16`] made while `NO_COLOR`
/// is set, which [`ColorChoice::support`] lets win. `NO_COLOR` is already
/// honoured where it belongs: [`ColorSupport::detect`] turns it into
/// [`ColorSupport::None`], so [`ColorChoice::Auto`] draws in monochrome,
/// whose theme sets no colour but the terminal's own.
///
/// Called once by `main`, before the first frame is drawn. Changes a
/// process-wide setting of crossterm (`crossterm::style::force_color_output`).
pub fn let_the_theme_decide_colours() {
    ratatui::crossterm::style::force_color_output(true);
}

/// The display options the player chooses on the options screen and keeps
/// between runs (`persist::OptionsFile`). The default is Unicode glyphs,
/// automatic colours and the dark theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Options {
    /// Characters to draw with.
    pub glyphs: GlyphSet,
    /// Colours to draw with.
    pub colors: ColorChoice,
    /// Dark or light theme.
    pub appearance: Appearance,
}

/// Everything that decides the theme of a session: the player's
/// [`Options`], the colour support detected at start-up, and the `--ascii`
/// flag of the command line, which wins over the glyph option for the
/// session without changing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    /// The player's options, as saved.
    pub options: Options,
    /// What the terminal supports, as detected at start-up.
    pub detected: ColorSupport,
    /// Whether `--ascii` was given: ASCII glyphs whatever the option says.
    pub forced_ascii: bool,
}

impl Settings {
    /// Returns the glyph set to draw with: ASCII when `--ascii` was given,
    /// the option's otherwise.
    pub const fn glyph_set(&self) -> GlyphSet {
        if self.forced_ascii {
            GlyphSet::Ascii
        } else {
            self.options.glyphs
        }
    }

    /// Returns the rendering mode: the colour option applied to the
    /// detected support, then the theme option (see
    /// [`ColorMode::for_terminal`]).
    pub const fn color_mode(&self) -> ColorMode {
        let support = self.options.colors.support(self.detected);
        ColorMode::for_terminal(support, self.options.appearance)
    }

    /// Builds the theme these settings call for.
    pub fn theme(&self) -> Theme {
        Theme::new(self.color_mode(), self.glyph_set())
    }
}

/// One of the four rendering modes (see the visual identity in
/// ARCHITECTURE.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    /// 24-bit colour on the dark ink background.
    TrueColorDark,
    /// 24-bit colour on the light parchment background.
    TrueColorLight,
    /// The 16 ANSI colours on the terminal's black.
    Ansi16,
    /// The terminal's own colours; hierarchy by bold, reversed and dim.
    Monochrome,
}

impl ColorMode {
    /// Picks the rendering mode for a terminal with `support` and the
    /// player's `appearance`.
    ///
    /// The light theme falls back to [`ColorMode::Monochrome`] with only 16
    /// colours, because the ANSI colours of light terminal schemes fail the
    /// contrast minimum on their own background (One Half Light, shipped
    /// with Windows Terminal: yellow 3.09, cyan 3.30, red 3.51). Without
    /// colour support the mode is always monochrome.
    pub const fn for_terminal(support: ColorSupport, appearance: Appearance) -> Self {
        match (support, appearance) {
            (ColorSupport::TrueColor, Appearance::Dark) => Self::TrueColorDark,
            (ColorSupport::TrueColor, Appearance::Light) => Self::TrueColorLight,
            (ColorSupport::Ansi16, Appearance::Dark) => Self::Ansi16,
            (ColorSupport::Ansi16, Appearance::Light) | (ColorSupport::None, _) => Self::Monochrome,
        }
    }
}

/// The semantic colour tokens, resolved for one rendering mode, and the
/// glyphs to draw with.
///
/// Each token field is named after its token and holds the full style to
/// apply: foreground colour and emphasis (bold, dim, reversed). The three
/// surface tokens (`bg`, `bg_panel`, `bg_selection`) set the background
/// instead. Some tokens share a value in a mode but stay separate, so they
/// can diverge without touching the screens.
#[derive(Debug, Clone)]
pub struct Theme {
    /// Background of the whole screen, painted on every cell.
    pub bg: Style,
    /// Background of overlays: sigil tracing, inventory, help.
    pub bg_panel: Style,
    /// Background of the selected menu line, always shown with the `›`
    /// marker as well.
    pub bg_selection: Style,
    /// Running text and menus.
    pub text: Style,
    /// Labels, key help and captions.
    pub text_muted: Style,
    /// Titles, the game's name and the seed.
    pub text_title: Style,
    /// Frames, separators and the empty part of bars.
    pub border: Style,
    /// Floor in view.
    pub floor: Style,
    /// Wall in view.
    pub wall_seen: Style,
    /// Explored wall out of view.
    pub wall_remembered: Style,
    /// Arch or passage.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "part of the token set; the generator makes no arches yet"
        )
    )]
    pub door: Style,
    /// Stairs down in view.
    pub stairs: Style,
    /// Explored stairs down out of view: dimmer than [`Theme::stairs`] and
    /// without its emphasis, yet still at text contrast, since the glyph
    /// marks the way down.
    pub stairs_remembered: Style,
    /// The seal of the bottom floor in view: the gold of what matters,
    /// emphasised like the stairs it replaces.
    pub seal: Style,
    /// Explored seal out of view: dimmer than [`Theme::seal`] and without
    /// its emphasis, yet still at text contrast, since the glyph marks the
    /// end of the run.
    pub seal_remembered: Style,
    /// Engraved glyph not read yet.
    pub engraving_unread: Style,
    /// Engraved glyph already deciphered.
    pub engraving_read: Style,
    /// Explored glyph not read yet, out of view: dimmer than
    /// [`Theme::engraving_unread`], yet still at text contrast, since it
    /// marks a sigil left to learn.
    pub engraving_unread_remembered: Style,
    /// Explored glyph already deciphered, out of view: dimmer than
    /// [`Theme::engraving_read`], an ambient landmark only.
    pub engraving_read_remembered: Style,
    /// The player.
    pub player: Style,
    /// Creatures of flesh (vermin, acolytes, homunculi).
    pub monster_hostile: Style,
    /// Creatures of spirit (eyes, spectres).
    pub monster_arcane: Style,
    /// Items on the ground and in the bag.
    pub item: Style,
    /// Free node of the sigil grid.
    pub sigil_node: Style,
    /// Stroke being traced or traced cleanly, and the nodes it links.
    pub sigil_stroke: Style,
    /// The pen: last node linked.
    pub sigil_cursor: Style,
    /// Unstable trace.
    pub sigil_unstable: Style,
    /// Failed trace.
    pub sigil_failed: Style,
    /// Health above half.
    pub hp_high: Style,
    /// Health from a quarter to half.
    pub hp_mid: Style,
    /// Health below a quarter, always followed by `!`.
    pub hp_low: Style,
    /// Neutral log line.
    pub log_info: Style,
    /// Gain or discovery in the log (prefix `+`).
    pub log_success: Style,
    /// Damage taken or threat in the log (prefix `!`).
    pub log_danger: Style,
    /// Spell cast in the log (prefix `~`).
    pub log_sigil: Style,
    /// Characters to draw the map and the interface with.
    pub glyphs: Glyphs,
}

impl Theme {
    /// Builds the theme for the given rendering `mode`, drawn with the
    /// given `glyph_set`.
    pub fn new(mode: ColorMode, glyph_set: GlyphSet) -> Self {
        let glyphs = Glyphs::new(glyph_set);
        match mode {
            ColorMode::TrueColorDark => Self::true_color_dark(glyphs),
            ColorMode::TrueColorLight => Self::true_color_light(glyphs),
            ColorMode::Ansi16 => Self::ansi16(glyphs),
            ColorMode::Monochrome => Self::monochrome(glyphs),
        }
    }

    /// Returns the style a creature of `family` is drawn with on the map:
    /// [`Theme::monster_hostile`] for the creatures of flesh (vermin,
    /// acolytes, homunculi), [`Theme::monster_arcane`] for the creatures of
    /// spirit (eyes, spectres).
    ///
    /// Common and elite forms share their family's style: the case of the
    /// letter tells them apart. In 16 colours both tokens are the same red
    /// and in monochrome they have no colour at all, so the family is read
    /// from the letter there.
    pub const fn creature(&self, family: Family) -> Style {
        match family {
            Family::Vermin | Family::Acolyte | Family::Homunculus => self.monster_hostile,
            Family::Eye | Family::Spectre => self.monster_arcane,
        }
    }

    /// The dark 24-bit theme: ink background, bone text.
    fn true_color_dark(glyphs: Glyphs) -> Self {
        Self {
            bg: on(0x0F0D15),
            bg_panel: on(0x1B1824),
            bg_selection: on(0x2E2742),
            text: fg(0xE6DECB),
            text_muted: fg(0xA49CB2),
            text_title: fg(0xE3B24F).add_modifier(Modifier::BOLD),
            border: fg(0x6E6684),
            floor: fg(0x6A6379),
            wall_seen: fg(0xB9AF9C),
            wall_remembered: fg(0x6E6878),
            door: fg(0xC4915A),
            stairs: fg(0xF2EADB).add_modifier(Modifier::BOLD),
            stairs_remembered: fg(0x968F84),
            seal: fg(0xE3B24F).add_modifier(Modifier::BOLD),
            seal_remembered: fg(0xA8873F),
            engraving_unread: fg(0x5ED6B8),
            engraving_read: fg(0x4F8F82),
            engraving_unread_remembered: fg(0x3A9682),
            engraving_read_remembered: fg(0x417167),
            player: fg(0xF5C451).add_modifier(Modifier::BOLD),
            monster_hostile: fg(0xEC6A50),
            monster_arcane: fg(0xB794F0),
            item: fg(0x8DBDE6),
            sigil_node: fg(0x6E6684),
            sigil_stroke: fg(0x5ED6B8),
            sigil_cursor: fg(0xF5C451).add_modifier(Modifier::BOLD),
            sigil_unstable: fg(0xF0A53C),
            sigil_failed: fg(0xEC6A50),
            hp_high: fg(0x76C891),
            hp_mid: fg(0xE3B24F),
            hp_low: fg(0xEC6A50),
            log_info: fg(0xA49CB2),
            log_success: fg(0x76C891),
            log_danger: fg(0xF07D63),
            log_sigil: fg(0x5ED6B8),
            glyphs,
        }
    }

    /// The light 24-bit "parchment" theme.
    fn true_color_light(glyphs: Glyphs) -> Self {
        Self {
            bg: on(0xF2EBDC),
            bg_panel: on(0xE7DECB),
            bg_selection: on(0xD6CAB0),
            text: fg(0x2A2230),
            text_muted: fg(0x5C5266),
            text_title: fg(0x7A4F08).add_modifier(Modifier::BOLD),
            border: fg(0x857868),
            floor: fg(0x8A7E70),
            wall_seen: fg(0x4A4038),
            wall_remembered: fg(0x85796C),
            door: fg(0x8A5428),
            stairs: fg(0x1E1824).add_modifier(Modifier::BOLD),
            stairs_remembered: fg(0x6B6258),
            seal: fg(0x7A4F08).add_modifier(Modifier::BOLD),
            seal_remembered: fg(0x80602A),
            engraving_unread: fg(0x0B6E58),
            engraving_read: fg(0x4F7D73),
            engraving_unread_remembered: fg(0x3A6E63),
            engraving_read_remembered: fg(0x6F8C85),
            player: fg(0x7A4E00).add_modifier(Modifier::BOLD),
            monster_hostile: fg(0xA82C1A),
            monster_arcane: fg(0x6A3FB0),
            item: fg(0x215685),
            sigil_node: fg(0x857868),
            sigil_stroke: fg(0x0B6E58),
            sigil_cursor: fg(0x7A4E00).add_modifier(Modifier::BOLD),
            sigil_unstable: fg(0x8C4A00),
            sigil_failed: fg(0xA82C1A),
            hp_high: fg(0x24693A),
            hp_mid: fg(0x7A4F08),
            hp_low: fg(0xA82C1A),
            log_info: fg(0x5C5266),
            log_success: fg(0x24693A),
            log_danger: fg(0xA82C1A),
            log_sigil: fg(0x0B6E58),
            glyphs,
        }
    }

    /// The 16-colour theme on black, with the named ANSI colours.
    ///
    /// The selection has no colour of its own here: it is reversed instead,
    /// which reads well whatever the terminal's colour scheme.
    fn ansi16(glyphs: Glyphs) -> Self {
        let named = |color| Style::new().fg(color);
        Self {
            bg: Style::new().bg(Color::Black),
            bg_panel: Style::new().bg(Color::Black),
            bg_selection: Style::new().add_modifier(Modifier::REVERSED),
            text: named(Color::White),
            text_muted: named(Color::Gray),
            text_title: named(Color::Yellow).add_modifier(Modifier::BOLD),
            border: named(Color::DarkGray),
            floor: named(Color::DarkGray),
            wall_seen: named(Color::Gray),
            wall_remembered: named(Color::DarkGray),
            door: named(Color::Yellow),
            stairs: named(Color::LightYellow).add_modifier(Modifier::BOLD),
            stairs_remembered: named(Color::Gray),
            seal: named(Color::Yellow).add_modifier(Modifier::BOLD),
            seal_remembered: named(Color::Gray),
            engraving_unread: named(Color::LightCyan),
            engraving_read: named(Color::Cyan),
            engraving_unread_remembered: named(Color::Cyan),
            engraving_read_remembered: named(Color::DarkGray),
            player: named(Color::LightYellow).add_modifier(Modifier::BOLD),
            monster_hostile: named(Color::LightRed),
            monster_arcane: named(Color::LightRed),
            item: named(Color::LightBlue),
            sigil_node: named(Color::DarkGray),
            sigil_stroke: named(Color::LightCyan),
            sigil_cursor: named(Color::LightYellow).add_modifier(Modifier::BOLD),
            sigil_unstable: named(Color::Yellow),
            sigil_failed: named(Color::LightRed),
            hp_high: named(Color::LightGreen),
            hp_mid: named(Color::Yellow),
            hp_low: named(Color::LightRed),
            log_info: named(Color::Gray),
            log_success: named(Color::LightGreen),
            log_danger: named(Color::LightRed),
            log_sigil: named(Color::LightCyan),
            glyphs,
        }
    }

    /// The colourless theme: the terminal's own foreground and background,
    /// with hierarchy carried by emphasis only.
    ///
    /// Bold marks what every mode emphasises (player, stairs and seal in
    /// view, titles, the sigil pen); dim fades the floor and remembered
    /// walls, never text; the selection is reversed. Remembered stairs and
    /// seal lose the bold they have in view but are not dimmed: they still
    /// mark the way down and the end of the run; likewise a remembered
    /// unread glyph, a sigil left to learn, is not dimmed, while a
    /// remembered read glyph is. Every piece of information is also carried
    /// by a glyph, a word or a prefix, so nothing is lost.
    fn monochrome(glyphs: Glyphs) -> Self {
        let plain = Style::new();
        let bold = Style::new().add_modifier(Modifier::BOLD);
        let dim = Style::new().add_modifier(Modifier::DIM);
        Self {
            bg: Style::new().fg(Color::Reset).bg(Color::Reset),
            bg_panel: Style::new().bg(Color::Reset),
            bg_selection: Style::new().add_modifier(Modifier::REVERSED),
            text: plain,
            text_muted: plain,
            text_title: bold,
            border: plain,
            floor: dim,
            wall_seen: plain,
            wall_remembered: dim,
            door: plain,
            stairs: bold,
            stairs_remembered: plain,
            seal: bold,
            seal_remembered: plain,
            engraving_unread: plain,
            engraving_read: plain,
            engraving_unread_remembered: plain,
            engraving_read_remembered: dim,
            player: bold,
            monster_hostile: plain,
            monster_arcane: plain,
            item: plain,
            sigil_node: plain,
            sigil_stroke: plain,
            sigil_cursor: bold,
            sigil_unstable: plain,
            sigil_failed: plain,
            hp_high: plain,
            hp_mid: plain,
            hp_low: plain,
            log_info: plain,
            log_success: plain,
            log_danger: plain,
            log_sigil: plain,
            glyphs,
        }
    }
}

/// A foreground style of the 24-bit colour `rgb`, written `0xRRGGBB`.
const fn fg(rgb: u32) -> Style {
    Style::new().fg(Color::from_u32(rgb))
}

/// A surface style: the 24-bit background `rgb`, written `0xRRGGBB`.
const fn on(rgb: u32) -> Style {
    Style::new().bg(Color::from_u32(rgb))
}

#[cfg(test)]
mod tests;
