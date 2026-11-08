//! A game in progress and its single entry point, [`Game::submit`].
//!
//! Exposes [`Game`], [`Status`], [`VisibleMonster`] and [`VisibleItem`]; with
//! the `test-support` feature, also `PLAYER_START` and the helpers that build
//! hand-made rooms for tests (`Game::in_fixed_room`).
//!
//! The game owns the run's seed (and the date of a daily run, see
//! [`Game::daily`]), the depth of the current floor, the world (map, actors,
//! items, glyphs and the player's inventory, in the `world` submodule), the
//! scheduler and the gameplay random stream. A new game starts on the first
//! generated floor of its seed, with its monsters, glyphs and items (see the
//! `generation` module). The player's action is applied, then the other
//! actors take their turns until the player may act again, and everything
//! that happened is returned as [`Event`]s. The game produces no text and
//! draws nothing.
//!
//! The game also keeps the player's field of view up to date: it is computed
//! when the game starts and again after each action of the player, and the
//! cells seen are remembered on the map. Monsters choose their actions with
//! the `ai` module.
//!
//! Fights are bump attacks: an actor that moves into a foe strikes it
//! instead of stepping, for [`ATTACK_COST`] energy. The player and the
//! monsters are foes; monsters never strike each other. Damage follows the
//! `combat` module, drawn from the run's `Purpose::Gameplay` stream. A
//! monster whose hit points fall to 0 leaves the map and the turn order; when
//! the player's do, the game is over ([`Status::GameOver`]) and
//! [`Game::submit`] no longer does anything.
//!
//! Standing on the stairs, the player goes down with `Action::Descend` to the
//! next floor of the seed, keeping its hit points, essence, inventory and
//! grimoire; the rules live in the `descent` submodule. The bottom floor has
//! a seal instead of stairs: the same action taken on it wins the run
//! ([`Status::Won`]), and [`Game::submit`] no longer does anything either.
//! Every floor has its own gameplay stream (decision D5), so a floor plays
//! out the same for a given seed and the same actions there, whatever
//! happened above.
//!
//! Sigils are cast with `Action::Cast`, whose rules (cost per stroke,
//! essence, effects, backlash) live in the `cast` submodule. The player
//! learns sigils by stepping onto the glyphs engraved on the floor: reading
//! is free and automatic, so exploring is enough to discover them. The
//! sigils learnt are kept in the [`Grimoire`], and only those can be cast.
//! Essence comes back slowly with time (see [`Game::submit`]). An actor held
//! by a sigil or a backlash loses its next turns: when its turn comes, it
//! spends the energy of a wait and does nothing.
//!
//! `Action::Explore` takes one step towards the nearest cell the player has
//! never seen; its rules live in the `explore` submodule, the choice of the
//! way in the `explore` module.
//!
//! Items lie on the floor until the player picks them up, then fill its
//! inventory until it uses or drops them; their rules live in the
//! `inventory` submodule. Only the player handles items.
//!
//! The game counts what the player does for the end-of-run summary
//! ([`Game::stats`]), from the events of each [`Game::submit`].
//!
//! A game is written to and read back from a save by the `save` module; the
//! `saved` submodule turns it into the save's records and back, checking
//! every value read.

mod cast;
mod descent;
mod explore;
mod inventory;
mod saved;
mod world;

use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};
use slotmap::SlotMap;

use crate::action::{self, Action, ActionOutcome};
use crate::actor::{Actor, ActorId, ActorKind};
use crate::ai;
use crate::combat::{self, Combatant};
use crate::content::{Content, Family, ItemFamily};
#[cfg(any(test, feature = "test-support"))]
use crate::content::{ItemDef, MonsterDef, SigilDef};
use crate::event::Event;
use crate::fov::{self, SIGHT_RANGE};
use crate::generation::{self, Engraving, FIRST_DEPTH, Floor, ItemSpawn, MonsterSpawn};
use crate::geometry::{Direction, Position};
use crate::item::{Item, ItemId};
use crate::map::Map;
use crate::rng::{DailyDate, Purpose, Seed};
use crate::schedule::{ATTACK_COST, Scheduler, WAIT_COST, action_cost, spend_energy};
use crate::sigil::{ESSENCE_REGEN_TICKS, Grimoire};
use crate::stats::Stats;
use world::World;

/// Cell where the player starts in the fixed room of
/// [`Game::in_fixed_room`].
///
/// A floor cell near the left wall whose eight neighbours are all floor, so
/// every direction can be tried on the first turn, and away from the pillar
/// at `map::FIXED_ROOM_PILLAR`.
#[cfg(any(test, feature = "test-support"))]
pub const PLAYER_START: Position = Position::new(5, 5);

/// A game in progress: the floor with its actors and items, and the flow of
/// time.
///
/// Between two calls to [`Game::submit`], it is always the player's turn,
/// unless the game is over.
#[derive(Debug, Clone)]
pub struct Game {
    /// The seed every random choice of this run follows from.
    seed: Seed,
    /// The date whose daily dungeon this run is, or `None` for a run of a
    /// seed chosen or drawn at random. When set, `seed` is always
    /// `Seed::daily` of it.
    daily: Option<DailyDate>,
    /// Number of the current floor: [`FIRST_DEPTH`] at the start, one more
    /// for each staircase taken.
    depth: u32,
    /// The game data the floors below are generated from, or `None` for a
    /// game started without it ([`Game::on_floor`], `Game::in_fixed_room`),
    /// whose floors below are bare layouts.
    content: Option<Content>,
    /// The current floor: map, actors, items, glyphs, and the player's
    /// inventory.
    world: World,
    /// Turn order and elapsed time.
    scheduler: Scheduler,
    /// What the player did so far, its turns included, for the end-of-run
    /// summary.
    stats: Stats,
    /// Stream of every random choice made while playing on the current
    /// floor, such as the damage of each blow or the backlash of an unstable
    /// cast: `seed.stream(depth, Purpose::Gameplay)`, started afresh on each
    /// floor.
    gameplay_rng: ChaCha8Rng,
    /// Whether the player is still alive.
    status: Status,
    /// The sigils the player has learnt, the only ones it can cast.
    grimoire: Grimoire,
    /// Number of the player's next unstable casts that will be cast pure,
    /// given by steadying items (`ItemEffect::Steady`).
    steady_casts: u32,
    /// Tick from which the player's essence recovery is counted: the last
    /// time it gained a point, or reached its maximum.
    essence_since: u64,
}

/// Whether a game can still be played, and how it ended if not.
///
/// A save keeps it as `Playing`, `Won` or as
/// `GameOver(killed_by: Monster(definition: "ossuary_rat"))`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Status {
    /// The player is alive: [`Game::submit`] takes its next action.
    Playing,
    /// The player died; the game is finished and [`Game::submit`] does
    /// nothing any more.
    GameOver {
        /// Who dealt the killing blow, for the end-of-run summary.
        /// [`Combatant::Player`] means the player died by its own hand,
        /// such as from the backlash of its own spell; no rule kills the
        /// player that way today (a scorching backlash always spares the
        /// last hit point), but saves and the summary accept it.
        killed_by: Combatant,
    },
    /// The player broke the seal of the bottom floor and won the run
    /// ([`Event::SealBroken`]); the game is finished and [`Game::submit`]
    /// does nothing any more. The player is alive.
    Won,
}

/// An item lying on a cell the player sees right now, as the binary needs it
/// to draw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibleItem<'game> {
    /// Key of the item.
    pub id: ItemId,
    /// Cell the item lies on; always visible to the player.
    pub position: Position,
    /// Identifier of the item's definition in the content files.
    pub definition: &'game str,
    /// Family of the item, which gives its glyph.
    pub family: ItemFamily,
}

/// A monster the player sees right now, as the binary needs it to draw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibleMonster<'game> {
    /// Key of the monster's actor.
    pub id: ActorId,
    /// Cell the monster stands on; always visible to the player.
    pub position: Position,
    /// Identifier of the monster's definition in the content files.
    pub definition: &'game str,
    /// Family of the monster, which gives its letter.
    pub family: Family,
    /// Whether the monster is the elite form of its family (upper-case
    /// letter).
    pub elite: bool,
}

impl Game {
    /// Starts a game of the run identified by `seed`, on its first floor
    /// (`generation::generate_with_monsters(seed, FIRST_DEPTH, content)`),
    /// with the player on that floor's start cell, moving at normal speed
    /// with the starting values of `combat::PLAYER_STATS`, and the floor's
    /// monsters.
    ///
    /// `content` is the validated game data, normally `Content::load()`; the
    /// monsters are drawn from its spawn table for the first floor. The
    /// player is created first, so it acts before the monsters on ties, then
    /// the monsters in the order they were drawn, each with the starting
    /// energy chosen by the generation (`MonsterSpawn::energy`).
    ///
    /// The floor's engraved glyphs and items are those of the generation.
    /// The player starts with an empty grimoire, an empty inventory and a
    /// full reserve of essence.
    ///
    /// The same seed and content always give the same floor, monsters,
    /// glyphs and items. Time is advanced until the player's first turn, so
    /// [`Game::submit`] can be called right away; no monster has acted yet.
    ///
    /// The game keeps a copy of `content` to generate the floors below,
    /// each with `generation::generate_with_monsters` at its depth.
    pub fn new(seed: Seed, content: &Content) -> Self {
        let mut game = Self::on_floor(
            seed,
            generation::generate_with_monsters(seed, FIRST_DEPTH, content),
        );
        game.content = Some(content.clone());
        game
    }

    /// Starts the daily dungeon of `date`: [`Game::new`] on the seed
    /// `Seed::daily(date)`, remembered as the daily run of that date
    /// ([`Game::daily_date`]) so the screens and the save can tell it from
    /// a run of a chosen seed.
    ///
    /// Every player who starts the daily dungeon of the same date gets the
    /// same floors, monsters, glyphs and items.
    pub fn daily(date: DailyDate, content: &Content) -> Self {
        let mut game = Self::new(Seed::daily(date), content);
        game.daily = Some(date);
        game
    }

    /// Starts a game of the run identified by `seed` on `floor`, with the
    /// player on its start cell, one monster per entry of `Floor::monsters`,
    /// created in that order with their starting energy, the glyphs of
    /// `Floor::glyphs` and the items of `Floor::items`.
    ///
    /// [`Game::new`] is this function on the first floor of the seed with
    /// its monsters. Given a floor from `generation::generate`, which places
    /// neither monster, glyph nor item, the player is alone: tests of
    /// movement, sight or the camera use that to walk a generated floor
    /// without a monster in the way. Time is advanced until the player's
    /// first turn.
    ///
    /// The floor counts as the first ([`Game::depth`] is [`FIRST_DEPTH`]),
    /// and the game has no content: the floors below its stairs are bare
    /// layouts of the seed (`generation::generate`), with neither monster,
    /// glyph nor item.
    pub fn on_floor(seed: Seed, floor: Floor) -> Self {
        let mut game = Self::start(seed, floor.map, floor.start, &floor.monsters);
        game.lay_out_floor_contents(floor.glyphs, floor.items);
        game
    }

    /// Starts a game of the run identified by `seed` in the hand-made room of
    /// [`Map::fixed_room`] instead of a generated floor, with the player at
    /// [`PLAYER_START`] and no other actor.
    ///
    /// The room never changes, whatever the seed, which makes it the ground
    /// of tests that need known walls and distances. The seed is only kept
    /// (see [`Game::seed`]). Monsters can be added with
    /// [`Game::spawn_monster`], glyphs with [`Game::engrave`], items with
    /// [`Game::place_item`]. The room has no stairs, so the player never
    /// leaves it.
    #[cfg(any(test, feature = "test-support"))]
    pub fn in_fixed_room(seed: Seed) -> Self {
        Self::start(seed, Map::fixed_room(), PLAYER_START, &[])
    }

    /// Starts a game on `map` with the player at `start` (`Actor::player`),
    /// and one monster per entry of `monsters`, computes what the player
    /// sees from there and runs the clock to the player's first turn.
    ///
    /// `start` and the monster cells are expected to be distinct walkable
    /// cells; the callers guarantee it.
    fn start(seed: Seed, map: Map, start: Position, monsters: &[MonsterSpawn]) -> Self {
        let mut actors = SlotMap::with_key();
        let player = actors.insert(Actor::player(start));
        let mut scheduler = Scheduler::default();
        scheduler.enrol(player);

        let mut game = Self {
            seed,
            daily: None,
            depth: FIRST_DEPTH,
            content: None,
            world: World::new(map, actors, player),
            scheduler,
            stats: Stats::default(),
            gameplay_rng: seed.stream(FIRST_DEPTH, Purpose::Gameplay),
            status: Status::Playing,
            grimoire: Grimoire::new(),
            steady_casts: 0,
            essence_since: 0,
        };
        game.add_floor_monsters(monsters);
        game.update_field_of_view();
        // The player is first in turn order and every monster starts below
        // the action threshold, so the player is the first actor ready and
        // no event can be produced here.
        game.run_until_player_turn(&mut Vec::new());
        game
    }

    /// Creates one monster per entry of `monsters`, in that order, with its
    /// starting energy, and puts each at the end of the turn order.
    fn add_floor_monsters(&mut self, monsters: &[MonsterSpawn]) {
        for spawn in monsters {
            let monster = self.world.actors.insert(Actor::monster(
                spawn.position(),
                spawn.definition(),
                spawn.energy(),
            ));
            self.scheduler.enrol(monster);
        }
    }

    /// Engraves `glyphs` and lays `items` on the current floor, as a new
    /// floor's generation placed them.
    fn lay_out_floor_contents(&mut self, glyphs: Vec<Engraving>, items: Vec<ItemSpawn>) {
        self.world.engravings = glyphs;
        for spawn in items {
            self.world
                .place_item(Item::on_floor(spawn.definition, spawn.position));
        }
    }

    /// Places a new monster of kind `definition` at `position` and returns
    /// its key.
    ///
    /// Unlike the monsters of a generated floor, it starts with no energy.
    /// It takes turns after every actor created before it when they are
    /// ready at the same time, the player included, so it never acts during
    /// the player's next action: after that action the player and a monster
    /// at least as fast both reach the action threshold on the same tick, and
    /// the player plays first. Such a monster takes its first turn during the
    /// player's second action; a slower one later. It has no memory of the
    /// player. It is still the player's turn afterwards.
    ///
    /// # Panics
    ///
    /// Panics if `position` is not walkable or already holds an actor:
    /// placing a monster there is a bug in the calling code.
    #[cfg(any(test, feature = "test-support"))]
    pub fn spawn_monster(&mut self, position: Position, definition: &MonsterDef) -> ActorId {
        assert!(
            self.world.map.is_walkable(position) && self.world.actor_at(position).is_none(),
            "cannot spawn an actor at ({}, {}): the cell is not free floor",
            position.x,
            position.y
        );
        let id = self
            .world
            .actors
            .insert(Actor::monster(position, definition, 0));
        self.scheduler.enrol(id);
        id
    }

    /// Engraves a glyph teaching `sigil` at `position`, as the generation
    /// does on a new floor.
    ///
    /// The player reads it by stepping onto the cell (see [`Game::submit`]);
    /// a glyph under the player is only read once the player steps onto it
    /// again. Hand-made rooms and tests use it; nothing else changes and it
    /// is still the player's turn.
    ///
    /// # Panics
    ///
    /// Panics if `position` is not walkable or already holds a glyph:
    /// engraving there is a bug in the calling code.
    #[cfg(any(test, feature = "test-support"))]
    pub fn engrave(&mut self, position: Position, sigil: &SigilDef) {
        assert!(
            self.world.map.is_walkable(position) && self.world.engraving_at(position).is_none(),
            "cannot engrave a glyph at ({}, {}): the cell is not floor free of glyphs",
            position.x,
            position.y
        );
        self.world
            .engravings
            .push(Engraving::new(position, sigil.clone()));
    }

    /// Lays an item of kind `definition` on the floor at `position`, as the
    /// generation does on a new floor, and returns its key.
    ///
    /// The player picks it up with [`Action::PickUp`] while standing on the
    /// cell. Hand-made rooms and tests use it; nothing else changes and it
    /// is still the player's turn.
    ///
    /// # Panics
    ///
    /// Panics if `position` is not walkable or already holds an item: a cell
    /// holds at most one item, so placing one there is a bug in the calling
    /// code.
    #[cfg(any(test, feature = "test-support"))]
    pub fn place_item(&mut self, position: Position, definition: &ItemDef) -> ItemId {
        self.world
            .place_item(Item::on_floor(definition.clone(), position))
    }

    /// Applies the player's `action`, then lets the other actors act until
    /// it is the player's turn again, and returns what happened in order.
    ///
    /// A move into a monster is a blow ([`Event::Attacked`], followed by
    /// [`Event::Died`] if it kills). A move into a wall or the edge of the
    /// map is refused: the only event is [`Event::MoveBlocked`], the player
    /// spends no energy, no time passes and no other actor acts. Otherwise
    /// the first events are the player's own, followed by those of every
    /// actor that acted before the player's next turn, and
    /// [`Game::player_turns`] grows by one.
    ///
    /// A cast ([`Action::Cast`]) costs `schedule::STROKE_COST` energy per
    /// node of its trace plus `schedule::CAST_COST`, all at once, and
    /// `sigil::ESSENCE_COST` essence: the longer the pattern, the more the
    /// monsters act before the player's next turn. The trace is recognised
    /// against the sigils of the [`Game::grimoire`] only. It is refused, with
    /// no cost and no time passing, when the player lacks essence
    /// ([`Event::NotEnoughEssence`]) or when the sigil it casts is aimed in a
    /// direction and none was given ([`Event::DirectionMissing`]). Otherwise
    /// [`Event::SigilCast`] comes first, then the events of the effect and of
    /// any backlash (see the `cast` submodule).
    ///
    /// A move that ends on an engraved glyph reads it: if its sigil is not
    /// known yet, it joins the grimoire and [`Event::GlyphLearned`] follows
    /// the [`Event::Moved`]. Reading takes no extra time.
    ///
    /// Going down ([`Action::Descend`]) costs `schedule::DESCEND_COST` and
    /// moves the player to the next floor (see the `descent` submodule):
    /// [`Event::Descended`] comes first, then the events of the monsters of
    /// the new floor, which can never act before the player's first turn
    /// there. On the seal of the bottom floor, the same action and cost
    /// break the seal: [`Event::SealBroken`] is the only event, no other
    /// actor acts and the game is won ([`Status::Won`]). Anywhere but on
    /// the stairs or the seal it is refused with no cost and no time passing
    /// ([`Event::NoStairsHere`]).
    ///
    /// A step of automatic exploration ([`Action::Explore`]) is a move
    /// towards the nearest cell the player has never seen, chosen on what
    /// it remembers of the floor (see the `explore` submodule): same cost
    /// and events as [`Action::Move`]. It never strikes; when every way
    /// closer is held by an actor it is refused like a move into a wall
    /// ([`Event::MoveBlocked`]), and when nothing is left to explore within
    /// reach with [`Event::NothingToExplore`], with no cost and no time
    /// passing either way.
    ///
    /// Items are picked up ([`Action::PickUp`]), used ([`Action::Use`]) and
    /// dropped ([`Action::Drop`]) for their own costs
    /// (`schedule::PICK_UP_COST`, `schedule::USE_COST`,
    /// `schedule::DROP_COST`); see the `inventory` submodule for their rules.
    /// Picking up from an empty cell or into a full inventory, using or
    /// dropping an item the player does not carry, using a page that teaches
    /// nothing new, and dropping onto a cell that already holds an item are
    /// refused with no cost and no time passing, like a move into a wall.
    ///
    /// After an action that took time, the field of view is computed again
    /// from the player's position (see [`Game::is_visible`]) before the other
    /// actors act. An actor held by a sigil loses its turns
    /// ([`Event::TurnLost`]) until its hold runs out, the player included.
    /// Before the player's next turn, its essence recovers one point per
    /// `sigil::ESSENCE_REGEN_TICKS` ticks spent below its maximum.
    ///
    /// If the player dies, the list ends with its [`Event::Died`], no other
    /// actor acts and the game is over ([`Game::status`]). Once the game is
    /// over, lost or won, this function changes nothing and returns no
    /// event.
    ///
    /// The events returned are counted in [`Game::stats`] before they are
    /// returned.
    pub fn submit(&mut self, action: Action) -> Vec<Event> {
        let mut events = Vec::new();
        if self.is_over() {
            return events;
        }
        if self.perform(self.world.player, action, &mut events) {
            self.stats.count_turn();
            self.update_field_of_view();
            self.run_until_player_turn(&mut events);
        }
        self.stats.record(&events);
        events
    }

    /// Returns whether the game can still be played and, if not, how it
    /// ended: who killed the player, or the seal broken.
    pub const fn status(&self) -> &Status {
        &self.status
    }

    /// Returns whether the game has ended, by the player's death or by its
    /// victory ([`Status::Won`]): no action is taken any more.
    pub const fn is_over(&self) -> bool {
        matches!(self.status, Status::GameOver { .. } | Status::Won)
    }

    /// Returns the seed of this run, as given to [`Game::new`] or
    /// `Game::in_fixed_room` (with the `test-support` feature).
    pub const fn seed(&self) -> Seed {
        self.seed
    }

    /// Returns the date whose daily dungeon this run is (a game started
    /// with [`Game::daily`]), or `None` for a run of a chosen or random
    /// seed. A saved daily run keeps its date.
    pub const fn daily_date(&self) -> Option<DailyDate> {
        self.daily
    }

    /// Returns the number of the floor the player is on: [`FIRST_DEPTH`] in
    /// a new game, one more after each descent ([`Action::Descend`]).
    pub const fn depth(&self) -> u32 {
        self.depth
    }

    /// Returns the current dungeon floor.
    pub const fn map(&self) -> &Map {
        &self.world.map
    }

    /// Returns the key of the player.
    pub const fn player(&self) -> ActorId {
        self.world.player
    }

    /// Returns the player's actor.
    ///
    /// The player is never removed: after its death, it stays on its cell
    /// with 0 hit points, so the final screen can still show it.
    pub fn player_actor(&self) -> &Actor {
        &self.world.actors[self.world.player]
    }

    /// Returns the actor with key `id`, or `None` if it does not exist, such
    /// as a monster that died.
    pub fn actor(&self, id: ActorId) -> Option<&Actor> {
        self.world.actors.get(id)
    }

    /// Iterates over every actor, the player included, with its key,
    /// whether the player sees it or not.
    ///
    /// The order is stable between calls but is not the turn order. To draw
    /// the map, use [`Game::visible_monsters`], which hides what the player
    /// cannot see.
    pub fn actors(&self) -> impl Iterator<Item = (ActorId, &Actor)> {
        self.world.actors.iter()
    }

    /// Iterates over the monsters standing on a cell the player sees right
    /// now (see [`Game::is_visible`]), with what is needed to draw them.
    ///
    /// A monster out of the player's field of view is never listed, even on
    /// a remembered cell. The order is stable between calls, like
    /// [`Game::actors`].
    pub fn visible_monsters(&self) -> impl Iterator<Item = VisibleMonster<'_>> {
        self.world.actors.iter().filter_map(|(id, actor)| {
            let monster = actor.as_monster()?;
            self.world
                .map
                .is_visible(actor.position)
                .then(|| VisibleMonster {
                    id,
                    position: actor.position,
                    definition: monster.definition(),
                    family: monster.family(),
                    elite: monster.elite(),
                })
        })
    }

    /// Returns the number of ticks elapsed since the game started.
    ///
    /// Ticks measure the flow of time for every actor; they are not the
    /// player's turns. A new game already shows one tick, the one that gave
    /// the player the energy for its first turn, and the gap between ticks
    /// and player turns depends on the player's speed and action costs. Use
    /// [`Game::player_turns`] to count what the player did.
    pub const fn ticks(&self) -> u64 {
        self.scheduler.ticks()
    }

    /// Returns the number of actions the player has performed that took
    /// time: 0 in a new game, one more per successful [`Game::submit`].
    ///
    /// A refused action (a move into a wall or another actor) does not
    /// count, and the turns of other actors, whatever their speed, never
    /// change it.
    pub const fn player_turns(&self) -> u64 {
        self.stats.turns()
    }

    /// Returns what the player did so far, for the end-of-run summary:
    /// creatures killed, sigils cast, glyphs learnt, items used, deepest
    /// floor and turns (see [`Stats`]).
    ///
    /// The counts follow the events [`Game::submit`] returned since the game
    /// started, and survive a save; they stay as they were once the game is
    /// over.
    pub const fn stats(&self) -> &Stats {
        &self.stats
    }

    /// Returns whether the player sees `position` right now.
    ///
    /// The field of view reaches as far as [`SIGHT_RANGE`] (16 cells along a
    /// row, 8 along a column) and is blocked by walls (see the `fov`
    /// module). It is computed when the game starts and after every action
    /// of the player that took time. The player's own cell is always
    /// visible; cells outside the map never are.
    pub fn is_visible(&self, position: Position) -> bool {
        self.world.map.is_visible(position)
    }

    /// Returns whether the player has seen `position` at least once on this
    /// floor, whether or not it is visible now.
    ///
    /// Every visible cell is remembered, and stays so after the player walks
    /// away. Cells outside the map are never remembered.
    pub fn is_remembered(&self, position: Position) -> bool {
        self.world.map.is_remembered(position)
    }

    /// Returns the sigils the player has learnt, in the order of the sigil
    /// file: the only ones a trace can cast.
    pub const fn grimoire(&self) -> &Grimoire {
        &self.grimoire
    }

    /// Returns the glyphs engraved on the current floor, whether the player
    /// has seen them or not; at most one per cell.
    ///
    /// A glyph whose sigil the grimoire knows has been read (or teaches
    /// nothing new); the others have not.
    pub fn engravings(&self) -> &[Engraving] {
        &self.world.engravings
    }

    /// Returns the glyph engraved at `position`, if any.
    pub fn engraving_at(&self, position: Position) -> Option<&Engraving> {
        self.world.engraving_at(position)
    }

    /// Returns the item with key `id`, on the floor or carried, or `None` if
    /// it was used up.
    pub fn item(&self, id: ItemId) -> Option<&Item> {
        self.world.item(id)
    }

    /// Iterates over the items lying on a cell the player sees right now
    /// (see [`Game::is_visible`]), with what is needed to draw them.
    ///
    /// An item out of the player's field of view is never listed, even on a
    /// remembered cell, and neither is a carried item. The order is stable
    /// between calls.
    pub fn visible_items(&self) -> impl Iterator<Item = VisibleItem<'_>> {
        self.world.items().filter_map(|(id, item)| {
            let position = item.position()?;
            self.world.map.is_visible(position).then(|| VisibleItem {
                id,
                position,
                definition: item.definition().id(),
                family: item.definition().family(),
            })
        })
    }

    /// Iterates over the items the player carries, with their keys, in the
    /// order they were picked up: at most `item::INVENTORY_CAPACITY` of
    /// them.
    pub fn inventory(&self) -> impl Iterator<Item = (ItemId, &Item)> {
        self.world
            .inventory()
            .iter()
            .filter_map(|&id| Some((id, self.world.item(id)?)))
    }

    /// Returns how many of the player's next unstable casts will be cast
    /// pure, thanks to steadying items; 0 at the start of a run.
    pub const fn steady_casts(&self) -> u32 {
        self.steady_casts
    }

    /// Computes what the player sees from its current position and records
    /// it on the map, which also remembers those cells.
    fn update_field_of_view(&mut self) {
        let origin = self.world.actors[self.world.player].position;
        let field = fov::visible_cells(&self.world.map, origin, SIGHT_RANGE);
        self.world.map.set_visible(field);
    }

    /// Lets non-player actors take their turns, in schedule order, until the
    /// player may act or has died. Their events are appended to `events`.
    ///
    /// A held actor whose turn comes loses it instead (see
    /// [`Game::lose_turn`]), the player included. When the player may act,
    /// its essence recovers first (see [`Game::recover_essence`]).
    fn run_until_player_turn(&mut self, events: &mut Vec<Event>) {
        while !self.is_over() {
            let id = self.scheduler.next_actor(&mut self.world.actors);
            if self.world.actors[id].held_turns > 0 {
                self.lose_turn(id, events);
                continue;
            }
            if id == self.world.player {
                self.recover_essence();
                return;
            }
            let action = self.non_player_action(id);
            // A refused action costs nothing, so the same actor would be
            // picked again forever: it waits instead, and time moves on.
            if !self.perform(id, action, events) {
                self.perform(id, Action::Wait, events);
            }
        }
    }

    /// Makes the held actor `id` lose the turn that just came: one less turn
    /// held, the energy of a wait spent, and [`Event::TurnLost`] appended.
    fn lose_turn(&mut self, id: ActorId, events: &mut Vec<Event>) {
        let actor = &mut self.world.actors[id];
        actor.held_turns -= 1;
        spend_energy(actor, WAIT_COST);
        events.push(Event::TurnLost { actor: id });
    }

    /// Gives the player one point of essence per
    /// [`ESSENCE_REGEN_TICKS`] ticks elapsed since `essence_since`, up to its
    /// maximum.
    ///
    /// Ticks left over towards the next point are kept; while the essence is
    /// full, the count restarts every turn, so time spent at the maximum is
    /// never banked.
    fn recover_essence(&mut self) {
        let now = self.scheduler.ticks();
        let player = &mut self.world.actors[self.world.player];
        let missing = player.max_essence() - player.essence();
        let periods = (now - self.essence_since) / ESSENCE_REGEN_TICKS;
        let gained = i32::try_from(periods).unwrap_or(i32::MAX).min(missing);
        player.set_essence(player.essence() + gained);
        self.essence_since += periods * ESSENCE_REGEN_TICKS;
        if player.essence() == player.max_essence() {
            self.essence_since = now;
        }
    }

    /// Chooses the action of the non-player actor `id`, whose turn it is,
    /// and updates its memory of the player.
    ///
    /// Monsters follow `ai::choose_action`. A monster sees as far as the
    /// player does, and sight between two floor cells is symmetric
    /// (decision D15, `fov` module), so the monster sees the player exactly
    /// when the player's field of view, computed after the player's last
    /// action, holds the monster's cell. Other actors count as obstacles on
    /// the way.
    fn non_player_action(&mut self, id: ActorId) -> Action {
        let player = self.world.actors[self.world.player].position;
        let actor = &self.world.actors[id];
        let Some(monster) = actor.as_monster() else {
            return Action::Wait;
        };
        let position = actor.position;
        let sees_player = self.world.map.is_visible(position);
        let mut last_seen = monster.last_seen_player;

        let action = ai::choose_action(
            &self.world.map,
            position,
            player,
            sees_player,
            &mut last_seen,
            |cell| self.world.actor_at(cell).is_some(),
        );

        if let ActorKind::Monster(monster) = &mut self.world.actors[id].kind {
            monster.last_seen_player = last_seen;
        }
        action
    }

    /// Applies `action` for actor `id`, appends the resulting events to
    /// `events` and returns whether it took time.
    ///
    /// A wait always succeeds. A move is handled by [`Game::step`], a cast
    /// by `Game::cast` (see the `cast` submodule), picking up, using and
    /// dropping an item by the `inventory` submodule, going down the
    /// stairs by the `descent` submodule and a step of automatic
    /// exploration by the `explore` submodule, which only the player calls
    /// on; all of them may be refused, in which case nothing is spent and
    /// `false` is returned.
    fn perform(&mut self, id: ActorId, action: Action, events: &mut Vec<Event>) -> bool {
        match action {
            Action::Move(direction) => self.step(id, direction, events),
            Action::Wait => {
                spend_energy(&mut self.world.actors[id], action_cost(action));
                events.push(Event::Waited { actor: id });
                true
            }
            Action::Cast { trace, direction } => self.cast(id, trace, direction, events),
            Action::PickUp => self.pick_up(id, events),
            Action::Use(item) => self.use_item(id, item, events),
            Action::Drop(item) => self.drop_item(id, item, events),
            Action::Descend => self.descend(id, events),
            Action::Explore => self.explore(id, events),
        }
    }

    /// Moves actor `id` one cell in `direction`, appends the resulting
    /// events to `events` and returns whether it took time.
    ///
    /// A move into a foe (see [`Game::foe_at`]) strikes it instead (see
    /// [`Game::strike`]) and `true` is returned. Otherwise, on success the
    /// actor moves, spends the move's energy cost and `true` is returned; the
    /// player then reads the glyph it stepped on, if any (see
    /// [`Game::read_glyph`]). A move towards a cell that is not walkable or
    /// holds another actor that is not a foe is refused: the actor keeps its
    /// position and energy, [`Event::MoveBlocked`] is appended and `false` is
    /// returned.
    fn step(&mut self, id: ActorId, direction: Direction, events: &mut Vec<Event>) -> bool {
        let action = Action::Move(direction);
        let from = self.world.actors[id].position;
        let target = from.step(direction);
        if let Some(foe) = self.foe_at(id, target) {
            self.strike(id, foe, events);
            return true;
        }
        let occupied = self.world.actor_at(target).is_some_and(|other| other != id);
        let outcome = match action::resolve(action, from, &self.world.map) {
            ActionOutcome::Performed { .. } if occupied => ActionOutcome::Blocked,
            outcome => outcome,
        };

        match outcome {
            ActionOutcome::Blocked => {
                events.push(Event::MoveBlocked {
                    actor: id,
                    from,
                    to: target,
                });
                false
            }
            ActionOutcome::Performed { position } => {
                let actor = &mut self.world.actors[id];
                actor.position = position;
                spend_energy(actor, action_cost(action));
                events.push(Event::Moved {
                    actor: id,
                    from,
                    to: position,
                });
                if id == self.world.player {
                    self.read_glyph(events);
                }
                true
            }
        }
    }

    /// Reads the glyph engraved under the player, if any: a sigil the
    /// grimoire does not know yet joins it and [`Event::GlyphLearned`] is
    /// appended to `events`; a known one changes nothing and reports
    /// nothing.
    fn read_glyph(&mut self, events: &mut Vec<Event>) {
        let position = self.world.player_position();
        let Some(engraving) = self.world.engraving_at(position) else {
            return;
        };
        if self.grimoire.learn(engraving.sigil()) {
            events.push(Event::GlyphLearned {
                sigil: engraving.sigil().id().to_owned(),
                position,
            });
        }
    }

    /// Returns the actor at `position` that `id` fights, if any.
    ///
    /// The player and the monsters are foes; two monsters are not, so they
    /// block each other instead of fighting. An actor is never its own foe.
    fn foe_at(&self, id: ActorId, position: Position) -> Option<ActorId> {
        let other = self.world.actor_at(position).filter(|&other| other != id)?;
        (self.world.actors[id].is_player() != self.world.actors[other].is_player()).then_some(other)
    }

    /// Makes `attacker` strike `target`, charging it [`ATTACK_COST`], and
    /// appends [`Event::Attacked`] to `events`.
    ///
    /// The damage is `combat::roll_damage` of the attacker's attack against
    /// the target's defense, drawn from the gameplay stream. A target left
    /// with no hit points dies (see [`Game::kill`]).
    fn strike(&mut self, attacker: ActorId, target: ActorId, events: &mut Vec<Event>) {
        let damage = combat::roll_damage(
            self.world.actors[attacker].attack(),
            self.world.actors[target].defense(),
            &mut self.gameplay_rng,
        );
        let struck = &mut self.world.actors[target];
        struck.take_damage(damage);
        let hp_left = struck.hp();
        spend_energy(&mut self.world.actors[attacker], ATTACK_COST);
        events.push(Event::Attacked {
            attacker,
            target,
            damage,
            hp_left,
        });
        if hp_left == 0 {
            self.kill(target, attacker, events);
        }
    }

    /// Records the death of `victim` at the hands of `killer` and appends
    /// [`Event::Died`] to `events`.
    ///
    /// A monster leaves the map and the turn order, which frees its cell. The
    /// player stays where it is, and the game is over.
    fn kill(&mut self, victim: ActorId, killer: ActorId, events: &mut Vec<Event>) {
        let who = self.combatant(victim);
        let killed_by = self.combatant(killer);
        if victim == self.world.player {
            self.status = Status::GameOver {
                killed_by: killed_by.clone(),
            };
        } else {
            self.world.actors.remove(victim);
            self.scheduler.remove(victim);
        }
        events.push(Event::Died {
            actor: victim,
            who,
            killed_by,
        });
    }

    /// Returns who the actor `id` is, as events record it.
    fn combatant(&self, id: ActorId) -> Combatant {
        match &self.world.actors[id].kind {
            ActorKind::Player => Combatant::Player,
            ActorKind::Monster(monster) => Combatant::Monster {
                definition: monster.definition().to_owned(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::{PLAYER_ATTACK, PLAYER_DEFENSE, PLAYER_MAX_HP};
    use crate::generation::MONSTERS_PER_FLOOR;
    use crate::geometry::Direction;
    use crate::map::Tile;
    use crate::path::DijkstraMap;
    use crate::path::test_maps::from_rows;
    use crate::schedule::{ACTION_THRESHOLD, NORMAL_SPEED};

    /// Seed of every test game; the fixed room does not depend on it.
    const SEED: Seed = Seed::new(42);

    /// Cells of the fixed room out of the player's sight from
    /// [`PLAYER_START`] (more than 16 columns away), where monsters wait
    /// unseen as long as the player stays near the left wall.
    const FAR_CELLS: [Position; 2] = [Position::new(25, 3), Position::new(27, 8)];

    /// A new game in the fixed room, the ground of most tests here.
    fn fixed_room_game() -> Game {
        Game::in_fixed_room(SEED)
    }

    /// The content shipped with the game.
    fn content() -> Content {
        Content::load().expect("the shipped content is valid")
    }

    /// A monster definition moving at `speed`, for tests that need a given
    /// pace: 5 hit points, attack 1, no defense.
    fn creature(speed: i32) -> MonsterDef {
        fighter(speed, 5, 1, 0)
    }

    /// A monster definition with the given speed and fighting values, for
    /// tests of fights.
    fn fighter(speed: i32, hp: i32, attack: i32, defense: i32) -> MonsterDef {
        let source = format!(
            "[Monster(id: \"test_creature\", name: \"test creature\", family: Vermin, \
             elite: false, speed: {speed}, hp: {hp}, attack: {attack}, defense: {defense}, \
             min_depth: 1)]"
        );
        let content = Content::parse(&source).expect("valid test monster");
        content.monsters()[0].clone()
    }

    /// The blows a fresh game of seed [`SEED`] deals, in order, for an
    /// attacker with `attack` against a target with `defense`: the gameplay
    /// stream of the first floor, read independently of the game.
    fn expected_blows(attack: i32, defense: i32, count: usize) -> Vec<i32> {
        let mut rng = SEED.stream(FIRST_DEPTH, Purpose::Gameplay);
        (0..count)
            .map(|_| combat::roll_damage(attack, defense, &mut rng))
            .collect()
    }

    /// The monster combatant of the test definitions, as events name it.
    fn test_creature() -> Combatant {
        Combatant::Monster {
            definition: "test_creature".to_owned(),
        }
    }

    /// Returns the `Attacked` events of `events` dealt by `attacker`.
    fn blows_by(events: &[Event], attacker: ActorId) -> Vec<&Event> {
        events
            .iter()
            .filter(|e| matches!(e, Event::Attacked { attacker: a, .. } if *a == attacker))
            .collect()
    }

    /// A game on the map drawn by `rows` (see `path::test_maps::from_rows`)
    /// with the player alone at `start`.
    fn game_on(rows: &[&str], start: Position) -> Game {
        Game::start(SEED, from_rows(rows), start, &[])
    }

    /// Plays `turns` waits of the player and returns every event.
    fn wait_turns(game: &mut Game, turns: usize) -> Vec<Event> {
        (0..turns).flat_map(|_| game.submit(Action::Wait)).collect()
    }

    /// Returns where the actor `id` stands.
    fn position_of(game: &Game, id: ActorId) -> Position {
        game.actor(id).expect("the actor exists").position()
    }

    /// Energy of every actor, keyed for comparison before and after a turn.
    fn energies(game: &Game) -> Vec<(ActorId, i32)> {
        game.actors().map(|(id, a)| (id, a.energy())).collect()
    }

    #[test]
    fn player_starts_on_open_floor_and_may_act() {
        let game = fixed_room_game();
        let player = game.player_actor();

        assert_eq!(player.position(), PLAYER_START);
        assert!(game.map().is_walkable(PLAYER_START));
        for direction in Direction::ALL {
            assert!(
                game.map().is_walkable(PLAYER_START.step(direction)),
                "neighbour {direction:?} of the start is not floor"
            );
        }
        assert!(player.energy() >= ACTION_THRESHOLD);
        assert_eq!(game.actors().count(), 1);
    }

    #[test]
    fn move_returns_a_moved_event_and_updates_the_player() {
        let mut game = fixed_room_game();
        let player = game.player();
        let ticks = game.ticks();

        let events = game.submit(Action::Move(Direction::NorthEast));

        let to = PLAYER_START.step(Direction::NorthEast);
        assert_eq!(
            events,
            [Event::Moved {
                actor: player,
                from: PLAYER_START,
                to
            }]
        );
        assert_eq!(game.player_actor().position(), to);
        assert_eq!(game.ticks(), ticks + 1);
    }

    #[test]
    fn wait_returns_a_waited_event_and_lets_time_pass() {
        let mut game = fixed_room_game();
        let player = game.player();
        let ticks = game.ticks();

        let events = game.submit(Action::Wait);

        assert_eq!(events, [Event::Waited { actor: player }]);
        assert_eq!(game.player_actor().position(), PLAYER_START);
        assert_eq!(game.ticks(), ticks + 1);
    }

    #[test]
    fn other_actors_act_before_the_player_turn_returns() {
        let mut game = fixed_room_game();
        let player = game.player();
        let fast = game.spawn_monster(FAR_CELLS[0], &creature(2 * NORMAL_SPEED));

        // The new monster starts without energy, so it first acts after the
        // player's next tick; the player was created first and keeps the
        // lead. Out of sight and with no memory of the player, it waits.
        assert_eq!(game.submit(Action::Wait), [Event::Waited { actor: player }]);
        let events = game.submit(Action::Move(Direction::South));

        assert_eq!(
            events,
            [
                Event::Moved {
                    actor: player,
                    from: PLAYER_START,
                    to: PLAYER_START.step(Direction::South),
                },
                Event::Waited { actor: fast },
                Event::Waited { actor: fast },
            ]
        );
    }

    #[test]
    fn double_speed_actor_acts_twice_per_normal_player_action() {
        let mut game = fixed_room_game();
        let player = game.player();
        let fast = game.spawn_monster(FAR_CELLS[0], &creature(2 * NORMAL_SPEED));
        let slow = game.spawn_monster(FAR_CELLS[1], &creature(NORMAL_SPEED / 2));

        let events: Vec<Event> = (0..20).flat_map(|_| game.submit(Action::Wait)).collect();

        let turns = |id| {
            events
                .iter()
                .filter(|e| **e == Event::Waited { actor: id })
                .count()
        };
        assert_eq!(turns(player), 20);
        // The other actors start one player turn behind (see above): 19 ticks
        // have passed for them when the player has acted 20 times.
        assert_eq!(turns(fast), 38);
        assert_eq!(turns(slow), 9);
    }

    #[test]
    fn wall_blocks_the_player_without_spending_time() {
        let mut game = fixed_room_game();
        let player = game.player();
        let fast = game.spawn_monster(FAR_CELLS[0], &creature(2 * NORMAL_SPEED));
        game.submit(Action::Wait);
        // Walk to the cell next to the left wall.
        for _ in 0..4 {
            game.submit(Action::Move(Direction::West));
        }
        let at_wall = Position::new(1, 5);
        assert_eq!(game.player_actor().position(), at_wall);
        let ticks = game.ticks();
        let energy_before = energies(&game);

        let events = game.submit(Action::Move(Direction::West));

        assert_eq!(
            events,
            [Event::MoveBlocked {
                actor: player,
                from: at_wall,
                to: Position::new(0, 5),
            }]
        );
        assert_eq!(game.player_actor().position(), at_wall);
        assert_eq!(game.ticks(), ticks, "no time passed");
        assert_eq!(energies(&game), energy_before, "nobody acted");
        assert_eq!(
            position_of(&game, fast),
            FAR_CELLS[0],
            "never saw the player"
        );
    }

    #[test]
    fn the_player_attacks_a_monster_by_moving_into_it() {
        let mut game = fixed_room_game();
        let player = game.player();
        let east = PLAYER_START.step(Direction::East);
        let monster = game.spawn_monster(east, &fighter(NORMAL_SPEED, 50, 1, 2));
        let energy = game.player_actor().energy();

        let events = game.submit(Action::Move(Direction::East));

        let damage = expected_blows(PLAYER_ATTACK, 2, 1)[0];
        assert_eq!(
            events,
            [Event::Attacked {
                attacker: player,
                target: monster,
                damage,
                hp_left: 50 - damage,
            }],
            "the monster had no energy yet, so it did not strike back"
        );
        assert_eq!(game.player_actor().position(), PLAYER_START, "no step");
        assert_eq!(position_of(&game, monster), east);
        assert_eq!(game.actor(monster).map(Actor::hp), Some(50 - damage));
        assert_eq!(game.player_turns(), 1, "a blow takes the player's turn");
        // One tick has given back a normal speed's worth of energy.
        assert_eq!(
            game.player_actor().energy(),
            energy - ATTACK_COST + NORMAL_SPEED
        );
    }

    #[test]
    fn the_damage_of_every_blow_follows_the_gameplay_stream_of_the_seed() {
        let play = || {
            let mut game = fixed_room_game();
            let east = PLAYER_START.step(Direction::East);
            // Too slow to strike back during the test, and too tough to die.
            let monster = game.spawn_monster(east, &fighter(1, 999, 1, 1));
            let events: Vec<Event> = (0..20)
                .flat_map(|_| game.submit(Action::Move(Direction::East)))
                .collect();
            (game, monster, events)
        };

        let (game, monster, events) = play();

        let damages: Vec<i32> = events
            .iter()
            .map(|event| match event {
                Event::Attacked { damage, .. } => *damage,
                other => panic!("only the player's blows expected, got {other:?}"),
            })
            .collect();
        assert_eq!(damages, expected_blows(PLAYER_ATTACK, 1, 20));
        assert_eq!(
            game.actor(monster).map(Actor::hp),
            Some(999 - damages.iter().sum::<i32>())
        );
        // A second game of the same seed replays the same fight.
        assert_eq!(play().2, events);
    }

    #[test]
    fn a_monster_next_to_the_player_attacks_instead_of_waiting() {
        let mut game = fixed_room_game();
        let player = game.player();
        let monster = game.spawn_monster(
            PLAYER_START.step(Direction::North),
            &fighter(NORMAL_SPEED, 10, 3, 0),
        );
        // The new monster first acts after the player's second action.
        assert_eq!(game.submit(Action::Wait), [Event::Waited { actor: player }]);

        let events = game.submit(Action::Wait);

        let damage = expected_blows(3, PLAYER_DEFENSE, 1)[0];
        assert_eq!(
            events,
            [
                Event::Waited { actor: player },
                Event::Attacked {
                    attacker: monster,
                    target: player,
                    damage,
                    hp_left: PLAYER_MAX_HP - damage,
                },
            ]
        );
        assert_eq!(game.player_actor().hp(), PLAYER_MAX_HP - damage);
        assert_eq!(
            position_of(&game, monster),
            PLAYER_START.step(Direction::North),
            "it stays on its cell"
        );
        assert_eq!(game.status(), &Status::Playing);
    }

    #[test]
    fn a_killed_monster_leaves_the_world_and_the_turn_order_and_frees_its_cell() {
        let mut game = fixed_room_game();
        let player = game.player();
        let east = PLAYER_START.step(Direction::East);
        // One hit point: the first blow kills, whatever the roll.
        let monster = game.spawn_monster(east, &fighter(NORMAL_SPEED, 1, 1, 0));

        let events = game.submit(Action::Move(Direction::East));

        let damage = expected_blows(PLAYER_ATTACK, 0, 1)[0];
        assert_eq!(
            events,
            [
                Event::Attacked {
                    attacker: player,
                    target: monster,
                    damage,
                    hp_left: 0,
                },
                Event::Died {
                    actor: monster,
                    who: test_creature(),
                    killed_by: Combatant::Player,
                },
            ]
        );
        assert!(game.actor(monster).is_none(), "removed from the world");
        assert!(!game.scheduler.is_enrolled(monster), "and from turn order");
        assert_eq!(game.world.actor_at(east), None);
        assert_eq!(game.visible_monsters().count(), 0);
        assert_eq!(game.status(), &Status::Playing);

        // The cell is free: the next move steps onto it, and turns go on
        // without the dead monster.
        let events = game.submit(Action::Move(Direction::East));
        assert_eq!(
            events,
            [Event::Moved {
                actor: player,
                from: PLAYER_START,
                to: east,
            }]
        );
        assert_eq!(wait_turns(&mut game, 3).len(), 3);
    }

    #[test]
    fn the_death_of_the_player_ends_the_game_and_submit_does_nothing_after() {
        let mut game = fixed_room_game();
        let player = game.player();
        // Each blow kills: attack 99 against defense 1 deals at least 97.
        let killer = game.spawn_monster(
            PLAYER_START.step(Direction::North),
            &fighter(NORMAL_SPEED, 10, 99, 0),
        );
        let other = game.spawn_monster(
            PLAYER_START.step(Direction::South),
            &fighter(NORMAL_SPEED, 10, 99, 0),
        );
        game.submit(Action::Wait);

        let events = game.submit(Action::Wait);

        let damage = expected_blows(99, PLAYER_DEFENSE, 1)[0];
        assert_eq!(
            events,
            [
                Event::Waited { actor: player },
                Event::Attacked {
                    attacker: killer,
                    target: player,
                    damage,
                    hp_left: 0,
                },
                Event::Died {
                    actor: player,
                    who: Combatant::Player,
                    killed_by: test_creature(),
                },
            ],
            "the second monster no longer acts once the player is dead"
        );
        assert_eq!(
            game.status(),
            &Status::GameOver {
                killed_by: test_creature()
            }
        );
        assert!(game.is_over());
        assert_eq!(game.player_actor().hp(), 0);
        assert!(!game.player_actor().is_alive());
        assert_eq!(game.player_actor().position(), PLAYER_START, "still drawn");

        // Nothing changes any more, whatever the action.
        let ticks = game.ticks();
        let turns = game.player_turns();
        let energy = energies(&game);
        let map = game.map().clone();
        for action in [
            Action::Wait,
            Action::Move(Direction::West),
            Action::Move(Direction::North),
        ] {
            assert_eq!(game.submit(action), [], "{action:?}");
        }
        assert_eq!(game.ticks(), ticks);
        assert_eq!(game.player_turns(), turns);
        assert_eq!(energies(&game), energy);
        assert_eq!(game.map(), &map);
        assert_eq!(game.player_actor().position(), PLAYER_START);
        assert_eq!(game.actor(killer).map(Actor::hp), Some(10));
        assert_eq!(game.actor(other).map(Actor::hp), Some(10));
    }

    #[test]
    fn a_monster_of_speed_200_strikes_twice_per_normal_player_action() {
        let mut game = fixed_room_game();
        let player = game.player();
        // Attack 1 against defense 1 always deals the minimum of 1, so the
        // player survives every blow of the test.
        let fast = game.spawn_monster(
            PLAYER_START.step(Direction::East),
            &fighter(2 * NORMAL_SPEED, 50, 1, 0),
        );
        game.submit(Action::Wait);

        for turn in 0..5 {
            let events = game.submit(Action::Wait);

            assert_eq!(events[0], Event::Waited { actor: player }, "turn {turn}");
            assert_eq!(blows_by(&events, fast).len(), 2, "turn {turn}");
            assert_eq!(events.len(), 3, "turn {turn}: {events:?}");
        }
        assert_eq!(game.player_actor().hp(), PLAYER_MAX_HP - 10);
    }

    #[test]
    fn monsters_never_attack_each_other() {
        let mut game = fixed_room_game();
        let first = game.spawn_monster(FAR_CELLS[0], &creature(NORMAL_SPEED));
        let beside = FAR_CELLS[0].step(Direction::East);
        let second = game.spawn_monster(beside, &creature(NORMAL_SPEED));
        let mut events = Vec::new();

        let performed = game.perform(first, Action::Move(Direction::East), &mut events);

        assert!(!performed);
        assert_eq!(
            events,
            [Event::MoveBlocked {
                actor: first,
                from: FAR_CELLS[0],
                to: beside,
            }]
        );
        assert_eq!(game.actor(second).map(Actor::hp), Some(5));
        assert_eq!(game.actor(first).map(Actor::energy), Some(0));
    }

    #[test]
    fn events_of_generated_games_keep_their_promises() {
        // Over real floors and fights: hit points stay within bounds, every
        // key in a turn's events names a live actor or one whose death is
        // reported in that turn, and a dead player ends the game.
        let content = content();
        let (mut blows, mut deaths) = (0, 0);
        for value in 0..20 {
            let mut game = Game::new(Seed::new(value), &content);
            let stairs = generation::generate(Seed::new(value), FIRST_DEPTH).stairs();
            for _ in 0..150 {
                let distances = DijkstraMap::new(game.map(), [stairs]);
                let action = distances
                    .downhill(game.player_actor().position())
                    .map_or(Action::Wait, Action::Move);
                let events = game.submit(action);

                let died: Vec<ActorId> = events
                    .iter()
                    .filter_map(|e| match e {
                        Event::Died { actor, .. } => Some(*actor),
                        _ => None,
                    })
                    .collect();
                deaths += died.len();
                for event in &events {
                    if let Event::Attacked {
                        attacker,
                        target,
                        damage,
                        hp_left,
                    } = event
                    {
                        blows += 1;
                        assert!(*damage >= combat::MIN_DAMAGE);
                        assert!(*hp_left >= 0);
                        assert!(game.actor(*attacker).is_some());
                        assert!(game.actor(*target).is_some() || died.contains(target));
                        assert_ne!(
                            game.actor(*attacker).map(Actor::is_player),
                            game.actor(*target).map(Actor::is_player),
                            "only foes fight, seed {value}"
                        );
                    }
                }
                for (_, actor) in game.actors() {
                    assert!((0..=actor.max_hp()).contains(&actor.hp()), "seed {value}");
                }
                assert_eq!(
                    game.is_over(),
                    !game.player_actor().is_alive(),
                    "seed {value}"
                );
                if game.is_over() {
                    assert_eq!(game.submit(Action::Wait), []);
                    break;
                }
            }
        }
        assert!(blows > 0 && deaths > 0, "the walks met no fight to check");
    }

    #[test]
    fn a_new_game_starts_on_the_first_generated_floor() {
        for value in [0, 42, u64::MAX] {
            let seed = Seed::new(value);
            let floor = generation::generate(seed, FIRST_DEPTH);

            let game = Game::new(seed, &content());

            let tiles = |map: &Map| map.positions().map(|p| map.tile(p)).collect::<Vec<_>>();
            assert_eq!(tiles(game.map()), tiles(floor.map()));
            assert_eq!(game.player_actor().position(), floor.start());
            assert_eq!(game.actors().count(), 1 + MONSTERS_PER_FLOOR);
            assert!(game.player_actor().energy() >= ACTION_THRESHOLD);
        }
    }

    #[test]
    fn the_player_can_walk_onto_the_stairs() {
        let seed = Seed::new(42);
        let floor = generation::generate(seed, FIRST_DEPTH);
        // Without monsters, which could stand in the way.
        let mut game = Game::on_floor(seed, floor.clone());
        let stairs = floor.stairs();
        let distances = DijkstraMap::new(floor.map(), [stairs]);

        // Follow the way down to the stairs; each step must be accepted.
        while let Some(direction) = distances.downhill(game.player_actor().position()) {
            let events = game.submit(Action::Move(direction));
            assert!(
                matches!(events.as_slice(), [Event::Moved { .. }]),
                "{events:?}"
            );
        }

        assert_eq!(game.player_actor().position(), stairs);
        assert_eq!(game.map().tile(stairs), Some(Tile::StairsDown));
    }

    #[test]
    fn a_game_keeps_the_seed_it_was_started_with() {
        for value in [0, 42, u64::MAX] {
            let mut game = Game::new(Seed::new(value), &content());
            game.submit(Action::Wait);

            assert_eq!(game.seed(), Seed::new(value));
            assert_eq!(game.daily_date(), None, "not a daily run");
        }
    }

    #[test]
    fn a_daily_game_is_the_game_of_its_daily_seed_and_keeps_its_date() {
        let content = content();
        let date = DailyDate::new(2026, 11, 8).expect("a real date");
        let mut daily = Game::daily(date, &content);
        let mut same_seed = Game::new(Seed::daily(date), &content);

        assert_eq!(daily.seed(), Seed::daily(date));
        assert_eq!(daily.daily_date(), Some(date));
        assert_eq!(same_seed.daily_date(), None, "a typed seed is not daily");
        assert_eq!(daily.map(), same_seed.map(), "same first floor");
        for _ in 0..20 {
            assert_eq!(daily.submit(Action::Wait), same_seed.submit(Action::Wait));
        }
        assert_eq!(daily.daily_date(), Some(date), "the date stays");

        let next_day = DailyDate::new(2026, 11, 9).expect("a real date");
        assert_ne!(
            Game::daily(next_day, &content).map(),
            daily.map(),
            "another date, another dungeon"
        );
    }

    #[test]
    fn a_new_game_has_no_player_turn_yet() {
        let game = fixed_room_game();

        assert_eq!(game.player_turns(), 0);
        assert_eq!(game.ticks(), 1, "the clock already ran to the first turn");
    }

    #[test]
    fn player_turns_count_only_player_actions_that_took_time() {
        let mut game = fixed_room_game();
        let fast = game.spawn_monster(FAR_CELLS[0], &creature(2 * NORMAL_SPEED));
        let slow = game.spawn_monster(FAR_CELLS[1], &creature(NORMAL_SPEED / 2));
        // Four moves west reach the wall, the fifth is refused, then waits.
        let actions = [
            Action::Move(Direction::West),
            Action::Move(Direction::West),
            Action::Move(Direction::West),
            Action::Move(Direction::West),
            Action::Move(Direction::West),
            Action::Wait,
            Action::Move(Direction::NorthWest),
            Action::Wait,
            Action::Wait,
            Action::Wait,
        ];

        let mut events = Vec::new();
        let mut performed = 0;
        for action in actions {
            let before = game.player_turns();
            let happened = game.submit(action);
            let blocked = matches!(happened.as_slice(), [Event::MoveBlocked { .. }]);
            if !blocked {
                performed += 1;
            }
            assert_eq!(
                game.player_turns(),
                before + u64::from(!blocked),
                "{action:?}"
            );
            events.extend(happened);
        }

        // The wall refuses the fifth move west and the move north-west.
        assert_eq!(performed, 8);
        assert_eq!(game.player_turns(), 8);
        let turns_of = |id| {
            events
                .iter()
                .filter(|e| **e == Event::Waited { actor: id })
                .count()
        };
        assert!(turns_of(fast) > 8, "the fast actor acted more often");
        assert!(turns_of(slow) < 8, "the slow actor acted less often");
    }

    #[test]
    fn a_new_game_already_shows_what_the_player_sees() {
        let game = fixed_room_game();

        assert!(game.is_visible(PLAYER_START));
        assert!(game.is_remembered(PLAYER_START));
        assert!(game.is_visible(Position::new(0, 5)), "the wall to the left");
        assert_eq!(
            game.map()
                .positions()
                .filter(|&p| game.is_visible(p))
                .collect::<Vec<_>>(),
            fov::visible_cells(game.map(), PLAYER_START, SIGHT_RANGE)
        );
        // Seventeen cells to the right is beyond the 16 cells sight reaches
        // along a row.
        assert!(game.is_visible(Position::new(21, 5)));
        assert!(!game.is_visible(Position::new(22, 5)));
        assert!(!game.is_remembered(Position::new(22, 5)));
    }

    #[test]
    fn the_field_of_view_follows_the_player() {
        let mut game = fixed_room_game();
        let far_right = Position::new(22, 5);

        for _ in 0..3 {
            game.submit(Action::Move(Direction::East));
        }

        let player = game.player_actor().position();
        assert_eq!(player, Position::new(8, 5));
        assert!(game.is_visible(player));
        assert!(game.is_visible(far_right));
        assert_eq!(
            game.map()
                .positions()
                .filter(|&p| game.is_visible(p))
                .collect::<Vec<_>>(),
            fov::visible_cells(game.map(), player, SIGHT_RANGE)
        );
    }

    #[test]
    fn a_refused_move_leaves_the_field_of_view_unchanged() {
        let mut game = fixed_room_game();
        for _ in 0..4 {
            game.submit(Action::Move(Direction::West));
        }
        let before = game.map().clone();

        game.submit(Action::Move(Direction::West));

        assert_eq!(game.map(), &before);
    }

    #[test]
    fn cells_seen_once_stay_remembered_after_the_player_leaves() {
        let mut game = fixed_room_game();
        let left_wall = Position::new(0, 5);
        let seen_at_start: Vec<Position> = game
            .map()
            .positions()
            .filter(|&p| game.is_visible(p))
            .collect();

        for _ in 0..12 {
            game.submit(Action::Move(Direction::East));
        }

        assert_eq!(game.player_actor().position(), Position::new(17, 5));
        assert!(!game.is_visible(left_wall), "now out of range");
        assert!(game.is_remembered(left_wall));
        assert!(seen_at_start.iter().all(|&p| game.is_remembered(p)));
        // Visible cells are always remembered.
        assert!(
            game.map()
                .positions()
                .all(|p| !game.is_visible(p) || game.is_remembered(p))
        );
    }

    #[test]
    fn a_new_generated_floor_shows_the_view_from_its_start() {
        let game = Game::new(SEED, &content());
        let map = game.map();
        let start = game.player_actor().position();

        let visible: Vec<Position> = map.positions().filter(|&p| game.is_visible(p)).collect();

        assert_eq!(visible, fov::visible_cells(map, start, SIGHT_RANGE));
        assert!(visible.contains(&start));
        // The rest of the floor lies behind walls or beyond the radius.
        assert!(visible.len() < map.positions().count());
    }

    #[test]
    #[should_panic(expected = "cannot spawn an actor at (5, 5)")]
    fn spawning_on_an_occupied_cell_panics() {
        fixed_room_game().spawn_monster(PLAYER_START, &creature(NORMAL_SPEED));
    }

    #[test]
    fn a_new_game_places_the_floor_monsters_with_their_head_start() {
        let content = content();
        for value in [0, 42, 7_777] {
            let seed = Seed::new(value);
            let floor = generation::generate_with_monsters(seed, FIRST_DEPTH, &content);

            let game = Game::new(seed, &content);

            let monsters: Vec<(Position, &str, i32)> = game
                .actors()
                .filter_map(|(_, a)| {
                    a.as_monster()
                        .map(|m| (a.position(), m.definition(), a.energy()))
                })
                .collect();
            let expected: Vec<(Position, &str, i32)> = floor
                .monsters()
                .iter()
                .map(|spawn| {
                    // One tick has passed to give the player its first turn.
                    let energy = spawn.energy() + spawn.definition().speed();
                    (spawn.position(), spawn.definition().id(), energy)
                })
                .collect();
            assert_eq!(monsters, expected, "seed {value}");
        }
    }

    #[test]
    fn the_player_acts_first_on_a_new_floor() {
        let content = content();
        for value in 0..20 {
            let mut game = Game::new(Seed::new(value), &content);
            let player = game.player();

            let events = game.submit(Action::Wait);

            assert_eq!(events.first(), Some(&Event::Waited { actor: player }));
        }
    }

    #[test]
    fn a_monster_in_sight_closes_in_by_the_shortest_path() {
        let mut game = fixed_room_game();
        // Behind the pillar from the player's side, so the straight line is
        // not the way.
        let monster = game.spawn_monster(Position::new(12, 7), &creature(NORMAL_SPEED));
        assert!(game.is_visible(Position::new(12, 7)));
        let player = PLAYER_START;
        // Its first turn comes after the player's first action.
        game.submit(Action::Wait);

        let mut distance = DijkstraMap::new(game.map(), [player])
            .distance(position_of(&game, monster))
            .expect("same room");
        while distance > 1 {
            let events = game.submit(Action::Wait);

            let now = position_of(&game, monster);
            let closer = DijkstraMap::new(game.map(), [player]).distance(now);
            assert_eq!(closer, Some(distance - 1), "one step closer each turn");
            assert!(matches!(events[1], Event::Moved { actor, .. } if actor == monster));
            distance -= 1;
        }

        // Next to the player, it stays there and strikes every turn.
        let at_contact = position_of(&game, monster);
        let events = wait_turns(&mut game, 3);
        assert_eq!(position_of(&game, monster), at_contact);
        assert_eq!(blows_by(&events, monster).len(), 3);
    }

    #[test]
    fn a_fast_monster_moves_twice_per_normal_player_action() {
        let mut game = fixed_room_game();
        let fast = game.spawn_monster(Position::new(12, 5), &creature(2 * NORMAL_SPEED));
        let player = game.player();
        game.submit(Action::Wait);

        let events = game.submit(Action::Wait);

        assert_eq!(
            events,
            [
                Event::Waited { actor: player },
                Event::Moved {
                    actor: fast,
                    from: Position::new(12, 5),
                    to: Position::new(11, 5),
                },
                Event::Moved {
                    actor: fast,
                    from: Position::new(11, 5),
                    to: Position::new(10, 5),
                },
            ]
        );
    }

    #[test]
    fn a_monster_walks_to_where_it_last_saw_the_player_then_waits() {
        // Two rooms with no opening between them.
        let rows = [
            "####################", //
            "#........#.........#", //
            "#........#.........#", //
            "#........#.........#", //
            "####################", //
        ];
        let last_seen = Position::new(7, 2);
        let mut game = game_on(&rows, last_seen);
        let monster = game.spawn_monster(Position::new(2, 2), &creature(NORMAL_SPEED));
        game.submit(Action::Wait);
        // The monster sees the player and steps towards it.
        game.submit(Action::Wait);
        assert_eq!(position_of(&game, monster), Position::new(3, 2));

        // The player vanishes into the other room, out of the monster's
        // sight.
        let hidden = Position::new(15, 2);
        game.world.actors[game.world.player].position = hidden;
        game.update_field_of_view();
        assert!(
            !fov::visible_cells(game.map(), position_of(&game, monster), SIGHT_RANGE)
                .contains(&hidden)
        );

        for expected_x in 4..=7 {
            game.submit(Action::Wait);
            assert_eq!(position_of(&game, monster), Position::new(expected_x, 2));
        }
        // On the last cell where it saw the player, it stops for good.
        let events = wait_turns(&mut game, 5);
        assert_eq!(position_of(&game, monster), last_seen);
        assert_eq!(
            events
                .iter()
                .filter(|e| **e == Event::Waited { actor: monster })
                .count(),
            5
        );
        let memory = game
            .actor(monster)
            .and_then(Actor::as_monster)
            .map(|m| m.last_seen_player());
        assert_eq!(memory, Some(None), "the player is forgotten");
    }

    #[test]
    fn a_monster_that_never_saw_the_player_stays_put() {
        let mut game = fixed_room_game();
        let monster = game.spawn_monster(FAR_CELLS[0], &creature(NORMAL_SPEED));

        wait_turns(&mut game, 10);

        assert_eq!(position_of(&game, monster), FAR_CELLS[0]);
    }

    #[test]
    fn a_monster_sees_the_player_exactly_when_the_player_sees_it() {
        // The rule `non_player_action` relies on: from the monster's own
        // field of view, computed independently, the answer is the same.
        let content = content();
        for value in 0..30 {
            let game = Game::new(Seed::new(value), &content);
            let player = game.player_actor().position();
            for cell in game
                .map()
                .positions()
                .filter(|&p| game.map().is_walkable(p))
            {
                let monster_sees =
                    fov::visible_cells(game.map(), cell, SIGHT_RANGE).contains(&player);
                assert_eq!(
                    monster_sees,
                    game.is_visible(cell),
                    "seed {value}, {cell:?}"
                );
            }
        }
    }

    #[test]
    fn monsters_out_of_sight_are_not_listed_as_visible() {
        let mut game = fixed_room_game();
        let near = game.spawn_monster(Position::new(9, 5), &creature(NORMAL_SPEED));
        let far = game.spawn_monster(FAR_CELLS[0], &creature(NORMAL_SPEED));

        let visible: Vec<VisibleMonster<'_>> = game.visible_monsters().collect();

        assert_eq!(
            visible,
            [VisibleMonster {
                id: near,
                position: Position::new(9, 5),
                definition: "test_creature",
                family: Family::Vermin,
                elite: false,
            }]
        );
        assert!(
            game.actor(far).is_some(),
            "the hidden one exists all the same"
        );

        // Walking away hides the near one too.
        for _ in 0..4 {
            game.submit(Action::Move(Direction::West));
        }
        let near_at = position_of(&game, near);
        let listed: Vec<ActorId> = game.visible_monsters().map(|m| m.id).collect();
        assert_eq!(listed.contains(&near), game.is_visible(near_at));
        assert!(!listed.contains(&far));
    }

    #[test]
    fn a_new_floor_shows_no_monster_and_lists_only_visible_ones_later() {
        let content = content();
        for value in 0..50 {
            let mut game = Game::new(Seed::new(value), &content);
            assert_eq!(game.visible_monsters().count(), 0, "seed {value}");

            // Walk towards the stairs for a while; whatever is listed must be
            // in view, and every monster in view must be listed.
            let stairs = generation::generate(Seed::new(value), FIRST_DEPTH).stairs();
            for _ in 0..40 {
                let distances = DijkstraMap::new(game.map(), [stairs]);
                let action = distances
                    .downhill(game.player_actor().position())
                    .map_or(Action::Wait, Action::Move);
                game.submit(action);

                let listed: Vec<Position> = game.visible_monsters().map(|m| m.position).collect();
                let in_view: Vec<Position> = game
                    .actors()
                    .filter(|(_, a)| a.as_monster().is_some() && game.is_visible(a.position()))
                    .map(|(_, a)| a.position())
                    .collect();
                assert_eq!(listed, in_view, "seed {value}");
            }
        }
    }
}
