//! The shape of an entry of the spawn file, as the `entry_file` reader reads
//! it.
//!
//! Exposes [`SpawnEntry`], one `Spawn(...)` rule before validation, which
//! implements `RawEntry`, and [`TargetEntry`], the creature or item it
//! places as written. References, bounds and missing fields are checked by
//! the `spawn` module of `content`, not here.

use serde::Deserialize;
use serde::de::MapAccess;

use super::SPAWNS_FILE;
use super::entry_file::{EntryKind, RawEntry, fill};

/// What a spawn rule places, as written in the file: `Monster("id")` or
/// `Item("id")`, the identifier not yet looked up.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(super) enum TargetEntry {
    /// Identifier of a monster of the monster file.
    Monster(String),
    /// Identifier of an item of the item file.
    Item(String),
}

impl TargetEntry {
    /// Returns the identifier the rule refers to, whatever its kind.
    pub(super) fn id(&self) -> &str {
        match self {
            Self::Monster(id) | Self::Item(id) => id,
        }
    }
}

/// One rule of the spawn file as written, before validation.
///
/// A field absent from the file is `None`. See `content/spawns.ron` for the
/// meaning of each field.
#[derive(Debug, Default)]
pub(super) struct SpawnEntry {
    /// The creature or item placed.
    pub(super) of: Option<TargetEntry>,
    /// First floor the rule covers.
    pub(super) from_depth: Option<u32>,
    /// Last floor the rule covers, included.
    pub(super) to_depth: Option<u32>,
    /// Relative chance of being drawn on those floors; read signed so that a
    /// negative weight is reported as out of range rather than as a type
    /// error.
    pub(super) weight: Option<i32>,
}

/// Names of the fields of a `Spawn(...)` entry, in the order of [`Field`].
const FIELDS: &[&str] = &["of", "from_depth", "to_depth", "weight"];

/// A field of a `Spawn(...)` entry, read from its name in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(field_identifier, rename_all = "snake_case")]
pub(super) enum Field {
    /// `of`
    Of,
    /// `from_depth`
    FromDepth,
    /// `to_depth`
    ToDepth,
    /// `weight`
    Weight,
}

impl RawEntry for SpawnEntry {
    type Field = Field;

    const KIND: EntryKind = EntryKind {
        file: SPAWNS_FILE,
        noun: "spawn",
        tag: "Spawn",
    };

    const FIELDS: &'static [&'static str] = FIELDS;

    /// Returns the name of `field` as written in the file.
    fn field_name(field: Field) -> &'static str {
        FIELDS[field as usize]
    }

    /// Returns the identifier of the creature or item placed, once the `of`
    /// field has been read: rules are named after what they place.
    fn id(&self) -> Option<&str> {
        self.of.as_ref().map(TargetEntry::id)
    }

    /// Reads the value of `field` into the matching member.
    fn read_field<'de, A: MapAccess<'de>>(
        &mut self,
        field: Field,
        map: &mut A,
    ) -> Result<(), A::Error> {
        let name = Self::field_name(field);
        match field {
            Field::Of => fill(&mut self.of, map.next_value()?, name),
            Field::FromDepth => fill(&mut self.from_depth, map.next_value()?, name),
            Field::ToDepth => fill(&mut self.to_depth, map.next_value()?, name),
            Field::Weight => fill(&mut self.weight, map.next_value()?, name),
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
            assert_eq!(SpawnEntry::field_name(field), name);
        }
    }

    #[test]
    fn a_target_is_read_with_its_kind_and_identifier() {
        let read = |text: &str| ron::from_str::<TargetEntry>(text).expect("a valid target");

        assert_eq!(
            read("Monster(\"acolyte\")"),
            TargetEntry::Monster("acolyte".to_owned())
        );
        assert_eq!(read("Item(\"veil_page\")").id(), "veil_page");
    }
}
