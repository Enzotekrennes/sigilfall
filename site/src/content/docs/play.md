---
title: How to play
description: The title screen, the command line, moving and fighting, tracing sigils, going down the stairs, and saving.
---

This page names the keys of a QWERTY keyboard. On AZERTY some of them
change: [Keyboard layouts](/sigilfall/keyboard/) lists both.

## The title screen

```sh
sigilfall
```

The title screen offers **Continue** (when a game is saved), **New game**,
**Seed** (type the seed of the new game), **Daily** (the daily dungeon of
today's UTC date, shown beside it), **Options** and **Quit**.

A daily run shows its date on the status line instead of the seed, and keeps
it when saved and resumed, even on a later day.

## Command line

| Option | Effect |
|---|---|
| `--seed <SEED>` | Start a game of this seed at once, without the title screen. The seed is a number such as `42`, or the form the game shows, such as `7F3A-91C2-0B5E-D4A8`. Refused while a game is saved, so that game is never lost: continue it, or abandon it from the title screen, first. |
| `--daily` | Start the daily dungeon at once, without the title screen: the same dungeon for every player of the same version of the game on the same UTC date, a new one each day at midnight UTC. Cannot be combined with `--seed`. Refused while a game is saved, as `--seed` is. |
| `--ascii` | Draw with plain ASCII characters instead of Unicode glyphs, for this session only, whatever the Glyphs option says. |
| `--lang <LANG>` | Play in `en` (English) or `fr` (French) for this session only. See [Language](/sigilfall/language/). |
| `--keyboard <LAYOUT>` | Use the keys of a `qwerty` or an `azerty` keyboard for this session only. See [Keyboard layouts](/sigilfall/keyboard/). |
| `--help`, `--version` | Print the help or the version. |

For example, to replay a shared dungeon:

```sh
sigilfall --seed 7F3A-91C2-0B5E-D4A8
```

## Moving and fighting

Move one cell at a time, diagonals included, with the arrows, `w a s d`, the
digits `1`-`9` around `5` (keypad or top row) or the vi keys
`h j k l y u b n`. `.` or `5` waits a turn.

Moving into a monster attacks it. A blow always hits, and deals the
attacker's attack minus the defender's defense, plus a roll between −1 and
+1, with at least 1 damage.

Time is counted in energy. Every actor gains its speed in energy on each
tick and acts once it holds 100; each action has its own cost. A monster
twice as fast as you acts twice per step you take.

The side panel shows your health and essence, the sigils you know, and what
is in view, with the current and maximum hit points of each monster.

### Exploring

`x` walks towards the nearest unexplored place, one step at a time, each
step a normal move. It stops as soon as a monster comes into view, an item or
an unread glyph appears, you are hurt, nothing is left to explore, any key
is pressed, or after 200 steps; the log says why. It does not start with a
monster in view.

### Items

`g` or `,` picks up the item underfoot, and `i` opens the inventory, which
holds 10 items. In the inventory, Up and Down (or `k` and `j`, `w` and `s`),
Home and End, or the digits `1`-`9` and `0` choose an item; Enter or `u`
uses it, `d` drops it, Escape closes the inventory.

## Tracing sigils

Every run starts with Ember (`7 5 3`), a spell that burns the nearest foe;
the first line of the log says so, with the key that opens the grid.

Press `t` to open the sigil grid. It is numbered like the keypad:

```text
7 8 9
4 5 6
1 2 3
```

| Keys | Action |
|---|---|
| `1`-`9`, or the keypad with or without Num Lock | Link that node to the trace |
| Backspace | Remove the last node |
| Enter | Cast the trace |
| Escape | Close the grid; no time passes |

A sigil aimed in a direction then asks for one: a movement key aims, Escape
goes back to the trace. After an unstable or failed cast, any key closes the
verdict.

What the trace does:

- A trace that matches a known pattern exactly casts it.
- One node off, the spell goes off **unstable** and a backlash follows.
- Anything else **fizzles**. A pattern that is nowhere near one you know is
  pointed out as missing from your grimoire.

Every cast spends 2 essence, out of 6 at most, and essence comes back
slowly. A cast takes half a turn plus a quarter of a turn for each node
linked, and the monsters keep moving meanwhile.

Glyphs carved in the floor (`§`) teach the other sigils when you step on
them. The grimoire beside the grid lists the patterns you know.

## Going down

Standing on the stairs, press `>` or `E` to go down to the next floor. You
keep your hit points, essence, inventory and grimoire.

The tenth floor has no stairs but a seal, `◎` on the map (`0` in ASCII).
Standing on it, the same keys break it and win the run.

## Saving and quitting

- `q` saves and quits from the map.
- Ctrl+C quits from any screen, saving the game in progress.
- Escape never quits: it closes the grid, the inventory and the sub-screens
  of the title, and does nothing on the map.

The next time you start the game, **Continue** resumes the saved run, which
plays out exactly as it would have without quitting. Death is final: the
save is deleted when you die, and when you break the seal.
[Files and options](/sigilfall/files/) says where the save is kept.

## The end of a run

After death, the summary shows the seed of the run and a line to copy and
share, such as:

```text
Sigilfall daily 2026-11-08 - depth 4, turn 812, 23 slain, 9 sigils
```

or `Sigilfall seed 7F3A-91C2-0B5E-D4A8 - …` for a run of a seed. Breaking
the seal opens the victory screen, with the same summary and a line that
says `victory` where the depth would be:

```text
Sigilfall daily 2026-11-08 - victory, turn 4120, 96 slain, 31 sigils
```

On either screen Enter goes back to the title and `q` quits. For one second
after the end of the run only Ctrl+C works, so a key already on its way does
not close the summary unseen.
