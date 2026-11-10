//! The shape of an entry of the monster file, as the `entry_file` reader
//! reads it.
//!
//! Exposes [`MonsterEntry`], one `Monster(...)` entry before validation,
//! which implements `RawEntry`. Bounds and missing fields are checked by the
//! `monster` module, not here.

use serde::Deserialize;
use serde::de::MapAccess;

use super::MONSTERS_FILE;
use super::entry_file::{EntryKind, RawEntry, fill};
use super::french::FrenchNounEntry;
use super::monster::Family;

/// One entry of the monster file as written, before validation.
///
/// A field absent from the file is `None`. See `MonsterDef` for the meaning
/// of each field.
#[derive(Debug, Default)]
pub(super) struct MonsterEntry {
    /// Stable identifier, not yet checked.
    pub(super) id: Option<String>,
    /// Name shown to the player, in English.
    pub(super) name: Option<String>,
    /// French name, with its article and gender.
    pub(super) name_fr: Option<FrenchNounEntry>,
    /// Name of several, in English.
    pub(super) plural: Option<String>,
    /// Name of several, in French, without an article.
    pub(super) plural_fr: Option<String>,
    /// Family of the creature.
    pub(super) family: Option<Family>,
    /// Whether this is the elite form of its family.
    pub(super) elite: Option<bool>,
    /// Energy gained per tick.
    pub(super) speed: Option<i32>,
    /// Hit points of a fresh creature.
    pub(super) hp: Option<i32>,
    /// Attack value.
    pub(super) attack: Option<i32>,
    /// Defense value.
    pub(super) defense: Option<i32>,
    /// Shallowest floor the creature can appear on.
    pub(super) min_depth: Option<u32>,
}

/// Names of the fields of a `Monster(...)` entry, in the order of [`Field`].
const FIELDS: &[&str] = &[
    "id",
    "name",
    "name_fr",
    "plural",
    "plural_fr",
    "family",
    "elite",
    "speed",
    "hp",
    "attack",
    "defense",
    "min_depth",
];

/// A field of a `Monster(...)` entry, read from its name in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(field_identifier, rename_all = "snake_case")]
pub(super) enum Field {
    /// `id`
    Id,
    /// `name`
    Name,
    /// `name_fr`
    NameFr,
    /// `plural`
    Plural,
    /// `plural_fr`
    PluralFr,
    /// `family`
    Family,
    /// `elite`
    Elite,
    /// `speed`
    Speed,
    /// `hp`
    Hp,
    /// `attack`
    Attack,
    /// `defense`
    Defense,
    /// `min_depth`
    MinDepth,
}

impl RawEntry for MonsterEntry {
    type Field = Field;

    const KIND: EntryKind = EntryKind {
        file: MONSTERS_FILE,
        noun: "monster",
        tag: "Monster",
    };

    const FIELDS: &'static [&'static str] = FIELDS;

    /// Returns the name of `field` as written in the file.
    fn field_name(field: Field) -> &'static str {
        FIELDS[field as usize]
    }

    /// Returns the identifier, once the `id` field has been read.
    fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    /// Reads the value of `field` into the matching member.
    fn read_field<'de, A: MapAccess<'de>>(
        &mut self,
        field: Field,
        map: &mut A,
    ) -> Result<(), A::Error> {
        let name = Self::field_name(field);
        match field {
            Field::Id => fill(&mut self.id, map.next_value()?, name),
            Field::Name => fill(&mut self.name, map.next_value()?, name),
            Field::NameFr => fill(&mut self.name_fr, map.next_value()?, name),
            Field::Plural => fill(&mut self.plural, map.next_value()?, name),
            Field::PluralFr => fill(&mut self.plural_fr, map.next_value()?, name),
            Field::Family => fill(&mut self.family, map.next_value()?, name),
            Field::Elite => fill(&mut self.elite, map.next_value()?, name),
            Field::Speed => fill(&mut self.speed, map.next_value()?, name),
            Field::Hp => fill(&mut self.hp, map.next_value()?, name),
            Field::Attack => fill(&mut self.attack, map.next_value()?, name),
            Field::Defense => fill(&mut self.defense, map.next_value()?, name),
            Field::MinDepth => fill(&mut self.min_depth, map.next_value()?, name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_names_match_the_names_read_from_the_file() {
        for &name in FIELDS {
            let field: Field = ron::from_str(name).expect("known field name");
            assert_eq!(MonsterEntry::field_name(field), name);
        }
    }
}
