//! What the current floor holds: its map, the actors, the items and the
//! engraved glyphs.
//!
//! Exposes to the `game` module [`World`], the state the rules read and
//! change, with the queries every rule needs (who stands where, what lies
//! where), the moves of items between the floor and the player's
//! inventory, and the change of floor down the stairs. It applies no rule of the game: time, fights, casting and
//! the effects of items live in `game` and its submodules.

use slotmap::SlotMap;

use crate::actor::{Actor, ActorId};
use crate::generation::Engraving;
use crate::geometry::Position;
use crate::item::{Item, ItemId};
use crate::map::Map;

/// The contents of the current floor and the player's belongings.
///
/// Every item is in `items`, either on a floor cell or carried; a carried
/// item is listed once in `inventory`, a floor item never is, and a cell
/// holds at most one item.
#[derive(Debug, Clone)]
pub(super) struct World {
    /// The current dungeon floor, with what the player sees and remembers.
    pub(super) map: Map,
    /// Every actor, the player included.
    pub(super) actors: SlotMap<ActorId, Actor>,
    /// Key of the player in `actors`.
    pub(super) player: ActorId,
    /// Every item, on the floor or carried by the player.
    items: SlotMap<ItemId, Item>,
    /// Items the player carries, in the order they were picked up.
    inventory: Vec<ItemId>,
    /// Glyphs engraved on the current floor, at most one per cell.
    pub(super) engravings: Vec<Engraving>,
}

impl World {
    /// Returns a floor `map` holding `actors`, among them the player with
    /// key `player`, with no item and no glyph.
    pub(super) fn new(map: Map, actors: SlotMap<ActorId, Actor>, player: ActorId) -> Self {
        Self {
            map,
            actors,
            player,
            items: SlotMap::with_key(),
            inventory: Vec::new(),
            engravings: Vec::new(),
        }
    }

    /// Returns the cell the player stands on.
    pub(super) fn player_position(&self) -> Position {
        self.actors[self.player].position
    }

    /// Returns the key of the actor standing at `position`, if any.
    pub(super) fn actor_at(&self, position: Position) -> Option<ActorId> {
        self.actors
            .iter()
            .find_map(|(id, actor)| (actor.position == position).then_some(id))
    }

    /// Returns the glyph engraved at `position`, if any.
    pub(super) fn engraving_at(&self, position: Position) -> Option<&Engraving> {
        self.engravings
            .iter()
            .find(|engraving| engraving.position() == position)
    }

    /// Returns the item with key `id`, on the floor or carried, or `None` if
    /// it was used up.
    pub(super) fn item(&self, id: ItemId) -> Option<&Item> {
        self.items.get(id)
    }

    /// Returns a world as a save recorded it: the floor `map`, `actors`
    /// with the player's key `player`, `items`, the keys of the carried
    /// items in pick-up order (`inventory`) and the glyphs `engravings`.
    ///
    /// Only the loading of a save uses it; the `saved` module has already
    /// checked the invariants of [`World`] (one item per cell, carried items
    /// listed once in the inventory, the player among the actors).
    pub(super) const fn restored(
        map: Map,
        actors: SlotMap<ActorId, Actor>,
        player: ActorId,
        items: SlotMap<ItemId, Item>,
        inventory: Vec<ItemId>,
        engravings: Vec<Engraving>,
    ) -> Self {
        Self {
            map,
            actors,
            player,
            items,
            inventory,
            engravings,
        }
    }

    /// Returns the slot map of every item, on the floor or carried, as a
    /// save copies it: keys, freed slots and all.
    pub(super) const fn item_slots(&self) -> &SlotMap<ItemId, Item> {
        &self.items
    }

    /// Iterates over every item, on the floor or carried, with its key.
    pub(super) fn items(&self) -> impl Iterator<Item = (ItemId, &Item)> {
        self.items.iter()
    }

    /// Returns the key of the item lying on the floor at `position`, if any.
    pub(super) fn item_at(&self, position: Position) -> Option<ItemId> {
        self.items
            .iter()
            .find_map(|(id, item)| (item.position() == Some(position)).then_some(id))
    }

    /// Returns the items the player carries, in the order they were picked
    /// up.
    pub(super) fn inventory(&self) -> &[ItemId] {
        &self.inventory
    }

    /// Returns whether the player carries the item `id`.
    pub(super) fn carries(&self, id: ItemId) -> bool {
        self.inventory.contains(&id)
    }

    /// Lays `item` on the floor at its cell and returns its new key.
    ///
    /// # Panics
    ///
    /// Panics if `item` is carried, or if its cell is not walkable or
    /// already holds an item: placing it there is a bug in the calling code.
    pub(super) fn place_item(&mut self, item: Item) -> ItemId {
        let position = item
            .position()
            .expect("an item is placed on a floor cell, not in the inventory");
        assert!(
            self.map.is_walkable(position) && self.item_at(position).is_none(),
            "cannot place an item at ({}, {}): the cell is not floor free of items",
            position.x,
            position.y
        );
        self.items.insert(item)
    }

    /// Moves the floor item `id` into the inventory, at its end.
    ///
    /// The caller checks that `id` lies on the floor and that the
    /// inventory has room.
    pub(super) fn carry(&mut self, id: ItemId) {
        self.items[id].set_position(None);
        self.inventory.push(id);
    }

    /// Moves the carried item `id` out of the inventory onto the floor at
    /// `position`; the other items keep their order.
    ///
    /// The caller checks that the player carries `id` and that no item lies
    /// at `position`.
    pub(super) fn put_down(&mut self, id: ItemId, position: Position) {
        self.inventory.retain(|&carried| carried != id);
        self.items[id].set_position(Some(position));
    }

    /// Replaces the current floor by `map`, empty, with the player standing
    /// at `start`, and returns the keys of the monsters left behind.
    ///
    /// Every actor but the player leaves the game, and so do the items lying
    /// on the floor and the engraved glyphs: what the player carries stays
    /// in its inventory, with the same keys. The new floor's monsters, glyphs
    /// and items are the caller's to add, and so is taking the monsters left
    /// behind out of the turn order.
    pub(super) fn change_floor(&mut self, map: Map, start: Position) -> Vec<ActorId> {
        let player = self.player;
        let left_behind: Vec<ActorId> = self.actors.keys().filter(|&id| id != player).collect();
        for &id in &left_behind {
            self.actors.remove(id);
        }
        self.items.retain(|_, item| item.is_carried());
        self.engravings.clear();
        self.map = map;
        self.actors[player].position = start;
        left_behind
    }

    /// Removes the carried item `id` from the game, used up, and returns it;
    /// its key no longer finds anything.
    ///
    /// The caller checks that the player carries `id`.
    pub(super) fn use_up(&mut self, id: ItemId) -> Item {
        self.inventory.retain(|&carried| carried != id);
        self.items
            .remove(id)
            .expect("only an item still in the game is used up")
    }
}
