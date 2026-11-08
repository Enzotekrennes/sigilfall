//! Field checks shared by the validation of every kind of entry.
//!
//! Exposes [`EntryCheck`], which checks the fields of one entry and builds
//! errors naming it, and [`UniqueIds`], which refuses an identifier used by
//! two entries of the same file. The rules specific to a kind of entry live
//! in its own module (`monster`, `sigil`).

use std::collections::HashMap;

use super::MAX_NAME_LENGTH;
use super::entry_file::EntryKind;
use super::error::{ContentError, FieldProblem};

/// Checks the fields of one entry, building errors that name it.
pub(super) struct EntryCheck {
    /// Path of the entry's file, for error messages.
    file: &'static str,
    /// How errors designate the entry (see [`EntryKind::label`]).
    label: String,
}

impl EntryCheck {
    /// Checks for the entry of `kind` at `position` (counted from 1),
    /// identified by `id` once it is known to be valid.
    pub(super) fn new(kind: &EntryKind, position: usize, id: Option<&str>) -> Self {
        Self {
            file: kind.file,
            label: kind.label(position, id),
        }
    }

    /// Builds the error for `field` of this entry.
    pub(super) fn fail(&self, field: &'static str, problem: FieldProblem) -> ContentError {
        ContentError::InvalidField {
            file: self.file,
            entry: self.label.clone(),
            field,
            problem,
        }
    }

    /// Returns the value of `field`, or an error if it is missing.
    pub(super) fn required<T>(
        &self,
        value: Option<T>,
        field: &'static str,
    ) -> Result<T, ContentError> {
        value.ok_or_else(|| self.fail(field, FieldProblem::Missing))
    }

    /// Returns the text of `field`, refusing a missing or blank one.
    pub(super) fn text(
        &self,
        value: Option<String>,
        field: &'static str,
    ) -> Result<String, ContentError> {
        let text = self.required(value, field)?;
        if text.trim().is_empty() {
            return Err(self.fail(field, FieldProblem::Empty));
        }
        Ok(text)
    }

    /// Returns the name in `value`, the field `name` shown to the player,
    /// refusing a missing or blank one, or one longer than
    /// [`MAX_NAME_LENGTH`] characters.
    pub(super) fn name(&self, value: Option<String>) -> Result<String, ContentError> {
        let name = self.text(value, "name")?;
        let length = name.chars().count();
        if length > MAX_NAME_LENGTH {
            return Err(self.fail(
                "name",
                FieldProblem::TooLong {
                    length,
                    max: MAX_NAME_LENGTH,
                },
            ));
        }
        Ok(name)
    }

    /// Returns the identifier in `value`, refusing a missing one or one that
    /// is not lower-case letters, digits and underscores starting with a
    /// letter: that form survives renames of the displayed name and is safe in
    /// save files.
    pub(super) fn identifier(&self, value: Option<String>) -> Result<String, ContentError> {
        let id = self.required(value, "id")?;
        let mut chars = id.chars();
        let starts_with_letter = chars.next().is_some_and(|c| c.is_ascii_lowercase());
        let rest_is_valid = chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if !(starts_with_letter && rest_is_valid) {
            return Err(self.fail("id", FieldProblem::BadIdentifier { id }));
        }
        Ok(id)
    }

    /// Returns the value of `field`, refusing a missing one or one outside
    /// `min..=max`.
    pub(super) fn in_range(
        &self,
        value: Option<i32>,
        field: &'static str,
        min: i32,
        max: i32,
    ) -> Result<i32, ContentError> {
        let value = self.required(value, field)?;
        if !(min..=max).contains(&value) {
            return Err(self.fail(
                field,
                FieldProblem::OutOfRange {
                    value: value.into(),
                    min: min.into(),
                    max: max.into(),
                },
            ));
        }
        Ok(value)
    }

    /// Returns the value of `field`, refusing a missing one or one below
    /// `min`.
    pub(super) fn at_least(
        &self,
        value: Option<u32>,
        field: &'static str,
        min: u32,
    ) -> Result<u32, ContentError> {
        let value = self.required(value, field)?;
        if value < min {
            return Err(self.fail(
                field,
                FieldProblem::TooLow {
                    value: value.into(),
                    min: min.into(),
                },
            ));
        }
        Ok(value)
    }
}

/// The identifiers already used in one file, with the entry that used each
/// first.
pub(super) struct UniqueIds {
    /// The kind of entry the file holds, for error messages.
    kind: EntryKind,
    /// Position (counted from 1) of the first entry using each identifier.
    first_entry_of: HashMap<String, usize>,
}

impl UniqueIds {
    /// No identifier used yet in a file of `kind` entries.
    pub(super) fn new(kind: EntryKind) -> Self {
        Self {
            kind,
            first_entry_of: HashMap::new(),
        }
    }

    /// Records that the entry at `position` (counted from 1) uses `id`.
    ///
    /// # Errors
    ///
    /// Returns [`ContentError::InvalidField`] on field `id`, with
    /// [`FieldProblem::Duplicate`] naming the first entry, if an earlier entry
    /// already uses `id`.
    pub(super) fn claim(&mut self, position: usize, id: &str) -> Result<(), ContentError> {
        if let Some(&first_entry) = self.first_entry_of.get(id) {
            return Err(EntryCheck::new(&self.kind, position, Some(id)).fail(
                "id",
                FieldProblem::Duplicate {
                    id: id.to_owned(),
                    first_entry,
                },
            ));
        }
        self.first_entry_of.insert(id.to_owned(), position);
        Ok(())
    }
}
