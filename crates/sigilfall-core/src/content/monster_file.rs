//! Reading of the monster file's RON text into raw entries.
//!
//! Exposes [`read`] and [`MonsterEntry`]. While parsing, the reader records
//! which entry and which field it is in, so that a syntax error (a missing
//! comma, a value of the wrong type, an unknown field or family) is reported
//! with that entry and field as well as its line. A missing field is not an
//! error here: every field of [`MonsterEntry`] is optional, and the `monster`
//! module reports it with the entry's identifier. Bounds are not checked
//! here either.

use std::cell::RefCell;
use std::fmt;

use serde::Deserialize;
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};

use super::MONSTERS_FILE;
use super::error::ContentError;
use super::monster::{Family, entry_label};

/// One entry of the monster file as written, before validation.
///
/// A field absent from the file is `None`. See `MonsterDef` for the meaning
/// of each field.
#[derive(Debug, Default)]
pub(super) struct MonsterEntry {
    /// Stable identifier, not yet checked.
    pub(super) id: Option<String>,
    /// Name shown to the player.
    pub(super) name: Option<String>,
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

/// Parses `source`, the text of the monster file: a RON list of
/// `Monster(...)` entries.
///
/// # Errors
///
/// Returns [`ContentError::Syntax`] when the text is not valid RON or does
/// not have that shape, with the line and column the parser stopped at, and
/// the entry and field it was reading.
pub(super) fn read(source: &str) -> Result<Vec<MonsterEntry>, ContentError> {
    let progress = RefCell::new(Progress::default());
    ron::Options::default()
        .from_str_seed(source, EntryList(&progress))
        .map_err(|error| ContentError::Syntax {
            file: MONSTERS_FILE,
            line: error.span.start.line,
            column: error.span.start.col,
            place: progress.borrow().describe(),
            message: error.code.to_string(),
        })
}

/// Names of the fields of a `Monster(...)` entry, in the order of [`Field`].
const FIELDS: &[&str] = &[
    "id",
    "name",
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
enum Field {
    /// `id`
    Id,
    /// `name`
    Name,
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

impl Field {
    /// Returns the name of the field as written in the file.
    const fn name(self) -> &'static str {
        FIELDS[self as usize]
    }
}

/// Where the parser is in the file, kept up to date while reading so that an
/// error can say where it happened.
#[derive(Debug, Default)]
struct Progress {
    /// Position of the entry being read, counted from 1; 0 before the first.
    entry: usize,
    /// Identifier of that entry, once its `id` field has been read.
    id: Option<String>,
    /// Where the parser is relative to the entries and their fields.
    place: Place,
}

/// Where the parser is relative to the entries and their fields.
#[derive(Debug, Default, Clone, Copy)]
enum Place {
    /// No entry has started yet (the list itself is being opened).
    #[default]
    BeforeFirstEntry,
    /// An entry has started and none of its fields has been read yet.
    EntryStart,
    /// The value of this field is being read.
    InField(&'static str),
    /// The value of this field has been read; the next field or the end of
    /// the entry is expected.
    AfterField(&'static str),
    /// The list is closed; only the end of the file is expected.
    AfterLastEntry,
}

impl Progress {
    /// Describes the current place for an error message, such as
    /// "monster `shade` (entry 6), field `speed`".
    fn describe(&self) -> String {
        let entry = entry_label(self.entry, self.id.as_deref());
        match self.place {
            Place::BeforeFirstEntry => "before the first monster".to_owned(),
            Place::EntryStart => entry,
            Place::InField(field) => format!("{entry}, field `{field}`"),
            Place::AfterField(field) => format!("{entry}, after field `{field}`"),
            Place::AfterLastEntry => "after the last monster".to_owned(),
        }
    }
}

/// Reads the list of entries, recording progress in the shared [`Progress`].
#[derive(Clone, Copy)]
struct EntryList<'a>(&'a RefCell<Progress>);

impl<'de> DeserializeSeed<'de> for EntryList<'_> {
    type Value = Vec<MonsterEntry>;

    /// Reads the list; see the [`Visitor`] implementation.
    fn deserialize<D: de::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_seq(self)
    }
}

impl<'de> Visitor<'de> for EntryList<'_> {
    type Value = Vec<MonsterEntry>;

    /// Names what the file must contain, for type errors.
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a list of `Monster(...)` entries")
    }

    /// Reads every entry in order, then marks the list as closed.
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut entries = Vec::new();
        while let Some(entry) = seq.next_element_seed(EntryReader(self.0))? {
            entries.push(entry);
        }
        self.0.borrow_mut().place = Place::AfterLastEntry;
        Ok(entries)
    }
}

/// Reads one `Monster(...)` entry, recording progress in the shared
/// [`Progress`].
#[derive(Clone, Copy)]
struct EntryReader<'a>(&'a RefCell<Progress>);

impl<'de> DeserializeSeed<'de> for EntryReader<'_> {
    type Value = MonsterEntry;

    /// Records that a new entry starts, then reads it; see the [`Visitor`]
    /// implementation.
    fn deserialize<D: de::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        {
            let mut progress = self.0.borrow_mut();
            progress.entry += 1;
            progress.id = None;
            progress.place = Place::EntryStart;
        }
        deserializer.deserialize_struct("Monster", FIELDS, self)
    }
}

impl<'de> Visitor<'de> for EntryReader<'_> {
    type Value = MonsterEntry;

    /// Names what an entry must look like, for type errors.
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a `Monster(...)` entry")
    }

    /// Reads the fields of the entry in any order, refusing unknown and
    /// repeated ones, and records the identifier as soon as it is read.
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut entry = MonsterEntry::default();
        while let Some(field) = map.next_key::<Field>()? {
            self.0.borrow_mut().place = Place::InField(field.name());
            match field {
                Field::Id => {
                    let id: String = map.next_value()?;
                    self.0.borrow_mut().id = Some(id.clone());
                    fill(&mut entry.id, id, field)?;
                }
                Field::Name => fill(&mut entry.name, map.next_value()?, field)?,
                Field::Family => fill(&mut entry.family, map.next_value()?, field)?,
                Field::Elite => fill(&mut entry.elite, map.next_value()?, field)?,
                Field::Speed => fill(&mut entry.speed, map.next_value()?, field)?,
                Field::Hp => fill(&mut entry.hp, map.next_value()?, field)?,
                Field::Attack => fill(&mut entry.attack, map.next_value()?, field)?,
                Field::Defense => fill(&mut entry.defense, map.next_value()?, field)?,
                Field::MinDepth => fill(&mut entry.min_depth, map.next_value()?, field)?,
            }
            self.0.borrow_mut().place = Place::AfterField(field.name());
        }
        Ok(entry)
    }
}

/// Stores `value` read for `field` into `slot`.
///
/// # Errors
///
/// Returns a duplicate-field error if `slot` already holds a value, that is
/// when the entry writes the same field twice.
fn fill<T, E: de::Error>(slot: &mut Option<T>, value: T, field: Field) -> Result<(), E> {
    if slot.is_some() {
        return Err(E::duplicate_field(field.name()));
    }
    *slot = Some(value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_names_match_the_names_read_from_the_file() {
        for &name in FIELDS {
            let field: Field = ron::from_str(name).expect("known field name");
            assert_eq!(field.name(), name);
        }
    }
}
