//! The language the game speaks: English or French.
//!
//! Exposes [`Language`], with [`Language::from_locale`], which reads the
//! language out of a locale name such as `fr-FR` or `en_US.UTF-8`, and
//! [`Language::detect`], which asks the operating system for its locale. The
//! language is an option of the options screen, kept in the options file
//! (`persist`); the system's language is only its default, used until the
//! player chooses one. `--lang` chooses it for one session.
//!
//! This module only names the languages: every text in each of them lives
//! in `messages`.

use serde::{Deserialize, Serialize};

/// A language of the game's text. Written as `English` or `French` in the
/// options file, and as `en` or `fr` after `--lang`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, clap::ValueEnum,
)]
pub enum Language {
    /// English, the default when the system speaks neither language.
    #[default]
    #[value(name = "en")]
    English,
    /// French.
    #[value(name = "fr")]
    French,
}

impl Language {
    /// Every language, in the order the options screen goes through them.
    pub const ALL: [Self; 2] = [Self::English, Self::French];

    /// Returns the language of the locale named `locale`, as the system
    /// gives it (`fr-FR`, `fr_CA.UTF-8`, `en-US`, `C`…): French when its
    /// language part, the letters before the first `-`, `_`, `.` or `@`, is
    /// `fr` in any case; English otherwise, the empty name included.
    ///
    /// Only the language part counts, so `frr` (North Frisian) is not
    /// French.
    pub fn from_locale(locale: &str) -> Self {
        let language = locale
            .split(['-', '_', '.', '@'])
            .next()
            .unwrap_or_default();
        if language.eq_ignore_ascii_case("fr") {
            Self::French
        } else {
            Self::English
        }
    }

    /// Returns the language of the operating system's locale (see
    /// [`Language::from_locale`]), or English when the system gives none.
    ///
    /// Reads the system's settings (the user's preferred languages on
    /// Windows and macOS, `LC_ALL`, `LC_MESSAGES` and `LANG` on Linux); it
    /// never fails.
    pub fn detect() -> Self {
        sys_locale::get_locale().map_or(Self::English, |locale| Self::from_locale(&locale))
    }

    /// Returns the language after this one in [`Language::ALL`], the first
    /// after the last: the next value of the option.
    pub const fn next(self) -> Self {
        Self::ALL[(self.position() + 1) % Self::ALL.len()]
    }

    /// Returns the language before this one in [`Language::ALL`], the last
    /// before the first: the previous value of the option.
    pub const fn previous(self) -> Self {
        Self::ALL[(self.position() + Self::ALL.len() - 1) % Self::ALL.len()]
    }

    /// Returns the position of this language in [`Language::ALL`].
    const fn position(self) -> usize {
        match self {
            Self::English => 0,
            Self::French => 1,
        }
    }

    /// Returns the name of the language in itself, as the options screen
    /// lists it whatever the language in use: "English", "Français".
    pub const fn endonym(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::French => "Français",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_french_locale_gives_french_in_every_usual_form() {
        for locale in [
            "fr",
            "FR",
            "fr-FR",
            "fr_FR",
            "fr_CA.UTF-8",
            "fr-BE",
            "fr@euro",
            "Fr-ch",
        ] {
            assert_eq!(Language::from_locale(locale), Language::French, "{locale}");
        }
    }

    #[test]
    fn any_other_locale_gives_english() {
        for locale in [
            "",
            "C",
            "POSIX",
            "en",
            "en-US",
            "en_GB.UTF-8",
            "de-DE",
            "frr",
            "fur",
            "f",
        ] {
            assert_eq!(Language::from_locale(locale), Language::English, "{locale}");
        }
    }

    #[test]
    fn the_option_goes_through_every_language_both_ways() {
        for language in Language::ALL {
            assert_ne!(language.next(), language);
            assert_eq!(language.next().previous(), language);
        }
        assert_eq!(Language::default(), Language::English);
    }
}
