//! A game turned into the records of a save, and back.
//!
//! Adds to [`Game`] `to_record` and `from_record`, which only the `save`
//! module calls. Writing copies every value of the state, naming
//! definitions by identifier. Reading checks every value before building
//! the game, so a faulty save is refused with a [`SaveError`] instead of
//! panicking or hanging later in play: definitions are looked up in the
//! content, keys must name actors and items of the save, and each value must
//! be one the rules could have produced (actors, items and glyphs on
//! walkable cells of the map, one of each per cell, hit points and essence
//! within their maximums, counters within bounds, the turn order holding
//! every actor once, statistics naming monsters of the content…).
//!
//! The format itself (records, version, RON) belongs to the `save` module.

use std::collections::HashSet;
use std::fmt::Display;
use std::ops::RangeInclusive;

use super::world::World;
use super::{Game, Status};
use crate::actor::{Actor, ActorId, ActorKind, Monster};
use crate::combat::{CombatStats, Combatant};
use crate::content::{
    Content, ItemDef, MAX_ATTACK, MAX_DEFENSE, MAX_HP, MAX_POWER, MAX_SPEED, SigilDef,
};
use crate::generation::{Engraving, FIRST_DEPTH};
use crate::geometry::Position;
use crate::item::{INVENTORY_CAPACITY, Item, ItemId};
use crate::map::Map;
use crate::rng::Seed;
use crate::save::{
    ActorKindRecord, ActorRecord, EngravingRecord, FORMAT_VERSION, GameRecord, ItemRecord,
    MapRecord, Reference, SaveError, SlotKey, capture, rebuild,
};
use crate::schedule::{ACTION_THRESHOLD, Scheduler};
use crate::sigil::{Grimoire, MAX_ESSENCE, MAX_STEADY_CASTS};
use crate::stats::Stats;

/// Largest energy an actor of a save may hold or owe, either way.
///
/// An actor holds at most the action threshold plus one tick of the fastest
/// speed before it acts, and owes at most the dearest action, a nine-stroke
/// cast of 275: ten actions' worth is far beyond both, and keeps the energy
/// arithmetic of the scheduler away from overflowing.
const MAX_ENERGY: i32 = 10 * ACTION_THRESHOLD;

/// Most turns an actor of a save may still lose to a hold: a hold lasts
/// the power of a sigil at most (`Effect::Hold`), and holds do not add up.
const MAX_HELD_TURNS: u32 = MAX_POWER.unsigned_abs();

/// Largest value of the counters of time in a save (ticks, player turns).
///
/// Half the range of `u64`, out of reach of any real game, so a faulty save
/// cannot make a counter overflow as the game goes on.
const MAX_COUNTER: u64 = u64::MAX / 2;

impl Game {
    /// Returns the records of a save of this game, at the current format
    /// version.
    pub(crate) fn to_record(&self) -> GameRecord {
        GameRecord {
            format_version: FORMAT_VERSION,
            seed: self.seed.value(),
            depth: self.depth,
            floors_from_content: self.content.is_some(),
            status: self.status.clone(),
            ticks: self.scheduler.ticks(),
            essence_since: self.essence_since,
            steady_casts: self.steady_casts,
            gameplay_rng: self.gameplay_rng.clone(),
            grimoire: self
                .grimoire
                .sigils()
                .iter()
                .map(|sigil| sigil.id().to_owned())
                .collect(),
            map: MapRecord::of(&self.world.map),
            player: SlotKey::of(self.world.player),
            turn_order: keys_of(self.scheduler.turn_order()),
            actors: capture(&self.world.actors, actor_stand_in, actor_record),
            items: capture(self.world.item_slots(), item_stand_in, item_record),
            inventory: keys_of(self.world.inventory()),
            engravings: self
                .world
                .engravings
                .iter()
                .map(|engraving| EngravingRecord {
                    position: engraving.position(),
                    sigil: engraving.sigil().id().to_owned(),
                })
                .collect(),
            stats: self.stats.clone(),
        }
    }

    /// Builds the game a save's `record` describes, with the definitions of
    /// `content`; `record.floors_from_content` decides whether the game
    /// keeps a copy of `content` for the floors below.
    ///
    /// # Errors
    ///
    /// Returns [`SaveError::UnknownReference`] for a monster, an item or a
    /// sigil `content` lacks, and [`SaveError::Inconsistent`] for any value
    /// the game could not run with (see the module documentation).
    pub(crate) fn from_record(record: GameRecord, content: &Content) -> Result<Self, SaveError> {
        check_counters(&record)?;
        check_stats(&record.stats, record.depth, content)?;
        let map = record.map.to_map()?;
        let grimoire = grimoire_of(&record.grimoire, content)?;
        let engravings = engravings_of(record.engravings, &map, content)?;

        let actors = record
            .actors
            .try_map(|key, actor| actor_of(key, actor, &map, content))?;
        let actors = rebuild::<ActorId, Actor>(actors, "actor", actor_stand_in)?;
        check_one_per_cell(actors.slots.values().map(Actor::position), "actor")?;
        let player = actors.key(record.player).ok_or_else(|| {
            inconsistent(format!(
                "the player is the actor in slot {}, which holds no actor",
                record.player.index
            ))
        })?;
        check_player(&actors.slots, player, &record.status, content)?;
        let turn_order = resolve_keys(&record.turn_order, "turn order", |key| actors.key(key))?;
        if turn_order.len() != actors.slots.len() {
            return Err(inconsistent(format!(
                "the turn order holds {} actors out of {}",
                turn_order.len(),
                actors.slots.len()
            )));
        }

        let items = record
            .items
            .try_map(|key, item| item_of(key, item, &map, content))?;
        let items = rebuild::<ItemId, Item>(items, "item", item_stand_in)?;
        check_one_per_cell(items.slots.values().filter_map(Item::position), "item")?;
        let inventory = resolve_keys(&record.inventory, "inventory", |key| items.key(key))?;
        let carried = items
            .slots
            .values()
            .filter(|item| item.is_carried())
            .count();
        if inventory.len() > INVENTORY_CAPACITY
            || inventory.len() != carried
            || inventory.iter().any(|&id| !items.slots[id].is_carried())
        {
            return Err(inconsistent(format!(
                "the inventory lists {} items, but {carried} items are carried; it must \
                 list each carried item once, {INVENTORY_CAPACITY} at most",
                inventory.len()
            )));
        }

        Ok(Self {
            seed: Seed::new(record.seed),
            depth: record.depth,
            content: record.floors_from_content.then(|| content.clone()),
            world: World::restored(
                map,
                actors.slots,
                player,
                items.slots,
                inventory,
                engravings,
            ),
            scheduler: Scheduler::restored(turn_order, record.ticks),
            stats: record.stats,
            gameplay_rng: record.gameplay_rng,
            status: record.status,
            grimoire,
            steady_casts: record.steady_casts,
            essence_since: record.essence_since,
        })
    }
}

/// Returns the saved form of each key of `ids`, in order.
fn keys_of<K: slotmap::Key>(ids: &[K]) -> Vec<SlotKey> {
    ids.iter().map(|&id| SlotKey::of(id)).collect()
}

/// An actor that fills a slot of the actor map for a moment while its
/// layout is read or rebuilt; it never stays in a game.
fn actor_stand_in() -> Actor {
    Actor::player(Position::new(0, 0))
}

/// An item that fills a slot of the item map for a moment while its layout
/// is read or rebuilt; it never stays in a game.
fn item_stand_in() -> Item {
    Item::on_floor(ItemDef::stand_in(), Position::new(0, 0))
}

/// Returns the saved form of `actor`.
fn actor_record(actor: &Actor) -> ActorRecord {
    ActorRecord {
        position: actor.position(),
        kind: match actor.kind() {
            ActorKind::Player => ActorKindRecord::Player,
            ActorKind::Monster(monster) => ActorKindRecord::Monster {
                definition: monster.definition().to_owned(),
                last_seen_player: monster.last_seen_player(),
            },
        },
        speed: actor.speed(),
        energy: actor.energy(),
        hp: actor.hp(),
        max_hp: actor.max_hp(),
        attack: actor.attack(),
        defense: actor.defense(),
        essence: actor.essence(),
        max_essence: actor.max_essence(),
        held_turns: actor.held_turns(),
    }
}

/// Returns the saved form of `item`.
fn item_record(item: &Item) -> ItemRecord {
    ItemRecord {
        definition: item.definition().id().to_owned(),
        position: item.position(),
    }
}

/// Returns a [`SaveError::Inconsistent`] describing `problem`.
const fn inconsistent(problem: String) -> SaveError {
    SaveError::Inconsistent(problem)
}

/// Checks that `value`, described by `what`, lies within `bounds`.
///
/// # Errors
///
/// Returns [`SaveError::Inconsistent`] naming `what`, the value and the
/// bounds.
fn check_bounds<T: PartialOrd + Display>(
    what: &str,
    value: T,
    bounds: RangeInclusive<T>,
) -> Result<(), SaveError> {
    if bounds.contains(&value) {
        return Ok(());
    }
    Err(inconsistent(format!(
        "{what} is {value}, expected {} to {}",
        bounds.start(),
        bounds.end()
    )))
}

/// Checks that `position`, where `what` stands, is a walkable cell of
/// `map`.
///
/// # Errors
///
/// Returns [`SaveError::Inconsistent`] naming `what`, the cell and the size
/// of the map.
fn check_cell(map: &Map, position: Position, what: &str) -> Result<(), SaveError> {
    if map.is_walkable(position) {
        return Ok(());
    }
    Err(inconsistent(format!(
        "{what} is at ({}, {}), which is not a walkable cell of the {}x{} map",
        position.x,
        position.y,
        map.width(),
        map.height()
    )))
}

/// Checks that no two of `positions`, those of things named `what`, are the
/// same cell.
///
/// # Errors
///
/// Returns [`SaveError::Inconsistent`] naming the first cell held twice.
fn check_one_per_cell(
    positions: impl Iterator<Item = Position>,
    what: &str,
) -> Result<(), SaveError> {
    let mut taken = HashSet::new();
    for position in positions {
        if !taken.insert(position) {
            return Err(inconsistent(format!(
                "two {what}s are at ({}, {})",
                position.x, position.y
            )));
        }
    }
    Ok(())
}

/// Checks the depth and the counters of time of `record`.
///
/// # Errors
///
/// Returns [`SaveError::Inconsistent`] for a depth below `FIRST_DEPTH`,
/// a counter above [`MAX_COUNTER`], an essence recovery counted from a tick
/// to come, or more steadied casts than the player can hold.
fn check_counters(record: &GameRecord) -> Result<(), SaveError> {
    check_bounds("the depth", record.depth, FIRST_DEPTH..=u32::MAX)?;
    check_bounds("the tick count", record.ticks, 0..=MAX_COUNTER)?;
    check_bounds(
        "the tick of the last essence recovery",
        record.essence_since,
        0..=record.ticks,
    )?;
    check_bounds(
        "the count of steadied casts",
        record.steady_casts,
        0..=MAX_STEADY_CASTS,
    )
}

/// Checks the run's statistics `stats` of a game on floor `depth`.
///
/// The other counts (casts, glyphs, items) are not tied to anything else
/// of the state, so any value is accepted.
///
/// # Errors
///
/// Returns [`SaveError::Inconsistent`] for a turn count above
/// [`MAX_COUNTER`], a deepest floor above the current one, or a kind of
/// monster killed 0 times, and [`SaveError::UnknownReference`] for a kind
/// of monster `content` lacks.
fn check_stats(stats: &Stats, depth: u32, content: &Content) -> Result<(), SaveError> {
    check_bounds("the player turn count", stats.turns(), 0..=MAX_COUNTER)?;
    check_bounds(
        "the deepest floor reached",
        stats.deepest(),
        depth..=u32::MAX,
    )?;
    for (definition, &count) in stats.kill_entries() {
        if content.monster(definition).is_none() {
            return Err(SaveError::UnknownReference {
                kind: Reference::Monster,
                id: definition.clone(),
            });
        }
        check_bounds(
            &format!("the count of `{definition}` killed"),
            count,
            1..=u32::MAX,
        )?;
    }
    Ok(())
}

/// Returns the definition of the sigil `id` in `content`.
///
/// # Errors
///
/// Returns [`SaveError::UnknownReference`] if `content` has no such sigil.
fn sigil_of<'content>(
    id: &str,
    content: &'content Content,
) -> Result<&'content SigilDef, SaveError> {
    content
        .sigil(id)
        .ok_or_else(|| SaveError::UnknownReference {
            kind: Reference::Sigil,
            id: id.to_owned(),
        })
}

/// Returns the grimoire knowing the sigils `ids`.
///
/// # Errors
///
/// Returns [`SaveError::UnknownReference`] for a sigil `content` lacks and
/// [`SaveError::Inconsistent`] for a sigil listed twice.
fn grimoire_of(ids: &[String], content: &Content) -> Result<Grimoire, SaveError> {
    let mut grimoire = Grimoire::new();
    for id in ids {
        if !grimoire.learn(sigil_of(id, content)?) {
            return Err(inconsistent(format!(
                "the grimoire lists the sigil `{id}` twice"
            )));
        }
    }
    Ok(grimoire)
}

/// Returns the glyphs `records` describes, engraved on `map`.
///
/// # Errors
///
/// Returns [`SaveError::UnknownReference`] for a sigil `content` lacks and
/// [`SaveError::Inconsistent`] for a glyph off the walkable cells or two
/// glyphs on one cell.
fn engravings_of(
    records: Vec<EngravingRecord>,
    map: &Map,
    content: &Content,
) -> Result<Vec<Engraving>, SaveError> {
    let engravings = records
        .into_iter()
        .map(|record| {
            check_cell(map, record.position, "a glyph")?;
            let sigil = sigil_of(&record.sigil, content)?;
            Ok(Engraving::new(record.position, sigil.clone()))
        })
        .collect::<Result<Vec<_>, SaveError>>()?;
    check_one_per_cell(engravings.iter().map(Engraving::position), "glyph")?;
    Ok(engravings)
}

/// Returns the actor `record` describes, saved in slot `key` of the actor
/// map, standing on `map`.
///
/// # Errors
///
/// Returns [`SaveError::UnknownReference`] for a monster `content` lacks,
/// and [`SaveError::Inconsistent`] for an actor off the walkable cells, a
/// remembered cell of the player off them, a value out of its bounds (speed,
/// fighting values and hit points as for a creature of the content, energy
/// within [`MAX_ENERGY`], held turns within [`MAX_HELD_TURNS`], essence
/// within its maximum) or a monster without hit points.
fn actor_of(
    key: SlotKey,
    record: ActorRecord,
    map: &Map,
    content: &Content,
) -> Result<Actor, SaveError> {
    let what = |field: &str| format!("the {field} of the actor in slot {}", key.index);
    check_cell(
        map,
        record.position,
        &format!("the actor in slot {}", key.index),
    )?;
    check_bounds(&what("speed"), record.speed, 1..=MAX_SPEED)?;
    check_bounds(&what("energy"), record.energy, -MAX_ENERGY..=MAX_ENERGY)?;
    check_bounds(&what("hit point maximum"), record.max_hp, 1..=MAX_HP)?;
    check_bounds(&what("hit points"), record.hp, 0..=record.max_hp)?;
    check_bounds(&what("attack"), record.attack, 0..=MAX_ATTACK)?;
    check_bounds(&what("defense"), record.defense, 0..=MAX_DEFENSE)?;
    check_bounds(
        &what("essence maximum"),
        record.max_essence,
        0..=MAX_ESSENCE,
    )?;
    check_bounds(&what("essence"), record.essence, 0..=record.max_essence)?;
    check_bounds(&what("held turns"), record.held_turns, 0..=MAX_HELD_TURNS)?;

    let kind = match record.kind {
        ActorKindRecord::Player => ActorKind::Player,
        ActorKindRecord::Monster {
            definition,
            last_seen_player,
        } => {
            let Some(found) = content.monster(&definition) else {
                return Err(SaveError::UnknownReference {
                    kind: Reference::Monster,
                    id: definition,
                });
            };
            // Monsters leave the map when they die: one still there is alive.
            check_bounds(&what("hit points"), record.hp, 1..=record.max_hp)?;
            if let Some(cell) = last_seen_player {
                check_cell(map, cell, &what("memory of the player"))?;
            }
            let mut monster = Monster::new(found);
            monster.last_seen_player = last_seen_player;
            ActorKind::Monster(monster)
        }
    };
    let stats = CombatStats {
        max_hp: record.max_hp,
        attack: record.attack,
        defense: record.defense,
    };
    Ok(
        Actor::new(record.position, record.speed, stats, kind).with_saved_state(
            record.hp,
            record.energy,
            record.essence,
            record.max_essence,
            record.held_turns,
        ),
    )
}

/// Checks that `player` is the only player among `actors`, alive exactly
/// while `status` says the game goes on, and that the killer a finished
/// game names is a monster of `content`.
///
/// # Errors
///
/// Returns [`SaveError::Inconsistent`] for a player key naming a monster,
/// another actor being a player, or a player whose hit points disagree with
/// `status`, and [`SaveError::UnknownReference`] for an unknown killer.
fn check_player(
    actors: &slotmap::SlotMap<ActorId, Actor>,
    player: ActorId,
    status: &Status,
    content: &Content,
) -> Result<(), SaveError> {
    let players = actors.values().filter(|actor| actor.is_player()).count();
    if !actors[player].is_player() || players != 1 {
        return Err(inconsistent(format!(
            "the game must have exactly one player, the actor its player key names; \
             it has {players}"
        )));
    }
    let alive = actors[player].is_alive();
    match status {
        Status::Playing if !alive => Err(inconsistent(
            "the game goes on but the player has no hit points left".to_owned(),
        )),
        Status::GameOver { .. } if alive => Err(inconsistent(
            "the game is over but the player still has hit points".to_owned(),
        )),
        Status::GameOver {
            killed_by: Combatant::Monster { definition },
        } if content.monster(definition).is_none() => Err(SaveError::UnknownReference {
            kind: Reference::Monster,
            id: definition.clone(),
        }),
        Status::Playing | Status::GameOver { .. } => Ok(()),
    }
}

/// Returns the keys `saved` names, in order, found by `find`; `what` names
/// the list in error messages.
///
/// # Errors
///
/// Returns [`SaveError::Inconsistent`] for a key naming nothing or a key
/// listed twice.
fn resolve_keys<K: slotmap::Key>(
    saved: &[SlotKey],
    what: &str,
    find: impl Fn(SlotKey) -> Option<K>,
) -> Result<Vec<K>, SaveError> {
    let mut keys = Vec::with_capacity(saved.len());
    for &key in saved {
        let found = find(key).ok_or_else(|| {
            inconsistent(format!(
                "the {what} names slot {} at version {}, which holds nothing",
                key.index, key.version
            ))
        })?;
        if keys.contains(&found) {
            return Err(inconsistent(format!(
                "the {what} lists slot {} twice",
                key.index
            )));
        }
        keys.push(found);
    }
    Ok(keys)
}

/// Returns the item `record` describes, saved in slot `key` of the item map.
///
/// # Errors
///
/// Returns [`SaveError::UnknownReference`] for an item `content` lacks, and
/// [`SaveError::Inconsistent`] for an item lying off the walkable cells of
/// `map`.
fn item_of(
    key: SlotKey,
    record: ItemRecord,
    map: &Map,
    content: &Content,
) -> Result<Item, SaveError> {
    let Some(definition) = content.item(&record.definition) else {
        return Err(SaveError::UnknownReference {
            kind: Reference::Item,
            id: record.definition,
        });
    };
    let mut item = Item::on_floor(definition.clone(), Position::new(0, 0));
    if let Some(position) = record.position {
        check_cell(map, position, &format!("the item in slot {}", key.index))?;
    }
    item.set_position(record.position);
    Ok(item)
}
