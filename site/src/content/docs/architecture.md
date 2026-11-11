---
title: Architecture
description: The two crates of Sigilfall, how a key press becomes a turn, how a floor is generated from a seed, and the key design decisions.
---

This page summarises the architecture. [ARCHITECTURE.md](https://github.com/Enzotekrennes/sigilfall/blob/main/ARCHITECTURE.md)
on GitHub gives every module, the flows and all the design decisions (D1 to
D28) with the alternatives set aside.

## Two crates

Sigilfall is a Cargo workspace with two crates: the game's rules in a
library, and the terminal application in a binary that depends on it.

| Crate | Kind | Responsibility |
|---|---|---|
| `crates/sigilfall-core` | library | Game logic: world state, turn order, dungeon generation, combat, sigils, items, content loading, saves. |
| `crates/sigilfall` | binary | Terminal application: command line, keymaps, screens, rendering, the files kept between runs. |

- **`sigilfall-core` depends on no terminal crate.** It depends on `rand`,
  `rand_chacha`, `serde`, `ron`, `slotmap` and `thiserror` only, so the
  compiler checks that the rules never draw or read a key, and every rule is
  tested without a terminal.
- **The core produces no text.** `Game::submit` returns `Event`s, plain data;
  the binary turns them into English or French sentences. The only words the
  core holds are the names of the content, in both languages.
- **The binary decides nothing about the game.** It maps keys to `Action`s,
  submits them, and draws the state it reads back.

The [sigilfall-core reference](/sigilfall/reference/core/) describes each
public module of the library.

## A key press becomes a turn

1. The application draws the screen, then waits for a key.
2. The keymap of the keyboard layout in use turns the key into a command for
   the active screen; releases, repeats and unbound keys do nothing.
3. On the game screen, a command that acts becomes an `Action` and goes to
   `Game::submit`. A sigil is drawn in the overlay first and submitted whole,
   as one `Action::Cast`.
4. `Game::submit` applies the player's action. An action the rules refuse (a
   wall, no stairs here, not enough essence) costs nothing. Otherwise its
   cost is taken from the player's energy and the scheduler runs: on every
   tick each actor gains its speed in energy, and an actor with 100 or more
   acts, ties going to the oldest actor.
5. Everything that happened comes back as a list of `Event`s, which also
   feeds the run's statistics.
6. The application turns the events into sentences, appends them to the
   message log, moves to the end screen if the game is over, and the next
   frame draws the new state.

## A floor from a seed

A run has one 64-bit seed. Each floor and each purpose (the kind of floor,
its layout, the fights) draws from its own ChaCha8 stream derived from
(seed, depth, purpose). A floor is therefore a pure function of the seed,
its depth and the content, whatever was played above, and the same seed
gives the same dungeon on every system.

- The kind of floor is drawn first, rooms or caves, weighted by
  `floors.ron`: floors 1 and 2 are always rooms, floor 10 is always a cave.
- Rooms and corridors are laid out by binary space partitioning; caves are
  grown by a cellular automaton and cut down to their largest connected area.
- The stairs go on the cell farthest from the start; on floor 10, the seal
  takes their place.
- Monsters are placed out of sight of the start, then glyphs, then items.

The daily dungeon is the run of a seed computed from the UTC date, with no
server involved.

## Content

The monsters, sigils, items, spawn tables and floor kinds live in RON files
in `crates/sigilfall-core/content`. They are embedded in the binary and
validated at start-up, with errors naming the file, entry, field and line. A
test checks that the shipped files are valid.

## Saves

A save is readable RON text with a format version. It keeps the random
streams and the order in which freed entity keys are reused, so a resumed run
plays out exactly as it would have without quitting. Every value read back is
checked before it is trusted; a faulty save is refused with a reason instead
of failing later in play.

## Key decisions

| # | Choice | Set aside |
|---|---|---|
| D1 | Two crates, core library and terminal binary, so the compiler enforces the boundary. | One crate with modules. |
| D2 | Plain structures: a `World` with the map and slot maps of actors and items, with generational keys and a deterministic order. | An ECS (bevy_ecs, hecs). |
| D3 | An energy scheduler: speed per tick, act at 100, a cost per action. | Player-then-monsters alternation. |
| D4 | `Game::submit(Action) -> Vec<Event>`: the core returns data, the binary writes the text. | Game logic that writes messages itself. |
| D5 | A `u64` seed and one random stream per (depth, purpose). | A single shared generator. |
| D6 | BSP, flood fill, Dijkstra maps, A* and symmetric shadowcasting written in the project, in integer arithmetic. | Pathfinding crates. |
| D7 | Sigils traced on a 3 × 3 keypad grid, with a time cost per stroke and patterns learnt from carved glyphs. | Spells from a menu, or single-use scrolls. |
| D8 | Content embedded in the binary and validated at start-up. | A data folder read at run time. |
| D9 | `unsafe` forbidden, documentation required on every item, clippy and rustdoc warnings as errors. | The default lints. |
| D18 | Trace recognition by edit distance; patterns at least 3 edits apart, so a near miss matches one sigil only. | A 2-edit gap. |
| D21 | Readable, versioned RON saves that keep the exact random stream and key reuse order. | A binary format. |
| D27 | Every text in English and in French, in a catalogue the compiler checks for completeness. | A translation framework and files read at run time. |
| D28 | QWERTY and AZERTY keymaps, one table per screen and layout, guessed from the locale until chosen. | Reading the layout from the system, which needs `unsafe` or is unreliable in a terminal. |

## Visual identity

The look of the game lives in the `theme` module of the binary: no screen
writes a colour or a glyph of its own.

- **Mood: an alchemist's notebook read by candlelight.** Ink background,
  bone-coloured text, gold for the player and what matters, verdigris for
  everything about sigils, vermilion for danger.
- **Everything reads without colour.** Each piece of information also has a
  glyph, a word or a prefix.
- **Four rendering modes**: 24-bit dark, 24-bit light ("parchment"), the 16
  ANSI colours, and monochrome.
- **Contrast**: every token that carries meaning reaches a WCAG 2.x ratio of
  4.5 on the surfaces it is drawn on, and a test recomputes every ratio.
