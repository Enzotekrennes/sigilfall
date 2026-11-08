//! Energy-based turn order: who acts next, and what acting costs.
//!
//! Time advances in ticks. On every tick each actor gains its speed in
//! energy; an actor whose energy reaches [`ACTION_THRESHOLD`] may act, and
//! acting spends the cost of the action ([`action_cost`]). A fast actor thus
//! acts more often than a slow one, in proportion to their speeds.
//!
//! Exposes the energy constants, [`action_cost`], [`spend_energy`] and
//! [`Scheduler`]. This module does not decide what an actor does nor apply
//! actions: the `game` module does.

use slotmap::SlotMap;

use crate::action::Action;
use crate::actor::{Actor, ActorId};

/// Energy an actor needs before it may act.
///
/// 100 matches [`NORMAL_SPEED`] and the normal action cost, so an actor of
/// normal speed acts exactly once per tick and speeds read as percentages.
pub const ACTION_THRESHOLD: i32 = 100;

/// Speed of the player and of ordinary creatures: one normal action per tick.
pub const NORMAL_SPEED: i32 = 100;

/// Energy spent by stepping one cell.
///
/// The normal cost: moving takes one turn of an actor at normal speed.
pub const MOVE_COST: i32 = 100;

/// Energy spent by waiting in place.
///
/// The normal cost, so that waiting lets exactly one turn of a normal-speed
/// creature pass.
pub const WAIT_COST: i32 = 100;

/// Energy spent by striking a foe on a neighbouring cell.
///
/// The normal cost, so a blow takes as long as the step it replaces: a
/// creature of speed 200 strikes twice per normal action of the player.
pub const ATTACK_COST: i32 = 100;

/// Returns the energy `action` costs when it is performed as such.
///
/// A move into a foe is not a step but a blow, which costs [`ATTACK_COST`]
/// instead; the `game` module, which finds the foe, charges that cost. A
/// blocked action costs nothing; that rule is applied by the caller, which
/// simply does not call [`spend_energy`] for it.
pub const fn action_cost(action: Action) -> i32 {
    match action {
        Action::Move(_) => MOVE_COST,
        Action::Wait => WAIT_COST,
    }
}

/// Charges `actor` `cost` energy for what it just did: [`action_cost`] of
/// its action, or [`ATTACK_COST`] for a blow.
///
/// The actor's energy may become negative when the cost exceeds what it had
/// stored; it then waits longer before its next turn.
pub fn spend_energy(actor: &mut Actor, cost: i32) {
    actor.energy -= cost;
}

/// Keeps the turn order of the actors and the time elapsed.
///
/// Turn order is the order in which actors were enrolled, which is their
/// creation order. It breaks ties: when several actors may act, the one
/// enrolled first goes first. Iterating a `SlotMap` would not give creation
/// order once slots are reused, hence the separate list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scheduler {
    /// Scheduled actors, in creation order.
    turn_order: Vec<ActorId>,
    /// Ticks elapsed since the scheduler was created.
    ticks: u64,
}

impl Scheduler {
    /// Adds `actor` at the end of the turn order, behind every actor enrolled
    /// before it.
    pub fn enrol(&mut self, actor: ActorId) {
        self.turn_order.push(actor);
    }

    /// Takes `actor` out of the turn order, for good; the others keep their
    /// order. Nothing happens if it was not enrolled.
    ///
    /// Called when an actor leaves the game, such as a monster that dies, so
    /// that [`Scheduler::next_actor`] never looks for it again.
    pub fn remove(&mut self, actor: ActorId) {
        self.turn_order.retain(|&id| id != actor);
    }

    /// Returns whether `actor` is in the turn order.
    pub fn is_enrolled(&self, actor: ActorId) -> bool {
        self.turn_order.contains(&actor)
    }

    /// Returns the number of ticks elapsed since the scheduler was created.
    ///
    /// Only [`Scheduler::next_actor`] advances it; performing or refusing an
    /// action does not.
    pub const fn ticks(&self) -> u64 {
        self.ticks
    }

    /// Returns the actor whose turn it is, advancing time until one can act.
    ///
    /// The first actor in turn order whose energy has reached
    /// [`ACTION_THRESHOLD`] is returned. While none has, a tick passes and
    /// every actor gains its speed in energy. The caller must make the
    /// returned actor act and spend energy (or otherwise lower its energy),
    /// or the same actor is returned again.
    ///
    /// # Panics
    ///
    /// Panics if no actor is enrolled, or if an enrolled actor is missing
    /// from `actors`: both are bugs in the code managing actors, and the first
    /// would otherwise loop forever.
    pub fn next_actor(&mut self, actors: &mut SlotMap<ActorId, Actor>) -> ActorId {
        assert!(
            !self.turn_order.is_empty(),
            "cannot pick the next actor: no actor is scheduled"
        );
        loop {
            if let Some(ready) = self.first_ready(actors) {
                return ready;
            }
            self.tick(actors);
        }
    }

    /// Returns the first actor in turn order with enough energy to act, if
    /// any.
    fn first_ready(&self, actors: &SlotMap<ActorId, Actor>) -> Option<ActorId> {
        self.turn_order
            .iter()
            .copied()
            .find(|&id| scheduled(actors, id).energy >= ACTION_THRESHOLD)
    }

    /// Advances time by one tick: every scheduled actor gains its speed in
    /// energy.
    fn tick(&mut self, actors: &mut SlotMap<ActorId, Actor>) {
        for &id in &self.turn_order {
            let actor = scheduled_mut(actors, id);
            actor.energy += actor.speed;
        }
        self.ticks += 1;
    }
}

/// Looks up a scheduled actor, panicking with context if it no longer exists.
fn scheduled(actors: &SlotMap<ActorId, Actor>, id: ActorId) -> &Actor {
    actors
        .get(id)
        .unwrap_or_else(|| panic!("scheduled actor {id:?} is missing from the actor map"))
}

/// Mutable counterpart of [`scheduled`].
fn scheduled_mut(actors: &mut SlotMap<ActorId, Actor>, id: ActorId) -> &mut Actor {
    actors
        .get_mut(id)
        .unwrap_or_else(|| panic!("scheduled actor {id:?} is missing from the actor map"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::ActorKind;
    use crate::combat::PLAYER_STATS;
    use crate::geometry::{Direction, Position};

    /// Creates an actor moving at `speed`; its other values do not matter
    /// to the schedule.
    fn actor(speed: i32) -> Actor {
        Actor::new(Position::new(1, 1), speed, PLAYER_STATS, ActorKind::Player)
    }

    /// Creates an actor of the given speed, stores it and enrols it.
    fn add(actors: &mut SlotMap<ActorId, Actor>, scheduler: &mut Scheduler, speed: i32) -> ActorId {
        let id = actors.insert(actor(speed));
        scheduler.enrol(id);
        id
    }

    /// Lets every actor act with a normal-cost action until `ticks` ticks
    /// have elapsed, returning the actors in the order they acted.
    fn run_for(
        actors: &mut SlotMap<ActorId, Actor>,
        scheduler: &mut Scheduler,
        ticks: u64,
    ) -> Vec<ActorId> {
        let mut turns = Vec::new();
        loop {
            let id = scheduler.next_actor(actors);
            if scheduler.ticks() > ticks {
                return turns;
            }
            spend_energy(&mut actors[id], action_cost(Action::Wait));
            turns.push(id);
        }
    }

    #[test]
    fn double_speed_acts_twice_as_often() {
        let mut actors = SlotMap::with_key();
        let mut scheduler = Scheduler::default();
        let normal = add(&mut actors, &mut scheduler, NORMAL_SPEED);
        let fast = add(&mut actors, &mut scheduler, 2 * NORMAL_SPEED);

        let turns = run_for(&mut actors, &mut scheduler, 50);

        let count = |id| turns.iter().filter(|&&t| t == id).count();
        assert_eq!(count(normal), 50);
        assert_eq!(count(fast), 100);
    }

    #[test]
    fn slower_actor_skips_ticks() {
        let mut actors = SlotMap::with_key();
        let mut scheduler = Scheduler::default();
        let slow = add(&mut actors, &mut scheduler, NORMAL_SPEED / 2);

        let turns = run_for(&mut actors, &mut scheduler, 10);

        assert_eq!(turns, vec![slow; 5]);
    }

    #[test]
    fn ties_go_to_the_actor_created_first() {
        let mut actors = SlotMap::with_key();
        let mut scheduler = Scheduler::default();
        let first = add(&mut actors, &mut scheduler, NORMAL_SPEED);
        let second = add(&mut actors, &mut scheduler, NORMAL_SPEED);
        let third = add(&mut actors, &mut scheduler, NORMAL_SPEED);

        let turns = run_for(&mut actors, &mut scheduler, 3);

        assert_eq!(
            turns,
            [
                first, second, third, first, second, third, first, second, third
            ]
        );
    }

    #[test]
    fn creation_order_wins_even_when_slots_are_reused() {
        let mut actors = SlotMap::with_key();
        let mut scheduler = Scheduler::default();
        // Free the first slot so the next insertion reuses it: slot order
        // and creation order now disagree.
        let removed = actors.insert(actor(NORMAL_SPEED));
        let older = add(&mut actors, &mut scheduler, NORMAL_SPEED);
        actors.remove(removed);
        let newer = add(&mut actors, &mut scheduler, NORMAL_SPEED);
        assert_eq!(actors.keys().next(), Some(newer), "slot reused first");

        let turns = run_for(&mut actors, &mut scheduler, 1);

        assert_eq!(turns, [older, newer]);
    }

    #[test]
    fn next_actor_waits_until_someone_is_ready() {
        let mut actors = SlotMap::with_key();
        let mut scheduler = Scheduler::default();
        let slow = add(&mut actors, &mut scheduler, 25);

        assert_eq!(scheduler.next_actor(&mut actors), slow);
        assert_eq!(scheduler.ticks(), 4);
        // Without spending energy, the same actor stays ready and no time
        // passes.
        assert_eq!(scheduler.next_actor(&mut actors), slow);
        assert_eq!(scheduler.ticks(), 4);
    }

    #[test]
    fn a_removed_actor_never_acts_again_and_the_others_keep_their_order() {
        let mut actors = SlotMap::with_key();
        let mut scheduler = Scheduler::default();
        let first = add(&mut actors, &mut scheduler, NORMAL_SPEED);
        let second = add(&mut actors, &mut scheduler, NORMAL_SPEED);
        let third = add(&mut actors, &mut scheduler, NORMAL_SPEED);

        scheduler.remove(second);
        actors.remove(second);

        assert!(!scheduler.is_enrolled(second));
        assert!(scheduler.is_enrolled(first) && scheduler.is_enrolled(third));
        let turns = run_for(&mut actors, &mut scheduler, 2);
        assert_eq!(turns, [first, third, first, third]);
    }

    #[test]
    fn normal_actions_and_blows_cost_the_threshold() {
        assert_eq!(action_cost(Action::Wait), ACTION_THRESHOLD);
        assert_eq!(
            action_cost(Action::Move(Direction::North)),
            ACTION_THRESHOLD
        );
        assert_eq!(ATTACK_COST, ACTION_THRESHOLD);

        let mut actor = actor(NORMAL_SPEED);
        spend_energy(&mut actor, action_cost(Action::Move(Direction::East)));
        assert_eq!(actor.energy(), -MOVE_COST);
        spend_energy(&mut actor, ATTACK_COST);
        assert_eq!(actor.energy(), -MOVE_COST - ATTACK_COST);
    }

    #[test]
    #[should_panic(expected = "no actor is scheduled")]
    fn empty_schedule_panics_instead_of_looping() {
        Scheduler::default().next_actor(&mut SlotMap::with_key());
    }
}
