# Sigilfall

**English** · [Français](README.fr.md)

[![CI](https://github.com/Enzotekrennes/sigilfall/actions/workflows/ci.yml/badge.svg)](https://github.com/Enzotekrennes/sigilfall/actions/workflows/ci.yml)

A terminal roguelike where spells are cast by tracing sigils on a 3 × 3 grid
with the number keys, on QWERTY and AZERTY keyboards alike: the pattern
decides the spell, new patterns are learnt from glyphs carved in the dungeon
floor, and a sloppy trace fizzles or backfires.

<!-- assets/demo.gif is recorded from demo.tape by the `demo` job of the CI
workflow (run it by hand or push a version tag), then committed by hand. -->
![A run of seed 9940: a glyph read, Ember traced and cast, a fight, the stairs down](assets/demo.gif)

Every stroke of a trace costs time, and the monsters act while you draw:
a long sigil is a strong spell, but a slow one. The first floors are rooms
joined by corridors; deeper down, natural caves take their place more and
more often, and the tenth floor is always one. There are no stairs on the
tenth floor but a seal, `◎` on the map (`0` in ASCII): break it to win the
run. Each run is generated from a seed you can share, so a friend can play
the same dungeon, and every day brings a daily dungeon, the same for every
player of the same version of the game on that UTC date. The whole game is
written in English and in French: it speaks the language of your system
until you choose one in the options.

## Install

Sigilfall runs in a terminal of at least 80 × 24 cells, on Windows, Linux
and macOS. Windows Terminal, or any terminal with a font that has box
drawing characters, shows it best; `--ascii` covers the others.

**Prebuilt binaries.** Each [release](https://github.com/Enzotekrennes/sigilfall/releases)
has archives for Windows (x86_64), Linux (x86_64, glibc) and macOS (Intel and
Apple Silicon), and two installers that download the right one:

```sh
# Linux and macOS
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/Enzotekrennes/sigilfall/releases/latest/download/sigilfall-installer.sh | sh
```

```powershell
# Windows (PowerShell)
powershell -ExecutionPolicy Bypass -c "irm https://github.com/Enzotekrennes/sigilfall/releases/latest/download/sigilfall-installer.ps1 | iex"
```

**From source**, with Rust 1.88 or later:

```sh
cargo install --locked --git https://github.com/Enzotekrennes/sigilfall sigilfall
```

## Play

```sh
sigilfall
```

The title screen offers **Continue** (when a game is saved), **New game**,
**Seed** (type the seed of the new game), **Daily** (the daily dungeon of
today's UTC date, shown beside it), **Options** and **Quit**. A daily run
shows its date on the status line instead of the seed, and keeps it when
saved and resumed, even on a later day.

### Command line

| Option | Effect |
|---|---|
| `--seed <SEED>` | Start a game of this seed at once, without the title screen. The seed is a number such as `42`, or the form the game shows, such as `7F3A-91C2-0B5E-D4A8`. Refused while a game is saved, so that game is never lost: continue it, or abandon it from the title screen, first. |
| `--daily` | Start the daily dungeon at once, without the title screen: the same dungeon for every player of the same version of the game on the same UTC date, a new one each day at midnight UTC. Cannot be combined with `--seed`. Refused while a game is saved, as `--seed` is. |
| `--ascii` | Draw with plain ASCII characters instead of Unicode glyphs, for this session only, whatever the Glyphs option says. |
| `--lang <LANG>` | Play in `en` (English) or `fr` (French) for this session only, whatever the Language option says. |
| `--keyboard <LAYOUT>` | Use the keys of a `qwerty` or an `azerty` keyboard for this session only, whatever the Keyboard option says. |
| `--help`, `--version` | Print the help or the version. |

### Keys

The keys follow the keyboard layout chosen in the options, QWERTY or AZERTY
(see [Options](#options)), so that moving and tracing never need Shift. The
two layouts share the arrows, the keypad (with or without Num Lock), the
digits and the vi keys `h j k l y u b n`; QWERTY adds WASD, and AZERTY adds
ZQSD and the top row typed without Shift, `& é " ' ( - è _ ç à`, which
stands for the digits `1` to `0` wherever a digit counts (with Shift, the
same keys give the digits, which work too).

On the map:

| Action | QWERTY | AZERTY |
|---|---|---|
| Move one cell, diagonals included; moving into a monster attacks it | Arrows, `w a s d`, `1`-`9` around `5` (keypad or top row), `h j k l y u b n` | Arrows, `z q s d`, `1`-`9` around `5` (keypad, or top row with or without Shift: `& é " ' - è _ ç`), `h j k l y u b n` |
| Move diagonally (the keypad without Num Lock) | Home, Page Up, End, Page Down | Home, Page Up, End, Page Down |
| Wait a turn | `.`, `5`, keypad centre | `.`, `5` or `(`, keypad centre |
| Open the sigil grid to trace a sigil | `t` | `t` |
| Pick up the item underfoot | `g`, `,` | `g`, `,` |
| Open the inventory | `i` | `i` |
| Go down the stairs, when standing on them; on the tenth floor, break the seal standing on it | `>`, `E` | `>`, `<`, `E` |
| Explore: walk towards the nearest unexplored place, step by step (see below) | `x` | `x` |
| Save and quit | `q` | `p` |
| Quit from any screen, saving the game in progress | Ctrl+C | Ctrl+C |

On AZERTY, `q` and `d` move, so quitting takes `p`. On both layouts the
letter that goes down the stairs is `E`: `d` moves east, and `e` does
nothing on the map, so Caps Lock never turns a step into a descent. A
movement letter typed in upper case does nothing, and the log asks once
whether Caps Lock is on. Escape
never quits: it closes the grid, the inventory
and the sub-screens of the title, and does nothing on the map. No key does
two things on one screen of one layout; a test goes through every key of
every screen to keep it so.

Exploring with `x` takes one step at a time, each one a normal move, and
stops as soon as a monster comes into view, an item or an unread glyph
appears, you are hurt, nothing is left to explore, any key is pressed, or
after 200 steps; the log says why. It does not start with a monster in view.
The key help at the bottom of the screen names the letters of your layout
for moving, the key of the stairs typed without Shift (`>` on QWERTY, `<` on
AZERTY), and says that quitting saves the game. It names `x` when the row
has room for it: from 80 columns in English, from 92 in French. The arrows,
the digits and the vi keys are listed here only.

While tracing a sigil, the grid is numbered like the keypad (7 8 9 on top,
1 2 3 at the bottom). Each node shows its digit, as the grimoire writes the
patterns, and on AZERTY the key that links it under the digit (`è` under 7):

| Keys | Action |
|---|---|
| `1`-`9`, on AZERTY also the top row without Shift (`&` for `1` to `ç` for `9`), or the keypad with or without Num Lock | Link that node to the trace |
| Backspace | Remove the last node |
| Enter | Cast the trace |
| Escape | Close the grid; no time passes |

A sigil aimed in a direction then asks for one: a movement key aims, Escape
goes back to the trace. After an unstable or failed cast, any key closes the
verdict.

In the inventory: Up and Down (or `k` and `j`, `w` and `s` on QWERTY, `z`
and `s` on AZERTY), Home and End, or the digits `1`-`9` and `0` (on AZERTY
also `&` to `à` without Shift) choose an item; Enter or `u` uses it, `d`
drops it, Escape closes the inventory.

On the title screen and the confirmation before abandoning a saved game: Up
and Down (or `k` and `j`, `w` and `s` on QWERTY, `z` and `s` on AZERTY),
Home and End, Enter; Escape goes back from the confirmation, `q` quits from
the title (`p` on AZERTY). While typing a seed: Backspace erases, Enter
starts, Escape goes back. On the options screen: Up and Down (or `k` and
`j`, `w` and `s`, `z` and `s`) choose, Left and Right (or `h` and `l`, `a`
and `d` on QWERTY, `q` and `d` on AZERTY), Enter or Space change the value,
Escape goes back. After death, the summary shows the seed
of the run and a line to copy and share, such as
`Sigilfall daily 2026-11-08 - depth 4, turn 812, 23 slain, 9 sigils` (or
`Sigilfall seed 7F3A-91C2-0B5E-D4A8 - …` for a run of a seed). Breaking the
seal opens the victory screen, with the same summary and a line that says
`victory` where the depth would be, such as
`Sigilfall daily 2026-11-08 - victory, turn 4120, 96 slain, 31 sigils`.
On either screen Enter goes back to the title, `q` quits (`p` on AZERTY).
For one second after the end of the run only Ctrl+C works, so a key already
on its way does not close the summary unseen.

### How sigils work

Every run starts with Ember (`7 5 3`), a spell that burns the nearest foe;
the first line of the log says so, with the key that opens the grid.
Glyphs carved in the floor (`§`) teach the other sigils when you step on
them, and the grimoire beside the grid lists the patterns you know. A trace
that matches a known pattern exactly casts it; one node off, the spell goes
off unstable and a backlash follows; anything else fizzles, and a pattern
that is nowhere near one you know is pointed out as missing from your
grimoire. Every cast spends
2 essence, which comes back slowly. A cast takes half a turn plus a quarter
of a turn for each node linked, and the monsters keep moving meanwhile.

### Options

**Glyphs** (Unicode or ASCII), **Colours** (Auto, 16 colours or Monochrome),
**Theme** (dark, or light for 24-bit colour terminals), **Language**
(English or Français) and **Keyboard** (QWERTY or AZERTY) apply at once and
are kept for the next sessions. Auto follows the terminal and `NO_COLOR`; 16
colours and Monochrome apply whatever they say. Until you choose a language,
the game speaks French on a system set to French and English otherwise.
Until you choose a keyboard, it takes AZERTY on a system set to the French
of France or Belgium, where AZERTY keyboards are the usual ones, and QWERTY
otherwise: the layout itself cannot be read from every system and terminal
in a safe and reliable way, so the game guesses from the language and region
of the system, and the option sets it right. The line to share on the end
screens stays in English, so two players' lines compare whatever their
language; `--help` is in English, as for most command-line tools.

## Files

The game keeps two files in the data folder of your system:

| System | Folder |
|---|---|
| Windows | `%APPDATA%\Sigilfall\data` |
| Linux | `$XDG_DATA_HOME/sigilfall`, by default `~/.local/share/sigilfall` |
| macOS | `~/Library/Application Support/Sigilfall` |

- `save.ron` holds the game in progress. It is written when you quit and
  deleted when you die (death is final) or break the seal. A save that
  cannot be read back (damaged, or written by another version) is renamed
  `save.ron.bad`, the title screen says why, and a new game starts.
- `options.ron` holds the options. If it cannot be read, the default
  options are used and the file is replaced at the next change.

Both are readable RON text.

### Environment variables

| Variable | Effect |
|---|---|
| `SIGILFALL_DATA_DIR` | Use this folder instead of the system's data folder, for the save and the options. Created if missing; ignored when empty. |
| `NO_COLOR` | When set to a non-empty value and **Colours** is Auto, draw without colour ([no-color.org](https://no-color.org)). Choosing 16 colours in the options wins over it. |
| `COLORTERM` | `truecolor` or `24bit` turns on 24-bit colour; otherwise the 16 ANSI colours are used, except in Windows Terminal (detected by `WT_SESSION`), which gets 24-bit colour. |

## Under the hood

- **Energy-based turns.** Every actor gains its speed in energy on each
  tick and acts once it holds 100; each action has its own cost. A monster
  twice as fast as you acts twice per step you take, and each node of a
  sigil costs time, so a long trace leaves the monsters room to act.
- **One random stream per purpose.** A run is a 64-bit seed; each floor
  draws its kind, layout and fights from separate ChaCha8 streams derived
  from (seed, depth, purpose). A floor depends on nothing played above it,
  and the same seed gives the same dungeon on every system.
- **Exact field of view.** Sight is symmetric shadowcasting with every
  slope kept as a fraction of two integers: no rounding, so if you see a
  monster, it sees you.
- **Rules apart from the screen.** The game logic is a library with no
  terminal dependency, which the compiler enforces; it returns events, and
  the terminal application turns them into sentences and draws.
- **Tested beyond examples.** Property tests generate thousands of seeds to
  check that every floor is connected and that sight is symmetric; rules
  were broken on purpose to check that the tests catch it; two bots, a
  reckless one and a careful one, play 80 runs each to hold the difficulty
  within its target.
- **Saves that replay exactly.** A save keeps the random streams and the
  order in which freed entity keys are reused, so a resumed run plays out
  exactly as it would have without quitting, and every value read back is
  checked before it is trusted.

[ARCHITECTURE.md](ARCHITECTURE.md) gives the modules, the flows and the
design decisions in detail.

## Build and test

```sh
cargo build --release --locked          # target/release/sigilfall
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy --workspace --locked -- -D warnings   # the shipped code alone
cargo fmt --all --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
```

The toolchain is pinned to stable by `rust-toolchain.toml`; the minimum
supported version is Rust 1.88. CI runs these checks on Linux, Windows and
macOS, and checks the build on Rust 1.88.

The demo GIF is recorded from [`demo.tape`](demo.tape) with
[VHS](https://github.com/charmbracelet/vhs) by the `demo` job of the CI
workflow, run by hand or on a version tag; the test
`app::tests::demo_tape` replays its keys to check that the recorded run
still reads a glyph, casts, fights and goes down the stairs.

Releases are built by [`dist`](https://github.com/axodotdev/cargo-dist) from
`dist-workspace.toml`: pushing a tag such as `v0.1.0` builds the binaries and
installers and publishes them as a GitHub release.

## Documentation

- [Documentation site](https://enzotekrennes.github.io/sigilfall/), in
  English and French: installing, playing, keyboard layouts, options, and a
  reference of the `sigilfall-core` modules with the full API docs.
- [ARCHITECTURE.md](ARCHITECTURE.md): the two crates, their modules, how a
  key press becomes a turn, the visual identity, and the design decisions
  with the alternatives set aside.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
