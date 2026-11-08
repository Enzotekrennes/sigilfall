//! A game in progress and its single entry point, [`Game::submit`].
//!
//! Exposes [`Game`] and [`PLAYER_START`]. The game owns the run's seed, the
//! map, the actors and the scheduler. A new game starts on the first
//! generated floor of its seed (see the `generation` module). The player's
//! action is applied, then the other actors take their turns until the
//! player may act again, and everything that happened is returned as
//! [`Event`]s. The game produces no text and draws nothing.
//!
//! The game also keeps the player's field of view up to date: it is computed
//! when the game starts and again after each action of the player, and the
//! cells seen are remembered on the map. Going down the stairs is not handled
//! yet.

use slotmap::SlotMap;

use crate::action::{self, Action, ActionOutcome};
use crate::actor::{Actor, ActorId};
use crate::event::Event;
use crate::fov::{self, SIGHT_RADIUS};
use crate::generation::{self, FIRST_DEPTH};
use crate::geometry::Position;
use crate::map::Map;
use crate::rng::Seed;
use crate::schedule::{NORMAL_SPEED, Scheduler, spend_energy};

/// Cell where the player starts in the fixed room of
/// [`Game::in_fixed_room`].
///
/// A floor cell near the left wall whose eight neighbours are all floor, so
/// every direction can be tried on the first turn, and away from the pillar
/// at `map::FIXED_ROOM_PILLAR`.
pub const PLAYER_START: Position = Position::new(5, 5);

/// A game in progress: the map, the actors on it and the flow of time.
///
/// Between two calls to [`Game::submit`], it is always the player's turn.
#[derive(Debug, Clone)]
pub struct Game {
    /// The seed every random choice of this run follows from.
    seed: Seed,
    /// The current dungeon floor.
    map: Map,
    /// Every actor, the player included.
    actors: SlotMap<ActorId, Actor>,
    /// Key of the player in `actors`.
    player: ActorId,
    /// Turn order and elapsed time.
    scheduler: Scheduler,
    /// Number of player actions that took time since the game started.
    player_turns: u64,
}

impl Game {
    /// Starts a game of the run identified by `seed`, on its first floor
    /// (`generation::generate(seed, FIRST_DEPTH)`), with the player on that
    /// floor's start cell, moving at normal speed, and no other actor.
    ///
    /// The same seed always gives the same floor. Time is advanced until the
    /// player's first turn, so [`Game::submit`] can be called right away.
    pub fn new(seed: Seed) -> Self {
        let floor = generation::generate(seed, FIRST_DEPTH);
        Self::start(seed, floor.map, floor.start)
    }

    /// Starts a game of the run identified by `seed` in the hand-made room of
    /// [`Map::fixed_room`] instead of a generated floor, with the player at
    /// [`PLAYER_START`] and no other actor.
    ///
    /// The room never changes, whatever the seed, which makes it the ground
    /// of tests that need known walls and distances. The seed is only kept
    /// (see [`Game::seed`]).
    pub fn in_fixed_room(seed: Seed) -> Self {
        Self::start(seed, Map::fixed_room(), PLAYER_START)
    }

    /// Starts a game on `map` with the player alone at `start`, at normal
    /// speed, computes what the player sees from there and runs the clock to
    /// the player's first turn.
    ///
    /// `start` is expected to be walkable; both callers guarantee it.
    fn start(seed: Seed, map: Map, start: Position) -> Self {
        let mut actors = SlotMap::with_key();
        let player = actors.insert(Actor::new(start, NORMAL_SPEED));
        let mut scheduler = Scheduler::default();
        scheduler.enrol(player);

        let mut game = Self {
            seed,
            map,
            actors,
            player,
            scheduler,
            player_turns: 0,
        };
        game.update_field_of_view();
        // Only the player exists, so no event can be produced here.
        game.run_until_player_turn(&mut Vec::new());
        game
    }

    /// Places a new non-player actor at `position` with the given `speed`
    /// (energy per tick, see `schedule::NORMAL_SPEED`) and returns its key.
    ///
    /// The actor starts with no energy and takes turns after every actor
    /// created before it when they are ready at the same time. It is still
    /// the player's turn afterwards.
    ///
    /// # Panics
    ///
    /// Panics if `position` is not walkable or already holds an actor, or if
    /// `speed` is not positive: placing an actor there is a bug in the
    /// calling code.
    pub fn spawn_actor(&mut self, position: Position, speed: i32) -> ActorId {
        assert!(
            self.map.is_walkable(position) && self.actor_at(position).is_none(),
            "cannot spawn an actor at ({}, {}): the cell is not free floor",
            position.x,
            position.y
        );
        let id = self.actors.insert(Actor::new(position, speed));
        self.scheduler.enrol(id);
        id
    }

    /// Applies the player's `action`, then lets the other actors act until
    /// it is the player's turn again, and returns what happened in order.
    ///
    /// A move into a wall, the edge of the map or another actor is refused:
    /// the only event is [`Event::MoveBlocked`], the player spends no energy,
    /// no time passes and no other actor acts. Otherwise the first event is
    /// the player's own, followed by those of every actor that acted before
    /// the player's next turn, and [`Game::player_turns`] grows by one.
    ///
    /// After an action that took time, the field of view is computed again
    /// from the player's position (see [`Game::is_visible`]) before the other
    /// actors act.
    pub fn submit(&mut self, action: Action) -> Vec<Event> {
        let mut events = Vec::new();
        if self.perform(self.player, action, &mut events) {
            self.player_turns += 1;
            self.update_field_of_view();
            self.run_until_player_turn(&mut events);
        }
        events
    }

    /// Returns the seed of this run, as given to [`Game::new`] or
    /// [`Game::in_fixed_room`].
    pub const fn seed(&self) -> Seed {
        self.seed
    }

    /// Returns the current dungeon floor.
    pub const fn map(&self) -> &Map {
        &self.map
    }

    /// Returns the key of the player.
    pub const fn player(&self) -> ActorId {
        self.player
    }

    /// Returns the player's actor.
    pub fn player_actor(&self) -> &Actor {
        &self.actors[self.player]
    }

    /// Returns the actor with key `id`, or `None` if it does not exist.
    pub fn actor(&self, id: ActorId) -> Option<&Actor> {
        self.actors.get(id)
    }

    /// Iterates over every actor, the player included, with its key.
    ///
    /// The order is stable between calls but is not the turn order.
    pub fn actors(&self) -> impl Iterator<Item = (ActorId, &Actor)> {
        self.actors.iter()
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
        self.player_turns
    }

    /// Returns whether the player sees `position` right now.
    ///
    /// The field of view reaches [`SIGHT_RADIUS`] cells and is blocked by
    /// walls (see the `fov` module). It is computed when the game starts and
    /// after every action of the player that took time. The player's own
    /// cell is always visible; cells outside the map never are.
    pub fn is_visible(&self, position: Position) -> bool {
        self.map.is_visible(position)
    }

    /// Returns whether the player has seen `position` at least once on this
    /// floor, whether or not it is visible now.
    ///
    /// Every visible cell is remembered, and stays so after the player walks
    /// away. Cells outside the map are never remembered.
    pub fn is_remembered(&self, position: Position) -> bool {
        self.map.is_remembered(position)
    }

    /// Computes what the player sees from its current position and records
    /// it on the map, which also remembers those cells.
    fn update_field_of_view(&mut self) {
        let origin = self.actors[self.player].position;
        let field = fov::visible_cells(&self.map, origin, SIGHT_RADIUS);
        self.map.set_visible(field);
    }

    /// Returns the key of the actor standing at `position`, if any.
    fn actor_at(&self, position: Position) -> Option<ActorId> {
        self.actors
            .iter()
            .find_map(|(id, actor)| (actor.position == position).then_some(id))
    }

    /// Lets non-player actors take their turns, in schedule order, until the
    /// player may act. Their events are appended to `events`.
    fn run_until_player_turn(&mut self, events: &mut Vec<Event>) {
        loop {
            let id = self.scheduler.next_actor(&mut self.actors);
            if id == self.player {
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

    /// Chooses the action of a non-player actor whose turn it is.
    ///
    /// Actors have no behaviour yet, so they always wait. Monster behaviour will
    /// replace this choice.
    const fn non_player_action(&self, _actor: ActorId) -> Action {
        Action::Wait
    }

    /// Applies `action` for actor `id` and appends the resulting event to
    /// `events`.
    ///
    /// On success the actor moves (if the action is a move), spends the
    /// action's energy cost and `true` is returned. A move towards a cell
    /// that is not walkable or holds another actor is refused: the actor
    /// keeps its position and energy, [`Event::MoveBlocked`] is appended and
    /// `false` is returned.
    fn perform(&mut self, id: ActorId, action: Action, events: &mut Vec<Event>) -> bool {
        let from = self.actors[id].position;
        let target = match action {
            Action::Move(direction) => from.step(direction),
            Action::Wait => from,
        };
        let occupied = self.actor_at(target).is_some_and(|other| other != id);
        let outcome = match action::resolve(action, from, &self.map) {
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
                let actor = &mut self.actors[id];
                actor.position = position;
                spend_energy(actor, action);
                events.push(match action {
                    Action::Move(_) => Event::Moved {
                        actor: id,
                        from,
                        to: position,
                    },
                    Action::Wait => Event::Waited { actor: id },
                });
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Direction;
    use crate::map::Tile;
    use crate::path::DijkstraMap;
    use crate::schedule::ACTION_THRESHOLD;

    /// Seed of every test game; the fixed room does not depend on it.
    const SEED: Seed = Seed::new(42);

    /// A new game in the fixed room, the ground of most tests here.
    fn fixed_room_game() -> Game {
        Game::in_fixed_room(SEED)
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
        let fast = game.spawn_actor(Position::new(10, 3), 2 * NORMAL_SPEED);

        // The new actor starts without energy, so it first acts after the
        // player's next tick; the player was created first and keeps the lead.
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
        let fast = game.spawn_actor(Position::new(10, 3), 2 * NORMAL_SPEED);
        let slow = game.spawn_actor(Position::new(12, 3), NORMAL_SPEED / 2);

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
        let fast = game.spawn_actor(Position::new(10, 3), 2 * NORMAL_SPEED);
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
            game.actor(fast).map(Actor::position),
            Some(Position::new(10, 3))
        );
    }

    #[test]
    fn another_actor_blocks_the_player() {
        let mut game = fixed_room_game();
        let player = game.player();
        let east = PLAYER_START.step(Direction::East);
        game.spawn_actor(east, NORMAL_SPEED);
        let ticks = game.ticks();

        let events = game.submit(Action::Move(Direction::East));

        assert_eq!(
            events,
            [Event::MoveBlocked {
                actor: player,
                from: PLAYER_START,
                to: east,
            }]
        );
        assert_eq!(game.player_actor().position(), PLAYER_START);
        assert_eq!(game.ticks(), ticks);
    }

    #[test]
    fn a_new_game_starts_on_the_first_generated_floor() {
        for value in [0, 42, u64::MAX] {
            let seed = Seed::new(value);
            let floor = generation::generate(seed, FIRST_DEPTH);

            let game = Game::new(seed);

            let tiles = |map: &Map| map.positions().map(|p| map.tile(p)).collect::<Vec<_>>();
            assert_eq!(tiles(game.map()), tiles(floor.map()));
            assert_eq!(game.player_actor().position(), floor.start());
            assert_eq!(game.actors().count(), 1);
            assert!(game.player_actor().energy() >= ACTION_THRESHOLD);
        }
    }

    #[test]
    fn the_player_can_walk_onto_the_stairs() {
        let seed = Seed::new(42);
        let mut game = Game::new(seed);
        let floor = generation::generate(seed, FIRST_DEPTH);
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
            let mut game = Game::new(Seed::new(value));
            game.submit(Action::Wait);

            assert_eq!(game.seed(), Seed::new(value));
        }
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
        let fast = game.spawn_actor(Position::new(10, 3), 2 * NORMAL_SPEED);
        let slow = game.spawn_actor(Position::new(12, 3), NORMAL_SPEED / 2);
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
            fov::visible_cells(game.map(), PLAYER_START, SIGHT_RADIUS)
        );
        // Nine cells to the right is beyond the radius of eight.
        assert!(!game.is_visible(Position::new(14, 5)));
        assert!(!game.is_remembered(Position::new(14, 5)));
    }

    #[test]
    fn the_field_of_view_follows_the_player() {
        let mut game = fixed_room_game();
        let far_right = Position::new(14, 5);

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
            fov::visible_cells(game.map(), player, SIGHT_RADIUS)
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
        let game = Game::new(SEED);
        let map = game.map();
        let start = game.player_actor().position();

        let visible: Vec<Position> = map.positions().filter(|&p| game.is_visible(p)).collect();

        assert_eq!(visible, fov::visible_cells(map, start, SIGHT_RADIUS));
        assert!(visible.contains(&start));
        // The rest of the floor lies behind walls or beyond the radius.
        assert!(visible.len() < map.positions().count());
    }

    #[test]
    #[should_panic(expected = "cannot spawn an actor at (5, 5)")]
    fn spawning_on_an_occupied_cell_panics() {
        fixed_room_game().spawn_actor(PLAYER_START, NORMAL_SPEED);
    }
}
