//! Acceptance tests that re-check F8 (floors and difficulty) and F14
//! (victory at the bottom) after task T36, on the public API of the core.
//!
//! Three things changed in T36 and are checked here from the outside:
//!
//! - a save that contradicts itself about the way down is refused by the
//!   reader: the bottom floor without a seal, with stairs, or a seal or a
//!   victory on a floor above it. Each damaged save is made by editing the
//!   text a real game wrote, so the reader is faced with what a player who
//!   edits the file by hand would produce;
//! - the spawn tables of the deep floors changed: the numbers written below
//!   come from the journal of T36, not from `spawns.ron`, so a table edited
//!   later without the plan being updated is noticed;
//! - the two elites got weaker but must stay the strongest creatures of
//!   their family, and a floor must stay the same for the same seed.
//!
//! Every loop is bounded.

use sigilfall_core::action::Action;
use sigilfall_core::content::Content;
use sigilfall_core::game::{Game, Status};
use sigilfall_core::generation::{generate_with_monsters, monsters_per_floor};
use sigilfall_core::geometry::{Direction, Position};
use sigilfall_core::map::{Map, Tile};
use sigilfall_core::path::find_path;
use sigilfall_core::rng::Seed;
use sigilfall_core::save::{self, SaveError};

/// The bottom floor, written out here and not read from the game.
const BOTTOM: u32 = 10;

/// Seeds whose floors are generated for the spawn checks.
const SEEDS: u64 = 60;

/// Most actions spent walking to the way down of one floor.
const WALK_BUDGET: usize = 1500;

/// One kind of monster that never gets to hurt: at speed 1 it gains a
/// hundredth of the energy it needs to act, and one blow kills it.
const HARMLESS: &str = r#"[
    Monster(id: "dust", name: "dust", family: Vermin, elite: false,
        speed: 1, hp: 1, attack: 0, defense: 0, min_depth: 1),
]"#;

/// The shipped content.
fn shipped() -> Content {
    Content::load().expect("the shipped content is valid")
}

/// The shipped sigils, items and floors with harmless monsters.
fn harmless() -> Content {
    Content::parse(HARMLESS).expect("valid test content")
}

/// Every cell of `map`, in reading order.
fn cells(map: &Map) -> Vec<Position> {
    let width = i32::try_from(map.width()).unwrap();
    let height = i32::try_from(map.height()).unwrap();
    (0..height)
        .flat_map(|y| (0..width).map(move |x| Position::new(x, y)))
        .collect()
}

/// The first cell of `map` holding one of `tiles`.
fn find(map: &Map, tiles: &[Tile]) -> Option<Position> {
    cells(map)
        .into_iter()
        .find(|&cell| map.tile(cell).is_some_and(|tile| tiles.contains(&tile)))
}

/// Walks the player of `game` onto the way down of its floor.
fn walk_down(game: &mut Game) {
    for _ in 0..WALK_BUDGET {
        let here = game.player_actor().position();
        let goal = find(game.map(), &[Tile::StairsDown, Tile::Seal]).expect("a way down");
        if here == goal {
            return;
        }
        let path = find_path(game.map(), here, goal, |_| false).expect("the way down is reachable");
        let direction: Direction = *path.first().expect("a step");
        game.submit(Action::Move(direction));
        assert!(!game.is_over(), "the walker is still playing");
    }
    panic!("the way down was not reached in {WALK_BUDGET} actions");
}

/// A game of `seed` with harmless monsters, played down to floor `depth`.
fn game_on_floor(seed: u64, depth: u32) -> Game {
    let mut game = Game::new(Seed::new(seed), &harmless());
    while game.depth() < depth {
        walk_down(&mut game);
        game.submit(Action::Descend);
    }
    game
}

/// The text of the saved `game`, with the first `from` of its tile rows
/// replaced by `to` (`None` leaves the tiles alone).
fn saved_with_tile(game: &Game, edit: Option<(char, char)>) -> String {
    let text = save::to_string(game).expect("a game is always written");
    let Some((from, to)) = edit else {
        return text;
    };
    let tiles = text.find("tiles: [").expect("a tiles block");
    let end = tiles + text[tiles..].find("],").expect("the end of the tiles");
    let offset = text[tiles..end]
        .match_indices(from)
        .map(|(offset, _)| offset)
        // Skip the characters of the word `tiles` itself.
        .find(|&offset| text[tiles..end][..offset].contains('"'))
        .unwrap_or_else(|| panic!("no `{from}` in the tiles"));
    let mut edited = text.clone();
    edited.replace_range(tiles + offset..=tiles + offset, &to.to_string());
    assert_ne!(edited, text, "the edit changed nothing");
    edited
}

/// The message of the refusal of `text`, failing if the reader accepts it.
fn refusal(text: &str) -> String {
    match save::from_str(text, &harmless()) {
        Err(SaveError::Inconsistent(message)) => message,
        Err(other) => panic!("expected an inconsistency, got {other:?}"),
        Ok(_) => panic!("the reader accepted a self-contradicting save"),
    }
}

// --- B1: saves that contradict themselves about the way down -----------------------------

#[test]
fn an_untouched_save_of_the_bottom_floor_and_of_a_floor_above_reads_back() {
    for (seed, depth) in [(42, BOTTOM), (7, 3)] {
        let game = game_on_floor(seed, depth);
        let text = saved_with_tile(&game, None);
        let loaded = save::from_str(&text, &harmless()).expect("a real save reads back");
        assert_eq!(loaded.depth(), depth);
        assert_eq!(save::to_string(&loaded).unwrap(), text, "nothing is lost");
    }
}

#[test]
fn the_bottom_floor_without_a_seal_is_refused_and_the_message_names_the_floor() {
    let game = game_on_floor(42, BOTTOM);
    let message = refusal(&saved_with_tile(&game, Some(('*', '.'))));
    assert!(message.contains("10"), "{message}");
    assert!(message.contains("seal"), "{message}");
}

#[test]
fn the_bottom_floor_with_stairs_instead_of_the_seal_is_refused() {
    let game = game_on_floor(42, BOTTOM);
    let message = refusal(&saved_with_tile(&game, Some(('*', '>'))));
    assert!(message.contains("10"), "{message}");
}

#[test]
fn the_bottom_floor_with_stairs_and_the_seal_is_refused() {
    let game = game_on_floor(7, BOTTOM);
    // A wall cell turned into stairs adds a second way down.
    let message = refusal(&saved_with_tile(&game, Some(('.', '>'))));
    assert!(message.contains("stairs"), "{message}");
}

#[test]
fn a_seal_on_a_floor_above_the_bottom_is_refused() {
    let game = game_on_floor(42, 3);
    let message = refusal(&saved_with_tile(&game, Some(('>', '*'))));
    assert!(message.contains("seal"), "{message}");
    assert!(message.contains('3'), "{message}");
}

#[test]
fn a_victory_on_a_floor_above_the_bottom_is_refused() {
    let game = game_on_floor(42, 3);
    let text = saved_with_tile(&game, None);
    assert!(text.contains("status: Playing"));
    let message = refusal(&text.replacen("status: Playing", "status: Won", 1));
    assert!(message.contains("won"), "{message}");
}

#[test]
fn a_victory_on_the_bottom_floor_still_reads_back() {
    let mut game = game_on_floor(42, BOTTOM);
    walk_down(&mut game);
    game.submit(Action::Descend);
    assert_eq!(game.status(), &Status::Won);
    let text = saved_with_tile(&game, None);
    let loaded = save::from_str(&text, &harmless()).expect("the won game reads back");
    assert_eq!(loaded.status(), &Status::Won);
}

// --- F8: the table of the depth, after the rebalance ----------------------------------------

/// Floors, as `(first, last)`, each monster may appear on after T36 (journal
/// of T36 and decision D24 of the plan).
const WINDOWS: [(&str, u32, u32); 8] = [
    ("ossuary_rat", 1, 9),
    ("wax_beetle", 1, 7),
    ("acolyte", 2, 10),
    ("homunculus", 2, 10),
    ("vigil_eye", 3, 10),
    ("shade", 3, 10),
    ("hierophant", 4, 10),
    ("lead_homunculus", 5, 10),
];

#[test]
fn every_monster_of_every_floor_comes_from_the_table_of_its_depth() {
    let content = shipped();
    let mut seen = [0usize; 8];
    for depth in 1..=BOTTOM {
        for value in 0..SEEDS {
            let floor = generate_with_monsters(Seed::new(value), depth, &content);
            assert_eq!(floor.monsters().len(), monsters_per_floor(depth));
            for monster in floor.monsters() {
                let id = monster.definition().id();
                let (index, &(_, first, last)) = WINDOWS
                    .iter()
                    .enumerate()
                    .find(|(_, (name, _, _))| *name == id)
                    .unwrap_or_else(|| panic!("unknown monster {id}"));
                assert!(
                    (first..=last).contains(&depth),
                    "{id} on floor {depth} (seed {value}), allowed {first}..={last}"
                );
                seen[index] += 1;
            }
        }
    }
    for (index, (name, first, last)) in WINDOWS.iter().enumerate() {
        assert!(
            seen[index] > 0,
            "{name} never drawn on floors {first}..={last}"
        );
    }
}

#[test]
fn the_number_of_monsters_follows_the_depth_and_stops_at_twelve() {
    let counts: Vec<usize> = (1..=BOTTOM).map(monsters_per_floor).collect();
    assert_eq!(counts, [6, 7, 8, 9, 10, 11, 12, 12, 12, 12]);
}

#[test]
fn the_same_seed_and_depth_give_the_same_floor_and_the_floor_of_a_played_run() {
    let content = shipped();
    for value in 0..10 {
        for depth in [1, 5, 8, BOTTOM] {
            let one = generate_with_monsters(Seed::new(value), depth, &content);
            let two = generate_with_monsters(Seed::new(value), depth, &content);
            let kinds = |floor: &sigilfall_core::generation::Floor| -> Vec<(String, Position)> {
                floor
                    .monsters()
                    .iter()
                    .map(|m| (m.definition().id().to_owned(), m.position()))
                    .collect()
            };
            assert_eq!(kinds(&one), kinds(&two), "seed {value}, floor {depth}");
            assert_eq!(one.start(), two.start());
        }
    }
    // A run that descends gets the floor of its seed, depth by depth.
    let game = game_on_floor(9940, 6);
    let floor = generate_with_monsters(Seed::new(9940), 6, &harmless());
    assert_eq!(floor.start(), game.player_actor().position());
}

#[test]
fn descending_off_the_stairs_does_nothing_and_costs_no_time() {
    let mut game = game_on_floor(5, 2);
    let before = save::to_string(&game).unwrap();
    let events = game.submit(Action::Descend);
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(game.depth(), 2);
    assert_eq!(save::to_string(&game).unwrap(), before, "no time passes");
}

// --- The elites stay the strongest of their family ------------------------------------------

#[test]
fn each_elite_stays_stronger_than_the_base_creature_of_its_family() {
    let content = shipped();
    for elite in content.monsters().iter().filter(|m| m.elite()) {
        for base in content
            .monsters()
            .iter()
            .filter(|m| !m.elite() && m.family() == elite.family())
        {
            assert!(
                elite.hp() > base.hp(),
                "{} hp {} vs {} hp {}",
                elite.id(),
                elite.hp(),
                base.id(),
                base.hp()
            );
            assert!(
                elite.attack() >= base.attack(),
                "{} attack vs {}",
                elite.id(),
                base.id()
            );
            assert!(
                elite.defense() >= base.defense(),
                "{} defense vs {}",
                elite.id(),
                base.id()
            );
        }
    }
    // The values T36 wrote in the plan (D24).
    let lead = content.monster("lead_homunculus").expect("shipped");
    assert_eq!((lead.hp(), lead.attack(), lead.defense()), (16, 5, 2));
    let hierophant = content.monster("hierophant").expect("shipped");
    assert_eq!(
        (hierophant.hp(), hierophant.attack(), hierophant.defense()),
        (14, 4, 1)
    );
}
