//! Automatic exploration: what `Action::Explore` does to the game.
//!
//! Adds to [`Game`] the private step of automatic exploration. The way is
//! chosen by the `explore` module on what the player remembers of the floor
//! (see `explore::explore_step` for the goals and the tie rule); the step
//! itself is an ordinary move ([`Game::step`]): same cost, same events, and
//! the glyph underfoot is read. Exploring never strikes: cells held by an
//! actor are stepped around, and when every way closer is held the step is
//! refused like a move into a wall ([`Event::MoveBlocked`]). With nothing
//! left to explore within reach it is refused too
//! ([`Event::NothingToExplore`]). A refusal costs no energy and no time.
//!
//! The step adds nothing to the state of the game: a save taken between
//! two steps reads back into a game that explores the same way.

use super::Game;
use crate::actor::ActorId;
use crate::event::Event;
use crate::explore::{self, ExploreStep};

impl Game {
    /// Makes the player `actor` take one step of automatic exploration,
    /// appends the events to `events` and returns whether it took time.
    ///
    /// A step moves the player as `Action::Move` in the direction
    /// `explore::explore_step` chose, for `schedule::MOVE_COST`. When every
    /// way closer is held by an actor, [`Event::MoveBlocked`] towards the
    /// first of them is appended; when no unexplored cell can be reached,
    /// [`Event::NothingToExplore`]; in both cases nothing else happens.
    pub(super) fn explore(&mut self, actor: ActorId, events: &mut Vec<Event>) -> bool {
        debug_assert_eq!(actor, self.world.player, "only the player explores");
        let from = self.world.actors[actor].position;
        let step = explore::explore_step(&self.world.map, from, |cell| {
            self.world
                .actor_at(cell)
                .is_some_and(|other| other != actor)
        });
        match step {
            ExploreStep::Step(direction) => self.step(actor, direction, events),
            ExploreStep::Blocked(direction) => {
                events.push(Event::MoveBlocked {
                    actor,
                    from,
                    to: from.step(direction),
                });
                false
            }
            ExploreStep::Explored => {
                events.push(Event::NothingToExplore { position: from });
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::Action;
    use crate::content::{Content, FloorKind};
    use crate::generation::{self, FIRST_DEPTH, Floor};
    use crate::geometry::{Direction, Position};
    use crate::path::flood_fill;
    use crate::path::test_maps::from_rows;
    use crate::rng::Seed;
    use crate::save;
    use crate::schedule::MOVE_COST;

    /// Seed of the test games.
    const SEED: Seed = Seed::new(29);

    /// Number of seeds whose first floor is explored to the end.
    const EXPLORED_SEEDS: u64 = 50;

    /// Number of seeds whose cave is explored to the end.
    const EXPLORED_CAVES: u64 = 20;

    /// The content shipped with the game.
    fn content() -> Content {
        Content::load().expect("the shipped content is valid")
    }

    /// A game on the map drawn by `rows` (see `path::test_maps::from_rows`)
    /// with the player alone at `start`.
    fn game_on(rows: &[&str], start: Position) -> Game {
        Game::start(SEED, from_rows(rows), start, &[])
    }

    /// A corridor 30 cells long, longer than the player sees, with the
    /// player at its west end: the unknown lies to the east.
    fn long_corridor() -> Game {
        game_on(
            &[
                "################################", //
                "#..............................#", //
                "################################", //
            ],
            Position::new(1, 1),
        )
    }

    /// Submits `Action::Explore` until the game refuses it, and returns
    /// every event and the number of steps taken.
    ///
    /// # Panics
    ///
    /// Panics if exploring takes more than `limit` steps.
    fn explore_to_the_end(game: &mut Game, limit: usize) -> (Vec<Event>, usize) {
        let mut events = Vec::new();
        for steps in 0..=limit {
            let step = game.submit(Action::Explore);
            let refused = step
                .iter()
                .any(|event| matches!(event, Event::NothingToExplore { .. }));
            events.extend(step);
            if refused {
                return (events, steps);
            }
        }
        panic!("still exploring after {limit} steps");
    }

    #[test]
    fn a_step_moves_the_player_towards_the_unknown_like_a_move() {
        let mut game = long_corridor();
        let player = game.player();
        let ticks = game.ticks();
        let seen_before = game.map().positions().filter(|&c| game.is_remembered(c));
        let seen_before = seen_before.count();

        let events = game.submit(Action::Explore);

        assert_eq!(
            events,
            [Event::Moved {
                actor: player,
                from: Position::new(1, 1),
                to: Position::new(2, 1),
            }]
        );
        assert_eq!(game.ticks(), ticks + 1, "a move's worth of time");
        assert_eq!(game.player_turns(), 1);
        let seen_after = game.map().positions().filter(|&c| game.is_remembered(c));
        assert!(seen_after.count() > seen_before, "the step shows more");
    }

    #[test]
    fn a_step_costs_exactly_what_a_move_costs() {
        let mut exploring = long_corridor();
        let mut walking = long_corridor();
        let player = exploring.player();

        exploring.submit(Action::Explore);
        walking.submit(Action::Move(Direction::East));

        assert_eq!(
            exploring.actor(player).map(|a| (a.position(), a.energy())),
            walking.actor(player).map(|a| (a.position(), a.energy())),
        );
        assert_eq!(exploring.ticks(), walking.ticks());
        assert_eq!(crate::schedule::action_cost(Action::Explore), MOVE_COST);
    }

    #[test]
    fn exploring_walks_the_corridor_to_its_end_and_then_stops() {
        let mut game = long_corridor();

        let (events, steps) = explore_to_the_end(&mut game, 100);

        assert_eq!(
            game.player_actor().position(),
            Position::new(23, 1),
            "8 cells short of the end wall, which the player then sees"
        );
        assert_eq!(steps, 22);
        assert_eq!(
            events.last(),
            Some(&Event::NothingToExplore {
                position: Position::new(23, 1)
            })
        );
    }

    #[test]
    fn nothing_to_explore_is_refused_without_any_cost() {
        let mut game = game_on(
            &[
                "#######", //
                "#.....#", //
                "#######", //
            ],
            Position::new(2, 1),
        );
        let energy = game.player_actor().energy();
        let ticks = game.ticks();

        let events = game.submit(Action::Explore);

        assert_eq!(
            events,
            [Event::NothingToExplore {
                position: Position::new(2, 1)
            }]
        );
        assert_eq!(game.player_actor().energy(), energy);
        assert_eq!(game.ticks(), ticks, "no time passed");
        assert_eq!(game.player_turns(), 0);
        assert_eq!(game.stats().turns(), 0);
    }

    #[test]
    fn exploring_never_strikes_a_monster_in_the_way() {
        let content = content();
        let mut game = long_corridor();
        let monster = game.spawn_monster(Position::new(2, 1), &content.monsters()[0]);
        let hp = game.actor(monster).map(|m| m.hp());
        let ticks = game.ticks();

        let events = game.submit(Action::Explore);

        assert_eq!(
            events,
            [Event::MoveBlocked {
                actor: game.player(),
                from: Position::new(1, 1),
                to: Position::new(2, 1),
            }]
        );
        assert_eq!(game.actor(monster).map(|m| m.hp()), hp);
        assert_eq!(game.ticks(), ticks, "no time passed");
    }

    #[test]
    fn the_same_seed_explores_the_same_way() {
        let content = content();
        let play = || {
            let mut game = Game::new(SEED, &content);
            (0..60)
                .flat_map(|_| game.submit(Action::Explore))
                .collect::<Vec<_>>()
        };

        let first = play();

        assert!(
            first
                .iter()
                .any(|event| matches!(event, Event::Moved { .. })),
            "the player explored"
        );
        assert_eq!(first, play());
    }

    /// Explores the first floor of every seed in `seeds` to the end and
    /// checks that every cell the player can walk to was seen, in fewer
    /// steps than there are such cells.
    ///
    /// # Panics
    ///
    /// Panics, naming the seed, if exploring takes more steps or leaves a
    /// reachable cell unseen.
    fn explore_first_floors(seeds: std::ops::Range<u64>) {
        explore_floors(seeds, |seed| generation::generate(seed, FIRST_DEPTH));
    }

    /// Explores the floor `floor_of` builds for every seed in `seeds` to the
    /// end, with the checks of [`explore_first_floors`].
    fn explore_floors(seeds: std::ops::Range<u64>, floor_of: impl Fn(Seed) -> Floor) {
        for seed in seeds {
            let floor = floor_of(Seed::new(seed));
            let mut game = Game::on_floor(Seed::new(seed), floor);
            let reachable = flood_fill(game.map(), game.player_actor().position());

            // Each step walks towards a cell never seen, and sight reaches
            // 8 cells: a floor needs far fewer steps than it has open cells.
            explore_to_the_end(&mut game, reachable.len());

            for cell in reachable {
                assert!(
                    game.is_remembered(cell),
                    "seed {seed}: reachable {cell:?} never seen"
                );
            }
        }
    }

    // The seeds are explored in two halves, which the test harness runs in
    // parallel: a floor takes about a second in a debug build.

    #[test]
    fn generated_floors_are_explored_to_the_end_in_bounded_steps_first_half() {
        explore_first_floors(0..EXPLORED_SEEDS / 2);
    }

    #[test]
    fn generated_floors_are_explored_to_the_end_in_bounded_steps_second_half() {
        explore_first_floors(EXPLORED_SEEDS / 2..EXPLORED_SEEDS);
    }

    /// Explores the cave of every seed in `seeds` to the end, with the
    /// checks of [`explore_first_floors`]: caves are wider open than rooms,
    /// with pillars and dead ends the way must go around.
    fn explore_caves(seeds: std::ops::Range<u64>) {
        explore_floors(seeds, |seed| {
            generation::generate_of_kind(seed, FIRST_DEPTH, FloorKind::Caves)
        });
    }

    #[test]
    fn caves_are_explored_to_the_end_in_bounded_steps_first_half() {
        explore_caves(0..EXPLORED_CAVES / 2);
    }

    #[test]
    fn caves_are_explored_to_the_end_in_bounded_steps_second_half() {
        explore_caves(EXPLORED_CAVES / 2..EXPLORED_CAVES);
    }

    #[test]
    fn a_save_between_two_steps_explores_on_the_same_way() {
        let content = content();
        let mut game = Game::new(SEED, &content);
        for _ in 0..25 {
            game.submit(Action::Explore);
        }
        let text = save::to_string(&game).expect("a game is always written");
        let mut loaded = save::from_str(&text, &content).expect("the save reads back");

        for turn in 0..60 {
            assert_eq!(
                game.submit(Action::Explore),
                loaded.submit(Action::Explore),
                "step {turn} after the load"
            );
        }
        assert_eq!(
            save::to_string(&game).expect("written"),
            save::to_string(&loaded).expect("written")
        );
    }

    #[test]
    fn an_explored_floor_stays_explored() {
        let mut game = Game::on_floor(SEED, generation::generate(SEED, FIRST_DEPTH));
        let cells = game.map().width() * game.map().height();
        explore_to_the_end(&mut game, cells);
        let here = game.player_actor().position();
        let ticks = game.ticks();

        for _ in 0..3 {
            assert_eq!(
                game.submit(Action::Explore),
                [Event::NothingToExplore { position: here }]
            );
        }
        assert_eq!(game.ticks(), ticks);
    }
}
