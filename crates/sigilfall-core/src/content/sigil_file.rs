//! The shape of an entry of the sigil file, as the `entry_file` reader reads
//! it.
//!
//! Exposes [`SigilEntry`], one `Sigil(...)` entry before validation, which
//! implements `RawEntry`. Patterns, bounds and missing fields are checked by
//! the `sigil` module of `content`, not here.

use serde::Deserialize;
use serde::de::MapAccess;

use super::SIGILS_FILE;
use super::entry_file::{EntryKind, RawEntry, fill};
use super::sigil::{Effect, Targeting};

/// One entry of the sigil file as written, before validation.
///
/// A field absent from the file is `None`. See `SigilDef` for the meaning of
/// each field.
#[derive(Debug, Default)]
pub(super) struct SigilEntry {
    /// Stable identifier, not yet checked.
    pub(super) id: Option<String>,
    /// Name shown to the player.
    pub(super) name: Option<String>,
    /// Digits of the nodes to link, in order, not yet checked.
    pub(super) pattern: Option<Vec<u8>>,
    /// What the sigil does.
    pub(super) effect: Option<Effect>,
    /// Strength of the effect.
    pub(super) power: Option<i32>,
    /// Whom or where the effect applies.
    pub(super) targeting: Option<Targeting>,
    /// Shallowest floor where the sigil's glyph can be found.
    pub(super) min_depth: Option<u32>,
    /// Whether a new run starts with the sigil in the grimoire.
    pub(super) known_at_start: Option<bool>,
}

/// Names of the fields of a `Sigil(...)` entry, in the order of [`Field`].
const FIELDS: &[&str] = &[
    "id",
    "name",
    "pattern",
    "effect",
    "power",
    "targeting",
    "min_depth",
    "known_at_start",
];

/// A field of a `Sigil(...)` entry, read from its name in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(field_identifier, rename_all = "snake_case")]
pub(super) enum Field {
    /// `id`
    Id,
    /// `name`
    Name,
    /// `pattern`
    Pattern,
    /// `effect`
    Effect,
    /// `power`
    Power,
    /// `targeting`
    Targeting,
    /// `min_depth`
    MinDepth,
    /// `known_at_start`
    KnownAtStart,
}

impl RawEntry for SigilEntry {
    type Field = Field;

    const KIND: EntryKind = EntryKind {
        file: SIGILS_FILE,
        noun: "sigil",
        tag: "Sigil",
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
            Field::Pattern => fill(&mut self.pattern, map.next_value()?, name),
            Field::Effect => fill(&mut self.effect, map.next_value()?, name),
            Field::Power => fill(&mut self.power, map.next_value()?, name),
            Field::Targeting => fill(&mut self.targeting, map.next_value()?, name),
            Field::MinDepth => fill(&mut self.min_depth, map.next_value()?, name),
            Field::KnownAtStart => fill(&mut self.known_at_start, map.next_value()?, name),
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
            assert_eq!(SigilEntry::field_name(field), name);
        }
    }
}
