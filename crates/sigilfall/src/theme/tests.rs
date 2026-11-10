//! Tests of the theme: colour detection, mode selection, and the contrast
//! guarantees of the visual identity (ARCHITECTURE.md) recomputed from the
//! token values.

use std::collections::HashMap;

use super::*;

/// WCAG 2.x minimum contrast for text and meaningful glyphs.
const TEXT_CONTRAST: f64 = 4.5;

/// WCAG 2.x minimum contrast for ambient graphics (floor, borders,
/// remembered walls…), which carry no information of their own.
const GRAPHIC_CONTRAST: f64 = 3.0;

/// Tokens held to [`GRAPHIC_CONTRAST`] only: the ambient graphics.
const AMBIENT_TOKENS: [&str; 6] = [
    "border",
    "floor",
    "wall_remembered",
    "engraving_read",
    "engraving_read_remembered",
    "sigil_node",
];

/// Tokens also drawn on `bg_panel`, the surface of the overlays, and held
/// to their minimum there too.
const PANEL_TOKENS: [&str; 10] = [
    "text",
    "text_muted",
    "text_title",
    "border",
    "sigil_node",
    "sigil_stroke",
    "sigil_cursor",
    "sigil_unstable",
    "sigil_failed",
    "item",
];

/// Tokens drawn on the selected line of a list (the inventory, the title
/// and options menus, the seed field), over `bg_selection`: its text, its
/// caption and the glyph of its item.
const SELECTION_TOKENS: [&str; 3] = ["text", "text_muted", "item"];

/// Every foreground token of `theme`, with its name.
fn foreground_tokens(theme: &Theme) -> [(&'static str, Style); 32] {
    [
        ("text", theme.text),
        ("text_muted", theme.text_muted),
        ("text_title", theme.text_title),
        ("border", theme.border),
        ("floor", theme.floor),
        ("wall_seen", theme.wall_seen),
        ("wall_remembered", theme.wall_remembered),
        ("door", theme.door),
        ("stairs", theme.stairs),
        ("stairs_remembered", theme.stairs_remembered),
        ("seal", theme.seal),
        ("seal_remembered", theme.seal_remembered),
        ("engraving_unread", theme.engraving_unread),
        ("engraving_read", theme.engraving_read),
        (
            "engraving_unread_remembered",
            theme.engraving_unread_remembered,
        ),
        ("engraving_read_remembered", theme.engraving_read_remembered),
        ("player", theme.player),
        ("monster_hostile", theme.monster_hostile),
        ("monster_arcane", theme.monster_arcane),
        ("item", theme.item),
        ("sigil_node", theme.sigil_node),
        ("sigil_stroke", theme.sigil_stroke),
        ("sigil_cursor", theme.sigil_cursor),
        ("sigil_unstable", theme.sigil_unstable),
        ("sigil_failed", theme.sigil_failed),
        ("hp_high", theme.hp_high),
        ("hp_mid", theme.hp_mid),
        ("hp_low", theme.hp_low),
        ("log_info", theme.log_info),
        ("log_success", theme.log_success),
        ("log_danger", theme.log_danger),
        ("log_sigil", theme.log_sigil),
    ]
}

/// The theme of `mode` with Unicode glyphs.
fn theme(mode: ColorMode) -> Theme {
    Theme::new(mode, GlyphSet::Unicode)
}

/// RGB value of `color`: as is for 24-bit colours, and from Campbell, the
/// default scheme of Windows Terminal, for the 16 named colours (the scheme
/// the 16-colour theme is measured on).
fn rgb(color: Color) -> (u8, u8, u8) {
    match color {
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Black => (0x0C, 0x0C, 0x0C),
        Color::Red => (0xC5, 0x0F, 0x1F),
        Color::Green => (0x13, 0xA1, 0x0E),
        Color::Yellow => (0xC1, 0x9C, 0x00),
        Color::Blue => (0x00, 0x37, 0xDA),
        Color::Magenta => (0x88, 0x17, 0x98),
        Color::Cyan => (0x3A, 0x96, 0xDD),
        Color::Gray => (0xCC, 0xCC, 0xCC),
        Color::DarkGray => (0x76, 0x76, 0x76),
        Color::LightRed => (0xE7, 0x48, 0x56),
        Color::LightGreen => (0x16, 0xC6, 0x0C),
        Color::LightYellow => (0xF9, 0xF1, 0xA5),
        Color::LightBlue => (0x3B, 0x78, 0xFF),
        Color::LightMagenta => (0xB4, 0x00, 0x9E),
        Color::LightCyan => (0x61, 0xD6, 0xD6),
        Color::White => (0xF2, 0xF2, 0xF2),
        other => panic!("no RGB value for {other:?}"),
    }
}

/// WCAG 2.x relative luminance of `color`.
fn luminance(color: Color) -> f64 {
    let linear = |channel: u8| {
        let value = f64::from(channel) / 255.0;
        if value <= 0.040_45 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    let (r, g, b) = rgb(color);
    0.0722f64.mul_add(linear(b), 0.2126f64.mul_add(linear(r), 0.7152 * linear(g)))
}

/// WCAG 2.x contrast ratio between `foreground` and `background`, from 1
/// to 21.
fn contrast(foreground: Color, background: Color) -> f64 {
    let (a, b) = (luminance(foreground), luminance(background));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// Checks that every foreground token of `theme` reaches its contrast
/// minimum on `bg`, and the panel tokens on `bg_panel` as well.
fn assert_contrasts(theme: &Theme) {
    let bg = theme.bg.bg.expect("bg sets a background");
    let panel = theme.bg_panel.bg.expect("bg_panel sets a background");
    for (name, style) in foreground_tokens(theme) {
        let color = style.fg.unwrap_or_else(|| panic!("{name} has no colour"));
        let minimum = if AMBIENT_TOKENS.contains(&name) {
            GRAPHIC_CONTRAST
        } else {
            TEXT_CONTRAST
        };
        let on_bg = contrast(color, bg);
        assert!(on_bg >= minimum, "{name} on bg: {on_bg:.2} < {minimum}");
        if PANEL_TOKENS.contains(&name) {
            let on_panel = contrast(color, panel);
            assert!(
                on_panel >= minimum,
                "{name} on bg_panel: {on_panel:.2} < {minimum}"
            );
        }
    }
}

/// An environment made of the given variables only.
fn env_of(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
    let vars: HashMap<String, OsString> = vars
        .iter()
        .map(|&(name, value)| (name.to_owned(), OsString::from(value)))
        .collect();
    move |name| vars.get(name).cloned()
}

#[test]
fn windows_terminal_and_colorterm_mean_true_color() {
    for vars in [
        [("WT_SESSION", "0f6c1a2e-0000-0000-0000-000000000000")],
        [("COLORTERM", "truecolor")],
        [("COLORTERM", "24bit")],
        [("COLORTERM", "TrueColor")],
    ] {
        assert_eq!(
            ColorSupport::from_env(env_of(&vars)),
            ColorSupport::TrueColor,
            "{vars:?}"
        );
    }
}

#[test]
fn other_terminals_get_16_colours() {
    for vars in [
        &[][..],
        &[("COLORTERM", "yes")],
        &[("TERM", "xterm-256color")],
    ] {
        assert_eq!(
            ColorSupport::from_env(env_of(vars)),
            ColorSupport::Ansi16,
            "{vars:?}"
        );
    }
}

#[test]
fn no_color_wins_over_everything_unless_empty() {
    let all = env_of(&[
        ("NO_COLOR", "1"),
        ("COLORTERM", "truecolor"),
        ("WT_SESSION", "x"),
    ]);
    assert_eq!(ColorSupport::from_env(all), ColorSupport::None);

    let empty = env_of(&[("NO_COLOR", ""), ("WT_SESSION", "x")]);
    assert_eq!(ColorSupport::from_env(empty), ColorSupport::TrueColor);
}

#[test]
fn mode_follows_support_and_appearance() {
    let cases = [
        (
            ColorSupport::TrueColor,
            Appearance::Dark,
            ColorMode::TrueColorDark,
        ),
        (
            ColorSupport::TrueColor,
            Appearance::Light,
            ColorMode::TrueColorLight,
        ),
        (ColorSupport::Ansi16, Appearance::Dark, ColorMode::Ansi16),
        (
            ColorSupport::Ansi16,
            Appearance::Light,
            ColorMode::Monochrome,
        ),
        (ColorSupport::None, Appearance::Dark, ColorMode::Monochrome),
        (ColorSupport::None, Appearance::Light, ColorMode::Monochrome),
    ];
    for (support, appearance, expected) in cases {
        assert_eq!(
            ColorMode::for_terminal(support, appearance),
            expected,
            "{support:?}, {appearance:?}"
        );
    }
}

#[test]
fn contrast_formula_matches_the_figures_of_the_design() {
    // The design quotes each ratio on the least favourable surface: bg_panel
    // for text (a panel token), bg for the player.
    let ratio = |token: Style, surface: Style| {
        let ratio = contrast(token.fg.unwrap(), surface.bg.unwrap());
        (ratio * 100.0).round() / 100.0
    };
    let dark = theme(ColorMode::TrueColorDark);
    let light = theme(ColorMode::TrueColorLight);
    let ansi = theme(ColorMode::Ansi16);

    assert_eq!(ratio(dark.text, dark.bg_panel), 13.03);
    assert_eq!(ratio(dark.player, dark.bg), 11.85);
    assert_eq!(ratio(light.text, light.bg_panel), 11.47);
    assert_eq!(ratio(light.player, light.bg), 6.06);
    assert_eq!(ratio(ansi.text, ansi.bg), 17.47);
    assert_eq!(ratio(ansi.item, ansi.bg), 4.95);
    // The seal, on bg: the map is never drawn on a panel.
    assert_eq!(ratio(dark.seal, dark.bg), 9.87);
    assert_eq!(ratio(dark.seal_remembered, dark.bg), 5.70);
    assert_eq!(ratio(light.seal, light.bg), 6.0);
    assert_eq!(ratio(light.seal_remembered, light.bg), 4.88);
    assert_eq!(ratio(ansi.seal, ansi.bg), 7.47);
    assert_eq!(ratio(ansi.seal_remembered, ansi.bg), 12.18);
}

/// Checks that every token of [`SELECTION_TOKENS`] reaches the text
/// minimum on `bg_selection`.
fn assert_selection_contrasts(theme: &Theme) {
    let selection = theme
        .bg_selection
        .bg
        .expect("bg_selection sets a background");
    for (name, style) in foreground_tokens(theme) {
        if SELECTION_TOKENS.contains(&name) {
            let ratio = contrast(style.fg.unwrap(), selection);
            assert!(ratio >= TEXT_CONTRAST, "{name} on bg_selection: {ratio:.2}");
        }
    }
}

#[test]
fn dark_true_colour_tokens_reach_aa_contrast() {
    let theme = theme(ColorMode::TrueColorDark);
    assert_contrasts(&theme);
    assert_selection_contrasts(&theme);
}

#[test]
fn light_true_colour_tokens_reach_aa_contrast() {
    let theme = theme(ColorMode::TrueColorLight);
    assert_contrasts(&theme);
    assert_selection_contrasts(&theme);
}

#[test]
fn the_selection_figures_of_the_design_hold() {
    // The lowest ratios of the selected line, in the light theme, as the
    // values of the theme were chosen to reach them.
    let light = theme(ColorMode::TrueColorLight);
    let on_selection = |token: Style| {
        let ratio = contrast(token.fg.unwrap(), light.bg_selection.bg.unwrap());
        (ratio * 100.0).round() / 100.0
    };

    assert_eq!(on_selection(light.item), 4.73);
    assert_eq!(on_selection(light.text_muted), 4.53);
}

#[test]
fn ansi16_tokens_reach_aa_contrast_on_campbell() {
    let theme = theme(ColorMode::Ansi16);
    assert_contrasts(&theme);
    assert!(theme.bg_selection.add_modifier.contains(Modifier::REVERSED));
}

#[test]
fn monochrome_uses_the_terminal_colours_only() {
    let theme = theme(ColorMode::Monochrome);
    let surfaces = [theme.bg, theme.bg_panel, theme.bg_selection];
    let tokens = foreground_tokens(&theme).map(|(_, style)| style);
    for style in surfaces.iter().chain(&tokens) {
        for color in [style.fg, style.bg].into_iter().flatten() {
            assert_eq!(color, Color::Reset, "{style:?}");
        }
    }
    assert!(theme.bg_selection.add_modifier.contains(Modifier::REVERSED));
    assert!(theme.floor.add_modifier.contains(Modifier::DIM));
    assert!(theme.wall_remembered.add_modifier.contains(Modifier::DIM));
    assert!(!theme.wall_seen.add_modifier.contains(Modifier::DIM));
    assert!(!theme.stairs_remembered.add_modifier.contains(Modifier::DIM));
    assert!(!theme.seal_remembered.add_modifier.contains(Modifier::DIM));
    assert!(
        !theme
            .engraving_unread_remembered
            .add_modifier
            .contains(Modifier::DIM)
    );
    assert!(
        theme
            .engraving_read_remembered
            .add_modifier
            .contains(Modifier::DIM)
    );
}

#[test]
fn emphasised_tokens_are_bold_and_text_is_never_dim_in_every_mode() {
    let modes = [
        ColorMode::TrueColorDark,
        ColorMode::TrueColorLight,
        ColorMode::Ansi16,
        ColorMode::Monochrome,
    ];
    for mode in modes {
        let theme = theme(mode);
        for (name, style) in [
            ("player", theme.player),
            ("stairs", theme.stairs),
            ("seal", theme.seal),
            ("text_title", theme.text_title),
            ("sigil_cursor", theme.sigil_cursor),
        ] {
            assert!(
                style.add_modifier.contains(Modifier::BOLD),
                "{mode:?}: {name}"
            );
        }
        for (name, style) in foreground_tokens(&theme) {
            if name.starts_with("text") || name.starts_with("log") {
                assert!(
                    !style.add_modifier.contains(Modifier::DIM),
                    "{mode:?}: {name}"
                );
            }
        }
    }
}

#[test]
fn every_mode_paints_a_background() {
    for (mode, expected) in [
        (ColorMode::TrueColorDark, Color::Rgb(0x0F, 0x0D, 0x15)),
        (ColorMode::TrueColorLight, Color::Rgb(0xF2, 0xEB, 0xDC)),
        (ColorMode::Ansi16, Color::Black),
        (ColorMode::Monochrome, Color::Reset),
    ] {
        assert_eq!(theme(mode).bg.bg, Some(expected), "{mode:?}");
    }
}

/// Settings for `options` on a terminal `detected` as such, with
/// `--ascii` if `forced_ascii`.
const fn settings(options: Options, detected: ColorSupport, forced_ascii: bool) -> Settings {
    Settings {
        options,
        detected,
        forced_ascii,
        forced_language: None,
    }
}

#[test]
fn auto_follows_the_terminal_and_the_other_choices_win_over_it() {
    for detected in [
        ColorSupport::TrueColor,
        ColorSupport::Ansi16,
        ColorSupport::None,
    ] {
        assert_eq!(ColorChoice::Auto.support(detected), detected);
        assert_eq!(ColorChoice::Ansi16.support(detected), ColorSupport::Ansi16);
        assert_eq!(
            ColorChoice::Monochrome.support(detected),
            ColorSupport::None
        );
    }
}

#[test]
fn the_settings_pick_the_mode_of_the_options_and_the_terminal() {
    let light = Options {
        appearance: Appearance::Light,
        ..Options::default()
    };
    let ansi16 = Options {
        colors: ColorChoice::Ansi16,
        ..Options::default()
    };
    let cases = [
        (
            Options::default(),
            ColorSupport::TrueColor,
            ColorMode::TrueColorDark,
        ),
        (light, ColorSupport::TrueColor, ColorMode::TrueColorLight),
        (Options::default(), ColorSupport::Ansi16, ColorMode::Ansi16),
        (ansi16, ColorSupport::TrueColor, ColorMode::Ansi16),
        // The light theme has no 16-colour form: monochrome instead.
        (
            Options {
                appearance: Appearance::Light,
                ..ansi16
            },
            ColorSupport::TrueColor,
            ColorMode::Monochrome,
        ),
        (
            Options::default(),
            ColorSupport::None,
            ColorMode::Monochrome,
        ),
        // An explicit choice wins over NO_COLOR.
        (ansi16, ColorSupport::None, ColorMode::Ansi16),
    ];
    for (options, detected, mode) in cases {
        let settings = settings(options, detected, false);
        assert_eq!(settings.color_mode(), mode, "{options:?} on {detected:?}");
        assert_eq!(settings.theme().bg, Theme::new(mode, GlyphSet::Unicode).bg);
    }
}

#[test]
fn an_explicit_colour_choice_reaches_the_terminal_under_no_color_and_auto_does_not() {
    use ratatui::backend::IntoCrossterm as _;
    use ratatui::crossterm::Command as _;
    use ratatui::crossterm::style::{SetForegroundColor, force_color_output};

    /// The escape sequence crossterm writes for the foreground of `style`.
    fn written(style: Style) -> String {
        let colour = style.fg.unwrap_or(Color::Reset).into_crossterm();
        let mut sequence = String::new();
        SetForegroundColor(colour)
            .write_ansi(&mut sequence)
            .unwrap();
        sequence
    }

    let no_color = ColorSupport::from_env(env_of(&[("NO_COLOR", "1")]));
    let ansi16 = Options {
        colors: ColorChoice::Ansi16,
        ..Options::default()
    };
    let explicit = settings(ansi16, no_color, false).theme();

    // The state crossterm puts itself in when it finds NO_COLOR set, reached
    // without changing the environment the other tests read: every colour
    // is dropped.
    force_color_output(false);
    assert_eq!(written(explicit.player), "\x1b[m", "crossterm drops it");

    let_the_theme_decide_colours();

    let sequence = written(explicit.player);
    assert!(sequence.starts_with("\x1b[38;5;"), "{sequence:?}");
    // Auto still honours NO_COLOR: its theme sets no colour of its own.
    let auto = settings(Options::default(), no_color, false).theme();
    for style in [
        auto.bg,
        auto.text,
        auto.player,
        auto.monster_hostile,
        auto.hp_low,
    ] {
        assert!(matches!(style.fg, None | Some(Color::Reset)), "{style:?}");
        assert!(matches!(style.bg, None | Some(Color::Reset)), "{style:?}");
    }
}

#[test]
fn ascii_on_the_command_line_wins_over_the_glyph_option() {
    let unicode = Options::default();
    let ascii = Options {
        glyphs: GlyphSet::Ascii,
        ..Options::default()
    };

    assert_eq!(
        settings(unicode, ColorSupport::TrueColor, false).glyph_set(),
        GlyphSet::Unicode
    );
    assert_eq!(
        settings(ascii, ColorSupport::TrueColor, false).glyph_set(),
        GlyphSet::Ascii
    );
    assert_eq!(
        settings(unicode, ColorSupport::TrueColor, true).glyph_set(),
        GlyphSet::Ascii
    );
    assert_eq!(
        settings(unicode, ColorSupport::TrueColor, true)
            .theme()
            .glyphs,
        Glyphs::new(GlyphSet::Ascii)
    );
}

#[test]
fn the_default_options_are_unicode_auto_and_dark() {
    assert_eq!(
        Options::default(),
        Options {
            glyphs: GlyphSet::Unicode,
            colors: ColorChoice::Auto,
            appearance: Appearance::Dark,
            language: Language::English,
        }
    );
}
