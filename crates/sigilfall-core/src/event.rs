//! What happened during a turn, reported by `Game::submit`.
//!
//! Exposes [`Event`]. Events are plain data with no text: the binary turns
//! them into message log lines and animations, and end-of-run statistics are
//! computed from them.
//!
//! Every [`ActorId`] in the events of one `Game::submit` call names either an
//! actor still in the game afterwards, or one that died during that call, in
//! which case the [`Event::Died`] of that actor follows in the same list and
//! tells who it was. Keys are generational, so a dead actor's key never names
//! another actor later. The same holds for an [`ItemId`]: an item used up is
//! named by the identifier of its definition in its [`Event::Used`].

use crate::actor::ActorId;
use crate::combat::Combatant;
use crate::geometry::Position;
use crate::item::{ItemId, ItemRefusal};
use crate::sigil::{Backlash, CastQuality};

/// Something that happened in the game, in the order it happened.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Event {
    /// An actor stepped from one cell to a neighbouring one.
    Moved {
        /// Actor that moved.
        actor: ActorId,
        /// Cell it left.
        from: Position,
        /// Cell it now stands on.
        to: Position,
    },
    /// An actor stayed in place and let time pass.
    Waited {
        /// Actor that waited.
        actor: ActorId,
    },
    /// An actor tried to step into a cell it cannot enter (a wall, the edge
    /// of the map or another actor it does not fight), or a step of
    /// automatic exploration found every way closer held by an actor, foe
    /// or not: exploring never strikes. It did not move, spent no energy
    /// and no time passed.
    MoveBlocked {
        /// Actor whose move was refused.
        actor: ActorId,
        /// Cell it stands on, unchanged.
        from: Position,
        /// Cell it tried to enter.
        to: Position,
    },
    /// An actor struck a foe on a neighbouring cell. Blows always land: the
    /// player strikes monsters and monsters strike the player, never each
    /// other.
    Attacked {
        /// Actor that struck.
        attacker: ActorId,
        /// Actor that was struck.
        target: ActorId,
        /// Hit points the blow took off, at least `combat::MIN_DAMAGE`. It
        /// may exceed what the target had left.
        damage: i32,
        /// Hit points the target has left after the blow, from 0 (the blow
        /// killed it, and [`Event::Died`] follows) to its maximum.
        hp_left: i32,
    },
    /// An actor's hit points fell to 0.
    ///
    /// A dead monster has already left the map and the turn order. When the
    /// actor is the player, the game is over (`Game::status`): the player
    /// stays on its cell and nothing else happens.
    Died {
        /// Actor that died; its key no longer finds a monster in the game.
        actor: ActorId,
        /// Who it was, so it can still be named.
        who: Combatant,
        /// Who dealt the killing blow: the cause of death, kept for the
        /// end-of-run summary.
        killed_by: Combatant,
    },
    /// The player cast a trace: the strokes, the validation and the essence
    /// are paid. The events of the effect follow, then, for an unstable
    /// cast, an [`Event::Backlash`]. A cast steadied by an item is reported
    /// pure, and [`Event::CastSteadied`] follows right away.
    SigilCast {
        /// Identifier of the sigil cast (`SigilDef::id`); `None` for a trace
        /// that matched no sigil of the grimoire, which fizzles with no
        /// other event.
        sigil: Option<String>,
        /// How well the trace matched: pure, unstable or fizzled.
        quality: CastQuality,
    },
    /// The player tried to cast without the essence a cast needs. Nothing
    /// happened: no essence, no energy and no time were spent.
    NotEnoughEssence {
        /// Essence the player has.
        essence: i32,
        /// Essence a cast needs (`sigil::ESSENCE_COST`).
        cost: i32,
    },
    /// The player traced a sigil aimed in a direction without giving one.
    /// Nothing happened: no essence, no energy and no time were spent.
    DirectionMissing {
        /// Identifier of the sigil the trace would cast.
        sigil: String,
    },
    /// A spell aimed at a creature found none: no enemy in sight, or none on
    /// the line within `sigil::SPELL_RANGE`. Its cost is still paid.
    SpellMissed {
        /// Identifier of the sigil cast.
        sigil: String,
    },
    /// An actor lost hit points to magic: a damage sigil or a scorching
    /// backlash, not a blow.
    Wounded {
        /// Actor that was hurt.
        actor: ActorId,
        /// Hit points taken off; may exceed what the actor had left.
        damage: i32,
        /// Hit points left afterwards; at 0, [`Event::Died`] follows.
        hp_left: i32,
    },
    /// An actor got hit points back from a sigil or an item.
    Healed {
        /// Actor that was healed.
        actor: ActorId,
        /// Hit points actually restored: 0 when it was unhurt, never more
        /// than what it lacked.
        amount: i32,
        /// Hit points it has afterwards.
        hp: i32,
    },
    /// An actor will lose its next turns, held by a sigil or dazed by a
    /// backlash.
    Held {
        /// Actor that is held.
        actor: ActorId,
        /// Number of its next turns it will lose from now on.
        turns: u32,
    },
    /// A held actor lost one of its turns: it spent the energy of a wait
    /// and did nothing.
    TurnLost {
        /// Actor whose turn was lost.
        actor: ActorId,
    },
    /// An actor was carried along a line by a blink sigil.
    Blinked {
        /// Actor that blinked.
        actor: ActorId,
        /// Cell it left.
        from: Position,
        /// Cell it landed on; equal to `from` when the first cell of the
        /// line was already blocked.
        to: Position,
    },
    /// An unstable cast went wrong after its effect. The event of its
    /// consequence follows: [`Event::Wounded`] for a scorch,
    /// [`Event::EssenceDrained`] for a drain, [`Event::Held`] for a daze.
    Backlash {
        /// Actor that cast the sigil and suffers the backlash.
        caster: ActorId,
        /// What went wrong.
        backlash: Backlash,
    },
    /// An actor's essence was emptied by a draining backlash.
    EssenceDrained {
        /// Actor that lost its essence.
        actor: ActorId,
        /// Essence lost; 0 if none was left.
        lost: i32,
    },
    /// The player read an engraved glyph whose sigil it did not know: the
    /// sigil is now in its grimoire. Reading a known glyph reports nothing.
    GlyphLearned {
        /// Identifier of the sigil learnt.
        sigil: String,
        /// Cell of the glyph.
        position: Position,
    },
    /// The player picked up the item lying on its cell: it is now the last
    /// item of its inventory.
    PickedUp {
        /// Key of the item.
        item: ItemId,
        /// Identifier of the item's definition, to name it.
        definition: String,
    },
    /// The player tried to pick up an item with a full inventory
    /// (`item::INVENTORY_CAPACITY`). The item stays on the floor; no energy
    /// and no time were spent.
    InventoryFull {
        /// Key of the item left on the floor.
        item: ItemId,
        /// Identifier of the item's definition, to name it.
        definition: String,
    },
    /// The player tried to pick up an item where none lies. Nothing
    /// happened: no energy and no time were spent.
    NothingToPickUp {
        /// The player's cell.
        position: Position,
    },
    /// The player used a carried item, which is spent: its key no longer
    /// finds anything. The event of its effect follows: [`Event::Healed`],
    /// [`Event::EssenceRestored`], [`Event::SigilLearned`] or
    /// [`Event::Steadied`].
    Used {
        /// Key the item had.
        item: ItemId,
        /// Identifier of the item's definition, to name it.
        definition: String,
    },
    /// The player laid a carried item on its cell; it left the inventory.
    Dropped {
        /// Key of the item.
        item: ItemId,
        /// Identifier of the item's definition, to name it.
        definition: String,
        /// Cell it now lies on.
        position: Position,
    },
    /// The player tried to use or drop an item and the game refused.
    /// Nothing happened: no energy and no time were spent.
    ItemRefused {
        /// Key given with the action.
        item: ItemId,
        /// Why the action was refused.
        reason: ItemRefusal,
    },
    /// An actor got essence back from an item.
    EssenceRestored {
        /// Actor whose essence was restored.
        actor: ActorId,
        /// Essence actually restored: 0 when it was full, never more than
        /// what it lacked.
        amount: i32,
        /// Essence it has afterwards.
        essence: i32,
    },
    /// The player learnt a sigil from an item, such as a page of a
    /// grimoire: the sigil is now in its grimoire.
    SigilLearned {
        /// Identifier of the sigil learnt.
        sigil: String,
    },
    /// The player's next unstable casts will be cast pure, thanks to a
    /// steadying item.
    Steadied {
        /// Number of next unstable casts that will be cast pure, this item's
        /// included; never more than `sigil::MAX_STEADY_CASTS`, so an item
        /// used at that cap adds nothing.
        casts: u32,
    },
    /// An unstable cast was cast pure instead, using up one steadied cast:
    /// the effect applies and no backlash follows. Comes right after its
    /// [`Event::SigilCast`].
    CastSteadied {
        /// Identifier of the sigil cast.
        sigil: String,
        /// Steadied casts left afterwards.
        casts_left: u32,
    },
    /// The player went down the stairs: the floor it left is gone, and it
    /// stands on the start cell of a new floor with its monsters, glyphs and
    /// items. Every monster and floor item of the floor left is gone with
    /// it; the player keeps its hit points, essence, inventory and grimoire.
    Descended {
        /// Number of the floor the player arrived on, one more than before.
        depth: u32,
    },
    /// The player tried to go down where there are no stairs. Nothing
    /// happened: no energy and no time were spent.
    NoStairsHere {
        /// The player's cell.
        position: Position,
    },
    /// The player asked for a step of automatic exploration and no cell it
    /// has never seen can be reached from where it stands: the floor is
    /// explored as far as the player can walk (see the `explore` module).
    /// Nothing happened: no energy and no time were spent.
    NothingToExplore {
        /// The player's cell.
        position: Position,
    },
}
