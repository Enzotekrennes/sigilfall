//! Items in the game: things lying on the floor or carried by the player.
//!
//! Exposes [`ItemId`], a generational key that never points to an item used
//! up, [`Item`], one item and where it is, [`INVENTORY_CAPACITY`], and
//! [`ItemRefusal`], why the game refused an action on an item. What kinds of
//! items exist is read from the content files (`content::ItemDef`); picking
//! up, using and dropping are done by the game (`Action::PickUp`,
//! `Action::Use`, `Action::Drop`), and placing items on a new floor by the
//! generation.

use crate::content::ItemDef;
use crate::geometry::Position;

slotmap::new_key_type! {
    /// Identifies one item in the game, on the floor or in the inventory.
    ///
    /// Keys are generational: once an item is used up, its old key no longer
    /// finds anything, even if its storage slot is reused.
    pub struct ItemId;
}

/// Most items the player can carry at once.
///
/// Ten: one digit key per slot on the inventory screen, and enough for a
/// few draughts, a page and an ink, so that choosing what to keep matters
/// without turning every floor into a sorting chore.
pub const INVENTORY_CAPACITY: usize = 10;

/// One item: what it is, and whether it lies on the floor or is carried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// What kind of item it is.
    definition: ItemDef,
    /// Floor cell it lies on, or `None` while the player carries it.
    position: Option<Position>,
}

impl Item {
    /// Returns an item of kind `definition` lying on the floor at
    /// `position`.
    pub(crate) const fn on_floor(definition: ItemDef, position: Position) -> Self {
        Self {
            definition,
            position: Some(position),
        }
    }

    /// Returns what kind of item it is: name, effect, family.
    pub const fn definition(&self) -> &ItemDef {
        &self.definition
    }

    /// Returns the floor cell the item lies on, or `None` while the player
    /// carries it.
    pub const fn position(&self) -> Option<Position> {
        self.position
    }

    /// Returns whether the player carries the item.
    pub const fn is_carried(&self) -> bool {
        self.position.is_none()
    }

    /// Moves the item to `position`: `Some` cell to lie on the floor there,
    /// `None` to be carried. Keeping the inventory in step is up to the
    /// caller.
    pub(crate) const fn set_position(&mut self, position: Option<Position>) {
        self.position = position;
    }
}

/// Why the game refused to use or drop an item. Nothing happened: no
/// energy and no time were spent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ItemRefusal {
    /// The player does not carry the item: it lies on the floor, it was
    /// used up, or the key never named an item.
    NotCarried,
    /// The item teaches a sigil the player already knows: using it would
    /// waste it.
    NothingToLearn,
    /// An item already lies on the player's cell, and a cell holds at most
    /// one item.
    CellTaken,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::Content;

    #[test]
    fn an_item_lies_on_the_floor_until_carried() {
        let content = Content::load().expect("the shipped content is valid");
        let draught = content.item("healing_draught").expect("a shipped item");
        let cell = Position::new(3, 4);

        let mut item = Item::on_floor(draught.clone(), cell);
        assert_eq!(item.definition(), draught);
        assert_eq!(item.position(), Some(cell));
        assert!(!item.is_carried());

        item.set_position(None);
        assert_eq!(item.position(), None);
        assert!(item.is_carried());
    }
}
