//! The little grammar the sentences of the catalogue need to name a
//! creature or an item: articles in English and French, the elision and
//! the agreement of French.
//!
//! Exposes [`Who`], something a sentence names, with its definite and
//! indefinite forms in each language and the French agreement of a word
//! with it, and the helpers [`capitalized`] and [`of_indefinite`]. The
//! facts of French (gender, elided article) come from the content
//! (`FrenchNoun`); nothing here guesses them from spelling, but the English
//! "a" or "an" does, which is right for every name of the content.

use sigilfall_core::combat::Combatant;
use sigilfall_core::content::{Content, FrenchNoun, Gender, ItemDef, MonsterDef};

use crate::language::Language;

/// Something a sentence names: the player, a creature, an item, or an
/// identifier the content does not define (shown as it is).
#[derive(Debug, Clone, Copy)]
pub(super) enum Who<'name> {
    /// The player, "you".
    Player,
    /// A creature of the content.
    Monster(&'name MonsterDef),
    /// An item of the content.
    Item(&'name ItemDef),
    /// An identifier missing from the content, only possible when a game
    /// was built from other content than the one given.
    Unknown(&'name str),
}

impl<'name> Who<'name> {
    /// The creature `definition` of `content`, or the identifier itself
    /// when `content` does not define it.
    pub(super) fn monster(content: &'name Content, definition: &'name str) -> Self {
        content
            .monster(definition)
            .map_or(Self::Unknown(definition), Self::Monster)
    }

    /// The item `definition` of `content`, or the identifier itself when
    /// `content` does not define it.
    pub(super) fn item(content: &'name Content, definition: &'name str) -> Self {
        content
            .item(definition)
            .map_or(Self::Unknown(definition), Self::Item)
    }

    /// The fighter `combatant`: the player, or its creature in `content`.
    pub(super) fn of(combatant: &'name Combatant, content: &'name Content) -> Self {
        match combatant {
            Combatant::Player => Self::Player,
            Combatant::Monster { definition } => Self::monster(content, definition),
        }
    }

    /// The French name of a creature or an item, `None` for the player and
    /// an unknown identifier.
    fn french(self) -> Option<&'name FrenchNoun> {
        match self {
            Self::Monster(monster) => Some(monster.french_name()),
            Self::Item(item) => Some(item.french_name()),
            Self::Player | Self::Unknown(_) => None,
        }
    }

    /// The English name, without an article.
    fn english(self) -> &'name str {
        match self {
            Self::Player => "you",
            Self::Monster(monster) => monster.name(),
            Self::Item(item) => item.name(),
            Self::Unknown(id) => id,
        }
    }

    /// The name with its definite article, in lower case: "the shade",
    /// "you"; "l'ombre", "le hiérophante", "vous".
    pub(super) fn definite(self, language: Language) -> String {
        match (language, self) {
            (Language::English, Self::Player) => "you".to_owned(),
            (Language::English, _) => format!("the {}", self.english()),
            (Language::French, Self::Player) => "vous".to_owned(),
            (Language::French, Self::Unknown(id)) => format!("le {id}"),
            (Language::French, _) => self.french().map_or_else(String::new, definite),
        }
    }

    /// The name with its indefinite article, in lower case: "an acolyte",
    /// "a shade"; "un acolyte", "une ombre". The player stays "you"
    /// ("vous").
    pub(super) fn indefinite(self, language: Language) -> String {
        match (language, self) {
            (_, Self::Player) => self.definite(language),
            (Language::English, _) => {
                let name = self.english();
                format!("{} {name}", english_article(name))
            }
            (Language::French, Self::Unknown(id)) => format!("un {id}"),
            (Language::French, _) => self.french().map_or_else(String::new, indefinite),
        }
    }

    /// `masculine` or `feminine`, the French word that agrees with this
    /// name: "brûlé" for the rat, "brûlée" for the shade. The player and an
    /// unknown identifier take the masculine, the unmarked form.
    pub(super) fn agreed(self, masculine: &'name str, feminine: &'name str) -> &'name str {
        match self.french() {
            Some(noun) => agreed(noun, masculine, feminine),
            None => masculine,
        }
    }
}

/// "an" before a `name` that starts with a vowel letter, "a" otherwise.
///
/// Spelling decides, not sound: right for every monster and item of the
/// game ("an acolyte", "a hierophant", "an essence phial"), wrong for a name
/// such as "unicorn" or "hour", which the content does not have.
fn english_article(name: &str) -> &'static str {
    let starts_with_vowel = name
        .chars()
        .next()
        .is_some_and(|first| "aeiouAEIOU".contains(first));
    if starts_with_vowel { "an" } else { "a" }
}

/// `noun` with its definite article: "le rat d'ossuaire", "la Fiole
/// d'essence", "l'ombre".
pub(super) fn definite(noun: &FrenchNoun) -> String {
    let article = match (noun.elided(), noun.gender()) {
        (true, _) => "l'",
        (false, Gender::Masculine) => "le ",
        (false, Gender::Feminine) => "la ",
    };
    format!("{article}{}", noun.noun())
}

/// `noun` with its indefinite article: "un rat d'ossuaire", "une ombre".
pub(super) fn indefinite(noun: &FrenchNoun) -> String {
    let article = match noun.gender() {
        Gender::Masculine => "un",
        Gender::Feminine => "une",
    };
    format!("{article} {}", noun.noun())
}

/// `noun` after "de" with its indefinite article: "d'un acolyte", "d'une
/// ombre", as in "victime d'un acolyte".
pub(super) fn of_indefinite(noun: &FrenchNoun) -> String {
    format!("d'{}", indefinite(noun))
}

/// `masculine` or `feminine`, as the gender of `noun` asks.
pub(super) const fn agreed<'word>(
    noun: &FrenchNoun,
    masculine: &'word str,
    feminine: &'word str,
) -> &'word str {
    match noun.gender() {
        Gender::Masculine => masculine,
        Gender::Feminine => feminine,
    }
}

/// `text` with its first letter in upper case, for the start of a
/// sentence: "l'ombre" becomes "L'ombre", "the shade" "The shade".
pub(super) fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The game's own content.
    fn content() -> Content {
        Content::load().expect("the shipped content is valid")
    }

    #[test]
    fn french_articles_follow_the_gender_and_the_elision_of_the_content() {
        let content = content();
        let named = |id: &'static str| Who::monster(&content, id);
        assert_eq!(
            named("ossuary_rat").definite(Language::French),
            "le rat d'ossuaire"
        );
        assert_eq!(named("acolyte").definite(Language::French), "l'acolyte");
        assert_eq!(
            named("hierophant").definite(Language::French),
            "le hiérophante"
        );
        assert_eq!(named("shade").definite(Language::French), "l'ombre");
        assert_eq!(named("shade").indefinite(Language::French), "une ombre");
        assert_eq!(named("acolyte").indefinite(Language::French), "un acolyte");
        assert_eq!(named("shade").agreed("brûlé", "brûlée"), "brûlée");
        assert_eq!(named("ossuary_rat").agreed("brûlé", "brûlée"), "brûlé");
        let phial = Who::item(&content, "essence_phial");
        assert_eq!(phial.definite(Language::French), "la fiole d'essence");
        assert_eq!(phial.indefinite(Language::English), "an essence phial");
        assert_eq!(Who::Player.definite(Language::French), "vous");
    }

    #[test]
    fn english_articles_follow_the_first_letter() {
        let content = content();
        assert_eq!(
            Who::monster(&content, "acolyte").indefinite(Language::English),
            "an acolyte"
        );
        assert_eq!(
            Who::monster(&content, "hierophant").indefinite(Language::English),
            "a hierophant"
        );
        assert_eq!(
            Who::monster(&content, "shade").definite(Language::English),
            "the shade"
        );
    }

    #[test]
    fn a_sentence_starts_with_a_capital_whatever_its_first_letter() {
        assert_eq!(capitalized("l'ombre"), "L'ombre");
        assert_eq!(capitalized("œil"), "Œil");
        assert_eq!(capitalized("élan"), "Élan");
        assert_eq!(capitalized(""), "");
    }
}
