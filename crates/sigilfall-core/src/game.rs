//! A game in progress and its single entry point, [`Game::submit`].
//!
//! Exposes [`Game`] and [`PLAYER_START`]. The game owns the map, the actors
//! and the scheduler. The player's action is applied, then the other actors
//! take their turns until the player may act again, and everything that
//! happened is returned as [`Event`]s. The game produces no text and draws
//! nothing.

use slotmap::SlotMap;

use crate::action::{self, Action, ActionOutcome};
use crate::actor::{Actor, ActorId};
use crate::event::Event;
use crate::geometry::Position;
use crate::map::Map;
use crate::schedule::{NORMAL_SPEED, Scheduler, spend_energy};

/// Cell of the fixed room where the player starts.
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
    /// The current dungeon floor.
    map: Map,
    /// Every actor, the player included.
    actors: SlotMap<ActorId, Actor>,
    /// Key of the player in `actors`.
    player: ActorId,
    /// Turn order and elapsed time.
    scheduler: Scheduler,
}

impl Game {
    /// Starts a game on the fixed room, with the player at [`PLAYER_START`]
    /// moving at normal speed and no other actor.
    ///
    /// Time is advanced until the player's first turn, so [`Game::submit`]
    /// can be called right away.
    pub fn new() -> Self {
        let mut actors = SlotMap::with_key();
        let player = actors.insert(Actor::new(PLAYER_START, NORMAL_SPEED));
        let mut scheduler = Scheduler::default();
        scheduler.enrol(player);

        let mut game = Self {
            map: Map::fixed_room(),
            actors,
            player,
            scheduler,
        };
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
    /// the player's next turn.
    pub fn submit(&mut self, action: Action) -> Vec<Event> {
        let mut events = Vec::new();
        if self.perform(self.player, action, &mut events) {
            self.run_until_player_turn(&mut events);
        }
        events
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
    pub const fn ticks(&self) -> u64 {
        self.scheduler.ticks()
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

impl Default for Game {
    /// Same as [`Game::new`].
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Direction;
    use crate::schedule::ACTION_THRESHOLD;

    /// Energy of every actor, keyed for comparison before and after a turn.
    fn energies(game: &Game) -> Vec<(ActorId, i32)> {
        game.actors().map(|(id, a)| (id, a.energy())).collect()
    }

    #[test]
    fn player_starts_on_open_floor_and_may_act() {
        let game = Game::new();
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
        let mut game = Game::new();
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
        let mut game = Game::new();
        let player = game.player();
        let ticks = game.ticks();

        let events = game.submit(Action::Wait);

        assert_eq!(events, [Event::Waited { actor: player }]);
        assert_eq!(game.player_actor().position(), PLAYER_START);
        assert_eq!(game.ticks(), ticks + 1);
    }

    #[test]
    fn other_actors_act_before_the_player_turn_returns() {
        let mut game = Game::new();
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
        let mut game = Game::new();
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
        let mut game = Game::new();
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
        let mut game = Game::new();
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
    #[should_panic(expected = "cannot spawn an actor at (5, 5)")]
    fn spawning_on_an_occupied_cell_panics() {
        Game::new().spawn_actor(PLAYER_START, NORMAL_SPEED);
    }
}
