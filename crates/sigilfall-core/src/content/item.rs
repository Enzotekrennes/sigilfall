//! Item definitions and the rules an item file must follow.
//!
//! Exposes [`ItemDef`], the validated description of one kind of item, its
//! [`ItemEffect`] and its [`ItemFamily`], which gives the glyph drawn for it
//! (see `theme::glyphs` in the binary). [`validate`] turns the raw entries
//! read from the item file into definitions, or reports the first fault with
//! the entry and field in cause: a missing field, a strength out of bounds, a
//! sigil that the sigil file does not define, a repeated identifier.
//!
//! This module describes items; placing them on a floor and using them is
//! done by the generation and the game.

use super::check::{EntryCheck, UniqueIds};
use super::entry_file::RawEntry;
use super::error::{ContentError, FieldProblem};
use super::item_file::{EffectEntry, ItemEntry};
use super::{MAX_POWER, SIGILS_FILE, SigilDef};
use crate::sigil::{MAX_ESSENCE, MAX_STEADY_CASTS};

/// What using an item does. The effect is spent with the item: every item is
/// used once.
///
/// The list is closed on purpose: each variant is a rule of the game, so a
/// new kind of effect is a change of the code, while a new item with an
/// existing effect is a change of the item file only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemEffect {
    /// Restores this many hit points to the player, never above its maximum.
    /// From 1 to [`MAX_POWER`].
    Heal(i32),
    /// Restores this many points of essence to the player, never above its
    /// maximum. From 1 to `sigil::MAX_ESSENCE`: more could never be used.
    RestoreEssence(i32),
    /// Teaches this sigil, as reading its engraved glyph would. Written in
    /// the file as the sigil's identifier, which must name a sigil of the
    /// sigil file. The item's `min_depth` is never shallower than the
    /// sigil's: a page must not hand out a sigil before its glyph could be
    /// found, which would bypass the progression the sigil file sets.
    Teach(SigilDef),
    /// Makes this many of the player's next unstable casts pure: the effect
    /// applies without a backlash. From 1 to `sigil::MAX_STEADY_CASTS`, the
    /// most the player can hold: more could never be used.
    Steady(u32),
}

impl ItemEffect {
    /// Returns the family of the items with this effect, which gives their
    /// glyph.
    ///
    /// The family follows from the effect, so the two can never disagree: a
    /// heal or essence draught is a [`ItemFamily::Vial`], a taught sigil a
    /// [`ItemFamily::Page`] of a grimoire, a steadying ink a
    /// [`ItemFamily::Catalyst`].
    pub const fn family(&self) -> ItemFamily {
        match self {
            Self::Heal(_) | Self::RestoreEssence(_) => ItemFamily::Vial,
            Self::Teach(_) => ItemFamily::Page,
            Self::Steady(_) => ItemFamily::Catalyst,
        }
    }
}

/// A family of items, drawn with its own glyph (`theme::glyphs` in the
/// binary).
///
/// Only the families of the shipped effects exist; the weapons and
/// talismans of the design join them with their effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ItemFamily {
    /// A vial to drink: `¡`, or `!` in ASCII.
    Vial,
    /// A page torn from a grimoire: `¶`, or `?` in ASCII.
    Page,
    /// A catalyst for casting: `♦`, or `*` in ASCII.
    Catalyst,
}

/// One kind of item, as described in the item file and validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemDef {
    /// Stable key used by saves.
    id: String,
    /// Name shown to the player.
    name: String,
    /// What using the item does.
    effect: ItemEffect,
    /// Shallowest floor the item can be found on.
    min_depth: u32,
}

impl ItemDef {
    /// Returns the stable identifier: lower-case letters, digits and
    /// underscores, unique among items.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the name shown to the player, in English; at most
    /// `content::MAX_NAME_LENGTH` characters.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns what using the item does.
    pub const fn effect(&self) -> &ItemEffect {
        &self.effect
    }

    /// Returns the family of the item, which gives its glyph (see
    /// [`ItemEffect::family`]).
    pub const fn family(&self) -> ItemFamily {
        self.effect.family()
    }

    /// Returns the shallowest floor the item can be found on; at least
    /// `generation::FIRST_DEPTH`, at most `content::MAX_DEPTH`.
    pub const fn min_depth(&self) -> u32 {
        self.min_depth
    }

    /// Returns whether floor `depth` is at least [`ItemDef::min_depth`]: the
    /// item is never placed shallower. Whether and how often it is placed
    /// there is up to the spawn tables (`Content::item_weights`).
    pub const fn can_appear_at(&self, depth: u32) -> bool {
        depth >= self.min_depth
    }

    /// Returns a definition that names no item of any content file: empty
    /// identifier and name, healing 1 hit point, found from the first floor.
    ///
    /// Only the `save` module uses it, to fill the slots of a slot map for a
    /// moment while it reads or rebuilds the layout of the item keys; an
    /// item of this kind never stays in a game.
    pub(crate) fn stand_in() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            effect: ItemEffect::Heal(1),
            min_depth: crate::generation::FIRST_DEPTH,
        }
    }
}

/// Turns the entries of the item file into definitions, in file order.
///
/// Each entry is checked in turn (identifier form, presence of every field,
/// name length, strength of the effect within its bounds, taught sigil
/// defined in `sigils`, not known from the start and found no deeper than
/// the page, `min_depth` within bounds), then against the entries before it
/// (identifier not reused). An empty list is accepted: floors then hold no
/// item.
///
/// # Errors
///
/// Returns the first fault found as [`ContentError::InvalidField`] naming
/// the entry and the field; a sigil missing from `sigils` is a
/// [`FieldProblem::UnknownReference`] on field `effect`, a sigil known from
/// the start a [`FieldProblem::KnownAtStart`] on field `effect`, a page shallower
/// than its sigil a [`FieldProblem::ShallowerThanReference`] on field
/// `min_depth`.
pub(super) fn validate(
    entries: Vec<ItemEntry>,
    sigils: &[SigilDef],
) -> Result<Vec<ItemDef>, ContentError> {
    let mut items = Vec::with_capacity(entries.len());
    let mut ids = UniqueIds::new(ItemEntry::KIND);
    for (index, entry) in entries.into_iter().enumerate() {
        let position = index + 1;
        let item = validate_entry(position, entry, sigils)?;
        ids.claim(position, &item.id)?;
        items.push(item);
    }
    Ok(items)
}

/// Validates the entry at `position` (counted from 1) on its own, without
/// looking at the other entries.
fn validate_entry(
    position: usize,
    entry: ItemEntry,
    sigils: &[SigilDef],
) -> Result<ItemDef, ContentError> {
    let kind = ItemEntry::KIND;
    let id = EntryCheck::new(&kind, position, None).identifier(entry.id)?;
    let check = EntryCheck::new(&kind, position, Some(&id));
    let name = check.name(entry.name)?;
    let effect = effect(&check, entry.effect, sigils)?;
    let min_depth = check.depth(entry.min_depth, "min_depth")?;
    if let ItemEffect::Teach(sigil) = &effect
        && min_depth < sigil.min_depth()
    {
        return Err(check.fail(
            "min_depth",
            FieldProblem::ShallowerThanReference {
                value: min_depth,
                reference: format!("sigil `{}`", sigil.id()),
                min_depth: sigil.min_depth(),
            },
        ));
    }
    Ok(ItemDef {
        id,
        name,
        effect,
        min_depth,
    })
}

/// Returns the effect described by `entry`, the field `effect` of the entry
/// `check` designates, refusing a missing one, a strength out of its bounds
/// (see [`ItemEffect`]), a taught sigil that `sigils` does not define, or
/// one every run already knows (`SigilDef::known_at_start`), which the page
/// could never teach.
fn effect(
    check: &EntryCheck,
    entry: Option<EffectEntry>,
    sigils: &[SigilDef],
) -> Result<ItemEffect, ContentError> {
    const FIELD: &str = "effect";
    let bounded = |value, max| check.in_range(Some(value), FIELD, 1, max);
    Ok(match check.required(entry, FIELD)? {
        EffectEntry::Heal(amount) => ItemEffect::Heal(bounded(amount, MAX_POWER)?),
        EffectEntry::RestoreEssence(amount) => {
            ItemEffect::RestoreEssence(bounded(amount, MAX_ESSENCE)?)
        }
        EffectEntry::Steady(casts) => {
            let most = i32::try_from(MAX_STEADY_CASTS).unwrap_or(i32::MAX);
            ItemEffect::Steady(bounded(casts, most)?.unsigned_abs())
        }
        EffectEntry::Teach(id) => {
            let Some(sigil) = sigils.iter().find(|sigil| sigil.id() == id) else {
                return Err(check.fail(
                    FIELD,
                    FieldProblem::UnknownReference {
                        noun: "sigil",
                        id,
                        file: SIGILS_FILE,
                    },
                ));
            };
            if sigil.known_at_start() {
                return Err(check.fail(
                    FIELD,
                    FieldProblem::KnownAtStart {
                        reference: format!("sigil `{}`", sigil.id()),
                    },
                ));
            }
            ItemEffect::Teach(sigil.clone())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::{Content, ITEMS_FILE, MAX_NAME_LENGTH};
    use crate::generation::FIRST_DEPTH;

    /// The monster file shipped with the game, valid, for parsing items.
    const MONSTERS: &str = include_str!("../../content/monsters.ron");

    /// The sigil file shipped with the game, valid, for parsing items.
    const SIGILS: &str = include_str!("../../content/sigils.ron");

    /// Fields of a valid test item identified by `id` with `effect`, in file
    /// order.
    fn fields(id: &str, effect: &str) -> Vec<(&'static str, String)> {
        [
            ("id", format!("\"{id}\"")),
            ("name", "\"test item\"".to_owned()),
            ("effect", effect.to_owned()),
            ("min_depth", "1".to_owned()),
        ]
        .into()
    }

    /// Writes one `Item(...)` entry, one field per line.
    fn write_entry(fields: &[(&'static str, String)]) -> String {
        let body: String = fields
            .iter()
            .map(|(name, value)| format!("        {name}: {value},\n"))
            .collect();
        format!("    Item(\n{body}    )")
    }

    /// A valid entry identified by `id` with `effect`.
    fn valid(id: &str, effect: &str) -> String {
        write_entry(&fields(id, effect))
    }

    /// A valid healing entry identified by `id`, except that `field` holds
    /// `value`.
    fn with(id: &str, field: &str, value: &str) -> String {
        let mut fields = fields(id, "Heal(5)");
        for (name, current) in &mut fields {
            if *name == field {
                *current = value.to_owned();
            }
        }
        write_entry(&fields)
    }

    /// An item file listing `entries`.
    fn file(entries: &[String]) -> String {
        format!("[\n{}\n]\n", entries.join(",\n"))
    }

    /// Parses the item file `source` with the shipped monsters and sigils.
    fn parse(source: &str) -> Result<Content, ContentError> {
        Content::parse_all(MONSTERS, SIGILS, source)
    }

    /// Parses `source`, expecting it to be refused, and returns the error and
    /// its message.
    fn refused(source: &str) -> (ContentError, String) {
        let error = parse(source).expect_err("the item file should be refused");
        let message = error.to_string();
        (error, message)
    }

    /// Asserts that `message` names the item file, `entry` and `field`.
    fn assert_names(message: &str, entry: &str, field: &str) {
        assert!(message.contains(ITEMS_FILE), "no file in: {message}");
        assert!(message.contains(entry), "no `{entry}` in: {message}");
        assert!(
            message.contains(&format!("`{field}`")),
            "no field `{field}` in: {message}"
        );
    }

    #[test]
    fn shipped_items_are_valid_and_cover_every_family() {
        let content = Content::load().expect("the shipped content files are valid");
        let items = content.items();

        assert!((3..=6).contains(&items.len()), "{} items", items.len());
        for family in [ItemFamily::Vial, ItemFamily::Page, ItemFamily::Catalyst] {
            assert!(
                items.iter().any(|item| item.family() == family),
                "no {family:?} shipped"
            );
        }
        let effects: Vec<&ItemEffect> = items.iter().map(ItemDef::effect).collect();
        assert!(effects.iter().any(|e| matches!(e, ItemEffect::Heal(_))));
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, ItemEffect::RestoreEssence(_)))
        );
        for item in items {
            assert!(
                item.name().chars().count() <= MAX_NAME_LENGTH,
                "{}",
                item.id()
            );
            if let ItemEffect::Teach(sigil) = item.effect() {
                assert_eq!(content.sigil(sigil.id()), Some(sigil), "{}", item.id());
            }
        }
        assert!(
            content.items_at(FIRST_DEPTH).next().is_some(),
            "no item to find on the first floor"
        );
    }

    #[test]
    fn valid_file_is_read_in_order_with_its_values() {
        let source = file(&[
            valid("draught", "Heal(7)"),
            valid("phial", "RestoreEssence(3)"),
            with("page", "effect", "Teach(\"veil\")").replace("min_depth: 1", "min_depth: 2"),
            valid("ink", "Steady(2)"),
        ]);

        let content = parse(&source).expect("valid items");

        let ids: Vec<&str> = content.items().iter().map(ItemDef::id).collect();
        assert_eq!(ids, ["draught", "phial", "page", "ink"]);
        let draught = content.item("draught").expect("defined");
        assert_eq!(draught.name(), "test item");
        assert_eq!(draught.effect(), &ItemEffect::Heal(7));
        assert_eq!(draught.family(), ItemFamily::Vial);
        assert_eq!(
            content.item("phial").map(ItemDef::effect),
            Some(&ItemEffect::RestoreEssence(3))
        );
        let page = content.item("page").expect("defined");
        let veil = content.sigil("veil").expect("shipped sigil").clone();
        assert_eq!(page.effect(), &ItemEffect::Teach(veil));
        assert_eq!(page.family(), ItemFamily::Page);
        assert_eq!(page.min_depth(), 2);
        assert!(!page.can_appear_at(1) && page.can_appear_at(2));
        let ink = content.item("ink").expect("defined");
        assert_eq!(ink.effect(), &ItemEffect::Steady(2));
        assert_eq!(ink.family(), ItemFamily::Catalyst);
        assert!(content.item("unknown").is_none());

        let first: Vec<&str> = content.items_at(1).map(ItemDef::id).collect();
        assert_eq!(first, ["draught", "phial", "ink"]);
    }

    #[test]
    fn a_page_teaching_a_sigil_known_at_start_is_refused() {
        let source = file(&[valid("ember_page", "Teach(\"ember\")")]);

        let (error, message) = refused(&source);

        assert_names(&message, "item `ember_page` (entry 1)", "effect");
        assert!(message.contains("known_at_start"), "{message}");
        assert_eq!(
            error,
            ContentError::InvalidField {
                file: ITEMS_FILE,
                entry: "item `ember_page` (entry 1)".to_owned(),
                field: "effect",
                problem: FieldProblem::KnownAtStart {
                    reference: "sigil `ember`".to_owned(),
                },
            }
        );
    }

    #[test]
    fn a_page_teaching_an_unknown_sigil_is_refused() {
        let source = file(&[
            valid("draught", "Heal(7)"),
            valid("lost_page", "Teach(\"inferno\")"),
        ]);

        let (error, message) = refused(&source);

        assert_names(&message, "item `lost_page` (entry 2)", "effect");
        assert!(message.contains("`inferno`"), "{message}");
        assert!(message.contains(SIGILS_FILE), "{message}");
        assert_eq!(
            error,
            ContentError::InvalidField {
                file: ITEMS_FILE,
                entry: "item `lost_page` (entry 2)".to_owned(),
                field: "effect",
                problem: FieldProblem::UnknownReference {
                    noun: "sigil",
                    id: "inferno".to_owned(),
                    file: SIGILS_FILE,
                },
            }
        );
    }

    #[test]
    fn the_shipped_items_are_refused_without_the_sigils_they_teach() {
        let lone_sigil = "[Sigil(id: \"spark\", name: \"Spark\", pattern: [7, 5, 3], \
                          effect: Damage, power: 1, targeting: NearestEnemy, min_depth: 1, \
                          known_at_start: false)]";
        let shipped_items = include_str!("../../content/items.ron");

        let error = Content::parse_all(MONSTERS, lone_sigil, shipped_items)
            .expect_err("the shipped pages name shipped sigils");

        assert!(
            matches!(
                error,
                ContentError::InvalidField {
                    file: ITEMS_FILE,
                    field: "effect",
                    problem: FieldProblem::UnknownReference { .. },
                    ..
                }
            ),
            "{error:?}"
        );
    }

    #[test]
    fn strengths_out_of_bounds_are_refused() {
        let cases = [
            "Heal(0)".to_owned(),
            format!("Heal({})", MAX_POWER + 1),
            "RestoreEssence(0)".to_owned(),
            format!("RestoreEssence({})", MAX_ESSENCE + 1),
            "Steady(-1)".to_owned(),
            format!("Steady({})", MAX_STEADY_CASTS + 1),
            format!("Steady({})", MAX_POWER + 1),
        ];
        for effect in cases {
            let (error, message) = refused(&file(&[valid("odd", &effect)]));

            assert_names(&message, "item `odd` (entry 1)", "effect");
            assert!(
                matches!(
                    error,
                    ContentError::InvalidField {
                        problem: FieldProblem::OutOfRange { .. },
                        ..
                    }
                ),
                "{effect}: {error:?}"
            );
        }
        let at_the_bounds = format!("RestoreEssence({MAX_ESSENCE})");
        assert!(parse(&file(&[valid("full", &at_the_bounds)])).is_ok());
        let at_the_bounds = format!("Steady({MAX_STEADY_CASTS})");
        assert!(parse(&file(&[valid("thick_ink", &at_the_bounds)])).is_ok());
    }

    #[test]
    fn a_page_found_shallower_than_the_glyph_of_its_sigil_is_refused() {
        let content = Content::load().expect("the shipped content is valid");
        let lance = content.sigil("lance").expect("a shipped sigil");
        let page = |depth: u32| {
            file(&[with("lance_page", "effect", "Teach(\"lance\")")
                .replace("min_depth: 1", &format!("min_depth: {depth}"))])
        };

        let (error, message) = refused(&page(lance.min_depth() - 1));

        assert_names(&message, "item `lance_page` (entry 1)", "min_depth");
        assert_eq!(
            error,
            ContentError::InvalidField {
                file: ITEMS_FILE,
                entry: "item `lance_page` (entry 1)".to_owned(),
                field: "min_depth",
                problem: FieldProblem::ShallowerThanReference {
                    value: lance.min_depth() - 1,
                    reference: "sigil `lance`".to_owned(),
                    min_depth: lance.min_depth(),
                },
            }
        );
        assert!(parse(&page(lance.min_depth())).is_ok());
        assert!(parse(&page(lance.min_depth() + 1)).is_ok());
        // The shipped pages follow the rule.
        for item in content.items() {
            if let ItemEffect::Teach(sigil) = item.effect() {
                assert!(item.min_depth() >= sigil.min_depth(), "{}", item.id());
            }
        }
    }

    #[test]
    fn duplicate_identifiers_missing_fields_and_long_names_are_refused() {
        let (_, message) = refused(&file(&[
            valid("draught", "Heal(1)"),
            valid("draught", "Heal(2)"),
        ]));
        assert_names(&message, "item `draught` (entry 2)", "id");

        let mut fields = fields("empty", "Heal(1)");
        fields.retain(|(name, _)| *name != "effect");
        let (_, message) = refused(&file(&[write_entry(&fields)]));
        assert_names(&message, "item `empty` (entry 1)", "effect");
        assert!(message.contains("missing"), "{message}");

        let long = format!("\"{}\"", "x".repeat(MAX_NAME_LENGTH + 1));
        let (_, message) = refused(&file(&[with("wordy", "name", &long)]));
        assert_names(&message, "item `wordy` (entry 1)", "name");

        let (_, message) = refused(&file(&[with("sunken", "min_depth", "0")]));
        assert_names(&message, "item `sunken` (entry 1)", "min_depth");
    }

    #[test]
    fn an_unknown_effect_reports_its_line_entry_and_field() {
        let (error, message) = refused(&file(&[valid("odd", "Explode(3)")]));

        assert!(
            matches!(error, ContentError::Syntax { line: 5, .. }),
            "{error:?}"
        );
        assert_names(&message, "item `odd` (entry 1), field", "effect");
    }

    #[test]
    fn an_empty_item_file_is_accepted() {
        let content = parse("[]").expect("floors may hold no item");

        assert!(content.items().is_empty());
        assert!(content.items_at(FIRST_DEPTH).next().is_none());
    }
}
