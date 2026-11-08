//! Monster definitions and the rules a monster file must follow.
//!
//! Exposes [`MonsterDef`], the validated description of one kind of
//! creature, [`Family`], and the bounds every definition must respect
//! ([`MAX_SPEED`], [`MAX_HP`], [`MAX_ATTACK`], [`MAX_DEFENSE`]). [`validate`]
//! turns the raw entries read by the `monster_file` module into definitions,
//! or reports the first fault with the entry and field in cause. This module
//! does not read RON, create actors nor pick which monsters spawn.

use std::collections::HashMap;

use serde::Deserialize;

use super::MONSTERS_FILE;
use super::error::{ContentError, FieldProblem};
use super::monster_file::MonsterEntry;
use crate::generation::FIRST_DEPTH;
use crate::schedule::NORMAL_SPEED;

/// Highest accepted speed.
///
/// Four times [`NORMAL_SPEED`]: such a creature already acts four times per
/// player move, and anything faster would kill before the player can react.
pub const MAX_SPEED: i32 = 4 * NORMAL_SPEED;

/// Highest accepted hit points.
///
/// Far above anything a balanced creature needs; it catches typos such as an
/// extra digit while leaving room for bosses.
pub const MAX_HP: i32 = 999;

/// Highest accepted attack value.
///
/// Two digits are plenty for the provisional combat values and keep a typo
/// from producing a creature that kills in one blow whatever the defense.
pub const MAX_ATTACK: i32 = 99;

/// Highest accepted defense value.
///
/// Same reasoning as [`MAX_ATTACK`]: a typo must not make a creature
/// impossible to hurt.
pub const MAX_DEFENSE: i32 = 99;

/// Family of a creature, which gives the letter drawn on the map: lower case
/// for the common form, upper case for the elite one (see `docs/design.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
pub enum Family {
    /// Creatures of flesh that swarm the lower halls (`v`, `V`).
    Vermin,
    /// Cultists who serve the dungeon (`a`, `A`).
    Acolyte,
    /// Artificial beings of alchemy (`h`, `H`).
    Homunculus,
    /// Floating watchers, creatures of spirit (`e`, `E`).
    Eye,
    /// Bodiless dead, creatures of spirit (`s`, `S`).
    Spectre,
}

/// One kind of creature, as described in the monster file and validated.
///
/// Every value respects the bounds of this module: speed and hit points are
/// strictly positive, and the creature may appear from floor
/// [`FIRST_DEPTH`] or deeper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonsterDef {
    /// Stable key used by saves and spawn tables.
    id: String,
    /// Name shown to the player.
    name: String,
    /// Family, which gives the letter drawn on the map.
    family: Family,
    /// Whether this is the strong form of its family.
    elite: bool,
    /// Energy gained per tick, from 1 to [`MAX_SPEED`].
    speed: i32,
    /// Hit points of a fresh creature, from 1 to [`MAX_HP`].
    hp: i32,
    /// Attack value, from 0 to [`MAX_ATTACK`].
    attack: i32,
    /// Defense value, from 0 to [`MAX_DEFENSE`].
    defense: i32,
    /// Shallowest floor the creature can appear on.
    min_depth: u32,
}

impl MonsterDef {
    /// Returns the stable identifier: lower-case letters, digits and
    /// underscores, unique among monsters.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the name shown to the player, in English.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the creature's family.
    pub const fn family(&self) -> Family {
        self.family
    }

    /// Returns whether the creature is the elite form of its family, drawn
    /// with an upper-case letter.
    pub const fn elite(&self) -> bool {
        self.elite
    }

    /// Returns the energy the creature gains per tick; always from 1 to
    /// [`MAX_SPEED`], so it can be given to `Actor::new` as is.
    pub const fn speed(&self) -> i32 {
        self.speed
    }

    /// Returns the hit points of a fresh creature; always from 1 to
    /// [`MAX_HP`].
    pub const fn hp(&self) -> i32 {
        self.hp
    }

    /// Returns the attack value; always from 0 to [`MAX_ATTACK`].
    pub const fn attack(&self) -> i32 {
        self.attack
    }

    /// Returns the defense value; always from 0 to [`MAX_DEFENSE`].
    pub const fn defense(&self) -> i32 {
        self.defense
    }

    /// Returns the shallowest floor the creature can appear on; at least
    /// [`FIRST_DEPTH`].
    pub const fn min_depth(&self) -> u32 {
        self.min_depth
    }

    /// Returns whether the creature may appear on floor `depth` (the floor
    /// number, [`FIRST_DEPTH`] for the first one).
    pub const fn can_appear_at(&self, depth: u32) -> bool {
        depth >= self.min_depth
    }
}

/// Describes the monster entry at `position` (counted from 1) in error
/// messages, with its identifier when known: "monster `acolyte` (entry 3)",
/// or "monster entry 3".
pub(super) fn entry_label(position: usize, id: Option<&str>) -> String {
    match id {
        Some(id) => format!("monster `{id}` (entry {position})"),
        None => format!("monster entry {position}"),
    }
}

/// Turns the entries of the monster file into definitions, in file order.
///
/// Each entry is checked in turn (identifier form and uniqueness, presence of
/// every field, bounds), then the whole list must hold at least one monster
/// able to appear on floor [`FIRST_DEPTH`].
///
/// # Errors
///
/// Returns the first fault found: [`ContentError::InvalidField`] naming the
/// entry and the field, [`ContentError::NoMonster`] for an empty list, or
/// [`ContentError::NoMonsterAtFirstDepth`].
pub(super) fn validate(entries: Vec<MonsterEntry>) -> Result<Vec<MonsterDef>, ContentError> {
    let mut monsters: Vec<MonsterDef> = Vec::with_capacity(entries.len());
    let mut first_entry_of: HashMap<String, usize> = HashMap::new();
    for (index, entry) in entries.into_iter().enumerate() {
        let position = index + 1;
        let monster = validate_entry(position, entry)?;
        if let Some(&first_entry) = first_entry_of.get(&monster.id) {
            return Err(EntryCheck::new(position, Some(&monster.id)).fail(
                "id",
                FieldProblem::Duplicate {
                    id: monster.id,
                    first_entry,
                },
            ));
        }
        first_entry_of.insert(monster.id.clone(), position);
        monsters.push(monster);
    }
    check_first_depth(&monsters)?;
    Ok(monsters)
}

/// Validates the entry at `position` (counted from 1) on its own, without
/// looking at the other entries.
fn validate_entry(position: usize, entry: MonsterEntry) -> Result<MonsterDef, ContentError> {
    let id = EntryCheck::new(position, None).identifier(entry.id)?;
    let check = EntryCheck::new(position, Some(&id));
    Ok(MonsterDef {
        name: check.text(entry.name, "name")?,
        family: check.required(entry.family, "family")?,
        elite: check.required(entry.elite, "elite")?,
        speed: check.in_range(entry.speed, "speed", 1, MAX_SPEED)?,
        hp: check.in_range(entry.hp, "hp", 1, MAX_HP)?,
        attack: check.in_range(entry.attack, "attack", 0, MAX_ATTACK)?,
        defense: check.in_range(entry.defense, "defense", 0, MAX_DEFENSE)?,
        min_depth: check.at_least(entry.min_depth, "min_depth", FIRST_DEPTH)?,
        id,
    })
}

/// Refuses a monster list that leaves floor [`FIRST_DEPTH`] without any
/// creature, naming the entry whose `min_depth` is the lowest.
fn check_first_depth(monsters: &[MonsterDef]) -> Result<(), ContentError> {
    if monsters
        .iter()
        .any(|monster| monster.can_appear_at(FIRST_DEPTH))
    {
        return Ok(());
    }
    let Some((index, shallowest)) = monsters
        .iter()
        .enumerate()
        .min_by_key(|(_, monster)| monster.min_depth)
    else {
        return Err(ContentError::NoMonster {
            file: MONSTERS_FILE,
        });
    };
    Err(ContentError::NoMonsterAtFirstDepth {
        file: MONSTERS_FILE,
        first_depth: FIRST_DEPTH,
        entry: entry_label(index + 1, Some(&shallowest.id)),
        min_depth: shallowest.min_depth,
    })
}

/// Checks the fields of one entry, building errors that name it.
struct EntryCheck {
    /// How errors designate the entry (see [`entry_label`]).
    label: String,
}

impl EntryCheck {
    /// Checks for the entry at `position` (counted from 1), identified by
    /// `id` once it is known to be valid.
    fn new(position: usize, id: Option<&str>) -> Self {
        Self {
            label: entry_label(position, id),
        }
    }

    /// Builds the error for `field` of this entry.
    fn fail(&self, field: &'static str, problem: FieldProblem) -> ContentError {
        ContentError::InvalidField {
            file: MONSTERS_FILE,
            entry: self.label.clone(),
            field,
            problem,
        }
    }

    /// Returns the value of `field`, or an error if it is missing.
    fn required<T>(&self, value: Option<T>, field: &'static str) -> Result<T, ContentError> {
        value.ok_or_else(|| self.fail(field, FieldProblem::Missing))
    }

    /// Returns the text of `field`, refusing a missing or blank one.
    fn text(&self, value: Option<String>, field: &'static str) -> Result<String, ContentError> {
        let text = self.required(value, field)?;
        if text.trim().is_empty() {
            return Err(self.fail(field, FieldProblem::Empty));
        }
        Ok(text)
    }

    /// Returns the identifier in `value`, refusing a missing one or one that
    /// is not lower-case letters, digits and underscores starting with a
    /// letter: that form survives renames of the displayed name and is safe in
    /// save files.
    fn identifier(&self, value: Option<String>) -> Result<String, ContentError> {
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
    fn in_range(
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
    fn at_least(
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
