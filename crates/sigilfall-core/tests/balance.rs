//! Balance of the dungeon, measured with a reckless player.
//!
//! The reckless bot charges the nearest monster it sees and, when it sees
//! none, walks to the stairs and goes down. It never picks up an item,
//! never casts and never retreats: the least careful way to play. The
//! balance aims at such a player dying, as a rule, between floors 2 and 4
//! (task T22 of the plan): the first floor teaches, the next ones punish
//! recklessness.
//!
//! The test plays [`RUNS`] seeds, prints the distribution of the floor of
//! death and of the turn of death, and checks it with robust statistics
//! (median and a share of the runs) rather than single runs, so that a small
//! change of the generation does not break it while a change of the balance
//! does. Written against the public API only.

use sigilfall_core::action::Action;
use sigilfall_core::content::Content;
use sigilfall_core::game::Game;
use sigilfall_core::geometry::Position;
use sigilfall_core::map::Tile;
use sigilfall_core::path::find_path;
use sigilfall_core::rng::Seed;

/// Number of runs measured, one per seed from 0.
const RUNS: u64 = 80;

/// Most player turns a run may last; a run still going then counts as a
/// survival. Far above the length of a run that ends between floors 2 and 4,
/// it bounds the test if the balance ever made the bot immortal.
const MAX_TURNS: u64 = 4000;

/// Deepest floor a run may reach; a run arriving there counts as a survival.
const MAX_DEPTH_REACHED: u32 = 12;

/// Floors the median floor of death must lie on.
const TARGET_DEPTHS: std::ops::RangeInclusive<u32> = 2..=4;

/// Smallest share of runs that must end by a death on a floor of
/// [`TARGET_DEPTHS`]: most runs, without asking every seed to agree.
const MIN_SHARE_IN_TARGET: f64 = 0.6;

/// How a run of the bot ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Outcome {
    /// Floor the run ended on.
    depth: u32,
    /// Player turns played.
    turns: u64,
    /// Whether the player died; `false` when a bound of the test stopped it.
    died: bool,
}

/// The bot's next action on the current state of `game`.
///
/// A visible monster is charged by the shortest walk (other monsters count
/// as obstacles on the way), the nearest first; with none in sight, the bot
/// goes down if it stands on the stairs, or walks towards them otherwise.
/// Every rule is deterministic, so a seed always gives the same run.
fn reckless_action(game: &Game) -> Action {
    let map = game.map();
    let me = game.player_actor().position();
    let monsters: Vec<Position> = game
        .actors()
        .filter(|(_, actor)| !actor.is_player())
        .map(|(_, actor)| actor.position())
        .collect();
    let occupied = |cell: Position| monsters.contains(&cell);

    let charge = game
        .visible_monsters()
        .filter_map(|monster| {
            let path = find_path(map, me, monster.position, occupied)?;
            Some((path.len(), monster.position.y, monster.position.x, path))
        })
        .min_by_key(|&(steps, y, x, _)| (steps, y, x));
    if let Some((_, _, _, path)) = charge
        && let Some(&direction) = path.first()
    {
        return Action::Move(direction);
    }

    if map.tile(me) == Some(Tile::StairsDown) {
        return Action::Descend;
    }
    let stairs = stairs(game);
    find_path(map, me, stairs, occupied)
        .or_else(|| find_path(map, me, stairs, |_| false))
        .and_then(|path| path.first().copied())
        .map_or(Action::Wait, Action::Move)
}

/// Returns the cell of the stairs of the current floor of `game`.
fn stairs(game: &Game) -> Position {
    let map = game.map();
    let width = i32::try_from(map.width()).expect("a floor fits in i32");
    let height = i32::try_from(map.height()).expect("a floor fits in i32");
    (0..height)
        .flat_map(|y| (0..width).map(move |x| Position::new(x, y)))
        .find(|&cell| map.tile(cell) == Some(Tile::StairsDown))
        .expect("every generated floor has stairs")
}

/// Plays one run of the bot on `seed` until death or a bound of the test.
fn play(seed: Seed, content: &Content) -> Outcome {
    let mut game = Game::new(seed, content);
    // Every submitted action either takes a turn or is refused; a refused
    // one (a blocked step) is followed by a wait, so the count of submits is
    // bounded by twice the turns.
    for _ in 0..2 * MAX_TURNS {
        if game.is_over() || game.player_turns() >= MAX_TURNS || game.depth() >= MAX_DEPTH_REACHED {
            break;
        }
        let before = game.player_turns();
        game.submit(reckless_action(&game));
        if game.player_turns() == before && !game.is_over() {
            game.submit(Action::Wait);
        }
    }
    Outcome {
        depth: game.depth(),
        turns: game.player_turns(),
        died: game.is_over(),
    }
}

/// Returns the middle value of `values`, the lower one of the two middles
/// for an even count.
fn median<T: Copy + Ord>(values: &[T]) -> T {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    sorted[(sorted.len() - 1) / 2]
}

#[test]
fn a_reckless_player_dies_between_the_second_and_the_fourth_floor() {
    let content = Content::load().expect("the shipped content is valid");
    let outcomes: Vec<Outcome> = (0..RUNS)
        .map(|value| play(Seed::new(value), &content))
        .collect();

    let deaths: Vec<&Outcome> = outcomes.iter().filter(|outcome| outcome.died).collect();
    let depths: Vec<u32> = outcomes.iter().map(|outcome| outcome.depth).collect();
    let turns: Vec<u64> = deaths.iter().map(|outcome| outcome.turns).collect();
    let mut histogram = String::new();
    for depth in 1..=MAX_DEPTH_REACHED {
        let died = deaths.iter().filter(|o| o.depth == depth).count();
        let alive = outcomes
            .iter()
            .filter(|o| !o.died && o.depth == depth)
            .count();
        if died + alive > 0 {
            histogram.push_str(&format!(
                "  floor {depth}: {died} deaths, {alive} survivals\n"
            ));
        }
    }
    let in_target = deaths
        .iter()
        .filter(|outcome| TARGET_DEPTHS.contains(&outcome.depth))
        .count();
    let share = in_target as f64 / outcomes.len() as f64;
    eprintln!(
        "reckless bot over {RUNS} seeds:\n{histogram}  median floor reached: {}\n  \
         deaths: {}; turn of death: min {}, median {}, max {}\n  \
         share of deaths on floors {}..={}: {share:.2}",
        median(&depths),
        deaths.len(),
        turns.iter().min().copied().unwrap_or(0),
        if turns.is_empty() { 0 } else { median(&turns) },
        turns.iter().max().copied().unwrap_or(0),
        TARGET_DEPTHS.start(),
        TARGET_DEPTHS.end(),
    );

    assert!(
        TARGET_DEPTHS.contains(&median(&depths)),
        "the median floor reached is {}, expected {TARGET_DEPTHS:?}",
        median(&depths)
    );
    assert!(
        share >= MIN_SHARE_IN_TARGET,
        "only {share:.2} of the runs die on floors {TARGET_DEPTHS:?}, expected at least \
         {MIN_SHARE_IN_TARGET}"
    );
    assert!(
        outcomes
            .iter()
            .all(|outcome| outcome.died || outcome.depth > *TARGET_DEPTHS.end()),
        "a run stopped by the turn bound on a shallow floor: the bot got stuck"
    );
}
