//! The shape of an entry of the item file, as the `entry_file` reader reads
//! it.
//!
//! Exposes [`ItemEntry`], one `Item(...)` entry before validation, which
//! implements `RawEntry`, and [`EffectEntry`], its effect as written. Bounds,
//! references to sigils and missing fields are checked by the `item` module
//! of `content`, not here.

use serde::Deserialize;
use serde::de::MapAccess;

use super::ITEMS_FILE;
use super::entry_file::{EntryKind, RawEntry, fill};
use super::french::FrenchNounEntry;

/// The effect of an item as written in the file, before validation: the
/// strength is not bounded yet and a taught sigil is only an identifier.
///
/// See `ItemEffect` for the meaning of each variant.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(super) enum EffectEntry {
    /// Hit points restored.
    Heal(i32),
    /// Essence restored.
    RestoreEssence(i32),
    /// Identifier of the sigil taught, not yet looked up.
    Teach(String),
    /// Number of unstable casts made pure.
    Steady(i32),
}

/// One entry of the item file as written, before validation.
///
/// A field absent from the file is `None`. See `ItemDef` for the meaning of
/// each field.
#[derive(Debug, Default)]
pub(super) struct ItemEntry {
    /// Stable identifier, not yet checked.
    pub(super) id: Option<String>,
    /// Name shown to the player, in English.
    pub(super) name: Option<String>,
    /// French name, with its article and gender.
    pub(super) name_fr: Option<FrenchNounEntry>,
    /// What using the item does.
    pub(super) effect: Option<EffectEntry>,
    /// Shallowest floor the item can be found on.
    pub(super) min_depth: Option<u32>,
}

/// Names of the fields of an `Item(...)` entry, in the order of [`Field`].
const FIELDS: &[&str] = &["id", "name", "name_fr", "effect", "min_depth"];

/// A field of an `Item(...)` entry, read from its name in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(field_identifier, rename_all = "snake_case")]
pub(super) enum Field {
    /// `id`
    Id,
    /// `name`
    Name,
    /// `name_fr`
    NameFr,
    /// `effect`
    Effect,
    /// `min_depth`
    MinDepth,
}

impl RawEntry for ItemEntry {
    type Field = Field;

    const KIND: EntryKind = EntryKind {
        file: ITEMS_FILE,
        noun: "item",
        tag: "Item",
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
            Field::Effect => fill(&mut self.effect, map.next_value()?, name),
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
            assert_eq!(ItemEntry::field_name(field), name);
        }
    }

    #[test]
    fn effects_are_read_with_their_value() {
        let read = |text: &str| ron::from_str::<EffectEntry>(text).expect("a known effect");

        assert_eq!(read("Heal(8)"), EffectEntry::Heal(8));
        assert_eq!(read("RestoreEssence(3)"), EffectEntry::RestoreEssence(3));
        assert_eq!(
            read("Teach(\"veil\")"),
            EffectEntry::Teach("veil".to_owned())
        );
        assert_eq!(read("Steady(2)"), EffectEntry::Steady(2));
    }
}
