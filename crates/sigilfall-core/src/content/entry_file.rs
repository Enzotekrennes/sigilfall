//! Reading of a content file's RON text into raw entries, for any kind of
//! entry (monsters, sigils).
//!
//! Exposes [`read`], the [`RawEntry`] trait each kind of entry implements to
//! be read, [`EntryKind`], which names a kind of entry in messages, and
//! [`fill`]. While parsing, the reader records which entry and which field it
//! is in, so that a syntax error (a missing comma, a value of the wrong type,
//! an unknown field or variant) is reported with that entry and field as well
//! as its line. A missing field is not an error here: every field of a raw
//! entry is optional, and validation reports it with the entry's identifier.
//! Bounds are not checked here either.

use std::cell::RefCell;
use std::fmt;
use std::marker::PhantomData;

use serde::de::{self, DeserializeOwned, DeserializeSeed, MapAccess, SeqAccess, Visitor};

use super::error::ContentError;

/// How a kind of entry is named in its file and in error messages.
#[derive(Debug, Clone, Copy)]
pub(super) struct EntryKind {
    /// Path of the file, relative to the crate, such as
    /// "content/monsters.ron".
    pub(super) file: &'static str,
    /// Noun for one entry in messages, such as "monster".
    pub(super) noun: &'static str,
    /// Name of the RON struct each entry is written as, such as "Monster".
    pub(super) tag: &'static str,
}

impl EntryKind {
    /// Describes the entry at `position` (counted from 1) in error messages,
    /// with its identifier when known: "monster `acolyte` (entry 3)", or
    /// "monster entry 3".
    pub(super) fn label(&self, position: usize, id: Option<&str>) -> String {
        let noun = self.noun;
        match id {
            Some(id) => format!("{noun} `{id}` (entry {position})"),
            None => format!("{noun} entry {position}"),
        }
    }
}

/// One entry of a content file as written, before validation.
///
/// Implemented by the raw entry of each kind, whose fields are all optional
/// so that a missing one is reported by validation with the entry's
/// identifier.
pub(super) trait RawEntry: Default {
    /// The fields of the entry, read from their names in the file; unknown
    /// names are refused when reading them.
    type Field: DeserializeOwned + Copy;

    /// How this kind of entry is named.
    const KIND: EntryKind;

    /// Names of the fields, as written in the file.
    const FIELDS: &'static [&'static str];

    /// Returns the name of `field` as written in the file.
    fn field_name(field: Self::Field) -> &'static str;

    /// Returns the identifier read so far, if the `id` field has been read.
    fn id(&self) -> Option<&str>;

    /// Reads the value of `field` from `map` and stores it in the entry.
    ///
    /// # Errors
    ///
    /// Returns the parser's error for a value of the wrong type, and a
    /// duplicate-field error (see [`fill`]) if the field was already read.
    fn read_field<'de, A: MapAccess<'de>>(
        &mut self,
        field: Self::Field,
        map: &mut A,
    ) -> Result<(), A::Error>;
}

/// Parses `source`, the text of a content file: a RON list of entries of
/// kind `E`, such as `Monster(...)`.
///
/// # Errors
///
/// Returns [`ContentError::Syntax`] when the text is not valid RON or does
/// not have that shape, with the line and column the parser stopped at, and
/// the entry and field it was reading.
pub(super) fn read<E: RawEntry>(source: &str) -> Result<Vec<E>, ContentError> {
    let progress = RefCell::new(Progress::new(E::KIND));
    ron::Options::default()
        .from_str_seed(source, EntryList::<E>::new(&progress))
        .map_err(|error| ContentError::Syntax {
            file: E::KIND.file,
            line: error.span.start.line,
            column: error.span.start.col,
            place: progress.borrow().describe(),
            message: error.code.to_string(),
        })
}

/// Stores `value` read for the field named `field` into `slot`.
///
/// # Errors
///
/// Returns a duplicate-field error if `slot` already holds a value, that is
/// when the entry writes the same field twice.
pub(super) fn fill<T, E: de::Error>(
    slot: &mut Option<T>,
    value: T,
    field: &'static str,
) -> Result<(), E> {
    if slot.is_some() {
        return Err(E::duplicate_field(field));
    }
    *slot = Some(value);
    Ok(())
}

/// Where the parser is in the file, kept up to date while reading so that an
/// error can say where it happened.
#[derive(Debug)]
struct Progress {
    /// The kind of entry the file holds.
    kind: EntryKind,
    /// Position of the entry being read, counted from 1; 0 before the first.
    entry: usize,
    /// Identifier of that entry, once its `id` field has been read.
    id: Option<String>,
    /// Where the parser is relative to the entries and their fields.
    place: Place,
}

/// Where the parser is relative to the entries and their fields.
#[derive(Debug, Clone, Copy)]
enum Place {
    /// No entry has started yet (the list itself is being opened).
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
    /// Progress at the start of a file of `kind` entries.
    const fn new(kind: EntryKind) -> Self {
        Self {
            kind,
            entry: 0,
            id: None,
            place: Place::BeforeFirstEntry,
        }
    }

    /// Describes the current place for an error message, such as
    /// "monster `shade` (entry 6), field `speed`".
    fn describe(&self) -> String {
        let entry = self.kind.label(self.entry, self.id.as_deref());
        let noun = self.kind.noun;
        match self.place {
            Place::BeforeFirstEntry => format!("before the first {noun}"),
            Place::EntryStart => entry,
            Place::InField(field) => format!("{entry}, field `{field}`"),
            Place::AfterField(field) => format!("{entry}, after field `{field}`"),
            Place::AfterLastEntry => format!("after the last {noun}"),
        }
    }
}

/// Reads the list of entries, recording progress in the shared [`Progress`].
struct EntryList<'a, E> {
    /// Where the parser is, shared with [`read`] for error messages.
    progress: &'a RefCell<Progress>,
    /// The kind of entry read; no value is stored.
    entry: PhantomData<fn() -> E>,
}

impl<'a, E> EntryList<'a, E> {
    /// A reader recording its progress in `progress`.
    const fn new(progress: &'a RefCell<Progress>) -> Self {
        Self {
            progress,
            entry: PhantomData,
        }
    }
}

impl<'de, E: RawEntry> DeserializeSeed<'de> for EntryList<'_, E> {
    type Value = Vec<E>;

    /// Reads the list; see the [`Visitor`] implementation.
    fn deserialize<D: de::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_seq(self)
    }
}

impl<'de, E: RawEntry> Visitor<'de> for EntryList<'_, E> {
    type Value = Vec<E>;

    /// Names what the file must contain, for type errors.
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "a list of `{}(...)` entries", E::KIND.tag)
    }

    /// Reads every entry in order, then marks the list as closed.
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut entries = Vec::new();
        while let Some(entry) = seq.next_element_seed(EntryReader::<E>::new(self.progress))? {
            entries.push(entry);
        }
        self.progress.borrow_mut().place = Place::AfterLastEntry;
        Ok(entries)
    }
}

/// Reads one entry, recording progress in the shared [`Progress`].
struct EntryReader<'a, E> {
    /// Where the parser is, shared with [`read`] for error messages.
    progress: &'a RefCell<Progress>,
    /// The kind of entry read; no value is stored.
    entry: PhantomData<fn() -> E>,
}

impl<'a, E> EntryReader<'a, E> {
    /// A reader recording its progress in `progress`.
    const fn new(progress: &'a RefCell<Progress>) -> Self {
        Self {
            progress,
            entry: PhantomData,
        }
    }
}

impl<'de, E: RawEntry> DeserializeSeed<'de> for EntryReader<'_, E> {
    type Value = E;

    /// Records that a new entry starts, then reads it; see the [`Visitor`]
    /// implementation.
    fn deserialize<D: de::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        {
            let mut progress = self.progress.borrow_mut();
            progress.entry += 1;
            progress.id = None;
            progress.place = Place::EntryStart;
        }
        deserializer.deserialize_struct(E::KIND.tag, E::FIELDS, self)
    }
}

impl<'de, E: RawEntry> Visitor<'de> for EntryReader<'_, E> {
    type Value = E;

    /// Names what an entry must look like, for type errors.
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "a `{}(...)` entry", E::KIND.tag)
    }

    /// Reads the fields of the entry in any order, refusing unknown and
    /// repeated ones, and records the identifier as soon as it is read.
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut entry = E::default();
        while let Some(field) = map.next_key::<E::Field>()? {
            let name = E::field_name(field);
            self.progress.borrow_mut().place = Place::InField(name);
            entry.read_field(field, &mut map)?;
            let mut progress = self.progress.borrow_mut();
            progress.id = entry.id().map(ToOwned::to_owned);
            progress.place = Place::AfterField(name);
        }
        Ok(entry)
    }
}
