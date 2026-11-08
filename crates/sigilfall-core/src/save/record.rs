//! The records a save is made of, as written in RON.
//!
//! Exposes to the crate [`GameRecord`], the whole save, and the records of
//! its parts: [`MapRecord`], [`ActorRecord`] and [`ActorKindRecord`],
//! [`ItemRecord`] and [`EngravingRecord`]. A record holds plain values only:
//! definitions are named by identifier and actors and items by their saved
//! keys (`SlotKey`), so a record can be read without the content. Turning a
//! game into records and back, with every check, is done by the `game`
//! module (its `saved` submodule); the map, being self-contained, is turned
//! back here.
//!
//! Every record refuses unknown fields, so a misspelt or foreign field is
//! reported instead of ignored.

use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

use super::SaveError;
use super::slots::{SlotKey, SlotsRecord};
use crate::game::Status;
use crate::geometry::Position;
use crate::map::{Map, Tile};
use crate::stats::Stats;

/// Longest side of a saved map, in cells.
///
/// Far above the 80 × 40 generated floors, it keeps a faulty save from
/// asking for a huge grid, and keeps coordinates well within `i32`.
const MAX_MAP_SIDE: usize = 1024;

/// Character of a wall in [`MapRecord::tiles`].
const WALL: char = '#';
/// Character of a floor cell in [`MapRecord::tiles`].
const FLOOR: char = '.';
/// Character of the stairs down in [`MapRecord::tiles`].
const STAIRS_DOWN: char = '>';

/// Character of a cell never seen in [`MapRecord::sight`].
const UNSEEN: char = ' ';
/// Character of a cell remembered but out of sight in [`MapRecord::sight`].
const REMEMBERED: char = 'm';
/// Character of a cell visible right now in [`MapRecord::sight`]; a visible
/// cell is always remembered too.
const VISIBLE: char = 'v';

/// A whole save: the state of a game, in the order it is written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GameRecord {
    /// Version of the format, `save::FORMAT_VERSION` when written.
    pub(crate) format_version: u32,
    /// Value of the run's seed (`Seed::value`).
    pub(crate) seed: u64,
    /// Number of the current floor.
    pub(crate) depth: u32,
    /// Whether the floors below are generated with the content (a game
    /// started with `Game::new`) or bare (`Game::on_floor`,
    /// `Game::in_fixed_room`).
    pub(crate) floors_from_content: bool,
    /// Whether the player is still alive, and if not who killed it.
    pub(crate) status: Status,
    /// Ticks elapsed since the game started.
    pub(crate) ticks: u64,
    /// Tick from which the player's essence recovery is counted.
    pub(crate) essence_since: u64,
    /// Unstable casts the player will still cast pure.
    pub(crate) steady_casts: u32,
    /// The gameplay random stream of the current floor, at the exact point
    /// it has reached: key, stream number and position in 32-bit words.
    pub(crate) gameplay_rng: ChaCha8Rng,
    /// Identifiers of the sigils the player has learnt.
    pub(crate) grimoire: Vec<String>,
    /// The current floor and what the player sees and remembers of it.
    pub(crate) map: MapRecord,
    /// Key of the player among the actors.
    pub(crate) player: SlotKey,
    /// Keys of the scheduled actors, first to act on a tie first.
    pub(crate) turn_order: Vec<SlotKey>,
    /// Every actor, the player included, with the layout of their keys.
    pub(crate) actors: SlotsRecord<ActorRecord>,
    /// Every item, on the floor or carried, with the layout of their keys.
    pub(crate) items: SlotsRecord<ItemRecord>,
    /// Keys of the carried items, in the order they were picked up.
    pub(crate) inventory: Vec<SlotKey>,
    /// Glyphs engraved on the current floor.
    pub(crate) engravings: Vec<EngravingRecord>,
    /// What the player did so far, its turns included; written last, after
    /// the state the game plays from.
    pub(crate) stats: Stats,
}

/// A floor as a save writes it: one string per row, top row first, for the
/// tiles and for the player's sight.
///
/// Tiles are `#` (wall), `.` (floor) and `>` (stairs down). Sight is ` `
/// (never seen), `m` (remembered, out of sight) and `v` (visible now). Both
/// grids have the same size, at least one cell and at most
/// [`MAX_MAP_SIDE`] on each side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MapRecord {
    /// Rows of tiles.
    pub(crate) tiles: Vec<String>,
    /// Rows of the player's sight.
    pub(crate) sight: Vec<String>,
}

impl MapRecord {
    /// Writes `map` as rows of characters.
    pub(crate) fn of(map: &Map) -> Self {
        let rows = |cell: &dyn Fn(Position) -> char| -> Vec<String> {
            (0..map.height())
                .map(|y| {
                    (0..map.width())
                        .map(|x| cell(grid_position(x, y)))
                        .collect()
                })
                .collect()
        };
        let tiles = rows(&|position| match map.tile(position) {
            Some(Tile::Floor) => FLOOR,
            Some(Tile::StairsDown) => STAIRS_DOWN,
            Some(Tile::Wall) | None => WALL,
        });
        let sight = rows(&|position| {
            if map.is_visible(position) {
                VISIBLE
            } else if map.is_remembered(position) {
                REMEMBERED
            } else {
                UNSEEN
            }
        });
        Self { tiles, sight }
    }

    /// Reads the rows back into a map.
    ///
    /// # Errors
    ///
    /// Returns [`SaveError::Inconsistent`] if the grid is empty or larger
    /// than [`MAX_MAP_SIDE`] on a side, if its rows differ in length, if the
    /// two grids differ in size, or if a row holds a character other than
    /// those of its grid, naming the row.
    pub(crate) fn to_map(&self) -> Result<Map, SaveError> {
        let height = self.tiles.len();
        let width = self.tiles.first().map_or(0, |row| row.chars().count());
        if !(1..=MAX_MAP_SIDE).contains(&width) || !(1..=MAX_MAP_SIDE).contains(&height) {
            return Err(SaveError::Inconsistent(format!(
                "the map is {width}x{height} cells, each side must be from 1 to {MAX_MAP_SIDE}"
            )));
        }
        if self.sight.len() != height {
            return Err(SaveError::Inconsistent(format!(
                "the map has {height} rows of tiles but {} rows of sight",
                self.sight.len()
            )));
        }

        let mut map = Map::new(width, height, Tile::Wall);
        let mut remembered = Vec::new();
        let mut visible = Vec::new();
        for (y, (tiles, sight)) in self.tiles.iter().zip(&self.sight).enumerate() {
            for grid in [("tiles", tiles), ("sight", sight)] {
                if grid.1.chars().count() != width {
                    return Err(SaveError::Inconsistent(format!(
                        "row {y} of the map {} is not {width} cells long",
                        grid.0
                    )));
                }
            }
            for (x, (tile, seen)) in tiles.chars().zip(sight.chars()).enumerate() {
                let position = grid_position(x, y);
                map.set_tile(position, tile_of(tile, y)?);
                match seen {
                    UNSEEN => {}
                    REMEMBERED => remembered.push(position),
                    VISIBLE => {
                        remembered.push(position);
                        visible.push(position);
                    }
                    other => {
                        return Err(SaveError::Inconsistent(format!(
                            "row {y} of the map sight holds `{other}`, expected \
                             `{UNSEEN}`, `{REMEMBERED}` or `{VISIBLE}`"
                        )));
                    }
                }
            }
        }
        // Showing a set of cells also remembers them and hides the others,
        // so remembering everything seen, then showing what is visible,
        // leaves both layers as saved.
        map.set_visible(remembered);
        map.set_visible(visible);
        Ok(map)
    }
}

/// Returns the tile written as `character` on row `y`.
///
/// # Errors
///
/// Returns [`SaveError::Inconsistent`] for a character that is no tile.
fn tile_of(character: char, y: usize) -> Result<Tile, SaveError> {
    match character {
        WALL => Ok(Tile::Wall),
        FLOOR => Ok(Tile::Floor),
        STAIRS_DOWN => Ok(Tile::StairsDown),
        other => Err(SaveError::Inconsistent(format!(
            "row {y} of the map tiles holds `{other}`, expected `{WALL}`, `{FLOOR}` or \
             `{STAIRS_DOWN}`"
        ))),
    }
}

/// Returns the position of column `x`, row `y` of a map at most
/// [`MAX_MAP_SIDE`] cells wide and high, so both fit in `i32`.
fn grid_position(x: usize, y: usize) -> Position {
    let coordinate = |value: usize| i32::try_from(value).unwrap_or(i32::MAX);
    Position::new(coordinate(x), coordinate(y))
}

/// An actor as a save writes it: every value that changes during play, and
/// what it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ActorRecord {
    /// Cell the actor stands on.
    pub(crate) position: Position,
    /// The player, or a monster with its definition and memory.
    pub(crate) kind: ActorKindRecord,
    /// Energy gained per tick.
    pub(crate) speed: i32,
    /// Energy stored towards its next action.
    pub(crate) energy: i32,
    /// Hit points left.
    pub(crate) hp: i32,
    /// Hit points when unhurt.
    pub(crate) max_hp: i32,
    /// Attack value.
    pub(crate) attack: i32,
    /// Defense value.
    pub(crate) defense: i32,
    /// Essence left.
    pub(crate) essence: i32,
    /// Largest reserve of essence.
    pub(crate) max_essence: i32,
    /// Turns the actor still loses to a hold.
    pub(crate) held_turns: u32,
}

/// What an actor is, as a save writes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) enum ActorKindRecord {
    /// The character controlled by the person playing.
    Player,
    /// A creature of the content.
    Monster {
        /// Identifier of its definition (`MonsterDef::id`).
        definition: String,
        /// Cell where it last saw the player, if it remembers one.
        last_seen_player: Option<Position>,
    },
}

/// An item as a save writes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ItemRecord {
    /// Identifier of its definition (`ItemDef::id`).
    pub(crate) definition: String,
    /// Cell it lies on, or `None` while the player carries it.
    pub(crate) position: Option<Position>,
}

/// An engraved glyph as a save writes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EngravingRecord {
    /// Cell it is engraved on.
    pub(crate) position: Position,
    /// Identifier of the sigil it teaches (`SigilDef::id`).
    pub(crate) sigil: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A change that breaks a saved map.
    type Corruption = fn(&mut MapRecord);

    /// A 4 × 3 map with every tile and every state of sight.
    fn sample() -> MapRecord {
        MapRecord {
            tiles: vec!["####".into(), "#.>#".into(), "####".into()],
            sight: vec!["vvm ".into(), "vvm ".into(), "    ".into()],
        }
    }

    #[test]
    fn a_map_reads_back_as_written() {
        let map = sample().to_map().expect("a valid map");

        assert_eq!((map.width(), map.height()), (4, 3));
        assert_eq!(map.tile(Position::new(2, 1)), Some(Tile::StairsDown));
        assert_eq!(map.tile(Position::new(1, 1)), Some(Tile::Floor));
        assert!(map.is_visible(Position::new(1, 1)));
        assert!(map.is_remembered(Position::new(2, 0)));
        assert!(!map.is_visible(Position::new(2, 0)));
        assert!(!map.is_remembered(Position::new(3, 0)));
        assert_eq!(MapRecord::of(&map), sample());
    }

    #[test]
    fn faulty_maps_are_refused_with_the_row_in_cause() {
        let cases: [(Corruption, &str); 6] = [
            (
                |map| map.tiles[1] = "#.x#".into(),
                "row 1 of the map tiles holds `x`",
            ),
            (
                |map| map.sight[2] = "  ? ".into(),
                "row 2 of the map sight holds `?`",
            ),
            (
                |map| map.tiles[2] = "###".into(),
                "row 2 of the map tiles is not 4",
            ),
            (
                |map| {
                    map.sight.pop();
                },
                "3 rows of tiles but 2 rows",
            ),
            (|map| map.tiles.clear(), "the map is 0x0 cells"),
            (
                |map| map.tiles = vec!["#".repeat(MAX_MAP_SIDE + 1); 3],
                "each side must be from 1 to",
            ),
        ];
        for (corrupt, expected) in cases {
            let mut map = sample();
            corrupt(&mut map);

            let error = map.to_map().expect_err(expected);
            assert!(
                matches!(&error, SaveError::Inconsistent(problem) if problem.contains(expected)),
                "expected `{expected}`, got {error}"
            );
        }
    }
}
