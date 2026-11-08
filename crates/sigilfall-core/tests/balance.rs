//! Balance of the dungeon, measured with a reckless player and a careful
//! one.
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
//!
//! The careful bot plays the whole game: it explores every floor, reads the
//! glyphs, picks up and uses the items, heals when low, casts Lance, Ember
//! and Recall, then goes down; on the bottom floor it breaks the seal. It
//! measures the depths a player who uses everything the game gives reaches
//! (task T32: the healing draughts of the deeper floors, decision D24), and
//! checks that such a player gets past the reckless one's floors while a
//! victory stays rare.

use sigilfall_core::action::Action;
use sigilfall_core::combat::Combatant;
use sigilfall_core::content::Content;
use sigilfall_core::event::Event;
use sigilfall_core::game::{Game, Status};
use sigilfall_core::geometry::{Direction, Position};
use sigilfall_core::item::{INVENTORY_CAPACITY, ItemId};
use sigilfall_core::map::Tile;
use sigilfall_core::path::find_path;
use sigilfall_core::rng::Seed;
use sigilfall_core::sigil::{ESSENCE_COST, SPELL_RANGE, Trace};

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

// ---------------------------------------------------------------------------
// The careful player: items and spells
// ---------------------------------------------------------------------------

/// Number of runs of the careful bot, one per seed from 0.
const CAREFUL_RUNS: u64 = 80;

/// Most player turns a run of the careful bot may last. It explores every
/// floor, so a run to the bottom takes a few thousand turns; far above that,
/// the bound only stops a bot that got stuck.
const CAREFUL_MAX_TURNS: u64 = 30_000;

/// Hit points at or under which the careful bot heals before anything else,
/// fight or not: two blows of the strongest creature (attack 6) and a bit.
const LOW_HP: i32 = 12;

/// Hit points a healing draught gives back (`content/items.ron`), written
/// out so the bot never drinks one when part of it would be lost.
const DRAUGHT_HEAL: i32 = 10;

/// Essence an essence phial gives back (`content/items.ron`).
const PHIAL_ESSENCE: i32 = 4;

/// Squared distance a spell on the nearest foe reaches, `8² + 8` (the disc
/// of radius `SPELL_RANGE`); the bot never wastes a cast on a farther foe.
const NEAREST_REACH_SQUARED: i32 = 72;

/// Floors the median floor reached by the careful bot must lie on: past the
/// floors where the reckless bot dies, short of the bottom.
const CAREFUL_TARGET_DEPTHS: std::ops::RangeInclusive<u32> = 4..=7;

/// Largest share of the careful bot's runs that may end in a victory: the
/// seal stays an achievement, not the usual end of a careful run.
const MAX_CAREFUL_WIN_SHARE: f64 = 0.1;

/// How a run of the careful bot ended.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CarefulOutcome {
    /// Floor the run ended on.
    depth: u32,
    /// Whether the player died.
    died: bool,
    /// Whether the player broke the seal of the bottom floor.
    won: bool,
    /// Healing draughts drunk during the run.
    draughts: u32,
    /// Healing draughts still carried when the run ended.
    draughts_left: usize,
    /// Definition of the creature that killed the player, if it died.
    killer: Option<String>,
}

/// The shipped traces the careful bot casts.
struct Spells {
    /// Damage to the nearest foe in reach.
    ember: Trace,
    /// Heals the caster.
    recall: Trace,
    /// Damage to the first creature in a direction.
    lance: Trace,
}

impl Spells {
    /// The traces of the shipped sigils of `content`.
    fn of(content: &Content) -> Self {
        let trace = |id: &str| *content.sigil(id).expect("a shipped sigil").pattern();
        Self {
            ember: trace("ember"),
            recall: trace("recall"),
            lance: trace("lance"),
        }
    }
}

/// The cells of the straight line from `from` in `direction`, at most
/// `SPELL_RANGE` of them, stopping before the first step the map refuses:
/// the cells an aimed spell crosses.
fn aimed_line(game: &Game, from: Position, direction: Direction) -> Vec<Position> {
    let mut cells = Vec::new();
    let mut cell = from;
    for _ in 0..SPELL_RANGE {
        if !game.map().can_step(cell, direction) {
            break;
        }
        cell = cell.step(direction);
        cells.push(cell);
    }
    cells
}

/// The direction in which the first creature on an aimed line is a monster,
/// the nearest such monster first, if any.
fn lance_direction(game: &Game, from: Position) -> Option<Direction> {
    Direction::ALL
        .into_iter()
        .filter_map(|direction| {
            let line = aimed_line(game, from, direction);
            let (steps, is_player) = line.iter().enumerate().find_map(|(steps, &cell)| {
                game.actors()
                    .find(|(_, actor)| actor.position() == cell)
                    .map(|(_, actor)| (steps, actor.is_player()))
            })?;
            (!is_player).then_some((steps, direction))
        })
        .min_by_key(|&(steps, _)| steps)
        .map(|(_, direction)| direction)
}

/// The key of a carried item of definition `definition`, if any.
fn carried(game: &Game, definition: &str) -> Option<ItemId> {
    game.inventory()
        .find(|(_, item)| item.definition().id() == definition)
        .map(|(id, _)| id)
}

/// The cell of the way down of the current floor: its stairs, or the seal of
/// the bottom floor.
fn way_down(game: &Game) -> Position {
    let map = game.map();
    let width = i32::try_from(map.width()).expect("a floor fits in i32");
    let height = i32::try_from(map.height()).expect("a floor fits in i32");
    (0..height)
        .flat_map(|y| (0..width).map(move |x| Position::new(x, y)))
        .find(|&cell| matches!(map.tile(cell), Some(Tile::StairsDown | Tile::Seal)))
        .expect("every generated floor has stairs or a seal")
}

/// The careful bot's next action on the current state of `game`, knowing
/// whether the floor is already `explored` and the cell of an unread glyph
/// or an item it set out for, its `errand`.
///
/// In order: pick up what lies underfoot; heal at [`LOW_HP`] or under (a
/// draught, else Recall); drink a draught out of a fight when none of it is
/// lost, an essence phial in a fight, a steadying ink out of one, a page of
/// the veil at once; in a fight, Lance at a monster first on an aimed line,
/// Ember at the nearest monster in reach, otherwise strike or close in; out
/// of a fight, go on with the errand or set out for the nearest unread glyph
/// or item in view (the errand is kept until reached, so a glimpse through a
/// gap is not forgotten one step later), explore, then take the way down.
/// Every rule is deterministic.
fn careful_action(
    game: &Game,
    explored: bool,
    errand: &mut Option<Position>,
    spells: &Spells,
) -> Action {
    let me = game.player_actor();
    let here = me.position();
    let hp = me.hp();
    let can_cast = me.essence() >= ESSENCE_COST;
    let knows = |id: &str| game.grimoire().knows(id);
    let foes: Vec<Position> = game.visible_monsters().map(|m| m.position).collect();
    let in_fight = !foes.is_empty();

    if game.visible_items().any(|item| item.position == here)
        && game.inventory().count() < INVENTORY_CAPACITY
    {
        return Action::PickUp;
    }
    if hp <= LOW_HP {
        if let Some(draught) = carried(game, "healing_draught") {
            return Action::Use(draught);
        }
        if can_cast && knows("recall") {
            return Action::Cast {
                trace: spells.recall,
                direction: None,
            };
        }
    }
    let wanted = [
        (
            "healing_draught",
            !in_fight && hp + DRAUGHT_HEAL <= me.max_hp(),
        ),
        (
            "essence_phial",
            in_fight && me.essence() + PHIAL_ESSENCE <= me.max_essence(),
        ),
        ("steadying_ink", !in_fight && game.steady_casts() == 0),
        ("veil_page", !knows("veil")),
    ];
    for (definition, now) in wanted {
        if now && let Some(item) = carried(game, definition) {
            return Action::Use(item);
        }
    }

    let monsters: Vec<Position> = game
        .actors()
        .filter(|(_, actor)| !actor.is_player())
        .map(|(_, actor)| actor.position())
        .collect();
    let occupied = |cell: Position| monsters.contains(&cell);
    if in_fight {
        if can_cast
            && knows("lance")
            && let Some(direction) = lance_direction(game, here)
        {
            return Action::Cast {
                trace: spells.lance,
                direction: Some(direction),
            };
        }
        let in_reach = foes.iter().any(|foe| {
            let (dx, dy) = (foe.x - here.x, foe.y - here.y);
            dx * dx + dy * dy <= NEAREST_REACH_SQUARED
        });
        if can_cast && knows("ember") && in_reach {
            return Action::Cast {
                trace: spells.ember,
                direction: None,
            };
        }
        let charge = foes
            .iter()
            .filter_map(|&foe| {
                let path = find_path(game.map(), here, foe, occupied)?;
                Some((path.len(), foe.y, foe.x, path))
            })
            .min_by_key(|&(steps, y, x, _)| (steps, y, x));
        return charge
            .and_then(|(_, _, _, path)| path.first().copied())
            .map_or(Action::Wait, Action::Move);
    }

    let pack_full = game.inventory().count() >= INVENTORY_CAPACITY;
    let glyphs = game
        .engravings()
        .iter()
        .filter(|glyph| game.is_visible(glyph.position()) && !knows(glyph.sigil().id()))
        .map(|glyph| glyph.position());
    let items = game
        .visible_items()
        .filter(|_| !pack_full)
        .map(|item| item.position);
    if *errand == Some(here) {
        *errand = None;
    }
    if errand.is_none() {
        *errand = glyphs
            .chain(items)
            .filter_map(|goal| {
                let steps = find_path(game.map(), here, goal, occupied)?.len();
                Some((steps, goal.y, goal.x, goal))
            })
            .min_by_key(|&(steps, y, x, _)| (steps, y, x))
            .map(|(_, _, _, goal)| goal);
    }
    if let Some(goal) = *errand {
        match find_path(game.map(), here, goal, occupied).and_then(|path| path.first().copied()) {
            Some(direction) => return Action::Move(direction),
            None => *errand = None,
        }
    }
    if !explored {
        return Action::Explore;
    }
    let down = way_down(game);
    if here == down {
        return Action::Descend;
    }
    find_path(game.map(), here, down, occupied)
        .or_else(|| find_path(game.map(), here, down, |_| false))
        .and_then(|path| path.first().copied())
        .map_or(Action::Wait, Action::Move)
}

/// Plays one run of the careful bot on `seed` until death, victory or a
/// bound of the test.
fn play_carefully(seed: Seed, content: &Content, spells: &Spells) -> CarefulOutcome {
    let mut game = Game::new(seed, content);
    let mut explored_floor = 0;
    let mut errand = None;
    let mut draughts = 0;
    for _ in 0..2 * CAREFUL_MAX_TURNS {
        if game.is_over() || game.player_turns() >= CAREFUL_MAX_TURNS {
            break;
        }
        let before = game.player_turns();
        let explored = explored_floor == game.depth();
        let action = careful_action(&game, explored, &mut errand, spells);
        let events = game.submit(action);
        for event in &events {
            match event {
                Event::Descended { .. } => errand = None,
                Event::NothingToExplore { .. } => explored_floor = game.depth(),
                Event::Used { definition, .. } if definition == "healing_draught" => {
                    draughts += 1;
                }
                _ => {}
            }
        }
        if game.player_turns() == before && !game.is_over() && explored_floor != game.depth() {
            game.submit(Action::Wait);
        }
    }
    CarefulOutcome {
        depth: game.depth(),
        died: matches!(game.status(), Status::GameOver { .. }),
        won: *game.status() == Status::Won,
        draughts,
        draughts_left: game
            .inventory()
            .filter(|(_, item)| item.definition().id() == "healing_draught")
            .count(),
        killer: match game.status() {
            Status::GameOver {
                killed_by: Combatant::Monster { definition },
            } => Some(definition.clone()),
            _ => None,
        },
    }
}

#[test]
fn a_careful_player_gets_past_the_reckless_floors_and_rarely_wins() {
    let content = Content::load().expect("the shipped content is valid");
    let spells = Spells::of(&content);
    // Runs are independent: one thread per seed, joined in seed order.
    let outcomes: Vec<CarefulOutcome> = std::thread::scope(|scope| {
        let runs: Vec<_> = (0..CAREFUL_RUNS)
            .map(|value| {
                let (content, spells) = (&content, &spells);
                scope.spawn(move || play_carefully(Seed::new(value), content, spells))
            })
            .collect();
        runs.into_iter()
            .map(|run| run.join().expect("a run of the careful bot panicked"))
            .collect()
    });
    let mut histogram = String::new();
    for depth in 1..=10 {
        let died = outcomes
            .iter()
            .filter(|o| o.died && o.depth == depth)
            .count();
        let won = outcomes
            .iter()
            .filter(|o| o.won && o.depth == depth)
            .count();
        let other = outcomes
            .iter()
            .filter(|o| !o.died && !o.won && o.depth == depth)
            .count();
        histogram.push_str(&format!(
            "  floor {depth}: {died} deaths, {won} wins, {other} stopped\n"
        ));
    }
    let depths: Vec<u32> = outcomes.iter().map(|o| o.depth).collect();
    let draughts: Vec<u32> = outcomes.iter().map(|o| o.draughts).collect();
    let mean_depth = f64::from(depths.iter().sum::<u32>()) / depths.len() as f64;
    let wins = outcomes.iter().filter(|o| o.won).count();
    eprintln!(
        "careful bot over {CAREFUL_RUNS} seeds:\n{histogram}  median floor reached: {} \
         (mean {mean_depth:.2})\n  \
         wins: {}; deaths on floors 5..=7: {}; draughts drunk: median {}, total {}",
        median(&depths),
        wins,
        outcomes
            .iter()
            .filter(|o| o.died && (5..=7).contains(&o.depth))
            .count(),
        median(&draughts),
        draughts.iter().sum::<u32>(),
    );
    let mut killers = std::collections::BTreeMap::new();
    for outcome in outcomes.iter().filter(|o| (5..=7).contains(&o.depth)) {
        if let Some(killer) = &outcome.killer {
            *killers.entry(killer.as_str()).or_insert(0) += 1;
        }
    }
    eprintln!(
        "  killers on floors 5..=7: {killers:?}; deaths with a draught left: {}",
        outcomes
            .iter()
            .filter(|o| o.died && o.draughts_left > 0)
            .count()
    );

    assert!(
        CAREFUL_TARGET_DEPTHS.contains(&median(&depths)),
        "the careful bot's median floor reached is {}, expected {CAREFUL_TARGET_DEPTHS:?}",
        median(&depths)
    );
    let win_share = wins as f64 / outcomes.len() as f64;
    assert!(
        win_share <= MAX_CAREFUL_WIN_SHARE,
        "{win_share:.2} of the careful runs win, expected at most {MAX_CAREFUL_WIN_SHARE}"
    );
    assert!(
        outcomes.iter().all(|o| o.died || o.won),
        "a careful run was stopped by the turn bound: the bot got stuck"
    );
}
