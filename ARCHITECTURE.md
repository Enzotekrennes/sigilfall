# Architecture

Sigilfall is a Cargo workspace with two crates: the game's rules in a
library, and the terminal application in a binary that depends on it.

| Crate | Kind | Responsibility |
|---|---|---|
| `crates/sigilfall-core` | library | Game logic: world state, turn order, dungeon generation, combat, sigils, items, content loading, saves. |
| `crates/sigilfall` | binary | Terminal application: command line, keymaps, screens, rendering, the files kept between runs. |

## The boundary

- **`sigilfall-core` depends on no terminal crate.** No rendering, terminal
  or input library appears in its dependency tree (`cargo tree -p
  sigilfall-core`): it depends on `rand`, `rand_chacha`, `serde`, `ron`,
  `slotmap` and `thiserror` only. The compiler therefore checks that the
  rules never draw or read a key, and every rule is tested without a
  terminal.
- **The core produces no text.** `Game::submit` returns `Event`s, plain
  data; the binary turns them into English sentences (`messages`).
- Dependencies point from the binary to the core, never the other way.
- The binary decides nothing about the game: it maps keys to `Action`s,
  submits them, and draws the state it reads back.

## Modules

### `sigilfall-core`

| Module | Responsibility |
|---|---|
| `action` | `Action`, what an actor can attempt on its turn (move, wait, cast, pick up, use, drop, descend, explore), and `resolve`, which checks a move against the map. |
| `actor` | `Actor` and `ActorId`: the player and the monsters, their position, speed, energy, hit points, essence and held turns. |
| `ai` | `choose_action`: a monster chases the player it sees by A*, goes to where it last saw it, then waits. |
| `combat` | The player's starting values and the damage formula of a blow. |
| `content` | `Content`: the monster, sigil and item definitions, the spawn tables and the floor kinds, parsed from the RON files embedded in the crate and validated, with errors naming the file, entry, field and line. |
| `content::check` | Field checks shared by every kind of entry. |
| `content::entry_file` | Reads a content file's RON text into raw entries, keeping their line numbers. |
| `content::error` | `ContentError`, every way a content file can be wrong. |
| `content::monster`, `content::monster_file` | Monster definitions, their rules, and the shape of a monster entry. |
| `content::sigil`, `content::sigil_file` | Sigil definitions (pattern, effect, power, targeting, depth), their rules, and the shape of a sigil entry. |
| `content::item`, `content::item_file` | Item definitions, their rules, and the shape of an item entry. |
| `content::spawn`, `content::spawn_file` | Spawn tables: how often each monster and item is drawn, floor by floor. |
| `content::floor`, `content::floor_file` | `FloorKind` (rooms or caves) and the floor rules: the weight of each kind, floor by floor, with the checks a rule must pass. |
| `event` | `Event`: what happened during a turn (moved, attacked, died, sigil cast, glyph learned, picked up, descended, seal broken…). |
| `explore` | `explore_step`: the step of automatic exploration towards the nearest unseen cell next to known ground, by a Dijkstra map planned on what the player remembers. |
| `fov` | Symmetric shadowcasting in integer fractions, within an elliptical `SightRange`: 16 cells along a row, 8 along a column (`SIGHT_RANGE`). |
| `game` | `Game`, the game in progress, and its single entry point `Game::submit`; `Status`, playing, lost or won. |
| `game::world` | What the current floor holds: map, actors, items, engraved glyphs, the player's inventory. |
| `game::cast` | What `Action::Cast` does: cost, essence, target, effect, backlash. |
| `game::inventory` | What picking up, using and dropping an item do. |
| `game::descent` | What `Action::Descend` does: the next floor of the seed, with the player's belongings; on the seal of the bottom floor, the victory that ends the run. |
| `game::explore` | What `Action::Explore` does: a move chosen by `explore`, never a blow, refused when nothing is left to explore. |
| `game::saved` | Turns a game into the records of a save and back, checking every value read. |
| `generation` | A floor built from the seed and the depth: its kind, layout, start, stairs (the seal on the bottom floor, `is_bottom_floor`), monsters, glyphs and items. |
| `generation::bsp` | Rooms and corridors laid out by binary space partitioning. |
| `generation::caves` | One natural cave grown by a cellular automaton, cut down to its largest connected area, and grown again (8 times at most) while that area is under 900 cells. |
| `geometry` | `Position` and the eight `Direction`s. |
| `item` | `Item` and `ItemId`: things on the floor or in the player's pack, and the pack's capacity. |
| `map` | `Map` and `Tile` (wall, floor, stairs down, seal): the grid of a floor, the cells in view and those remembered. |
| `path` | Reachability and distances on a map. |
| `path::flood` | Flood fill: the cells reachable from a cell. |
| `path::dijkstra` | Dijkstra maps: the step distance from every cell to a set of goals. |
| `path::astar` | A*: a shortest walk between two cells. |
| `rng` | `Seed`, and the random streams derived from (seed, depth, purpose); `DailyDate` and `Seed::daily`, the seed of the daily dungeon of a UTC date. |
| `save` | A game written as versioned RON text, and read back. |
| `save::record` | The records a save is made of. |
| `save::slots` | Slot maps as a save records them: the keys in use and the order in which freed slots are reused. |
| `save::error` | `SaveError`, every way writing or reading a save can fail. |
| `schedule` | The energy-based turn order and the cost of each action. |
| `sigil` | Sigils: traces on the 3 × 3 grid, their recognition, the grimoire and the rules of casting. |
| `sigil::trace` | `Node` and `Trace`: the grid and the rules a trace follows. |
| `sigil::recognition` | `recognize`: a finished trace matched against the known sigils by edit distance. |
| `sigil::grimoire` | `Grimoire`: the sigils the player has learnt. |
| `sigil::casting` | Essence, range, cast quality and backlash. |
| `stats` | `Stats`: what the player did during the run, for the death summary. |

### `sigilfall`

| Module | Responsibility |
|---|---|
| `main` | Reads the command line, prepares and restores the terminal (also on a panic), runs the application and saves the game on leaving. |
| `cli` | `Cli`: the `--seed`, `--daily` and `--ascii` options, parsed by clap. |
| `app` | `App`: the screen state machine (title, seed entry, confirmation, options, game, sigil overlay, inventory, end screen), the handling of each key, and the event loop. |
| `app::exploring` | The walk of automatic exploration (`x`): when it stops, its step limit and the pause between steps. |
| `input` | The keymaps: one function per kind of screen, from a key press to a command. Only presses count, not releases. |
| `clock` | Where the application reads the wall-clock time, for the end screen's short grace period only; tests replace it with a manual clock. Game time never depends on it. |
| `casting` | The sigil overlay's own state: the trace being drawn, the direction prompt, the verdict of a cast. |
| `messages` | Every English sentence and label the player reads, including the description of each `Event`. |
| `message_log` | The message log: lines with a tone, oldest first, with a bounded history. |
| `persist` | The data folder (`SIGILFALL_DATA_DIR` or the system's), `save.ron` and `options.ron`, written atomically. |
| `theme` | The colour tokens of `docs/design.md`, the four colour modes and the light theme, colour detection, and the player's options. |
| `theme::glyphs` | Every character the screens draw with, in Unicode and in ASCII. |
| `ui` | Drawing with ratatui from read-only state: picks the screen for the terminal's size (a resize message under 80 × 24). |
| `ui::camera` | Which part of the 80 × 40 floor the map area shows, centred on the player. |
| `ui::game` | The game screen: status line, map, side panel, log, key help. |
| `ui::hud` | The side panel: health, essence, sigils known, what is in view. |
| `ui::log` | The message log under the map. |
| `ui::sigil` | The sigil overlay: the grid with the trace stroke by stroke, the time and essence it costs, the direction prompt and the verdict. |
| `ui::grimoire` | The grimoire beside the grid. |
| `ui::inventory` | The inventory over the map. |
| `ui::death` | The death summary: cause, depth, kills, sigils cast, the seed to replay. |
| `ui::title` | The title screen, its menu, the seed entry and the confirmation before a saved game is abandoned. |
| `ui::options` | The options screen. |

## Flows

### A key press becomes a turn

1. `App::run` draws the screen, then waits for a key (crossterm).
2. `input` turns the key into a command for the active screen; releases,
   repeats and unbound keys do nothing.
3. On the game screen, a command that acts becomes an `Action` and goes to
   `Game::submit`. A sigil is drawn in the overlay first and submitted
   whole, as one `Action::Cast`, once validated.
4. `Game::submit` applies the player's action. An action the rules refuse
   (a wall, no stairs here, not enough essence) costs nothing. Otherwise
   its cost is taken from the player's energy and the scheduler runs: on
   every tick each actor gains its speed in energy, and an actor with 100
   or more acts, ties going to the oldest actor. Monsters choose with `ai`
   until it is the player's turn again; the field of view is recomputed.
5. Everything that happened comes back as `Vec<Event>`, which also feeds
   `Stats`.
6. The application turns the events into sentences (`messages`), appends
   them to the `message_log`, moves to the end screen if the player died,
   and the next frame draws the new state.

### Generation from a seed

A run has one `Seed` (a `u64`). `Seed::stream(depth, purpose)` derives an
independent ChaCha8 stream for each floor and each purpose. `FloorKind`
draws the kind of the floor first, rooms or caves, weighted by the rules of
`floors.ron` for that depth; it has a stream of its own so that choosing a
kind never shifts the draws of a layout, and a floor of rooms stays the one
it was before caves existed. `Generation` then builds the floor: the layout
(rooms and corridors by BSP, or one cave grown by a cellular automaton),
the start (a random room, or one end of the cave), stairs on the cell
farthest from the start by a Dijkstra map (on the bottom floor, 10, the
seal takes their cell and there are no stairs), monsters out of sight of the
start, two glyphs and items, placed by the same steps for both kinds.
`Gameplay` draws the damage rolls and backlashes on that floor. A floor is
a pure function of the seed, its depth and the content, whatever was played
above.
The daily dungeon is a run of `Seed::daily(date)`: the UTC date, packed
as year, month and day, mixed with a fixed tag through SplitMix64, so two
dates never share a seed. The binary reads the date from its clock; the
core never does.

### Content

`monsters.ron`, `sigils.ron`, `items.ron`, `spawns.ron` and `floors.ron`
live in `crates/sigilfall-core/content` and are embedded in the binary with
`include_str!`. `Content::load` parses them at start-up and validates every
entry (bounds, cross references, sigil patterns at least 3 edits apart, a
sigil to find on the first floor, a floor kind for every floor…); a test
checks that the shipped files are valid, so an installed binary never meets
an invalid one.

### Saves

Quitting a game (`q` or Ctrl+C) writes it: `save::to_string` turns the game
into RON records headed by a format version, `persist` writes them to
`save.ron.tmp` and renames it over `save.ron`. At launch, a readable save
offers "Continue" on the title screen; `save::from_str` rebuilds the game,
including the gameplay stream and the order in which freed slot-map keys
are reused, so the resumed game plays exactly as it would have. An
unreadable save is renamed `save.ron.bad`. The save is deleted the moment
the player dies. A game whose seal was broken is over too (`Status::Won`):
`Game::submit` takes no action any more, as after a death.

## Decisions

The full reasoning is in `docs/03-plan.md`; each line gives the choice, then
the alternative set aside.

| # | Choice | Set aside |
|---|---|---|
| D1 | Two crates, core library and terminal binary, so the compiler enforces the boundary. | One crate with modules, where rules could import ratatui by mistake. |
| D2 | Plain structures: a `World` with the map and `SlotMap`s of actors and items, with generational keys and a deterministic order. | An ECS (bevy_ecs, hecs): indirection for fewer than ten kinds of entities, and a query order that threatens seed reproducibility. |
| D3 | Energy scheduler from the start: speed per tick, act at 100, a cost per action, ties by creation order. | Player-then-monsters alternation, or a timestamp priority queue. |
| D4 | `Game::submit(Action) -> Vec<Event>`: the core returns data, the binary writes the text. | Game logic that writes messages itself. |
| D5 | A `u64` seed and one random stream per (depth, purpose). | A single shared generator, where a floor would depend on what was played before. |
| D6 | BSP, flood fill, Dijkstra maps, A* and symmetric shadowcasting written in the project, in integer arithmetic. | The `pathfinding` or `bracket-pathfinding` crates. |
| D7 | Sigils traced on a 3 × 3 keypad grid, validated by Enter, cancelled by Escape, with a time cost per stroke and patterns learnt from carved glyphs. | Spells chosen from a menu, or single-use scrolls. |
| D8 | Content embedded in the binary and validated at start-up. | A data folder read at run time, which `cargo install` would not copy. |
| D9 | `unsafe` forbidden, documentation required on every item, clippy and rustdoc warnings as errors. | The default lints. |
| D10 | Game text in English, centralised in `messages`. | French, or two languages from the start. |
| D11 | An 80 × 40 floor, larger than the screen, seen through a camera. | A floor the size of the screen, with about 18 rows of map at 80 × 24. |
| D12 | `▒` walls, verdigris magic with a gold player, the grimoire visible while tracing. | `█` or `═ ║` walls, amethyst magic, tracing from memory. |
| D13 | GitHub repository created at the end of the build, private then public. | A private repository from the first week. |
| D14 | Camera centred on the player and clamped to the edges; the floor centred along an axis where the screen is larger. | A floor anchored at the top left. |
| D15 | Sight reaching an ellipse 16 cells across and 8 up and down (a terminal cell is about twice as tall as wide, so it looks round), light passing a diagonal gap like a step does, walls lit when part of their face is. | Diagonal gaps opaque to sight; a disc of radius 8; a 12 × 8 ellipse. |
| D16 | Six monsters out of sight of the start, with random starting energy; monsters see through the player's field of view. | Zero starting energy, one field of view per monster. |
| D17 | Bump attacks; damage = max(1, attack − defense + roll in −1..+1); no miss. | A separate attack action, a to-hit roll, player stats in data. |
| D18 | Recognition by Levenshtein distance on nodes; patterns at least 3 edits apart, so a near miss matches one sigil only. | A 2-edit gap with "first in the file wins". |
| D19 | One `Action::Cast` per trace: 50 energy plus 25 per node, 2 essence per cast (6 at most, 1 back every 12 ticks), glyphs read by stepping on them. | Node-by-node tracing in the core, essence cost per sigil, a dedicated "read" action. |
| D20 | `Action::Descend` on the stairs only; a gameplay stream per floor; weighted spawn tables in `spawns.ron`; more monsters deeper (6 + depth − 1, at most 12). | Spawn weights inside each definition, difficulty by stats only. |
| D21 | Readable RON saves with a format version (v4 since T33: the seal tile and the won status; v3 added the date of a daily run), state only (definitions by id), the exact gameplay stream and slot-map reuse order. | `slotmap`'s own serde form, or a binary format. |
| D22 | Floor kinds in data (`floors.ron`, weights by band of floors), drawn on a stream of their own (`FloorKind`); caves grown by a cellular automaton (45 % rock, 5 passes, rule 4-5), the largest area kept, regrown below 900 cells; floors 1-2 always rooms, 10 and deeper always caves. | Floor weights added to `spawns.ron`, and the kind drawn on the generation stream, which would have changed every existing floor and the demo. |
| D23 | The bottom floor (10) has a seal instead of stairs, on the cell farthest from the start; `Action::Descend` on it breaks it and wins the run (`Event::SealBroken`, `Status::Won`), refused elsewhere at no cost like a descent off the stairs. | A descent without end, scored by depth. |
