//! Automatic exploration on the game screen: the walk that one press of the
//! explore key (`input::EXPLORE_KEY`) starts, and the rules that stop it.
//!
//! Exposes [`Exploration`], the walk in progress, [`Halt`], why it stopped,
//! [`Sightings`], what the player saw before a step, [`foe_in_view`] and
//! [`halt_after_step`], the checks made before the first step and after
//! every step, and the limits [`MAX_EXPLORE_STEPS`] and
//! [`EXPLORE_STEP_DELAY`].
//!
//! Each step is one `Action::Explore` submitted to the game, which chooses
//! the way (`sigilfall_core::explore`); this module only decides when to
//! stop. The `app` module submits the steps, draws the screen between them,
//! listens for a key that interrupts the walk and logs why it stopped
//! (`messages::exploration_halted`).

use std::time::Duration;

use sigilfall_core::event::Event;
use sigilfall_core::game::Game;
use sigilfall_core::geometry::Position;
use sigilfall_core::item::ItemId;

/// Most steps one press of the explore key takes before the walk stops by
/// itself.
///
/// A first floor of the game is explored in 300 to 400 steps, and crossing
/// it from one end to the other takes about 80: 200 steps cover a good part
/// of a floor in one press while giving the player back the control
/// regularly, and they bound the walk whatever happens, so a press can
/// never keep the game busy for ever.
pub const MAX_EXPLORE_STEPS: u32 = 200;

/// Pause between two steps of the walk, during which the screen shows the
/// last step and a key press interrupts the walk.
///
/// 25 ms gives 40 steps a second: fast enough that a long corridor takes a
/// moment, slow enough that the eye follows the player and has time to
/// react, and [`MAX_EXPLORE_STEPS`] steps last 5 seconds at most.
pub const EXPLORE_STEP_DELAY: Duration = Duration::from_millis(25);

/// A walk of automatic exploration in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Exploration {
    /// Steps taken since the explore key was pressed.
    steps: u32,
}

impl Exploration {
    /// Returns the walk once one more step has been taken.
    #[must_use]
    pub const fn stepped(self) -> Self {
        Self {
            steps: self.steps.saturating_add(1),
        }
    }

    /// Returns the steps taken since the explore key was pressed.
    pub const fn steps(self) -> u32 {
        self.steps
    }
}

/// Why a walk of automatic exploration stopped, or did not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Halt {
    /// A monster was in view when the explore key was pressed: no step was
    /// taken. Holds the identifier of its definition, to name it.
    FoeInView(String),
    /// A monster came into view after a step. Holds the identifier of its
    /// definition.
    FoeAppeared(String),
    /// An item came into view after a step. Holds the identifier of its
    /// definition.
    ItemAppeared(String),
    /// A glyph the player has not read came into view after a step.
    GlyphAppeared,
    /// The player lost hit points during a step.
    Hurt,
    /// The way on was held by another actor; the game refused the step.
    Blocked,
    /// The walk took [`MAX_EXPLORE_STEPS`] steps.
    StepLimit,
    /// The player pressed a key, or the terminal became too small to show
    /// the game.
    Interrupted,
    /// Nothing is left to explore within reach; the game refused the step
    /// and its event already says so.
    Explored,
}

/// The items and the unread glyphs the player sees, noted before a step so
/// that what comes into view with it can be told apart.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Sightings {
    /// Keys of the items lying in view.
    items: Vec<ItemId>,
    /// Cells of the glyphs in view whose sigil the grimoire does not know.
    unread_glyphs: Vec<Position>,
}

impl Sightings {
    /// Notes what the player of `game` sees right now.
    pub fn of(game: &Game) -> Self {
        Self {
            items: game.visible_items().map(|item| item.id).collect(),
            unread_glyphs: unread_glyphs_in_view(game).collect(),
        }
    }
}

/// Returns the identifier of the definition of the first monster the
/// player of `game` sees, in the order of `Game::visible_monsters`, or
/// `None` when none is in view.
pub fn foe_in_view(game: &Game) -> Option<String> {
    game.visible_monsters()
        .next()
        .map(|monster| monster.definition.to_owned())
}

/// Decides whether the walk stops after a step of exploration, and why.
///
/// `events` are the events of the step, `game` the game after it, `before`
/// what the player saw before it ([`Sightings::of`]), `hp_before` the
/// player's hit points before it, and `walk` the walk with this step
/// counted. The reasons, in order:
/// 1. the game refused the step: nothing left to explore
///    ([`Halt::Explored`]) or the way held by an actor ([`Halt::Blocked`]);
/// 2. the player lost hit points ([`Halt::Hurt`]);
/// 3. a monster is in view ([`Halt::FoeAppeared`]);
/// 4. an item, then an unread glyph, came into view: in view now and not
///    before the step ([`Halt::ItemAppeared`], [`Halt::GlyphAppeared`]);
///    what was already in view when the step began never stops the walk;
/// 5. the walk has taken [`MAX_EXPLORE_STEPS`] steps ([`Halt::StepLimit`]).
///
/// Returns `None` when the walk goes on.
pub fn halt_after_step(
    events: &[Event],
    game: &Game,
    before: &Sightings,
    hp_before: i32,
    walk: Exploration,
) -> Option<Halt> {
    for event in events {
        match event {
            Event::NothingToExplore { .. } => return Some(Halt::Explored),
            Event::MoveBlocked { actor, .. } if *actor == game.player() => {
                return Some(Halt::Blocked);
            }
            _ => {}
        }
    }
    if game.player_actor().hp() < hp_before {
        return Some(Halt::Hurt);
    }
    if let Some(foe) = foe_in_view(game) {
        return Some(Halt::FoeAppeared(foe));
    }
    if let Some(item) = game
        .visible_items()
        .find(|item| !before.items.contains(&item.id))
    {
        return Some(Halt::ItemAppeared(item.definition.to_owned()));
    }
    if unread_glyphs_in_view(game).any(|cell| !before.unread_glyphs.contains(&cell)) {
        return Some(Halt::GlyphAppeared);
    }
    (walk.steps() >= MAX_EXPLORE_STEPS).then_some(Halt::StepLimit)
}

/// The cells of the glyphs the player of `game` sees and has not read: their
/// sigil is not in the grimoire.
fn unread_glyphs_in_view(game: &Game) -> impl Iterator<Item = Position> + '_ {
    game.engravings()
        .iter()
        .filter(|engraving| game.is_visible(engraving.position()))
        .filter(|engraving| !game.grimoire().knows(engraving.sigil().id()))
        .map(|engraving| engraving.position())
}

#[cfg(test)]
mod tests {
    use sigilfall_core::content::Content;
    use sigilfall_core::rng::Seed;

    use super::*;

    /// A new game in the fixed room, where the player stands alone.
    fn fixed_room() -> Game {
        Game::in_fixed_room(Seed::new(29))
    }

    /// The content shipped with the game.
    fn content() -> Content {
        Content::load().expect("the shipped content is valid")
    }

    /// A walk that has taken one step.
    fn one_step() -> Exploration {
        Exploration::default().stepped()
    }

    #[test]
    fn a_walk_counts_its_steps() {
        let walk = Exploration::default();

        assert_eq!(walk.steps(), 0);
        assert_eq!(walk.stepped().stepped().steps(), 2);
    }

    #[test]
    fn a_quiet_step_lets_the_walk_go_on() {
        let game = fixed_room();
        let before = Sightings::of(&game);
        let hp = game.player_actor().hp();

        assert_eq!(halt_after_step(&[], &game, &before, hp, one_step()), None);
    }

    #[test]
    fn losing_hit_points_stops_the_walk() {
        let game = fixed_room();
        let before = Sightings::of(&game);
        let hp = game.player_actor().hp();

        assert_eq!(
            halt_after_step(&[], &game, &before, hp + 1, one_step()),
            Some(Halt::Hurt)
        );
    }

    #[test]
    fn a_refused_step_stops_the_walk_and_says_why() {
        let game = fixed_room();
        let before = Sightings::of(&game);
        let hp = game.player_actor().hp();
        let here = game.player_actor().position();
        let explored = [Event::NothingToExplore { position: here }];
        let blocked = [Event::MoveBlocked {
            actor: game.player(),
            from: here,
            to: here,
        }];

        assert_eq!(
            halt_after_step(&explored, &game, &before, hp, one_step()),
            Some(Halt::Explored)
        );
        assert_eq!(
            halt_after_step(&blocked, &game, &before, hp, one_step()),
            Some(Halt::Blocked)
        );
    }

    #[test]
    fn a_monster_in_view_stops_the_walk() {
        let content = content();
        let mut game = fixed_room();
        let before = Sightings::of(&game);
        let hp = game.player_actor().hp();
        let rat = &content.monsters()[0];
        game.spawn_monster(Position::new(7, 5), rat);

        assert_eq!(foe_in_view(&game), Some(rat.id().to_owned()));
        assert_eq!(
            halt_after_step(&[], &game, &before, hp, one_step()),
            Some(Halt::FoeAppeared(rat.id().to_owned()))
        );
    }

    #[test]
    fn an_item_stops_the_walk_only_when_it_comes_into_view() {
        let content = content();
        let mut game = fixed_room();
        let hp = game.player_actor().hp();
        let draught = &content.items()[0];
        game.place_item(Position::new(7, 5), draught);
        let seen = Sightings::of(&game);

        assert_eq!(
            halt_after_step(&[], &game, &Sightings::default(), hp, one_step()),
            Some(Halt::ItemAppeared(draught.id().to_owned()))
        );
        assert_eq!(halt_after_step(&[], &game, &seen, hp, one_step()), None);
    }

    #[test]
    fn an_unread_glyph_stops_the_walk_when_it_comes_into_view() {
        let content = content();
        let mut game = fixed_room();
        let hp = game.player_actor().hp();
        game.engrave(Position::new(7, 5), &content.sigils()[0]);
        let seen = Sightings::of(&game);

        assert_eq!(
            halt_after_step(&[], &game, &Sightings::default(), hp, one_step()),
            Some(Halt::GlyphAppeared)
        );
        assert_eq!(halt_after_step(&[], &game, &seen, hp, one_step()), None);
    }

    #[test]
    fn the_walk_stops_at_its_step_limit() {
        let game = fixed_room();
        let before = Sightings::of(&game);
        let hp = game.player_actor().hp();
        let mut walk = Exploration::default();
        for _ in 1..MAX_EXPLORE_STEPS {
            walk = walk.stepped();
        }

        assert_eq!(halt_after_step(&[], &game, &before, hp, walk), None);
        assert_eq!(
            halt_after_step(&[], &game, &before, hp, walk.stepped()),
            Some(Halt::StepLimit)
        );
    }
}
