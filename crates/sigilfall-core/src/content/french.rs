//! The French names of the content: the grammar a French sentence needs to
//! name a creature or an item.
//!
//! Exposes [`Gender`], [`FrenchNoun`], the validated French name of a
//! monster or an item, and to the content readers [`FrenchNounEntry`], that
//! name as written in a content file, with [`parse_noun`], which checks it.
//! A sigil's French name is a proper noun used without an article, a plain
//! text checked like the English name.
//!
//! A French sentence agrees with the noun it names ("l'ombre est brûlée",
//! "le rat est brûlé") and puts the right article before it ("le", "la",
//! "l'", "un", "une"). The article cannot be guessed from the spelling: "l'homoncule"
//! but "le hiérophante", whose h forbids the elision. So a content file
//! writes the French name with its definite article and its gender, such as
//! `Masculine("le hiérophante")`, and the binary builds every other form
//! from them. The core never writes a sentence: it only keeps these facts.

use serde::Deserialize;

/// The grammatical gender of a French noun.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
pub enum Gender {
    /// "le", "un", past participles without an added `e`.
    Masculine,
    /// "la", "une", past participles with an added `e`.
    Feminine,
}

/// The French name of a monster or an item as written in a content file:
/// its gender, and the noun with its definite article, such as
/// `Feminine("l'ombre")`. Checked by [`parse_noun`].
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub enum FrenchNounEntry {
    /// A masculine noun, written after "le " or "l'".
    Masculine(String),
    /// A feminine noun, written after "la " or "l'".
    Feminine(String),
}

/// The validated French name of a monster or an item: the noun without its
/// article, its gender, and whether its article is elided ("l'").
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FrenchNoun {
    /// The noun without its article, such as "rat d'ossuaire".
    noun: String,
    /// Its gender.
    gender: Gender,
    /// Whether its definite article is "l'" rather than "le" or "la".
    elided: bool,
}

impl FrenchNoun {
    /// Returns the noun without its article, such as "rat d'ossuaire"; never
    /// blank, at most `content::MAX_NAME_LENGTH` characters.
    pub fn noun(&self) -> &str {
        &self.noun
    }

    /// Returns the gender of the noun.
    pub const fn gender(&self) -> Gender {
        self.gender
    }

    /// Returns whether the definite article is elided: "l'ombre", but "le
    /// hiérophante".
    pub const fn elided(&self) -> bool {
        self.elided
    }

    /// A name that stands for no content entry: an empty masculine noun.
    /// Only the `save` module's stand-in definitions use it.
    pub(crate) const fn stand_in() -> Self {
        Self {
            noun: String::new(),
            gender: Gender::Masculine,
            elided: false,
        }
    }
}

/// Why a French name as written is refused by [`parse_noun`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NounProblem {
    /// The text does not start with an article that suits its gender.
    BadArticle,
    /// Nothing follows the article.
    Empty,
}

/// Reads `entry` into a [`FrenchNoun`]: the text must start with its
/// definite article, `le ` for a masculine noun, `la ` for a feminine one,
/// or `l'` for either, followed by a noun that is not blank.
///
/// # Errors
///
/// Returns [`NounProblem::BadArticle`] when the article is missing or does
/// not suit the gender ("la rat"), and [`NounProblem::Empty`] when only the
/// article is written.
pub(super) fn parse_noun(entry: FrenchNounEntry) -> Result<FrenchNoun, NounProblem> {
    let (gender, text) = match entry {
        FrenchNounEntry::Masculine(text) => (Gender::Masculine, text),
        FrenchNounEntry::Feminine(text) => (Gender::Feminine, text),
    };
    let article = match gender {
        Gender::Masculine => "le ",
        Gender::Feminine => "la ",
    };
    let (noun, elided) = if let Some(noun) = text.strip_prefix("l'") {
        (noun, true)
    } else if let Some(noun) = text.strip_prefix(article) {
        (noun, false)
    } else {
        return Err(NounProblem::BadArticle);
    };
    if noun.trim().is_empty() {
        return Err(NounProblem::Empty);
    }
    Ok(FrenchNoun {
        noun: noun.to_owned(),
        gender,
        elided,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_article_gives_the_elision_and_must_suit_the_gender() {
        let rat = parse_noun(FrenchNounEntry::Masculine("le rat".to_owned())).unwrap();
        assert_eq!(
            (rat.noun(), rat.gender(), rat.elided()),
            ("rat", Gender::Masculine, false)
        );
        let shade = parse_noun(FrenchNounEntry::Feminine("l'ombre".to_owned())).unwrap();
        assert_eq!(
            (shade.noun(), shade.gender(), shade.elided()),
            ("ombre", Gender::Feminine, true)
        );

        for wrong in [
            FrenchNounEntry::Feminine("le rat".to_owned()),
            FrenchNounEntry::Masculine("la fiole".to_owned()),
            FrenchNounEntry::Masculine("rat".to_owned()),
            FrenchNounEntry::Masculine("L'ombre".to_owned()),
        ] {
            assert_eq!(parse_noun(wrong), Err(NounProblem::BadArticle));
        }
        assert_eq!(
            parse_noun(FrenchNounEntry::Feminine("la  ".to_owned())),
            Err(NounProblem::Empty)
        );
    }
}
