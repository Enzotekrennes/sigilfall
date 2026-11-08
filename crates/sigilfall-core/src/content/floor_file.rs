//! The shape of an entry of the floor file, as the `entry_file` reader reads
//! it.
//!
//! Exposes [`FloorEntry`], one `Floor(...)` rule before validation, which
//! implements `RawEntry`. The kind of floor is read as a [`FloorKind`]
//! directly, so an unknown kind is a syntax error naming the rule and the
//! field. Bounds, overlaps and missing fields are checked by the `floor`
//! module of `content`, not here.

use serde::Deserialize;
use serde::de::MapAccess;

use super::FLOORS_FILE;
use super::entry_file::{EntryKind, RawEntry, fill};
use super::floor::FloorKind;

/// One rule of the floor file as written, before validation.
///
/// A field absent from the file is `None`. See `content/floors.ron` for the
/// meaning of each field.
#[derive(Debug, Default)]
pub(super) struct FloorEntry {
    /// The kind of floor the rule weighs.
    pub(super) kind: Option<FloorKind>,
    /// First floor the rule covers.
    pub(super) from_depth: Option<u32>,
    /// Last floor the rule covers, included.
    pub(super) to_depth: Option<u32>,
    /// Relative chance of the kind on those floors; read signed so that a
    /// negative weight is reported as out of range rather than as a type
    /// error.
    pub(super) weight: Option<i32>,
}

/// Names of the fields of a `Floor(...)` entry, in the order of [`Field`].
const FIELDS: &[&str] = &["kind", "from_depth", "to_depth", "weight"];

/// A field of a `Floor(...)` entry, read from its name in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(field_identifier, rename_all = "snake_case")]
pub(super) enum Field {
    /// `kind`
    Kind,
    /// `from_depth`
    FromDepth,
    /// `to_depth`
    ToDepth,
    /// `weight`
    Weight,
}

impl RawEntry for FloorEntry {
    type Field = Field;

    const KIND: EntryKind = EntryKind {
        file: FLOORS_FILE,
        noun: "floor",
        tag: "Floor",
    };

    const FIELDS: &'static [&'static str] = FIELDS;

    /// Returns the name of `field` as written in the file.
    fn field_name(field: Field) -> &'static str {
        FIELDS[field as usize]
    }

    /// Returns the kind of floor as written (such as `Caves`), once the
    /// `kind` field has been read: rules are named after the kind they
    /// weigh.
    fn id(&self) -> Option<&str> {
        self.kind.map(FloorKind::name)
    }

    /// Reads the value of `field` into the matching member.
    fn read_field<'de, A: MapAccess<'de>>(
        &mut self,
        field: Field,
        map: &mut A,
    ) -> Result<(), A::Error> {
        let name = Self::field_name(field);
        match field {
            Field::Kind => fill(&mut self.kind, map.next_value()?, name),
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
            assert_eq!(FloorEntry::field_name(field), name);
        }
    }

    #[test]
    fn a_kind_is_read_by_the_name_the_rules_are_named_after() {
        for kind in FloorKind::ALL {
            let read: FloorKind = ron::from_str(kind.name()).expect("a known kind");
            assert_eq!(read, kind);
        }
        assert!(ron::from_str::<FloorKind>("Mazes").is_err());
    }
}
