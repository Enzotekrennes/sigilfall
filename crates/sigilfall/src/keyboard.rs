//! The keyboard layout the keys follow: QWERTY or AZERTY.
//!
//! Exposes [`Keyboard`], with [`Keyboard::from_locale`], which guesses the
//! layout from a locale name such as `fr-FR` or `en_US.UTF-8`, and
//! [`Keyboard::detect`], which asks the operating system for its locale.
//! The layout is an option of the options screen, kept in the options file
//! (`persist`); the guess is only its default, used until the player
//! chooses one. `--keyboard` chooses it for one session.
//!
//! The layout itself is never read from the system: Windows and macOS only
//! give it through calls the language cannot check (`unsafe`), and a
//! terminal on Linux, or one reached over SSH, has no reliable way to know
//! it. The locale is read without either, and a French locale of France or
//! Belgium is the case where AZERTY is the layout sold with the computers;
//! French of Canada, Switzerland or Luxembourg comes with QWERTY or QWERTZ.
//!
//! This module only names the layouts: the keys bound on each live in
//! `input`.

use serde::{Deserialize, Serialize};

/// A keyboard layout the keys are chosen for. Written as `Qwerty` or
/// `Azerty` in the options file, and as `qwerty` or `azerty` after
/// `--keyboard`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, clap::ValueEnum,
)]
pub enum Keyboard {
    /// QWERTY, the default: the digits on the top row without Shift, and
    /// WASD under the left hand.
    #[default]
    #[value(name = "qwerty")]
    Qwerty,
    /// AZERTY, the French and Belgian layout: the top row gives
    /// `& é " ' ( - è _ ç à` without Shift and the digits with it, and ZQSD
    /// sit where WASD do on QWERTY.
    #[value(name = "azerty")]
    Azerty,
}

impl Keyboard {
    /// Every layout, in the order the options screen goes through them.
    pub const ALL: [Self; 2] = [Self::Qwerty, Self::Azerty];

    /// Returns the layout most likely used with the locale named `locale`,
    /// as the system gives it (`fr-FR`, `fr_BE.UTF-8`, `fr-Latn-FR`,
    /// `en-US`, `C`…): AZERTY when its language part, the letters before the
    /// first `-` or `_`, is `fr` and its region, the first part of two
    /// letters after it, is `FR` or `BE` (any case), or is missing; QWERTY
    /// otherwise, the empty name included.
    ///
    /// A French locale without a region is taken as France's, by far the
    /// most common case; the encoding (`.UTF-8`) and the modifier (`@euro`)
    /// are ignored.
    pub fn from_locale(locale: &str) -> Self {
        let name = locale.split(['.', '@']).next().unwrap_or_default();
        let mut parts = name.split(['-', '_']);
        let language = parts.next().unwrap_or_default();
        if !language.eq_ignore_ascii_case("fr") {
            return Self::Qwerty;
        }
        let region = parts.find(|part| part.len() == 2 && part.chars().all(char::is_alphabetic));
        match region {
            None => Self::Azerty,
            Some(region)
                if AZERTY_REGIONS
                    .iter()
                    .any(|r| r.eq_ignore_ascii_case(region)) =>
            {
                Self::Azerty
            }
            Some(_) => Self::Qwerty,
        }
    }

    /// Returns the layout most likely used on this system, from its locale
    /// (see [`Keyboard::from_locale`]), or QWERTY when the system gives
    /// none.
    ///
    /// Reads the system's settings (the user's preferred languages on
    /// Windows and macOS, `LC_ALL`, `LC_MESSAGES` and `LANG` on Linux); it
    /// never fails.
    pub fn detect() -> Self {
        sys_locale::get_locale().map_or(Self::Qwerty, |locale| Self::from_locale(&locale))
    }

    /// Returns the layout after this one in [`Keyboard::ALL`], the first
    /// after the last: the next value of the option.
    pub const fn next(self) -> Self {
        Self::ALL[(self.position() + 1) % Self::ALL.len()]
    }

    /// Returns the layout before this one in [`Keyboard::ALL`], the last
    /// before the first: the previous value of the option.
    pub const fn previous(self) -> Self {
        Self::ALL[(self.position() + Self::ALL.len() - 1) % Self::ALL.len()]
    }

    /// Returns the position of this layout in [`Keyboard::ALL`].
    const fn position(self) -> usize {
        match self {
            Self::Qwerty => 0,
            Self::Azerty => 1,
        }
    }

    /// Returns the name of the layout, the same in every language:
    /// "QWERTY", "AZERTY".
    pub const fn name(self) -> &'static str {
        match self {
            Self::Qwerty => "QWERTY",
            Self::Azerty => "AZERTY",
        }
    }
}

/// Regions whose French locale comes with an AZERTY keyboard: France and
/// Belgium.
const AZERTY_REGIONS: [&str; 2] = ["FR", "BE"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn french_of_france_and_belgium_gives_azerty_in_every_usual_form() {
        for locale in [
            "fr",
            "FR",
            "fr-FR",
            "fr_FR",
            "fr_FR.UTF-8",
            "fr-BE",
            "fr_be",
            "fr@euro",
            "fr_FR@euro",
            "fr-Latn-FR",
        ] {
            assert_eq!(Keyboard::from_locale(locale), Keyboard::Azerty, "{locale}");
        }
    }

    #[test]
    fn any_other_locale_gives_qwerty() {
        for locale in [
            "",
            "C",
            "POSIX",
            "en",
            "en-US",
            "en_GB.UTF-8",
            "de-DE",
            "fr-CA",
            "fr_CH",
            "fr-LU",
            "frr",
            "fur",
            "nl-BE",
        ] {
            assert_eq!(Keyboard::from_locale(locale), Keyboard::Qwerty, "{locale}");
        }
    }

    #[test]
    fn the_option_goes_through_every_layout_both_ways() {
        for keyboard in Keyboard::ALL {
            assert_ne!(keyboard.next(), keyboard);
            assert_eq!(keyboard.next().previous(), keyboard);
        }
        assert_eq!(Keyboard::default(), Keyboard::Qwerty);
        assert_eq!(Keyboard::Azerty.name(), "AZERTY");
    }
}
