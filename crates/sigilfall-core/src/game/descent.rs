//! Going down the stairs, or breaking the seal of the bottom floor: what
//! `Action::Descend` does to the game.
//!
//! Adds to [`Game`] the private step of the descent. Only the player
//! descends, and only from the cell of the stairs; anywhere else the action
//! is refused like a move into a wall ([`Event::NoStairsHere`]), with no
//! energy and no time spent.
//!
//! The bottom floor has no stairs but a seal on the cell they would have
//! (decision D23, `generation::is_bottom_floor`). The same action taken on
//! the seal breaks it: the run is won ([`Status::Won`]) and the game is
//! over, as after the player's death. Away from the seal it is refused
//! exactly as away from the stairs.
//!
//! A descent replaces the whole floor: the map, the monsters, the glyphs and
//! the items lying on the floor are those of the next depth of the seed
//! (`generation::generate_with_monsters`, or a bare `generation::generate`
//! for a game without content), and the player arrives on its start cell.
//! The player keeps everything that is its own: hit points, essence,
//! inventory, grimoire, steadied casts, and its key. The gameplay stream
//! restarts as the stream of the new depth (decision D5), so a floor plays
//! out the same for a given seed whatever happened on the floors above.

use super::{Game, Status};
use crate::action::Action;
use crate::actor::ActorId;
use crate::event::Event;
use crate::generation::{self, Floor};
use crate::map::Tile;
use crate::rng::Purpose;
use crate::schedule::{action_cost, spend_energy};

impl Game {
    /// Makes the player `actor` go down the stairs it stands on, or break
    /// the seal it stands on, appends the events to `events` and returns
    /// whether it took time.
    ///
    /// Off the stairs and the seal, [`Event::NoStairsHere`] is appended and
    /// nothing else happens. On the seal, see [`Game::break_seal`]. On the
    /// stairs, the player spends `schedule::DESCEND_COST`, the depth grows
    /// by one, the next floor replaces the current one (see
    /// [`Game::enter_floor`]), the gameplay stream becomes that of the new
    /// depth and [`Event::Descended`] is appended.
    pub(super) fn descend(&mut self, actor: ActorId, events: &mut Vec<Event>) -> bool {
        debug_assert_eq!(actor, self.world.player, "only the player descends");
        let position = self.world.actors[actor].position;
        match self.world.map.tile(position) {
            Some(Tile::StairsDown) => {}
            Some(Tile::Seal) => {
                self.break_seal(actor, events);
                return true;
            }
            Some(Tile::Floor | Tile::Wall) | None => {
                events.push(Event::NoStairsHere { position });
                return false;
            }
        }
        spend_energy(&mut self.world.actors[actor], action_cost(Action::Descend));
        self.depth = self.depth.saturating_add(1);
        let floor = match &self.content {
            Some(content) => generation::generate_with_monsters(self.seed, self.depth, content),
            None => generation::generate(self.seed, self.depth),
        };
        self.enter_floor(floor);
        self.gameplay_rng = self.seed.stream(self.depth, Purpose::Gameplay);
        events.push(Event::Descended { depth: self.depth });
        true
    }

    /// Makes the player `actor`, standing on the seal, break it: it spends
    /// `schedule::DESCEND_COST` like a descent, the game is won
    /// ([`Status::Won`]) and [`Event::SealBroken`] is appended.
    ///
    /// The floor, the depth and the gameplay stream stay as they are. Since
    /// the game is over, no other actor acts afterwards and later actions
    /// are ignored (`Game::submit`).
    fn break_seal(&mut self, actor: ActorId, events: &mut Vec<Event>) {
        let position = self.world.actors[actor].position;
        spend_energy(&mut self.world.actors[actor], action_cost(Action::Descend));
        self.status = Status::Won;
        events.push(Event::SealBroken { position });
    }

    /// Replaces the current floor by `floor`: the monsters and floor items
    /// left behind leave the game and the turn order, the player stands on
    /// the floor's start, and the floor's monsters, glyphs and items are
    /// placed as on a new game.
    ///
    /// The new monsters join the turn order behind the player, in the order
    /// they were drawn, each with its head start below the action
    /// threshold, so none of them acts before the player's first turn on the
    /// floor. The field of view is the caller's to update.
    fn enter_floor(&mut self, floor: Floor) {
        for left_behind in self.world.change_floor(floor.map, floor.start) {
            self.scheduler.remove(left_behind);
        }
        self.add_floor_monsters(&floor.monsters);
        self.lay_out_floor_contents(floor.glyphs, floor.items);
    }
}

#[cfg(test)]
mod tests {
    use rand_chacha::ChaCha8Rng;

    use super::*;
    use crate::content::{Content, MAX_DEPTH};
    use crate::game::Status;
    use crate::generation::{FIRST_DEPTH, is_bottom_floor, monsters_per_floor};
    use crate::geometry::{Direction, Position};
    use crate::item::ItemId;
    use crate::rng::Seed;
    use crate::save;
    use crate::schedule::{DESCEND_COST, NORMAL_SPEED};

    /// Seed of the test games.
    const SEED: Seed = Seed::new(42);

    /// Deepest floor the tests walk down to.
    const DEEPEST_WALKED: u32 = 6;

    /// The content shipped with the game.
    fn content() -> Content {
        Content::load().expect("the shipped content is valid")
    }

    /// Cell of the stairs of the current floor.
    fn stairs(game: &Game) -> Position {
        game.map()
            .positions()
            .find(|&cell| game.map().tile(cell) == Some(Tile::StairsDown))
            .expect("every generated floor has stairs")
    }

    /// Puts the player on the stairs of its floor, as if it had walked
    /// there: generated monsters never stand on the stairs.
    fn stand_on_stairs(game: &mut Game) {
        let cell = stairs(game);
        assert!(game.world.actor_at(cell).is_none(), "the stairs are free");
        let player = game.world.player;
        game.world.actors[player].position = cell;
    }

    /// What a floor holds, as a player could observe it, keyed by nothing
    /// that depends on what happened above: tiles, the player's cell,
    /// monsters (cell, kind, hit points, energy), glyphs, and floor items
    /// (cell, kind). Monsters and items are sorted, since the order of the
    /// game's lists depends on which storage slots earlier floors freed.
    type FloorState = (
        Vec<Option<Tile>>,
        Position,
        Vec<(Position, String, i32, i32)>,
        Vec<(Position, String)>,
        Vec<(Position, String)>,
    );

    /// Returns the state of the current floor of `game` (see [`FloorState`]).
    fn floor_state(game: &Game) -> FloorState {
        let map = game.map();
        let tiles = map.positions().map(|cell| map.tile(cell)).collect();
        let monsters = game
            .actors()
            .filter_map(|(_, actor)| {
                let monster = actor.as_monster()?;
                Some((
                    actor.position(),
                    monster.definition().to_owned(),
                    actor.hp(),
                    actor.energy(),
                ))
            })
            .collect();
        let glyphs = game
            .engravings()
            .iter()
            .map(|glyph| (glyph.position(), glyph.sigil().id().to_owned()))
            .collect();
        let items = game
            .world
            .items()
            .filter_map(|(_, item)| Some((item.position()?, item.definition().id().to_owned())))
            .collect();
        sorted((
            tiles,
            game.player_actor().position(),
            monsters,
            glyphs,
            items,
        ))
    }

    /// The state `floor` gives right after the player's arrival, one tick
    /// having passed for its monsters (see [`FloorState`]).
    fn arrival_state(floor: &Floor) -> FloorState {
        let tiles = floor
            .map()
            .positions()
            .map(|cell| floor.map().tile(cell))
            .collect();
        let monsters = floor
            .monsters()
            .iter()
            .map(|spawn| {
                let definition = spawn.definition();
                (
                    spawn.position(),
                    definition.id().to_owned(),
                    definition.hp(),
                    spawn.energy() + definition.speed(),
                )
            })
            .collect();
        let glyphs = floor
            .glyphs()
            .iter()
            .map(|glyph| (glyph.position(), glyph.sigil().id().to_owned()))
            .collect();
        let items = floor
            .items()
            .iter()
            .map(|spawn| (spawn.position(), spawn.definition().id().to_owned()))
            .collect();
        sorted((tiles, floor.start(), monsters, glyphs, items))
    }

    /// Returns `state` with its monsters and items sorted by cell.
    fn sorted(mut state: FloorState) -> FloorState {
        state
            .2
            .sort_unstable_by_key(|monster| (monster.0.y, monster.0.x));
        state.4.sort_unstable_by_key(|item| (item.0.y, item.0.x));
        state
    }

    /// Places an item of kind `id` under the player and picks it up.
    fn carry(game: &mut Game, content: &Content, id: &str) -> ItemId {
        let item = game.place_item(
            game.player_actor().position(),
            content.item(id).expect("a shipped item"),
        );
        let events = game.submit(Action::PickUp);
        assert!(
            matches!(events.first(), Some(Event::PickedUp { .. })),
            "{events:?}"
        );
        item
    }

    #[test]
    fn descending_from_the_stairs_goes_one_floor_deeper_and_keeps_the_player() {
        let content = content();
        let mut game = Game::new(SEED, &content);
        let player = game.player();
        let draught = carry(&mut game, &content, "healing_draught");
        let ember = content.sigil("ember").expect("a shipped sigil");
        game.grimoire.learn(ember);
        game.steady_casts = 2;
        let floor_items: Vec<ItemId> = game.world.items().map(|(id, _)| id).collect();
        let monsters_above: Vec<ActorId> = game
            .actors()
            .filter(|(id, _)| *id != player)
            .map(|(id, _)| id)
            .collect();
        {
            let hurt = &mut game.world.actors[player];
            hurt.take_damage(7);
            hurt.set_essence(1);
        }
        // No essence is due before the descent, so any change would be the
        // descent's doing.
        game.essence_since = game.ticks();
        stand_on_stairs(&mut game);
        let (hp, turns, ticks) = (game.player_actor().hp(), game.player_turns(), game.ticks());
        let energy = game.player_actor().energy();

        let events = game.submit(Action::Descend);

        assert_eq!(
            events,
            [Event::Descended {
                depth: FIRST_DEPTH + 1
            }],
            "no monster of the new floor acts before the player"
        );
        assert_eq!(game.depth(), FIRST_DEPTH + 1);
        assert_eq!(game.player(), player, "the same player");
        assert_eq!(game.player_actor().hp(), hp);
        assert_eq!(game.player_actor().essence(), 1);
        assert_eq!(
            game.inventory().map(|(id, _)| id).collect::<Vec<_>>(),
            [draught]
        );
        assert!(game.grimoire().knows("ember"));
        assert_eq!(game.steady_casts(), 2);
        assert_eq!(game.player_turns(), turns + 1);
        assert_eq!(game.ticks(), ticks + 1, "a descent takes one action");
        assert_eq!(
            game.player_actor().energy(),
            energy - DESCEND_COST + NORMAL_SPEED,
            "the descent cost its energy, then one tick passed"
        );
        assert_eq!(game.status(), &Status::Playing);
        for id in monsters_above {
            assert!(game.actor(id).is_none(), "{id:?} was left behind");
        }
        for id in floor_items.into_iter().filter(|&id| id != draught) {
            assert!(game.item(id).is_none(), "{id:?} was left behind");
        }

        let below = generation::generate_with_monsters(SEED, FIRST_DEPTH + 1, &content);
        assert_eq!(floor_state(&game), arrival_state(&below));
        assert_eq!(game.actors().count(), 1 + below.monsters().len());
        assert!(
            game.is_visible(below.start()),
            "the view is that of the new floor"
        );
    }

    #[test]
    fn every_floor_down_is_the_floor_of_the_seed_at_that_depth() {
        let content = content();
        let mut game = Game::new(SEED, &content);
        let mut walk = Game::new(SEED, &content);

        for depth in FIRST_DEPTH + 1..=DEEPEST_WALKED {
            stand_on_stairs(&mut game);
            stand_on_stairs(&mut walk);
            assert_eq!(game.submit(Action::Descend), [Event::Descended { depth }]);
            walk.submit(Action::Descend);

            let floor = generation::generate_with_monsters(SEED, depth, &content);
            assert_eq!(game.depth(), depth);
            assert_eq!(floor_state(&game), arrival_state(&floor), "depth {depth}");
            assert_eq!(
                game.actors().count() - 1,
                monsters_per_floor(depth),
                "depth {depth}"
            );
            assert_eq!(floor_state(&walk), floor_state(&game), "depth {depth}");
            assert_eq!(game.gameplay_rng, SEED.stream(depth, Purpose::Gameplay));
        }
    }

    /// Plays the first floor of [`SEED`] in a way of its own: fights a rat
    /// put next to the player to its death, then waits, drawing from the
    /// gameplay stream and letting time and essence move on.
    fn play_the_first_floor_differently(game: &mut Game, content: &Content) {
        let rat = content.monster("ossuary_rat").expect("a shipped monster");
        let east = game.player_actor().position().step(Direction::East);
        let cell = if game.map().is_walkable(east) {
            east
        } else {
            game.player_actor().position().step(Direction::West)
        };
        let direction = if cell == east {
            Direction::East
        } else {
            Direction::West
        };
        let foe = game.spawn_monster(cell, rat);
        for _ in 0..20 {
            if game.actor(foe).is_none() {
                break;
            }
            game.submit(Action::Move(direction));
        }
        assert!(game.actor(foe).is_none(), "the rat died");
        for _ in 0..15 {
            game.submit(Action::Wait);
        }
        assert!(!game.is_over());
    }

    #[test]
    fn the_second_floor_does_not_depend_on_how_the_first_was_played() {
        let content = content();
        let mut straight = Game::new(SEED, &content);
        let mut detour = Game::new(SEED, &content);
        play_the_first_floor_differently(&mut detour, &content);
        assert_ne!(
            straight.gameplay_rng, detour.gameplay_rng,
            "the first floors drew differently"
        );
        assert_ne!(straight.ticks(), detour.ticks());

        stand_on_stairs(&mut straight);
        stand_on_stairs(&mut detour);
        straight.submit(Action::Descend);
        detour.submit(Action::Descend);

        assert_eq!(floor_state(&straight), floor_state(&detour));
        assert_eq!(straight.gameplay_rng, detour.gameplay_rng);
        let fresh: ChaCha8Rng = SEED.stream(FIRST_DEPTH + 1, Purpose::Gameplay);
        assert_eq!(straight.gameplay_rng, fresh);
    }

    #[test]
    fn descending_off_the_stairs_is_refused_without_cost() {
        let content = content();
        let mut game = Game::new(SEED, &content);
        let position = game.player_actor().position();
        assert_ne!(game.map().tile(position), Some(Tile::StairsDown));
        let energies: Vec<(ActorId, i32)> = game
            .actors()
            .map(|(id, actor)| (id, actor.energy()))
            .collect();
        let before = floor_state(&game);
        let ticks = game.ticks();

        let events = game.submit(Action::Descend);

        assert_eq!(events, [Event::NoStairsHere { position }]);
        assert_eq!(game.depth(), FIRST_DEPTH);
        assert_eq!(game.ticks(), ticks, "no time passed");
        assert_eq!(game.player_turns(), 0, "the refusal is not a turn");
        let after: Vec<(ActorId, i32)> = game
            .actors()
            .map(|(id, actor)| (id, actor.energy()))
            .collect();
        assert_eq!(after, energies, "nobody spent or gained energy");
        assert_eq!(floor_state(&game), before, "the floor is unchanged");
    }

    #[test]
    fn a_game_without_content_descends_to_bare_floors() {
        let mut game = Game::on_floor(SEED, generation::generate(SEED, FIRST_DEPTH));
        stand_on_stairs(&mut game);

        assert_eq!(
            game.submit(Action::Descend),
            [Event::Descended {
                depth: FIRST_DEPTH + 1
            }]
        );

        let bare = generation::generate(SEED, FIRST_DEPTH + 1);
        assert_eq!(floor_state(&game), arrival_state(&bare));
        assert_eq!(game.actors().count(), 1, "the player alone");
    }

    #[test]
    fn the_fixed_room_has_no_stairs_to_take() {
        let mut game = Game::in_fixed_room(SEED);
        let position = game.player_actor().position();

        assert_eq!(
            game.submit(Action::Descend),
            [Event::NoStairsHere { position }]
        );
        assert_eq!(game.depth(), FIRST_DEPTH);
    }

    /// Takes the stairs of every floor of `game` until it stands on floor
    /// `depth`, putting the player on the stairs before each descent.
    fn go_down_to(game: &mut Game, depth: u32) {
        while game.depth() < depth {
            stand_on_stairs(game);
            let below = game.depth() + 1;
            assert_eq!(
                game.submit(Action::Descend),
                [Event::Descended { depth: below }]
            );
        }
    }

    /// Cell of the seal of the current floor, if it has one.
    fn seal(game: &Game) -> Option<Position> {
        game.map()
            .positions()
            .find(|&cell| game.map().tile(cell) == Some(Tile::Seal))
    }

    /// Puts the player of `game` on `cell`, as if it had walked there.
    fn stand_on(game: &mut Game, cell: Position) {
        assert!(game.world.actor_at(cell).is_none(), "{cell:?} is free");
        let player = game.world.player;
        game.world.actors[player].position = cell;
    }

    /// A game of [`SEED`] with the shipped content, its player standing on
    /// the seal of the bottom floor.
    fn on_the_seal(content: &Content) -> Game {
        let mut game = Game::new(SEED, content);
        go_down_to(&mut game, MAX_DEPTH);
        let cell = seal(&game).expect("the bottom floor has a seal");
        stand_on(&mut game, cell);
        game
    }

    /// Writes `game` as a save.
    fn saved(game: &Game) -> String {
        save::to_string(game).expect("a game is always written")
    }

    #[test]
    fn the_bottom_floor_has_a_seal_where_its_stairs_would_be_and_no_stairs() {
        let content = content();
        let mut game = Game::new(SEED, &content);
        for depth in FIRST_DEPTH..MAX_DEPTH {
            go_down_to(&mut game, depth);
            assert!(!is_bottom_floor(depth));
            assert_eq!(seal(&game), None, "floor {depth} has no seal");
        }

        go_down_to(&mut game, MAX_DEPTH);

        assert!(is_bottom_floor(MAX_DEPTH));
        let floor = generation::generate_with_monsters(SEED, MAX_DEPTH, &content);
        assert_eq!(seal(&game), Some(floor.stairs()));
        assert!(
            game.map()
                .positions()
                .all(|cell| game.map().tile(cell) != Some(Tile::StairsDown)),
            "the bottom floor has no stairs"
        );
        assert_eq!(floor_state(&game), arrival_state(&floor));
    }

    #[test]
    fn breaking_the_seal_wins_the_run_and_ends_the_game() {
        let content = content();
        let mut game = on_the_seal(&content);
        let player = game.player();
        let position = game.player_actor().position();
        let (energy, ticks, turns) = (
            game.player_actor().energy(),
            game.ticks(),
            game.player_turns(),
        );
        let monsters = |game: &Game| -> Vec<(ActorId, Position, i32)> {
            game.actors()
                .filter(|(id, _)| *id != player)
                .map(|(id, actor)| (id, actor.position(), actor.energy()))
                .collect()
        };
        let before = monsters(&game);
        assert!(!before.is_empty(), "monsters wait on the bottom floor");

        let events = game.submit(Action::Descend);

        assert_eq!(events, [Event::SealBroken { position }]);
        assert_eq!(game.status(), &Status::Won);
        assert!(game.is_over());
        assert_eq!(game.depth(), MAX_DEPTH, "nothing lies below");
        assert_eq!(game.player_actor().position(), position);
        assert!(game.player_actor().is_alive());
        assert_eq!(game.player_actor().energy(), energy - DESCEND_COST);
        assert_eq!(game.player_turns(), turns + 1, "breaking it is an action");
        assert_eq!(game.ticks(), ticks, "no time passed after it");
        assert_eq!(monsters(&game), before, "no monster acted after it");
        assert_eq!(game.stats().deepest(), MAX_DEPTH);
    }

    #[test]
    fn no_action_is_taken_once_the_seal_is_broken() {
        let content = content();
        let mut game = on_the_seal(&content);
        game.submit(Action::Descend);
        let text = saved(&game);
        let actions = [
            Action::Descend,
            Action::Wait,
            Action::Explore,
            Action::PickUp,
            Action::Move(Direction::East),
            Action::Move(Direction::West),
        ];

        for action in actions {
            assert_eq!(game.submit(action), [], "{action:?}");
        }

        assert_eq!(game.status(), &Status::Won);
        assert_eq!(saved(&game), text, "nothing changed");
    }

    #[test]
    fn breaking_the_seal_away_from_it_is_refused_without_cost() {
        let content = content();
        let mut game = Game::new(SEED, &content);
        go_down_to(&mut game, MAX_DEPTH);
        let position = game.player_actor().position();
        assert_ne!(seal(&game), Some(position));
        let before = saved(&game);

        let events = game.submit(Action::Descend);

        assert_eq!(events, [Event::NoStairsHere { position }]);
        assert_eq!(game.status(), &Status::Playing);
        assert_eq!(saved(&game), before, "no time, no energy, nothing changed");
    }

    #[test]
    fn a_game_without_content_also_ends_on_a_seal() {
        let mut game = Game::on_floor(SEED, generation::generate(SEED, FIRST_DEPTH));
        go_down_to(&mut game, MAX_DEPTH);
        let cell = seal(&game).expect("the bare bottom floor has a seal");
        assert_eq!(cell, generation::generate(SEED, MAX_DEPTH).stairs());
        stand_on(&mut game, cell);

        assert_eq!(
            game.submit(Action::Descend),
            [Event::SealBroken { position: cell }]
        );
        assert_eq!(game.status(), &Status::Won);
    }

    #[test]
    fn a_game_on_the_bottom_floor_is_saved_and_plays_on_identically() {
        let content = content();
        let mut game = Game::new(SEED, &content);
        go_down_to(&mut game, MAX_DEPTH);
        let text = saved(&game);
        assert!(text.contains('*'), "the seal is written in the map");

        let mut loaded = save::from_str(&text, &content).expect("the save reads back");

        assert_eq!(seal(&loaded), seal(&game));
        assert_eq!(loaded.depth(), MAX_DEPTH);
        assert_eq!(saved(&loaded), text);
        for direction in Direction::ALL.into_iter().cycle().take(40) {
            assert_eq!(
                loaded.submit(Action::Move(direction)),
                game.submit(Action::Move(direction))
            );
        }
        let cell = seal(&game).expect("the bottom floor has a seal");
        if game.world.actor_at(cell).is_none() {
            stand_on(&mut game, cell);
            stand_on(&mut loaded, cell);
        }
        assert_eq!(loaded.submit(Action::Descend), game.submit(Action::Descend));
        assert_eq!(saved(&loaded), saved(&game));
    }

    #[test]
    fn a_won_game_is_saved_and_read_back_won() {
        let content = content();
        let mut game = on_the_seal(&content);
        game.submit(Action::Descend);
        let text = saved(&game);
        assert!(text.contains("status: Won"), "{text}");

        let mut loaded = save::from_str(&text, &content).expect("a won game reads back");

        assert_eq!(loaded.status(), &Status::Won);
        assert!(loaded.submit(Action::Wait).is_empty());
        assert_eq!(saved(&loaded), text);
    }

    #[test]
    fn a_save_of_a_won_game_with_a_dead_player_is_refused() {
        let content = content();
        let mut game = on_the_seal(&content);
        game.submit(Action::Descend);
        let player = game.world.player;
        game.world.actors[player].take_damage(i32::MAX);
        let text = saved(&game);

        let error = save::from_str(&text, &content).expect_err("a dead winner is refused");

        assert!(
            error.to_string().contains("the game was won but"),
            "{error}"
        );
    }
}
